//! Facilities for parsing and scraping kitsunekko.

/*

The website uses a rather deliberate and static HTML output.
Since it's so simple to parse I'm going to parse it using the regex crate.

Examples:

For the main directory listing:
<tr><td colspan="2"><a href="/dirlist.php?dir=subtitles%2Fjapanese%2FOokami+Kakushi%2F" class=""><strong>Ookami Kakushi</strong> </a></td> <td class="tdright" title="Jul 15 2012 09:40:52 PM" > 1&nbsp;decade </td></tr>
<tr><td colspan="2"><a href="/dirlist.php?dir=subtitles%2Fjapanese%2FOokami-san+to+Shichinin+no+Nakama-tachi%2F" class=""><strong>Ookami-san to Shichinin no Nakama-tachi</strong> </a></td> <td class="tdright" title="Jul 15 2012 09:41:18 PM" > 1&nbsp;decade </td></tr>
<tr><td colspan="2"><a href="/dirlist.php?dir=subtitles%2Fjapanese%2Fother%2F" class=""><strong>other</strong> </a></td> <td class="tdright" title="Jul 15 2012 09:45:42 PM" > 1&nbsp;decade </td></tr>

For an item's file listing:
<tr><td><a href="subtitles/japanese/Undead Unluck/Undead.Unluck.S01E01.WEBRip.Netflix.ja[cc].srt" class=""><strong>Undead.Unluck.S01E01.WEBRip.Netflix.ja[cc].srt</strong> </a></td> <td class="tdleft"  title="37996"  > 37&nbsp;KB </td> <td class="tdright" title="Nov 08 2023 06:09:20 AM" > 3&nbsp;months </td></tr>
<tr><td><a href="subtitles/japanese/Undead Unluck/Undead.Unluck.S01E02.WEBRip.Netflix.ja[cc].srt" class=""><strong>Undead.Unluck.S01E02.WEBRip.Netflix.ja[cc].srt</strong> </a></td> <td class="tdleft"  title="47352"  > 46&nbsp;KB </td> <td class="tdright" title="Nov 08 2023 06:09:20 AM" > 3&nbsp;months </td></tr>
<tr><td><a href="subtitles/japanese/Undead Unluck/Undead.Unluck.S01E03.WEBRip.Netflix.ja[cc].srt" class=""><strong>Undead.Unluck.S01E03.WEBRip.Netflix.ja[cc].srt</strong> </a></td> <td class="tdleft"  title="39607"  > 39&nbsp;KB </td> <td class="tdright" title="Nov 08 2023 06:09:19 AM" > 3&nbsp;months </td></tr>

*/

use anyhow::{bail, Context};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, io::Write, path::PathBuf, sync::OnceLock};
use time::{
    format_description::FormatItem,
    macros::{format_description, offset},
    OffsetDateTime, PrimitiveDateTime,
};
use tokio::task::JoinSet;
use tracing::{info, warn};

use crate::{
    anilist::{Media, MediaTitle},
    audit::{AuditLogEntry, ScrapeDirectory, ScrapeResult},
    models::EntryFlags,
    tmdb, AppState,
};

fn regex() -> &'static Regex {
    static HTML_REGEX: OnceLock<Regex> = OnceLock::new();
    HTML_REGEX.get_or_init(|| {
        Regex::new(r#"<tr>.+<a\s*href=\x{22}(?P<url>[^\x{22}]+)\x{22}\s+(?:class=\x{22}[^\x{22}]*\x{22})?>\s*<strong>(?P<name>.+)</strong>.+<td.+title=\x{22}(?P<date>[^\x{22}]+)\x{22}.+</tr>"#).unwrap()
    })
}

fn remove_parentheses(haystack: &str) -> String {
    static PARENTHESES_REGEX: OnceLock<Regex> = OnceLock::new();
    let re = PARENTHESES_REGEX.get_or_init(|| Regex::new(r"(?:\([^)]+\))").unwrap());
    re.replace_all(haystack, "").into_owned()
}

