//! Sterile workload-gate entrypoint. This runs before ordinary admission or
//! signal ownership. The owner alone supplies the private execution request.
use std::collections::BTreeSet;
use std::ffi::{CString, OsString};
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

const COMMAND: &str = "__workload-gate";
const MAX_REQUEST: usize = 1024 * 1024;
const STARTUP_TIMEOUT: Duration = Duration::from_secs(10);
const FAILURE_EXIT: i32 = 125;

/// This protocol is private to the exact runtime ABI. Paths and environment
/// values preserve Unix bytes; they never enter argv or persistent diagnostics.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    executable: Vec<u8>,
    args: Vec<Vec<u8>>,
    env: Vec<(Vec<u8>, Vec<u8>)>,
    cwd: Vec<u8>,
}

struct Execution {
    executable: CString,
    args: Vec<CString>,
    env: Vec<CString>,
    cwd: CString,
}

#[derive(Clone, Copy)]
#[repr(u8)]
enum Failure {
    Frame = 1,
    Request = 2,
    Setup = 3,
    Exec = 4,
}

/// Return None for ordinary runtime commands. No ordinary runtime initialization
/// may precede this dispatch; invalid internal invocations emit no request data.
pub fn dispatch(args: &[OsString]) -> Option<i32> {
    if args.first().is_none_or(|arg| arg != COMMAND) {
        return None;
    }
    let [_, descriptor] = args else {
        return Some(FAILURE_EXIT);
    };
    let Some(fd) = descriptor
        .to_str()
        .and_then(|value| value.parse::<i32>().ok())
        .filter(|fd| *fd >= 3)
    else {
        return Some(FAILURE_EXIT);
    };
    // Verify the inherited descriptor before creating an owning Rust socket.
    let mut peer = std::mem::MaybeUninit::<libc::sockaddr_un>::zeroed();
    let mut length = std::mem::size_of::<libc::sockaddr_un>() as libc::socklen_t;
    if unsafe { libc::getpeername(fd, peer.as_mut_ptr().cast(), &mut length) } != 0
        || unsafe { peer.assume_init().sun_family } as i32 != libc::AF_UNIX
    {
        return Some(FAILURE_EXIT);
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
        return Some(FAILURE_EXIT);
    }
    // SAFETY: this internal process takes sole ownership of its inherited socket.
    let mut channel = unsafe { UnixStream::from_raw_fd(fd) };
    let result = receive_and_exec(&mut channel, STARTUP_TIMEOUT);
    let failure = result.expect_err("successful exec never returns");
    // UnixStream writes suppress SIGPIPE. Reporting is best effort and bounded:
    // the socket is nonblocking and the fixed failure record is one byte.
    let _ = channel.write(&[failure as u8]);
    Some(FAILURE_EXIT)
}

fn receive_and_exec(channel: &mut UnixStream, timeout: Duration) -> Result<(), Failure> {
    reset_signals()?;
    let fd = channel.as_raw_fd();
    if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
        return Err(Failure::Setup);
    }
    channel.set_nonblocking(true).map_err(|_| Failure::Setup)?;
    let deadline = Instant::now() + timeout;
    let mut header = [0; 8];
    read_exact(channel, &mut header, deadline)?;
    if &header[..4] != b"NXG1" {
        return Err(Failure::Frame);
    }
    let size = u32::from_be_bytes(header[4..].try_into().unwrap()) as usize;
    if size == 0 || size > MAX_REQUEST {
        return Err(Failure::Frame);
    }
    let mut bytes = vec![0; size];
    read_exact(channel, &mut bytes, deadline)?;
    // Write-half closure terminates the sole request. Extra bytes or a second
    // request reject; a complete buffered request may authorize after owner loss.
    let mut trailing = [0];
    if read_some(channel, &mut trailing, deadline)? != 0 {
        return Err(Failure::Frame);
    }
    let request: Request = serde_json::from_slice(&bytes).map_err(|_| Failure::Request)?;
    let execution = Execution::validate(request)?;
    execution.exec()
}

