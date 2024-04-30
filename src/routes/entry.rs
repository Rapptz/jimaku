use crate::anilist::{self, MediaTitle};
use crate::database::{is_unique_constraint_violation, Table};
use crate::download::{validate_path, DownloadResponse};
use crate::error::{ApiError, ApiErrorCode, InternalError};
use crate::flash::{FlashMessage, Flasher, Flashes};
use crate::headers::Referrer;
use crate::models::{Account, AccountCheck, DirectoryEntry, EntryFlags};
use crate::ratelimit::RateLimit;
use crate::utils::{is_over_length, FRAGMENT};
use crate::{audit, filters};
use crate::{tmdb, AppState};
use anyhow::{bail, Context};
use askama::Template;
use axum::body::{Body, Bytes};
use axum::extract::multipart::Field;
use axum::extract::{Json, Multipart, Query};
use axum::http::header::{CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_TYPE};
use axum::http::{HeaderName, HeaderValue};
use axum::response::Redirect;
use axum::routing::{delete, get, post};
use axum::{
    extract::{Form, Path, Request, State},
    response::{IntoResponse, Response},
    Router,
};
use percent_encoding::percent_encode;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::PathBuf;
use time::OffsetDateTime;
use tokio::task::JoinSet;
use tower::ServiceExt;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeFile;
use utoipa::ToSchema;

/// Represents a file entry, e.g. a subtitle or a ZIP file or whatever else.
#[derive(Debug, Serialize, ToSchema)]
pub(crate) struct FileEntry {
    /// The file's download URL.
    pub(crate) url: String,
    /// The file's name.
    pub(crate) name: String,
    /// The file's size in bytes.
    pub(crate) size: u64,
    /// The date the file was last modified, in UTC, as an RFC3339 string.
    #[serde(with = "time::serde::rfc3339")]
    pub(crate) last_modified: OffsetDateTime,
}

#[derive(Template)]
#[template(path = "entry.html")]
struct EntryTemplate {
    account: Option<Account>,
    entry: DirectoryEntry,
    files: Vec<FileEntry>,
    flashes: Flashes,
}

pub(crate) fn get_file_entries(entry_id: i64, path: &std::path::Path) -> std::io::Result<Vec<FileEntry>> {
    let mut entries = Vec::new();
    for file in path.read_dir()? {
        let entry = file?;
        let filename = entry.file_name();
        let Some(filename) = filename.to_str() else {
            continue;
        };

        let Ok(metadata) = entry.metadata() else { continue };
        let last_modified = if let Ok(time) = metadata.modified() {
            time.into()
        } else {
            OffsetDateTime::UNIX_EPOCH
        };

        let url = format!(
            "/entry/{entry_id}/download/{}",
            percent_encode(filename.as_bytes(), FRAGMENT)
        );
        entries.push(FileEntry {
            url,
            name: filename.into(),
            size: metadata.len(),
            last_modified,
        });
    }
    Ok(entries)
}

async fn get_entry(
    State(state): State<AppState>,
    Path(entry_id): Path<i64>,
    account: Option<Account>,
    flashes: Flashes,
) -> Result<Response, InternalError> {
    let Some(entry) = state.get_directory_entry(entry_id).await else {
        return Ok(Redirect::to("/").into_response());
    };
    let files = get_file_entries(entry_id, &entry.path)?;
    Ok(EntryTemplate {
        account,
        entry,
        files,
        flashes,
    }
    .into_response())
}

async fn download_entry(
    State(state): State<AppState>,
    Path((entry_id, filename)): Path<(i64, String)>,
    req: Request,
) -> DownloadResponse {
    let Some(base) = state.get_directory_entry_path(entry_id).await else {
        return DownloadResponse::NotFound;
    };

    let Some(path) = validate_path(&base, filename.as_str()) else {
        return DownloadResponse::NotFound;
    };

    match ServeFile::new(path).oneshot(req).await {
        Ok(res) => DownloadResponse::File(res.map(axum::body::Body::new)),
        Err(_) => DownloadResponse::NotFound,
    }
}

#[derive(Debug, Deserialize)]
struct CreateDirectoryEntry {
    #[serde(deserialize_with = "crate::utils::empty_string_is_none")]
    #[serde(default)]
    anilist_url: Option<String>,
    #[serde(deserialize_with = "crate::utils::empty_string_is_none")]
    #[serde(default)]
    tmdb_url: Option<String>,
    #[serde(deserialize_with = "crate::utils::empty_string_is_none")]
    #[serde(default)]
    name: Option<String>,
    #[serde(default = "crate::utils::default_true")]
    anime: bool,
}

#[derive(Debug, Default)]
pub struct PendingDirectoryEntry {
    pub anilist_id: Option<u32>,
    pub tmdb_id: Option<tmdb::Id>,
    pub name: Option<String>,
    pub titles: Option<MediaTitle>,
    pub flags: Option<EntryFlags>,
    pub notes: Option<String>,
    pub anime: bool,
}

