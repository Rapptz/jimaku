use std::borrow::Cow;

use askama::Template;
use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};

use crate::models::Account;

#[derive(Template)]
#[template(path = "error.html")]
pub struct ErrorTemplate {
    account: Option<Account>,
    error: anyhow::Error,
}

/// Inteprets an [`anyhow::Error`] as an internal server error.
pub struct InternalError(anyhow::Error);

impl IntoResponse for InternalError {
    fn into_response(self) -> Response {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            ErrorTemplate {
                account: None,
                error: self.0,
            },
        )
            .into_response()
    }
}

impl<E> From<E> for InternalError
where
    E: Into<anyhow::Error>,
{
    fn from(value: E) -> Self {
        Self(value.into())
    }
}

/// An error type that represents its errors as a JSON response
#[derive(Debug, Clone, Eq, PartialEq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ApiError {
    pub error: Cow<'static, str>,
    pub code: ApiErrorCode,
}

/// An error code that the client can use to quickly check error conditions.
#[derive(Debug, Copy, Clone, Eq, PartialEq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum ApiErrorCode {
    ServerError = 0,
    BadRequest = 1,
    UsernameRegistered = 2,
    IncorrectLogin = 3,
    NoPermissions = 4,
    EntryAlreadyExists = 5,
    NotFound = 6,
}

impl ApiErrorCode {
    pub fn from_number(number: u8) -> Option<Self> {
        match number {
            1 => Some(Self::BadRequest),
            2 => Some(Self::UsernameRegistered),
            3 => Some(Self::IncorrectLogin),
            4 => Some(Self::NoPermissions),
            5 => Some(Self::EntryAlreadyExists),
            6 => Some(Self::NotFound),
            _ => None,
        }
    }
}

impl Serialize for ApiErrorCode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_u8(*self as u8)
    }
}

impl<'de> Deserialize<'de> for ApiErrorCode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let num = u8::deserialize(deserializer)?;
        Self::from_number(num).ok_or_else(|| serde::de::Error::custom("unknown error code"))
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status_code(), Json(self)).into_response()
    }
}

impl ApiError {
    /// Creates a new [`ApiError`] with [`ApiErrorCode::BadRequest`] as the code.
    pub fn new<S>(s: S) -> Self
    where
        S: Into<Cow<'static, str>>,
    {
        Self {
            error: s.into(),
            code: ApiErrorCode::BadRequest,
        }
    }

    pub fn with_code(mut self, code: ApiErrorCode) -> Self {
        self.code = code;
        self
    }

    pub fn incorrect_login() -> Self {
        Self {
            error: "incorrect username or password".into(),
            code: ApiErrorCode::IncorrectLogin,
        }
    }

    pub fn forbidden() -> Self {
        Self {
            error: "no permissions to do this action".into(),
            code: ApiErrorCode::NoPermissions,
        }
    }

    pub fn not_found(error: impl Into<Cow<'static, str>>) -> Self {
        Self {
            error: error.into(),
            code: ApiErrorCode::NotFound,
        }
    }

    fn status_code(&self) -> StatusCode {
        if self.code == ApiErrorCode::ServerError {
            StatusCode::INTERNAL_SERVER_ERROR
        } else if self.code == ApiErrorCode::NoPermissions {
            StatusCode::FORBIDDEN
        } else if self.code == ApiErrorCode::NotFound {
            StatusCode::NOT_FOUND
        } else {
            StatusCode::BAD_REQUEST
        }
    }
}

impl<E> From<E> for ApiError
where
    E: Into<anyhow::Error>,
{
    fn from(value: E) -> Self {
        Self {
            error: Cow::Owned(value.into().to_string()),
            code: ApiErrorCode::ServerError,
        }
    }
}
