//! The private socketpair channel between the runtime and its internal helper
//! processes (workload gate, background session owner, presenter). One owner
//! for descriptor admission, magic + big-endian length framing, and bounded
//! poll-based waiting. The protocol is private to the exact runtime ABI.

use std::ffi::OsString;
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::net::UnixStream;
use std::process::Command;
use std::time::{Duration, Instant};

/// Exit status of an internal helper whose invocation or channel is invalid.
pub(crate) const FAILURE_EXIT: i32 = 125;
const HEADER: usize = 8;
const POLL_SLICE: Duration = Duration::from_millis(10);

/// One framed protocol: a per-channel magic and a bounded body size.
pub(crate) struct Protocol {
    pub magic: [u8; 4],
    pub max: usize,
}

impl Protocol {
    /// Frame a nonempty body within the protocol bound.
    pub fn encode(&self, body: &[u8]) -> io::Result<Vec<u8>> {
        if body.is_empty() || body.len() > self.max {
            return Err(io::ErrorKind::InvalidData.into());
        }
        let mut frame = Vec::with_capacity(HEADER + body.len());
        frame.extend(self.magic);
        frame.extend((body.len() as u32).to_be_bytes());
        frame.extend(body);
        Ok(frame)
    }

    pub fn write(
        &self,
        channel: &mut UnixStream,
        body: &[u8],
        deadline: Instant,
    ) -> io::Result<()> {
        write_all(channel, &self.encode(body)?, deadline)
    }

    /// Read exactly one frame and nothing after it before the deadline.
    pub fn read(&self, channel: &mut UnixStream, deadline: Instant) -> io::Result<Vec<u8>> {
        let mut reader = FrameReader::new(self);
        loop {
            if let Some(body) = reader.step(channel)? {
                return Ok(body);
            }
            wait(channel, libc::POLLIN, deadline)?;
        }
    }
}

/// Incremental nonblocking frame accumulation for callers that interleave
/// other observations. It never reads past the end of its frame.
pub(crate) struct FrameReader<'a> {
    protocol: &'a Protocol,
    received: Vec<u8>,
}

impl<'a> FrameReader<'a> {
    pub fn new(protocol: &'a Protocol) -> Self {
        Self {
            protocol,
            received: Vec::new(),
        }
    }

    /// `Ok(None)` means incomplete so far; EOF before a complete frame or an
    /// invalid header is an error.
    pub fn step(&mut self, channel: &mut UnixStream) -> io::Result<Option<Vec<u8>>> {
        loop {
            let wanted = match self.expected()? {
                Some(total) if self.received.len() == total => {
                    return Ok(Some(self.received.split_off(HEADER)));
                }
                Some(total) => total - self.received.len(),
                None => HEADER - self.received.len(),
            };
            let start = self.received.len();
            self.received.resize(start + wanted, 0);
            let result = channel.read(&mut self.received[start..]);
            let read = match &result {
                Ok(read) => *read,
                Err(_) => 0,
            };
            self.received.truncate(start + read);
            match result {
                Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(None),
                Err(error) => return Err(error),
            }
        }
    }

    /// The total frame length once the header is complete and valid.
    fn expected(&self) -> io::Result<Option<usize>> {
        if self.received.len() < HEADER {
            return Ok(None);
        }
        if self.received[..4] != self.protocol.magic {
            return Err(io::ErrorKind::InvalidData.into());
        }
        let size = u32::from_be_bytes(self.received[4..HEADER].try_into().expect("four bytes"));
        let size = size as usize;
        if size == 0 || size > self.protocol.max {
            return Err(io::ErrorKind::InvalidData.into());
        }
        Ok(Some(HEADER + size))
    }
}

