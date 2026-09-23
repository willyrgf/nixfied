//! The authored placeholder grammar. Unknown child syntax stays literal, but
//! never hides a recognized reference nested inside it.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Port,
    Host,
    StateDir,
    Secret,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Reference<'a> {
    Port(Option<&'a str>),
    Host(Option<&'a str>),
    StateDir,
    Secret(&'a str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Token<'a> {
    Literal(&'a str),
    Reference(Reference<'a>),
    Malformed { kind: Kind, empty: bool },
}

pub(crate) fn tokenize(value: &str) -> Vec<Token<'_>> {
    let mut tokens = Vec::new();
    let mut cursor = 0;
    let mut literal = 0;
    while let Some(offset) = value[cursor..].find("${") {
        let start = cursor + offset;
        let text = &value[start..];
        let recognized = [
            ("${port", Kind::Port),
            ("${host", Kind::Host),
            ("${stateDir", Kind::StateDir),
            ("${secret", Kind::Secret),
        ]
        .into_iter()
        .find_map(|(prefix, kind)| {
            let rest = text.strip_prefix(prefix)?;
            if rest.starts_with('}') && kind != Kind::Secret {
                let reference = match kind {
                    Kind::Port => Reference::Port(None),
                    Kind::Host => Reference::Host(None),
                    Kind::StateDir => Reference::StateDir,
                    Kind::Secret => unreachable!(),
                };
                return Some((Token::Reference(reference), prefix.len() + 1));
            }
            if let Some(payload) = rest.strip_prefix(':')
                && kind != Kind::StateDir
            {
                let end = payload.find('}');
                if let Some(end) = end {
                    let name = &payload[..end];
                    if !name.is_empty() && !name.contains(['{', '}']) {
                        let reference = match kind {
                            Kind::Port => Reference::Port(Some(name)),
                            Kind::Host => Reference::Host(Some(name)),
                            Kind::Secret => Reference::Secret(name),
                            Kind::StateDir => unreachable!(),
                        };
                        return Some((Token::Reference(reference), prefix.len() + end + 2));
                    }
                }
                // Advance only over the opener so a malformed endpoint cannot
                // hide a nested secret from the earlier secret-check phase.
                return Some((
                    Token::Malformed {
                        kind,
                        empty: end == Some(0),
                    },
                    2,
                ));
            }
            if rest.is_empty() && kind != Kind::Secret {
                return Some((Token::Malformed { kind, empty: false }, 2));
            }
            None
        });
        if let Some((token, length)) = recognized {
            if literal < start {
                tokens.push(Token::Literal(&value[literal..start]));
            }
            tokens.push(token);
            cursor = start + length;
            literal = cursor;
        } else {
            cursor = start + 2;
        }
    }
    if literal < value.len() {
        tokens.push(Token::Literal(&value[literal..]));
    }
    tokens
}

use crate::error::{ErrorCode, RuntimeError, RuntimeResult};
use nixfied_manifest::{SecretDescriptor, ServiceId};
use std::collections::{BTreeMap, BTreeSet};

/// Checked authored text. Only parsing can construct its private pieces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template(Vec<Piece>);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum EndpointSelector {
    Primary,
    Own(String),
    Service(ServiceId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Piece {
    Literal(String),
    Port(EndpointSelector),
    Host(EndpointSelector),
    StateDir,
    Secret(String),
}

pub(crate) enum Owner<'a> {
    Service(&'a str),
    Task(&'a str),
}

pub(crate) struct Scope<'a> {
    pub owner: Owner<'a>,
    pub has_primary: bool,
    pub own_endpoints: BTreeSet<&'a str>,
    pub services: BTreeSet<&'a str>,
}

impl Scope<'_> {
    fn owner(&self) -> String {
        match self.owner {
            Owner::Service(id) => format!("service {id}"),
            Owner::Task(id) => format!("task {id}"),
        }
    }

    fn endpoint(&self, name: Option<&str>, port: bool) -> RuntimeResult<EndpointSelector> {
        let Some(name) = name else {
            if self.has_primary {
                return Ok(EndpointSelector::Primary);
            }
            let placeholder = if port { "${port}" } else { "${host}" };
            return Err(RuntimeError::new(
                ErrorCode::ManifestAdmission,
                match self.owner {
                    Owner::Service(id) => format!(
                        "service {id} references {placeholder} but declares no endpoint to resolve it"
                    ),
                    Owner::Task(id) => format!(
                        "task {id} references {placeholder} but requires no service to resolve it"
                    ),
                },
            ));
        };
        if self.own_endpoints.contains(name) {
            return Ok(EndpointSelector::Own(name.to_string()));
        }
        if self.services.contains(name) {
            return Ok(EndpointSelector::Service(ServiceId::new(name)));
        }
        let scope = match self.owner {
            Owner::Service(_) => "own endpoints or addressable connectsTo",
            Owner::Task(_) => "addressable requires",
        };
        Err(RuntimeError::new(
            ErrorCode::ManifestAdmission,
            format!(
                "{} references the endpoint of {name} without declaring it in {scope}",
                self.owner()
            ),
        ))
    }
}

impl Template {
    pub(crate) fn parse(
        value: &str,
        scope: &Scope<'_>,
        secrets: &BTreeMap<String, SecretDescriptor>,
        allow_secret: bool,
    ) -> RuntimeResult<Self> {
        let pieces = tokenize(value)
            .into_iter()
            .map(|token| {
                Ok(match token {
                    Token::Literal(text) => Piece::Literal(text.to_string()),
                    Token::Reference(Reference::Port(name)) => {
                        Piece::Port(scope.endpoint(name, true)?)
                    }
                    Token::Reference(Reference::Host(name)) => {
                        Piece::Host(scope.endpoint(name, false)?)
                    }
                    Token::Reference(Reference::StateDir) => Piece::StateDir,
                    Token::Reference(Reference::Secret(id)) => {
                        if !allow_secret {
                            return Err(RuntimeError::new(
                                ErrorCode::ManifestAdmission,
                                "secret placeholders are only allowed in invocation.env values",
                            ));
                        }
                        if !secrets.contains_key(id) {
                            return Err(RuntimeError::new(
                                ErrorCode::ManifestAdmission,
                                format!("secret placeholder references undeclared secret {id}"),
                            ));
                        }
                        Piece::Secret(id.to_string())
                    }
                    Token::Malformed { kind, .. } => {
                        return Err(RuntimeError::new(
                            ErrorCode::ManifestAdmission,
                            format!("{} has a malformed {kind:?} placeholder", scope.owner()),
                        ));
                    }
                })
            })
            .collect::<RuntimeResult<Vec<_>>>()?;
        Ok(Self(pieces))
    }

    pub(crate) fn pieces(&self) -> &[Piece] {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_grammar_vectors_match_the_independent_nix_contract() {
        use serde_json::{Value, json};
        let vectors: Vec<Value> =
            serde_json::from_str(include_str!("../tests/fixtures/invocation-templates.json"))
                .unwrap();
        for vector in vectors {
            let text = vector["text"].as_str().unwrap();
            let tokens: Vec<Value> = tokenize(text)
                .into_iter()
                .map(|token| match token {
                    Token::Literal(text) => json!({"literal": text}),
                    Token::Reference(Reference::Port(name)) => json!({"port": name}),
                    Token::Reference(Reference::Host(name)) => json!({"host": name}),
                    Token::Reference(Reference::StateDir) => json!({"stateDir": null}),
                    Token::Reference(Reference::Secret(name)) => json!({"secret": name}),
                    Token::Malformed { kind, empty } => {
                        let kind = match kind {
                            Kind::Port => "port",
                            Kind::Host => "host",
                            Kind::StateDir => "stateDir",
                            Kind::Secret => "secret",
                        };
                        json!({"malformed": kind, "empty": empty})
                    }
                })
                .collect();
            assert_eq!(json!(tokens), vector["tokens"], "{text}");
        }
    }
}
