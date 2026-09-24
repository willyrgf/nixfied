//! Coordinate non-atomic close-on-exec setup with every runtime workload spawn.
use std::io;
use std::process::{Child, Command};

#[cfg(target_os = "macos")]
static DESCRIPTOR_SETUP: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(target_os = "macos")]
pub(crate) fn exclude_spawn<T>(action: impl FnOnce() -> io::Result<T>) -> io::Result<T> {
    let _guard = DESCRIPTOR_SETUP
        .lock()
        .map_err(|_| io::Error::other("descriptor setup coordination is poisoned"))?;
    action()
}

pub(crate) fn command(command: &mut Command) -> io::Result<Child> {
    #[cfg(target_os = "macos")]
    {
        exclude_spawn(|| command.spawn())
    }
    #[cfg(target_os = "linux")]
    {
        command.spawn()
    }
}