const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:122.0) Gecko/20100101 Firefox/122.0";
const DATE_FORMAT: &[FormatItem<'static>] =
    format_description!("[month repr:short] [day] [year] [hour repr:12]:[minute]:[second] [period case:upper]");
const BASE_URL: &str = "https://kitsunekko.net";

#[derive(Debug, Clone)]
pub struct Directory {
    pub url: String,
    pub name: String,
    pub date: OffsetDateTime,
    pub files: Vec<File>,
}

#[derive(Debug, Clone)]
pub struct File {
    pub url: String,
    pub name: String,
    pub date: OffsetDateTime,
}

impl From<File> for Directory {
    fn from(value: File) -> Self {
        Self {
            url: value.url,
            name: value.name,
            date: value.date,
            files: Vec::new(),
        }
    }
}

impl File {
    async fn download(self, client: reqwest::Client, directory: PathBuf) -> anyhow::Result<()> {
        let path = directory.join(&self.name);
        if path.exists() {
            return Ok(());
        }

        let resp = client.get(&self.url).send().await?;
        if let Some(bytes) = resp.content_length() {
            if bytes >= crate::MAX_UPLOAD_SIZE {
                bail!(
                    "file at {} is over the maximum file size with {} bytes",
                    self.url,
                    bytes
                );
            }
        }

        let bytes = resp.bytes().await?;
        let mut file =
            std::fs::File::create(&path).with_context(|| format!("Could not create file at {}", path.display()))?;
        file.write_all(&bytes)?;
        Ok(())
    }

    fn is_supported(&self) -> bool {
        let extensions = [".zip", ".ass", ".srt", ".7z", ".rar", ".sup"];
        extensions.iter().any(|s| self.name.ends_with(s))
    }
}

impl Directory {
    /// Updates the `files` attribute with the file entries found for this entry.
    pub async fn find_files(&mut self, client: &reqwest::Client, date: &OffsetDateTime) -> anyhow::Result<()> {
        self.files = get_entries(client, &self.url).await?;
        self.files.retain(|f| f.is_supported() && &f.date > date);
        Ok(())
    }

    /// Concurrently downloads every file in this directory
    pub async fn download_files(self, client: &reqwest::Client, directory: PathBuf) -> anyhow::Result<()> {
        let mut set = JoinSet::new();
        for file in self.files {
            set.spawn(file.download(client.clone(), directory.clone()));
        }
        while let Some(result) = set.join_next().await {
            if let Ok(Err(e)) = result {
                warn!(error = %e, "Could not download file");
            }
        }
        Ok(())
    }
}

/// Returns a list of file entries from the URL.
///
/// This does not actually download anything. It merely fetches the information.
/// This method works with both the main directory listing and the subdirectory listings.
/// However, the return type is always [`File`]. Consider using `into()` to convert it
/// into a [`Directory`].
pub async fn get_entries(client: &reqwest::Client, url: &str) -> anyhow::Result<Vec<File>> {
    let body = client
        .get(url)
        .header(
            reqwest::header::USER_AGENT,
            reqwest::header::HeaderValue::from_static(USER_AGENT),
        )
        .send()
        .await?
        .text()
        .await?;

    let re = regex();
    re.captures_iter(&body)
        .map(|cap| {
            let captured_url = &cap["url"];
            let mut url = String::with_capacity(BASE_URL.len() + captured_url.len() + 1);
            url.push_str(BASE_URL);
            if !captured_url.starts_with('/') {
                url.push('/');
            }
            url.push_str(captured_url);
            anyhow::Ok(File {
                url,
                name: sanitise_file_name::sanitise(&cap["name"]),
                date: PrimitiveDateTime::parse(&cap["date"], DATE_FORMAT)?.assume_offset(offset!(+02:00)),
            })
        })
        .collect()
}

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
    #[serde(default)]
    pub movie: bool,
    #[serde(default)]
    pub adult: bool,
}

#[inline]
fn levenshtein_distance(title: &MediaTitle, query: &str) -> usize {
    let base = strsim::levenshtein(query, &title.romaji);
    if let Some(other) = &title.english {
        base.min(strsim::levenshtein(query, other))
    } else {
        base
    }
}

#[inline]
fn case_insensitive_search(title: &MediaTitle, query: &str) -> bool {
    let base = title.romaji.eq_ignore_ascii_case(query);
    if !base {
        if let Some(s) = &title.english {
            let result = s.eq_ignore_ascii_case(query);
            if !result {
                if let Some(jp) = &title.native {
                    return jp.eq_ignore_ascii_case(query);
                }
            } else {
                return true;
            }
        } else if let Some(jp) = &title.native {
            return jp.eq_ignore_ascii_case(query);
        }
    }
    base
}

