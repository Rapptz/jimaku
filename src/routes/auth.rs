use crate::{
    auth::{hash_password, validate_password},
    database::is_unique_constraint_violation,
    error::{ApiError, ApiErrorCode},
    filters,
    flash::{FlashMessage, Flasher, Flashes},
    headers::Referrer,
    models::{is_valid_username, Account, AccountFlags, DirectoryEntry},
    ratelimit::RateLimit,
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
    #[serde(deserialize_with = "crate::utils::empty_string_is_none")]
    session_description: Option<String>,
    action: AuthenticationAction,
}

fn cookie_to_response(cookie: Cookie<'static>) -> Response {
    let mut response = Redirect::to("/").into_response();
    response
        .headers_mut()
        .insert(SET_COOKIE, HeaderValue::from_str(&cookie.to_string()).unwrap());
    response
}

async fn register(state: &AppState, token: &Option<Token>, credentials: Credentials) -> Result<Response, ApiError> {
    if token.is_some() {
        return Err(ApiError::new("user already has an account"));
    }

    if !is_valid_username(&credentials.username) {
        return Err(ApiError::new("invalid username given"));
    }

    if !((8..=128).contains(&credentials.password.len())) {
        return Err(ApiError::new("password length must be 8 to 128 characters"));
    }

    let password_hash = hash_password(&credentials.password)?;
    let result: rusqlite::Result<Option<Account>> = state
        .database()
        .get(
            "INSERT INTO account(name, password) VALUES (?, ?) RETURNING *",
            [credentials.username, password_hash],
        )
        .await;

    match result {
        Ok(Some(account)) => {
            let token = Token::new(account.id)?;
            let cookie = token.to_cookie(&state.config().secret_key);
            state.save_session(&token, credentials.session_description).await;
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

async fn authenticate(state: &AppState, credentials: Credentials) -> Result<Response, ApiError> {
    if !is_valid_username(&credentials.username) {
        return Err(ApiError::new("invalid username given"));
    }

    if !((8..=128).contains(&credentials.password.len())) {
        return Err(ApiError::new("password length must be 8 to 128 characters"));
    }

    let account: Option<Account> = state
        .database()
        .get("SELECT * FROM account WHERE name = ?", [credentials.username])
        .await?;

    // Mitigate timing attacks by always comparing password hashes regardless of whether it's found or not
    let hash = account
        .as_ref()
        .map(|a| &a.password)
        .unwrap_or(&state.incorrect_default_password_hash);

    if validate_password(&credentials.password, hash).is_ok() {
        match account {
            Some(acc) => {
                state.invalidate_account_cache(acc.id);
                let token = Token::new(acc.id)?;
                let cookie = token.to_cookie(&state.config().secret_key);
                state.save_session(&token, credentials.session_description).await;
                Ok(cookie_to_response(cookie))
            }
            None => Err(ApiError::incorrect_login()),
        }
    } else {
        Err(ApiError::incorrect_login())
    }
}

async fn logout(State(state): State<AppState>, token: Token) -> TokenRejection {
    state.invalidate_account_cache(token.id);
    state.invalidate_session(&token.base64()).await;
    TokenRejection
}

#[derive(Deserialize)]
struct ChangePasswordForm {
    old_password: String,
    new_password: String,
    #[serde(deserialize_with = "crate::utils::empty_string_is_none")]
    session_description: Option<String>,
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
            let Ok(token) = Token::new(account.id) else {
                return flasher.add("Failed to obtain new token cookie").bail(&url);
            };
            let cookie = token.to_cookie(&state.config().secret_key);
            state.invalidate_account_sessions(account.id).await;
            state.save_session(&token, form.session_description).await;
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
        AuthenticationAction::Login => authenticate(&state, credentials).await,
        AuthenticationAction::Register => register(&state, &token, credentials).await,
    };
    match result {
        Ok(r) => r,
        Err(e) => flasher.add(e.error.into_owned()).bail("/login"),
    }
}

#[derive(Template)]
#[template(path = "account.html")]
struct AccountInfoTemplate {
    account: Option<Account>,
    user: Account,
    entries: Vec<DirectoryEntry>,
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

async fn show_other_account_info(
    State(state): State<AppState>,
    account: Account,
    Path(name): Path<String>,
) -> Result<AccountInfoTemplate, Redirect> {
    let Some(user) = state
        .database()
        .get::<Account, _, _>("SELECT * FROM account WHERE name = ?", [name])
        .await
        .ok()
        .flatten()
    else {
        return Err(Redirect::to("/"));
    };

    let entries = state
        .database()
        .all("SELECT * FROM directory_entry WHERE creator_id = ?", [user.id])
        .await
        .unwrap_or_default();

    Ok(AccountInfoTemplate {
        account: Some(account),
        user,
        entries,
    })
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

    state.invalidate_account_cache(id);
    Ok(StatusCode::NO_CONTENT)
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/account/authenticate",
            post(login_form).layer(RateLimit::default().quota(10, 60.0).build()),
        )
        .route("/login", get(login))
        .route("/logout", get(logout))
        .route("/account", get(account_info))
        .route("/account/change_password", post(change_password))
        .route("/user/:name", get(show_other_account_info))
        .route("/account/:id/edit", post(edit_account))
}
