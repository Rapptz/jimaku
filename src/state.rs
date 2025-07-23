use quick_cache::sync::Cache;
use std::{collections::HashSet, path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::{RwLock, RwLockReadGuard};

use crate::{
    audit::AuditLogEntry,
    auth::hash_password,
    cached::TimedCachedValue,
    database::Table,
    logging::RequestLogger,
    models::{Account, DirectoryEntry, Session},
    notification::NotificationService,
    relations::Relations,
    token::MAX_TOKEN_AGE,
    Config, Database,
};

#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub struct SessionInfo {
    pub id: i64,
    pub api_key: bool,
    pub created_at: time::OffsetDateTime,
}

impl From<Session> for SessionInfo {
    fn from(value: Session) -> Self {
        Self {
            id: value.account_id,
            api_key: value.api_key,
            created_at: value.created_at,
        }
    }
}

impl SessionInfo {
    /// Returns `true` if the session is expired
    pub fn is_expired(&self) -> bool {
        !self.api_key && time::OffsetDateTime::now_utc() > (self.created_at + MAX_TOKEN_AGE)
    }
}

struct InnerState {
    config: Config,
    database: Database,
    cached_directories: TimedCachedValue<Vec<DirectoryEntry>>,
    relations: RwLock<Relations>,
    cached_users: Cache<i64, Account>,
    valid_sessions: Cache<String, SessionInfo>,
}

/// Global application state for the axum Router.
#[derive(Clone)]
pub struct AppState {
    inner: Arc<InnerState>,
    pub client: reqwest::Client,
    pub requests: RequestLogger,
    pub notifications: NotificationService,
    pub incorrect_default_password_hash: String,
}

impl AppState {
    pub async fn new(config: Config, database: Database) -> Self {
        let incorrect_default_password_hash =
            hash_password("incorrect-default-password").expect("could not hash default password");
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(600))
            .build()
            .expect("could not build HTTP client");

