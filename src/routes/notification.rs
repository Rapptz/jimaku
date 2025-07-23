use askama::Template;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};

use crate::{error::ApiError, models::Account, notification::NotificationData, utils::HtmlPage, AppState};

#[derive(Serialize)]
struct NotificationCount {
    count: u64,
}

async fn notification_count(State(state): State<AppState>, account: Account) -> Json<NotificationCount> {
    let count = state.get_notification_count(&account).await;
    Json(NotificationCount { count })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct NotificationQuery {
    #[serde(default)]
    before: Option<i64>,
    #[serde(default)]
    after: Option<i64>,
}

#[derive(Debug, Serialize)]
struct NotificationEntry {
    id: i64,
    name: String,
    japanese_name: Option<String>,
    english_name: Option<String>,
}

#[derive(Debug, Serialize)]
struct Notification {
    timestamp: i64,
    payload: NotificationData,
    entry: Option<NotificationEntry>,
}

#[derive(Debug, Serialize)]
struct NotificationResult {
    last_ack: i64,
    notifications: Vec<Notification>,
}

async fn get_notifications(
    State(state): State<AppState>,
    Query(query): Query<NotificationQuery>,
    account: Account,
) -> Result<Json<NotificationResult>, ApiError> {
    let mut sql = r###"
        SELECT notification.ts AS "timestamp",
               notification.payload AS "payload",
               notification.entry_id AS "entry_id",
               directory_entry.name AS "name",
               directory_entry.japanese_name AS "japanese_name",
               directory_entry.english_name AS "english_name"
        FROM notification
        LEFT JOIN directory_entry ON directory_entry.id = notification.entry_id
        WHERE notification.user_id = ?
    "###
    .to_owned();

    let mut params = vec![account.id];
    if let Some(before) = query.before {
        sql.push_str(" AND notification.ts < ?");
        params.push(before);
    }
    if let Some(after) = query.after {
        sql.push_str(" AND notification.ts > ?");
        params.push(after);
    }

    sql.push_str("ORDER BY notification.ts DESC LIMIT 100;");

    let last_ack = account.notification_ack.unwrap_or_default();
    let result = state
        .database()
        .call(move |conn| -> rusqlite::Result<_> {
            let mut result = NotificationResult {
                last_ack,
                notifications: Vec::default(),
            };
            let mut stmt = conn.prepare_cached(sql.as_str())?;
            let mut rows = stmt.query(rusqlite::params_from_iter(params))?;
            while let Some(row) = rows.next()? {
                let entry = match row.get("name")? {
                    Some(name) => Some(NotificationEntry {
                        id: row.get("entry_id")?,
                        name,
                        japanese_name: row.get("japanese_name")?,
                        english_name: row.get("english_name")?,
                    }),
                    None => None,
                };
                result.notifications.push(Notification {
                    timestamp: row.get("timestamp")?,
                    payload: row.get("payload")?,
                    entry,
                });
            }
            Ok(result)
        })
        .await?;
    Ok(Json(result))
}

async fn ack_notifications(State(state): State<AppState>, account: Account) -> StatusCode {
    state.update_notification_ack(&account).await;
    StatusCode::OK
}

#[derive(Template)]
#[template(path = "notification.html")]
struct NotificationTemplate {
    account: Option<Account>,
}

async fn notifications(account: Account) -> HtmlPage<NotificationTemplate> {
    HtmlPage(NotificationTemplate { account: Some(account) })
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/notifications/count", get(notification_count))
        .route("/notifications", get(notifications))
        .route("/notifications/query", get(get_notifications))
        .route("/notifications/ack", post(ack_notifications))
}
