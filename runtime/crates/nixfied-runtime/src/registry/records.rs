#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryIdentity {
    pub project_id: String,
    pub environment: String,
    pub slot: i64,
    pub runtime_abi: String,
    pub toolchain_id: String,
}

impl RegistryIdentity {
    pub fn for_slot(
        project_id: impl Into<String>,
        environment: impl Into<String>,
        slot: u32,
        runtime_abi: impl Into<String>,
        toolchain_id: impl Into<String>,
    ) -> Self {
        Self {
            project_id: project_id.into(),
            environment: environment.into(),
            slot: i64::from(slot),
            runtime_abi: runtime_abi.into(),
            toolchain_id: toolchain_id.into(),
        }
    }

    pub fn m0(
        project_id: impl Into<String>,
        runtime_abi: impl Into<String>,
        toolchain_id: impl Into<String>,
    ) -> Self {
        Self::for_slot(project_id, "dev", 0, runtime_abi, toolchain_id)
    }
}