async fn get_anilist_info(client: &reqwest::Client, query: &str) -> anyhow::Result<Option<Media>> {
    // The order of this is weird because I wanna rely on the response sort order before doing any
    // postprocessing, but doing it this way avoids the needless clone
    let has_parens = query.contains('(');
    let query_no_parens = remove_parentheses(query);
    let mut result = crate::anilist::search(client, query).await?;

    if result.len() == 1 {
        return Ok(Some(result.swap_remove(0)));
    }

    if result.is_empty() {
        if has_parens {
            result = crate::anilist::search(client, &query_no_parens).await?;
        } else {
            return Ok(None);
        }
    } else if has_parens {
        let extra = crate::anilist::search(client, &query_no_parens).await?;
        result.extend_from_slice(&extra);
    }

    // Sort and remove duplicate entries by ID
    result.sort_by_key(|s| s.id);
    result.dedup_by_key(|s| s.id);

    // Check if there's an exact match using case insensitive search
    if let Some(idx) = result.iter().position(|m| case_insensitive_search(&m.title, query)) {
        return Ok(Some(result.swap_remove(idx)));
    }

    match result.len() {
        0 => Ok(None),
        1 => Ok(Some(result.swap_remove(0))),
        _ => Ok(result
            .into_iter()
            .min_by_key(|m| levenshtein_distance(&m.title, &query_no_parens))),
    }
}

async fn get_redirects(state: &AppState) -> Option<HashMap<String, i64>> {
    let from_storage = state
        .database()
        .get_from_storage::<String>("kitsunekko_redirects")
        .await?;
    serde_json::from_str(&from_storage).ok()
}

