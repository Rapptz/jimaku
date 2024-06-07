use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::{anilist::MediaTitle, models::EntryFlags, tmdb, AppState};

/// A fixture that represents a directory entry that is pending addition to the database.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fixture {
    pub path: PathBuf,
    pub original_name: String,
    #[serde(with = "time::serde::timestamp")]
    pub last_updated_at: OffsetDateTime,
    #[serde(default)]
    pub anilist_id: Option<u32>,
    #[serde(default)]
    pub tmdb_id: Option<tmdb::Id>,
    pub title: MediaTitle,
    #[serde(default, with = "crate::models::expand_flags")]
    pub flags: EntryFlags,
}

pub async fn commit_fixtures(state: &AppState, fixtures: Vec<Fixture>) -> anyhow::Result<()> {
    state
        .database()
        .call(move |conn| -> rusqlite::Result<()> {
            let sql = r#"
                INSERT INTO directory_entry(path, last_updated_at, flags, anilist_id, tmdb_id, english_name, japanese_name, name)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?)
                ON CONFLICT DO UPDATE
                SET last_updated_at = MAX(last_updated_at, EXCLUDED.last_updated_at)
            "#;
            let tx = conn.transaction()?;
            {
                let mut stmt = tx.prepare(sql)?;
                for fixture in fixtures {
                    stmt.execute((
                        fixture.path.to_string_lossy(),
                        fixture.last_updated_at,
                        fixture.flags,
                        fixture.anilist_id,
                        fixture.tmdb_id,
                        fixture.title.english,
                        fixture.title.native,
                        fixture.title.romaji,
                    ))?;
                }
            }
            tx.commit()?;
            Ok(())
        })
        .await?;
    state.cached_directories().invalidate().await;
    Ok(())
}