        let requests = RequestLogger::new().expect("could not build request logger");
        let notifications = NotificationService::new().expect("could not start notification service");
        Self {
            inner: Arc::new(InnerState {
                config,
                database,
                relations: RwLock::new(Relations::load(&client).await.unwrap_or_default()),
                cached_directories: TimedCachedValue::new(Duration::from_secs(60 * 30)),
                cached_users: Cache::new(1000),
                valid_sessions: Cache::new(1000),
            }),
            client,
            requests,
            notifications,
            incorrect_default_password_hash,
        }
    }

    pub fn config(&self) -> &Config {
        &self.inner.config
    }

    pub fn database(&self) -> &Database {
        &self.inner.database
    }

    /// Sends an audit log entry.
    ///
    /// Errors are silently dropped, since they can't be handled anyway.
    pub async fn audit(&self, entry: AuditLogEntry) {
        let err = self
            .database()
            .execute(
                "INSERT INTO audit_log(id, entry_id, account_id, data) VALUES (?, ?, ?, ?)",
                (entry.id, entry.entry_id, entry.account_id, entry.data),
            )
            .await;

        if let Err(e) = err {
            tracing::error!(error=%e, "Could not insert audit log entry");
        }
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

    pub fn clear_session_cache(&self) {
        self.inner.valid_sessions.clear();
    }

    /// Returns if the session is valid (i.e. in the database or cache).
    pub async fn is_session_valid(&self, session: &str) -> Option<SessionInfo> {
        match self.inner.valid_sessions.get_value_or_guard_async(session).await {
            Ok(info) => {
                if info.is_expired() {
                    self.invalidate_session(session).await;
                    None
                } else {
                    Some(info)
                }
            }
            Err(guard) => match self
                .database()
                .get_by_id::<Session>(session.to_owned())
                .await
                .ok()
                .flatten()
            {
                Some(info) => {
                    let info = SessionInfo::from(info);
                    if info.is_expired() {
                        self.invalidate_session(session).await;
                        None
                    } else {
                        let _ = guard.insert(info);
                        Some(info)
                    }
                }
                None => None,
            },
        }
    }

    /// Returns the account associated with the session and account ID if they're valid.
    ///
    /// This is merely an optimisation to avoid doing multiple database lookups.
    pub async fn get_session_account(&self, session: &str, id: i64, api_key: bool) -> Option<Account> {
        match self.inner.valid_sessions.get_value_or_guard_async(session).await {
            Ok(info) => {
                if info.is_expired() {
                    self.invalidate_session(session).await;
                    return None;
                }
                let account = self.get_account(info.id).await;
                if account.is_none() {
                    self.inner.valid_sessions.remove(session);
                }
                account
            }
            Err(guard) => {
                let query = r#"
                    SELECT account.id AS id, account.name AS name, account.password AS password,
                           account.flags AS flags, session.api_key AS api_key, session.created_at AS created_at,
                           account.anilist_username AS anilist_username,
                           account.notification_ack AS notification_ack
                    FROM account INNER JOIN session ON session.account_id = account.id
                    WHERE session.id = ? AND session.account_id = ? AND session.api_key = ?
                "#;
                match self
                    .database()
                    .get_row(
                        query,
                        (session.to_owned(), id, api_key),
                        |row| -> rusqlite::Result<(Account, SessionInfo)> {
                            let account = Account::from_row(row)?;
                            let info = SessionInfo {
                                id: account.id,
                                api_key: row.get("api_key")?,
                                created_at: row.get("created_at")?,
                            };
                            Ok((account, info))
                        },
                    )
                    .await
                    .ok()
                {
                    Some((account, info)) => {
                        if info.is_expired() {
                            self.invalidate_session(session).await;
                            None
                        } else {
                            let _ = guard.insert(info);
                            self.inner.cached_users.insert(account.id, account.clone());
                            Some(account)
                        }
                    }
                    None => None,
                }
            }
        }
    }

    /// Invalidate the given session
    ///
    /// This can invalidate API tokens as well.
    pub async fn invalidate_session(&self, session: &str) -> bool {
        self.inner.valid_sessions.remove(session);
        self.database()
            .execute("DELETE FROM session WHERE id = ?", (session.to_owned(),))
            .await
            .is_ok()
    }

    /// Saves the session given by the token to the database
    pub async fn save_session(&self, token: &crate::token::Token, description: Option<String>) {
        let query =
            "INSERT INTO session(id, account_id, description, api_key) VALUES (?, ?, ?, ?) ON CONFLICT DO NOTHING";
        let _ = self
            .database()
            .execute(query, (token.base64(), token.id, description, token.api_key))
            .await;
    }

    pub async fn invalidate_api_keys(&self, id: i64) {
        let sessions: Vec<Session> = self
            .database()
            .all(
                "DELETE FROM session WHERE account_id = ? AND api_key != 0 RETURNING *",
                [id],
            )
            .await
            .unwrap_or_default();

        for session in sessions {
            self.inner.valid_sessions.remove(&session.id);
        }
    }

    pub async fn generate_api_key(&self, id: i64) -> anyhow::Result<String> {
        let mut token = crate::token::Token::new(id)?;
        token.api_key = true;
        let key = token.base64();
        self.database()
            .execute(
                "INSERT INTO session(id, account_id, description, api_key) VALUES (?, ?, 'API Key', 1)",
                (key.clone(), id),
            )
            .await?;
        Ok(key)
    }

    pub async fn get_api_key(&self, id: i64) -> Option<String> {
        self.database()
            .get_row(
                "SELECT id FROM session WHERE account_id = ? AND api_key = 1",
                [id],
                |row| row.get("id"),
            )
            .await
            .ok()
    }

    /// Invalidate all sessions used by the account.
    ///
    /// This does *not* invalidate API tokens.
    pub async fn invalidate_account_sessions(&self, id: i64) {
        let sessions: Vec<Session> = self
            .database()
            .all(
                "DELETE FROM session WHERE account_id = ? AND api_key = 0 RETURNING *",
                [id],
            )
            .await
            .unwrap_or_default();

        for session in sessions {
            self.inner.valid_sessions.remove(&session.id);
        }
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

    /// Gets the directory entry's path by its TMDB ID.
    pub async fn get_tmdb_directory_entry_path(&self, id: crate::tmdb::Id) -> Option<PathBuf> {
        if let Some(guard) = self.cached_directories().get().await {
            let found = guard.iter().find(|x| x.tmdb_id == Some(id));
            // Cache hit, return a copy
            if let Some(hit) = found {
                return Some(hit.path.clone());
            }
        }

        self.database()
            .get_row("SELECT path FROM directory_entry WHERE tmdb_id = ?", [id], |row| {
                let str: String = row.get("path")?;
                Ok(PathBuf::from(str))
            })
            .await
            .ok()
    }

    /// Returns the anime relations bound to this state
    pub async fn anime_relations(&self) -> RwLockReadGuard<'_, Relations> {
        self.inner.relations.read().await
    }

    /// Updates the anime relations to the one given
    pub async fn set_anime_relations(&self, relations: Relations) {
        let mut guard = self.inner.relations.write().await;
        *guard = relations;
    }

    /// Returns true if the given entry ID is bookmarked by the user.
    pub async fn is_bookmarked(&self, user_id: i64, entry_id: i64) -> bool {
        self.database()
            .get_row(
                "SELECT 1 FROM bookmark WHERE user_id = ? AND entry_id = ?",
                (user_id, entry_id),
                |row| Ok(row.get_ref(0).is_ok()),
            )
            .await
            .unwrap_or_default()
    }

    /// Returns the bookmarked entry IDs by the user
    pub async fn bookmarked_ids(&self, user_id: i64) -> rusqlite::Result<HashSet<i64>> {
        self.database()
            .call(move |conn| -> rusqlite::Result<_> {
                let mut result = HashSet::new();
                let query = "SELECT entry_id FROM bookmark WHERE user_id = ?";
                let mut stmt = conn.prepare_cached(query)?;
                let mut rows = stmt.query((user_id,))?;
                while let Some(row) = rows.next()? {
                    result.insert(row.get(0)?);
                }
                Ok(result)
            })
            .await
    }

    /// Returns the notification count for a user
    pub async fn get_notification_count(&self, account: &Account) -> u64 {
        self.database()
            .get_row(
                "SELECT COUNT(*) FROM notification WHERE user_id = ? AND ts > ?",
                (account.id, account.notification_ack.unwrap_or_default()),
                |row| row.get(0),
            )
            .await
            .ok()
            .flatten()
            .unwrap_or_default()
    }

    /// Updates the notification ack on the account
    pub async fn update_notification_ack(&self, account: &Account) {
        let _ = self
            .database()
            .execute(
                "UPDATE account SET notification_ack = ? WHERE id = ?",
                (crate::utils::unix_now_ms(), account.id),
            )
            .await;
        self.invalidate_account_cache(account.id);
    }
}
