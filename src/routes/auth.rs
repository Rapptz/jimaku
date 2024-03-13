use crate::{
    auth::{hash_password, validate_password},
    database::is_unique_constraint_violation,
    error::{ApiError, ApiErrorCode},
    filters,
    flash::{FlashMessage, Flasher, Flashes},
    models::{is_valid_username, Account, AccountFlags, DirectoryEntry},
    ratelimit::RateLimit,
    referrer::Referrer,
    token::{Token, TokenRejection},
    AppState,
};
use askama::Template;
use axum::{
    extract::{Path, State},
    http::{header::SET_COOKIE, HeaderValue, StatusCode},
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
    Form, Json, Router,
};
use cookie::Cookie;
use serde::Deserialize;

#[derive(Template)]
#[template(path = "login.html")]
struct LoginTemplate {
    account: Option<Account>,
    flashes: Flashes,
}

async fn login(account: Option<Account>, flashes: Flashes) -> Response {
    if account.is_some() {
        Redirect::to("/").into_response()
    } else {
        LoginTemplate { account, flashes }.into_response()
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum AuthenticationAction {
    Login,
    Register,
}

impl<'de> Deserialize<'de> for AuthenticationAction {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = std::borrow::Cow::<'_, str>::deserialize(deserializer)?;
        match s.as_ref() {
            "login" => Ok(Self::Login),
            "register" => Ok(Self::Register),
            _ => Err(serde::de::Error::custom("invalid authentication action provided")),
        }
    }
}

#[derive(Debug, Deserialize)]
struct Credentials {
    username: String,
    password: String,
    action: AuthenticationAction,
}

fn cookie_to_response(cookie: Cookie<'static>) -> Response {
    let mut response = Redirect::to("/").into_response();
    response
        .headers_mut()
        .insert(SET_COOKIE, HeaderValue::from_str(&cookie.to_string()).unwrap());
    response
}

async fn register(
    state: &AppState,
    token: &Option<Token>,
    username: String,
    password: String,
) -> Result<Response, ApiError> {
    if token.is_some() {
        return Err(ApiError::new("user already has an account"));
    }

    if !is_valid_username(&username) {
        return Err(ApiError::new("invalid username given"));
    }

    if !((8..=128).contains(&password.len())) {
        return Err(ApiError::new("password length must be 8 to 128 characters"));
    }

    let password_hash = hash_password(&password)?;
    let result: rusqlite::Result<Option<Account>> = state
        .database()
        .get(
            "INSERT INTO account(name, password) VALUES (?, ?) RETURNING *",
            [username, password_hash],
        )
        .await;

    match result {
        Ok(Some(account)) => {
            let token = Token::new(account.id);
            let cookie = token.to_cookie(&state.config().secret_key)?;
            Ok(cookie_to_response(cookie))
        }
        Ok(None) => Err(ApiError {
            error: "account registration returned no rows".into(),
            code: ApiErrorCode::ServerError,
        }),
        Err(e) => {
            if is_unique_constraint_violation(&e) {
                Err(ApiError::new("username already taken").with_code(ApiErrorCode::UsernameRegistered))
            } else {
                Err(e.into())
            }
        }
    }
}

async fn authenticate(state: &AppState, username: String, password: String) -> Result<Response, ApiError> {
    if !is_valid_username(&username) {
        return Err(ApiError::new("invalid username given"));
    }

    if !((8..=128).contains(&password.len())) {
        return Err(ApiError::new("password length must be 8 to 128 characters"));
    }

    let account: Option<Account> = state
        .database()
        .get("SELECT * FROM account WHERE name = ?", [username])
        .await?;

    // Mitigate timing attacks by always comparing password hashes regardless of whether it's found or not
    let hash = account
        .as_ref()
        .map(|a| &a.password)
        .unwrap_or(&state.incorrect_default_password_hash);

    if validate_password(&password, hash).is_ok() {
        match account {
            Some(acc) => {
                let token = Token::new(acc.id);
                let cookie = token.to_cookie(&state.config().secret_key)?;
                Ok(cookie_to_response(cookie))
            }
            None => Err(ApiError::incorrect_login()),
        }
    } else {
        Err(ApiError::incorrect_login())
    }
}

#[derive(Deserialize)]
struct ChangePasswordForm {
    old_password: String,
    new_password: String,
}

async fn change_password(
    State(state): State<AppState>,
    Referrer(url): Referrer,
    token: Token,
    flasher: Flasher,
    Form(form): Form<ChangePasswordForm>,
) -> Response {
    if !((8..=128).contains(&form.new_password.len())) {
        return flasher.add("Password length must be 8 to 128 characters").bail(&url);
    }

    let result = state
        .database()
        .get::<Account, _, _>("SELECT * FROM account WHERE id = ?", [token.id])
        .await;

    let account = match result {
        Ok(Some(account)) => account,
        Ok(None) => {
            flasher.add("Somehow, this account does not exist.");
            return TokenRejection.into_response();
        }
        Err(e) => {
            return flasher.add(format!("SQL error: {e}")).bail(&url);
        }
    };

    if validate_password(&form.old_password, &account.password).is_err() {
        return flasher.add("Invalid password").bail(&url);
    }

    let Ok(changed_hash) = hash_password(&form.new_password) else {
        return flasher
            .add("Failed to hash password somehow. Try again later?")
            .bail(&url);
    };

    match state
        .database()
        .execute(
            "UPDATE account SET password = ? WHERE id = ?",
            (changed_hash, account.id),
        )
        .await
    {
        Ok(_) => {
            let token = Token::new(account.id);
            let Ok(cookie) = token.to_cookie(&state.config().secret_key) else {
                return flasher.add("Failed to obtain new token cookie").bail(&url);
            };
            flasher.add(FlashMessage::success("Successfully changed password."));
            cookie_to_response(cookie)
        }
        Err(e) => flasher.add(format!("SQL error: {e}")).bail(&url),
    }
}

async fn login_form(
    State(state): State<AppState>,
    token: Option<Token>,
    flasher: Flasher,
    Form(credentials): Form<Credentials>,
) -> Response {
    let result = match credentials.action {
        AuthenticationAction::Login => authenticate(&state, credentials.username, credentials.password).await,
        AuthenticationAction::Register => register(&state, &token, credentials.username, credentials.password).await,
    };
    match result {
        Ok(r) => r,
        Err(e) => flasher.add(e.error.into_owned()).bail("/login"),
    }
}

#[derive(Template)]
#[template(path = "account.html")]
pub struct AccountInfoTemplate {
    pub account: Option<Account>,
    pub user: Account,
    pub entries: Vec<DirectoryEntry>,
}

async fn account_info(State(state): State<AppState>, account: Account) -> AccountInfoTemplate {
    let entries = state
        .database()
        .all("SELECT * FROM directory_entry WHERE creator_id = ?", [account.id])
        .await
        .unwrap_or_default();

    AccountInfoTemplate {
        account: Some(account.clone()),
        user: account,
        entries,
    }
}

#[derive(Deserialize)]
struct EditAccountPayload {
    editor: bool,
}

async fn edit_account(
    State(state): State<AppState>,
    account: Account,
    Path(id): Path<i64>,
    Json(payload): Json<EditAccountPayload>,
) -> Result<StatusCode, ApiError> {
    if !account.flags.is_admin() {
        return Err(ApiError::forbidden());
    }

    let mut flags = AccountFlags::new();
    flags.set_editor(payload.editor);
    state
        .database()
        .execute("UPDATE account SET flags = ? WHERE id = ?", (flags, id))
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/account/authenticate",
            post(login_form).layer(RateLimit::default().quota(10, 60.0).build()),
        )
        .route("/login", get(login))
        .route("/logout", get(TokenRejection))
        .route("/account", get(account_info))
        .route("/account/change_password", post(change_password))
        .route("/account/:id/edit", post(edit_account))
}
