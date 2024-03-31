use std::path::PathBuf;

use rusqlite::{
    types::{FromSql, FromSqlResult, ToSqlOutput, ValueRef},
    ToSql,
};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::{database::Table, tmdb};

#[derive(Deserialize, Serialize, PartialEq, Eq, Clone, Copy)]
pub struct DirectoryFlags(u32);

impl FromSql for DirectoryFlags {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let value = u32::column_result(value)?;
        Ok(Self(value))
    }
}

impl ToSql for DirectoryFlags {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(self.0.into())
    }
}

impl DirectoryFlags {
    const ANIME: u32 = 1 << 0;
    const LOW_QUALITY: u32 = 1 << 1;
    const EXTERNAL: u32 = 1 << 2;
    const MOVIE: u32 = 1 << 3;
    const ADULT: u32 = 1 << 4;

    pub const fn new() -> Self {
        Self(Self::ANIME)
    }

    #[inline]
    fn has_flag(&self, val: u32) -> bool {
        (self.0 & val) == val
    }

    #[inline]
    fn toggle_flag(&mut self, val: u32, toggle: bool) {
        if toggle {
            self.0 |= val;
        } else {
            self.0 &= !val;
        }
    }

    pub fn is_anime(&self) -> bool {
        self.has_flag(Self::ANIME)
    }

    pub fn set_anime(&mut self, toggle: bool) {
        self.toggle_flag(Self::ANIME, toggle)
    }

    pub fn is_low_quality(&self) -> bool {
        self.has_flag(Self::LOW_QUALITY)
    }

    pub fn set_low_quality(&mut self, toggle: bool) {
        self.toggle_flag(Self::LOW_QUALITY, toggle)
    }

    pub fn is_external(&self) -> bool {
        self.has_flag(Self::EXTERNAL)
    }

    pub fn set_external(&mut self, toggle: bool) {
        self.toggle_flag(Self::EXTERNAL, toggle)
    }

    pub fn is_movie(&self) -> bool {
        self.has_flag(Self::MOVIE)
    }

    pub fn set_movie(&mut self, toggle: bool) {
        self.toggle_flag(Self::MOVIE, toggle)
    }

    pub fn is_adult(&self) -> bool {
        self.has_flag(Self::ADULT)
    }

    pub fn set_adult(&mut self, toggle: bool) {
        self.toggle_flag(Self::ADULT, toggle)
    }
}

impl Default for DirectoryFlags {
    fn default() -> Self {
        Self(Self::ANIME)
    }
}

impl std::fmt::Debug for DirectoryFlags {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DirectoryFlags")
            .field("value", &self.0)
            .field("anime", &self.is_anime())
            .field("low_quality", &self.is_low_quality())
            .field("external", &self.is_external())
            .finish()
    }
}

/// A directory entry that contains subtitles.
///
/// These are typically backed by e.g. an anilist or tmdb entry to
/// facilitate some features.
#[derive(Debug, Serialize, PartialEq, Eq, Clone)]
pub struct DirectoryEntry {
    /// The ID of the directory entry.
    pub id: i64,
    /// The physical exact path where this entry belongs in the filesystem.
    pub path: PathBuf,
    /// The romaji name of the entry.
    pub name: String,
    /// The flags associated with this entry
    pub flags: DirectoryFlags,
    /// The date of the newest uploaded file
    #[serde(rename = "last_modified")]
    #[serde(with = "time::serde::timestamp")]
    pub last_updated_at: OffsetDateTime,
    /// The account ID that created this entry
    pub creator_id: Option<i64>,
    /// The anilist ID of this entry.
    pub anilist_id: Option<u32>,
    /// The TMDB ID of this entry.
    pub tmdb_id: Option<tmdb::Id>,
    /// Extra notes that the entry might have.
    ///
    /// Supports a limited set of markdown. Can only be set by editors.
    pub notes: Option<String>,
    /// The English name of the entry.
    pub english_name: Option<String>,
    /// The Japanese name of the entry, i.e. with kanji and kana.
    pub japanese_name: Option<String>,
}

impl Table for DirectoryEntry {
    const NAME: &'static str = "directory_entry";

    const COLUMNS: &'static [&'static str] = &[
        "id",
        "path",
        "flags",
        "last_updated_at",
        "creator_id",
        "anilist_id",
        "tmdb_id",
        "notes",
        "english_name",
        "japanese_name",
        "name",
    ];

    fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        let path: String = row.get("path")?;
        Ok(Self {
            id: row.get("id")?,
            path: PathBuf::from(path),
            name: row.get("name")?,
            flags: row.get("flags")?,
            last_updated_at: row.get("last_updated_at")?,
            creator_id: row.get("creator_id")?,
            anilist_id: row.get("anilist_id")?,
            tmdb_id: row.get("tmdb_id")?,
            notes: row.get("notes")?,
            english_name: row.get("english_name")?,
            japanese_name: row.get("japanese_name")?,
        })
    }
}

