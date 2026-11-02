use std::path::Path;
use std::sync::Mutex;

use hostwick_core::{HostProfile, ProfileId, ProfileStore, StoreError};
use rusqlite::{Connection, OptionalExtension, params};

/// Ordered schema migrations; index + 1 is the `user_version` after applying.
/// Never edit a shipped migration — append a new one.
const MIGRATIONS: &[&str] = &[
    // v1
    "CREATE TABLE profiles (
        id         TEXT PRIMARY KEY,
        name       TEXT NOT NULL,
        folder     TEXT NOT NULL DEFAULT '',
        host       TEXT NOT NULL,
        data       TEXT NOT NULL,
        updated_at INTEGER NOT NULL
     );
     CREATE INDEX profiles_name ON profiles(name);
     CREATE INDEX profiles_folder ON profiles(folder);
     CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
];

/// Profiles stored as JSON documents with indexed columns for lookup. The
/// JSON column lets profile fields evolve without a migration per field.
pub struct SqliteStore {
    conn: Mutex<Connection>,
}

fn be(e: impl std::fmt::Display) -> StoreError {
    StoreError::Backend(e.to_string())
}

impl SqliteStore {
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(be)?;
        }
        let existed = path.exists();
        let conn = Connection::open(path).map_err(be)?;
        let store = Self::init(conn)?;
        if existed {
            store.backup_before_migrate(path)?;
        }
        store.migrate()?;
        Ok(store)
    }

    pub fn in_memory() -> Result<Self, StoreError> {
        let store = Self::init(Connection::open_in_memory().map_err(be)?)?;
        store.migrate()?;
        Ok(store)
    }

    fn init(conn: Connection) -> Result<Self, StoreError> {
        // WAL: readers never block the writer; crash-safe.
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(be)?;
        conn.pragma_update(None, "synchronous", "NORMAL")
            .map_err(be)?;
        conn.pragma_update(None, "foreign_keys", "ON").map_err(be)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(be)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Connection>, StoreError> {
        self.conn
            .lock()
            .map_err(|_| StoreError::Backend("store lock poisoned".into()))
    }

    fn version(conn: &Connection) -> Result<usize, StoreError> {
        let v: i64 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .map_err(be)?;
        Ok(usize::try_from(v).unwrap_or(0))
    }

    /// Copy the database aside before upgrading its schema, so a bad
    /// migration can always be rolled back by hand.
    fn backup_before_migrate(&self, path: &Path) -> Result<(), StoreError> {
        let conn = self.lock()?;
        let v = Self::version(&conn)?;
        if v < MIGRATIONS.len() && v > 0 {
            let backup = path.with_extension(format!("db.bak-v{v}"));
            conn.execute("VACUUM INTO ?1", params![backup.to_string_lossy()])
                .map_err(be)?;
            tracing::info!(?backup, "backed up database before migration");
        }
        Ok(())
    }

    fn migrate(&self) -> Result<(), StoreError> {
        let mut conn = self.lock()?;
        let current = Self::version(&conn)?;
        if current > MIGRATIONS.len() {
            return Err(StoreError::Backend(format!(
                "database schema v{current} is newer than this Hostwick (v{}); please update",
                MIGRATIONS.len()
            )));
        }
        for (i, sql) in MIGRATIONS.iter().enumerate().skip(current) {
            let tx = conn.transaction().map_err(be)?;
            tx.execute_batch(sql).map_err(be)?;
            tx.pragma_update(None, "user_version", (i + 1) as i64)
                .map_err(be)?;
            tx.commit().map_err(be)?;
        }
        Ok(())
    }

    pub fn schema_version(&self) -> Result<usize, StoreError> {
        Self::version(&*self.lock()?)
    }

    pub fn setting(&self, key: &str) -> Result<Option<String>, StoreError> {
        self.lock()?
            .query_row(
                "SELECT value FROM settings WHERE key = ?1",
                params![key],
                |r| r.get(0),
            )
            .optional()
            .map_err(be)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<(), StoreError> {
        self.lock()?
            .execute(
                "INSERT INTO settings(key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )
            .map(|_| ())
            .map_err(be)
    }
}

fn decode(json: String) -> Result<HostProfile, StoreError> {
    serde_json::from_str(&json).map_err(|e| StoreError::Corrupt(e.to_string()))
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl ProfileStore for SqliteStore {
    fn list(&self) -> Result<Vec<HostProfile>, StoreError> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare(
                "SELECT data FROM profiles ORDER BY folder COLLATE NOCASE, name COLLATE NOCASE",
            )
            .map_err(be)?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0)).map_err(be)?;
        let mut out = Vec::new();
        for row in rows {
            match decode(row.map_err(be)?) {
                Ok(p) => out.push(p),
                // One bad row must not hide every other profile.
                Err(e) => tracing::warn!(error = %e, "skipping unreadable profile"),
            }
        }
        Ok(out)
    }

    fn get(&self, id: ProfileId) -> Result<HostProfile, StoreError> {
        let json: Option<String> = self
            .lock()?
            .query_row(
                "SELECT data FROM profiles WHERE id = ?1",
                params![id.to_string()],
                |r| r.get(0),
            )
            .optional()
            .map_err(be)?;
        decode(json.ok_or_else(|| StoreError::NotFound(id.to_string()))?)
    }

    /// Matches `name` or `folder/name`, case-insensitively.
    fn find_by_name(&self, name: &str) -> Result<Option<HostProfile>, StoreError> {
        let json: Option<String> = self
            .lock()?
            .query_row(
                "SELECT data FROM profiles
                 WHERE name = ?1 COLLATE NOCASE
                    OR (folder <> '' AND folder || '/' || name = ?1 COLLATE NOCASE)
                 ORDER BY folder = '' DESC LIMIT 1",
                params![name],
                |r| r.get(0),
            )
            .optional()
            .map_err(be)?;
        json.map(decode).transpose()
    }

    fn upsert(&self, p: &HostProfile) -> Result<(), StoreError> {
        let json = serde_json::to_string(p).map_err(|e| StoreError::Corrupt(e.to_string()))?;
        self.lock()?
            .execute(
                "INSERT INTO profiles(id, name, folder, host, data, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(id) DO UPDATE SET name = excluded.name, folder = excluded.folder,
                    host = excluded.host, data = excluded.data, updated_at = excluded.updated_at",
                params![p.id.to_string(), p.name, p.folder, p.host, json, now()],
            )
            .map(|_| ())
            .map_err(be)
    }

    fn delete(&self, id: ProfileId) -> Result<(), StoreError> {
        let n = self
            .lock()?
            .execute(
                "DELETE FROM profiles WHERE id = ?1",
                params![id.to_string()],
            )
            .map_err(be)?;
        if n == 0 {
            Err(StoreError::NotFound(id.to_string()))
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hostwick_core::ForwardSpec;

    #[test]
    fn crud_and_lookup() -> Result<(), StoreError> {
        let s = SqliteStore::in_memory()?;
        assert_eq!(s.schema_version()?, MIGRATIONS.len());
        let mut a = HostProfile::new("web", "10.0.0.1");
        a.folder = "prod".into();
        a.forwards.push(ForwardSpec::Dynamic {
            bind_addr: "127.0.0.1".into(),
            bind_port: 1080,
        });
        let b = HostProfile::new("Bastion", "jump.example.com");
        s.upsert(&a)?;
        s.upsert(&b)?;

        assert_eq!(s.get(a.id)?, a);
        assert_eq!(s.find_by_name("prod/WEB")?.map(|p| p.id), Some(a.id));
        assert_eq!(s.find_by_name("web")?.map(|p| p.id), Some(a.id));
        assert_eq!(s.find_by_name("bastion")?.map(|p| p.id), Some(b.id));
        assert!(s.find_by_name("nope")?.is_none());
        let names: Vec<_> = s.list()?.into_iter().map(|p| p.name).collect();
        assert_eq!(names, vec!["Bastion", "web"]);

        a.host = "10.0.0.2".into();
        s.upsert(&a)?;
        assert_eq!(s.get(a.id)?.host, "10.0.0.2");
        s.delete(a.id)?;
        assert!(matches!(s.get(a.id), Err(StoreError::NotFound(_))));
        assert!(matches!(s.delete(a.id), Err(StoreError::NotFound(_))));

        s.set_setting("theme", "dark")?;
        s.set_setting("theme", "light")?;
        assert_eq!(s.setting("theme")?.as_deref(), Some("light"));
        Ok(())
    }

    #[test]
    fn reopen_keeps_data_and_rejects_newer_schema() -> Result<(), StoreError> {
        let dir = tempfile::tempdir().map_err(be)?;
        let path = dir.path().join("hostwick.db");
        let p = HostProfile::new("x", "h");
        SqliteStore::open(&path)?.upsert(&p)?;
        assert_eq!(SqliteStore::open(&path)?.get(p.id)?.name, "x");

        Connection::open(&path)
            .map_err(be)?
            .pragma_update(None, "user_version", 999)
            .map_err(be)?;
        assert!(SqliteStore::open(&path).is_err());
        Ok(())
    }

    #[test]
    fn corrupt_row_does_not_hide_others() -> Result<(), StoreError> {
        let s = SqliteStore::in_memory()?;
        s.upsert(&HostProfile::new("ok", "h"))?;
        s.lock()?
            .execute(
                "INSERT INTO profiles(id,name,folder,host,data,updated_at) VALUES ('x','bad','','h','{not json',0)",
                [],
            )
            .map_err(be)?;
        assert_eq!(s.list()?.len(), 1);
        Ok(())
    }
}
