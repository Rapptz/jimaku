use std::path::PathBuf;

use crate::{
    download::{validate_path, DownloadResponse},
    filters,
};
use askama::Template;
use axum::{
    extract::{Path, Query, Request, State},
    http::StatusCode,
    response::Redirect,
    routing::get,
    Extension, Json, Router,
};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use tower::ServiceExt as _;
use tower_http::services::ServeFile;

use crate::{
    cached::BodyCache,
    error::ApiError,
    models::Account,
    trash::{Trash, TrashListing},
    utils::logs_directory,
    AppState,
};

fn available_logs() -> std::io::Result<Vec<String>> {
    let path = logs_directory();
    let mut result = Vec::new();
    for entry in path.read_dir()? {
        let entry = entry?;
        let mut filename = entry.file_name().to_string_lossy().into_owned();
        if filename.ends_with(".log") {
            filename.truncate(filename.len() - 4);
        }
        result.push(filename);
    }
    result.sort_by(|a, b| b.cmp(a));
    Ok(result)
}

#[derive(Deserialize)]
struct LogsQuery {
    days: u8,
}

const DATE_FORMAT: &[time::format_description::FormatItem<'_>] =
    time::macros::format_description!("[year]-[month]-[day]");

async fn append_logs(path: PathBuf, buffer: &mut Vec<serde_json::Value>) -> std::io::Result<()> {
    let loaded = tokio::fs::read_to_string(path).await?;
    for line in loaded.lines() {
        let Ok(value) = serde_json::from_str(line) else {
            continue;
        };
        buffer.push(value);
    }
    Ok(())
}

async fn get_last_logs(account: Account, Query(query): Query<LogsQuery>) -> Result<Json<serde_json::Value>, ApiError> {
    if !account.flags.is_admin() {
        return Err(ApiError::forbidden());
    }

    let dir = logs_directory();
    let mut result = Vec::new();
    let mut date = OffsetDateTime::now_utc().date();
    for _ in 0..query.days.max(30) {
        let path = dir.join(date.format(&DATE_FORMAT)?).with_extension("log");
        if append_logs(path, &mut result).await.is_err() {
            break;
        }
        date = date.previous_day().unwrap();
    }

    Ok(Json(serde_json::Value::Array(result)))
}

async fn get_logs_from(Path(mut date): Path<String>, account: Account) -> Result<Json<serde_json::Value>, ApiError> {
    if !account.flags.is_admin() {
        return Err(ApiError::forbidden());
    }

    if date == "today" {
        date = OffsetDateTime::now_utc().date().format(&DATE_FORMAT)?;
    }

    let file = logs_directory().join(date).with_extension("log");
    let mut array = Vec::with_capacity(1000);
    append_logs(file, &mut array).await?;
    Ok(Json(serde_json::Value::Array(array)))
}

#[derive(Template)]
#[template(path = "admin.html")]
struct AdminIndexTemplate {
    account: Option<Account>,
    logs: Vec<String>,
}

async fn admin_index(account: Account) -> Result<AdminIndexTemplate, StatusCode> {
    if !account.flags.is_admin() {
        return Err(StatusCode::FORBIDDEN);
    }

    Ok(AdminIndexTemplate {
        account: Some(account),
        logs: available_logs().unwrap_or_default(),
    })
}

async fn invalidate_caches(
    State(state): State<AppState>,
    account: Account,
    Extension(cache): Extension<BodyCache>,
) -> Redirect {
    if account.flags.is_admin() {
        state.cached_directories().invalidate().await;
        state.clear_account_cache();
        state.clear_session_cache();
        cache.invalidate_all();
    }
    Redirect::to("/")
}

#[derive(Template)]
#[template(path = "admin_trash.html")]
struct AdminTrashTemplate {
    account: Option<Account>,
    listing: TrashListing,
    trash: Trash,
}

async fn show_trash(account: Account) -> Result<AdminTrashTemplate, Redirect> {
    if !account.flags.is_admin() {
        return Err(Redirect::to("/"));
    }

    let Ok(trash) = Trash::new() else {
        return Err(Redirect::to("/"));
    };

    let listing = trash.list().await.unwrap_or_default();

    Ok(AdminTrashTemplate {
        account: Some(account),
        trash,
        listing,
    })
}

#[derive(Debug, Deserialize, Copy, Clone, Eq, PartialEq, Hash)]
#[serde(rename_all = "lowercase")]
enum TrashRequestAction {
    Delete,
    Restore,
}

#[derive(Deserialize)]
struct TrashRequest {
    files: Vec<PathBuf>,
    action: TrashRequestAction,
}

#[derive(Serialize, Default)]
struct TrashResponse {
    success: usize,
    failed: usize,
}

async fn trash_management(
    account: Account,
    Json(payload): Json<TrashRequest>,
) -> Result<Json<TrashResponse>, ApiError> {
    if !account.flags.is_admin() {
        return Err(ApiError::forbidden());
    }

    let trash = Trash::new()?;
    let mut response = TrashResponse::default();
    for filename in payload.files {
        let result = match payload.action {
            TrashRequestAction::Delete => trash.delete(filename).await,
            TrashRequestAction::Restore => trash.restore(filename).await,
        };
        match result {
            Ok(()) => response.success += 1,
            Err(_) => response.failed += 1,
        }
    }

    Ok(Json(response))
}

async fn download_trash(account: Account, Path(path): Path<String>, req: Request) -> DownloadResponse {
    if !account.flags.is_admin() {
        return DownloadResponse::NotFound;
    }

    let Ok(trash) = Trash::new() else {
        return DownloadResponse::NotFound;
    };
    let Some(path) = validate_path(trash.files_path(), path.as_str()) else {
        return DownloadResponse::NotFound;
    };

    match ServeFile::new(path).oneshot(req).await {
        Ok(res) => DownloadResponse::File(res.map(axum::body::Body::new)),
        Err(_) => DownloadResponse::NotFound,
    }
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/admin/logs", get(get_last_logs))
        .route("/admin/logs/:date", get(get_logs_from))
        .route("/admin", get(admin_index))
        .route("/admin/trash", get(show_trash).post(trash_management))
        .route("/admin/trash/download/*path", get(download_trash))
        .route("/admin/cache/invalidate", get(invalidate_caches))
}
