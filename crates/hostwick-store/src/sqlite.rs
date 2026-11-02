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
    // v2: deleted profiles, so settings sync can propagate deletions.
    "CREATE TABLE tombstones (
        id         TEXT PRIMARY KEY,
        deleted_ms INTEGER NOT NULL
     );",
];

const APP_SETTINGS_KEY: &str = "sync.app_settings";
const APP_SETTINGS_MS_KEY: &str = "sync.app_settings_ms";

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
        set_setting_on(&*self.lock()?, key, value)
    }

    /// Record the app's own settings (an opaque JSON string) for settings
    /// sync. The change time is stamped only when the text changes, so it
    /// is safe to call on every save.
    pub fn set_app_settings(&self, json: &str) -> Result<(), StoreError> {
        let conn = self.lock()?;
        let old: Option<String> = conn
            .query_row(
                "SELECT value FROM settings WHERE key = ?1",
                params![APP_SETTINGS_KEY],
                |r| r.get(0),
            )
            .optional()
            .map_err(be)?;
        if old.as_deref() == Some(json) {
            return Ok(());
        }
        set_setting_on(&conn, APP_SETTINGS_KEY, json)?;
        set_setting_on(&conn, APP_SETTINGS_MS_KEY, &now_ms().to_string())
    }

    /// The app settings recorded by [`set_app_settings`] (or received by
    /// sync) and when they last changed (Unix ms).
    ///
    /// [`set_app_settings`]: Self::set_app_settings
    pub fn app_settings(&self) -> Result<Option<(String, u64)>, StoreError> {
        let Some(json) = self.setting(APP_SETTINGS_KEY)? else {
            return Ok(None);
        };
        let ms = self
            .setting(APP_SETTINGS_MS_KEY)?
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        Ok(Some((json, ms)))
    }

    /// Deleted profile ids and when they were deleted (Unix ms).
    pub fn tombstones(&self) -> Result<Vec<(ProfileId, u64)>, StoreError> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare("SELECT id, deleted_ms FROM tombstones ORDER BY id")
            .map_err(be)?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
            .map_err(be)?;
        let mut out = Vec::new();
        for row in rows {
            let (id, ms) = row.map_err(be)?;
            if let Ok(id) = id.parse() {
                out.push((id, u64::try_from(ms).unwrap_or(0)));
            }
        }
        Ok(out)
    }

    /// Apply a merged sync state in one transaction: store `upserts` as
    /// given (keeping their `updated_ms`), delete `deletes`, replace the
    /// tombstone table, and set the app settings if given.
    pub(crate) fn apply_sync(
        &self,
        upserts: &[HostProfile],
        deletes: &[ProfileId],
        tombstones: &[(ProfileId, u64)],
        app_settings: Option<(&str, u64)>,
    ) -> Result<(), StoreError> {
        let mut conn = self.lock()?;
        let tx = conn.transaction().map_err(be)?;
        for p in upserts {
            write_profile(&tx, p)?;
        }
        for id in deletes {
            tx.execute(
                "DELETE FROM profiles WHERE id = ?1",
                params![id.to_string()],
            )
            .map_err(be)?;
        }
        tx.execute("DELETE FROM tombstones", []).map_err(be)?;
        for (id, ms) in tombstones {
            tx.execute(
                "INSERT INTO tombstones(id, deleted_ms) VALUES (?1, ?2)",
                params![id.to_string(), i64::try_from(*ms).unwrap_or(i64::MAX)],
            )
            .map_err(be)?;
        }
        if let Some((json, ms)) = app_settings {
            set_setting_on(&tx, APP_SETTINGS_KEY, json)?;
            set_setting_on(&tx, APP_SETTINGS_MS_KEY, &ms.to_string())?;
        }
        tx.commit().map_err(be)
    }
}

