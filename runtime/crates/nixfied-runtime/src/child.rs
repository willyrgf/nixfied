//! Sole owner of direct-child wait operations. Containment and capture remain
//! separate obligations after the process has been reaped.

use std::io;
use std::process::{Child, ExitStatus};

pub(crate) struct OwnedChild {
    state: ChildState,
}

enum ChildState {
    Unreaped(Child),
    Reaped { pid: u32, status: ExitStatus },
}

impl From<Child> for OwnedChild {
    fn from(child: Child) -> Self {
        Self {
            state: ChildState::Unreaped(child),
        }
    }
}

impl OwnedChild {
    pub(crate) fn id(&self) -> u32 {
        match &self.state {
            ChildState::Unreaped(child) => child.id(),
            ChildState::Reaped { pid, .. } => *pid,
        }
    }

    pub(crate) fn observe(&mut self) -> io::Result<Option<ExitStatus>> {
        match &mut self.state {
            ChildState::Reaped { status, .. } => Ok(Some(*status)),
            ChildState::Unreaped(child) => {
                let pid = child.id();
                match child.try_wait()? {
                    None => Ok(None),
                    Some(status) => {
                        self.state = ChildState::Reaped { pid, status };
                        Ok(Some(status))
                    }
                }
            }
        }
    }

    pub(crate) fn kill(&mut self) -> io::Result<()> {
        // Observe first: a completed child's numeric PID is no longer signaling
        // authority. The reaped alternative contains no OS child handle.
        self.observe()?;
        match &mut self.state {
            ChildState::Unreaped(child) => child.kill(),
            ChildState::Reaped { .. } => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    #[test]
    fn reaped_child_retains_exit_evidence_without_wait_or_signal_authority() {
        let executable = std::env::var_os("NIXFIED_TEST_CHILD").expect("Nix child fixture");
        let child = Command::new(executable)
            .args(["exit", "7"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut owned = OwnedChild::from(child);
        let pid = owned.id();
        let deadline = Instant::now() + Duration::from_secs(5);
        let status = loop {
            if let Some(status) = owned.observe().unwrap() {
                break status;
            }
            assert!(Instant::now() < deadline, "fixture must exit");
            std::thread::sleep(Duration::from_millis(1));
        };
        assert_eq!(status.code(), Some(7));
        let mut raw_status = 0;
        assert_eq!(
            unsafe { libc::waitpid(pid as i32, &mut raw_status, libc::WNOHANG) },
            -1
        );
        assert_eq!(
            io::Error::last_os_error().raw_os_error(),
            Some(libc::ECHILD)
        );
        owned.kill().unwrap();
        assert_eq!(owned.id(), pid);
        assert_eq!(owned.observe().unwrap().unwrap().code(), Some(7));
        assert!(matches!(owned.state, ChildState::Reaped { .. }));
    }
}
