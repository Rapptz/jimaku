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
struct IndexTemplate<'a> {
    account: Option<Account>,
    entries: &'a [DirectoryEntry],
    flashes: Flashes,
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
    let template = IndexTemplate {
        account,
        entries: entries.as_slice(),
        flashes,
    };
    cacher.cache_template("index", template, encoding, bypass_cache).await
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
        .route("/help", get(help_page))
        .merge(auth::routes())
        .merge(entry::routes())
        .merge(admin::routes())
}
