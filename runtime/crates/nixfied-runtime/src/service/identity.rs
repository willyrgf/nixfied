use std::collections::BTreeMap;

use nixfied_model::{
    CleanupPolicy, ContainmentRequirement, Endpoint, ExecSpec, Lifecycle, PersistencePolicy,
    ServiceId, ServiceSpec, StatePolicy, Target, UniqueVec,
};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::execution::ServiceIdentity;

/// Compute a service's reuse identity from its actual lowered contract, replacing
/// the hashes the model used to carry. The components mirror the wire contract a
/// service reuse must be sensitive to — endpoint, state policy, behavioral runtime
/// contract, and build target — but are derived here, so the `service_instance_id`
/// registry key is a pure function of what the runtime executes. The values are
/// only ever fed into `service_instance_id`; nothing compares them across the
/// Nix/runtime boundary, so the runtime owns the scheme outright.
pub fn compute_service_identity(
    service: &ServiceSpec,
    execs: &BTreeMap<String, ExecSpec>,
    state: &StatePolicy,
    target: &Target,
) -> ServiceIdentity {
    // The execs the lifecycle actually invokes (prepare/start); a wiring or exec
    // change must move the identity, so they are part of the runtime contract.
    let lifecycle_exec_ids: Vec<&str> = [
        service.lifecycle.prepare.exec_id.as_ref(),
        Some(&service.lifecycle.start.exec_id),
    ]
    .into_iter()
    .flatten()
    .map(|id| id.as_str())
    .collect();
    let lifecycle_execs: BTreeMap<&str, &ExecSpec> = execs
        .iter()
        .filter(|(id, _)| lifecycle_exec_ids.contains(&id.as_str()))
        .map(|(id, exec)| (id.as_str(), exec))
        .collect();

    ServiceIdentity {
        endpoint_identity_hash: hash_json(
            "endpoint-identity",
            &EndpointIdentityInputs {
                endpoints: &service.endpoints,
                primary_endpoint: &service.primary_endpoint,
            },
        ),
        state_identity_hash: hash_json(
            "state-identity",
            &StateIdentityInputs {
                state_epoch: &state.state_epoch,
                cleanup_policy: &state.cleanup_policy,
                persistence: &state.persistence,
            },
        ),
        runtime_compatibility_hash: hash_json(
            "runtime-compatibility",
            &RuntimeIdentityInputs {
                lifecycle: &service.lifecycle,
                endpoints: &service.endpoints,
                primary_endpoint: &service.primary_endpoint,
                containment: &service.containment,
                connects_to: &service.connects_to,
                execs: lifecycle_execs,
            },
        ),
        target_identity_hash: hash_json("target-identity", target),
    }
}

/// The endpoint inputs a reuse identity depends on: the full endpoint set and
/// which one is primary, so adding, removing, or re-pointing an endpoint moves
/// the identity.
#[derive(Serialize)]
struct EndpointIdentityInputs<'a> {
    endpoints: &'a BTreeMap<String, Endpoint>,
    primary_endpoint: &'a str,
}

/// The state inputs a reuse identity depends on: the epoch and the cleanup /
/// persistence policies. The marker identity is deliberately excluded — it is a
/// separate ownership concern, not part of the service contract.
#[derive(Serialize)]
struct StateIdentityInputs<'a> {
    state_epoch: &'a str,
    cleanup_policy: &'a CleanupPolicy,
    persistence: &'a PersistencePolicy,
}

/// The behavioral contract a reuse identity depends on: the lifecycle, endpoints,
/// containment, wiring, and the execs the lifecycle invokes.
#[derive(Serialize)]
struct RuntimeIdentityInputs<'a> {
    lifecycle: &'a Lifecycle,
    endpoints: &'a BTreeMap<String, Endpoint>,
    primary_endpoint: &'a str,
    containment: &'a ContainmentRequirement,
    connects_to: &'a UniqueVec<ServiceId>,
    execs: BTreeMap<&'a str, &'a ExecSpec>,
}

/// Hash a serializable identity component under a domain tag. Serialization of
/// these admitted-model structs cannot fail; an empty string on the impossible
/// error path still yields a deterministic digest.
fn hash_json<T: Serialize>(tag: &str, value: &T) -> String {
    let json = serde_json::to_string(value).unwrap_or_default();
    hash_fields(&[tag, &json])
}

pub fn service_address_hash(
    project_id: &str,
    environment: &str,
    slot: u32,
    service_name: &str,
) -> String {
    let slot = slot.to_string();
    hash_fields(&[
        "service-address",
        project_id,
        environment,
        &slot,
        service_name,
    ])
}

pub fn service_instance_id(service_address_hash: &str, identity: &ServiceIdentity) -> String {
    hash_fields(&[
        "service-instance",
        service_address_hash,
        &identity.endpoint_identity_hash,
        &identity.state_identity_hash,
        &identity.runtime_compatibility_hash,
        &identity.target_identity_hash,
    ])
}

fn hash_fields(fields: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for field in fields {
        hasher.update((field.len() as u64).to_be_bytes());
        hasher.update(field.as_bytes());
    }
    hex::encode(hasher.finalize())
}
