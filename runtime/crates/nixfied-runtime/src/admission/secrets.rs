use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use nixfied_model::{Model, SecretSourceKind};

use crate::error::{ErrorCode, RuntimeError, RuntimeResult};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ResolvedSecrets {
    values: BTreeMap<String, String>,
}

impl ResolvedSecrets {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn get(&self, id: &str) -> Option<&str> {
        self.values.get(id).map(String::as_str)
    }

    pub(crate) fn values(&self) -> impl Iterator<Item = &str> {
        self.values.values().map(String::as_str)
    }

    #[cfg(test)]
    pub(crate) fn from_values(values: BTreeMap<String, String>) -> Self {
        Self { values }
    }
}

pub fn check_secret_references(model: &Model) -> RuntimeResult<()> {
    check_secret_descriptors(model)?;
    let declared = model
        .secrets
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    for invocation in invocations(model) {
        for value in &invocation.run {
            if value.contains("${secret:") {
                return Err(RuntimeError::new(
                    ErrorCode::ModelAdmission,
                    "secret placeholders are only allowed in invocation.env values",
                ));
            }
        }
        for value in invocation.env.values() {
            if has_unclosed_secret_ref(value) {
                return Err(RuntimeError::new(
                    ErrorCode::ModelAdmission,
                    format!("malformed secret placeholder in invocation env value: {value}"),
                ));
            }
            for reference in secret_refs(value) {
                if reference.is_empty() {
                    return Err(RuntimeError::new(
                        ErrorCode::ModelAdmission,
                        "secret placeholder must name a declared secret",
                    ));
                }
                if !declared.contains(reference) {
                    return Err(RuntimeError::new(
                        ErrorCode::ModelAdmission,
                        format!("secret placeholder references undeclared secret {reference}"),
                    ));
                }
            }
        }
    }
    Ok(())
}

pub fn resolve_secrets(model: &Model) -> RuntimeResult<ResolvedSecrets> {
    check_secret_references(model)?;
    let mut base = None;
    let mut values = BTreeMap::new();
    for (id, descriptor) in &model.secrets {
        let value = match descriptor.source.kind {
            SecretSourceKind::EnvVar => {
                let env_var = descriptor
                    .source
                    .env_var
                    .as_deref()
                    .expect("descriptor validation requires envVar");
                read_env_secret(id, env_var)?
            }
            SecretSourceKind::File => {
                let base = match &base {
                    Some(base) => base,
                    None => base.insert(secrets_base()?),
                };
                let path = descriptor
                    .source
                    .path
                    .as_deref()
                    .expect("descriptor validation requires path");
                read_file_secret(id, base, path)?
            }
        };
        values.insert(id.clone(), value);
    }
    Ok(ResolvedSecrets { values })
}

fn check_secret_descriptors(model: &Model) -> RuntimeResult<()> {
    for (id, descriptor) in &model.secrets {
        if descriptor.secret_id != *id {
            return Err(RuntimeError::new(
                ErrorCode::ModelAdmission,
                format!(
                    "secret descriptor key {id} disagrees with secretId {}",
                    descriptor.secret_id
                ),
            ));
        }
        match descriptor.source.kind {
            SecretSourceKind::EnvVar => {
                let env_var = descriptor.source.env_var.as_deref().ok_or_else(|| {
                    RuntimeError::new(
                        ErrorCode::ModelAdmission,
                        format!("secret {id} env-var resolver requires envVar"),
                    )
                })?;
                if env_var.is_empty() || descriptor.source.path.is_some() {
                    return Err(RuntimeError::new(
                        ErrorCode::ModelAdmission,
                        format!("secret {id} env-var resolver must declare only envVar"),
                    ));
                }
            }
            SecretSourceKind::File => {
                let path = descriptor.source.path.as_deref().ok_or_else(|| {
                    RuntimeError::new(
                        ErrorCode::ModelAdmission,
                        format!("secret {id} file resolver requires path"),
                    )
                })?;
                if path.is_empty()
                    || Path::new(path).is_absolute()
                    || Path::new(path).components().any(disallowed_component)
                    || descriptor.source.env_var.is_some()
                {
                    return Err(RuntimeError::new(
                        ErrorCode::ModelAdmission,
                        format!(
                            "secret {id} file resolver must declare only a confined relative path"
                        ),
                    ));
                }
            }
        }
    }
    Ok(())
}

pub(crate) fn secret_refs(value: &str) -> Vec<&str> {
    refs_after_prefix("${secret:", value)
}

fn invocations(model: &Model) -> impl Iterator<Item = &nixfied_model::InvocationSpec> {
    let service_values = model.services.values().flat_map(|service| {
        let lifecycle = &service.lifecycle;
        std::iter::once(&lifecycle.start.invocation)
            .chain(lifecycle.ready.probe.invocation.as_ref())
            .chain(lifecycle.health.probe.invocation.as_ref())
    });
    let task_values = model
        .tasks
        .values()
        .filter_map(|task| task.invocation.as_ref());
    service_values.chain(task_values)
}

