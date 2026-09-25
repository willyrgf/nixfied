//! The slot's application tree, opened only from the slot guard's held
//! state-base descriptor. Preparation and cleanup never resolve its path again,
//! never follow a symlink in its ancestry, and read or publish its marker only
//! through the opened root.
use std::ffi::{CStr, CString};
use std::io;
use std::path::{Path, PathBuf};

use crate::error::{ErrorCode, RuntimeError, RuntimeResult};
use crate::filesystem::{Directory, EntryKind, FileIdentity};
use crate::state::marker::{StateIdentity, StateMarker};
use crate::state::ownership::{SlotGuard, refuse_network_filesystem};

pub(crate) const MARKER: &CStr = c".nixfied-state.json";
const MARKER_LIMIT: usize = 64 * 1024;

/// The application root derived from placement, never a caller-supplied path.
pub(crate) struct ApplicationTree {
    base: Directory,
    ancestry: [CString; 3],
    pub(crate) name: CString,
    pub(crate) relative: String,
    pub(crate) path: PathBuf,
}

pub(crate) enum Observed {
    Absent,
    Present {
        parent: Directory,
        root: Directory,
        identity: FileIdentity,
    },
}

impl ApplicationTree {
    /// The identity must name the slot the guard holds: a tree is never
    /// derived for another owner's placement.
    pub(crate) fn new(guard: &SlotGuard, identity: &StateIdentity) -> RuntimeResult<Self> {
        guard.check_identity(
            &identity.project_id,
            &identity.environment,
            i64::from(identity.slot),
        )?;
        let slot = guard.slot();
        let relative = Path::new("data").join(slot.relative());
        let component = |value: &str| {
            CString::new(value).map_err(|_| unowned("placement component contains NUL"))
        };
        Ok(Self {
            path: guard.state_base().join(&relative),
            base: guard.state_base_directory()?,
            ancestry: [
                c"data".to_owned(),
                component(slot.project())?,
                component(slot.environment())?,
            ],
            name: component(&slot.slot().to_string())?,
            relative: relative.to_string_lossy().into_owned(),
        })
    }

    pub(crate) fn open_parent(&self) -> RuntimeResult<Option<Directory>> {
        let mut directory = self
            .base
            .try_clone()
            .map_err(|error| self.io_error(error))?;
        for name in &self.ancestry {
            match directory
                .entry(name)
                .map_err(|error| self.io_error(error))?
            {
                None => return Ok(None),
                Some(EntryKind::Directory(_)) => {
                    directory = directory
                        .open_owned_child(name)
                        .map_err(|error| self.io_error(error))?;
                }
                Some(_) => return Err(unowned("application root ancestry is not a directory")),
            }
        }
        Ok(Some(directory))
    }

    pub(crate) fn open(&self) -> RuntimeResult<Observed> {
        let Some(parent) = self.open_parent()? else {
            return Ok(Observed::Absent);
        };
        match parent
            .entry(&self.name)
            .map_err(|error| self.io_error(error))?
        {
            None => Ok(Observed::Absent),
            Some(EntryKind::Directory(_)) => {
                let root = parent
                    .open_owned_child(&self.name)
                    .map_err(|error| self.io_error(error))?;
                refuse_network_filesystem(&root)?;
                let identity = root.identity().map_err(|error| self.io_error(error))?;
                Ok(Observed::Present {
                    parent,
                    root,
                    identity,
                })
            }
            Some(_) => Err(unowned(
                "application root is not a directory; refusing to follow or use it",
            )),
        }
    }

    /// Create each missing component as a private directory and return the
    /// opened root. Existing components are opened without following symlinks.
    pub(crate) fn materialize(&self) -> RuntimeResult<Directory> {
        let mut directory = self
            .base
            .try_clone()
            .map_err(|error| self.write_error(error))?;
        for name in self.ancestry.iter().chain(std::iter::once(&self.name)) {
            directory = match directory
                .entry(name)
                .map_err(|error| self.write_error(error))?
            {
                None => directory.create_private_child(name),
                Some(EntryKind::Directory(_)) => directory.open_owned_child(name),
                Some(_) => {
                    return Err(RuntimeError::new(
                        ErrorCode::StateUnwritable,
                        format!(
                            "application root {} traverses a non-directory entry",
                            self.path.display()
                        ),
                    ));
                }
            }
            .map_err(|error| self.write_error(error))?;
        }
        refuse_network_filesystem(&directory)?;
        Ok(directory)
    }

    /// Replace the root's marker atomically and durably.
    pub(crate) fn publish_marker(
        &self,
        root: &Directory,
        marker: &StateMarker,
    ) -> RuntimeResult<()> {
        let bytes = serde_json::to_vec_pretty(marker).map_err(|error| {
            RuntimeError::new(
                ErrorCode::StateUnwritable,
                format!("failed to serialize state marker: {error}"),
            )
        })?;
        root.publish_file(MARKER, &bytes).map_err(|error| {
            RuntimeError::new(
                ErrorCode::StateUnwritable,
                format!(
                    "failed to publish state marker in {}: {error}",
                    self.path.display()
                ),
            )
        })
    }

    pub(crate) fn sync_parent(&self) -> RuntimeResult<()> {
        if let Some(parent) = self.open_parent()? {
            parent.sync().map_err(|error| self.io_error(error))?;
        }
        Ok(())
    }

    pub(crate) fn io_error(&self, error: io::Error) -> RuntimeError {
        RuntimeError::new(
            ErrorCode::StateUnowned,
            format!(
                "failed to inspect application root {}: {error}",
                self.path.display()
            ),
        )
    }

    fn write_error(&self, error: io::Error) -> RuntimeError {
        RuntimeError::new(
            ErrorCode::StateUnwritable,
            format!(
                "failed to materialize application root {}: {error}",
                self.path.display()
            ),
        )
    }
}

pub(crate) fn read_root_marker(root: &Directory) -> RuntimeResult<Option<StateMarker>> {
    match root.entry(MARKER) {
        Ok(None) => return Ok(None),
        Ok(Some(EntryKind::File(_))) => {}
        Ok(Some(_)) => return Err(unowned("state marker is not a regular file")),
        Err(error) => return Err(marker_error(error)),
    }
    let bytes = root
        .read_regular_file(MARKER, MARKER_LIMIT)
        .map_err(marker_error)?;
    serde_json::from_slice(&bytes).map(Some).map_err(|error| {
        RuntimeError::new(
            ErrorCode::StateUnowned,
            format!("state marker is invalid: {error}"),
        )
    })
}

fn marker_error(error: io::Error) -> RuntimeError {
    RuntimeError::new(
        ErrorCode::StateUnowned,
        format!("state marker is unreadable: {error}"),
    )
}

pub(crate) fn unowned(message: &str) -> RuntimeError {
    RuntimeError::new(ErrorCode::StateUnowned, message.to_owned())
}

#[cfg(test)]
mod tests {
    #[test]
    fn marker_name_matches_the_published_marker() {
        assert_eq!(
            super::MARKER.to_bytes(),
            crate::state::MARKER_FILE_NAME.as_bytes()
        );
    }
}
