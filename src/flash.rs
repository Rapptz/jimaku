use std::{
    convert::Infallible,
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{
    extract::{FromRequestParts, Request},
    http::{header::SET_COOKIE, request::Parts, HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Redirect, Response},
};
use cookie::Cookie;
use serde::{Deserialize, Serialize};

use crate::key::SecretKey;

// Clippy does not understand Bytes layout (https://github.com/rust-lang/rust-clippy/issues/5812)
#[allow(clippy::declare_interior_mutable_const)]
const REMOVE_FLASH_MESSAGES: HeaderValue =
    HeaderValue::from_static("flash_messages=; Path=/; HttpOnly; SameSite=Lax; Expires=Thu, 01 Jan 1970 00:00:00 GMT");

/// A container responsible for handling flash messages
#[derive(Debug, Clone, Default)]
pub struct Flasher {
    data: Arc<Mutex<Vec<FlashMessage>>>,
}

/// A read-only container for received flash messages from a prior request.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Flashes(Vec<FlashMessage>);

impl std::ops::Deref for Flashes {
    type Target = [FlashMessage];

    fn deref(&self) -> &Self::Target {
        self.0.as_slice()
    }
}

/// The data that represents an actual flash emssage.
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Default, Clone)]
pub struct FlashMessage {
    /// The content of the message.
    #[serde(rename = "c")]
    pub content: String,
    /// The level of the message
    #[serde(rename = "l")]
    pub level: FlashLevel,
}

impl FlashMessage {
    /// Creates a new flash message with the given content and level.
    pub fn new(content: impl Into<String>, level: FlashLevel) -> Self {
        Self {
            content: content.into(),
            level,
        }
    }

    /// Changes the flash message level.
    pub fn with_level(mut self, level: FlashLevel) -> Self {
        self.level = level;
        self
    }

    /// Creates a new flash message with the given content with Info level.
    pub fn info(content: impl Into<String>) -> Self {
        Self::new(content, FlashLevel::Info)
    }

    /// Creates a new flash message with the given content with Success level.
    pub fn success(content: impl Into<String>) -> Self {
        Self::new(content, FlashLevel::Success)
    }

    /// Creates a new flash message with the given content with Warning level.
    pub fn warning(content: impl Into<String>) -> Self {
        Self::new(content, FlashLevel::Warning)
    }

    /// Creates a new flash message with the given content with Error level.
    pub fn error(content: impl Into<String>) -> Self {
        Self::new(content, FlashLevel::Error)
    }

    pub fn html(&self) -> impl std::fmt::Display + '_ {
        FlashMessageHtml { message: self }
    }
}

impl<'a> From<&'a str> for FlashMessage {
    fn from(value: &'a str) -> Self {
        Self {
            content: value.to_owned(),
            level: FlashLevel::Error,
        }
    }
}

impl From<String> for FlashMessage {
    fn from(content: String) -> Self {
        Self {
            content,
            level: FlashLevel::Error,
        }
    }
}

struct FlashMessageHtml<'a> {
    message: &'a FlashMessage,
}

impl<'a> std::fmt::Display for FlashMessageHtml<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, r#"<div class="alert {}" role="alert">"#, self.message.level)?;
        writeln!(
            f,
            r#"<p>{}</p>"#,
            askama::filters::escape(&self.message.content, askama::filters::Html).unwrap()
        )?;
        writeln!(
            f,
            r#"<button type="button" aria-hidden=true class="close" onclick="closeAlert(event)"></button>"#
        )?;
        writeln!(f, r"</div>")?;
        Ok(())
    }
}

/// The level for the flash message.
#[derive(Debug, PartialEq, Eq, Copy, Clone)]
#[repr(u8)]
pub enum FlashLevel {
    Info,
    Success,
    Warning,
    Error,
}

impl Default for FlashLevel {
    fn default() -> Self {
        Self::Info
    }
}

impl std::fmt::Display for FlashLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Info => write!(f, "info"),
            Self::Success => write!(f, "success"),
            Self::Warning => write!(f, "warning"),
            Self::Error => write!(f, "error"),
        }
    }
}

impl Serialize for FlashLevel {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_u8(*self as u8)
    }
}

impl<'de> Deserialize<'de> for FlashLevel {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let level: u8 = Deserialize::deserialize(deserializer)?;
        match level {
            0 => Ok(FlashLevel::Info),
            1 => Ok(FlashLevel::Success),
            2 => Ok(FlashLevel::Warning),
            3 => Ok(FlashLevel::Error),
            _ => Err(serde::de::Error::custom("invalid flash level")),
        }
    }
}

impl Serialize for Flasher {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let inner = self.data.as_ref();
        inner.serialize(serializer)
    }
}

impl Flasher {
    pub fn new() -> Self {
        Self {
            data: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Adds a flash message.
    pub fn add(&self, message: impl Into<FlashMessage>) -> &Self {
        let mut guard = self.data.lock().unwrap();
        guard.push(message.into());
        self
    }

    /// Creates a redirect response.
    ///
    /// This is mainly for fluent-style chaining.
    pub fn bail(&self, url: impl AsRef<str>) -> Response {
        Redirect::to(url.as_ref()).into_response()
    }

    /// Returns the cookie for these flash messages.
    ///
    /// If no flash messages are found or an error happened, then this returns `None`.
    fn to_cookie(&self, key: &SecretKey) -> Option<Cookie<'static>> {
        let messages = self.data.lock().unwrap();
        if messages.is_empty() {
            None
        } else {
            let cookie = Cookie::build(("flash_messages", key.sign(&messages.as_slice()).ok()?))
                .same_site(cookie::SameSite::Lax)
                .expires(cookie::Expiration::DateTime(
                    time::OffsetDateTime::now_utc() + Duration::from_secs(3600),
                ))
                .path("/")
                .http_only(true)
                .build();
            Some(cookie)
        }
    }
}

impl<S> FromRequestParts<S> for Flasher
where
    S: Send + Sync,
{
    type Rejection = StatusCode;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<Flasher>()
            .ok_or(StatusCode::INTERNAL_SERVER_ERROR)
            .cloned()
    }
}

impl<S> FromRequestParts<S> for Flashes
where
    S: Send + Sync,
{
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(parts.extensions.get::<Flashes>().cloned().unwrap_or_default())
    }
}

/// A middleware that implements the actual flashing messages
pub async fn process_flash_messages(mut request: Request, next: Next) -> Response {
    let key = request
        .extensions()
        .get::<SecretKey>()
        .copied()
        .expect("missing secret key");
    let flashes = request
        .extensions()
        .get::<Vec<Cookie>>()
        .and_then(|cookies| cookies.iter().find(|c| c.name() == "flash_messages"))
        .and_then(|cookie| key.verify(cookie.value()).map(Flashes))
        .unwrap_or_default();

    let had_messages = !flashes.is_empty();
    let flasher = Flasher::new();
    request.extensions_mut().insert(flashes);
    request.extensions_mut().insert(flasher.clone());

    let mut response = next.run(request).await;

    if let Some(cookie) = flasher.to_cookie(&key) {
        response
            .headers_mut()
            .insert(SET_COOKIE, HeaderValue::from_str(&cookie.to_string()).unwrap());
    } else if had_messages {
        response.headers_mut().insert(SET_COOKIE, REMOVE_FLASH_MESSAGES);
    }
    response
}
