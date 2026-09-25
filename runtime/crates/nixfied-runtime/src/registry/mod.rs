pub mod events;
pub mod records;
pub mod schema;
pub mod session;
pub mod sqlite;
pub mod status;

pub use events::EventInsert;
pub use records::RegistryIdentity;
pub use schema::SCHEMA_VERSION;
pub use sqlite::{Registry, RegistryReader};

/// The one mapping of a SQLite failure: the registry evidence cannot be trusted.
pub(crate) fn sql_error(error: rusqlite::Error) -> crate::RuntimeError {
    crate::RuntimeError::new(crate::ErrorCode::RegistryCorrupt, error.to_string())
}
