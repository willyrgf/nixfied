//! Typed registry status domains.
//!
//! Every status column in the registry is a closed set of strings. Modeling each
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

db_status! {
    /// `runs.status`: the overall outcome of a run.
    RunStatus {
        ServiceStarting => "service-starting",
        Canceling => "canceling",
        Canceled => "canceled",
        Completed => "completed",
        TaskSucceeded => "task-succeeded",
        TaskFailed => "task-failed",
        ServiceFailed => "service-failed",
        ProcEscaped => "proc-escaped",
        Stale => "stale",
    }
}

db_status! {
    /// `processes.status`: a spawned process's state.
    ProcessStatus {
        Starting => "starting",
        Running => "running",
        Ready => "ready",
        Stopped => "stopped",
        Succeeded => "succeeded",
        Failed => "failed",
        Canceled => "canceled",
        Escaped => "escaped",
        Stale => "stale",
    }
}

db_status! {
    /// `run_leases.status`: ownership of a run's service reservation.
    RunLeaseStatus {
        Active => "active",
        Canceling => "canceling",
        Canceled => "canceled",
        Completed => "completed",
        Failed => "failed",
        Stale => "stale",
    }
}

db_status! {
    /// `ports.status`: a port reservation's binding state.
    PortStatus {
        Reserved => "reserved",
        Active => "active",
        Released => "released",
        Stale => "stale",
    }
}

db_status! {
    /// `cleanups.status`: a marker-gated cleanup's state.
    CleanupStatus {
        Intent => "intent",
        Deleted => "deleted",
        Failed => "failed",
    }
}

/// Lease statuses that keep a reservation open (not yet terminal).
pub const LEASE_OPEN: &[RunLeaseStatus] = &[RunLeaseStatus::Active, RunLeaseStatus::Canceling];

/// Lease statuses that release ownership for the run owner.
pub const LEASE_TERMINAL: &[RunLeaseStatus] = &[
    RunLeaseStatus::Completed,
    RunLeaseStatus::Canceled,
    RunLeaseStatus::Failed,
    RunLeaseStatus::Stale,
];

/// Port statuses that keep a reservation open.
pub const PORT_OPEN: &[PortStatus] = &[PortStatus::Reserved, PortStatus::Active];

/// Process statuses that count as still live during reconciliation.
pub const PROCESS_ACTIVE: &[ProcessStatus] = &[
    ProcessStatus::Starting,
    ProcessStatus::Running,
    ProcessStatus::Ready,
];

/// Run statuses that are already terminal (a stale sweep must skip them).
pub const RUN_TERMINAL: &[RunStatus] = &[
    RunStatus::Canceled,
    RunStatus::TaskFailed,
    RunStatus::ServiceFailed,
    RunStatus::ProcEscaped,
];

/// Cleanup statuses a prior-cleanup lookup considers.
pub const CLEANUP_PRIOR: &[CleanupStatus] = &[CleanupStatus::Intent, CleanupStatus::Deleted];

#[cfg(test)]
mod tests {
    use super::*;

    /// The serialized strings are the registry's on-disk wire format: a rename
    /// must break here, never silently write an unreadable column.
    fn assert_round_trips<S: DbStatus + std::fmt::Debug + PartialEq>(all: &[S]) {
        for &status in all {
            assert_eq!(S::from_db(status.as_str()), Some(status));
        }
        assert_eq!(S::from_db("not-a-status"), None);
    }

    #[test]
    fn wire_strings_round_trip() {
        assert_round_trips(&[
            RunStatus::ServiceStarting,
            RunStatus::Canceling,
            RunStatus::Canceled,
            RunStatus::Completed,
            RunStatus::TaskSucceeded,
            RunStatus::TaskFailed,
            RunStatus::ServiceFailed,
            RunStatus::ProcEscaped,
            RunStatus::Stale,
        ]);
        assert_round_trips(&[
            PortStatus::Reserved,
            PortStatus::Active,
            PortStatus::Released,
            PortStatus::Stale,
        ]);
        assert_round_trips(&[
            CleanupStatus::Intent,
            CleanupStatus::Deleted,
            CleanupStatus::Failed,
        ]);
    }

    #[test]
    fn sql_in_list_quotes_and_joins() {
        assert_eq!(sql_in_list(LEASE_OPEN), "'active', 'canceling'");
        assert_eq!(sql_in_list(PORT_OPEN), "'reserved', 'active'");
        assert_eq!(
            sql_in_list(RUN_TERMINAL),
            "'canceled', 'task-failed', 'service-failed', 'proc-escaped'"
        );
    }
}
