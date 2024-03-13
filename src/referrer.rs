use axum::{
    extract::FromRequestParts,
    http::{header::REFERER, request::Parts, HeaderValue, StatusCode, Uri},
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
