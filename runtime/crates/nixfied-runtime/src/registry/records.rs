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

    pub fn default_slot(
        project_id: impl Into<String>,
        runtime_abi: impl Into<String>,
        toolchain_id: impl Into<String>,
    ) -> Self {
        Self::for_slot(project_id, "dev", 0, runtime_abi, toolchain_id)
    }
}

/// Stored spelling is retained for exact row comparisons and event payloads;
/// host and endpoint_id are decoded once before consumers can act on the row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StoredEndpoint {
    pub(crate) endpoint_key: String,
    pub(crate) service_instance_id: String,
    pub(crate) endpoint_id: String,
    pub(crate) address: String,
    pub(crate) host: nixfied_manifest::LoopbackHost,
    pub(crate) port: u16,
    pub(crate) status: super::status::PortStatus,
    pub(crate) owner_process_key: String,
}

pub(crate) fn read_open_endpoints(
    connection: &rusqlite::Connection,
    service_instance_id: Option<&str>,
) -> crate::RuntimeResult<Vec<StoredEndpoint>> {
    use super::status::{self, DbStatus, PortStatus};
    use crate::{ErrorCode, RuntimeError};
    let sql_error =
        |error: rusqlite::Error| RuntimeError::new(ErrorCode::RegistryCorrupt, error.to_string());
    let mut statement = connection
        .prepare(&format!(
            "SELECT ep.endpoint_key, ep.address, ep.port, ep.status, ep.owner_process_key, ep.service_instance_id,
                EXISTS (SELECT 1 FROM processes p WHERE p.process_key = ep.owner_process_key
                    AND p.service_instance_id = ep.service_instance_id
                    AND p.environment = ep.environment AND p.slot = ep.slot)
         FROM ports ep WHERE (?1 IS NULL OR ep.service_instance_id = ?1) AND ep.status IN ({})
         ORDER BY ep.endpoint_key",
            status::sql_in_list(status::PORT_OPEN)
        ))
        .map_err(sql_error)?;
    let rows = statement
        .query_map([service_instance_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, u16>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, bool>(6)?,
            ))
        })
        .map_err(sql_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sql_error)?;
    rows.into_iter().map(|(endpoint_key, address, port, status, owner_process_key, service_instance_id, has_owner)| {
        let status = PortStatus::parse_db(&status)?;
        if !has_owner {
            return Err(RuntimeError::new(ErrorCode::RegistryCorrupt,
                format!("endpoint {endpoint_key} has no matching process owner in its slot")));
        }
        if owner_process_key.is_empty() {
            return Err(RuntimeError::new(ErrorCode::RegistryCorrupt, "endpoint has no owning process"));
        }
        let prefix = format!("{service_instance_id}:");
        let endpoint_id = endpoint_key.strip_prefix(&prefix).ok_or_else(|| RuntimeError::new(
            ErrorCode::RegistryCorrupt,
            format!("endpoint key {endpoint_key} does not belong to service {service_instance_id}")
        ))?;
        if endpoint_id.is_empty() {
            return Err(RuntimeError::new(ErrorCode::RegistryCorrupt,
                format!("endpoint key {endpoint_key} has an empty endpoint identity")));
        }
        let endpoint_id = endpoint_id.to_owned();
        let host = nixfied_manifest::LoopbackHost::parse(&address)
            .map_err(|message| RuntimeError::new(ErrorCode::RegistryCorrupt, message))?;
        Ok(StoredEndpoint { endpoint_key, service_instance_id, endpoint_id, address, host, port, status, owner_process_key })
    }).collect()
}