fn read_exact(
    channel: &mut UnixStream,
    mut bytes: &mut [u8],
    deadline: Instant,
) -> Result<(), Failure> {
    while !bytes.is_empty() {
        let count = read_some(channel, bytes, deadline)?;
        if count == 0 {
            return Err(Failure::Frame);
        }
        bytes = &mut bytes[count..];
    }
    Ok(())
}

fn read_some(
    channel: &mut UnixStream,
    bytes: &mut [u8],
    deadline: Instant,
) -> Result<usize, Failure> {
    loop {
        if Instant::now() >= deadline {
            return Err(Failure::Frame);
        }
        match channel.read(bytes) {
            Ok(count) => return Ok(count),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                let remaining = deadline.saturating_duration_since(Instant::now());
                let mut poll = libc::pollfd {
                    fd: channel.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                };
                let result =
                    unsafe { libc::poll(&mut poll, 1, remaining.as_millis().clamp(1, 50) as i32) };
                if result < 0 && io::Error::last_os_error().kind() != io::ErrorKind::Interrupted {
                    return Err(Failure::Frame);
                }
            }
            Err(_) => return Err(Failure::Frame),
        }
    }
}

fn reset_signals() -> Result<(), Failure> {
    unsafe {
        let mut mask = std::mem::zeroed();
        if libc::sigemptyset(&mut mask) != 0
            || libc::sigprocmask(libc::SIG_SETMASK, &mask, std::ptr::null_mut()) != 0
        {
            return Err(Failure::Setup);
        }
        for signal in [
            libc::SIGHUP,
            libc::SIGINT,
            libc::SIGQUIT,
            libc::SIGPIPE,
            libc::SIGTERM,
            libc::SIGCHLD,
            libc::SIGUSR1,
            libc::SIGUSR2,
            libc::SIGALRM,
            libc::SIGTSTP,
            libc::SIGTTIN,
            libc::SIGTTOU,
        ] {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = libc::SIG_DFL;
            if libc::sigemptyset(&mut action.sa_mask) != 0
                || libc::sigaction(signal, &action, std::ptr::null_mut()) != 0
            {
                return Err(Failure::Setup);
            }
        }
    }
    Ok(())
}

impl Execution {
    fn validate(request: Request) -> Result<Self, Failure> {
        fn absolute(value: Vec<u8>) -> Result<CString, Failure> {
            if value.first() != Some(&b'/') {
                return Err(Failure::Request);
            }
            CString::new(value).map_err(|_| Failure::Request)
        }
        let executable = absolute(request.executable)?;
        let cwd = absolute(request.cwd)?;
        let mut args = Vec::with_capacity(request.args.len() + 1);
        args.push(executable.clone());
        for arg in request.args {
            args.push(CString::new(arg).map_err(|_| Failure::Request)?);
        }
        let mut names = BTreeSet::new();
        let mut env = Vec::with_capacity(request.env.len());
        for (mut name, value) in request.env {
            if name.is_empty() || name.contains(&b'=') || !names.insert(name.clone()) {
                return Err(Failure::Request);
            }
            name.push(b'=');
            name.extend(value);
            env.push(CString::new(name).map_err(|_| Failure::Request)?);
        }
        Ok(Self {
            executable,
            args,
            env,
            cwd,
        })
    }

    fn exec(self) -> Result<(), Failure> {
        let mut args: Vec<_> = self.args.iter().map(|arg| arg.as_ptr()).collect();
        args.push(std::ptr::null());
        let mut env: Vec<_> = self.env.iter().map(|value| value.as_ptr()).collect();
        env.push(std::ptr::null());
        unsafe {
            if libc::chdir(self.cwd.as_ptr()) != 0 {
                return Err(Failure::Setup);
            }
            libc::execve(self.executable.as_ptr(), args.as_ptr(), env.as_ptr());
        }
        Err(Failure::Exec)
    }
}

