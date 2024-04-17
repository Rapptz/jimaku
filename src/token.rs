use std::convert::Infallible;

use axum::{
    extract::FromRequestParts,
    http::{
        header::{LOCATION, SET_COOKIE},
        request::Parts,
        Extensions, HeaderValue, StatusCode,
    },
    response::{IntoResponse, IntoResponseParts, Response},
};
use base64::{prelude::BASE64_URL_SAFE_NO_PAD, Engine};
use cookie::Cookie;
use hmac::Mac;

use crate::{key::SecretKey, models::Account, AppState};

/// An authentication token.
///
/// In order to prevent tampering, the token is split into two sections:
///
/// <token>.<hmac>
///
/// The token is the base64 binary data of the information below. The hmac is just
/// the hmac of the actual token data preceeding it. The hmac is *not* stored
/// in the database. It is merely an anti-tamper aspect when using cookies.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct Token {
    pub id: i64,
    pub api_key: bool,
    pub nonce: SecretKey,
}

impl Token {
    pub const BINARY_SIZE: usize = 8 + 1 + 1 + 1 + std::mem::size_of::<SecretKey>();

    /// Creates a new token for the given account ID.
    pub fn new(id: i64) -> anyhow::Result<Self> {
        Ok(Self {
            id,
            api_key: false,
            nonce: SecretKey::random()?,
        })
    }

    /// Converts a base64 string to a Token if possible
    pub fn from_base64(s: &str) -> Option<Self> {
        let bytes = BASE64_URL_SAFE_NO_PAD.decode(s.as_bytes()).ok()?;
        Self::from_bytes(&bytes)
    }

    /// Converts a binary string to a Token if possible
    ///
    /// This is the inverse of [`Self::to_bytes`].
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != Self::BINARY_SIZE {
            return None;
        }
        let mut id_bytes = [0u8; 8];
        id_bytes.copy_from_slice(&bytes[..8]);
        let id = i64::from_be_bytes(id_bytes);
        if bytes[8] != b'.' && bytes[10] != b'.' {
            return None;
        }

        let api_key = bytes[9] != 0;
        let mut nonce = SecretKey(Default::default());
        nonce.0.copy_from_slice(&bytes[11..]);
        Some(Self { id, api_key, nonce })
    }

    /// Validates the signed token to ensure it hasn't been tampered.
    pub fn from_signed(value: &str, key: &SecretKey) -> Option<Self> {
        let (payload, signature) = value.split_once('.')?;
        let bytes = BASE64_URL_SAFE_NO_PAD.decode(payload.as_bytes()).ok()?;
        let signature = BASE64_URL_SAFE_NO_PAD.decode(signature.as_bytes()).ok()?;
        let mut hmac = key.hmac();
        hmac.update(&bytes);
        hmac.verify_slice(&signature).ok()?;
        Self::from_bytes(&bytes)
    }

    /// Returns the binary version of the token
    pub fn to_bytes(&self) -> [u8; Self::BINARY_SIZE] {
        // The token format is a simple binary encoded form of:
        // <id>.<api>.<nonce>
        // The numbers are encoded in network byte order (aka big endian)
        let mut bytes = [0; Self::BINARY_SIZE];
        bytes[..8].copy_from_slice(&self.id.to_be_bytes());
        bytes[8] = b'.';
        bytes[9] = self.api_key as u8;
        bytes[10] = b'.';
        bytes[11..].copy_from_slice(&self.nonce.0);
        bytes
    }

    /// Returns the base64 representation of the binary form of the token.
    ///
    /// This is what's actually stored in the database as a session ID.
    pub fn base64(&self) -> String {
        let bytes = self.to_bytes();
        let mut buffer = String::with_capacity(4 * (bytes.len() / 3));
        BASE64_URL_SAFE_NO_PAD.encode_string(bytes, &mut buffer);
        buffer
    }

    /// Returns the signed representation of the binary form of the token.
    ///
    /// To handle this, use [`Self::from_signed`].
    pub fn signed(&self, key: &SecretKey) -> String {
        let mut mac = key.hmac();
        let bytes = self.to_bytes();
        mac.update(&bytes);
        let signature = mac.finalize().into_bytes();
        let mut buffer = String::with_capacity((4 * (signature.len() / 3)) + (4 * (bytes.len() / 3)) + 1);
        BASE64_URL_SAFE_NO_PAD.encode_string(bytes, &mut buffer);
        buffer.push('.');
        BASE64_URL_SAFE_NO_PAD.encode_string(signature, &mut buffer);
        buffer
    }

    /// Returns the Cookie containing the signed token.
    pub fn to_cookie(&self, key: &SecretKey) -> Cookie<'static> {
        Cookie::build(("token", self.signed(key)))
            .path("/")
            .same_site(cookie::SameSite::Lax)
            .http_only(true)
            .build()
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
            let cookie = self.to_cookie(key);
            res.headers_mut()
                .insert(SET_COOKIE, HeaderValue::from_str(&cookie.to_string()).unwrap());
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
    Token::from_signed(cookie.value(), key)
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
        let cookie = parts
            .extensions
            .get::<Vec<Cookie>>()
            .and_then(|cookies| cookies.iter().find(|c| c.name() == "token"))
            .ok_or(TokenRejection)?;

        let token = parts
            .extensions
            .get::<SecretKey>()
            .and_then(|key| Token::from_signed(cookie.value(), key))
            .ok_or(TokenRejection)?;

        // This unwrap is safe because it's validated above
        let (session_id, _) = cookie.value().split_once('.').unwrap();
        state
            .get_session_account(session_id, token.id, false)
            .await
            .ok_or(TokenRejection)
    }
}