impl From<CreateDirectoryEntry> for PendingDirectoryEntry {
    fn from(value: CreateDirectoryEntry) -> Self {
        Self {
            anilist_id: value.anilist_url.as_deref().and_then(crate::utils::get_anilist_id),
            tmdb_id: value.tmdb_url.as_deref().and_then(tmdb::get_tmdb_id),
            name: value.name,
            anime: value.anime,
            notes: None,
            titles: None,
            flags: None,
        }
    }
}

impl PendingDirectoryEntry {
    async fn get_info(&self, state: &AppState) -> anyhow::Result<Option<(MediaTitle, EntryFlags)>> {
        if let Some((title, flags)) = self.titles.as_ref().zip(self.flags) {
            return Ok(Some((title.clone(), flags)));
        }
        match self.anilist_id {
            Some(id) => {
                let media = anilist::search_by_id(&state.client, id)
                    .await
                    .with_context(|| "AniList returned an error. Please try again later.".to_owned())?
                    .with_context(|| "AniList did not return results for this URL.".to_owned())?;
                let mut flags = EntryFlags::new();
                flags.set_anime(self.anime);
                flags.set_movie(media.is_movie());
                flags.set_adult(media.adult);
                Ok(Some((media.title, flags)))
            }
            None => {
                if let Some(id) = self.tmdb_id {
                    let info = tmdb::get_media_info(&state.client, &state.config().tmdb_api_key, id)
                        .await
                        .with_context(|| "TMDB returned an error. Please try again later.".to_owned())?
                        .with_context(|| "TMDB did not return results for this URL.".to_owned())?;

                    let mut flags = EntryFlags::new();
                    flags.set_anime(self.anime);
                    flags.set_movie(id.is_movie());
                    flags.set_adult(info.is_adult());
                    Ok(Some((info.titles(), flags)))
                } else {
                    Ok(None)
                }
            }
        }
    }

    fn path(&self, name: &str, anime: bool, state: &AppState) -> PathBuf {
        // Series names aren't unique but directory names are
        // So try to give it some noise depending on the anilist ID or tmdb ID
        // This ordeal could also be entirely avoided by just using numeric folder names
        // But having human readable folder names is fine

        // A prefix is used for the flat directory structure since it's easier to
        // reason about in the code.
        let prefix = if !anime { "[drama] " } else { "" };
        let directory_name = if let Some(id) = self.anilist_id {
            sanitise_file_name::sanitise(&format!("{prefix}{name} [{id}]"))
        } else if let Some(id) = self.tmdb_id {
            sanitise_file_name::sanitise(&format!("{prefix}{name} [{id}]"))
        } else {
            // Avoid the extra allocation if possible
            if anime {
                sanitise_file_name::sanitise(name)
            } else {
                sanitise_file_name::sanitise(&format!("[drama] {name}"))
            }
        };

        state.config().subtitle_path.join(directory_name)
    }
}

pub async fn raw_create_directory_entry(
    state: &AppState,
    account: Account,
    pending: PendingDirectoryEntry,
    api: bool,
) -> Result<(i64, PathBuf), ApiError> {
    let creator_id = account.id;

    let (names, flags) = match pending.get_info(state).await? {
        Some(title) => title,
        None if account.flags.is_editor() => {
            if let Some(name) = pending.name.clone() {
                let mut flags = EntryFlags::new();
                flags.set_anime(pending.anime);
                (MediaTitle::new(name), flags)
            } else {
                return Err(ApiError::new("Missing name, anilist_id, or tmdb_id for directory."));
            }
        }
        None => return Err(ApiError::new("Missing anilist_id or tmdb_id for directory.")),
    };

    let path = pending.path(&names.romaji, pending.anime, state);
    if path.exists() {
        return Err(ApiError::new("Path already exists.").with_code(ApiErrorCode::EntryAlreadyExists));
    }

    let Some(path_string) = path.to_str() else {
        return Err(ApiError::new("Resulting path was not UTF-8."));
    };

    let query = r#"
        INSERT INTO directory_entry(path, creator_id, tmdb_id, anilist_id, flags, notes, name, english_name, japanese_name)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
        RETURNING id;
    "#;
    let path_string = path_string.to_owned();
    let romaji = names.romaji.clone();
    let response = state
        .database()
        .call(move |con| -> Result<(i64, PathBuf), ApiError> {
            let tx = con.transaction()?;
            let result: rusqlite::Result<i64> = {
                let mut stmt = tx.prepare_cached(query)?;
                stmt.query_row(
                    (
                        path_string.to_owned(),
                        creator_id,
                        pending.tmdb_id,
                        pending.anilist_id,
                        flags,
                        pending.notes,
                        names.romaji,
                        names.english,
                        names.native,
                    ),
                    |row| row.get("id"),
                )
            };

            let url = match result {
                Ok(entry_id) => {
                    std::fs::create_dir(&path).map_err(|_| {
                        ApiError::new(format!("Could not create directory {}", path.display()))
                            .with_code(ApiErrorCode::ServerError)
                    })?;
                    (entry_id, path)
                }
                Err(e) if is_unique_constraint_violation(&e) => {
                    return Err(ApiError::new("Entry already exists.").with_code(ApiErrorCode::EntryAlreadyExists))
                }
                Err(e) => return Err(e.into()),
            };

            tx.commit()?;
            Ok(url)
        })
        .await;

    if let Ok((entry_id, _)) = &response {
        let audit_data = audit::CreateEntry {
            anime: pending.anime,
            api,
            name: romaji.clone(),
            tmdb_id: pending.tmdb_id,
            anilist_id: pending.anilist_id,
        };
        state
            .audit(audit::AuditLogEntry::full(audit_data, *entry_id, account.id))
            .await;
        let anilist_url = match pending.anilist_id {
            Some(id) => format!("https://anilist.co/anime/{id}"),
            None => String::from("Unknown"),
        };
        let tmdb_url = match pending.tmdb_id {
            Some(id) => id.url(),
            None => String::from("Unknown"),
        };
        let title = if api {
            format!("[API] New Entry: {romaji}")
        } else {
            format!("New Entry: {romaji}")
        };
        state.send_alert(
            crate::discord::Alert::success(title)
                .url(format!("/entry/{entry_id}"))
                .account(account)
                .field("Anime", pending.anime)
                .field("AniList URL", anilist_url)
                .field("TMDB URL", tmdb_url),
        );
        state.cached_directories().invalidate().await;
    }
    response
}

