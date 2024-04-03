use std::convert::Infallible;

use axum::{
    extract::FromRequestParts,
    http::{
        header::{ACCEPT_ENCODING, REFERER, USER_AGENT},
        request::Parts,
        HeaderValue, StatusCode, Uri,
    },
};

use crate::AppState;

#[derive(Debug, Clone)]
pub struct Referrer(pub String);

#[async_trait::async_trait]
impl FromRequestParts<AppState> for Referrer {
    type Rejection = (StatusCode, &'static str);

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        if let Some(referrer) = get_safe_referrer(state.config(), parts.headers.get(REFERER)) {
            Ok(Referrer(referrer.to_string()))
        } else {
            Err((StatusCode::BAD_REQUEST, "`Referer` header is missing or invalid"))
        }
    }
}

#[derive(Debug, Default, Copy, Clone)]
pub struct AcceptEncoding {
    pub brotli: bool,
    pub gzip: bool,
}

#[async_trait::async_trait]
impl<S> FromRequestParts<S> for AcceptEncoding
where
    S: Send + Sync,
{
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let Some(header) = parts.headers.get(ACCEPT_ENCODING).and_then(|x| x.to_str().ok()) else {
            return Ok(Self::default());
        };

        let mut result = Self::default();
        for value in header.split(',').map(|s| s.trim()) {
            let inner = value.split_once(";q=").map(|(lhs, _)| lhs).unwrap_or(value);
            match inner {
                "br" => result.brotli = true,
                "gzip" => result.gzip = true,
                _ => continue,
            }
        }
        Ok(result)
    }
}

/// Returns a safe URL to redirect from based off of the Referer header.
///
/// If no header is found or the URL is unsafe then this returns None.
pub fn get_safe_referrer(config: &crate::Config, header: Option<&HeaderValue>) -> Option<String> {
    let uri = Uri::try_from(header?.as_bytes()).ok()?;
    match uri.host() {
        Some(host) => config.is_valid_host(host).then(|| uri.to_string()),
        // If this is None then it's a relative URI and should be fine, even if it 404s.
        None => Some(uri.to_string()),
    }
}

#[derive(Debug, Clone)]
pub struct UserAgent(pub String);

#[async_trait::async_trait]
impl<S> FromRequestParts<S> for UserAgent
where
    S: Send + Sync,
{
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(Self(
            parts
                .headers
                .get(USER_AGENT)
                .and_then(|x| x.to_str().ok())
                .map(String::from)
                .unwrap_or_default(),
        ))
    }
}
