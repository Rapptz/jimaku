use quick_cache::sync::Cache;
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::RwLockReadGuard;

use crate::{
    auth::hash_password,
    cached::TimedCachedValue,
    models::{Account, DirectoryEntry},
    Config, Database,
};

struct InnerState {
    config: Config,
    database: Database,
    cached_directories: TimedCachedValue<Vec<DirectoryEntry>>,
    cached_users: Cache<i64, Account>,
}

/// Global application state for the axum Router.
#[derive(Clone)]
pub struct AppState {
    inner: Arc<InnerState>,
    pub client: reqwest::Client,
    pub incorrect_default_password_hash: String,
}

impl AppState {
    pub fn new(config: Config, database: Database) -> Self {
        let incorrect_default_password_hash =
            hash_password("incorrect-default-password").expect("could not hash default password");
        Self {
            inner: Arc::new(InnerState {
                config,
                database,
                cached_directories: TimedCachedValue::new(Duration::from_secs(60 * 30)),
                cached_users: Cache::new(1000),
            }),
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(600))
                .build()
                .expect("could not build HTTP client"),
            incorrect_default_password_hash,
        }
    }

    pub fn config(&self) -> &Config {
        &self.inner.config
    }

    pub fn database(&self) -> &Database {
        &self.inner.database
    }

    /// Sends an alert webhook with the given webhook payload.
    ///
    /// This sends the request in the background so there's no way to detect
    /// if it failed or not.
    pub fn send_alert<T: serde::Serialize + Send + 'static>(&self, payload: T) {
        if let Some(wh) = self.config().webhook.clone() {
            let client = self.client.clone();
            tokio::spawn(async move { wh.prepare(payload).send(&client).await });
        }
    }

    pub fn cached_directories(&self) -> &TimedCachedValue<Vec<DirectoryEntry>> {
        &self.inner.cached_directories
    }

    pub async fn get_account(&self, id: i64) -> Option<Account> {
        match self.inner.cached_users.get_value_or_guard_async(&id).await {
            Ok(acc) => Some(acc),
            Err(guard) => match self.database().get_by_id::<Account>(id).await.ok().flatten() {
                Some(account) => {
                    let _ = guard.insert(account.clone());
                    Some(account)
                }
                None => None,
            },
        }
    }

    pub fn invalidate_account_cache(&self, id: i64) {
        self.inner.cached_users.remove(&id);
    }

    pub fn clear_account_cache(&self) {
        self.inner.cached_users.clear();
    }

    pub async fn directory_entries(&self) -> RwLockReadGuard<'_, Vec<DirectoryEntry>> {
        {
            let reader = self.inner.cached_directories.get().await;
            if let Some(lock) = reader {
                return lock;
            }
        }

        // Cache miss
        let entries = self
            .database()
            .all("SELECT * FROM directory_entry ORDER BY name ASC", [])
            .await
            .unwrap_or_default();
        self.inner.cached_directories.set(entries).await
    }

    /// Gets the directory by ID via cache, if available.
    ///
    /// If not found in cache then it calls the database.
    /// This incurs the cost of one clone regardless of the case.
    ///
    /// All errors are coerced into None.
    pub async fn get_directory_entry(&self, id: i64) -> Option<DirectoryEntry> {
        if let Some(guard) = self.cached_directories().get().await {
            let found = guard.iter().find(|x| x.id == id);
            // Cache hit, return a copy
            if found.is_some() {
                return found.cloned();
            }
        }

        self.database().get_by_id(id).await.ok().flatten()
    }

    /// Gets the directory entry's path.
    ///
    /// This is a small optimisation to avoid cloning the entire [`DirectoryEntry`] struct
    /// when the only thing needed is the path.
    pub async fn get_directory_entry_path(&self, id: i64) -> Option<PathBuf> {
        if let Some(guard) = self.cached_directories().get().await {
            let found = guard.iter().find(|x| x.id == id);
            // Cache hit, return a copy
            if let Some(hit) = found {
                return Some(hit.path.clone());
            }
        }

        self.database()
            .get_row("SELECT path FROM directory_entry WHERE id = ?", [id], |row| {
                let str: String = row.get("path")?;
                Ok(PathBuf::from(str))
            })
            .await
            .ok()
    }

    /// Gets the directory entry's path by its AniList ID.
    pub async fn get_anilist_directory_entry_path(&self, id: u32) -> Option<PathBuf> {
        if let Some(guard) = self.cached_directories().get().await {
            let found = guard.iter().find(|x| x.anilist_id == Some(id));
            // Cache hit, return a copy
            if let Some(hit) = found {
                return Some(hit.path.clone());
            }
        }

        self.database()
            .get_row("SELECT path FROM directory_entry WHERE anilist_id = ?", [id], |row| {
                let str: String = row.get("path")?;
                Ok(PathBuf::from(str))
            })
            .await
            .ok()
    }
}
