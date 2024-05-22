use axum::extract::{Multipart, State};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::{
    anilist::MediaTitle,
    error::{ApiError, ApiErrorCode},
    models::{DirectoryEntry, EntryFlags},
    routes::entry::{
        get_file_entries, raw_create_directory_entry, raw_upload_file, FileEntry, PendingDirectoryEntry, UploadResult,
    },
    tmdb, AppState,
};

use super::{
    auth::ApiToken,
    utils::{ApiJson as Json, ApiPath as Path, ApiQuery as Query, RateLimitResponse},
};

/// Details
///
/// Get the top level details of an entry by its ID.
#[utoipa::path(
    get,
    path = "/api/entries/{id}",
    responses(
        (status = 200, description = "Successfully retrieved entry", body = Entry),
        (status = 400, description = "Invalid ID given", body = ApiError),
        (status = 401, description = "User is unauthenticated", body = ApiError),
        (status = 404, description = "Entry not found", body = ApiError),
        (status = 429, response = RateLimitResponse),
    ),
    params(
        ("id" = i64, Path, description = "The entry's ID")
    ),
    security(
        ("api_key" = [])
    ),
    tag = "entries"
)]
pub async fn get_entry_by_id(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    _auth: ApiToken,
) -> Result<Json<DirectoryEntry>, ApiError> {
    match state.get_directory_entry(id).await {
        Some(entry) => Ok(Json(entry)),
        None => Err(ApiError::not_found("This entry could not be found")),
    }
}

/// Files
///
/// Get the files associated with an entry.
#[utoipa::path(
    get,
    path = "/api/entries/{id}/files",
    responses(
        (status = 200, description = "Successful response", body = [FileEntry]),
        (status = 400, description = "Invalid ID given", body = ApiError),
        (status = 401, description = "User is unauthenticated", body = ApiError),
        (status = 404, description = "Entry not found", body = ApiError),
        (status = 429, response = RateLimitResponse),
    ),
    params(
        ("id" = i64, Path, description = "The entry's ID")
    ),
    security(
        ("api_key" = [])
    ),
    tag = "entries"
)]
pub async fn get_entry_files(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    _auth: ApiToken,
) -> Result<Json<Vec<FileEntry>>, ApiError> {
    match state.get_directory_entry_path(id).await {
        Some(path) => {
            let mut files = get_file_entries(id, &path)?;
            let url = state.config().canonical_url();
            for file in files.iter_mut() {
                file.url = url.clone() + file.url.as_str();
            }
            Ok(Json(files))
        }
        None => Err(ApiError::not_found("This entry could not be found")),
    }
}

#[derive(Deserialize, IntoParams)]
pub struct SearchQuery {
    /// Return entries that are anime.
    #[serde(default = "crate::utils::default_true")]
    #[param(default = true)]
    anime: bool,
    /// Return the entry that has the given AniList ID.
    #[serde(default)]
    anilist_id: Option<u32>,
    /// Return the entry that has the given TMDB ID.
    ///
    /// Check the documentation for TMDB ID encoding.
    #[serde(deserialize_with = "crate::utils::generic_empty_string_is_none")]
    #[param(pattern = r#"(tv|movie):(\d+)"#, value_type = Option<String>, example = "tv:12345")]
    #[serde(default)]
    tmdb_id: Option<tmdb::Id>,
    /// Return entries that match the given string.
    ///
    /// Currently this search is done through a fuzzy
    /// search.
    #[serde(deserialize_with = "crate::utils::generic_empty_string_is_none")]
    #[serde(default)]
    query: Option<String>,

    /// Return entries that are after this UNIX timestamp (in seconds).
    #[serde(default)]
    after: Option<i64>,

    /// Return entries that are before this UNIX timestamp (in seconds).
    #[serde(default)]
    before: Option<i64>,
}

impl SearchQuery {
    fn get_best_fuzzy_score(&self, entry: &DirectoryEntry) -> Option<sublime_fuzzy::Match> {
        let query = self.query.as_deref()?;
        let mut max = sublime_fuzzy::best_match(query, &entry.name);
        if let Some(target) = entry.english_name.as_deref() {
            max = max.max(sublime_fuzzy::best_match(query, target));
        }
        if let Some(target) = entry.japanese_name.as_deref() {
            max = max.max(sublime_fuzzy::best_match(query, target));
        }
        max
    }

