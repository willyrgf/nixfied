//! One-bit per-session cancellation FIFO.
//!
//! The endpoint lives in the session's never-reused evidence directory. Any
//! byte means "cancel this session"; the endpoint carries no lifecycle state and
//! grants no cleanup authority. The owner keeps a separate writer open so the
//! reader never observes idle EOF, and a scoped receiver only sets the existing
//! cancellation token.
use std::ffi::CStr;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::cancellation::CancellationToken;
use crate::error::{ErrorCode, RuntimeError, RuntimeResult};

pub const CONTROL_FIFO_NAME: &str = "control";
const CONTROL_FIFO: &CStr = c"control";
const RECEIVER_POLL: Duration = Duration::from_millis(50);

/// The owner's endpoint. It must be shut down while the slot is still held.
pub struct SessionControl {
    directory: OwnedFd,
    path: PathBuf,
    keeper: Option<OwnedFd>,
    stop: Arc<AtomicBool>,
    receiver: Option<JoinHandle<RuntimeResult<()>>>,
}

impl SessionControl {
    /// Create the session's FIFO exclusively and start its receiver. A
    /// pre-existing endpoint is an identity collision, never reused.
    pub fn establish(run_dir: &Path, cancellation: &CancellationToken) -> RuntimeResult<Self> {
        let path = run_dir.join(CONTROL_FIFO_NAME);
        let error = |operation: &str, error: io::Error| {
            RuntimeError::new(
                ErrorCode::StateUnwritable,
                format!(
                    "failed to {operation} session control endpoint {}: {error}",
                    path.display()
                ),
            )
        };
        let directory = open_directory(run_dir).map_err(|e| error("open", e))?;
        if unsafe { libc::mkfifoat(directory.as_raw_fd(), CONTROL_FIFO.as_ptr(), 0o600) } != 0 {
            return Err(error("create", io::Error::last_os_error()));
        }
        let reader = open_fifo(&directory, libc::O_RDONLY).map_err(|e| error("open", e))?;
        // The reader exists, so the nonblocking keeper open cannot fail with ENXIO.
        let keeper = open_fifo(&directory, libc::O_WRONLY).map_err(|e| error("open", e))?;
        let stop = Arc::new(AtomicBool::new(false));
        let receiver = {
            let stop = Arc::clone(&stop);
            let cancellation = cancellation.clone();
            std::thread::Builder::new()
                .name("nixfied-session-control".into())
                .spawn(move || receive(reader, &stop, &cancellation))
                .map_err(|e| error("start receiver for", e))?
        };
        Ok(Self {
            directory,
            path,
            keeper: Some(keeper),
            stop,
            receiver: Some(receiver),
        })
    }

    /// Stop and join the receiver, close every descriptor, then remove the
    /// endpoint. Later senders observe the absent endpoint and the settled
    /// session record instead of a reader.
    pub fn shutdown(mut self) -> RuntimeResult<()> {
        self.stop_receiver()
    }

    fn stop_receiver(&mut self) -> RuntimeResult<()> {
        self.stop.store(true, Ordering::SeqCst);
        let joined = match self.receiver.take() {
            Some(receiver) => receiver.join().unwrap_or_else(|_| {
                Err(RuntimeError::new(
                    ErrorCode::LifecycleFailed,
                    "session control receiver panicked",
                ))
            }),
            None => return Ok(()),
        };
        drop(self.keeper.take());
        let removed =
            if unsafe { libc::unlinkat(self.directory.as_raw_fd(), CONTROL_FIFO.as_ptr(), 0) } == 0
            {
                Ok(())
            } else {
                Err(RuntimeError::new(
                    ErrorCode::StateUnwritable,
                    format!(
                        "failed to remove session control endpoint {}: {}",
                        self.path.display(),
                        io::Error::last_os_error()
                    ),
                ))
            };
        crate::error::both(joined, removed)
    }
}

impl Drop for SessionControl {
    fn drop(&mut self) {
        let _ = self.stop_receiver();
    }
}

/// Poll with a bounded interval so shutdown never waits on an idle FIFO. A
/// transport failure requests ordinary finalization and reports its cause.
fn receive(
    reader: OwnedFd,
    stop: &AtomicBool,
    cancellation: &CancellationToken,
) -> RuntimeResult<()> {
    let mut buffer = [0_u8; 64];
    while !stop.load(Ordering::SeqCst) {
        let mut poll = libc::pollfd {
            fd: reader.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let ready = unsafe { libc::poll(&mut poll, 1, RECEIVER_POLL.as_millis() as libc::c_int) };
        if ready < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            cancellation.cancel();
            return Err(transport_error(error));
        }
        if ready == 0 {
            continue;
        }
        let read =
            unsafe { libc::read(reader.as_raw_fd(), buffer.as_mut_ptr().cast(), buffer.len()) };
        if read > 0 {
            cancellation.cancel();
            continue;
        }
        let error = io::Error::last_os_error();
        match error.kind() {
            io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted => {}
            _ if read == 0 => {
                // The owner's keeper prevents EOF; losing it is a transport failure.
                cancellation.cancel();
                return Err(transport_error(io::Error::from(
                    io::ErrorKind::UnexpectedEof,
                )));
            }
            _ => {
                cancellation.cancel();
                return Err(transport_error(error));
            }
        }
    }
    Ok(())
}

