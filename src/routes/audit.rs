use std::collections::HashMap;

use askama::Template;
use axum::{
    extract::{Query, State},
    response::Redirect,
    routing::get,
    Json, Router,
};
use rusqlite::types::Value;
use serde::{Deserialize, Serialize};

use crate::{
    audit::{AuditLogData, AuditLogEntry},
    database::Table,
    error::ApiError,
    models::{Account, DirectoryEntry},
    utils::HtmlPage,
    AppState,
};

#[derive(Debug, Serialize)]
struct EntryTitles {
    name: String,
    japanese_name: Option<String>,
    english_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AuditLogQuery {
    #[serde(default)]
    entry_id: Option<i64>,
    #[serde(default)]
    account_id: Option<i64>,
    #[serde(default)]
    before: Option<i64>,
    #[serde(default)]
    after: Option<i64>,
    #[serde(default, rename = "type")]
    kind: Option<String>,
}

impl AuditLogQuery {
    fn into_sql_parts(self) -> (String, Vec<Value>) {
        let mut filters = Vec::new();
        let mut params = Vec::new();
        if let Some(entry_id) = self.entry_id {
            filters.push("audit_log.entry_id = ?");
            params.push(Value::Integer(entry_id));
        }
        if let Some(account_id) = self.account_id {
            filters.push("audit_log.account_id = ?");
            params.push(Value::Integer(account_id));
        }
        if let Some(before) = self.before {
            filters.push("audit_log.id < ?");
            params.push(Value::Integer(before));
        }
        if let Some(after) = self.after {
            filters.push("audit_log.id > ?");
            params.push(Value::Integer(after));
        }

        if let Some(kind) = self.kind {
            filters.push("json_extract(audit_log.data, '$.type') = ?");
            params.push(Value::Text(kind));
        }

        if filters.is_empty() {
            (String::new(), params)
        } else {
            (filters.join(" AND "), params)
        }
    }
}

#[derive(Debug, Default, Serialize)]
struct AuditLogResult {
    logs: Vec<AuditLogEntry>,
    entries: HashMap<i64, EntryTitles>,
    users: HashMap<i64, String>,
}

async fn get_audit_logs(
    State(state): State<AppState>,
    Query(query): Query<AuditLogQuery>,
    account: Account,
) -> Result<Json<AuditLogResult>, ApiError> {
    if !account.flags.is_editor() {
        return Err(ApiError::forbidden());
    }

    let (filter, params) = query.into_sql_parts();
    let mut query = r###"
        SELECT audit_log.*,
               directory_entry.name AS "name",
               directory_entry.japanese_name AS "japanese_name",
               directory_entry.english_name AS "english_name",
               account.name AS "account_name"
        FROM audit_log
        LEFT JOIN directory_entry ON directory_entry.id = audit_log.entry_id
        LEFT JOIN account ON account.id = audit_log.account_id
    "###
    .to_owned();

    if !filter.is_empty() {
        query.push_str("WHERE ");
        query.push_str(&filter);
    }
    query.push_str("ORDER BY audit_log.id DESC LIMIT 100");

    let mut result = state
        .database()
        .call(move |connection| -> rusqlite::Result<_> {
            let mut result = AuditLogResult::default();
            let mut stmt = connection.prepare_cached(query.as_str())?;
            let mut rows = stmt.query(rusqlite::params_from_iter(params))?;
            while let Some(row) = rows.next()? {
                let log = AuditLogEntry::from_row(row)?;
                if let Some(entry_id) = log.entry_id {
                    let title = EntryTitles {
                        name: row.get("name")?,
                        english_name: row.get("english_name")?,
                        japanese_name: row.get("japanese_name")?,
                    };
                    result.entries.insert(entry_id, title);
                }
                if let Some(account_id) = log.account_id {
                    result.users.insert(account_id, row.get("account_name")?);
                }
                result.logs.push(log);
            }
            Ok(result)
        })
        .await?;

    // Check what requires backfilling
    let mut backfilled_ids = Vec::new();
    for entry in result.logs.iter() {
        if let AuditLogData::MoveEntry(s) = &entry.data {
            if !result.entries.contains_key(&s.entry_id) {
                backfilled_ids.push(s.entry_id);
            }
        }
    }

    if !backfilled_ids.is_empty() {
        let mut query = "SELECT * FROM directory_entry WHERE id IN (".to_string();
        for _ in &backfilled_ids {
            query.push('?');
            query.push(',');
        }
        if query.ends_with(',') {
            query.pop();
        }
        query.push(')');
        let entries: Vec<DirectoryEntry> = state
            .database()
            .all(query, rusqlite::params_from_iter(backfilled_ids))
            .await
            .unwrap_or_default();

        for entry in entries {
            result.entries.insert(
                entry.id,
                EntryTitles {
                    name: entry.name,
                    english_name: entry.english_name,
                    japanese_name: entry.japanese_name,
                },
            );
        }
    }

    // These are backfilled user IDs
    let mut backfilled_ids = Vec::new();
    for entry in result.logs.iter() {
        if let AuditLogData::ResolveReport(s) = &entry.data {
            if let Some(reporter_id) = s.reporter_id {
                if !result.users.contains_key(&reporter_id) {
                    backfilled_ids.push(Value::Integer(reporter_id));
                }
            }
        }
    }

    if !backfilled_ids.is_empty() {
        let query = "SELECT id, name FROM account WHERE id IN rarray(?)";
        let users = state
            .database()
            .call(move |conn| -> rusqlite::Result<_> {
                let mut stmt = conn.prepare_cached(query)?;
                let mut rows = stmt.query((std::rc::Rc::new(backfilled_ids),))?;
                let mut result = Vec::<(i64, String)>::new();
                while let Some(row) = rows.next()? {
                    result.push((row.get("id")?, row.get("name")?));
                }
                Ok(result)
            })
            .await
            .unwrap_or_default();
        for (id, name) in users {
            result.users.insert(id, name);
        }
    }

    Ok(Json(result))
}

#[derive(Template)]
#[template(path = "audit.html")]
struct AuditLogTemplate {
    account: Option<Account>,
}

async fn logs(account: Account) -> Result<HtmlPage<AuditLogTemplate>, Redirect> {
    if !account.flags.is_editor() {
        Err(Redirect::to("/"))
    } else {
        Ok(HtmlPage(AuditLogTemplate { account: Some(account) }))
    }
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/audit-logs", get(get_audit_logs))
        .route("/logs", get(logs))
}