    fn apply(&self, entry: &DirectoryEntry) -> Option<isize> {
        if self.anime != entry.flags.is_anime() {
            return None;
        }

        if self.anilist_id.is_some() {
            return (self.anilist_id == entry.anilist_id).then_some(isize::MAX);
        }

        if self.tmdb_id.is_some() {
            return (self.tmdb_id == entry.tmdb_id).then_some(isize::MAX);
        }

        let ts = entry.last_updated_at.unix_timestamp();
        if let Some(after) = self.after {
            if ts < after {
                return None;
            }
        }

        if let Some(before) = self.before {
            if ts > before {
                return None;
            }
        }

        if let Some(m) = self.get_best_fuzzy_score(entry) {
            return (m.score() >= 100).then_some(m.score());
        }

        self.query.is_none().then_some(100)
    }
}

/// Search
///
/// Returns all entries that meet a specific criteria.
#[utoipa::path(
    get,
    path = "/api/entries/search",
    responses(
        (status = 200, description = "Successful response", body = [Entry]),
        (status = 401, description = "User is unauthenticated", body = ApiError),
        (status = 429, response = RateLimitResponse),
    ),
    params(SearchQuery),
    security(
        ("api_key" = [])
    ),
    tag = "entries"
)]
pub async fn search_entries(
    State(state): State<AppState>,
    Query(query): Query<SearchQuery>,
    _auth: ApiToken,
) -> Result<Json<Vec<DirectoryEntry>>, ApiError> {
    let entries = state.directory_entries().await;
    let mut entries = entries
        .iter()
        .filter_map(|s| query.apply(s).zip(Some(s.clone())))
        .collect::<Vec<_>>();
    entries.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
    Ok(Json(entries.into_iter().map(|(_, entry)| entry).collect()))
}

#[derive(Deserialize, IntoParams)]
pub struct CreateQuery {
    /// Create an entry backed by the given AniList ID.
    #[serde(deserialize_with = "crate::utils::generic_empty_string_is_none")]
    #[serde(default)]
    anilist_id: Option<u32>,
    /// Create an entry backed by the given TMDB ID.
    ///
    /// Check the documentation for the string TMDB ID encoding.
    #[serde(deserialize_with = "crate::utils::generic_empty_string_is_none")]
    #[param(pattern = r#"(tv|movie):(\d+)"#, value_type = Option<String>, example = "tv:12345")]
    #[serde(default)]
    tmdb_id: Option<tmdb::Id>,
}

#[derive(Deserialize, ToSchema)]
pub struct CreatePayload {
    /// Create an entry backed by the given AniList ID.
    #[serde(default)]
    anilist_id: Option<u32>,
    /// Create an entry backed by the given TMDB ID.
    ///
    /// Check the documentation for the string TMDB ID encoding.
    #[serde(default)]
    #[schema(pattern = r#"(tv|movie):(\d+)"#, value_type = Option<String>, example = "tv:12345")]
    tmdb_id: Option<tmdb::Id>,
    /// Create an entry with the given Romaji name.
    ///
    /// This is only available for API keys bound to editor users.
    #[serde(default)]
    name: Option<String>,
    /// Create an entry with the given Japanese name.
    ///
    /// This is only available for API keys bound to editor users.
    #[serde(default)]
    japanese_name: Option<String>,
    /// Create an entry with the given English name.
    ///
    /// This is only available for API keys bound to editor users.
    #[serde(default)]
    english_name: Option<String>,
    /// Create an entry with the given flags.
    ///
    /// This is only available for API keys bound to editor users.
    #[serde(default, with = "crate::models::expand_flags::option")]
    flags: Option<EntryFlags>,
}

#[derive(Serialize, ToSchema)]
pub struct CreateEntryResult {
    /// The resulting entry ID
    entry_id: i64,
}

