#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;

const FAST: KdfParams = KdfParams {
    m_cost_kib: 1024,
    t_cost: 1,
    p_cost: 1,
};

fn pass(v: &str) -> SecretString {
    SecretString::from(v.to_owned())
}

struct Device {
    name: &'static str,
    store: SqliteStore,
}

impl Device {
    fn new(name: &'static str) -> Self {
        Self {
            name,
            store: SqliteStore::in_memory().unwrap(),
        }
    }

    fn sync_in(&self, folder: &Path) -> SettingsSync<'_> {
        SettingsSync::new(&self.store, folder, pass("sync pass"))
            .with_kdf(FAST)
            .with_device_name(self.name)
    }

    fn add(&self, name: &str, host: &str) -> HostProfile {
        let p = HostProfile::new(name, host);
        self.store.upsert(&p).unwrap();
        self.store.get(p.id).unwrap()
    }

    fn by_name(&self, name: &str) -> Option<HostProfile> {
        self.store.find_by_name(name).unwrap()
    }

    fn names(&self) -> Vec<String> {
        let mut v: Vec<String> = self
            .store
            .list()
            .unwrap()
            .into_iter()
            .map(|p| p.name)
            .collect();
        v.sort();
        v
    }
}

/// Clock granularity is 1 ms; make sure "later" really is later.
fn tick() {
    std::thread::sleep(std::time::Duration::from_millis(3));
}

fn decrypt(path: &Path, p: &str) -> Payload {
    let bytes = std::fs::read(path).unwrap();
    let (plain, _) = sealed::open(&bytes, &pass(p), PURPOSE).unwrap();
    serde_json::from_slice(&plain).unwrap()
}

#[test]
fn conflict_copy_names() {
    for n in [
        "hostwick-sync (Dhruva's conflicted copy 2026-11-02).hwsync",
        "hostwick-sync (conflicted copy 2026-11-02 101010).hwsync",
        "hostwick-sync.sync-conflict-20261102-101010-ABCDEFG.hwsync",
        "hostwick-sync-DESKTOP-4H2K.hwsync",
        "hostwick-sync (1).hwsync",
        "hostwick-sync 2.hwsync",
        "Hostwick-Sync-LAPTOP.HWSYNC",
    ] {
        assert!(is_conflict_copy(n), "{n}");
    }
    for n in [
        "hostwick-sync.hwsync",
        ".hostwick-sync.hwsync.123-456.tmp",
        "hostwick-sync.hwsync.bak",
        "other.hwsync",
        "notes.txt",
    ] {
        assert!(!is_conflict_copy(n), "{n}");
    }
}

#[test]
fn merge_rules() {
    let v = |ms: u64, host: &str| serde_json::json!({"updated_ms": ms, "host": host});
    let mut a = State::default();
    let mut b = State::default();
    a.profiles.insert("1".into(), v(10, "old"));
    b.profiles.insert("1".into(), v(20, "new"));
    // Deleted at 15 on A, but edited later (20) on B: the edit wins.
    a.tombstones.insert("1".into(), 15);
    // Edited at 5, deleted at 30: the deletion wins.
    a.profiles.insert("2".into(), v(5, "x"));
    b.tombstones.insert("2".into(), 30);
    // Same time, different content: deterministic winner.
    a.profiles.insert("3".into(), v(7, "a"));
    b.profiles.insert("3".into(), v(7, "b"));
    a.app_settings = Some(SettingsBlob {
        json: "{\"t\":1}".into(),
        updated_ms: 1,
    });
    b.app_settings = Some(SettingsBlob {
        json: "{\"t\":2}".into(),
        updated_ms: 2,
    });

    let m = merge(&a, &b);
    assert_eq!(m, merge(&b, &a), "merge is symmetric");
    assert_eq!(m, merge(&m, &a), "merge is idempotent");
    assert_eq!(m.profiles["1"]["host"], "new");
    assert!(
        !m.tombstones.contains_key("1"),
        "superseded tombstone dropped"
    );
    assert!(!m.profiles.contains_key("2"));
    assert_eq!(m.tombstones["2"], 30);
    assert_eq!(m.profiles["3"]["host"], "b");
    assert_eq!(m.app_settings.as_ref().map(|s| s.updated_ms), Some(2));
}

