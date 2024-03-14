use std::io::Write;
use std::path::{Component, PathBuf};

use crate::anilist::{self, MediaTitle};
use crate::database::{is_unique_constraint_violation, Table};
use crate::error::{ApiError, InternalError};
use crate::filters;
use crate::flash::{FlashMessage, Flasher, Flashes};
use crate::models::{Account, AccountCheck, DirectoryEntry};
use crate::ratelimit::RateLimit;
use crate::referrer::Referrer;
use crate::utils::is_over_length;
use crate::AppState;
use anyhow::{bail, Context};
use askama::Template;
use axum::body::{Body, Bytes};
use axum::extract::multipart::Field;
use axum::extract::{Multipart, Query};
use axum::http::header::{CONTENT_DISPOSITION, CONTENT_TYPE};
use axum::http::{HeaderName, StatusCode};
use axum::response::Redirect;
use axum::routing::{delete, get, post};
use axum::Json;
use axum::{
    extract::{Form, Path, Request, State},
    response::{IntoResponse, Response},
    Router,
};
use percent_encoding::{percent_encode, AsciiSet, CONTROLS};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use tokio::task::JoinSet;
use tower::ServiceExt;
use tower_http::services::ServeFile;

const FRAGMENT: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'<')
    .add(b'>')
    .add(b'[')
    .add(b']')
    .add(b'`')
    .add(b'#');

#[derive(Debug, Serialize)]
struct FileEntry {
    #[serde(skip)]
    url: String,
    name: String,
    size: u64,
    #[serde(with = "time::serde::timestamp")]
    last_modified: OffsetDateTime,
}

#[derive(Template)]
#[template(path = "entry.html")]
struct EntryTemplate {
    account: Option<Account>,
    entry: DirectoryEntry,
    files: Vec<FileEntry>,
    flashes: Flashes,
}