pub async fn scrape(state: &AppState, date: OffsetDateTime) -> anyhow::Result<Vec<Fixture>> {
    let mut potential_dupes: HashMap<u32, Fixture> = HashMap::new();
    let mut result = Vec::new();

    let mut directories = get_entries(
        &state.client,
        "https://kitsunekko.net/dirlist.php?dir=subtitles%2Fjapanese%2F",
    )
    .await?
    .into_iter()
    .filter(|f| f.date > date)
    .map(Directory::from)
    .collect::<Vec<_>>();

    directories.sort_by_key(|s| s.date);
    let subtitle_path = state.config().subtitle_path.as_path();
    let total = directories.len();
    let redirects = get_redirects(state).await.unwrap_or_default();
    for (index, mut entry) in directories.into_iter().enumerate() {
        entry.find_files(&state.client, &date).await?;
        if entry.files.is_empty() {
            info!(
                "[{}/{}] skipping {:?} due to having no files",
                index + 1,
                total,
                &entry.name
            );
            continue;
        }

        let mut directory = subtitle_path.join(&entry.name);
        if let Some(entry_id) = redirects.get(&entry.name) {
            info!(
                "[{}/{}] redirecting {:?} to entry ID {}",
                index + 1,
                total,
                &entry.name,
                entry_id
            );
            if let Some(original) = state.get_directory_entry(*entry_id).await {
                directory = original.path;
                let as_fixture = Fixture {
                    path: directory.clone(),
                    original_name: original.name.clone(),
                    last_updated_at: entry.date,
                    anilist_id: original.anilist_id,
                    tmdb_id: original.tmdb_id,
                    title: MediaTitle {
                        romaji: original.name,
                        english: original.english_name,
                        native: original.japanese_name,
                    },
                    movie: original.flags.is_movie(),
                    adult: original.flags.is_adult(),
                };
                if let Some(anilist_id) = original.anilist_id {
                    potential_dupes.insert(anilist_id, as_fixture);
                } else {
                    result.push(as_fixture);
                }
            }
        } else if let Ok(Some(media)) = get_anilist_info(&state.client, &entry.name).await {
            if let Some(fixture) = potential_dupes.get_mut(&media.id) {
                directory = fixture.path.clone();
                fixture.last_updated_at = fixture.last_updated_at.max(entry.date);
                info!(
                    "entry {:?} is a duplicate with anilist ID of {}, downloading to original path {} instead",
                    &entry.name,
                    media.id,
                    fixture.path.strip_prefix(subtitle_path).unwrap().display()
                );
            } else {
                // Check if it this AniList ID exists in the database already
                if let Some(path) = state.get_anilist_directory_entry_path(media.id).await {
                    directory = path;
                }
                potential_dupes.insert(
                    media.id,
                    Fixture {
                        original_name: entry.name.clone(),
                        path: directory.clone(),
                        last_updated_at: entry.date,
                        anilist_id: Some(media.id),
                        tmdb_id: None,
                        adult: media.adult,
                        movie: media.is_movie(),
                        title: media.title,
                    },
                );
            }
        } else {
            result.push(Fixture {
                path: directory.clone(),
                last_updated_at: entry.date,
                original_name: entry.name.clone(),
                anilist_id: None,
                tmdb_id: None,
                title: MediaTitle::new(entry.name.clone()),
                adult: false,
                movie: false,
            });
        }

        if !directory.exists() {
            if let Err(e) = std::fs::create_dir_all(&directory) {
                if e.kind() != std::io::ErrorKind::AlreadyExists {
                    return Err(e).with_context(|| format!("Could not create directory {}", directory.display()));
                }
            }
        }

        let name = entry.name.clone();
        entry.download_files(&state.client, directory).await?;
        info!("[{}/{}] finished downloading {:?}", index + 1, total, name);
    }

    result.extend(potential_dupes.into_values());
    info!(
        "finished downloading {} entries ({} total, {} skipped)",
        result.len(),
        total,
        total - result.len()
    );
    Ok(result)
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
                    let mut flags = EntryFlags::default();
                    flags.set_low_quality(true);
                    flags.set_external(true);
                    flags.set_movie(fixture.movie);
                    flags.set_adult(fixture.adult);
                    stmt.execute((
                        fixture.path.to_string_lossy(),
                        fixture.last_updated_at,
                        flags,
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

pub async fn auto_scrape_loop(state: AppState) {
    let (signal_tx, signal_rx) = tokio::sync::mpsc::channel::<()>(1);
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.unwrap();
        drop(signal_rx);
    });

    loop {
        let enabled = state
            .database()
            .get_from_storage::<bool>("kitsunekko_scrape_enabled")
            .await
            .unwrap_or(true);

        if !enabled {
            break;
        }

        let mut date = state
            .database()
            .get_from_storage::<OffsetDateTime>("kitsunekko_scrape_date")
            .await
            .unwrap_or(OffsetDateTime::UNIX_EPOCH);
        let result = scrape(&state, date).await;
        match result {
            Ok(fixtures) => {
                let new_date = fixtures.iter().map(|x| x.last_updated_at).max();
                let mut scrape = ScrapeResult {
                    directories: fixtures
                        .iter()
                        .map(|f| ScrapeDirectory {
                            original_name: f.original_name.clone(),
                            name: f.title.romaji.clone(),
                            anilist_id: f.anilist_id,
                        })
                        .collect(),
                    error: false,
                    date: new_date,
                };
                if let Err(e) = commit_fixtures(&state, fixtures).await {
                    tracing::error!(error = %e, "Error occurred while committing fixtures");
                    scrape.error = true;
                } else if let Some(dt) = new_date {
                    date = dt;
                    let preview = crate::utils::join_iter(
                        "\n",
                        scrape.directories.iter().map(|x| format!("- {}", x.name)).take(25),
                    );
                    state.send_alert(
                        crate::discord::Alert::success("Scraped from Kitsunekko")
                            .url("/logs")
                            .description(preview)
                            .field("Total", scrape.directories.len()),
                    );
                }
                state.audit(AuditLogEntry::new(scrape)).await;
            }
            Err(e) => {
                state.audit(AuditLogEntry::new(ScrapeResult::errored())).await;
                tracing::error!(error = %e, "Error occurred while scraping Kitsunekko");
            }
        }

        let _ = state.database().update_storage("kitsunekko_scrape_date", date).await;
        tokio::select! {
            _ = tokio::time::sleep(std::time::Duration::from_secs(3600)) => {

            }
            _ = signal_tx.closed() => {
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_kitsunekko_parse() -> anyhow::Result<()> {
        let client = reqwest::Client::new();
        let url = "https://kitsunekko.net/dirlist.php?dir=subtitles%2Fjapanese%2F";
        let captures = get_entries(&client, url).await?;
        println!("got {} entries", captures.len());
        println!("{:?}", &captures[0..5]);
        Ok(())
    }

    #[tokio::test]
    async fn test_anilist() -> anyhow::Result<()> {
        let client = reqwest::Client::new();
        let queries = [
            "Kusuriya no Hitorigoto (The Apothecary Diaries)",
            "Chibi Maruko-chan (1990)",
            "Boku no Kokoro no Yabai Yatsu",
            "Haikyuu!! S4",
            "Haikyuu!! Second Season",
            "Beyblade X",
        ];

        for query in queries {
            let search = get_anilist_info(&client, query).await?;
            println!("{query}: {search:#?}");
        }
        Ok(())
    }
}
