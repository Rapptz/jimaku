use std::{
    convert::Infallible,
    time::{Duration, SystemTime},
};

use axum::{
    extract::FromRequestParts,
    http::{
        header::{LOCATION, SET_COOKIE},
        request::Parts,
        Extensions, HeaderValue, StatusCode,
    },
    response::{IntoResponse, IntoResponseParts, Response},
};
use cookie::Cookie;
use serde::{Deserialize, Serialize};

use crate::{key::SecretKey, models::Account, AppState};

/// An authentication token.
///
/// This token is signed using the secret key to ensure it's not tampered. It's similar to a JWT, except
/// manually implemented to save on dependencies.
///
/// The format is a simple `<base64 payload>.<base64 signature>`. The signature is a HMACSHA256 of
/// the payload with the given secret key.
///
/// Since this is essentially a JWT there is no method of revocation on password change.
#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct Token {
    pub id: i64,
    #[serde(with = "timestamp_ms")]
    #[serde(rename = "exp")]
    pub expires: SystemTime,
}

mod timestamp_ms {
    use serde::{self, Deserialize, Deserializer, Serializer};
    use std::time::{Duration, SystemTime};

    pub fn serialize<S>(value: &SystemTime, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let duration = value.duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default();
        serializer.serialize_f64(duration.as_secs_f64())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<SystemTime, D::Error>
    where
        D: Deserializer<'de>,
    {
        let secs = f64::deserialize(deserializer)?;
        SystemTime::UNIX_EPOCH
            .checked_add(Duration::from_secs_f64(secs))
            .ok_or_else(|| serde::de::Error::custom("invalid timestamp"))
    }
}

/// The default expiry of an auth token in seconds
///
/// Currently 90 days
const DEFAULT_EXPIRY: u64 = 3600 * 24 * 90;

impl Token {
    /// Creates a new token for the given account ID.
    pub fn new(id: i64) -> Self {
        Self {
            id,
            expires: SystemTime::now() + Duration::from_secs(DEFAULT_EXPIRY),
        }
    }

    /// Returns the Cookie containing the signed token.
    pub fn to_cookie(self, key: &SecretKey) -> anyhow::Result<Cookie<'static>> {
        Ok(Cookie::build(("token", key.sign(&self)?))
            .path("/")
            .same_site(cookie::SameSite::Lax)
            .http_only(true)
            .expires(cookie::Expiration::DateTime(self.expires.into()))
            .build())
    }
}

impl IntoResponseParts for Token {
    type Error = Infallible;

    fn into_response_parts(
        self,
        mut res: axum::response::ResponseParts,
    ) -> Result<axum::response::ResponseParts, Self::Error> {
        // This is a silent failure unfortunately
        if let Some(key) = res.extensions().get::<SecretKey>() {
            if let Ok(cookie) = self.to_cookie(key) {
                res.headers_mut()
                    .insert(SET_COOKIE, HeaderValue::from_str(&cookie.to_string()).unwrap());
            }
        }
        Ok(res)
    }
}

#[derive(Copy, Clone)]
pub struct TokenRejection;

impl IntoResponse for TokenRejection {
    fn into_response(self) -> Response {
        let cookie = Cookie::build(("token", ""))
            .path("/")
            .expires(cookie::time::OffsetDateTime::UNIX_EPOCH)
            .build()
            .to_string();
        (
            StatusCode::SEE_OTHER,
            [
                (LOCATION, HeaderValue::from_str("/").unwrap()),
                (SET_COOKIE, HeaderValue::from_str(&cookie).unwrap()),
            ],
        )
            .into_response()
    }
}

/// Synchronously obtains an authenticated cookie
pub fn get_token_from_request(exts: &Extensions) -> Option<Token> {
    let cookie = exts
        .get::<Vec<Cookie>>()
        .and_then(|cookies| cookies.iter().find(|c| c.name() == "token"))?;

    let key = exts.get::<SecretKey>()?;
    key.verify::<Token>(cookie.value())
}

#[async_trait::async_trait]
impl<S> FromRequestParts<S> for Token
where
    S: Send + Sync,
{
    type Rejection = TokenRejection;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let token = get_token_from_request(&parts.extensions).ok_or(TokenRejection)?;
        parts.extensions.insert(token.clone());
        Ok(token)
    }
}

#[async_trait::async_trait]
impl FromRequestParts<AppState> for Account {
    type Rejection = TokenRejection;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let token = get_token_from_request(&parts.extensions).ok_or(TokenRejection)?;
        state.get_account(token.id).await.ok_or(TokenRejection)
    }
}