async fn create_directory_entry(
    State(state): State<AppState>,
    account: Account,
    flasher: Flasher,
    Referrer(url): Referrer,
    Form(payload): Form<CreateDirectoryEntry>,
) -> Response {
    let response = raw_create_directory_entry(&state, account, payload.into(), false).await;
    match response {
        Ok((entry_id, _)) => Redirect::to(&format!("/entry/{entry_id}")).into_response(),
        Err(e) => flasher.add(e.error.as_ref()).bail(&url),
    }
}

#[derive(Deserialize)]
struct EditDirectoryEntry {
    name: String,
    #[serde(deserialize_with = "crate::utils::empty_string_is_none")]
    japanese_name: Option<String>,
    #[serde(deserialize_with = "crate::utils::empty_string_is_none")]
    english_name: Option<String>,
    #[serde(deserialize_with = "anilist_id_or_url")]
    anilist_id: Option<u32>,
    #[serde(deserialize_with = "crate::utils::empty_string_is_none")]
    notes: Option<String>,
    #[serde(rename = "tmdb_url", deserialize_with = "tmdb_url")]
    tmdb_id: Option<tmdb::Id>,
    #[serde(default)]
    low_quality: bool,
    #[serde(default)]
    adult: bool,
    #[serde(default)]
    movie: bool,
    #[serde(default)]
    anime: bool,
}

impl EditDirectoryEntry {
    fn apply_flags(&self, mut flags: EntryFlags) -> EntryFlags {
        flags.set_low_quality(self.low_quality);
        flags.set_adult(self.adult);
        flags.set_movie(self.movie);
        flags.set_anime(self.anime);
        flags
    }

    fn titles(self) -> MediaTitle {
        MediaTitle {
            romaji: self.name,
            english: self.english_name,
            native: self.japanese_name,
        }
    }

    fn validate(&self) -> Vec<&'static str> {
        let mut errors = Vec::new();
        if self.name.len() > 1024 {
            errors.push("Name cannot be more than 1024 bytes.");
        }
        if is_over_length(&self.english_name, 1024) {
            errors.push("English name cannot be more than 1024 bytes.");
        }

        if is_over_length(&self.japanese_name, 1024) {
            errors.push("Japanese name cannot be more than 1024 bytes.");
        }

        if is_over_length(&self.notes, 1024) {
            errors.push("Notes cannot be more than 1024 bytes.");
        }
        errors
    }
}

fn anilist_id_or_url<'de, D>(de: D) -> Result<Option<u32>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt = Option::<String>::deserialize(de)?;
    let opt = opt.as_deref();
    match opt {
        None | Some("") => Ok(None),
        Some(s) => {
            if s.chars().all(|x| x.is_ascii_digit()) {
                s.parse::<u32>().map(Some).map_err(serde::de::Error::custom)
            } else {
                crate::utils::get_anilist_id(s)
                    .ok_or_else(|| serde::de::Error::custom("Invalid anilist ID or URL provided"))
                    .map(Some)
            }
        }
    }
}

fn tmdb_url<'de, D>(de: D) -> Result<Option<tmdb::Id>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt = Option::<String>::deserialize(de)?;
    let opt = opt.as_deref();
    match opt {
        None | Some("") => Ok(None),
        Some(s) => tmdb::get_tmdb_id(s)
            .ok_or_else(|| serde::de::Error::custom("Invalid TMDB URL provided"))
            .map(Some),
    }
}

