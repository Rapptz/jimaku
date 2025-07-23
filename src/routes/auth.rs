use crate::{
    auth::{hash_password, validate_password},
    database::is_unique_constraint_violation,
    error::{ApiError, ApiErrorCode},
    filters,
    flash::{FlashMessage, Flasher, Flashes},
    headers::Referrer,
    key::SecretKey,
    logging::BadRequestReason,
    models::{is_valid_username, Account, DirectoryEntry, Session},
    ratelimit::RateLimit,
    token::{Token, TokenRejection},
    utils::Patch,
    AppState,
};
use askama::Template;
use axum::{
    extract::{Path, State},
    http::{header::SET_COOKIE, HeaderValue, StatusCode},
    response::{IntoResponse, Redirect, Response},
    routing::{delete, get, post},
    Form, Json, Router,
};
use cookie::Cookie;
use serde::{Deserialize, Serialize};

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

async fn logout_all(State(state): State<AppState>, account: Account) -> TokenRejection {
    state.invalidate_account_cache(account.id);
    state.invalidate_account_sessions(account.id).await;
    TokenRejection
}

#[derive(Deserialize)]
struct InvalidateSessionPayload {
    session_id: String,
}

async fn invalidate_session(
    State(state): State<AppState>,
    account: Account,
    Json(payload): Json<InvalidateSessionPayload>,
) -> StatusCode {
    let key = state.config().secret_key;
    match Token::from_signed(&payload.session_id, &key) {
        Some(token) => {
            if token.id == account.id {
                state.invalidate_session(&token.base64()).await;
                StatusCode::NO_CONTENT
            } else {
                StatusCode::NOT_FOUND
            }
        }
        None => StatusCode::NOT_FOUND,
    }
}

async fn remove_all_bookmarks(State(state): State<AppState>, account: Account) -> StatusCode {
    if state
        .database()
        .execute("DELETE FROM bookmark WHERE user_id = ?", (account.id,))
        .await
        .is_err()
    {
        StatusCode::BAD_GATEWAY
    } else {
        StatusCode::OK
    }
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
        Err(e) => {
            let mut response = flasher.add(e.error.into_owned()).bail("/login");
            response.extensions_mut().insert(BadRequestReason::IncorrectLogin);
            response
        }
    }
}

#[derive(Template)]
#[template(path = "account.html")]
struct AccountInfoTemplate {
    account: Option<Account>,
    user: Account,
    entries: Vec<DirectoryEntry>,
    bookmarks: Vec<DirectoryEntry>,
    sessions: Vec<Session>,
    current_session: Option<Session>,
    api_key: Option<String>,
    key: SecretKey,
}

impl AccountInfoTemplate {
    async fn new(account: Account, user: Account, current_token: Token, state: &AppState) -> Self {
        let entries = state
            .database()
            .all("SELECT * FROM directory_entry WHERE creator_id = ?", [user.id])
            .await
            .unwrap_or_default();

        let mut sessions = if user.id == account.id {
            state
                .database()
                .all("SELECT * FROM session WHERE account_id = ?", [user.id])
                .await
                .unwrap_or_default()
        } else {
            Vec::<Session>::new()
        };

        let bookmarks = user.get_bookmarks(state.database()).await.unwrap_or_default();
        let session_id = current_token.base64();
        let current_session = sessions
            .iter()
            .position(|s| s.id == session_id)
            .map(|idx| sessions.swap_remove(idx));

        let api_key = sessions
            .iter()
            .position(|s| s.api_key)
            .map(|idx| sessions.swap_remove(idx).id);

        sessions.sort_by_key(|s| std::cmp::Reverse(s.created_at));
        let key = state.config().secret_key;

        Self {
            account: Some(account),
            bookmarks,
            user,
            entries,
            sessions,
            current_session,
            api_key,
            key,
        }
    }
}

async fn account_info(State(state): State<AppState>, token: Token, account: Account) -> AccountInfoTemplate {
    AccountInfoTemplate::new(account.clone(), account, token, &state).await
}

async fn show_other_account_info(
    State(state): State<AppState>,
    token: Token,
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

    Ok(AccountInfoTemplate::new(account, user, token, &state).await)
}

#[derive(Deserialize)]
struct EditAccountPayload {
    #[serde(default)]
    editor: Option<bool>,
    #[serde(default)]
    restricted: Option<bool>,
    #[serde(default)]
    anilist_username: Patch<String>,
}

async fn edit_account(
    State(state): State<AppState>,
    mut account: Account,
    Path(id): Path<i64>,
    Json(payload): Json<EditAccountPayload>,
) -> Result<StatusCode, ApiError> {
    // Admin accounts are the only ones that can toggle the editor flag
    // Admins and regular users can change their anilist_username
    if payload.editor.is_some() && !account.flags.is_admin() {
        return Err(ApiError::forbidden());
    }

    // Admin accounts are the only ones that can toggle the restricted flag
    if payload.restricted.is_some() && !account.flags.is_admin() {
        return Err(ApiError::forbidden());
    }

    if account.id != id && !account.flags.is_admin() {
        return Err(ApiError::forbidden());
    }

    if account.id != id {
        match state.get_account(id).await {
            Some(acc) => account = acc,
            None => return Err(ApiError::not_found("user does not exist")),
        }
    }

    if let Some(toggle) = payload.editor {
        account.flags.set_editor(toggle);
    }

    if let Some(toggle) = payload.restricted {
        account.flags.set_restricted(toggle);
    }

    // At some point it might make sense to verify that this username is valid
    // But there's no current security risk in just letting this be as-is,
    // outside of some extraneous 404s being formed.
    // As far as I know, AniList usernames can only be ASCII alphanumeric
    if let Some(toggle) = payload.anilist_username.to_option() {
        let is_valid = toggle
            .as_ref()
            .map(|s| s.as_bytes().iter().all(u8::is_ascii_alphanumeric))
            .unwrap_or(true);
        if !is_valid {
            return Err(ApiError::new("AniList username is not proper"));
        }
        account.anilist_username = toggle;
    }

    state
        .database()
        .execute(
            "UPDATE account SET flags = ?, anilist_username = ? WHERE id = ?",
            (account.flags, account.anilist_username, id),
        )
        .await?;

    state.invalidate_account_cache(id);
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct GenerateApiKey {
    new: bool,
}

#[derive(Serialize)]
struct GeneratedApiKey {
    token: String,
}

async fn generate_api_key(
    State(state): State<AppState>,
    account: Account,
    Json(payload): Json<GenerateApiKey>,
) -> Result<Json<GeneratedApiKey>, ApiError> {
    if !payload.new {
        state.invalidate_api_keys(account.id).await;
    }
    let token = state.generate_api_key(account.id).await?;
    Ok(Json(GeneratedApiKey { token }))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/account/authenticate",
            post(login_form).layer(RateLimit::default().quota(10, 60.0).build()),
        )
        .route("/login", get(login))
        .route("/logout", get(logout))
        .route("/logout/all", get(logout_all))
        .route("/account/invalidate", post(invalidate_session))
        .route("/account/bookmarks", delete(remove_all_bookmarks))
        .route("/account", get(account_info))
        .route(
            "/account/api_key",
            post(generate_api_key).layer(RateLimit::default().quota(1, 600.0).build()),
        )
        .route("/account/change_password", post(change_password))
        .route("/user/:name", get(show_other_account_info))
        .route("/account/:id/edit", post(edit_account))
}
