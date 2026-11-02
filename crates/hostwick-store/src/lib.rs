//! Persistent storage: app paths (incl. portable mode) and the SQLite
//! profile store.

mod paths;
mod sqlite;

pub use paths::AppPaths;
pub use sqlite::SqliteStore;