pub(crate) fn write_all(
    channel: &mut UnixStream,
    mut bytes: &[u8],
    deadline: Instant,
) -> io::Result<()> {
    while !bytes.is_empty() {
        match channel.write(bytes) {
            Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
            Ok(written) => bytes = &bytes[written..],
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                wait(channel, libc::POLLOUT, deadline)?
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

/// Require write-half closure: any further byte is a protocol violation.
pub(crate) fn read_eof(channel: &mut UnixStream, deadline: Instant) -> io::Result<()> {
    let mut byte = [0_u8];
    loop {
        match channel.read(&mut byte) {
            Ok(0) => return Ok(()),
            Ok(_) => return Err(io::ErrorKind::InvalidData.into()),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                wait(channel, libc::POLLIN, deadline)?
            }
            Err(error) => return Err(error),
        }
    }
}

/// Wait at most one short slice for readiness; `TimedOut` once the deadline
/// passed. Callers re-check their own conditions between slices.
pub(crate) fn wait(
    channel: &UnixStream,
    events: libc::c_short,
    deadline: Instant,
) -> io::Result<()> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Err(io::ErrorKind::TimedOut.into());
    }
    let mut descriptor = libc::pollfd {
        fd: channel.as_raw_fd(),
        events,
        revents: 0,
    };
    let timeout = remaining.min(POLL_SLICE).as_millis().max(1) as i32;
    if unsafe { libc::poll(&mut descriptor, 1, timeout) } < 0 {
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
    Ok(())
}

/// A close-on-exec, nonblocking AF_UNIX stream socketpair.
pub(crate) fn pair() -> io::Result<(UnixStream, UnixStream)> {
    fn create(kind: libc::c_int) -> io::Result<(UnixStream, UnixStream)> {
        let mut descriptors = [-1; 2];
        if unsafe { libc::socketpair(libc::AF_UNIX, kind, 0, descriptors.as_mut_ptr()) } != 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: socketpair returned two fresh, uniquely owned descriptors.
        Ok(unsafe {
            (
                UnixStream::from_raw_fd(descriptors[0]),
                UnixStream::from_raw_fd(descriptors[1]),
            )
        })
    }
    #[cfg(target_os = "linux")]
    let pair = create(libc::SOCK_STREAM | libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK)?;
    #[cfg(target_os = "macos")]
    let pair = crate::spawn::exclude_spawn(|| {
        let pair = create(libc::SOCK_STREAM)?;
        for channel in [&pair.0, &pair.1] {
            if unsafe { libc::fcntl(channel.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) } != 0 {
                return Err(io::Error::last_os_error());
            }
            channel.set_nonblocking(true)?;
            no_sigpipe(channel)?;
        }
        Ok(pair)
    })?;
    Ok(pair)
}

/// Pass `child` to the spawned helper as its descriptor argument. After fork
/// the parent's end closes and only the child's end survives exec; the
/// parent's descriptor flags never change.
pub(crate) fn inherit(command: &mut Command, child: &UnixStream, parent: &UnixStream) {
    use std::os::unix::process::CommandExt;
    let inherited = child.as_raw_fd();
    let parent = parent.as_raw_fd();
    command.arg(inherited.to_string());
    // SAFETY: only async-signal-safe descriptor syscalls run after fork, and
    // the caller keeps both descriptors open until the spawn returns.
    unsafe {
        command.pre_exec(move || {
            if libc::close(parent) != 0 || libc::fcntl(inherited, libc::F_SETFD, 0) != 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

/// Return None unless `args` invoke `command`. Otherwise admit exactly one
/// inherited descriptor argument: a connected AF_UNIX stream socket, made
/// close-on-exec and nonblocking before this process owns it. Any mismatch
/// yields the helper failure exit and reads no data.
pub(crate) fn admit(args: &[OsString], command: &str) -> Option<Result<UnixStream, i32>> {
    if args.first().is_none_or(|arg| arg != command) {
        return None;
    }
    let [_, descriptor] = args else {
        return Some(Err(FAILURE_EXIT));
    };
    let Some(fd) = descriptor
        .to_str()
        .and_then(|value| value.parse::<i32>().ok())
        .filter(|fd| *fd >= 3)
    else {
        return Some(Err(FAILURE_EXIT));
    };
    // Verify the inherited descriptor before creating an owning Rust socket.
    let mut peer = std::mem::MaybeUninit::<libc::sockaddr_un>::zeroed();
    let mut length = std::mem::size_of::<libc::sockaddr_un>() as libc::socklen_t;
    if unsafe { libc::getpeername(fd, peer.as_mut_ptr().cast(), &mut length) } != 0
        || unsafe { peer.assume_init().sun_family } as i32 != libc::AF_UNIX
    {
        return Some(Err(FAILURE_EXIT));
    }
    let mut kind: libc::c_int = 0;
    let mut kind_length = std::mem::size_of_val(&kind) as libc::socklen_t;
    if unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_TYPE,
            (&raw mut kind).cast(),
            &mut kind_length,
        )
    } != 0
        || kind != libc::SOCK_STREAM
    {
        return Some(Err(FAILURE_EXIT));
    }
    if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
        return Some(Err(FAILURE_EXIT));
    }
    // SAFETY: this internal process takes sole ownership of its inherited socket.
    let channel = unsafe { UnixStream::from_raw_fd(fd) };
    if channel.set_nonblocking(true).is_err() {
        return Some(Err(FAILURE_EXIT));
    }
    #[cfg(target_os = "macos")]
    if no_sigpipe(&channel).is_err() {
        return Some(Err(FAILURE_EXIT));
    }
    Some(Ok(channel))
}

#[cfg(target_os = "macos")]
fn no_sigpipe(channel: &UnixStream) -> io::Result<()> {
    let enabled: libc::c_int = 1;
    if unsafe {
        libc::setsockopt(
            channel.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_NOSIGPIPE,
            (&raw const enabled).cast(),
            std::mem::size_of_val(&enabled) as libc::socklen_t,
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST: Protocol = Protocol {
        magic: *b"NXT1",
        max: 16,
    };

    fn pair() -> (UnixStream, UnixStream) {
        let (left, right) = UnixStream::pair().unwrap();
        right.set_nonblocking(true).unwrap();
        (left, right)
    }

    #[test]
    fn frames_reject_truncation_bad_headers_and_oversize() {
        let deadline = || Instant::now() + Duration::from_secs(1);
        let (mut left, mut right) = pair();
        TEST.write(&mut left, b"{}", deadline()).unwrap();
        assert_eq!(TEST.read(&mut right, deadline()).unwrap(), b"{}");
        assert!(TEST.encode(b"").is_err());
        assert!(TEST.encode(&[0; 17]).is_err());

        // Incremental reads never consume bytes after the frame.
        let mut reader = FrameReader::new(&TEST);
        left.write_all(b"NXT1\0\0\0\x02{").unwrap();
        assert_eq!(reader.step(&mut right).unwrap(), None);
        left.write_all(b"}F").unwrap();
        assert_eq!(reader.step(&mut right).unwrap(), Some(b"{}".to_vec()));
        let mut rest = [0];
        assert_eq!((&right).read(&mut rest).unwrap(), 1);
        assert_eq!(rest, *b"F");

        // Shut down explicitly: a concurrently forked test child may briefly
        // hold another copy of this descriptor.
        let mut reader = FrameReader::new(&TEST);
        left.write_all(b"NXT1\0\0\0\x02{").unwrap();
        assert_eq!(reader.step(&mut right).unwrap(), None);
        left.shutdown(std::net::Shutdown::Write).unwrap();
        assert!(reader.step(&mut right).is_err());
        assert!(read_eof(&mut right, deadline()).is_ok());

        for bad in [
            &b"NXT1\xff\xff\xff\xff"[..],
            b"NXT1\0\0\0\0",
            b"XXXX\0\0\0\x02{}",
        ] {
            let (mut left, mut right) = pair();
            left.write_all(bad).unwrap();
            assert!(FrameReader::new(&TEST).step(&mut right).is_err());
        }

        let (mut left, mut right) = pair();
        left.write_all(b"x").unwrap();
        assert!(read_eof(&mut right, deadline()).is_err());
        assert_eq!(
            TEST.read(&mut right, Instant::now()).unwrap_err().kind(),
            io::ErrorKind::TimedOut
        );
    }

    #[test]
    fn admission_accepts_only_one_inherited_unix_stream_descriptor() {
        let args = |values: &[&str]| values.iter().map(OsString::from).collect::<Vec<_>>();
        assert!(admit(&args(&["run"]), "__helper").is_none());
        for invalid in [
            args(&["__helper"]),
            args(&["__helper", "2"]),
            args(&["__helper", "x"]),
            args(&["__helper", "999999"]),
            args(&["__helper", "3", "extra"]),
        ] {
            assert_eq!(
                admit(&invalid, "__helper").unwrap().unwrap_err(),
                FAILURE_EXIT
            );
        }
        let datagram = std::os::unix::net::UnixDatagram::pair().unwrap();
        let fd = datagram.0.as_raw_fd().to_string();
        assert!(
            admit(&args(&["__helper", &fd]), "__helper")
                .unwrap()
                .is_err()
        );
    }
}
