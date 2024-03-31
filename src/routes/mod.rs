use crate::{
    cached::BodyCache,
    filters,
    flash::Flashes,
    headers::AcceptEncoding,
    models::{Account, AccountCheck},
};
use askama::Template;
use axum::{extract::State, response::IntoResponse, routing::get, Extension, Router};

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

pub fn all() -> Router<AppState> {
    Router::new()
        .route("/", get(index))
        .route("/dramas", get(dramas))
        .route("/help", get(help_page))
        .merge(auth::routes())
        .merge(entry::routes())
        .merge(admin::routes())
}
