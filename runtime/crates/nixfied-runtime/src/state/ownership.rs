//! Exclusive mutation authority for one placed slot. Lock availability permits
//! recovery; it never certifies that a predecessor's workloads have settled.
use std::ffi::CString;
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStrExt;
use std::path::{Component, PathBuf};

use super::HostPlacement;
use crate::cancellation::CancellationToken;
use crate::error::{ErrorCode, RuntimeError, RuntimeResult};
use crate::filesystem::{Directory, PrivateFile};

/// No cloning or raw-descriptor interface: mutation owners borrow this guard.
/// Drop closes on unwind; normal finalization uses the checked consuming release.
pub struct SlotGuard {
    file: PrivateFile,
    directory: Directory,
    registry_path: PathBuf,
    state_base: PathBuf,
    ancestors: Vec<(Directory, CString)>,
}

impl SlotGuard {
    pub fn acquire(
        placement: &HostPlacement,
        cancellation: &CancellationToken,
    ) -> RuntimeResult<Self> {
        cancellation.check()?;
        // HostPlacement currently has public fields. Validate its entire slot
        // relation before bootstrapping rather than trusting a constructed path.
        let relative = placement
            .registry_dir
            .strip_prefix(&placement.state_base)
            .map_err(|_| invalid("registry placement escapes the state base"))?;
        let components: Vec<_> = relative.components().collect();
        let [
            Component::Normal(namespace),
            Component::Normal(project),
            Component::Normal(environment),
            Component::Normal(slot),
        ] = components.as_slice()
        else {
            return Err(invalid("invalid slot coordination placement"));
        };
        if *namespace != "registry"
            || placement.state_root
                != placement
                    .state_base
                    .join("data")
                    .join(project)
                    .join(environment)
                    .join(slot)
        {
            return Err(invalid("incoherent slot placement"));
        }
        for (field, component) in [("projectId", project), ("environment", environment)] {
            let value = component
                .to_str()
                .ok_or_else(|| invalid("invalid slot identity encoding"))?;
            super::placement::normal_component(field, value)?;
        }
        let slot_text = slot
            .to_str()
            .ok_or_else(|| invalid("invalid slot component"))?;
        if slot_text
            .parse::<u32>()
            .ok()
            .is_none_or(|number| number.to_string() != slot_text)
        {
            return Err(invalid("invalid slot component"));
        }
        let mut directory =
            Directory::private_anchor(&placement.state_base).map_err(acquisition_error)?;
        let mut ancestors = Vec::new();
        for component in [namespace, project, environment, slot] {
            cancellation.check()?;
            let component =
                CString::new(component.as_bytes()).map_err(|_| invalid("NUL in slot placement"))?;
            let child = directory
                .create_private_child(&component)
                .map_err(acquisition_error)?;
            ancestors.push((directory, component));
            directory = child;
        }
        cancellation.check()?;
        let file = directory
            .open_private_file(c"slot.lock")
            .map_err(acquisition_error)?;
        loop {
            cancellation.check()?;
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
                break;
            }
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            if error.kind() == io::ErrorKind::WouldBlock {
                return Err(RuntimeError::new(
                    ErrorCode::CleanupRefused,
                    "slot is already owned by another session",
                ));
            }
            return Err(acquisition_error(error));
        }
        directory
            .verify_private_file(c"slot.lock", &file)
            .map_err(acquisition_error)?;
        cancellation.check()?;
        let guard = Self {
            file,
            directory,
            registry_path: placement.registry_path(),
            state_base: placement.state_base.clone(),
            ancestors,
        };
        guard.validate()?;
        Ok(guard)
    }

    pub(crate) fn check_identity(
        &self,
        project: &str,
        environment: &str,
        slot: i64,
    ) -> RuntimeResult<()> {
        let project = super::placement::normal_component("projectId", project)?;
        let environment = super::placement::normal_component("environment", environment)?;
        let slot = u32::try_from(slot).map_err(|_| invalid("invalid registry slot"))?;
        let expected = self
            .state_base
            .join("registry")
            .join(project)
            .join(environment)
            .join(slot.to_string())
            .join("registry.sqlite3");
        if expected != self.registry_path {
            return Err(invalid(
                "registry identity does not match held slot authority",
            ));
        }
        Ok(())
    }

    pub fn registry_path(&self) -> &std::path::Path {
        &self.registry_path
    }

    /// Recheck the rendezvous entry before mutation. Held identity does not
    /// protect against future interference by another writer with the same UID.
    pub fn validate(&self) -> RuntimeResult<()> {
        self.ancestors[0]
            .0
            .verify_anchor(&self.state_base)
            .map_err(acquisition_error)?;
        for (index, (parent, name)) in self.ancestors.iter().enumerate() {
            let child = self
                .ancestors
                .get(index + 1)
                .map(|(directory, _)| directory)
                .unwrap_or(&self.directory);
            parent
                .verify_child(name, child)
                .map_err(acquisition_error)?;
        }
        self.directory
            .verify_private_file(c"slot.lock", &self.file)
            .map_err(acquisition_error)
    }

    pub fn release(self) -> RuntimeResult<()> {
        self.file.close().map_err(|error| {
            RuntimeError::new(
                ErrorCode::StateUnwritable,
                format!("failed to close slot authority: {error}"),
            )
        })
    }
}

