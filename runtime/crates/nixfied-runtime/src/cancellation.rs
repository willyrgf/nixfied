use std::mem;
use std::num::NonZeroU64;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use crate::error::{ErrorCode, RuntimeError, RuntimeResult};

static PROCESS_SIGNAL_CANCELED: AtomicBool = AtomicBool::new(false);
static PROCESS_SIGNALS: AtomicUsize = AtomicUsize::new(0);

/// One caller-local observation budget. Carry this same value through selection,
/// transport and task waiting; constructing an absolute full-duration deadline
/// would overflow for valid large u64 millisecond limits.
#[derive(Debug)]
pub struct WaitBudget {
    started: Instant,
    limit: Duration,
}

impl WaitBudget {
    pub fn new(milliseconds: NonZeroU64) -> Self {
        Self {
            started: Instant::now(),
            limit: Duration::from_millis(milliseconds.get()),
        }
    }

    pub fn remaining(&self) -> Duration {
        self.limit.saturating_sub(self.started.elapsed())
    }

    pub fn is_expired(&self) -> bool {
        self.remaining().is_zero()
    }

    /// A checked short deadline for native channel helpers, never more than
    /// 10 ms or the caller's remaining budget. None means local expiry.
    pub fn poll_deadline(&self) -> Option<Instant> {
        let now = Instant::now();
        let remaining = self
            .limit
            .saturating_sub(now.saturating_duration_since(self.started));
        if remaining.is_zero() {
            None
        } else {
            now.checked_add(remaining.min(Duration::from_millis(10)))
        }
    }
}

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

/// Whether this process received SIGINT, SIGTERM, or SIGHUP. Unlike a
/// session token, a FIFO request never sets it, so command-scoped presentation
/// keeps draining after a remote `down`.
pub fn signal_received() -> bool {
    PROCESS_SIGNAL_CANCELED.load(Ordering::SeqCst)
}

pub fn canceled_error() -> RuntimeError {
    RuntimeError::new(ErrorCode::Canceled, "run was canceled")
}

extern "C" fn handle_signal(_signal: libc::c_int) {
    PROCESS_SIGNAL_CANCELED.store(true, Ordering::SeqCst);
    PROCESS_SIGNALS.fetch_add(1, Ordering::SeqCst);
}

/// Termination signals received so far. A later value than an earlier
/// snapshot means a new request arrived after that point.
pub fn signal_count() -> usize {
    PROCESS_SIGNALS.load(Ordering::SeqCst)
}

pub struct ProcessSignalGuard {
    previous: Vec<(libc::c_int, libc::sigaction)>,
}

impl ProcessSignalGuard {
    pub fn install() -> RuntimeResult<Self> {
        PROCESS_SIGNAL_CANCELED.store(false, Ordering::SeqCst);
        let mut previous = Vec::with_capacity(4);
        for signal in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
            match install_handler(signal, handle_signal as *const () as libc::sighandler_t)
                .map_err(|error| RuntimeError::new(ErrorCode::PlatformUnsupported,
                    format!("failed to install cancellation signal handler for signal {signal}: {error}")))
            {
                Ok(previous_action) => previous.push(previous_action),
                Err(error) => {
                    restore_handlers(&previous);
                    return Err(error);
                }
            }
        }
        // Command output writes must observe EPIPE as a typed projection issue.  The
        // default disposition would terminate the runtime before the worker can
        // report the broken pipe.  The prior disposition is restored by Drop.
        match install_handler(libc::SIGPIPE, libc::SIG_IGN).map_err(|error| {
            RuntimeError::new(
                ErrorCode::PlatformUnsupported,
                format!("failed to install SIGPIPE handling: {error}"),
            )
        }) {
            Ok(previous_action) => previous.push(previous_action),
            Err(error) => {
                restore_handlers(&previous);
                return Err(error);
            }
        }
        Ok(Self { previous })
    }
}

impl Drop for ProcessSignalGuard {
    fn drop(&mut self) {
        restore_handlers(&self.previous);
    }
}

fn restore_handlers(previous: &[(libc::c_int, libc::sigaction)]) {
    for (signal, previous) in previous.iter().rev() {
        unsafe {
            libc::sigaction(*signal, previous, std::ptr::null_mut());
        }
    }
}

fn install_handler(
    signal: libc::c_int,
    handler: libc::sighandler_t,
) -> std::io::Result<(libc::c_int, libc::sigaction)> {
    unsafe {
        let mut action: libc::sigaction = mem::zeroed();
        let mut previous: libc::sigaction = mem::zeroed();
        action.sa_sigaction = handler;
        action.sa_flags = 0;
        libc::sigemptyset(&mut action.sa_mask);
        if libc::sigaction(signal, &action, &mut previous) == 0 {
            Ok((signal, previous))
        } else {
            Err(std::io::Error::last_os_error())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observation_budget_accepts_full_u64_without_absolute_deadline_overflow() {
        let budget = WaitBudget::new(NonZeroU64::new(u64::MAX).unwrap());
        assert!(!budget.is_expired());
        assert!(budget.remaining() > Duration::from_secs(u32::MAX.into()));
        let before = Instant::now();
        let deadline = budget.poll_deadline().unwrap();
        let after = Instant::now();
        assert!(deadline >= before);
        assert!(deadline <= after + Duration::from_millis(10));
    }

    #[test]
    fn observation_budget_keeps_elapsed_time_and_saturates_after_expiry() {
        let started = Instant::now().checked_sub(Duration::from_secs(2)).unwrap();
        let budget = WaitBudget {
            started,
            limit: Duration::from_secs(1),
        };
        assert_eq!(budget.remaining(), Duration::ZERO);
        assert!(budget.is_expired());
        assert!(budget.poll_deadline().is_none());
    }
}
