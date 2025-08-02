use askama::Template;
use axum::{
    extract::{Path, Query, State},
    response::Redirect,
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};

use crate::{
    audit,
    database::Table,
    error::ApiError,
    models::{Account, Report, ReportStatus},
    routes::api::utils::ApiJson as Json,
    utils::HtmlPage,
    AppState,
};

#[derive(Debug, Deserialize)]
struct ReportQuery {
    #[serde(default)]
    entry_id: Option<i64>,
    #[serde(default)]
    account_id: Option<i64>,
    #[serde(default)]
    before: Option<i64>,
    #[serde(default)]
    id: Option<i64>,
}

impl ReportQuery {
    fn to_sql(&self) -> (String, Vec<i64>) {
        let mut filters = Vec::new();
        let mut params = Vec::new();

        if let Some(entry_id) = self.entry_id {
            filters.push("report.entry_id = ?");
            params.push(entry_id);
        }

        if let Some(account_id) = self.account_id {
            filters.push("report.account_id = ?");
            params.push(account_id);
        }

        if let Some(before) = self.before {
            filters.push("report.id < ?");
            params.push(before);
        }

        if let Some(id) = self.id {
            filters.push("report.id = ?");
            params.push(id);
        }

        (filters.join(" AND "), params)
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct EntryTitles {
    pub(crate) name: String,
    pub(crate) japanese_name: Option<String>,
    pub(crate) english_name: Option<String>,
}

/// Basically a Report instance with account and entry information
#[derive(Debug, Serialize)]
pub(crate) struct RichReport {
    pub(crate) report: Report,
    pub(crate) entry: Option<EntryTitles>,
    pub(crate) account_name: String,
}

impl RichReport {
    /// Returns the query without the filter
    pub(crate) fn starting_query() -> String {
        r###"
        SELECT report.*,
               directory_entry.name AS "name",
               directory_entry.japanese_name AS "japanese_name",
               directory_entry.english_name AS "english_name",
               account.name AS "account_name"
        FROM report
        LEFT JOIN directory_entry ON directory_entry.id = report.entry_id
        LEFT JOIN account ON account.id = report.account_id
        "###
        .to_owned()
    }

    pub(crate) fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        let report = Report::from_row(row)?;
        let entry = if report.entry_id.is_some() {
            Some(EntryTitles {
                name: row.get("name")?,
                japanese_name: row.get("japanese_name")?,
                english_name: row.get("english_name")?,
            })
        } else {
            None
        };

        Ok(RichReport {
            report,
            entry,
            account_name: row.get("account_name")?,
        })
    }
}

async fn query_reports(
    State(state): State<AppState>,
    Query(query): Query<ReportQuery>,
    account: Account,
) -> Result<Json<Vec<RichReport>>, ApiError> {
    let is_checking_self = query.account_id.map(|s| s == account.id).unwrap_or_default();
    if !(account.flags.is_editor() || is_checking_self) {
        return Err(ApiError::forbidden());
    }

    let (filter, params) = query.to_sql();
    let mut query = RichReport::starting_query();

    if !filter.is_empty() {
        query.push_str("WHERE ");
        query.push_str(&filter);
    }

    query.push_str("ORDER BY report.id DESC LIMIT 100");

    let result = state
        .database()
        .call(move |connection| -> rusqlite::Result<_> {
            let mut result = Vec::new();
            let mut stmt = connection.prepare_cached(query.as_str())?;
            let mut rows = stmt.query(rusqlite::params_from_iter(params))?;
            while let Some(row) = rows.next()? {
                result.push(RichReport::from_row(row)?);
            }

            Ok(result)
        })
        .await?;

    Ok(Json(result))
}

#[derive(Debug, Deserialize)]
struct UpdateReportPayload {
    status: ReportStatus,
    response: String,
}

async fn update_report(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    account: Account,
    Json(payload): Json<UpdateReportPayload>,
) -> Result<Json<Report>, ApiError> {
    if !account.flags.is_editor() {
        return Err(ApiError::forbidden());
    }

    let query = "UPDATE report SET status = ?, response = ? WHERE id = ? RETURNING *";
    let result: Option<Report> = state
        .database()
        .get(query, (payload.status, payload.response.clone(), id))
        .await?;

    match result {
        Some(report) => {
            if report.status != ReportStatus::Pending {
                if let Some(account_id) = report.account_id {
                    state.notifications.notify_answered_report(account_id, report.id);
                }
            }
            let mut audit = audit::AuditLogEntry::new(audit::ResolveReport {
                report_id: report.id,
                status: report.status,
                name: report.payload.name.clone(),
                reporter_id: report.account_id,
                response: payload.response,
            });
            audit.entry_id = report.entry_id;
            audit.account_id = Some(account.id);
            state.audit(audit).await;

            Ok(Json(report))
        }
        None => Err(ApiError::not_found("report not found")),
    }
}

#[derive(Template)]
#[template(path = "reports.html")]
struct ReportsTemplate {
    account: Option<Account>,
}

async fn reports(account: Account) -> Result<HtmlPage<ReportsTemplate>, Redirect> {
    if !account.flags.is_editor() {
        Err(Redirect::to("/"))
    } else {
        Ok(HtmlPage(ReportsTemplate { account: Some(account) }))
    }
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/reports", get(reports))
        .route("/reports/query", get(query_reports))
        .route("/report/{id}", post(update_report))
}