async fn edit_directory_entry(
    State(state): State<AppState>,
    Path(entry_id): Path<i64>,
    account: Account,
    flasher: Flasher,
    Referrer(url): Referrer,
    Form(payload): Form<EditDirectoryEntry>,
) -> Response {
    if !account.flags.is_editor() {
        return flasher.add("You do not have permissions to edit this.").bail(&url);
    }

    let Some(entry) = state.get_directory_entry(entry_id).await else {
        return flasher.add("Directory entry not found.").bail(&url);
    };

    let errors = payload.validate();
    if !errors.is_empty() {
        for error in errors {
            flasher.add(error);
        }
        return Redirect::to(&url).into_response();
    }

    // maybe refactor this?
    let mut columns = Vec::with_capacity(11);
    let mut params: Vec<Box<dyn rusqlite::ToSql + Send>> = Vec::with_capacity(11);
    let mut audit_data = audit::EditEntry::default();
    let flags = payload.apply_flags(entry.flags);

    if entry.name != payload.name {
        columns.push("name");
        audit_data.before.name = Some(entry.name);
        audit_data.after.name = Some(payload.name.clone());
        params.push(Box::new(payload.name));
    }
    if entry.japanese_name != payload.japanese_name {
        columns.push("japanese_name");
        audit_data.before.japanese_name = entry.japanese_name;
        audit_data.after.japanese_name = payload.japanese_name.clone();
        params.push(Box::new(payload.japanese_name));
    }
    if entry.english_name != payload.english_name {
        columns.push("english_name");
        audit_data.before.english_name = entry.english_name;
        audit_data.after.english_name = payload.english_name.clone();
        params.push(Box::new(payload.english_name));
    }
    if entry.anilist_id != payload.anilist_id {
        columns.push("anilist_id");
        audit_data.before.anilist_id = entry.anilist_id;
        audit_data.after.anilist_id = payload.anilist_id;
        params.push(Box::new(payload.anilist_id));
    }
    if entry.tmdb_id != payload.tmdb_id {
        columns.push("tmdb_id");
        audit_data.before.tmdb_id = entry.tmdb_id;
        audit_data.after.tmdb_id = payload.tmdb_id;
        params.push(Box::new(payload.tmdb_id));
    }
    if entry.notes != payload.notes {
        columns.push("notes");
        audit_data.before.notes = entry.notes;
        audit_data.after.notes = payload.notes.clone();
        params.push(Box::new(payload.notes));
    }
    if entry.flags != flags {
        columns.push("flags");
        audit_data.before.flags = Some(entry.flags);
        audit_data.after.flags = Some(flags);
        params.push(Box::new(flags));
    }

    if !columns.is_empty() {
        params.push(Box::new(entry_id));
        let query = DirectoryEntry::update_query(&columns);
        audit_data.changed = columns.into_iter().map(String::from).collect();
        match state
            .database()
            .execute(query, rusqlite::params_from_iter(params))
            .await
        {
            Ok(_) => {
                state.cached_directories().invalidate().await;
                state
                    .audit(audit::AuditLogEntry::full(audit_data, entry_id, account.id))
                    .await;
                flasher.add(FlashMessage::success("Successfully edited entry."));
                Redirect::to(&url).into_response()
            }
            Err(rusqlite::Error::SqliteFailure(error, Some(s)))
                if error.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE =>
            {
                if let Some(suffix) = s.strip_prefix("UNIQUE constraint failed: directory_entry.") {
                    flasher
                        .add(format!("An entry already exists with this {suffix} field."))
                        .bail(&url)
                } else {
                    flasher
                        .add("An entry already exists with one of these fields.")
                        .bail(&url)
                }
            }
            Err(e) => flasher.add(format!("SQL Error: {e}")).bail(&url),
        }
    } else {
        Redirect::to(&url).into_response()
    }
}

#[derive(Deserialize)]
struct SearchQueryParams {
    #[serde(default)]
    anilist_id: Option<u32>,
    #[serde(default)]
    tmdb_id: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Serialize)]
struct SearchResult {
    entry_id: i64,
}

async fn search_directory_entries(
    State(state): State<AppState>,
    account: Account,
    Query(params): Query<SearchQueryParams>,
) -> Result<Json<SearchResult>, ApiError> {
    if !account.flags.is_editor() {
        return Err(ApiError::forbidden());
    }

    if params.anilist_id.is_none() && params.name.is_none() && params.tmdb_id.is_none() {
        return Err(ApiError::new("Missing search parameter"));
    }

    let path = params
        .name
        .as_deref()
        .map(sanitise_file_name::sanitise)
        .and_then(|x| state.config().subtitle_path.join(x).to_str().map(String::from));

    let entry = state
        .database()
        .get_row(
            "SELECT id FROM directory_entry WHERE anilist_id = ? OR tmdb_id = ? OR name = ? OR path = ?",
            (params.anilist_id, params.tmdb_id, params.name, path),
            |row| row.get(0),
        )
        .await
        .optional()?;
    match entry {
        Some(entry_id) => Ok(Json(SearchResult { entry_id })),
        None => Err(ApiError::not_found("Entry not found.")),
    }
}

#[derive(Deserialize)]
struct MoveDirectoryEntries {
    #[serde(default)]
    anilist_id: Option<u32>,
    #[serde(default, rename = "tmdb")]
    tmdb_id: Option<tmdb::Id>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    entry_id: Option<i64>,
    #[serde(default = "crate::utils::default_true")]
    anime: bool,
    files: Vec<String>,
}

#[derive(Serialize)]
struct BulkFileOperationResponse {
    entry_id: i64,
    success: usize,
    failed: usize,
}

