use std::os::unix::ffi::OsStrExt;
use std::path::{Component, Path, PathBuf};

use crate::error::{ErrorCode, RuntimeError, RuntimeResult};

/// One placed slot. Its components are validated once; every placement path
/// and the slot guard's authority derive from it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotIdentity {
    project: String,
    environment: String,
    slot: u32,
}

impl SlotIdentity {
    pub fn new(project: &str, environment: &str, slot: u32) -> RuntimeResult<Self> {
        normal_component("projectId", project)?;
        normal_component("environment", environment)?;
        Ok(Self {
            project: project.to_owned(),
            environment: environment.to_owned(),
            slot,
        })
    }

    pub fn project(&self) -> &str {
        &self.project
    }

    pub fn environment(&self) -> &str {
        &self.environment
    }

    pub fn slot(&self) -> u32 {
        self.slot
    }

    /// Whether a registry or state identity names this slot.
    pub(crate) fn names(&self, project: &str, environment: &str, slot: i64) -> bool {
        self.project == project && self.environment == environment && i64::from(self.slot) == slot
    }

    /// The `project/environment/slot` path that both disjoint roots nest.
    pub(crate) fn relative(&self) -> PathBuf {
        Path::new(&self.project)
            .join(&self.environment)
            .join(self.slot.to_string())
    }
}

/// Absolute lexical coordinates of one slot. Derivation neither creates state
/// nor claims filesystem identity; mutation still requires the slot guard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotLocation {
    state_base: PathBuf,
    slot: SlotIdentity,
}

impl SlotLocation {
    pub fn new(
        slot: SlotIdentity,
        state_base: impl AsRef<Path>,
        invocation_cwd: &Path,
    ) -> RuntimeResult<Self> {
        let state_base = state_base.as_ref();
        if state_base.as_os_str().is_empty() || state_base.as_os_str().as_bytes().contains(&0) {
            return Err(RuntimeError::new(
                ErrorCode::StateUnwritable,
                "state base must be nonempty and NUL-free",
            ));
        }
        let state_base = if state_base.is_absolute() {
            state_base.to_path_buf()
        } else {
            if !invocation_cwd.is_absolute() {
                return Err(RuntimeError::new(
                    ErrorCode::StateUnwritable,
                    "invocation cwd must be absolute",
                ));
            }
            invocation_cwd.join(state_base)
        };
        Self::from_absolute(slot, state_base)
    }

    fn from_absolute(slot: SlotIdentity, state_base: PathBuf) -> RuntimeResult<Self> {
        if !state_base.is_absolute() || state_base.as_os_str().as_bytes().contains(&0) {
            return Err(RuntimeError::new(
                ErrorCode::StateUnwritable,
                "state placement must be absolute and NUL-free",
            ));
        }
        Ok(Self { state_base, slot })
    }

    pub fn state_base(&self) -> &Path {
        &self.state_base
    }

    pub fn slot(&self) -> &SlotIdentity {
        &self.slot
    }

    /// Application data and coordination/evidence have structurally disjoint roots.
    pub fn state_root(&self) -> PathBuf {
        self.state_base.join("data").join(self.slot.relative())
    }

    /// Cleanup evidence survives deletion of the parallel slot state tree.
    pub fn registry_dir(&self) -> PathBuf {
        self.state_base.join("registry").join(self.slot.relative())
    }

    pub fn registry_path(&self) -> PathBuf {
        self.registry_dir().join("registry.sqlite3")
    }

    pub fn for_run(&self, run_id: &str) -> RuntimeResult<HostPlacement> {
        normal_component("runId", run_id)?;
        Ok(HostPlacement {
            location: self.clone(),
            run_id: run_id.to_owned(),
        })
    }
}

/// One run's evidence derives only from its slot location and run component.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostPlacement {
    location: SlotLocation,
    run_id: String,
}

impl HostPlacement {
    pub fn location(&self) -> &SlotLocation {
        &self.location
    }

    pub fn state_base(&self) -> &Path {
        self.location.state_base()
    }

    pub fn slot(&self) -> &SlotIdentity {
        self.location.slot()
    }

    pub fn state_root(&self) -> PathBuf {
        self.location.state_root()
    }

    pub fn registry_dir(&self) -> PathBuf {
        self.location.registry_dir()
    }

    pub fn registry_path(&self) -> PathBuf {
        self.location.registry_path()
    }

    pub fn run_id(&self) -> &str {
        &self.run_id
    }

    pub fn run_dir(&self) -> PathBuf {
        self.registry_dir().join("runs").join(&self.run_id)
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.run_dir().join("logs")
    }

    pub fn artifacts_dir(&self) -> PathBuf {
        self.run_dir().join("artifacts")
    }

    pub fn summary_path(&self) -> PathBuf {
        self.run_dir().join("summary.json")
    }
}

pub fn state_base_from_env() -> RuntimeResult<PathBuf> {
    if let Some(value) = std::env::var_os("NIXFIED_STATE_DIR")
        && !value.is_empty()
    {
        return Ok(PathBuf::from(value));
    }
    if let Some(value) = std::env::var_os("XDG_STATE_HOME")
        && !value.is_empty()
    {
        return Ok(PathBuf::from(value).join("nixfied"));
    }
    let home = std::env::var_os("HOME").ok_or_else(|| {
        RuntimeError::new(
            ErrorCode::StateUnwritable,
            "HOME is not set and NIXFIED_STATE_DIR was not provided",
        )
    })?;
    let home = PathBuf::from(home);
    if cfg!(target_os = "macos") {
        Ok(home.join("Library/Application Support/nixfied"))
    } else {
        Ok(home.join(".local/state/nixfied"))
    }
}

/// Native slot placement shared by admitted execution and registry ownership.
pub fn derive_slot_placement(
    project: &str,
    environment: &str,
    slot: u32,
    run_id: &str,
    state_base: impl AsRef<Path>,
) -> RuntimeResult<HostPlacement> {
    derive_slot_location(project, environment, slot, state_base)?.for_run(run_id)
}

pub fn derive_slot_location(
    project: &str,
    environment: &str,
    slot: u32,
    state_base: impl AsRef<Path>,
) -> RuntimeResult<SlotLocation> {
    let slot = SlotIdentity::new(project, environment, slot)?;
    if state_base.as_ref().is_absolute() {
        return SlotLocation::from_absolute(slot, state_base.as_ref().to_path_buf());
    }
    let cwd = std::env::current_dir().map_err(|_| {
        RuntimeError::new(
            ErrorCode::StateUnwritable,
            "cannot resolve placement invocation cwd",
        )
    })?;
    SlotLocation::new(slot, state_base, &cwd)
}

fn normal_component(field: &str, value: &str) -> RuntimeResult<()> {
    let path = Path::new(value);
    let mut components = path.components();
    let normal = matches!(components.next(), Some(Component::Normal(part)) if part == value)
        && components.next().is_none();
    if value.is_empty() || value.contains(['/', '\0']) || value.contains("${") || !normal {
        return Err(RuntimeError::new(
            ErrorCode::StateUnwritable,
            format!("{field} must be a single normal path component without template syntax"),
        ));
    }
    Ok(())
}
