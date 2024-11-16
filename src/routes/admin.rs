use std::path::PathBuf;

use crate::{
    audit,
    download::{validate_path, DownloadResponse},
    filters,
    logging::RequestLogEntry,
    utils::logs_directory,
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
    AppState,
};

#[derive(Deserialize)]
struct LogsQuery {
    begin: Option<i64>,
    end: Option<i64>,
    days: Option<u8>,
}

fn datetime_to_unix_ms(dt: OffsetDateTime) -> i64 {
    (dt.unix_timestamp_nanos() / 1_000_000) as i64
}

const DATE_FORMAT: &[time::format_description::FormatItem<'_>] =
    time::macros::format_description!("[year]-[month]-[day]");

impl LogsQuery {
    fn limit(&self) -> (i64, i64) {
        // This is going to be represented as `ts >= begin AND ts <= end`
        // The default is "today" with proper day boundaries
        if let Some(days) = self.days {
            let now = OffsetDateTime::now_utc();
            // This API sucks since it's not calendar aware but nothing I can do about that
            let begin = now.saturating_sub(time::Duration::days(days as i64));
            return (datetime_to_unix_ms(begin), datetime_to_unix_ms(now));
        }

        match (self.begin, self.end) {
            (None, None) => {
                let now = OffsetDateTime::now_utc();
                let start = now.replace_time(time::Time::MIDNIGHT);
                let end = now.replace_time(time::macros::time!(23:59));
                (datetime_to_unix_ms(start), datetime_to_unix_ms(end))
            }
            (None, Some(end)) => (0, end),
            (Some(begin), None) => (begin, i64::MAX),
            (Some(begin), Some(end)) => (begin, end),
        }
    }
}

async fn get_last_logs(
    account: Account,
    State(state): State<AppState>,
    Query(query): Query<LogsQuery>,
) -> Result<Json<Vec<RequestLogEntry>>, ApiError> {
    if !account.flags.is_admin() {
        return Err(ApiError::forbidden());
    }

    let (begin, end) = query.limit();
    Ok(Json(
        state
            .requests
            .query("SELECT * FROM request WHERE ts >= ? AND ts <= ?", (begin, end))
            .await?,
    ))
}

async fn get_server_logs(account: Account) -> Result<Json<serde_json::Value>, ApiError> {
    if !account.flags.is_admin() {
        return Err(ApiError::forbidden());
    }

    let today = OffsetDateTime::now_utc().date();
    let path = logs_directory().join(today.format(&DATE_FORMAT)?).with_extension("log");
    let file = tokio::fs::read_to_string(path).await?;
    let mut result = Vec::new();
    for line in file.lines() {
        let Ok(value) = serde_json::from_str(line) else {
            continue;
        };
        result.push(value);
    }

    Ok(Json(serde_json::Value::Array(result)))
}

#[derive(Template)]
#[template(path = "admin.html")]
struct AdminIndexTemplate {
    account: Option<Account>,
}

async fn admin_index(account: Account) -> Result<AdminIndexTemplate, StatusCode> {
    if !account.flags.is_admin() {
        return Err(StatusCode::FORBIDDEN);
    }

    Ok(AdminIndexTemplate { account: Some(account) })
}

async fn admin_user_by_id(
    State(state): State<AppState>,
    account: Account,
    Path(user_id): Path<i64>,
) -> Result<Redirect, StatusCode> {
    if !account.flags.is_admin() {
        return Err(StatusCode::FORBIDDEN);
    }
    match state.get_account(user_id).await {
        Some(acc) => Ok(Redirect::to(&format!("/user/{}", acc.name))),
        None => Ok(Redirect::to("/")),
    }
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
    files: Vec<String>,
    action: TrashRequestAction,
}

#[derive(Serialize, Default)]
struct TrashResponse {
    success: usize,
    failed: usize,
}

async fn trash_management(
    State(state): State<AppState>,
    account: Account,
    Json(payload): Json<TrashRequest>,
) -> Result<Json<TrashResponse>, ApiError> {
    if !account.flags.is_admin() {
        return Err(ApiError::forbidden());
    }

    let trash = Trash::new()?;
    let mut response = TrashResponse::default();
    let mut data = audit::TrashAction {
        restore: payload.action == TrashRequestAction::Restore,
        files: Vec::with_capacity(payload.files.len()),
    };
    for name in payload.files {
        let filename = PathBuf::from(&name);
        let result = match payload.action {
            TrashRequestAction::Delete => trash.delete(filename).await,
            TrashRequestAction::Restore => trash.restore(filename).await,
        };
        data.add_file(name, result.is_err());
        match result {
            Ok(()) => response.success += 1,
            Err(_) => response.failed += 1,
        }
    }

    state
        .audit(audit::AuditLogEntry::new(data).with_account(account.id))
        .await;
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
        .route("/admin/logs/server", get(get_server_logs))
        .route("/admin", get(admin_index))
        .route("/admin/user/:id", get(admin_user_by_id))
        .route("/admin/trash", get(show_trash).post(trash_management))
        .route("/admin/trash/download/*path", get(download_trash))
        .route("/admin/cache/invalidate", get(invalidate_caches))
}