fn transport_error(error: io::Error) -> RuntimeError {
    RuntimeError::new(
        ErrorCode::LifecycleFailed,
        format!("session control endpoint failed: {error}"),
    )
}

/// What a sender observed at one selected session's endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancellationDelivery {
    /// A live reader holds the endpoint; the byte was written or one is pending.
    Requested,
    /// No reader or endpoint: the owner is not (or no longer) listening.
    Unavailable,
}

/// Request cancellation of exactly the session owning `run_dir`. Never blocks:
/// no reader, a missing endpoint, or a broken pipe mean the owner is absent.
pub fn request_cancellation(run_dir: &Path) -> RuntimeResult<CancellationDelivery> {
    let failure = |error: io::Error| {
        RuntimeError::new(
            ErrorCode::LifecycleFailed,
            format!(
                "failed to request cancellation through {}: {error}",
                run_dir.join(CONTROL_FIFO_NAME).display()
            ),
        )
    };
    let directory = match open_directory(run_dir) {
        Ok(directory) => directory,
        Err(error) if error.raw_os_error() == Some(libc::ENOENT) => {
            return Ok(CancellationDelivery::Unavailable);
        }
        Err(error) => return Err(failure(error)),
    };
    let writer = match open_fifo(&directory, libc::O_WRONLY) {
        Ok(writer) => writer,
        Err(error) if matches!(error.raw_os_error(), Some(libc::ENXIO | libc::ENOENT)) => {
            return Ok(CancellationDelivery::Unavailable);
        }
        Err(error) => return Err(failure(error)),
    };
    loop {
        let written = unsafe { libc::write(writer.as_raw_fd(), [1_u8].as_ptr().cast(), 1) };
        if written == 1 {
            return Ok(CancellationDelivery::Requested);
        }
        let error = io::Error::last_os_error();
        match error.raw_os_error() {
            Some(libc::EINTR) => continue,
            // A full buffer already holds pending cancellation bytes.
            Some(libc::EAGAIN) => return Ok(CancellationDelivery::Requested),
            Some(libc::EPIPE) => return Ok(CancellationDelivery::Unavailable),
            _ => return Err(failure(error)),
        }
    }
}

/// Open `<registry>/runs/<session>` through held directory descriptors. The
/// managed `runs` and session components are never followed as symlinks and
/// must be private directories of the effective user.
fn open_directory(run_dir: &Path) -> io::Result<OwnedFd> {
    let invalid = || io::Error::new(io::ErrorKind::InvalidInput, "invalid session path");
    let session = run_dir.file_name().ok_or_else(invalid)?;
    let runs = run_dir.parent().ok_or_else(invalid)?;
    if runs.file_name() != Some(std::ffi::OsStr::new("runs")) {
        return Err(invalid());
    }
    let anchor = runs.parent().ok_or_else(invalid)?;
    let anchor = std::ffi::CString::new(std::os::unix::ffi::OsStrExt::as_bytes(anchor.as_os_str()))
        .map_err(|_| invalid())?;
    let anchor = owned(unsafe {
        libc::open(
            anchor.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    })?;
    let runs = private_child(&anchor, c"runs")?;
    let session = std::ffi::CString::new(std::os::unix::ffi::OsStrExt::as_bytes(session))
        .map_err(|_| invalid())?;
    private_child(&runs, &session)
}

fn private_child(parent: &OwnedFd, name: &std::ffi::CStr) -> io::Result<OwnedFd> {
    let bytes = name.to_bytes();
    if bytes.is_empty() || bytes == b"." || bytes == b".." || bytes.contains(&b'/') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid session component",
        ));
    }
    let child = owned(unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    })?;
    let mut stat = std::mem::MaybeUninit::<libc::stat>::zeroed();
    if unsafe { libc::fstat(child.as_raw_fd(), stat.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: fstat initialized the complete stat on success.
    let stat = unsafe { stat.assume_init() };
    if stat.st_uid != unsafe { libc::geteuid() } || stat.st_mode & 0o077 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "session directory is not private to the effective user",
        ));
    }
    Ok(child)
}

