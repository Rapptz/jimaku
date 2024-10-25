mod auth;
mod entries;
pub mod utils;

use crate::{filters, models::Account, ratelimit::RateLimit, AppState};
use askama::Template;
use axum::{
    extract::State,
    http::{
        header::{AUTHORIZATION, USER_AGENT},
        Method,
    },
    routing::{get, post},
    Json, Router,
};
use tower_http::cors::{AllowOrigin, CorsLayer};
use utoipa::{
    openapi::security::{ApiKey, ApiKeyValue, SecurityScheme},
    Modify, OpenApi,
};

use crate::error::ApiError;
pub use auth::{copy_api_token, ApiToken};
pub use entries::SearchQuery;

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Jimaku",
        description = include_str!("../../../templates/api_description.md"),
        version = "beta"
    ),
    paths(
        entries::get_entry_by_id,
        entries::get_entry_files,
        entries::search_entries,
        entries::create_entry,
        entries::upload_files,
    ),
    components(
        schemas(
            ApiError,
            crate::models::EntryFlags,
            crate::models::DirectoryEntry,
            crate::routes::entry::FileEntry,
            crate::routes::entry::UploadResult,
        ),
        responses(utils::RateLimitResponse),
    ),
    modifiers(&RequiredAuthentication),
    tags(
        (name = "entries", description = "Working with entries on the site")
    )
)]
pub struct Schema;

struct RequiredAuthentication;

impl Modify for RequiredAuthentication {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        if let Some(components) = openapi.components.as_mut() {
            components.add_security_scheme(
                "api_key",
                SecurityScheme::ApiKey(ApiKey::Header(ApiKeyValue::new("Authorization"))),
            )
        }
    }
}

#[derive(Template)]
#[template(path = "api.html")]
struct ApiDocumentation {
    api_key: String,
}

async fn spec() -> Json<utoipa::openapi::OpenApi> {
    Json(Schema::openapi())
}

async fn docs(State(state): State<AppState>, account: Option<Account>) -> ApiDocumentation {
    let api_key = if let Some(acc) = &account {
        state.get_api_key(acc.id).await.unwrap_or_default()
    } else {
        String::new()
    };
    ApiDocumentation { api_key }
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/openapi.json", get(spec))
        .route("/docs", get(docs))
        .route("/entries/:id", get(entries::get_entry_by_id))
        .route("/entries/:id/files", get(entries::get_entry_files))
        .route("/entries/search", get(entries::search_entries))
        .route("/entries", post(entries::create_entry))
        .route("/entries/:id/upload", post(entries::upload_files))
        .route_layer(RateLimit::default().quota(25, 60.0).build())
        .route_layer(
            CorsLayer::new()
                .allow_methods([Method::GET, Method::POST])
                .allow_credentials(true)
                .allow_origin(AllowOrigin::mirror_request())
                .allow_headers([AUTHORIZATION, USER_AGENT]),
        )
}