#[test]
fn two_devices_sync_edits_and_deletes_both_ways() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path();
    let (a, b) = (Device::new("A"), Device::new("B"));

    a.add("web", "10.0.0.1");
    a.add("db", "10.0.0.2");
    a.store.set_app_settings(r#"{"theme":"dark"}"#).unwrap();
    let r = a.sync_in(folder).sync().unwrap();
    assert!(r.uploaded && r.local_unchanged());
    assert_eq!(r.profiles, 2);
    let file = folder.join(SYNC_FILE_NAME);
    assert!(file.exists());
    let raw = std::fs::read_to_string(&file).unwrap();
    assert!(
        !raw.contains("10.0.0.1") && !raw.contains("dark"),
        "plaintext on disk"
    );
    assert_eq!(decrypt(&file, "sync pass").written_by, "A");

    // B starts with its own host; gets A's, sends its own.
    b.add("mail", "10.0.0.3");
    let r = b.sync_in(folder).sync().unwrap();
    let mut added = r.added.clone();
    added.sort();
    assert_eq!(added, vec!["db", "web"]);
    assert_eq!(r.app_settings.as_deref(), Some(r#"{"theme":"dark"}"#));
    assert_eq!(
        b.store.app_settings().unwrap().map(|s| s.0).as_deref(),
        Some(r#"{"theme":"dark"}"#)
    );
    assert!(r.uploaded);
    assert_eq!(b.names(), vec!["db", "mail", "web"]);

    // Nothing new: no rewrite.
    let before = std::fs::read(&file).unwrap();
    let r = b.sync_in(folder).sync().unwrap();
    assert!(!r.uploaded && r.local_unchanged());
    assert_eq!(std::fs::read(&file).unwrap(), before);

    // A: gets mail; deletes db. B: edits web.
    let r = a.sync_in(folder).sync().unwrap();
    assert_eq!(r.added, vec!["mail"]);
    tick();
    a.store.delete(a.by_name("db").unwrap().id).unwrap();
    tick();
    let mut web = b.by_name("web").unwrap();
    web.port = 2222;
    b.store.upsert(&web).unwrap();

    let r = a.sync_in(folder).sync().unwrap();
    assert!(r.uploaded && r.local_unchanged());
    let r = b.sync_in(folder).sync().unwrap();
    assert_eq!(r.deleted, vec!["db"]);
    assert!(r.uploaded);
    let r = a.sync_in(folder).sync().unwrap();
    assert_eq!(r.updated, vec!["web"]);
    assert_eq!(a.by_name("web").unwrap().port, 2222);
    assert_eq!(a.names(), b.names());
    assert_eq!(a.names(), vec!["mail", "web"]);

    // Both edit the same host: the later edit wins everywhere.
    let mut wa = a.by_name("web").unwrap();
    wa.host = "from-a".into();
    a.store.upsert(&wa).unwrap();
    tick();
    let mut wb = b.by_name("web").unwrap();
    wb.host = "from-b".into();
    b.store.upsert(&wb).unwrap();
    a.sync_in(folder).sync().unwrap();
    b.sync_in(folder).sync().unwrap();
    a.sync_in(folder).sync().unwrap();
    assert_eq!(a.by_name("web").unwrap().host, "from-b");
    assert_eq!(b.by_name("web").unwrap().host, "from-b");

    // Delete on A, but B edits it afterwards (without having seen the
    // delete): the newer edit brings it back.
    a.store.delete(a.by_name("mail").unwrap().id).unwrap();
    tick();
    let mut m = b.by_name("mail").unwrap();
    m.notes = "still needed".into();
    b.store.upsert(&m).unwrap();
    a.sync_in(folder).sync().unwrap();
    b.sync_in(folder).sync().unwrap();
    let r = a.sync_in(folder).sync().unwrap();
    assert_eq!(r.added, vec!["mail"]);
    assert_eq!(a.by_name("mail").unwrap().notes, "still needed");
    // Only db's deletion is still remembered.
    let mail_id = a.by_name("mail").unwrap().id;
    let tombs = a.store.tombstones().unwrap();
    assert_eq!(tombs.len(), 1);
    assert!(tombs.iter().all(|(id, _)| *id != mail_id));

    // Settings changed later on B reach A.
    tick();
    b.store.set_app_settings(r#"{"theme":"light"}"#).unwrap();
    b.sync_in(folder).sync().unwrap();
    let r = a.sync_in(folder).sync().unwrap();
    assert_eq!(r.app_settings.as_deref(), Some(r#"{"theme":"light"}"#));

    // The tombstones and profiles agree on both sides.
    assert_eq!(
        a.sync_in(folder).local_state().unwrap(),
        b.sync_in(folder).local_state().unwrap()
    );
}

#[test]
fn wrong_passphrase_and_missing_folder() {
    let dir = tempfile::tempdir().unwrap();
    let a = Device::new("A");
    a.add("web", "h");
    a.sync_in(dir.path()).sync().unwrap();
    let other = SettingsSync::new(&a.store, dir.path(), pass("nope")).with_kdf(FAST);
    let err = other.sync().unwrap_err();
    assert!(matches!(err, SyncError::WrongPassphrase(_)), "{err}");
    assert!(err.to_string().contains("wrong sync passphrase"));
    assert!(matches!(other.pull(), Err(SyncError::WrongPassphrase(_))));

    let gone = SettingsSync::new(&a.store, dir.path().join("missing"), pass("x"));
    assert!(matches!(gone.sync(), Err(SyncError::NoFolder(_))));

    std::fs::write(dir.path().join(SYNC_FILE_NAME), b"not json").unwrap();
    assert!(matches!(
        a.sync_in(dir.path()).sync(),
        Err(SyncError::Corrupt { .. })
    ));
}

#[test]
fn conflicted_copies_are_merged_then_removed() {
    let shared = tempfile::tempdir().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    let (a, b, c) = (Device::new("A"), Device::new("B"), Device::new("C"));
    a.add("a-host", "1");
    a.sync_in(shared.path()).sync().unwrap();

    // B and C wrote "at the same time"; their versions became conflicted
    // copies next to the main file.
    b.add("b-host", "2");
    b.sync_in(elsewhere.path()).sync().unwrap();
    let names = [
        "hostwick-sync (B's conflicted copy 2026-11-02).hwsync",
        "hostwick-sync.sync-conflict-20261102-101010-ABCDEFG.hwsync",
    ];
    std::fs::copy(
        elsewhere.path().join(SYNC_FILE_NAME),
        shared.path().join(names[0]),
    )
    .unwrap();
    c.add("c-host", "3");
    let elsewhere2 = tempfile::tempdir().unwrap();
    c.sync_in(elsewhere2.path()).sync().unwrap();
    std::fs::copy(
        elsewhere2.path().join(SYNC_FILE_NAME),
        shared.path().join(names[1]),
    )
    .unwrap();
    // A copy made with another passphrase is reported and left alone.
    let foreign = SettingsSync::new(&c.store, elsewhere2.path(), pass("other")).with_kdf(FAST);
    std::fs::remove_file(elsewhere2.path().join(SYNC_FILE_NAME)).unwrap();
    foreign.push().unwrap();
    let foreign_name = "hostwick-sync-OTHERPC.hwsync";
    std::fs::copy(
        elsewhere2.path().join(SYNC_FILE_NAME),
        shared.path().join(foreign_name),
    )
    .unwrap();

    let r = a.sync_in(shared.path()).sync().unwrap();
    let mut added = r.added.clone();
    added.sort();
    assert_eq!(added, vec!["b-host", "c-host"]);
    assert_eq!(r.merged_copies.len(), 2);
    assert_eq!(r.skipped_copies.len(), 1);
    assert!(r.skipped_copies[0].1.contains("wrong sync passphrase"));
    for n in names {
        assert!(!shared.path().join(n).exists(), "{n} not removed");
    }
    assert!(shared.path().join(foreign_name).exists());
    // The merged main file has everything.
    let p = decrypt(&shared.path().join(SYNC_FILE_NAME), "sync pass");
    assert_eq!(p.state.profiles.len(), 3);
    // No temporary files left.
    let leftovers: Vec<_> = std::fs::read_dir(shared.path())
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty());
}

#[test]
fn push_and_pull_replace_one_side() {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (Device::new("A"), Device::new("B"));
    assert!(matches!(
        a.sync_in(dir.path()).pull(),
        Err(SyncError::NothingToPull(_))
    ));
    a.add("one", "1");
    a.store.set_app_settings("{}").unwrap();
    let r = a.sync_in(dir.path()).push().unwrap();
    assert!(r.uploaded);
    assert_eq!(r.profiles, 1);

    // Pull replaces B's hosts entirely and does not touch the file.
    b.add("local-only", "x");
    let before = std::fs::read(dir.path().join(SYNC_FILE_NAME)).unwrap();
    let r = b.sync_in(dir.path()).pull().unwrap();
    assert_eq!(r.added, vec!["one"]);
    assert_eq!(r.deleted, vec!["local-only"]);
    assert_eq!(r.app_settings.as_deref(), Some("{}"));
    assert!(!r.uploaded);
    assert_eq!(b.names(), vec!["one"]);
    assert_eq!(
        std::fs::read(dir.path().join(SYNC_FILE_NAME)).unwrap(),
        before
    );

    // Push replaces the file with B's state; a new passphrase re-keys it.
    b.add("two", "2");
    SettingsSync::new(&b.store, dir.path(), pass("new pass"))
        .with_kdf(FAST)
        .push()
        .unwrap();
    assert!(matches!(
        a.sync_in(dir.path()).sync(),
        Err(SyncError::WrongPassphrase(_))
    ));
    let r = SettingsSync::new(&a.store, dir.path(), pass("new pass"))
        .with_kdf(FAST)
        .sync()
        .unwrap();
    assert_eq!(r.added, vec!["two"]);
}

#[test]
fn fields_from_newer_versions_survive_a_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let a = Device::new("A");
    // A "newer Hostwick" wrote a profile with a field we don't know.
    let mut state = State::default();
    let id = ProfileId::new().to_string();
    state.profiles.insert(
        id.clone(),
        serde_json::json!({
            "id": id, "name": "future", "host": "h", "updated_ms": 5,
            "future_field": {"x": 1}
        }),
    );
    let key = SealKey::new(&pass("sync pass"), FAST).unwrap();
    let payload = Payload {
        format: FORMAT,
        written_by: "future".into(),
        written_ms: 5,
        state,
    };
    std::fs::write(
        dir.path().join(SYNC_FILE_NAME),
        key.seal(&serde_json::to_vec(&payload).unwrap(), PURPOSE)
            .unwrap(),
    )
    .unwrap();

    a.add("mine", "m");
    let r = a.sync_in(dir.path()).sync().unwrap();
    assert_eq!(r.added, vec!["future"]);
    assert!(r.uploaded);
    let p = decrypt(&dir.path().join(SYNC_FILE_NAME), "sync pass");
    assert_eq!(p.state.profiles[&id]["future_field"]["x"], 1);
    assert_eq!(p.state.profiles.len(), 2);

    // A file format from the future is refused, not misread.
    let newer = Payload {
        format: FORMAT + 1,
        written_by: String::new(),
        written_ms: 0,
        state: State::default(),
    };
    std::fs::write(
        dir.path().join(SYNC_FILE_NAME),
        key.seal(&serde_json::to_vec(&newer).unwrap(), PURPOSE)
            .unwrap(),
    )
    .unwrap();
    assert!(matches!(
        a.sync_in(dir.path()).sync(),
        Err(SyncError::TooNew(_))
    ));
}
