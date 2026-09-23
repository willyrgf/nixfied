use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use nixfied_manifest::{Manifest, SecretSourceKind};

use crate::error::{ErrorCode, RuntimeError, RuntimeResult};
use crate::template::{Kind, Reference, Token, tokenize};

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

pub fn check_secret_references(manifest: &Manifest) -> RuntimeResult<()> {
    checked_secret_sources(manifest)?;
    check_references(manifest)
}

fn check_references(manifest: &Manifest) -> RuntimeResult<()> {
    for invocation in crate::execution::invocations(manifest) {
        for value in &invocation.run {
            if tokenize(value).iter().any(|token| {
                matches!(
                    token,
                    Token::Reference(Reference::Secret(_))
                        | Token::Malformed {
                            kind: Kind::Secret,
                            ..
                        }
                )
            }) {
                return Err(RuntimeError::new(
                    ErrorCode::ManifestAdmission,
                    "secret placeholders are only allowed in invocation.env values",
                ));
            }
        }
        for value in invocation.env.values() {
            let tokens = tokenize(value);
            if let Some(empty) = tokens.iter().find_map(|token| match token {
                Token::Malformed {
                    kind: Kind::Secret,
                    empty,
                } => Some(*empty),
                _ => None,
            }) {
                return Err(RuntimeError::new(
                    ErrorCode::ManifestAdmission,
                    if empty {
                        "secret placeholder must name a declared secret".to_string()
                    } else {
                        format!("malformed secret placeholder in invocation env value: {value}")
                    },
                ));
            }
            for token in tokens {
                if let Token::Reference(Reference::Secret(reference)) = token
                    && !manifest.secrets.contains_key(reference)
                {
                    return Err(RuntimeError::new(
                        ErrorCode::ManifestAdmission,
                        format!("secret placeholder references undeclared secret {reference}"),
                    ));
                }
            }
        }
    }
    Ok(())
}

pub fn resolve_secrets(manifest: &Manifest) -> RuntimeResult<ResolvedSecrets> {
    let sources = checked_secret_sources(manifest)?;
    check_references(manifest)?;
    let mut base = None;
    let mut values = BTreeMap::new();
    for (id, source) in sources {
        let value = match source {
            SecretSource::EnvVar(env_var) => read_env_secret(id, env_var)?,
            SecretSource::File(path) => {
                let base = match &base {
                    Some(base) => base,
                    None => base.insert(secrets_base()?),
                };
                read_file_secret(id, base, path)?
            }
        };
        values.insert(id.to_owned(), value);
    }
    Ok(ResolvedSecrets { values })
}

enum SecretSource<'a> {
    EnvVar(&'a str),
    File(&'a str),
}

fn checked_secret_sources(manifest: &Manifest) -> RuntimeResult<Vec<(&str, SecretSource<'_>)>> {
    let mut sources = Vec::with_capacity(manifest.secrets.len());
    for (id, descriptor) in &manifest.secrets {
        if descriptor.secret_id != *id {
            return Err(RuntimeError::new(
                ErrorCode::ManifestAdmission,
                format!(
                    "secret descriptor key {id} disagrees with secretId {}",
                    descriptor.secret_id
                ),
            ));
        }
        let source = match descriptor.source.kind {
            SecretSourceKind::EnvVar => {
                let env_var = descriptor.source.env_var.as_deref().ok_or_else(|| {
                    RuntimeError::new(
                        ErrorCode::ManifestAdmission,
                        format!("secret {id} env-var resolver requires envVar"),
                    )
                })?;
                if env_var.is_empty() || descriptor.source.path.is_some() {
                    return Err(RuntimeError::new(
                        ErrorCode::ManifestAdmission,
                        format!("secret {id} env-var resolver must declare only envVar"),
                    ));
                }
                SecretSource::EnvVar(env_var)
            }
            SecretSourceKind::File => {
                let path = descriptor.source.path.as_deref().ok_or_else(|| {
                    RuntimeError::new(
                        ErrorCode::ManifestAdmission,
                        format!("secret {id} file resolver requires path"),
                    )
                })?;
                if path.is_empty()
                    || Path::new(path).is_absolute()
                    || Path::new(path).components().any(disallowed_component)
                    || descriptor.source.env_var.is_some()
                {
                    return Err(RuntimeError::new(
                        ErrorCode::ManifestAdmission,
                        format!(
                            "secret {id} file resolver must declare only a confined relative path"
                        ),
                    ));
                }
                SecretSource::File(path)
            }
        };
        sources.push((id.as_str(), source));
    }
    Ok(sources)
}

fn read_env_secret(id: &str, env_var: &str) -> RuntimeResult<String> {
    match std::env::var(env_var) {
        Ok(value) => normalize_secret_value(id, value),
        Err(std::env::VarError::NotUnicode(_)) => Err(secret_unavailable(format!(
            "secret {id} env var {env_var} is unavailable: value is not valid UTF-8"
        ))),
        Err(error @ std::env::VarError::NotPresent) => Err(secret_unavailable(format!(
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
    fn malformed_secret_sources_reject_before_references_or_value_reads() {
        use nixfied_manifest::fixtures::{SyntheticManifestOptions, synthetic_manifest};
        use serde_json::json;
        for (source, diagnostic) in [
            (
                json!({"kind":"env-var"}),
                "secret bad env-var resolver requires envVar",
            ),
            (
                json!({"kind":"env-var","envVar":""}),
                "secret bad env-var resolver must declare only envVar",
            ),
            (
                json!({"kind":"env-var","envVar":"TOKEN","path":"token"}),
                "secret bad env-var resolver must declare only envVar",
            ),
            (
                json!({"kind":"file"}),
                "secret bad file resolver requires path",
            ),
            (
                json!({"kind":"file","path":""}),
                "secret bad file resolver must declare only a confined relative path",
            ),
            (
                json!({"kind":"file","path":"/token"}),
                "secret bad file resolver must declare only a confined relative path",
            ),
            (
                json!({"kind":"file","path":"../token"}),
                "secret bad file resolver must declare only a confined relative path",
            ),
            (
                json!({"kind":"file","path":"token","envVar":"TOKEN"}),
                "secret bad file resolver must declare only a confined relative path",
            ),
        ] {
            let mut value = synthetic_manifest(&SyntheticManifestOptions::default());
            // This earlier valid descriptor must not trigger filesystem access:
            // validate every descriptor, then references, then resolve values.
            value["secrets"] = json!({
                "a": {"secretId":"a","source":{"kind":"file","path":"absent-fixture-secret"}},
                "bad": {"secretId":"bad","source":source}
            });
            value["tasks"]["smoke"]["invocation"]["env"]["TOKEN"] = json!("${secret:undeclared}");
            let manifest: Manifest = serde_json::from_value(value).unwrap();
            for error in [
                check_secret_references(&manifest).unwrap_err(),
                resolve_secrets(&manifest).unwrap_err(),
            ] {
                assert_eq!(error.code, ErrorCode::ManifestAdmission);
                assert_eq!(error.message, diagnostic);
            }
        }
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
