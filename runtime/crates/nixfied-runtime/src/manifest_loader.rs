use std::path::{Path, PathBuf};

use nixfied_manifest::{Manifest, Validate};
use sha2::{Digest, Sha256};

use crate::error::{ErrorCode, RuntimeError, RuntimeResult};

#[derive(Debug, Clone)]
pub struct LoadedManifest {
    pub path: PathBuf,
    pub raw_len: usize,
    pub computed_manifest_hash: String,
    pub manifest: Manifest,
}

#[derive(Debug, Clone)]
pub struct RawManifest {
    pub path: PathBuf,
    pub raw: Vec<u8>,
    pub computed_manifest_hash: String,
}

impl RawManifest {
    pub fn raw_len(&self) -> usize {
        self.raw.len()
    }
}

pub fn read_raw_manifest(path: impl AsRef<Path>) -> RuntimeResult<RawManifest> {
    let path = path.as_ref().to_path_buf();
    let raw = std::fs::read(&path).map_err(|error| {
        RuntimeError::new(
            ErrorCode::ManifestInvalid,
            format!("failed to read manifest bytes: {error}"),
        )
    })?;
    let computed_manifest_hash = hex::encode(Sha256::digest(&raw));

    Ok(RawManifest {
        path,
        raw,
        computed_manifest_hash,
    })
}

pub fn parse_loaded_manifest(raw_manifest: RawManifest) -> RuntimeResult<LoadedManifest> {
    let manifest: Manifest = serde_json::from_slice(&raw_manifest.raw).map_err(|error| {
        RuntimeError::new(
            ErrorCode::ManifestInvalid,
            format!("failed to parse manifest JSON: {error}"),
        )
        .with_manifest(&raw_manifest.path, &raw_manifest.computed_manifest_hash)
    })?;
    manifest.validate().map_err(|error| {
        let message = format!("manifest contract validation failed: {error}");
        let runtime_error = match &error {
            nixfied_manifest::ValidationError::RuntimeAbi { .. }
            | nixfied_manifest::ValidationError::ToolchainId { .. }
            | nixfied_manifest::ValidationError::ManifestVersion { .. } => {
                RuntimeError::new(ErrorCode::RuntimeAbiMismatch, message)
            }
            _ => RuntimeError::new(ErrorCode::ManifestInvalid, message),
        };
        runtime_error.with_manifest(&raw_manifest.path, &raw_manifest.computed_manifest_hash)
    })?;

    let raw_len = raw_manifest.raw_len();
    Ok(LoadedManifest {
        path: raw_manifest.path,
        raw_len,
        computed_manifest_hash: raw_manifest.computed_manifest_hash,
        manifest,
    })
}

pub fn load_manifest(path: impl AsRef<Path>) -> RuntimeResult<LoadedManifest> {
    parse_loaded_manifest(read_raw_manifest(path)?)
}