fn invalid(message: &'static str) -> RuntimeError {
    RuntimeError::new(ErrorCode::StateUnowned, message)
}
fn acquisition_error(error: io::Error) -> RuntimeError {
    RuntimeError::new(
        ErrorCode::StateUnowned,
        format!("invalid slot coordination object: {error}"),
    )
}

#[cfg(test)]
pub(crate) fn fixture_guard(
    root: &std::path::Path,
    identity: &crate::registry::RegistryIdentity,
) -> SlotGuard {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(root).unwrap();
    std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o700)).unwrap();
    let placement = super::placement::derive_slot_placement(
        &identity.project_id,
        &identity.environment,
        identity.slot.try_into().unwrap(),
        "test-run",
        root,
    )
    .unwrap();
    SlotGuard::acquire(&placement, &CancellationToken::new()).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    use std::sync::atomic::{AtomicU64, Ordering};

    // Isolate raw fork proofs from other guard fixtures in this harness.
    static LOCK_TEST: std::sync::Mutex<()> = std::sync::Mutex::new(());
    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Fixture {
        root: PathBuf,
        placement: HostPlacement,
    }
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "nixfied-slot-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
            let registry_dir = root.join("registry/project/dev/0");
            let run_dir = registry_dir.join("runs/session");
            let placement = HostPlacement {
                state_base: root.clone(),
                state_root: root.join("data/project/dev/0"),
                registry_dir,
                logs_dir: run_dir.join("logs"),
                artifacts_dir: run_dir.join("artifacts"),
                summary_path: run_dir.join("summary.json"),
                run_dir,
            };
            Self { root, placement }
        }
        fn acquire(&self) -> RuntimeResult<SlotGuard> {
            SlotGuard::acquire(&self.placement, &CancellationToken::new())
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn independent_opens_contend_and_release_preserves_the_lock_inode() {
        let _isolated = LOCK_TEST.lock().unwrap();
        let fixture = Fixture::new();
        let guard = fixture.acquire().unwrap();
        let path = fixture.placement.registry_dir.join("slot.lock");
        let inode = fs::metadata(&path).unwrap().ino();
        fs::write(&path, b"inert contents").unwrap();
        assert!(matches!(fixture.acquire(), Err(error) if error.code == ErrorCode::CleanupRefused));
        // Closing an unrelated open must not release an open-description flock.
        drop(fs::File::open(&path).unwrap());
        assert!(fixture.acquire().is_err());
        assert!(!fixture.placement.state_root.exists());
        assert!(!fixture.placement.registry_path().exists());
        assert!(!fixture.placement.run_dir.exists());
        guard.release().unwrap();
        let next = fixture.acquire().unwrap();
        assert_eq!(fs::metadata(&path).unwrap().ino(), inode);
        assert_eq!(fs::read(&path).unwrap(), b"inert contents");
        next.release().unwrap();
    }

    #[test]
    fn anchor_aliases_contend_and_unwind_closes_authority() {
        let _isolated = LOCK_TEST.lock().unwrap();
        let fixture = Fixture::new();
        let alias = fixture.root.with_extension("alias");
        std::os::unix::fs::symlink(&fixture.root, &alias).unwrap();
        let mut placement = fixture.placement.clone();
        placement.state_base = alias.clone();
        placement.registry_dir = alias.join("registry/project/dev/0");
        placement.state_root = alias.join("data/project/dev/0");
        let guard = fixture.acquire().unwrap();
        assert!(SlotGuard::acquire(&placement, &CancellationToken::new()).is_err());
        let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let _guard = guard;
            panic!("test unwind");
        }));
        assert!(unwind.is_err());
        SlotGuard::acquire(&placement, &CancellationToken::new())
            .unwrap()
            .release()
            .unwrap();
        fs::remove_file(alias).unwrap();
    }

    #[test]
    fn unsafe_lock_objects_and_managed_symlinks_are_rejected() {
        let _isolated = LOCK_TEST.lock().unwrap();
        for kind in ["symlink", "hardlink", "fifo", "mode", "directory"] {
            let fixture = Fixture::new();
            fixture.acquire().unwrap().release().unwrap();
            let lock = fixture.placement.registry_dir.join("slot.lock");
            fs::remove_file(&lock).unwrap();
            let other = fixture.root.join("other");
            fs::write(&other, b"preserve").unwrap();
            fs::set_permissions(&other, fs::Permissions::from_mode(0o600)).unwrap();
            match kind {
                "symlink" => std::os::unix::fs::symlink(&other, &lock).unwrap(),
                "hardlink" => fs::hard_link(&other, &lock).unwrap(),
                "fifo" => {
                    let name = CString::new(lock.as_os_str().as_bytes()).unwrap();
                    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
                }
                "mode" => {
                    fs::write(&lock, b"wrong mode").unwrap();
                    fs::set_permissions(&lock, fs::Permissions::from_mode(0o644)).unwrap();
                }
                "directory" => fs::create_dir(&lock).unwrap(),
                _ => unreachable!(),
            }
            assert!(fixture.acquire().is_err(), "{kind}");
            assert_eq!(fs::read(&other).unwrap(), b"preserve");
        }
        let fixture = Fixture::new();
        let outside = fixture.root.join("outside");
        fs::create_dir(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, fixture.root.join("registry")).unwrap();
        assert!(fixture.acquire().is_err());
        assert_eq!(fs::read_dir(outside).unwrap().count(), 0);
    }

    #[test]
    fn replacement_and_cancellation_fail_closed() {
        let _isolated = LOCK_TEST.lock().unwrap();
        let fixture = Fixture::new();
        let token = CancellationToken::new();
        token.cancel();
        assert!(SlotGuard::acquire(&fixture.placement, &token).is_err());
        assert!(!fixture.placement.registry_dir.exists());
        let guard = fixture.acquire().unwrap();
        let lock = fixture.placement.registry_dir.join("slot.lock");
        fs::rename(&lock, lock.with_extension("old")).unwrap();
        fs::write(&lock, b"replacement").unwrap();
        fs::set_permissions(&lock, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(guard.validate().is_err());
        guard.release().unwrap();
        assert_eq!(fs::read(&lock).unwrap(), b"replacement");
    }
    #[test]
    fn changed_ancestry_is_detected_and_missing_anchor_is_private() {
        let _isolated = LOCK_TEST.lock().unwrap();
        let fixture = Fixture::new();
        let guard = fixture.acquire().unwrap();
        let registry = fixture.root.join("registry");
        fs::rename(&registry, fixture.root.join("old-registry")).unwrap();
        fs::create_dir(&registry).unwrap();
        assert!(guard.validate().is_err());
        guard.release().unwrap();

        let fixture = Fixture::new();
        fs::remove_dir(&fixture.root).unwrap();
        let guard = fixture.acquire().unwrap();
        for path in [
            &fixture.root,
            &fixture.root.join("registry"),
            &fixture.placement.registry_dir,
        ] {
            assert_eq!(fs::metadata(path).unwrap().mode() & 0o777, 0o700);
        }
        assert_eq!(
            fs::metadata(fixture.placement.registry_dir.join("slot.lock"))
                .unwrap()
                .mode()
                & 0o777,
            0o600
        );
        guard.release().unwrap();
    }

    #[test]
    fn forked_child_closes_inherited_authority_without_unlocking_parent() {
        let _isolated = LOCK_TEST.lock().unwrap();
        let fixture = Fixture::new();
        let guard = fixture.acquire().unwrap();
        let inherited = guard.file.as_raw_fd();
        let lock = CString::new(
            fixture
                .placement
                .registry_dir
                .join("slot.lock")
                .as_os_str()
                .as_bytes(),
        )
        .unwrap();
        // Only async-signal-safe syscalls in the child; no Rust destructors or
        // test harness work runs after fork in this multithreaded process.
        let pid = unsafe { libc::fork() };
        assert!(pid >= 0);
        if pid == 0 {
            unsafe {
                libc::close(inherited);
                let fd = libc::open(
                    lock.as_ptr(),
                    libc::O_RDWR | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                );
                if fd < 0 {
                    libc::_exit(2);
                }
                let result = libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB);
                #[cfg(target_os = "linux")]
                let code = *libc::__errno_location();
                #[cfg(target_os = "macos")]
                let code = *libc::__error();
                libc::close(fd);
                libc::_exit(if result == -1 && code == libc::EWOULDBLOCK {
                    0
                } else {
                    3
                });
            }
        }
        let mut status = 0;
        assert_eq!(unsafe { libc::waitpid(pid, &mut status, 0) }, pid);
        assert!(libc::WIFEXITED(status));
        assert_eq!(libc::WEXITSTATUS(status), 0);
        assert!(fixture.acquire().is_err());
        guard.release().unwrap();
        fixture.acquire().unwrap().release().unwrap();
    }
}