/// Open the endpoint without following a symlink and verify it is a private
/// FIFO owned by the effective user before any byte is exchanged.
fn open_fifo(directory: &OwnedFd, access: libc::c_int) -> io::Result<OwnedFd> {
    let fd = owned(unsafe {
        libc::openat(
            directory.as_raw_fd(),
            CONTROL_FIFO.as_ptr(),
            access | libc::O_NONBLOCK | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    })?;
    let mut stat = std::mem::MaybeUninit::<libc::stat>::zeroed();
    if unsafe { libc::fstat(fd.as_raw_fd(), stat.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: fstat initialized the complete stat on success.
    let stat = unsafe { stat.assume_init() };
    if stat.st_mode & libc::S_IFMT != libc::S_IFIFO
        || stat.st_uid != unsafe { libc::geteuid() }
        || stat.st_mode & 0o077 != 0
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "session control endpoint is not a private FIFO",
        ));
    }
    Ok(fd)
}

fn owned(fd: libc::c_int) -> io::Result<OwnedFd> {
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: a successful open returned a new descriptor owned here.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A private `<registry>/runs/<session>` layout, as placement creates it.
    fn directory() -> (crate::test_support::TestDir, PathBuf) {
        use std::os::unix::fs::DirBuilderExt;
        let root = crate::test_support::TestDir::new("control");
        let path = root.join("runs/session");
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&path)
            .unwrap();
        (root, path)
    }

    fn wait_for(token: &CancellationToken) -> bool {
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while std::time::Instant::now() < deadline {
            if token.is_canceled() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        false
    }

    #[test]
    fn duplicate_requests_set_only_the_selected_token_and_shutdown_removes_the_endpoint() {
        let (_first_root, first_dir) = directory();
        let (_second_root, second_dir) = directory();
        let first = CancellationToken::new();
        let second = CancellationToken::new();
        let first_control = SessionControl::establish(&first_dir, &first).unwrap();
        let second_control = SessionControl::establish(&second_dir, &second).unwrap();
        // No idle busy loop or spurious cancellation from the keeper writer.
        std::thread::sleep(RECEIVER_POLL * 3);
        assert!(!first.is_canceled() && !second.is_canceled());
        for _ in 0..3 {
            assert_eq!(
                request_cancellation(&first_dir).unwrap(),
                CancellationDelivery::Requested
            );
        }
        assert!(wait_for(&first));
        assert!(
            !second.is_canceled(),
            "a request never reaches another session"
        );
        first_control.shutdown().unwrap();
        assert!(!first_dir.join(CONTROL_FIFO_NAME).exists());
        assert_eq!(
            request_cancellation(&first_dir).unwrap(),
            CancellationDelivery::Unavailable
        );
        second_control.shutdown().unwrap();
        std::fs::remove_dir_all(first_dir).unwrap();
        std::fs::remove_dir_all(second_dir).unwrap();
    }

    #[test]
    fn collisions_absent_readers_and_substituted_objects_never_deliver() {
        let (_root, dir) = directory();
        let token = CancellationToken::new();
        let control = SessionControl::establish(&dir, &token).unwrap();
        assert!(
            SessionControl::establish(&dir, &CancellationToken::new()).is_err(),
            "an existing endpoint is an identity collision"
        );
        drop(control);
        assert!(!token.is_canceled());

        // A FIFO without a reader: the sender reports absence without blocking.
        let fifo =
            std::ffi::CString::new(dir.join(CONTROL_FIFO_NAME).as_os_str().as_encoded_bytes())
                .unwrap();
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        assert_eq!(
            request_cancellation(&dir).unwrap(),
            CancellationDelivery::Unavailable
        );
        std::fs::remove_file(dir.join(CONTROL_FIFO_NAME)).unwrap();

        // Symlinks and regular files are rejected before any write.
        let target = dir.join("target");
        std::fs::write(&target, b"untouched").unwrap();
        std::os::unix::fs::symlink(&target, dir.join(CONTROL_FIFO_NAME)).unwrap();
        assert!(request_cancellation(&dir).is_err());
        std::fs::remove_file(dir.join(CONTROL_FIFO_NAME)).unwrap();
        std::fs::write(dir.join(CONTROL_FIFO_NAME), b"").unwrap();
        assert!(request_cancellation(&dir).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"untouched");
        assert_eq!(
            request_cancellation(&dir.parent().unwrap().join("missing")).unwrap(),
            CancellationDelivery::Unavailable
        );
        // A symlinked session directory is never followed.
        let link = dir.parent().unwrap().join("linked");
        std::os::unix::fs::symlink(&dir, &link).unwrap();
        assert!(request_cancellation(&link).is_err());
        assert!(SessionControl::establish(&link, &CancellationToken::new()).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_full_buffer_already_holds_the_pending_request_and_never_blocks() {
        let (_root, dir) = directory();
        let fifo =
            std::ffi::CString::new(dir.join(CONTROL_FIFO_NAME).as_os_str().as_encoded_bytes())
                .unwrap();
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        // A reader that never drains, like a stopped owner.
        let reader = owned(unsafe {
            libc::open(
                fifo.as_ptr(),
                libc::O_RDONLY | libc::O_NONBLOCK | libc::O_CLOEXEC,
            )
        })
        .unwrap();
        let writer = owned(unsafe {
            libc::open(
                fifo.as_ptr(),
                libc::O_WRONLY | libc::O_NONBLOCK | libc::O_CLOEXEC,
            )
        })
        .unwrap();
        let chunk = [1_u8; 4096];
        while unsafe { libc::write(writer.as_raw_fd(), chunk.as_ptr().cast(), chunk.len()) } > 0 {}
        assert_eq!(
            io::Error::last_os_error().raw_os_error(),
            Some(libc::EAGAIN)
        );
        let started = std::time::Instant::now();
        for _ in 0..16 {
            assert_eq!(
                request_cancellation(&dir).unwrap(),
                CancellationDelivery::Requested
            );
        }
        assert!(started.elapsed() < Duration::from_secs(1));
        drop((reader, writer));
        std::fs::remove_dir_all(dir.parent().unwrap().parent().unwrap()).unwrap();
    }

    #[test]
    fn many_requests_cancel_once_and_shutdown_stays_bounded() {
        let (_root, dir) = directory();
        let token = CancellationToken::new();
        let control = SessionControl::establish(&dir, &token).unwrap();
        // A sender that connects and closes without a byte is not a request:
        // the keeper writer prevents the receiver from observing EOF.
        let fifo =
            std::ffi::CString::new(dir.join(CONTROL_FIFO_NAME).as_os_str().as_encoded_bytes())
                .unwrap();
        for _ in 0..8 {
            drop(
                owned(unsafe {
                    libc::open(
                        fifo.as_ptr(),
                        libc::O_WRONLY | libc::O_NONBLOCK | libc::O_CLOEXEC,
                    )
                })
                .unwrap(),
            );
        }
        std::thread::sleep(RECEIVER_POLL * 3);
        assert!(!token.is_canceled(), "a closed silent writer never cancels");
        let senders: Vec<_> = (0..8)
            .map(|_| {
                let dir = dir.clone();
                std::thread::spawn(move || {
                    for _ in 0..20_000 {
                        assert_eq!(
                            request_cancellation(&dir).unwrap(),
                            CancellationDelivery::Requested
                        );
                    }
                })
            })
            .collect();
        for sender in senders {
            sender.join().unwrap();
        }
        assert!(wait_for(&token));
        let started = std::time::Instant::now();
        control.shutdown().unwrap();
        assert!(started.elapsed() < Duration::from_secs(1));
        assert_eq!(
            request_cancellation(&dir).unwrap(),
            CancellationDelivery::Unavailable
        );
        std::fs::remove_dir_all(dir.parent().unwrap().parent().unwrap()).unwrap();
    }

    #[test]
    fn non_private_or_misplaced_session_directories_are_refused() {
        use std::os::unix::fs::PermissionsExt;
        let (_root, dir) = directory();
        let runs = dir.parent().unwrap().to_path_buf();
        for target in [&dir, &runs] {
            std::fs::set_permissions(target, std::fs::Permissions::from_mode(0o755)).unwrap();
            assert!(SessionControl::establish(&dir, &CancellationToken::new()).is_err());
            assert!(request_cancellation(&dir).is_err());
            std::fs::set_permissions(target, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        assert!(!dir.join(CONTROL_FIFO_NAME).exists());
        // A session outside `runs/` is not a session endpoint.
        let stray = runs.parent().unwrap().join("session");
        std::fs::create_dir(&stray).unwrap();
        std::fs::set_permissions(&stray, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(SessionControl::establish(&stray, &CancellationToken::new()).is_err());
        assert!(request_cancellation(&stray).is_err());
        // A symlinked `runs` component is never followed.
        let other = runs.parent().unwrap().join("other");
        std::fs::create_dir(&other).unwrap();
        std::os::unix::fs::symlink(&runs, other.join("runs")).unwrap();
        let aliased = other.join("runs").join("session");
        assert!(SessionControl::establish(&aliased, &CancellationToken::new()).is_err());
        assert!(request_cancellation(&aliased).is_err());
        std::fs::remove_dir_all(runs.parent().unwrap()).unwrap();
    }
}
