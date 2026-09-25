//! Sterile workload-gate entrypoint. This runs before ordinary admission or
//! signal ownership. The owner alone supplies the private execution request.
use std::collections::BTreeSet;
use std::ffi::{CString, OsString};
use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

const COMMAND: &str = "__workload-gate";
const MAX_REQUEST: usize = 1024 * 1024;
const STARTUP_TIMEOUT: Duration = Duration::from_secs(10);

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

const PROTOCOL: crate::channel::Protocol = crate::channel::Protocol {
    magic: *b"NXG1",
    max: MAX_REQUEST,
};

/// Return None for ordinary runtime commands. No ordinary runtime initialization
/// may precede this dispatch; invalid internal invocations emit no request data.
pub fn dispatch(args: &[OsString]) -> Option<i32> {
    let mut channel = match crate::channel::admit(args, COMMAND)? {
        Ok(channel) => channel,
        Err(exit) => return Some(exit),
    };
    let result = receive_and_exec(&mut channel, STARTUP_TIMEOUT);
    let failure = result.expect_err("successful exec never returns");
    // UnixStream writes suppress SIGPIPE. Reporting is best effort and bounded:
    // the socket is nonblocking and the fixed failure record is one byte.
    let _ = channel.write(&[failure as u8]);
    Some(crate::channel::FAILURE_EXIT)
}

fn receive_and_exec(channel: &mut UnixStream, timeout: Duration) -> Result<(), Failure> {
    reset_signals()?;
    let deadline = Instant::now() + timeout;
    let bytes = PROTOCOL
        .read(channel, deadline)
        .map_err(|_| Failure::Frame)?;
    // Write-half closure terminates the sole request. Extra bytes or a second
    // request reject; a complete buffered request may authorize after owner loss.
    crate::channel::read_eof(channel, deadline).map_err(|_| Failure::Frame)?;
    let request: Request = serde_json::from_slice(&bytes).map_err(|_| Failure::Request)?;
    let execution = Execution::validate(request)?;
    execution.exec()
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

/// A refused release. Only a committed registration leaves a process record
/// that the caller must settle from the contained child's facts.
pub enum Refusal<U, R = U> {
    Unregistered(U),
    Registered(R),
}

/// Even after an ambiguous permission send the caller retains the process
/// handle and must contain/reap it. Never turn delivery failure into detachment.
pub struct LaunchFailure {
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
        let frame = PROTOCOL
            .encode(&bytes)
            .map_err(|_| launch_error("workload execution request exceeds startup limit"))?;
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
        let (channel, child_channel) = crate::channel::pair()
            .map_err(|_| launch_error("cannot create workload startup channel"))?;
        let mut command = std::process::Command::new(launcher);
        command.arg(COMMAND);
        // SlotGuard appends its own close; command consumption prevents
        // callbacks outliving these descriptors.
        crate::channel::inherit(&mut command, &child_channel, &channel);
        command
            .env_clear()
            .current_dir("/")
            .process_group(0)
            .stdin(stdin)
            .stdout(stdout)
            .stderr(stderr);
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
    ) -> Result<std::process::Child, Refusal<LaunchFailure>> {
        let Self {
            child,
            mut channel,
            frame,
            deadline,
        } = self;
        let mut registered = false;
        let result = (|| {
            checkpoint()?;
            check_deadline(deadline)?;
            register(&child)?;
            registered = true;
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
            Err(error) => {
                let failure = LaunchFailure {
                    child,
                    error: Box::new(error),
                };
                Err(if registered {
                    Refusal::Registered(failure)
                } else {
                    Refusal::Unregistered(failure)
                })
            }
        }
    }
}

fn wait_startup(
    channel: &UnixStream,
    events: libc::c_short,
    deadline: Instant,
) -> crate::RuntimeResult<()> {
    crate::channel::wait(channel, events, deadline).map_err(|error| {
        launch_error(if error.kind() == io::ErrorKind::TimedOut {
            "workload startup deadline expired"
        } else {
            "workload startup observation failed"
        })
    })
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
