use crate::{
    cached::BodyCache,
    error::ApiError,
    filters,
    flash::Flashes,
    headers::{AcceptEncoding, UserAgent},
    models::{Account, AccountCheck},
    utils::HtmlPage,
};
use askama::Template;
use axum::{
    extract::{Path, Query, State},
    response::IntoResponse,
    routing::get,
    Extension, Router,
};
use reqwest::header::{CONTENT_TYPE, USER_AGENT};

use crate::{models::DirectoryEntry, AppState};

mod admin;
mod api;
mod audit;
mod auth;
mod entry;
mod notification;
mod opensearch;
mod relations;

pub use api::{copy_api_token, ApiToken, SearchQuery};

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
    editor: bool,
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
    let editor = account.flags().is_editor();
    let template = ListingTemplate {
        account,
        entries: entries.iter().filter(|e| e.flags.is_anime()),
        flashes,
        url: state.config().canonical_url(),
        anime: true,
        editor,
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
    let editor = account.flags().is_editor();
    let template = ListingTemplate {
        account,
        entries: entries.iter().filter(|e| !e.flags.is_anime()),
        flashes,
        url: state.config().url_to("/dramas"),
        anime: false,
        editor,
    };
    cacher.cache_template("dramas", template, encoding, bypass_cache).await
}

#[derive(Template)]
#[template(path = "help.html")]
struct HelpTemplate {
    account: Option<Account>,
}

async fn help_page(account: Option<Account>) -> impl IntoResponse {
    HtmlPage(HelpTemplate { account })
}

#[derive(Template)]
#[template(path = "contact.html")]
struct ContactTemplate {
    account: Option<Account>,
}

async fn contact_page(account: Option<Account>) -> impl IntoResponse {
    HtmlPage(ContactTemplate { account })
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

#[derive(Template)]
#[template(path = "anilist.html")]
struct AniListTemplate {
    account: Option<Account>,
    user_name: String,
}

async fn show_anilist_page(account: Option<Account>, Path(user_name): Path<String>) -> impl IntoResponse {
    HtmlPage(AniListTemplate { account, user_name })
}

pub fn all() -> Router<AppState> {
    Router::new()
        .route("/", get(index))
        .route("/dramas", get(dramas))
        .route("/help", get(help_page))
        .route("/contact", get(contact_page))
        .route("/download-zip", get(bypass_download_zip_cors))
        .route("/anilist/{name}", get(show_anilist_page))
        .merge(auth::routes())
        .merge(entry::routes())
        .merge(admin::routes())
        .merge(audit::routes())
        .merge(relations::routes())
        .merge(opensearch::routes())
        .merge(notification::routes())
        .nest("/api", api::routes())
}
