//! Implements an audit log trail for editor actions

use rusqlite::{types::FromSql, ToSql};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::{database::Table, models::EntryFlags, tmdb};

/*
    It's important to note that the data in here should be backwards compatible.
*/

/// Audit log data for a created directory entry
///
/// For this data, `entry_id` and `account_id` are only null if the data is deleted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateEntry {
    /// Whether the entry created was an anime one
    pub anime: bool,
    /// Whether the entry was created using the API
    pub api: bool,
    /// The name of the entry, typically Romaji
    pub name: String,
    /// The TMDB ID of the entry.
    pub tmdb_id: Option<tmdb::Id>,
    /// Th AniList ID of the entry.
    pub anilist_id: Option<u32>,
}

/// A directory that was scraped
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScrapeDirectory {
    /// The original name
    pub original_name: String,
    /// The romaji name of the entry that was mapped
    pub name: String,
    /// The anilist ID of the entry
    pub anilist_id: Option<u32>,
}

/// Audit log data for a successful scrape attempt
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScrapeResult {
    /// The directories that were scraped
    pub directories: Vec<ScrapeDirectory>,
    /// Whether the scrape errored out
    pub error: bool,
    /// The date that has been scraped up to.
    #[serde(with = "time::serde::rfc3339::option")]
    pub date: Option<OffsetDateTime>,
}

impl ScrapeResult {
    /// A shortcut constructor to signify that the scrape failed
    pub fn errored() -> Self {
        Self {
            directories: Vec::new(),
            error: true,
            date: None,
        }
    }
}

/// Inner data for a file operation
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileOperation {
    /// The name of the file.
    pub name: String,
    /// Whether the file failed to be moved over.
    pub failed: bool,
}

/// Audit log data for a file move operation
///
/// For this data, `entry_id` and `account_id` are only null if the data is deleted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MoveEntry {
    /// Whether the entry that was moved is an anime one.
    #[serde(default = "crate::utils::default_true")]
    pub anime: bool,
    /// The name of the moved to entry, if any, typically Romaji.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The TMDB ID of the moved to entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tmdb_id: Option<tmdb::Id>,
    /// Th AniList ID of the moved to entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anilist_id: Option<u32>,
    /// The moved to entry ID
    pub entry_id: i64,
    /// The files requested to be moved
    pub files: Vec<FileOperation>,
    /// Whether a new entry was created
    pub created: bool,
}

impl MoveEntry {
    pub fn new(entry_id: i64) -> Self {
        Self {
            anime: true,
            name: None,
            tmdb_id: None,
            anilist_id: None,
            entry_id,
            created: false,
            files: Vec::new(),
        }
    }

    pub fn add_file(&mut self, name: String, failed: bool) {
        self.files.push(FileOperation { name, failed });
    }
}

/// Inner data for a rename file operation
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenamedFile {
    /// The original name
    pub from: String,
    /// The new name
    pub to: String,
    /// Whether the rename failed
    pub failed: bool,
}

/// Audit log data for a rename operation
///
/// For this data, `entry_id` and `account_id` are only null if the data is deleted.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenameFiles {
    pub files: Vec<RenamedFile>,
}

impl RenameFiles {
    pub fn add_file(&mut self, from: String, to: String, failed: bool) {
        self.files.push(RenamedFile { from, to, failed });
    }
}

/// Audit log data for a file upload operation
///
/// For this data, `entry_id` and `account_id` are only null if the data is deleted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Upload {
    pub files: Vec<FileOperation>,
    pub api: bool,
}

impl Upload {
    pub fn add_file(&mut self, name: String, failed: bool) {
        self.files.push(FileOperation { name, failed });
    }
}

/// Audit log data for a file delete operation
///
/// For this data, `entry_id` and `account_id` are only null if the data is deleted.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeleteFiles {
    pub files: Vec<FileOperation>,
    /// Whether the file deletion was permanent
    pub permanent: bool,
    /// The reason for the file delete, if any
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl DeleteFiles {
    pub fn add_file(&mut self, name: String, failed: bool) {
        self.files.push(FileOperation { name, failed });
    }
}

/// Audit log data for an entry delete operation
///
/// For this data, `entry_id` is null and `account_id` is null the data is deleted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeleteEntry {
    /// The name of the deleted entry
    pub name: String,
    /// Whether the deletion of the directory failed
    #[serde(default)]
    pub failed: bool,
}

/// Audit log data for a file delete operation
///
/// For this data, `entry_id` and `account_id` are only null if the data is deleted.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportFiles {
    pub files: Vec<String>,
    pub reason: String,
}

/// Audit log data for an entry delete operation
///
/// For this data, `entry_id` and `account_id` are only null if the data is deleted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportEntry {
    /// The name of the deleted entry
    pub name: String,
    pub reason: String,
}

/// Inner audit log data that represents a snapshot of the entry being edited
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntrySnapshot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub japanese_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub english_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anilist_id: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tmdb_id: Option<tmdb::Id>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(with = "crate::models::expand_flags::option")]
    pub flags: Option<EntryFlags>,
}

