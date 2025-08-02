use std::{collections::HashSet, time::Duration};

use crossbeam_channel::{Receiver, Sender};
use serde::{Deserialize, Serialize};

use crate::{
    database::Table,
    utils::{sql_json_bridge, unix_duration, unix_now_ms},
};

/// A notification when a new subtitle has been uploaded
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewSubtitleUploaded {
    pub files: Vec<String>,
}

/// A notification when a new report has been sent in
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewReport {
    pub report_id: i64,
}

/// A notification sent to a user when their report has been responded to
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportAnswered {
    pub report_id: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
pub enum NotificationData {
    NewSubtitle(NewSubtitleUploaded),
    NewReport(NewReport),
    ReportAnswered(ReportAnswered),
}

impl NotificationData {
    /// Converts it into a JSON string but coerces the error into a rusqlite::Result
    fn to_json(&self) -> rusqlite::Result<String> {
        match serde_json::to_string(&self) {
            Ok(payload) => Ok(payload),
            Err(e) => Err(rusqlite::Error::ToSqlConversionFailure(Box::new(e))),
        }
    }
}

impl From<NewSubtitleUploaded> for NotificationData {
    fn from(v: NewSubtitleUploaded) -> Self {
        Self::NewSubtitle(v)
    }
}

sql_json_bridge!(NotificationData);

/// The message that is processed by the notification service
enum NotificationMessage {
    /// Notify that a new subtitle has been uploaded
    NewSubtitleUploaded {
        entry_id: i64,
        files: Vec<String>,
    },
    NewReport {
        report_id: i64,
    },
    ReportAnswered {
        account_id: i64,
        report_id: i64,
    },
    /// Notifies that old notifications should be cleaned
    Clean,
}

/// Responsible for sending notifications
#[derive(Debug, Clone)]
pub struct NotificationService {
    sender: Sender<NotificationMessage>,
}

impl NotificationService {
    pub fn new() -> anyhow::Result<Self> {
        let (sender, receiver) = crossbeam_channel::unbounded();
        let connection = rusqlite::Connection::open(crate::database::directory()?)?;
        rusqlite::vtab::array::load_module(&connection)?;
        std::thread::spawn(|| {
            let mut worker = NotificationWorker::new(receiver, connection);
            worker.run();
        });
        Ok(Self { sender })
    }

    pub fn notify_new_subtitles(&self, entry_id: i64, files: Vec<String>) {
        let _ = self
            .sender
            .send(NotificationMessage::NewSubtitleUploaded { entry_id, files });
    }

    pub fn cleanup(&self) {
        let _ = self.sender.send(NotificationMessage::Clean);
    }

    pub fn notify_new_report(&self, report_id: i64) {
        let _ = self.sender.send(NotificationMessage::NewReport { report_id });
    }

    pub fn notify_answered_report(&self, account_id: i64, report_id: i64) {
        let _ = self
            .sender
            .send(NotificationMessage::ReportAnswered { account_id, report_id });
    }
}

/// The actual worker for the notification service
struct NotificationWorker {
    receiver: Receiver<NotificationMessage>,
    connection: rusqlite::Connection,
}

impl NotificationWorker {
    fn new(receiver: Receiver<NotificationMessage>, connection: rusqlite::Connection) -> Self {
        Self { receiver, connection }
    }

    fn process_new_subtitle_uploads(&mut self, entry_id: i64, files: Vec<String>) -> rusqlite::Result<()> {
        let mut new_notified_users: HashSet<i64> = {
            let mut stmt = self
                .connection
                .prepare_cached("SELECT user_id FROM bookmark WHERE entry_id = ?")?;

            let iter = stmt.query_map((entry_id,), |row| row.get("user_id"))?;
            iter.collect::<rusqlite::Result<_>>()?
        };

        if new_notified_users.is_empty() {
            return Ok(());
        }

        let mut collapsed_notifications = Vec::new();

        {
            // This query gets all the users who have an unread notification regarding this query
            // The entry_id column is only used for this type of notification currently, and hopefully forever
            let mut stmt = self.connection.prepare_cached(
                r#"
                SELECT account.id, notification.id
                FROM account
                INNER JOIN notification ON notification.user_id = account.id
                WHERE notification.entry_id = ?
                AND notification.ts > COALESCE(account.notification_ack, 0)
            "#,
            )?;

            let mut rows = stmt.query((entry_id,))?;
            while let Some(row) = rows.next()? {
                let user_id = row.get(0)?;
                let notification_id: rusqlite::types::Value = row.get(1)?;
                new_notified_users.remove(&user_id);
                collapsed_notifications.push(notification_id);
            }
        }

        let tx = self.connection.transaction()?;
        let data = NotificationData::NewSubtitle(NewSubtitleUploaded { files });

        let payload = data.to_json()?;
        // Send a new broadcasted notification to everyone who is new
        let ts = unix_now_ms();
        {
            let query = r"INSERT INTO notification(ts, entry_id, user_id, payload) VALUES (?, ?, ?, ?)";
            let mut stmt = tx.prepare_cached(query)?;

            for user in new_notified_users {
                stmt.execute((ts, entry_id, user, payload.as_str()))?;
            }
        }

        // Collapse unread notifications with updated data for users who have one
        if !collapsed_notifications.is_empty() {
            let rc = std::rc::Rc::new(collapsed_notifications);
            let query = r###"
                UPDATE notification SET
                ts = ?1,
                payload = json_set(payload, '$.files', (
                    SELECT json_group_array(value)
                    FROM (
                        SELECT value FROM json_each(json_extract(payload, '$.files'))
                        UNION
                        SELECT value FROM json_each(json_extract(?2, '$.files'))
                    )
                ))
                WHERE id IN rarray(?3);
            "###;

            let mut stmt = tx.prepare_cached(query)?;
            stmt.execute((ts, payload.as_str(), rc.clone()))?;
        }

        tx.commit()?;
        Ok(())
    }

    fn process_new_report(&self, report_id: i64) -> rusqlite::Result<()> {
        let ts = unix_now_ms();
        let data = NotificationData::NewReport(NewReport { report_id });
        let payload = data.to_json()?;
        let query = r###"
            INSERT INTO notification(ts, user_id, payload)
            SELECT ?, account.id, ? FROM account WHERE (flags & 3) != 0;
        "###;

        let mut stmt = self.connection.prepare_cached(query)?;
        stmt.execute((ts, payload.as_str()))?;
        Ok(())
    }

    fn send_simple_notification(&self, account_id: i64, data: NotificationData) -> rusqlite::Result<()> {
        let ts = unix_now_ms();
        let payload = data.to_json()?;
        let query = "INSERT INTO notification(ts, user_id, payload) VALUES (?, ?, ?)";
        let mut stmt = self.connection.prepare_cached(query)?;
        stmt.execute((ts, account_id, payload.as_str()))?;
        Ok(())
    }

    fn process_answered_report(&self, account_id: i64, report_id: i64) -> rusqlite::Result<()> {
        let data = NotificationData::ReportAnswered(ReportAnswered { report_id });
        self.send_simple_notification(account_id, data)
    }

    fn cleanup(&mut self) -> rusqlite::Result<()> {
        let now = unix_duration();
        // 120 days ago
        let threshold = now.saturating_sub(Duration::from_secs(120 * 86400)).as_millis() as i64;
        let query = "DELETE FROM notification WHERE ts <= ?";
        self.connection.execute(query, (threshold,))?;
        Ok(())
    }

    fn run(&mut self) {
        while let Ok(msg) = self.receiver.recv() {
            match msg {
                NotificationMessage::NewSubtitleUploaded { entry_id, files } => {
                    if let Err(e) = self.process_new_subtitle_uploads(entry_id, files) {
                        tracing::error!(error = %e, "error when processing new subtitle notifications");
                    }
                }
                NotificationMessage::Clean => {
                    if let Err(e) = self.cleanup() {
                        tracing::error!(error = %e, "error when cleaning notifications");
                    }
                }
                NotificationMessage::NewReport { report_id } => {
                    if let Err(e) = self.process_new_report(report_id) {
                        tracing::error!(error = %e, "error when processing new report notifications");
                    }
                }
                NotificationMessage::ReportAnswered { account_id, report_id } => {
                    if let Err(e) = self.process_answered_report(account_id, report_id) {
                        tracing::error!(error = %e, "error when processing new report notifications");
                    }
                }
            }
        }
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize)]
pub struct Notification {
    pub id: i64,
    pub ts: i64,
    pub entry_id: Option<i64>,
    pub user_id: i64,
    pub payload: NotificationData,
}

impl Table for Notification {
    const NAME: &'static str = "notification";

    const COLUMNS: &'static [&'static str] = &["id", "ts", "entry_id", "user_id", "payload"];

    type Id = i64;

    fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get("id")?,
            ts: row.get("ts")?,
            entry_id: row.get("entry_id")?,
            user_id: row.get("user_id")?,
            payload: row.get("payload")?,
        })
    }
}
