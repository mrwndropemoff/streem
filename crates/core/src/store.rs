//! Local SQLite storage. One file, `streem.db`, in the data directory.
//!
//! Every function here blocks; callers run them inside `tokio::task::spawn_blocking`.

use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, params};

/// Until sign-in exists, every row belongs to this **account**.
pub const LOCAL_ACCOUNT: &str = "local";

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
];

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("could not create the data folder: {0}")]
    Io(#[from] std::io::Error),
    #[error("local storage failed: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

/// An **addon** row as saved: its URL and the manifest text the **addon** served.
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

    /// Marks an **addon** as removed. The row stays so sync can tell other devices.
    pub fn remove_addon(&self, url: &str) -> Result<(), StoreError> {
        self.lock().execute(
            "UPDATE addons SET deleted = 1, dirty = 1, updated_at = ?3
             WHERE account_id = ?1 AND url = ?2",
            params![LOCAL_ACCOUNT, url, now_millis()],
        )?;
        Ok(())
    }
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
}