async fn move_directory_entries(
    State(state): State<AppState>,
    Path(from_entry_id): Path<i64>,
    account: Account,
    Json(payload): Json<MoveDirectoryEntries>,
) -> Result<Json<BulkFileOperationResponse>, ApiError> {
    if !account.flags.is_editor() {
        return Err(ApiError::forbidden());
    }

    let Some(entry) = state.get_directory_entry_path(from_entry_id).await else {
        return Err(ApiError::not_found("Directory entry not found."));
    };
    let (entry_id, path, mut audit_data) = match payload.entry_id {
        Some(entry_id) => {
            let Some(path) = state.get_directory_entry_path(entry_id).await else {
                return Err(ApiError::not_found(format!("Directory entry {entry_id} not found.")));
            };
            (entry_id, path, audit::MoveEntry::new(entry_id))
        }
        None => {
            let (entry_id, path) = raw_create_directory_entry(
                &state,
                account.clone(),
                PendingDirectoryEntry {
                    anilist_id: payload.anilist_id,
                    tmdb_id: payload.tmdb_id,
                    name: payload.name.clone(),
                    anime: payload.anime,
                    titles: None,
                    flags: None,
                    notes: None,
                },
                false,
            )
            .await?;
            let audit_data = audit::MoveEntry {
                anime: payload.anime,
                name: payload.name.clone(),
                tmdb_id: payload.tmdb_id,
                anilist_id: payload.anilist_id,
                entry_id,
                created: true,
                files: Vec::new(),
            };
            (entry_id, path, audit_data)
        }
    };

    let mut success = 0;
    let mut failed = 0;
    audit_data.files.reserve(payload.files.len());
    for file in payload.files {
        let from = entry.join(&file);
        let to = path.join(&file);
        let result = tokio::fs::rename(from, to).await;
        audit_data.add_file(file, result.is_err());
        match result {
            Ok(_) => success += 1,
            Err(_) => failed += 1,
        }
    }

    let _ = state
        .database()
        .execute(
            "UPDATE directory_entry SET last_updated_at = CURRENT_TIMESTAMP WHERE id = ?",
            [entry_id],
        )
        .await;

    state.cached_directories().invalidate().await;
    state
        .audit(audit::AuditLogEntry::full(audit_data, from_entry_id, account.id))
        .await;
    Ok(Json(BulkFileOperationResponse {
        entry_id,
        success,
        failed,
    }))
}

#[derive(Deserialize)]
struct BulkFilesPayload {
    files: Vec<String>,
    #[serde(default)]
    delete_parent: bool,
    #[serde(default)]
    reason: Option<String>,
}

async fn bulk_delete_files(
    State(state): State<AppState>,
    Path(entry_id): Path<i64>,
    account: Account,
    Json(payload): Json<BulkFilesPayload>,
) -> Result<Json<BulkFileOperationResponse>, ApiError> {
    if !account.flags.is_editor() {
        return Err(ApiError::forbidden());
    }

    let Some(entry) = state.get_directory_entry_path(entry_id).await else {
        return Err(ApiError::not_found("Directory entry not found."));
    };

    if !account.flags.is_admin() && payload.reason.is_none() {
        return Err(ApiError::new("Reason must be provided"));
    }

    if let Some(reason) = payload.reason.as_deref() {
        if reason.is_empty() {
            return Err(ApiError::new("Reason cannot be empty"));
        } else if reason.len() > 512 {
            return Err(ApiError::new("Reason can only be up to 512 characters long"));
        }
    }

    let mut success = 0;
    let mut failed = 0;
    if payload.delete_parent {
        if !account.flags.is_admin() {
            return Err(ApiError::forbidden());
        }
        let name = state
            .database()
            .get_row(
                "DELETE FROM directory_entry WHERE id = ? RETURNING name",
                [entry_id],
                |r| r.get("name"),
            )
            .await?;
        state.cached_directories().invalidate().await;
        tokio::fs::remove_dir_all(entry).await?;
        state
            .audit(audit::AuditLogEntry::full(
                audit::DeleteEntry { name },
                entry_id,
                account.id,
            ))
            .await;
    } else {
        let trash = crate::trash::Trash::new()?;
        let mut audit_data = audit::DeleteFiles {
            permanent: account.flags.is_admin(),
            files: Vec::with_capacity(payload.files.len()),
            reason: payload.reason.clone(),
        };
        let total = payload.files.len();
        let description = crate::utils::join_iter("\n", payload.files.iter().map(|x| format!("- {x}")).take(25));
        for file in payload.files {
            let path = entry.join(&file);
            let result = if account.flags.is_admin() {
                tokio::fs::remove_file(path).await
            } else {
                trash.put(path, entry_id, payload.reason.clone()).await
            };
            audit_data.add_file(file, result.is_err());
            match result {
                Ok(_) => success += 1,
                Err(_) => failed += 1,
            }
        }
        state
            .audit(audit::AuditLogEntry::full(audit_data, entry_id, account.id))
            .await;
        state.send_alert(
            crate::discord::Alert::error("Deleted Files")
                .url(format!("/logs?entry_id={entry_id}"))
                .description(description)
                .account(account)
                .field("Total", total)
                .field("Failed", failed),
        );
    }

    Ok(Json(BulkFileOperationResponse {
        entry_id,
        success,
        failed,
    }))
}

#[derive(Deserialize)]
struct RenameFileRequest {
    from: String,
    to: String,
}

