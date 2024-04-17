use axum::{
    extract::FromRequestParts,
    http::{header::AUTHORIZATION, request::Parts},
};

use crate::{error::ApiError, AppState};

/// An API token
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ApiToken {
    pub id: i64,
}

#[async_trait::async_trait]
impl FromRequestParts<AppState> for ApiToken {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        match parts
            .headers
            .get(AUTHORIZATION)
            .and_then(|x| x.to_str().ok())
            .map(String::from)
        {
            Some(auth) => match state.is_session_valid(&auth).await {
                Some(info) if info.api_key => Ok(Self { id: info.id }),
                _ => Err(ApiError::unauthorized()),
            },
            None => Err(ApiError::unauthorized()),
        }
    }
}
