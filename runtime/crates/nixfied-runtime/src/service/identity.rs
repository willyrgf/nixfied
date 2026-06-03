use nixfied_model::{Model, ServiceIdentity};
use sha2::{Digest, Sha256};

pub fn service_address_hash(model: &Model, service_name: &str) -> String {
    hash_fields(&[
        "service-address",
        &model.project.project_id,
        "dev",
        "0",
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
