use axum::{
    extract::State,
    routing::{get, post},
    Json, Router,
};
use serde::Serialize;

use crate::{error::ApiError, models::Account, relations::Relations, AppState};

async fn get_anime_relations(State(state): State<AppState>) -> Json<Relations> {
    Json(state.anime_relations().await.clone())
}

#[derive(Debug, Serialize)]
struct RelationDates {
    last_modified: time::Date,
    #[serde(with = "time::serde::rfc3339")]
    created_at: time::OffsetDateTime,
}

async fn get_anime_relations_date(State(state): State<AppState>) -> Json<RelationDates> {
    let relations = state.anime_relations().await;
    Json(RelationDates {
        last_modified: relations.last_modified,
        created_at: relations.created_at,
    })
}

async fn update_anime_relations(account: Account, State(state): State<AppState>) -> Result<Json<time::Date>, ApiError> {
    if !account.flags.is_admin() {
        return Err(ApiError::forbidden());
    }

    let new = Relations::load(&state.client).await?;
    let date = new.last_modified;
    state.set_anime_relations(new).await;
    Ok(Json(date))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/anime-relations", get(get_anime_relations))
        .route("/anime-relations/date", get(get_anime_relations_date))
        .route("/anime-relations/update", post(update_anime_relations))
}
