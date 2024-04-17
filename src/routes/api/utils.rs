use async_trait::async_trait;
use axum::{
    extract::{path::Path, FromRequest, FromRequestParts, Query, Request},
    response::{IntoResponse, Response},
    Json,
};
use serde::de::DeserializeOwned;

use crate::error::ApiError;

pub struct ApiPath<T>(pub T);

#[async_trait]
impl<S, T> FromRequestParts<S> for ApiPath<T>
where
    T: DeserializeOwned + Send,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut axum::http::request::Parts, state: &S) -> Result<Self, Self::Rejection> {
        match Path::<T>::from_request_parts(parts, state).await {
            Ok(value) => Ok(Self(value.0)),
            Err(rejection) => Err(ApiError::new(rejection.to_string())),
        }
    }
}

pub struct ApiQuery<T>(pub T);

#[async_trait]
impl<S, T> FromRequestParts<S> for ApiQuery<T>
where
    T: DeserializeOwned + Send,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut axum::http::request::Parts, state: &S) -> Result<Self, Self::Rejection> {
        match Query::<T>::from_request_parts(parts, state).await {
            Ok(value) => Ok(Self(value.0)),
            Err(rejection) => Err(ApiError::new(rejection.to_string())),
        }
    }
}

pub struct ApiJson<T>(pub T);

#[async_trait]
impl<S, T> FromRequest<S> for ApiJson<T>
where
    // these trait bounds are copied from `impl FromRequest for axum::extract::path::Path`
    T: DeserializeOwned + Send,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        match Json::<T>::from_request(req, state).await {
            Ok(value) => Ok(Self(value.0)),
            Err(rejection) => Err(ApiError::new(rejection.to_string())),
        }
    }
}

impl<T> IntoResponse for ApiJson<T>
where
    axum::Json<T>: IntoResponse,
{
    fn into_response(self) -> Response {
        axum::Json(self.0).into_response()
    }
}

/// Rate limit exceeded.
#[derive(utoipa::ToResponse)]
#[response(headers(
    ("x-ratelimit-limit" = u16, description = "The number of requests you can make"),
    ("x-ratelimit-remaining" = u16, description = "The number of requests remaining"),
    ("x-ratelimit-reset" = f32, description = "The time, in UNIX timestamp seconds, when you can make requests again. Note this has a fractional component for milliseconds."),
    ("x-ratelimit-reset-after" = f32, description = "The number of seconds before you can try again. Note this has a fractional component for milliseconds."),
))]
pub struct RateLimitResponse(#[to_schema] ApiError);
