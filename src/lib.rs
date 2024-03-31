pub mod anilist;
pub mod auth;
pub mod cached;
pub mod cli;
mod config;
pub mod database;
pub mod discord;
pub mod error;
pub mod filters;
pub mod flash;
pub mod headers;
pub mod key;
pub mod kitsunekko;
pub mod logging;
pub mod models;
pub mod ratelimit;
pub mod routes;
mod state;
pub mod tmdb;
pub mod token;
pub mod utils;

pub use cli::{Command, PROGRAM_NAME};
pub use config::{Config, CONFIG};
pub use database::Database;
pub use state::AppState;
pub use utils::MAX_BODY_SIZE;
pub use utils::MAX_UPLOAD_SIZE;

/// A middleware responsible for parsing cookies into a Vec<Cookie> extension for use
/// for other cookie-related middleware.
///
/// This middleware must come *after* the cookie related middlewares.
pub async fn parse_cookies(mut req: axum::extract::Request, next: axum::middleware::Next) -> axum::response::Response {
    let cookies = req
        .headers()
        .get_all(axum::http::header::COOKIE)
        .iter()
        .filter_map(|header| header.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|cookie| cookie::Cookie::parse_encoded(cookie.trim().to_owned()).ok())
        .collect::<Vec<_>>();

    req.extensions_mut().insert(cookies);
    next.run(req).await
}
