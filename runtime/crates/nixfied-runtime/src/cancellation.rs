use std::mem;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use crate::error::{ErrorCode, RuntimeError, RuntimeResult};

static PROCESS_SIGNAL_CANCELED: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Default)]
pub struct CancellationToken {
    canceled: Arc<AtomicBool>,
}

impl CancellationToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.canceled.store(true, Ordering::SeqCst);
    }

    pub fn is_canceled(&self) -> bool {
        self.canceled.load(Ordering::SeqCst) || PROCESS_SIGNAL_CANCELED.load(Ordering::SeqCst)
    }

    pub fn check(&self) -> RuntimeResult<()> {
        if self.is_canceled() {
            Err(canceled_error())
        } else {
            Ok(())
        }
    }
}

pub fn canceled_error() -> RuntimeError {
    RuntimeError::new(ErrorCode::Canceled, "run was canceled")
}

pub fn sleep_cancellable(duration: Duration, token: &CancellationToken) -> RuntimeResult<()> {
    let deadline = Instant::now() + duration;
    while Instant::now() < deadline {
        token.check()?;
        let remaining = deadline.saturating_duration_since(Instant::now());
        thread::sleep(remaining.min(Duration::from_millis(10)));
    }
    token.check()
}

extern "C" fn handle_signal(_signal: libc::c_int) {
    PROCESS_SIGNAL_CANCELED.store(true, Ordering::SeqCst);
}

pub struct ProcessSignalGuard {
    previous: Vec<(libc::c_int, libc::sigaction)>,
}

impl ProcessSignalGuard {
    pub fn install() -> RuntimeResult<Self> {
        PROCESS_SIGNAL_CANCELED.store(false, Ordering::SeqCst);
        let mut previous = Vec::with_capacity(3);
        for signal in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
            previous.push(install_handler(signal)?);
        }
        Ok(Self { previous })
    }
}

impl Drop for ProcessSignalGuard {
    fn drop(&mut self) {
        for (signal, previous) in self.previous.iter().rev() {
            unsafe {
                libc::sigaction(*signal, previous, std::ptr::null_mut());
            }
        }
    }
}

fn install_handler(signal: libc::c_int) -> RuntimeResult<(libc::c_int, libc::sigaction)> {
    unsafe {
        let mut action: libc::sigaction = mem::zeroed();
        let mut previous: libc::sigaction = mem::zeroed();
        action.sa_sigaction = handle_signal as usize;
        action.sa_flags = 0;
        libc::sigemptyset(&mut action.sa_mask);
        if libc::sigaction(signal, &action, &mut previous) == 0 {
            Ok((signal, previous))
        } else {
            Err(RuntimeError::new(
                ErrorCode::PlatformUnsupported,
                format!(
                    "failed to install cancellation signal handler for signal {signal}: {}",
                    std::io::Error::last_os_error()
                ),
            ))
        }
    }
}
