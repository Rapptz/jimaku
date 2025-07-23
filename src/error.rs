use std::borrow::Cow;

use askama::Template;
use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{logging::BadRequestReason, models::Account, utils::HtmlPage};

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
            HtmlPage(ErrorTemplate {
                account: None,
                error: self.0,
            }),
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
#[derive(Debug, Clone, Eq, PartialEq, PartialOrd, Ord, Serialize, Deserialize, ToSchema)]
pub struct ApiError {
    /// The error message for this error.
    pub error: Cow<'static, str>,
    /// The associated error code.
    #[schema(value_type = u8)]
    pub code: ApiErrorCode,
}

/// An error code that the client can use to quickly check error conditions.
#[derive(Debug, Copy, Clone, Eq, PartialEq, PartialOrd, Ord, Hash, ToSchema)]
#[repr(u8)]
pub enum ApiErrorCode {
    /// An internal server error happened. This should be rather rare.
    ServerError = 0,
    /// The client provided request was invalid in some way or another.
    BadRequest = 1,
    /// An internal error code that represents that the account is already registered.
    ///
    /// Do not rely on this or use it.
    #[schema(deprecated)]
    UsernameRegistered = 2,
    /// An internal error code that represents that the account has provided valid credentials.
    ///
    /// Do not rely on this or use it.
    #[schema(deprecated)]
    IncorrectLogin = 3,
    /// The client does not have permission to execute this action.
    NoPermissions = 4,
    /// The entry already exists.
    EntryAlreadyExists = 5,
    /// The entity being searched for does not exist.
    NotFound = 6,
    /// The client is not authorized, a proper authorization header must be provided.
    Unauthorized = 7,
    /// The client is being rate limited.
    RateLimited = 8,
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
            7 => Some(Self::Unauthorized),
            8 => Some(Self::RateLimited),
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
        let incorrect_login = self.code == ApiErrorCode::IncorrectLogin;
        let mut response = (self.status_code(), Json(self)).into_response();
        if incorrect_login {
            response.extensions_mut().insert(BadRequestReason::IncorrectLogin);
        }
        response
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

    pub fn unauthorized() -> Self {
        Self {
            error: "unauthorized".into(),
            code: ApiErrorCode::Unauthorized,
        }
    }

    pub fn rate_limited() -> Self {
        Self {
            error: "rate limit exceeded".into(),
            code: ApiErrorCode::RateLimited,
        }
    }

    fn status_code(&self) -> StatusCode {
        if self.code == ApiErrorCode::ServerError {
            StatusCode::INTERNAL_SERVER_ERROR
        } else if self.code == ApiErrorCode::NoPermissions {
            StatusCode::FORBIDDEN
        } else if self.code == ApiErrorCode::NotFound {
            StatusCode::NOT_FOUND
        } else if self.code == ApiErrorCode::Unauthorized {
            StatusCode::UNAUTHORIZED
        } else if self.code == ApiErrorCode::RateLimited {
            StatusCode::TOO_MANY_REQUESTS
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