/// Data that is passed around from the server to the frontend JavaScript
#[derive(Debug, Clone, Serialize)]
pub struct DirectoryEntryData<'a> {
    /// The romaji name of the entry.
    pub name: &'a str,
    /// The flags associated with this entry
    pub flags: DirectoryFlags,
    /// When the entry was last updated
    #[serde(rename = "last_modified")]
    #[serde(with = "time::serde::timestamp")]
    pub last_updated_at: &'a OffsetDateTime,
    /// The anilist ID of this entry.
    pub anilist_id: Option<u32>,
    /// The TMDB ID of this entry.
    pub tmdb_id: Option<tmdb::Id>,
    /// The English name of the entry.
    pub english_name: &'a Option<String>,
    /// The Japanese name of the entry, i.e. with kanji and kana.
    pub japanese_name: &'a Option<String>,
}

impl DirectoryEntry {
    /// Returns data safe for embedding into the frontend
    pub fn data(&self) -> DirectoryEntryData<'_> {
        DirectoryEntryData {
            name: &self.name,
            flags: self.flags,
            last_updated_at: &self.last_updated_at,
            anilist_id: self.anilist_id,
            tmdb_id: self.tmdb_id,
            english_name: &self.english_name,
            japanese_name: &self.japanese_name,
        }
    }

    /// Returns an appropriate description for the og:description meta tag
    pub fn description(&self) -> String {
        let mut base = String::from("Download Japanese subtitles for ");
        base.push_str(&self.name);
        base.push_str(". ");
        if let Some(english) = self.english_name.as_deref() {
            base.push_str("Also known as ");
            base.push_str(english);
            base.push_str(" in English");
        }

        if let Some(japanese) = self.japanese_name.as_deref() {
            if self.english_name.is_none() {
                base.push_str("Also known as ");
            } else {
                base.push_str(" or ");
            }
            base.push_str(japanese);
            base.push_str(" in Japanese");
        }
        if let Some(ch) = base.as_bytes().last() {
            if *ch == b' ' {
                base.pop();
            } else {
                base.push('.');
            }
        }
        base
    }
}

#[derive(Deserialize, Serialize, Default, PartialEq, Eq, Clone, Copy)]
pub struct AccountFlags(u32);

impl FromSql for AccountFlags {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        let value = u32::column_result(value)?;
        Ok(Self(value))
    }
}

impl ToSql for AccountFlags {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(self.0.into())
    }
}

impl AccountFlags {
    const ADMIN: u32 = 1 << 0;
    const EDITOR: u32 = 1 << 1;

    pub const fn new() -> Self {
        Self(0)
    }

    #[inline]
    fn has_flag(&self, val: u32) -> bool {
        (self.0 & val) == val
    }

    #[inline]
    fn toggle_flag(&mut self, val: u32, toggle: bool) {
        if toggle {
            self.0 |= val;
        } else {
            self.0 &= !val;
        }
    }

    pub fn is_admin(&self) -> bool {
        self.has_flag(Self::ADMIN)
    }

    pub fn set_admin(&mut self, toggle: bool) {
        self.toggle_flag(Self::ADMIN, toggle)
    }

    pub fn is_editor(&self) -> bool {
        self.is_admin() || self.has_flag(Self::EDITOR)
    }

    pub fn set_editor(&mut self, toggle: bool) {
        self.toggle_flag(Self::EDITOR, toggle)
    }
}

impl std::fmt::Debug for AccountFlags {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AccountFlags")
            .field("value", &self.0)
            .field("editor", &self.is_editor())
            .field("admin", &self.is_admin())
            .finish()
    }
}

/// A registered account.
///
/// This server implements a rather simple authentication scheme.
/// Passwords are hashed using Argon2. No emails are stored.
///
/// Authentication is also done using [`crate::token::Token`] instead of
/// maintaining a session database.
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Account {
    /// The account ID.
    pub id: i64,
    /// The username of the account.
    ///
    /// Usernames are all lowercase, and can only contain [a-z0-9._\-] characters.
    pub name: String,
    /// The Argon hashed password.
    pub password: String,
    /// The account flags associated with this account.
    pub flags: AccountFlags,
}

impl Table for Account {
    const NAME: &'static str = "account";

    const COLUMNS: &'static [&'static str] = &["id", "name", "password", "flags"];

    fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get("id")?,
            name: row.get("name")?,
            password: row.get("password")?,
            flags: row.get("flags")?,
        })
    }
}

/// A trait for getting some information out of the account.
///
/// This works with `Option<Account>` as well. It's basically
/// just a cleaner way of doing `map` followed by `unwrap_or_default`.
pub trait AccountCheck {
    fn flags(&self) -> AccountFlags;
}

impl AccountCheck for Account {
    fn flags(&self) -> AccountFlags {
        self.flags
    }
}

impl AccountCheck for Option<Account> {
    fn flags(&self) -> AccountFlags {
        self.as_ref().map(|t| t.flags).unwrap_or_default()
    }
}

pub fn is_valid_username(s: &str) -> bool {
    s.len() >= 3
        && s.len() <= 32
        && s.as_bytes()
            .iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'.' || *c == b'_' || *c == b'-')
}