/// A validated, bounded request, prepared before any bootstrap child exists.
/// Its bytes can only be sent by the registration-before-permission operation.
pub struct PreparedLaunch {
    frame: Vec<u8>,
}

pub struct PendingLaunch {
    child: std::process::Child,
    channel: UnixStream,
    frame: Vec<u8>,
    deadline: Instant,
}

/// Even after an ambiguous permission send the caller retains the process
/// handle and must contain/reap it. Never turn delivery failure into detachment.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Registration {
    Unconfirmed,
    Committed,
}

pub struct LaunchFailure {
    pub registration: Registration,
    pub child: std::process::Child,
    pub error: Box<crate::RuntimeError>,
}

impl PreparedLaunch {
    pub fn new(
        executable: &str,
        args: &[String],
        env: &std::collections::BTreeMap<String, String>,
        cwd: &std::path::Path,
    ) -> crate::RuntimeResult<Self> {
        use std::os::unix::ffi::OsStrExt;
        let request = Request {
            executable: executable.as_bytes().to_vec(),
            args: args.iter().map(|arg| arg.as_bytes().to_vec()).collect(),
            env: env
                .iter()
                .map(|(name, value)| (name.as_bytes().to_vec(), value.as_bytes().to_vec()))
                .collect(),
            cwd: cwd.as_os_str().as_bytes().to_vec(),
        };
        Execution::validate(request.clone())
            .map_err(|_| launch_error("invalid workload execution request"))?;
        let bytes = serde_json::to_vec(&request)
            .map_err(|_| launch_error("cannot encode workload execution request"))?;
        if bytes.len() > MAX_REQUEST {
            return Err(launch_error(
                "workload execution request exceeds startup limit",
            ));
        }
        let mut frame = Vec::with_capacity(8 + bytes.len());
        frame.extend(b"NXG1");
        frame.extend((bytes.len() as u32).to_be_bytes());
        frame.extend(bytes);
        Ok(Self { frame })
    }