async fn bulk_rename_files(
    State(state): State<AppState>,
    Path(entry_id): Path<i64>,
    account: Account,
    Json(files): Json<Vec<RenameFileRequest>>,
) -> Result<Json<BulkFileOperationResponse>, ApiError> {
    if !account.flags.is_editor() {
        return Err(ApiError::forbidden());
    }

    let Some(entry) = state.get_directory_entry_path(entry_id).await else {
        return Err(ApiError::not_found("Directory entry not found."));
    };

    let mut data = audit::RenameFiles {
        files: Vec::with_capacity(files.len()),
    };
    let mut success = 0;
    let mut failed = 0;
    for file in files {
        let from = entry.join(&file.from);
        let to = entry.join(&file.to);
        let result = tokio::fs::rename(from, to).await;
        data.add_file(file.from, file.to, result.is_err());
        match result {
            Ok(_) => success += 1,
            Err(_) => failed += 1,
        }
    }

    state
        .audit(audit::AuditLogEntry::full(data, entry_id, account.id))
        .await;
    Ok(Json(BulkFileOperationResponse {
        entry_id,
        success,
        failed,
    }))
}

#[derive(Debug)]
struct ProcessedFile {
    path: PathBuf,
    bytes: Bytes,
}

impl ProcessedFile {
    fn write_to_disk(self) -> std::io::Result<()> {
        let mut fp = std::fs::File::create(self.path)?;
        fp.write_all(&self.bytes)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingFileEntry {
    pub name: String,
    #[serde(with = "crate::utils::base64_bytes")]
    pub data: Vec<u8>,
}

impl PendingFileEntry {
    pub fn write_to_disk(&self, base_path: PathBuf) -> std::io::Result<()> {
        let path = base_path.join(sanitise_file_name::sanitise(&self.name));
        let mut fp = std::fs::File::create(path)?;
        fp.write_all(&self.data)?;
        Ok(())
    }
}

struct ProcessedFiles {
    files: Vec<ProcessedFile>,
    skipped: usize,
}

async fn verify_file(
    entry_path: &std::path::Path,
    file_name: PathBuf,
    field: Field<'_>,
) -> anyhow::Result<ProcessedFile> {
    match file_name.extension().and_then(|ext| ext.to_str()) {
        Some("srt" | "ass" | "ssa" | "zip" | "sub" | "sup" | "idx" | "7z") => {
            let path = entry_path.join(file_name);
            if path.exists() {
                bail!("filename already exists")
            }
            let bytes = field.bytes().await?;
            Ok(ProcessedFile { path, bytes })
        }
        _ => bail!("invalid file extension"),
    }
}

async fn process_files(entry_path: &std::path::Path, mut multipart: Multipart) -> anyhow::Result<ProcessedFiles> {
    let mut files = Vec::new();
    let mut skipped = 0;
    while let Some(field) = multipart.next_field().await? {
        let Some(name) = field.file_name().map(sanitise_file_name::sanitise).map(PathBuf::from) else {
            tracing::debug!("Skipped file due to missing filename");
            skipped += 1;
            continue;
        };

        match verify_file(entry_path, name, field).await {
            Ok(file) => files.push(file),
            Err(e) => {
                tracing::debug!(error=%e, "Skipped file due to validation issue");
                skipped += 1
            }
        }
    }
    Ok(ProcessedFiles { files, skipped })
}

/// The result of an upload operation.
#[derive(Debug, Clone, Copy, Serialize, ToSchema)]
pub struct UploadResult {
    /// The number of files that did not succeed due to a filesystem error.
    errors: usize,
    /// The number of files that were processed.
    total: usize,
    /// The number of files that were skipped due to some reason
    skipped: usize,
}

impl UploadResult {
    pub fn is_success(&self) -> bool {
        self.total > 0 && self.errors == 0 && self.skipped == 0
    }

    pub fn is_error(&self) -> bool {
        self.total == self.errors
    }

    pub fn successful(&self) -> usize {
        self.total - self.errors
    }
}

pub async fn raw_upload_file(
    state: AppState,
    entry_id: i64,
    account: Account,
    multipart: Multipart,
    api: bool,
) -> Result<UploadResult, ApiError> {
    let Some(entry) = state.get_directory_entry_path(entry_id).await else {
        return Err(ApiError::not_found("Entry not found"));
    };

    let Ok(processed) = process_files(&entry, multipart).await else {
        return Err(ApiError::new("Internal error when processing files").with_code(ApiErrorCode::ServerError));
    };

    if processed.files.is_empty() {
        return Err(ApiError::new("Did not upload any files."));
    }

    let mut errored = 0usize;
    let total = processed.files.len();
    let mut data = audit::Upload {
        files: Vec::with_capacity(total),
        api,
    };
    let mut set = JoinSet::new();
    for file in processed.files.into_iter() {
        set.spawn_blocking(move || {
            let name = file.path.file_name().and_then(|x| x.to_str()).unwrap().to_owned();
            let failed = file.write_to_disk().is_err();
            audit::FileOperation { name, failed }
        });
    }

    while let Some(task) = set.join_next().await {
        match task {
            Ok(op) => {
                errored += op.failed as usize;
                data.files.push(op);
            }
            _ => errored += 1,
        }
    }

    let successful = total > 0 && errored == 0 && processed.skipped == 0;
    if successful && errored != total {
        let _ = state
            .database()
            .execute(
                "UPDATE directory_entry SET last_updated_at = CURRENT_TIMESTAMP WHERE id = ?",
                [entry_id],
            )
            .await;
        state.cached_directories().invalidate().await;
    }

    state
        .audit(audit::AuditLogEntry::full(data, entry_id, account.id))
        .await;

    Ok(UploadResult {
        errors: errored,
        total,
        skipped: processed.skipped,
    })
}

async fn upload_file(
    State(state): State<AppState>,
    Path(entry_id): Path<i64>,
    Referrer(url): Referrer,
    account: Account,
    flasher: Flasher,
    multipart: Multipart,
) -> Response {
    let result = match raw_upload_file(state, entry_id, account, multipart, false).await {
        Ok(result) => result,
        Err(msg) => return flasher.add(msg.error.as_ref()).bail(&url),
    };
    let message = if result.is_success() {
        FlashMessage::success("Upload successful.")
    } else if result.is_error() {
        FlashMessage::error("Upload failed.")
    } else {
        let successful = result.successful();
        FlashMessage::warning(format!(
            "Uploaded {successful} file{}, {} {} skipped and {} failed",
            if successful == 1 { "" } else { "s" },
            result.skipped,
            if result.skipped == 1 { "was" } else { "were" },
            result.errors,
        ))
    };
    flasher.add(message).bail(&url)
}

async fn bulk_download(
    State(state): State<AppState>,
    Path(entry_id): Path<i64>,
    Json(payload): Json<BulkFilesPayload>,
) -> Result<Response, ApiError> {
    let Some(entry) = state.get_directory_entry(entry_id).await else {
        return Err(ApiError::not_found("Directory entry not found."));
    };

    let filename = sanitise_file_name::sanitise(&format!("{}.zip", &entry.name));
    let buffer = tokio::task::spawn_blocking(move || -> std::io::Result<_> {
        let options = zip::write::FileOptions::default();
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));

        for file in payload.files {
            let path = entry.path.join(&file);
            let Ok(contents) = std::fs::read(&path) else {
                continue;
            };
            zip.start_file(file, options)?;
            zip.write_all(&contents)?;
        }
        let mut buffer = zip.finish()?.into_inner();
        buffer.shrink_to_fit();
        Ok(Bytes::from(buffer))
    })
    .await??;

