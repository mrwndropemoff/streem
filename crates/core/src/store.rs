//! Local SQLite storage. One file, `streem.db`, in the data directory.
//!
//! Every function here blocks; callers run them inside `tokio::task::spawn_blocking`.

use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, params};

use crate::settings::Settings;

use crate::LibraryItem;

/// Until sign-in exists, every row belongs to this **account**.
pub const LOCAL_ACCOUNT: &str = "local";
/// Until **profiles** exist, every library row belongs to this **profile**.
pub const LOCAL_PROFILE: &str = "local";

/// Applied in order. The schema version is the number of entries already applied
/// (`PRAGMA user_version`). Append new migrations at the end; never edit an old one.
const MIGRATIONS: &[&str] = &[
    // 1: the installed **addons**. `deleted`, `dirty` and `updated_at` are used by sync.
    "CREATE TABLE addons (
        account_id    TEXT    NOT NULL,
        url           TEXT    NOT NULL,
        position      INTEGER NOT NULL,
        manifest_json TEXT    NOT NULL,
        updated_at    INTEGER NOT NULL,
        deleted       INTEGER NOT NULL DEFAULT 0,
        dirty         INTEGER NOT NULL DEFAULT 0,
        PRIMARY KEY (account_id, url)
    )",
    // 2: the library. `deleted`, `dirty` and `updated_at` are used by sync.
    "CREATE TABLE library_items (
        account_id TEXT    NOT NULL,
        profile_id TEXT    NOT NULL,
        meta_id    TEXT    NOT NULL,
        meta_type  TEXT    NOT NULL,
        name       TEXT    NOT NULL,
        poster     TEXT,
        updated_at INTEGER NOT NULL,
        deleted    INTEGER NOT NULL DEFAULT 0,
        dirty      INTEGER NOT NULL DEFAULT 0,
        PRIMARY KEY (profile_id, meta_id)
    )",
    // 3: the **account**'s settings, one row per **account**. `dirty` and `updated_at` are
    // used by sync.
    "CREATE TABLE account_settings (
        account_id      TEXT PRIMARY KEY,
        real_debrid_key TEXT,
        audio_lang      TEXT,
        subtitle_lang   TEXT,
        updated_at      INTEGER,
        dirty           INTEGER
    )",
];

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("could not create the data folder: {0}")]
    Io(#[from] std::io::Error),
    #[error("local storage failed: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

/// An **addon** row as saved: its URL and the manifest text the **addon** served. The manifest
/// is empty while it has not been fetched yet: an **addon** whose URL has a placeholder,
/// installed while no Real-Debrid key was saved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredAddon {
    pub url: String,
    pub manifest_json: String,
}

#[derive(Clone)]
pub struct Store {
    connection: Arc<Mutex<Connection>>,
}

impl Store {
    /// Opens `streem.db` in `data_dir`, creating the folder and the tables if needed.
    pub fn open(data_dir: &Path) -> Result<Store, StoreError> {
        std::fs::create_dir_all(data_dir)?;
        let mut connection = Connection::open(data_dir.join("streem.db"))?;
        migrate(&mut connection)?;
        Ok(Store {
            connection: Arc::new(Mutex::new(connection)),
        })
    }

    fn lock(&self) -> MutexGuard<'_, Connection> {
        self.connection
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// True when no **addon** row exists at all, removed ones included.
    pub fn has_no_addon_rows(&self) -> Result<bool, StoreError> {
        let count: i64 = self
            .lock()
            .query_row("SELECT COUNT(*) FROM addons", [], |row| row.get(0))?;
        Ok(count == 0)
    }

