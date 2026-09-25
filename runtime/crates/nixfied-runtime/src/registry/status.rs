//! Typed registry status domains.
//!
//! Every status column in the registry is a closed set of strings. Manifesting each
//! as an enum gives one source of truth for the wire strings, a total parse on
//! read, an exhaustive match on classification (active vs terminal), and SQL
//! `IN (...)` lists derived from the same variants — so a mistyped or missed
//! status is a compile error, not a silently wrong row.

use crate::error::{ErrorCode, RuntimeError, RuntimeResult};

/// A status stored as a fixed string in a registry column.
pub trait DbStatus: Copy + Sized + 'static {
    /// The domain name, for parse-error messages.
    const DOMAIN: &'static str;
    fn as_str(self) -> &'static str;
    fn from_db(raw: &str) -> Option<Self>;

    /// Parse a value read back from the registry, rejecting an unknown string.
    fn parse_db(raw: &str) -> RuntimeResult<Self> {
        Self::from_db(raw).ok_or_else(|| {
            RuntimeError::new(
                ErrorCode::RegistryCorrupt,
                format!("registry holds unknown {} status {raw:?}", Self::DOMAIN),
            )
        })
    }
}

/// A SQL fragment listing the given statuses for an `IN (...)` clause. The values
/// are `'static` enum strings, so the interpolation cannot inject.
pub fn sql_in_list<S: DbStatus>(statuses: &[S]) -> String {
    statuses
        .iter()
        .map(|status| format!("'{}'", status.as_str()))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Registry queries use `p` for the process row whose ownership is checked. A
/// terminal status whose containment was never proven remains an obligation,
/// together with any endpoint evidence it owns.
pub(crate) fn unsettled_terminal_sql() -> String {
    format!(
        "p.ownership = 'unresolved' AND p.status NOT IN ({})",
        sql_in_list(PROCESS_ACTIVE)
    )
}

pub(crate) fn actionable_process_sql() -> &'static str {
    "p.ownership = 'unresolved'"
}

macro_rules! db_status {
    ($(#[$meta:meta])* $name:ident { $($variant:ident => $lit:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum $name {
            $($variant),+
        }

        impl DbStatus for $name {
            const DOMAIN: &'static str = stringify!($name);
            fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $lit),+
                }
            }
            fn from_db(raw: &str) -> Option<Self> {
                match raw {
                    $($lit => Some(Self::$variant),)+
                    _ => None,
                }
            }
        }
    };
}

include!("../generated/status.rs");

/// Recorded statuses of a process that has not reached a terminal outcome.
pub const PROCESS_ACTIVE: &[ProcessStatus] = &[ProcessStatus::Running, ProcessStatus::Ready];