    pub fn spawn(
        self,
        launcher: &std::path::Path,
        authority: &crate::state::ownership::SlotGuard,
        stdin: std::process::Stdio,
        stdout: std::process::Stdio,
        stderr: std::process::Stdio,
    ) -> crate::RuntimeResult<PendingLaunch> {
        use std::os::unix::process::CommandExt;
        authority.validate()?;
        if !launcher.is_absolute() {
            return Err(launch_error("workload gate executable must be absolute"));
        }
        let (channel, child_channel) =
            startup_pair().map_err(|_| launch_error("cannot create workload startup channel"))?;
        let inherited = child_channel.as_raw_fd();
        let writer = channel.as_raw_fd();
        let mut command = std::process::Command::new(launcher);
        command
            .arg(COMMAND)
            .arg(inherited.to_string())
            .env_clear()
            .current_dir("/")
            .process_group(0)
            .stdin(stdin)
            .stdout(stdout)
            .stderr(stderr);
        // SAFETY: only async-signal-safe descriptor syscalls run after fork.
        // The parent's flags never change. SlotGuard appends its own close;
        // command consumption prevents callbacks outliving these descriptors.
        unsafe {
            command.pre_exec(move || {
                if libc::close(writer) != 0 || libc::fcntl(inherited, libc::F_SETFD, 0) != 0 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let deadline = Instant::now() + STARTUP_TIMEOUT;
        let child = authority
            .spawn(command)
            .map_err(|_| launch_error("cannot spawn workload gate"))?;
        drop(child_channel);
        Ok(PendingLaunch {
            child,
            channel,
            frame: self.frame,
            deadline,
        })
    }
}

impl PendingLaunch {
    pub fn id(&self) -> u32 {
        self.child.id()
    }

    /// Abandon an inert bootstrap before registration, preserving the child for
    /// the caller's checked containment and reap. No request bytes were sent.
    pub fn abort(self) -> std::process::Child {
        let Self { child, channel, .. } = self;
        drop(channel);
        child
    }

    /// The callback must verify identity, commit process/event evidence, and
    /// establish live supervision. No execution bytes are sent on its failure.
    /// This consuming operation cannot retry permission or relinquish a child
    /// on failure. EOF after delivery is not a successful workload outcome.
    pub fn register_and_release(
        self,
        register: impl FnOnce(&std::process::Child) -> crate::RuntimeResult<()>,
        mut checkpoint: impl FnMut() -> crate::RuntimeResult<()>,
    ) -> Result<std::process::Child, LaunchFailure> {
        let Self {
            child,
            mut channel,
            frame,
            deadline,
        } = self;
        let mut registration = Registration::Unconfirmed;
        let result = (|| {
            checkpoint()?;
            check_deadline(deadline)?;
            register(&child)?;
            registration = Registration::Committed;
            checkpoint()?;
            let mut remaining = frame.as_slice();
            while !remaining.is_empty() {
                checkpoint()?;
                check_deadline(deadline)?;
                match channel.write(remaining) {
                    Ok(0) => {
                        return Err(launch_error(
                            "workload startup channel closed during permission",
                        ));
                    }
                    Ok(count) => remaining = &remaining[count..],
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        wait_startup(&channel, libc::POLLOUT, deadline)?
                    }
                    Err(_) => return Err(launch_error("workload permission delivery failed")),
                }
            }
            checkpoint()?;
            check_deadline(deadline)?;
            channel
                .shutdown(std::net::Shutdown::Write)
                .map_err(|_| launch_error("workload permission termination failed"))?;
            loop {
                checkpoint()?;
                check_deadline(deadline)?;
                let mut failure = [0];
                match channel.read(&mut failure) {
                    Ok(0) => return Ok(()),
                    Ok(_) => {
                        return Err(launch_error(match failure[0] {
                            1 => "workload gate rejected startup frame",
                            2 => "workload gate rejected execution request",
                            3 => "workload gate setup failed",
                            4 => "workload exec failed",
                            _ => "workload gate returned an invalid failure code",
                        }));
                    }
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        wait_startup(&channel, libc::POLLIN, deadline)?
                    }
                    Err(_) => return Err(launch_error("workload startup response failed")),
                }
            }
        })();
        drop(channel);
        match result {
            Ok(()) => Ok(child),
            Err(error) => Err(LaunchFailure {
                registration,
                child,
                error: Box::new(error),
            }),
        }
    }
}

fn startup_pair() -> io::Result<(UnixStream, UnixStream)> {
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
        }
        Ok(pair)
    })?;
    Ok(pair)
}

fn wait_startup(
    channel: &UnixStream,
    events: libc::c_short,
    deadline: Instant,
) -> crate::RuntimeResult<()> {
    check_deadline(deadline)?;
    let remaining = deadline.saturating_duration_since(Instant::now());
    let mut descriptor = libc::pollfd {
        fd: channel.as_raw_fd(),
        events,
        revents: 0,
    };
    let result = unsafe {
        libc::poll(
            &mut descriptor,
            1,
            remaining.as_millis().clamp(1, 10) as i32,
        )
    };
    if result < 0 && io::Error::last_os_error().kind() != io::ErrorKind::Interrupted {
        return Err(launch_error("workload startup observation failed"));
    }
    Ok(())
}
fn check_deadline(deadline: Instant) -> crate::RuntimeResult<()> {
    if Instant::now() >= deadline {
        Err(launch_error("workload startup deadline expired"))
    } else {
        Ok(())
    }
}
fn launch_error(message: &'static str) -> crate::RuntimeError {
    crate::RuntimeError::new(crate::ErrorCode::ProcEscape, message)
}

#[cfg(test)]
pub(crate) fn test_launcher() -> std::path::PathBuf {
    std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("nixfied-runtime")
}