fn set_setting_on(conn: &Connection, key: &str, value: &str) -> Result<(), StoreError> {
    conn.execute(
        "INSERT INTO settings(key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )
    .map(|_| ())
    .map_err(be)
}

/// Insert or replace a profile row exactly as given.
fn write_profile(conn: &Connection, p: &HostProfile) -> Result<(), StoreError> {
    let json = serde_json::to_string(p).map_err(|e| StoreError::Corrupt(e.to_string()))?;
    conn.execute(
        "INSERT INTO profiles(id, name, folder, host, data, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(id) DO UPDATE SET name = excluded.name, folder = excluded.folder,
            host = excluded.host, data = excluded.data, updated_at = excluded.updated_at",
        params![p.id.to_string(), p.name, p.folder, p.host, json, now()],
    )
    .map(|_| ())
    .map_err(be)
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

/// Unix milliseconds, the clock settings sync merges by.
pub(crate) fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
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

    /// Saves the profile with `updated_ms` set to now (later than any
    /// earlier save of it), so settings sync sees the newest edit win.
    fn upsert(&self, p: &HostProfile) -> Result<(), StoreError> {
        let mut conn = self.lock()?;
        let tx = conn.transaction().map_err(be)?;
        let prev: Option<String> = tx
            .query_row(
                "SELECT data FROM profiles WHERE id = ?1",
                params![p.id.to_string()],
                |r| r.get(0),
            )
            .optional()
            .map_err(be)?;
        let prev_ms = prev
            .and_then(|j| decode(j).ok())
            .map_or(0, |old| old.updated_ms);
        let mut p = p.clone();
        p.updated_ms = now_ms().max(prev_ms.saturating_add(1));
        write_profile(&tx, &p)?;
        tx.execute(
            "DELETE FROM tombstones WHERE id = ?1",
            params![p.id.to_string()],
        )
        .map_err(be)?;
        tx.commit().map_err(be)
    }

    /// Deletes the profile and records a tombstone so settings sync removes
    /// it on other devices too.
    fn delete(&self, id: ProfileId) -> Result<(), StoreError> {
        let mut conn = self.lock()?;
        let tx = conn.transaction().map_err(be)?;
        let n = tx
            .execute(
                "DELETE FROM profiles WHERE id = ?1",
                params![id.to_string()],
            )
            .map_err(be)?;
        if n == 0 {
            return Err(StoreError::NotFound(id.to_string()));
        }
        tx.execute(
            "INSERT INTO tombstones(id, deleted_ms) VALUES (?1, ?2)
             ON CONFLICT(id) DO UPDATE SET deleted_ms = excluded.deleted_ms",
            params![id.to_string(), i64::try_from(now_ms()).unwrap_or(i64::MAX)],
        )
        .map_err(be)?;
        tx.commit().map_err(be)
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

        let stored = s.get(a.id)?;
        assert!(stored.updated_ms > 0, "upsert stamps updated_ms");
        assert_eq!(
            HostProfile {
                updated_ms: a.updated_ms,
                ..stored.clone()
            },
            a
        );
        assert_eq!(s.find_by_name("prod/WEB")?.map(|p| p.id), Some(a.id));
        assert_eq!(s.find_by_name("web")?.map(|p| p.id), Some(a.id));
        assert_eq!(s.find_by_name("bastion")?.map(|p| p.id), Some(b.id));
        assert!(s.find_by_name("nope")?.is_none());
        let names: Vec<_> = s.list()?.into_iter().map(|p| p.name).collect();
        assert_eq!(names, vec!["Bastion", "web"]);

        a.host = "10.0.0.2".into();
        s.upsert(&a)?;
        assert_eq!(s.get(a.id)?.host, "10.0.0.2");
        assert!(s.get(a.id)?.updated_ms > stored.updated_ms, "monotonic");
        assert!(s.tombstones()?.is_empty());
        s.delete(a.id)?;
        let tomb = s.tombstones()?;
        assert_eq!(tomb.len(), 1);
        assert_eq!(tomb[0].0, a.id);
        assert!(tomb[0].1 >= stored.updated_ms);
        // Re-creating a deleted profile clears its tombstone.
        s.upsert(&a)?;
        assert!(s.tombstones()?.is_empty());
        s.delete(a.id)?;
        assert!(matches!(s.get(a.id), Err(StoreError::NotFound(_))));
        assert!(matches!(s.delete(a.id), Err(StoreError::NotFound(_))));

        s.set_setting("theme", "dark")?;
        s.set_setting("theme", "light")?;
        assert_eq!(s.setting("theme")?.as_deref(), Some("light"));

        assert!(s.app_settings()?.is_none());
        s.set_app_settings(r#"{"theme":"dark"}"#)?;
        let (json, ms) = s.app_settings()?.unwrap_or_default();
        assert_eq!(json, r#"{"theme":"dark"}"#);
        assert!(ms > 0);
        // Same text: the change time is not bumped.
        std::thread::sleep(std::time::Duration::from_millis(5));
        s.set_app_settings(r#"{"theme":"dark"}"#)?;
        assert_eq!(s.app_settings()?.map(|x| x.1), Some(ms));
        Ok(())
    }

    #[test]
    fn migrates_a_v1_database() -> Result<(), StoreError> {
        let dir = tempfile::tempdir().map_err(be)?;
        let path = dir.path().join("hostwick.db");
        {
            let c = Connection::open(&path).map_err(be)?;
            c.execute_batch(MIGRATIONS[0]).map_err(be)?;
            c.pragma_update(None, "user_version", 1).map_err(be)?;
            c.execute(
                "INSERT INTO profiles(id,name,folder,host,data,updated_at) VALUES (?1,'old','','h',?2,0)",
                params![
                    "6f1c2c1e-6f1c-4c1e-8f1c-6f1c2c1e6f1c",
                    r#"{"id":"6f1c2c1e-6f1c-4c1e-8f1c-6f1c2c1e6f1c","name":"old","host":"h"}"#
                ],
            )
            .map_err(be)?;
        }
        let s = SqliteStore::open(&path)?;
        assert_eq!(s.schema_version()?, MIGRATIONS.len());
        assert!(dir.path().join("hostwick.db.bak-v1").exists());
        let p = s.list()?.pop().ok_or_else(|| be("missing"))?;
        assert_eq!((p.name.as_str(), p.updated_ms), ("old", 0));
        s.delete(p.id)?;
        assert_eq!(s.tombstones()?.len(), 1);
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
