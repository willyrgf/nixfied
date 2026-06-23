use std::path::PathBuf;

use nixfied_model::validation::validate_cache_component;
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::error::{ErrorCode, RuntimeError, RuntimeResult};
use crate::execution::{CacheMode, CacheScope, ResolvedCacheEnv};
use crate::state::HostPlacement;
use crate::state::placement::{canonicalize_existing, materialize_owned_dir};

const CACHE_SCHEMA: &str = "nixfied-cache-env-v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheIdentity<'a> {
    pub target_json: &'a str,
    pub runtime_abi: &'a str,
    pub toolchain_id: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaterializedCacheEnv {
    pub env_var: String,
    pub family: String,
    pub mode: CacheMode,
    pub scope: CacheScope,
    pub digest: String,
    pub path: PathBuf,
}

pub fn materialize_cache_env(
    placement: &HostPlacement,
    binding: &ResolvedCacheEnv,
    identity: &CacheIdentity<'_>,
) -> RuntimeResult<MaterializedCacheEnv> {
    validate_cache_component("invocation.cacheEnv.family", &binding.family).map_err(|error| {
        RuntimeError::new(
            ErrorCode::ModelAdmission,
            format!("invalid cache family {}: {error}", binding.family),
        )
    })?;
    let digest = cache_digest(binding, identity)?;
    let owner_root = match binding.scope {
        CacheScope::Run => &placement.run_dir,
        CacheScope::Slot => &placement.state_root,
    };
    let cache_family_root = owner_root.join("caches").join(&binding.family);
    let canonical_owner_root = canonicalize_existing("cache owner root", owner_root)?;
    materialize_owned_dir(owner_root, &canonical_owner_root, &cache_family_root)?;
    let canonical_cache_family_root =
        canonicalize_existing("cache family root", &cache_family_root)?;
    let path = cache_family_root.join(&digest);
    materialize_owned_dir(&cache_family_root, &canonical_cache_family_root, &path)?;

    Ok(MaterializedCacheEnv {
        env_var: binding.env_var.clone(),
        family: binding.family.clone(),
        mode: binding.mode,
        scope: binding.scope,
        digest,
        path,
    })
}

fn cache_digest(binding: &ResolvedCacheEnv, identity: &CacheIdentity<'_>) -> RuntimeResult<String> {
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct DigestInput<'a> {
        schema: &'static str,
        family: &'a str,
        mode: CacheMode,
        scope: CacheScope,
        key_parts: &'a [String],
        target: &'a str,
        runtime_abi: &'a str,
        toolchain_id: &'a str,
    }

    let input = DigestInput {
        schema: CACHE_SCHEMA,
        family: &binding.family,
        mode: binding.mode,
        scope: binding.scope,
        key_parts: &binding.key_parts,
        target: identity.target_json,
        runtime_abi: identity.runtime_abi,
        toolchain_id: identity.toolchain_id,
    };
    let bytes = serde_json::to_vec(&input)
        .map_err(|error| RuntimeError::new(ErrorCode::ModelAdmission, error.to_string()))?;
    Ok(hex::encode(Sha256::digest(&bytes)))
}