fn refs_after_prefix<'a>(prefix: &str, value: &'a str) -> Vec<&'a str> {
    let mut refs = Vec::new();
    let mut rest = value;
    while let Some(start) = rest.find(prefix) {
        rest = &rest[start + prefix.len()..];
        let Some(end) = rest.find('}') else { break };
        refs.push(&rest[..end]);
        rest = &rest[end..];
    }
    refs
}

pub(crate) fn has_unclosed_secret_ref(value: &str) -> bool {
    let mut rest = value;
    while let Some(start) = rest.find("${secret:") {
        rest = &rest[start + "${secret:".len()..];
        let Some(end) = rest.find('}') else {
            return true;
        };
        rest = &rest[end + 1..];
    }
    false
}

fn read_env_secret(id: &str, env_var: &str) -> RuntimeResult<String> {
    match std::env::var(env_var) {
        Ok(value) => normalize_secret_value(id, value),
        Err(error) => Err(secret_unavailable(format!(
            "secret {id} env var {env_var} is unavailable: {error}"
        ))),
    }
}

fn read_file_secret(id: &str, base: &Path, declared: &str) -> RuntimeResult<String> {
    let path = resolve_secret_file(base, declared)?;
    let value = std::fs::read_to_string(&path).map_err(|error| {
        secret_unavailable(format!(
            "secret {id} file {} is unavailable: {error}",
            path.display()
        ))
    })?;
    normalize_secret_value(id, value)
}

fn resolve_secret_file(base: &Path, declared: &str) -> RuntimeResult<PathBuf> {
    let relative = Path::new(declared);
    if declared.is_empty()
        || relative.is_absolute()
        || relative.components().any(disallowed_component)
    {
        return Err(secret_unavailable(format!(
            "secret file path must be confined under NIXFIED_SECRETS_DIR: {declared}"
        )));
    }
    let base = base.canonicalize().map_err(|error| {
        secret_unavailable(format!(
            "failed to canonicalize secrets dir {}: {error}",
            base.display()
        ))
    })?;
    let path = base.join(relative).canonicalize().map_err(|error| {
        secret_unavailable(format!("failed to resolve secret file {declared}: {error}"))
    })?;
    if !path.is_file() || !path.starts_with(&base) {
        return Err(secret_unavailable(format!(
            "secret file {} escaped secrets dir {}",
            path.display(),
            base.display()
        )));
    }
    Ok(path)
}

fn secrets_base() -> RuntimeResult<PathBuf> {
    if let Some(value) = std::env::var_os("NIXFIED_SECRETS_DIR")
        && !value.is_empty()
    {
        return Ok(PathBuf::from(value));
    }
    default_secrets_base()
}

fn default_secrets_base() -> RuntimeResult<PathBuf> {
    if let Some(value) = std::env::var_os("XDG_CONFIG_HOME")
        && !value.is_empty()
    {
        return Ok(PathBuf::from(value).join("nixfied/secrets"));
    }
    let home = std::env::var_os("HOME").ok_or_else(|| {
        RuntimeError::new(
            ErrorCode::SecretUnavailable,
            "HOME is not set and NIXFIED_SECRETS_DIR was not provided",
        )
    })?;
    let home = PathBuf::from(home);
    if cfg!(target_os = "macos") {
        Ok(home.join("Library/Application Support/nixfied/secrets"))
    } else {
        Ok(home.join(".config/nixfied/secrets"))
    }
}

fn normalize_secret_value(id: &str, value: String) -> RuntimeResult<String> {
    let normalized = value.trim_end_matches(['\r', '\n']).to_string();
    if normalized.is_empty() {
        return Err(secret_unavailable(format!(
            "secret {id} resolved to empty material"
        )));
    }
    Ok(normalized)
}

fn disallowed_component(component: Component<'_>) -> bool {
    matches!(
        component,
        Component::Prefix(_) | Component::RootDir | Component::ParentDir
    )
}

fn secret_unavailable(message: impl Into<String>) -> RuntimeError {
    RuntimeError::new(ErrorCode::SecretUnavailable, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_refs_extracts_placeholders() {
        assert_eq!(
            secret_refs("${secret:one}:${secret:two}"),
            vec!["one", "two"]
        );
        assert!(secret_refs("${secret:unterminated").is_empty());
    }

    #[test]
    fn normalizes_trailing_newlines_but_rejects_empty_material() {
        assert_eq!(
            normalize_secret_value("token", "value\n".to_string()).unwrap(),
            "value"
        );
        assert_eq!(
            normalize_secret_value("token", "\n\r\n".to_string())
                .unwrap_err()
                .code,
            ErrorCode::SecretUnavailable
        );
    }
}