    /// Installed (not removed) **addons** in display order.
    pub fn list_addons(&self) -> Result<Vec<StoredAddon>, StoreError> {
        let connection = self.lock();
        let mut statement = connection.prepare(
            "SELECT url, manifest_json FROM addons
             WHERE account_id = ?1 AND deleted = 0 ORDER BY position, url",
        )?;
        let rows = statement.query_map(params![LOCAL_ACCOUNT], |row| {
            Ok(StoredAddon {
                url: row.get(0)?,
                manifest_json: row.get(1)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Saves an **addon**. A new one goes to the end of the list; installing an
    /// **addon** that was removed brings its row back, at the end.
    pub fn upsert_addon(&self, url: &str, manifest_json: &str) -> Result<(), StoreError> {
        self.lock().execute(
            "INSERT INTO addons
                 (account_id, url, position, manifest_json, updated_at, deleted, dirty)
             VALUES (?1, ?2,
                 (SELECT COALESCE(MAX(position) + 1, 0) FROM addons WHERE account_id = ?1),
                 ?3, ?4, 0, 1)
             ON CONFLICT (account_id, url) DO UPDATE SET
                 position = CASE WHEN addons.deleted = 1 THEN excluded.position
                                 ELSE addons.position END,
                 manifest_json = excluded.manifest_json,
                 updated_at = excluded.updated_at,
                 deleted = 0,
                 dirty = 1",
            params![LOCAL_ACCOUNT, url, manifest_json, now_millis()],
        )?;
        Ok(())
    }

    /// The **account**'s settings; all unset when none were ever saved.
    pub fn load_settings(&self) -> Result<Settings, StoreError> {
        let settings = self
            .lock()
            .query_row(
                "SELECT real_debrid_key, audio_lang, subtitle_lang FROM account_settings
                 WHERE account_id = ?1",
                params![LOCAL_ACCOUNT],
                |row| {
                    Ok(Settings {
                        real_debrid_key: row.get(0)?,
                        audio_lang: row.get(1)?,
                        subtitle_lang: row.get(2)?,
                    })
                },
            )
            .optional()?;
        Ok(settings.unwrap_or_default())
    }

    /// Saves the Real-Debrid key, leaving the other settings as they are.
    pub fn save_real_debrid_key(&self, key: Option<&str>) -> Result<(), StoreError> {
        self.lock().execute(
            "INSERT INTO account_settings (account_id, real_debrid_key, updated_at, dirty)
             VALUES (?1, ?2, ?3, 1)
             ON CONFLICT (account_id) DO UPDATE SET
                 real_debrid_key = excluded.real_debrid_key,
                 updated_at = excluded.updated_at,
                 dirty = 1",
            params![LOCAL_ACCOUNT, key, now_millis()],
        )?;
        Ok(())
    }

    /// Saves the preferred audio and subtitle languages, leaving the key as it is.
    pub fn save_languages(
        &self,
        audio: Option<&str>,
        subtitle: Option<&str>,
    ) -> Result<(), StoreError> {
        self.lock().execute(
            "INSERT INTO account_settings
                 (account_id, audio_lang, subtitle_lang, updated_at, dirty)
             VALUES (?1, ?2, ?3, ?4, 1)
             ON CONFLICT (account_id) DO UPDATE SET
                 audio_lang = excluded.audio_lang,
                 subtitle_lang = excluded.subtitle_lang,
                 updated_at = excluded.updated_at,
                 dirty = 1",
            params![LOCAL_ACCOUNT, audio, subtitle, now_millis()],
        )?;
        Ok(())
    }

    /// Marks an **addon** as removed. The row stays so sync can tell other devices.
    pub fn remove_addon(&self, url: &str) -> Result<(), StoreError> {
        self.lock().execute(
            "UPDATE addons SET deleted = 1, dirty = 1, updated_at = ?3
             WHERE account_id = ?1 AND url = ?2",
            params![LOCAL_ACCOUNT, url, now_millis()],
        )?;
        Ok(())
    }

    /// The titles in the library (not removed ones), most recently added first.
    pub fn list_library(&self) -> Result<Vec<LibraryItem>, StoreError> {
        let connection = self.lock();
        let mut statement = connection.prepare(
            "SELECT meta_type, meta_id, name, poster FROM library_items
             WHERE profile_id = ?1 AND deleted = 0 ORDER BY updated_at DESC, meta_id",
        )?;
        let rows = statement.query_map(params![LOCAL_PROFILE], |row| {
            Ok(LibraryItem {
                meta_type: row.get(0)?,
                meta_id: row.get(1)?,
                name: row.get(2)?,
                poster: row.get(3)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Saves a title to the library, at the front. Adding a removed title brings its row back.
    pub fn add_to_library(&self, item: &LibraryItem) -> Result<(), StoreError> {
        let connection = self.lock();
        let updated_at = next_library_time(&connection)?;
        connection.execute(
            "INSERT INTO library_items
                 (account_id, profile_id, meta_id, meta_type, name, poster,
                  updated_at, deleted, dirty)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, 1)
             ON CONFLICT (profile_id, meta_id) DO UPDATE SET
                 meta_type = excluded.meta_type,
                 name = excluded.name,
                 poster = excluded.poster,
                 updated_at = excluded.updated_at,
                 deleted = 0,
                 dirty = 1",
            params![
                LOCAL_ACCOUNT,
                LOCAL_PROFILE,
                item.meta_id,
                item.meta_type,
                item.name,
                item.poster,
                updated_at,
            ],
        )?;
        Ok(())
    }

    /// Marks a title as removed from the library. The row stays so sync can tell other devices.
    pub fn remove_from_library(&self, meta_id: &str) -> Result<(), StoreError> {
        let connection = self.lock();
        let updated_at = next_library_time(&connection)?;
        connection.execute(
            "UPDATE library_items SET deleted = 1, dirty = 1, updated_at = ?3
             WHERE profile_id = ?1 AND meta_id = ?2 AND deleted = 0",
            params![LOCAL_PROFILE, meta_id, updated_at],
        )?;
        Ok(())
    }
}

/// The `updated_at` for a library write: now, but always later than every earlier write, so
/// two titles added within the same millisecond still list in the order they were added.
fn next_library_time(connection: &Connection) -> Result<i64, StoreError> {
    let latest: Option<i64> = connection.query_row(
        "SELECT MAX(updated_at) FROM library_items WHERE profile_id = ?1",
        params![LOCAL_PROFILE],
        |row| row.get(0),
    )?;
    Ok(latest.map_or(now_millis(), |latest| now_millis().max(latest + 1)))
}

fn migrate(connection: &mut Connection) -> Result<(), StoreError> {
    let applied: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    for (index, sql) in MIGRATIONS.iter().enumerate().skip(applied as usize) {
        let transaction = connection.transaction()?;
        transaction.execute_batch(sql)?;
        transaction.pragma_update(None, "user_version", index as i64 + 1)?;
        transaction.commit()?;
    }
    Ok(())
}

/// Milliseconds since 1970.
fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        (dir, store)
    }

    fn flags(store: &Store, url: &str) -> (i64, i64) {
        store
            .lock()
            .query_row(
                "SELECT deleted, dirty FROM addons WHERE url = ?1",
                [url],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap()
    }

    #[test]
    fn new_addons_are_listed_in_install_order() {
        let (_dir, store) = open();
        store.upsert_addon("https://b/manifest.json", "{}").unwrap();
        store.upsert_addon("https://a/manifest.json", "{}").unwrap();
        let urls: Vec<_> = store
            .list_addons()
            .unwrap()
            .into_iter()
            .map(|a| a.url)
            .collect();
        assert_eq!(urls, ["https://b/manifest.json", "https://a/manifest.json"]);
    }

    #[test]
    fn removing_keeps_the_row_and_marks_it() {
        let (_dir, store) = open();
        let url = "https://a/manifest.json";
        store.upsert_addon(url, "{}").unwrap();
        store
            .lock()
            .execute("UPDATE addons SET dirty = 0", [])
            .unwrap();

        store.remove_addon(url).unwrap();

        assert!(store.list_addons().unwrap().is_empty());
        assert!(!store.has_no_addon_rows().unwrap());
        assert_eq!(flags(&store, url), (1, 1));
    }

    #[test]
    fn installing_again_brings_the_row_back_at_the_end() {
        let (_dir, store) = open();
        store.upsert_addon("https://a/manifest.json", "{}").unwrap();
        store.upsert_addon("https://b/manifest.json", "{}").unwrap();
        store.remove_addon("https://a/manifest.json").unwrap();
        store.upsert_addon("https://a/manifest.json", "{}").unwrap();

        let urls: Vec<_> = store
            .list_addons()
            .unwrap()
            .into_iter()
            .map(|a| a.url)
            .collect();
        assert_eq!(urls, ["https://b/manifest.json", "https://a/manifest.json"]);
        assert_eq!(flags(&store, "https://a/manifest.json"), (0, 1));
    }

    #[test]
    fn schema_version_is_recorded_and_reopening_keeps_data() {
        let dir = tempfile::tempdir().unwrap();
        Store::open(dir.path())
            .unwrap()
            .upsert_addon("https://a/manifest.json", "{}")
            .unwrap();

        let store = Store::open(dir.path()).unwrap();
        let version: i64 = store
            .lock()
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, MIGRATIONS.len() as i64);
        assert_eq!(store.list_addons().unwrap().len(), 1);
    }

    #[test]
    fn a_database_from_the_library_build_gets_the_settings_table() {
        let dir = tempfile::tempdir().unwrap();
        {
            // What the build before account settings left: migrations 1 and 2.
            let mut connection = Connection::open(dir.path().join("streem.db")).unwrap();
            let transaction = connection.transaction().unwrap();
            for sql in &MIGRATIONS[..2] {
                transaction.execute_batch(sql).unwrap();
            }
            transaction.pragma_update(None, "user_version", 2).unwrap();
            transaction.commit().unwrap();
        }

        let store = Store::open(dir.path()).unwrap();
        store.save_real_debrid_key(Some("KEY")).unwrap();
        assert_eq!(
            store.load_settings().unwrap().real_debrid_key.as_deref(),
            Some("KEY")
        );
    }

    #[test]
    fn settings_start_unset_and_each_save_keeps_the_other_columns() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        assert_eq!(store.load_settings().unwrap(), Settings::default());

        store.save_languages(Some("eng"), Some("fre")).unwrap();
        store.save_real_debrid_key(Some("KEY")).unwrap();
        store.save_languages(Some("jpn"), None).unwrap();

        let reopened = Store::open(dir.path()).unwrap();
        let expected = Settings {
            real_debrid_key: Some("KEY".into()),
            audio_lang: Some("jpn".into()),
            subtitle_lang: None,
        };
        assert_eq!(reopened.load_settings().unwrap(), expected);
        let dirty: i64 = reopened
            .lock()
            .query_row("SELECT dirty FROM account_settings", [], |row| row.get(0))
            .unwrap();
        assert_eq!(dirty, 1);

        reopened.save_real_debrid_key(None).unwrap();
        assert_eq!(reopened.load_settings().unwrap().real_debrid_key, None);
    }

    fn film(meta_id: &str) -> LibraryItem {
        LibraryItem {
            meta_type: "movie".into(),
            meta_id: meta_id.into(),
            name: format!("Film {meta_id}"),
            poster: None,
        }
    }

    /// `(account_id, profile_id, deleted, dirty, updated_at)` of a library row.
    fn library_row(store: &Store, meta_id: &str) -> (String, String, i64, i64, i64) {
        store
            .lock()
            .query_row(
                "SELECT account_id, profile_id, deleted, dirty, updated_at
                 FROM library_items WHERE meta_id = ?1",
                [meta_id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .unwrap()
    }

    #[test]
    fn adding_to_the_library_saves_a_dirty_local_row() {
        let (_dir, store) = open();
        store.add_to_library(&film("tt1")).unwrap();

        let (account, profile, deleted, dirty, updated_at) = library_row(&store, "tt1");
        assert_eq!((account.as_str(), profile.as_str()), ("local", "local"));
        assert_eq!((deleted, dirty), (0, 1));
        assert!(
            updated_at > 1_600_000_000_000,
            "{updated_at} is not in milliseconds"
        );
    }

    #[test]
    fn removing_from_the_library_keeps_the_row_and_marks_it() {
        let (_dir, store) = open();
        store.add_to_library(&film("tt1")).unwrap();
        let (.., added_at) = library_row(&store, "tt1");
        store
            .lock()
            .execute("UPDATE library_items SET dirty = 0", [])
            .unwrap();

        store.remove_from_library("tt1").unwrap();

        assert!(store.list_library().unwrap().is_empty());
        let (_, _, deleted, dirty, removed_at) = library_row(&store, "tt1");
        assert_eq!((deleted, dirty), (1, 1));
        assert!(removed_at > added_at);
    }

    #[test]
    fn adding_a_removed_title_again_brings_its_row_back_at_the_front() {
        let (_dir, store) = open();
        store.add_to_library(&film("tt1")).unwrap();
        store.add_to_library(&film("tt2")).unwrap();
        store.remove_from_library("tt1").unwrap();
        store.add_to_library(&film("tt1")).unwrap();

        let ids: Vec<_> = store
            .list_library()
            .unwrap()
            .into_iter()
            .map(|item| item.meta_id)
            .collect();
        assert_eq!(ids, ["tt1", "tt2"]);
        let (_, _, deleted, dirty, _) = library_row(&store, "tt1");
        assert_eq!((deleted, dirty), (0, 1));
    }
}