    let body = Body::from(buffer);
    let headers = [
        (CONTENT_TYPE, "application/zip"),
        (CONTENT_DISPOSITION, &format!("attachment; filename=\"{filename}\"")),
        (HeaderName::from_static("x-jimaku-filename"), &filename),
    ];
    Ok((headers, body).into_response())
}

#[derive(Deserialize)]
struct RelationsRequest {
    anilist_ids: Vec<u32>,
}

async fn relations(
    State(state): State<AppState>,
    Json(requested): Json<RelationsRequest>,
) -> Result<Json<Vec<DirectoryEntry>>, ApiError> {
    let mut query = "SELECT * FROM directory_entry WHERE anilist_id IN (".to_string();
    for _ in &requested.anilist_ids {
        query.push('?');
        query.push(',');
    }
    if query.ends_with(',') {
        query.pop();
    }
    query.push(')');
    let entries = state
        .database()
        .all(query, rusqlite::params_from_iter(requested.anilist_ids))
        .await?;

    Ok(Json(entries))
}

#[derive(Deserialize)]
struct TmdbQuery {
    id: tmdb::Id,
}

#[derive(Serialize)]
struct TmdbInfo {
    title: MediaTitle,
    adult: bool,
    movie: bool,
}

async fn tmdb_lookup(
    State(state): State<AppState>,
    account: Account,
    Query(query): Query<TmdbQuery>,
) -> Result<Json<Option<TmdbInfo>>, ApiError> {
    if !account.flags.is_editor() {
        return Err(ApiError::forbidden());
    }

    Ok(Json(
        tmdb::get_media_info(&state.client, &state.config().tmdb_api_key, query.id)
            .await?
            .map(|info| TmdbInfo {
                title: info.titles(),
                adult: info.is_adult(),
                movie: query.id.is_movie(),
            }),
    ))
}

#[derive(Deserialize)]
struct ImportEntry {
    anime: bool,
    name: String,
}

#[derive(Template)]
#[template(path = "entry_import.html")]
struct ImportEntryTemplate {
    account: Option<Account>,
    flashes: Flashes,
    pending: DirectoryEntry,
    anime: bool,
}