/// Create
///
/// Creates an entry backed by an AniList or TMDB ID.
///
/// This endpoint is atomic. If an entry already exists with
/// the given AniList or TMDB ID then that pre-existing entry
/// is returned instead.
///
/// An entry becomes an anime entry if an AniList ID is given.
///
/// Note that only API keys bound to editor users can use
/// fields other than `tmdb_id` and `anilist_id`.
#[utoipa::path(
    post,
    path = "/api/entries",
    request_body = inline(CreatePayload),
    responses(
        (status = 200, description = "Successful response", body = inline(CreateEntryResult)),
        (status = 400, description = "An error occurred", body = ApiError),
        (status = 401, description = "User is unauthenticated", body = ApiError),
        (status = 403, description = "The user does not have permission to do this", body = ApiError),
        (status = 429, response = RateLimitResponse),
    ),
    params(CreateQuery),
    security(
        ("api_key" = [])
    ),
    tag = "entries"
)]
pub async fn create_entry(
    State(state): State<AppState>,
    Query(query): Query<CreateQuery>,
    auth: ApiToken,
    Json(payload): Json<CreatePayload>,
) -> Result<Json<CreateEntryResult>, ApiError> {
    let Some(account) = state.get_account(auth.id).await else {
        return Err(ApiError::unauthorized());
    };
    let anilist_id = payload.anilist_id.or(query.anilist_id);
    let tmdb_id = payload.tmdb_id.or(query.tmdb_id);
    if !account.flags.is_editor()
        && (payload.name.is_some()
            || payload.japanese_name.is_some()
            || payload.english_name.is_some()
            || payload.flags.is_some())
    {
        return Err(ApiError::forbidden());
    }

    let flags = if payload.flags.is_none() && payload.name.is_some() {
        let mut flags = EntryFlags::new();
        flags.set_anime(anilist_id.is_some());
        Some(flags)
    } else {
        payload.flags
    };
    let titles = if let Some(name) = &payload.name {
        Some(MediaTitle {
            romaji: name.clone(),
            english: payload.english_name,
            native: payload.japanese_name,
        })
    } else {
        None
    };

    let pending = PendingDirectoryEntry {
        anime: anilist_id.is_some(),
        anilist_id,
        tmdb_id,
        titles,
        flags,
        ..Default::default()
    };

    let entry_id = match raw_create_directory_entry(&state, account, pending, true).await {
        Ok((entry_id, _)) => entry_id,
        Err(e) if e.code == ApiErrorCode::EntryAlreadyExists => state
            .database()
            .get_row(
                "SELECT id FROM directory_entry WHERE anilist_id = ? OR tmdb_id = ? OR name = ?",
                (anilist_id, tmdb_id, payload.name),
                |row| row.get("id"),
            )
            .await
            .optional()?
            .ok_or(e)?,
        Err(e) => return Err(e),
    };
    Ok(Json(CreateEntryResult { entry_id }))
}

#[derive(ToSchema)]
struct UploadedFiles {
    #[schema(format = Binary)]
    #[allow(dead_code)]
    file: Vec<String>,
}

/// Upload
///
/// Upload files to a given entry.
///
/// Multiple files can be uploaded at a time. The field name should be
/// `file` and the `filename` should point to the subtitle filename.
/// You can have multiple `file` fields.
#[utoipa::path(
    post,
    path = "/api/entries/{id}/upload",
    request_body(
        content = inline(UploadedFiles),
        content_type = "multipart/form-data",
        description = "The files to upload"
    ),
    responses(
        (status = 200, description = "Upload processed", body = UploadResult),
        (status = 400, description = "An error occurred", body = ApiError),
        (status = 401, description = "User is unauthenticated", body = ApiError),
        (status = 403, description = "The user does not have permission to do this", body = ApiError),
        (status = 404, description = "Entry not found", body = ApiError),
        (status = 429, response = RateLimitResponse),
    ),
    params(
        ("id" = i64, Path, description = "The entry's ID")
    ),
    security(
        ("api_key" = [])
    ),
    tag = "entries"
)]
pub async fn upload_files(
    State(state): State<AppState>,
    Path(entry_id): Path<i64>,
    auth: ApiToken,
    multipart: Multipart,
) -> Result<Json<UploadResult>, ApiError> {
    let Some(account) = state.get_account(auth.id).await else {
        return Err(ApiError::unauthorized());
    };
    let result = raw_upload_file(state, entry_id, account, multipart, true).await?;
    if result.is_error() {
        return Err(ApiError::new("Upload failed"));
    }
    Ok(Json(result))
}
