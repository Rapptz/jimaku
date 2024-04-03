use crate::{
    cached::BodyCache,
    error::ApiError,
    filters,
    flash::Flashes,
    headers::{AcceptEncoding, UserAgent},
    models::{Account, AccountCheck},
};
use askama::Template;
use axum::{
    extract::{Query, State},
    response::IntoResponse,
    routing::get,
    Extension, Router,
};
use reqwest::header::{CONTENT_TYPE, USER_AGENT};

use crate::{models::DirectoryEntry, AppState};

mod admin;
mod auth;
mod entry;

#[derive(Template)]
#[template(path = "index.html")]
struct ListingTemplate<'a, It>
where
    It: Iterator<Item = &'a DirectoryEntry> + Clone,
{
    account: Option<Account>,
    entries: It,
    flashes: Flashes,
    url: String,
    anime: bool,
}

async fn index(
    State(state): State<AppState>,
    account: Option<Account>,
    flashes: Flashes,
    encoding: AcceptEncoding,
    Extension(cacher): Extension<BodyCache>,
) -> impl IntoResponse {
    let entries = state.directory_entries().await;
    let bypass_cache = account.is_some();
    let template = ListingTemplate {
        account,
        entries: entries.iter().filter(|e| e.flags.is_anime()),
        flashes,
        url: state.config().canonical_url(),
        anime: true,
    };
    cacher.cache_template("index", template, encoding, bypass_cache).await
}

async fn dramas(
    State(state): State<AppState>,
    account: Option<Account>,
    flashes: Flashes,
    encoding: AcceptEncoding,
    Extension(cacher): Extension<BodyCache>,
) -> impl IntoResponse {
    let entries = state.directory_entries().await;
    let bypass_cache = account.is_some();
    let template = ListingTemplate {
        account,
        entries: entries.iter().filter(|e| !e.flags.is_anime()),
        flashes,
        url: state.config().url_to("/dramas"),
        anime: false,
    };
    cacher.cache_template("dramas", template, encoding, bypass_cache).await
}

#[derive(Template)]
#[template(path = "help.html")]
struct HelpTemplate {
    account: Option<Account>,
}

async fn help_page(account: Option<Account>) -> impl IntoResponse {
    HelpTemplate { account }
}

#[derive(serde::Deserialize)]
struct BypassCorsDownloadZip {
    url: String,
}

async fn bypass_download_zip_cors(
    State(state): State<AppState>,
    account: Account,
    user_agent: UserAgent,
    Query(query): Query<BypassCorsDownloadZip>,
) -> Result<impl IntoResponse, ApiError> {
    if !account.flags.is_editor() {
        return Err(ApiError::forbidden());
    }

    let response = state
        .client
        .get(query.url)
        .header(USER_AGENT, &user_agent.0)
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(ApiError::new(format!(
            "URL responded with {}",
            response.status().as_u16()
        )));
    }

    match response.headers().get(CONTENT_TYPE) {
        None => return Err(ApiError::new("URL did not provide a content-type header")),
        Some(header) => {
            if header.as_bytes() != b"application/zip" && header.as_bytes() != b"application/octet-stream" {
                return Err(ApiError::new("URL did not provide an appropriate content-type header"));
            }
        }
    };

    Ok(response.bytes().await?)
}

pub fn all() -> Router<AppState> {
    Router::new()
        .route("/", get(index))
        .route("/dramas", get(dramas))
        .route("/help", get(help_page))
        .route("/download-zip", get(bypass_download_zip_cors))
        .merge(auth::routes())
        .merge(entry::routes())
        .merge(admin::routes())
}
