//! Typed identifiers for the model's cross-references. Each is a `#[serde(transparent)]`
//! newtype over `String`, so the wire shape is unchanged (still a JSON string),
//! but a reference can no longer be confused with an unrelated string or with a
//! reference of a different namespace. The runtime's lowering resolves each
//! reference against the declarations once and mints a typed handle, so the
//! executor's later lookups are infallible by construction.

use serde::{Deserialize, Serialize};

macro_rules! id_newtype {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(
            Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Construct from any string-like value (code and tests).
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            /// Borrow the underlying id as a string slice.
            pub fn as_str(&self) -> &str {
                &self.0
            }

            /// Consume into the owned string.
            pub fn into_string(self) -> String {
                self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }

        // `Borrow<str>` lets a `BTreeMap<$name, V>` (or set) be queried with a
        // plain `&str` key. The newtype's `Eq`/`Ord`/`Hash` all defer to the inner
        // `String`, so they agree with `str`'s, satisfying `Borrow`'s contract.
        impl std::borrow::Borrow<str> for $name {
            fn borrow(&self) -> &str {
                &self.0
            }
        }

        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self(value)
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self(value.to_string())
            }
        }
    };
}

id_newtype!(
    /// A realised closure's id (`Model.closures` key).
    ClosureId
);
id_newtype!(
    /// A source codebase's id (`Model.codebases[].codebaseId`).
    CodebaseId
);
id_newtype!(
    /// A service's id (`Model.services` key).
    ServiceId
);
id_newtype!(
    /// A task's id (`Model.tasks` key).
    TaskId
);
id_newtype!(
    /// A flattened plan node's id: the step path (docs/DERIVATION_SPEC.md §2).
    NodeId
);
id_newtype!(
    /// A lifecycle/task operation id, globally unique across the model.
    OperationId
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_round_trips_as_a_plain_string() {
        let id = ServiceId::new("postgres");
        let json = serde_json::to_string(&id).expect("serialize");
        assert_eq!(json, "\"postgres\"");
        let back: ServiceId = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, id);
        assert_eq!(back.as_str(), "postgres");
    }

    #[test]
    fn id_deserializes_from_a_json_string() {
        let id: ClosureId = serde_json::from_str("\"svc-closure\"").expect("deserialize");
        assert_eq!(id.as_str(), "svc-closure");
    }
}
