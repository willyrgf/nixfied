pub mod events;
pub mod observation;
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
impl From<rusqlite::Error> for crate::RuntimeError {
    fn from(error: rusqlite::Error) -> Self {
        Self::new(crate::ErrorCode::RegistryCorrupt, error.to_string())
    }
}