fn get_file_entries(entry_id: i64, path: &std::path::Path) -> std::io::Result<Vec<FileEntry>> {
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

fn validate_path(base: &std::path::Path, requested: &str) -> Option<PathBuf> {
    let path = std::path::Path::new(requested.trim_start_matches('/'));
    let mut path_to_file = base.to_path_buf();
    for component in path.components() {
        match component {
            Component::Normal(cmp) => {
                if std::path::Path::new(&cmp)
                    .components()
                    .all(|c| matches!(c, Component::Normal(_)))
                {
                    path_to_file.push(cmp);
                } else {
                    return None;
                }
            }
            Component::CurDir => {}
            Component::Prefix(_) | Component::RootDir | Component::ParentDir => return None,
        }
    }
    Some(path_to_file)
}

enum DownloadResponse {
    File(Response),
    NotFound,
}

impl IntoResponse for DownloadResponse {
    fn into_response(self) -> Response {
        match self {
            DownloadResponse::File(r) => r,
            DownloadResponse::NotFound => StatusCode::NOT_FOUND.into_response(),
        }
    }
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

#[derive(Deserialize)]
struct CreateDirectoryEntry {
    #[serde(deserialize_with = "crate::utils::empty_string_is_none")]
    anilist_url: Option<String>,
    #[serde(deserialize_with = "crate::utils::empty_string_is_none")]
    name: Option<String>,
}

async fn raw_create_directory_entry(
    state: AppState,
    account: &Account,
    name: Option<String>,
    anilist_id: Option<u32>,
) -> anyhow::Result<(i64, PathBuf)> {
    let creator_id = account.id;

    let names = match anilist_id {
        Some(id) => {
            let media = anilist::search_by_id(&state.client, id)
                .await
                .with_context(|| "AniList returned an error. Please try again later.".to_owned())?
                .with_context(|| "AniList did not return results for this URL.".to_owned())?;
            media.title
        }
        None if account.flags.is_editor() => {
            if let Some(name) = name {
                MediaTitle::new(name)
            } else {
                bail!("Missing name for directory.");
            }
        }
        None => bail!("Missing anilist_id for directory."),
    };

    // Series names aren't unique but directory names are
    // So try to give it some noise depending on the anilist ID
    // This ordeal could also be entirely avoided by just using numeric folder names
    // But having human readable folder names is fine
    let directory_name = if let Some(id) = anilist_id {
        sanitise_file_name::sanitise(&format!("{} [{}]", names.romaji, id))
    } else {
        sanitise_file_name::sanitise(&names.romaji)
    };

    // Verify that the path can be created
    let path = state.config().subtitle_path.join(directory_name);

    if path.exists() {
        bail!("Path already exists.");
    }

    let Some(path_string) = path.to_str() else {
        bail!("Resulting path was not UTF-8.");
    };

    let query = r#"
        INSERT INTO directory_entry(path, creator_id, anilist_id, name, english_name, japanese_name)
        VALUES (?, ?, ?, ?, ?, ?)
        RETURNING id;
    "#;
    let path_string = path_string.to_owned();
    let response = state
        .database()
        .call(move |con| -> anyhow::Result<(i64, PathBuf)> {
            let tx = con.transaction()?;
            let result: rusqlite::Result<i64> = {
                let mut stmt = tx.prepare_cached(query)?;
                stmt.query_row(
                    (
                        path_string.to_owned(),
                        creator_id,
                        anilist_id,
                        names.romaji,
                        names.english,
                        names.native,
                    ),
                    |row| row.get("id"),
                )
            };

            let url = match result {
                Ok(entry_id) => {
                    std::fs::create_dir(&path)
                        .with_context(|| format!("Could not create directory {}", path.display()))?;
                    (entry_id, path)
                }
                Err(e) if is_unique_constraint_violation(&e) => return Err(anyhow::anyhow!("Entry already exists.")),
                Err(e) => return Err(e.into()),
            };

            tx.commit()?;
            Ok(url)
        })
        .await;

    if response.is_ok() {
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
    let anilist_id = payload.anilist_url.as_deref().and_then(crate::utils::get_anilist_id);
    let response = raw_create_directory_entry(state, &account, payload.name, anilist_id).await;
    match response {
        Ok((entry_id, _)) => Redirect::to(&format!("/entry/{entry_id}")).into_response(),
        Err(e) => flasher.add(e.to_string()).bail(&url),
    }
}

#[derive(Deserialize)]
struct EditDirectoryEntry {
    name: String,
    #[serde(deserialize_with = "crate::utils::empty_string_is_none")]
    japanese_name: Option<String>,
    #[serde(deserialize_with = "crate::utils::empty_string_is_none")]
    english_name: Option<String>,
    #[serde(deserialize_with = "crate::utils::generic_empty_string_is_none")]
    anilist_id: Option<u32>,
    #[serde(deserialize_with = "crate::utils::empty_string_is_none")]
    notes: Option<String>,
    #[serde(default)]
    low_quality: bool,
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

    let mut invalid = false;
    if is_over_length(&payload.english_name, 1024) {
        flasher.add("English name cannot be more than 1024 bytes.");
        invalid = true;
    }

    if is_over_length(&payload.japanese_name, 1024) {
        flasher.add("Japanese name cannot be more than 1024 bytes.");
        invalid = true;
    }

    if is_over_length(&payload.notes, 1024) {
        flasher.add("Notes cannot be more than 1024 bytes.");
        invalid = true;
    }

    if invalid {
        return Redirect::to(&url).into_response();
    }

    // maybe refactor this?
    let mut columns = Vec::with_capacity(10);
    let mut params: Vec<Box<dyn rusqlite::ToSql + Send>> = Vec::with_capacity(10);
    if entry.name != payload.name {
        columns.push("name");
        params.push(Box::new(payload.name));
    }
    if entry.japanese_name != payload.japanese_name {
        columns.push("japanese_name");
        params.push(Box::new(payload.japanese_name));
    }
    if entry.english_name != payload.english_name {
        columns.push("english_name");
        params.push(Box::new(payload.english_name));
    }
    if entry.anilist_id != payload.anilist_id {
        columns.push("anilist_id");
        params.push(Box::new(payload.anilist_id));
    }
    if entry.notes != payload.notes {
        columns.push("notes");
        params.push(Box::new(payload.notes));
    }
    if entry.flags.is_low_quality() != payload.low_quality {
        columns.push("flags");
        let mut flag = entry.flags;
        flag.set_low_quality(payload.low_quality);
        params.push(Box::new(flag));
    }

    if !columns.is_empty() {
        params.push(Box::new(entry_id));
        let query = DirectoryEntry::update_query(columns);
        match state
            .database()
            .execute(query, rusqlite::params_from_iter(params))
            .await
        {
            Ok(_) => {
                state.cached_directories().invalidate().await;
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

    if params.anilist_id.is_none() && params.name.is_none() {
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
            "SELECT id FROM directory_entry WHERE anilist_id = ? OR name = ? OR path = ?",
            (params.anilist_id, params.name, path),
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
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    entry_id: Option<i64>,
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
    Path(entry_id): Path<i64>,
    account: Account,
    Json(payload): Json<MoveDirectoryEntries>,
) -> Result<Json<BulkFileOperationResponse>, ApiError> {
    if !account.flags.is_editor() {
        return Err(ApiError::forbidden());
    }

    let Some(entry) = state.get_directory_entry_path(entry_id).await else {
        return Err(ApiError::not_found("Directory entry not found."));
    };
    let (entry_id, path) = match payload.entry_id {
        Some(entry_id) => {
            let Some(path) = state.get_directory_entry_path(entry_id).await else {
                return Err(ApiError::not_found(format!("Directory entry {entry_id} not found.")));
            };
            (entry_id, path)
        }
        None => raw_create_directory_entry(state, &account, payload.name, payload.anilist_id).await?,
    };

    let mut success = 0;
    let mut failed = 0;
    for file in payload.files {
        let from = entry.join(&file);
        let to = path.join(&file);
        match tokio::fs::rename(from, to).await {
            Ok(_) => success += 1,
            Err(_) => failed += 1,
        }
    }
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
}

async fn bulk_delete_files(
    State(state): State<AppState>,
    Path(entry_id): Path<i64>,
    account: Account,
    Json(payload): Json<BulkFilesPayload>,
) -> Result<Json<BulkFileOperationResponse>, ApiError> {
    if !account.flags.is_admin() {
        return Err(ApiError::forbidden());
    }

    let Some(entry) = state.get_directory_entry_path(entry_id).await else {
        return Err(ApiError::not_found("Directory entry not found."));
    };

    let mut success = 0;
    let mut failed = 0;
    if payload.delete_parent {
        state
            .database()
            .execute("DELETE FROM directory_entry WHERE id = ?", [entry_id])
            .await?;
        state.cached_directories().invalidate().await;
        tokio::fs::remove_dir_all(entry).await?;
    } else {
        for file in payload.files {
            let path = entry.join(&file);
            match tokio::fs::remove_file(path).await {
                Ok(_) => success += 1,
                Err(_) => failed += 1,
            }
        }
    }
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
        Some("srt" | "ass" | "ssa" | "zip" | "sub" | "sup") => {
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

async fn upload_file(
    State(state): State<AppState>,
    Path(entry_id): Path<i64>,
    Referrer(url): Referrer,
    _account: Account,
    flasher: Flasher,
    multipart: Multipart,
) -> Response {
    let Some(entry) = state.get_directory_entry_path(entry_id).await else {
        return flasher.add("Directory entry not found.").bail(&url);
    };

    let Ok(processed) = process_files(&entry, multipart).await else {
        return flasher.add("Internal error when processing files").bail(&url);
    };

    if processed.files.is_empty() {
        return flasher.add("Did not upload any files.").bail(&url);
    }

    let mut errored = 0usize;
    let total = processed.files.len();
    let mut set = JoinSet::new();
    for file in processed.files.into_iter() {
        set.spawn_blocking(move || file.write_to_disk());
    }

    while let Some(task) = set.join_next().await {
        match task {
            Ok(Ok(())) => continue,
            _ => errored += 1,
        }
    }

    let _ = state
        .database()
        .execute(
            "UPDATE directory_entry SET last_updated_at = CURRENT_TIMESTAMP WHERE id = ?",
            [entry_id],
        )
        .await;

    let message = if total > 0 && errored == 0 && processed.skipped == 0 {
        FlashMessage::success("Upload successful.")
    } else if errored == total {
        FlashMessage::error("Upload failed.")
    } else {
        let successful = total - errored;
        FlashMessage::warning(format!(
            "Uploaded {successful} file{}, {} {} skipped and {errored} failed",
            if successful == 1 { "" } else { "s" },
            processed.skipped,
            if processed.skipped == 1 { "was" } else { "were" },
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
                tracing::debug!("Skipping file in {}", path.display());
                continue;
            };
            tracing::info!("Found {} with {} bytes", path.display(), contents.len());
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
        (CONTENT_DISPOSITION, &format!("attachment; filename=\"{filename:?}\"")),
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

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/entry/:id", get(get_entry))
        .route("/entry/:id/download/*path", get(download_entry))
        .route(
            "/entry/create",
            post(create_directory_entry).layer(RateLimit::default().quota(5, 30.0).build()),
        )
        .route("/entry/:id/edit", post(edit_directory_entry))
        .route("/entry/:id/move", post(move_directory_entries))
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
}
