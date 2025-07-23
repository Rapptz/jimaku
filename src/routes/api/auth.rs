use axum::{
    extract::{FromRequestParts, Request, State},
    http::{header::AUTHORIZATION, request::Parts, HeaderMap},
    middleware::Next,
    response::Response,
};

use crate::{error::ApiError, AppState};

/// An API token
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ApiToken {
    pub id: i64,
}

async fn extract_api_token_from_headers(headers: &HeaderMap, state: &AppState) -> Option<ApiToken> {
    let auth = headers
        .get(AUTHORIZATION)
        .and_then(|x| x.to_str().ok())
        .map(String::from)?;
    let info = state.is_session_valid(&auth).await?;
    if info.api_key {
        Some(ApiToken { id: info.id })
    } else {
        None
    }
}

impl FromRequestParts<AppState> for ApiToken {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        extract_api_token_from_headers(&parts.headers, state)
            .await
            .ok_or_else(ApiError::unauthorized)
    }
}

pub async fn copy_api_token(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let api_token = extract_api_token_from_headers(request.headers(), &state).await;
    let mut response = next.run(request).await;
    if let Some(token) = api_token {
        response.extensions_mut().insert(token);
    }
    response
}