async fn get_pending_directory_entry(state: &AppState, anime: bool, name: String) -> DirectoryEntry {
    let mut temporary = DirectoryEntry::temporary(name.clone());
    temporary.flags.set_anime(anime);
    if anime {
        let media = anilist::search(&state.client, &name)
            .await
            .map(|m| m.into_iter().next());
        if let Ok(Some(media)) = media {
            temporary.anilist_id = Some(media.id);
            temporary.flags.set_movie(media.is_movie());
            temporary.flags.set_adult(media.adult);
            temporary.name = media.title.romaji;
            temporary.japanese_name = media.title.native;
            temporary.english_name = media.title.english;
        }
    } else {
        let info = tmdb::find_match(&state.client, &state.config().tmdb_api_key, &name).await;
        if let Ok(Some(info)) = info {
            temporary.tmdb_id = Some(info.id);
            temporary.flags.set_movie(info.id.is_movie());
            temporary.flags.set_adult(info.is_adult());
            let titles = info.titles();
            temporary.name = titles.romaji;
            temporary.japanese_name = titles.native;
            temporary.english_name = titles.english;
        }
    }
    temporary
}

async fn import_entry(
    State(state): State<AppState>,
    account: Account,
    flashes: Flashes,
    flasher: Flasher,
    Form(payload): Form<ImportEntry>,
) -> Response {
    if !account.flags.is_editor() {
        return flasher.add("You do not have permissions to do this.").bail("/");
    }

    let pending = get_pending_directory_entry(&state, payload.anime, payload.name).await;
    let mut response = ImportEntryTemplate {
        account: Some(account),
        flashes,
        pending,
        anime: payload.anime,
    }
    .into_response();
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("private, no-store"));
    response
}

#[derive(Deserialize)]
struct ImportQuery {
    anime: bool,
}

#[derive(Serialize)]
struct ImportResult {
    entry_id: i64,
    errors: usize,
}

#[derive(Deserialize)]
struct CreateImportedEntry {
    files: Vec<PendingFileEntry>,
    #[serde(flatten)]
    inner: EditDirectoryEntry,
}

async fn create_imported_entry(
    State(state): State<AppState>,
    account: Account,
    Query(query): Query<ImportQuery>,
    Json(payload): Json<CreateImportedEntry>,
) -> Result<Json<ImportResult>, ApiError> {
    if !account.flags.is_editor() {
        return Err(ApiError::forbidden());
    }

    let validation_errors = payload.inner.validate();
    if !validation_errors.is_empty() {
        return Err(ApiError::new(validation_errors.join("\n")));
    }

    let mut flags = payload.inner.apply_flags(EntryFlags::new());
    flags.set_anime(query.anime);
    let pending = PendingDirectoryEntry {
        anilist_id: payload.inner.anilist_id,
        tmdb_id: payload.inner.tmdb_id,
        name: None,
        flags: Some(flags),
        anime: query.anime,
        notes: payload.inner.notes.clone(),
        titles: Some(payload.inner.titles()),
    };

    // Unfortunately have to pay this cost twice
    let path = pending.path(pending.titles.as_ref().unwrap().romaji.as_str(), pending.anime, &state);
    let anilist_id = pending.anilist_id;
    let tmdb_id = pending.tmdb_id;
    let account_id = account.id;

    let (id, path) = match raw_create_directory_entry(&state, account, pending, false).await {
        Ok(p) => p,
        Err(e) if e.code == ApiErrorCode::EntryAlreadyExists => state
            .database()
            .get_row(
                "SELECT id, path FROM directory_entry WHERE path = ? OR anilist_id = ? OR tmdb_id = ?",
                (path.display().to_string(), anilist_id, tmdb_id),
                |row| Ok((row.get("id")?, PathBuf::from(row.get::<_, String>("path")?))),
            )
            .await
            .optional()?
            .ok_or(e)?,
        Err(e) => return Err(e),
    };

    let mut set = JoinSet::new();
    let mut data = audit::Upload {
        files: Vec::with_capacity(payload.files.len()),
        api: false,
    };
    for file in payload.files {
        let p = path.clone();
        set.spawn_blocking(move || {
            let failed = file.write_to_disk(p).is_err();
            audit::FileOperation {
                name: file.name,
                failed,
            }
        });
    }

    let mut errors = 0;
    while let Some(task) = set.join_next().await {
        match task {
            Ok(op) => {
                errors += op.failed as usize;
                data.files.push(op);
            }
            _ => errors += 1,
        }
    }

    state.audit(audit::AuditLogEntry::full(data, id, account_id)).await;

    Ok(Json(ImportResult { entry_id: id, errors }))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/entry/:id", get(get_entry))
        .route(
            "/entry/:id/download/*path",
            get(download_entry).layer(CorsLayer::permissive()),
        )
        .route(
            "/entry/create",
            post(create_directory_entry).layer(RateLimit::default().quota(5, 30.0).build()),
        )
        .route("/entry/:id/edit", post(edit_directory_entry))
        .route("/entry/:id/move", post(move_directory_entries))
        .route("/entry/:id/rename", post(bulk_rename_files))
        .route("/entry/:id", delete(bulk_delete_files))
        .route("/entry/search", get(search_directory_entries))
        .route(
            "/entry/:id/upload",
            post(upload_file).layer(RateLimit::default().quota(5, 5.0).build()),
        )
        .route(
            "/entry/:id/bulk",
            post(bulk_download).layer(RateLimit::default().quota(5, 5.0).build()),
        )
        .route("/entry/relations", post(relations))
        .route("/entry/tmdb", get(tmdb_lookup))
        .route("/entry/import", post(import_entry))
        .route("/entry/import/create", post(create_imported_entry))
}
