//! Persistent storage: app paths (incl. portable mode) and the SQLite
//! profile store, and settings sync through a shared folder.

mod paths;
pub mod settings_sync;
mod sqlite;

pub use paths::AppPaths;
pub use settings_sync::{SYNC_FILE_NAME, SettingsSync, SyncError, SyncReport};
pub use sqlite::SqliteStore;