/// Audit log data for an entry edit operation
///
/// For this data, `entry_id` and `account_id` are only null if the data is deleted.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EditEntry {
    /// The state of the entry before the edit
    pub before: EntrySnapshot,
    /// The state of the entry after the edit
    pub after: EntrySnapshot,
    /// The columns that were changed from this edit
    pub changed: Vec<String>,
}

/// Audit log data for things related to the trash
///
/// For this data, `account_id` is never null but `entry_id` is.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrashAction {
    pub files: Vec<FileOperation>,
    /// Whether the data was restored or just deleted
    pub restore: bool,
}

impl TrashAction {
    pub fn add_file(&mut self, name: String, failed: bool) {
        self.files.push(FileOperation { name, failed });
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
pub enum AuditLogData {
    CreateEntry(CreateEntry),
    ScrapeResult(ScrapeResult),
    MoveEntry(MoveEntry),
    RenameFiles(RenameFiles),
    Upload(Upload),
    DeleteFiles(DeleteFiles),
    DeleteEntry(DeleteEntry),
    TrashAction(TrashAction),
    EditEntry(EditEntry),
    ReportFiles(ReportFiles),
    ReportEntry(ReportEntry),
}

impl From<ReportEntry> for AuditLogData {
    fn from(v: ReportEntry) -> Self {
        Self::ReportEntry(v)
    }
}

impl From<ReportFiles> for AuditLogData {
    fn from(v: ReportFiles) -> Self {
        Self::ReportFiles(v)
    }
}

impl From<TrashAction> for AuditLogData {
    fn from(v: TrashAction) -> Self {
        Self::TrashAction(v)
    }
}

impl From<EditEntry> for AuditLogData {
    fn from(v: EditEntry) -> Self {
        Self::EditEntry(v)
    }
}

impl From<DeleteEntry> for AuditLogData {
    fn from(v: DeleteEntry) -> Self {
        Self::DeleteEntry(v)
    }
}

impl From<DeleteFiles> for AuditLogData {
    fn from(v: DeleteFiles) -> Self {
        Self::DeleteFiles(v)
    }
}

impl From<Upload> for AuditLogData {
    fn from(v: Upload) -> Self {
        Self::Upload(v)
    }
}

impl From<RenameFiles> for AuditLogData {
    fn from(v: RenameFiles) -> Self {
        Self::RenameFiles(v)
    }
}

impl From<MoveEntry> for AuditLogData {
    fn from(v: MoveEntry) -> Self {
        Self::MoveEntry(v)
    }
}

impl From<ScrapeResult> for AuditLogData {
    fn from(v: ScrapeResult) -> Self {
        Self::ScrapeResult(v)
    }
}

impl From<CreateEntry> for AuditLogData {
    fn from(v: CreateEntry) -> Self {
        Self::CreateEntry(v)
    }
}

impl FromSql for AuditLogData {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        serde_json::from_str(value.as_str()?).map_err(|e| rusqlite::types::FromSqlError::Other(Box::new(e)))
    }
}

impl ToSql for AuditLogData {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        let as_str = serde_json::to_string(self).map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        Ok(rusqlite::types::ToSqlOutput::Owned(as_str.into()))
    }
}

/// An audit log entry
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditLogEntry {
    /// The ID of the entry. This is represented as a datetime with milliseconds precision
    /// in the database.
    pub id: i64,
    /// The directory entry that this audit log entry is the target of.
    pub entry_id: Option<i64>,
    /// The account responsible for this audit log entry.
    pub account_id: Option<i64>,
    /// The actual data for this audit log entry.
    pub data: AuditLogData,
}

fn datetime_to_ms(dt: OffsetDateTime) -> i64 {
    let ts = dt.unix_timestamp_nanos() / 1_000_000;
    ts as i64
}

impl AuditLogEntry {
    /// Creates a new audit log entry
    pub fn new<T>(data: T) -> Self
    where
        T: Into<AuditLogData>,
    {
        Self {
            id: datetime_to_ms(OffsetDateTime::now_utc()),
            entry_id: None,
            account_id: None,
            data: data.into(),
        }
    }

    pub fn full<T>(data: T, entry_id: i64, account_id: i64) -> Self
    where
        T: Into<AuditLogData>,
    {
        Self {
            id: datetime_to_ms(OffsetDateTime::now_utc()),
            entry_id: Some(entry_id),
            account_id: Some(account_id),
            data: data.into(),
        }
    }

    pub fn with_account(mut self, account_id: i64) -> Self {
        self.account_id = Some(account_id);
        self
    }

    pub fn created_at(&self) -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp_nanos(self.id as i128 * 1_000_000).unwrap_or(OffsetDateTime::UNIX_EPOCH)
    }
}

impl Table for AuditLogEntry {
    const NAME: &'static str = "audit_log";
    const COLUMNS: &'static [&'static str] = &["id", "entry_id", "account_id", "data"];
    type Id = i64;

    fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get("id")?,
            entry_id: row.get("entry_id")?,
            account_id: row.get("account_id")?,
            data: row.get("data")?,
        })
    }
}
