//! Facilities for parsing and scraping jpsubbers.com.

/*

The website uses a rather deliberate and static HTML output.
Since it's so simple to parse I'm going to parse it using the regex crate.

Examples:

For the main directory listing:
<div style="line-height:1.5"><a href="index.php?p=/@OtherSPs">@OtherSPs</a></div>
<div style="line-height:1.5"><a href="index.php?p=/アンチ・ヒーロー">アンチ・ヒーロー</a></div>
<div style="line-height:1.5"><a href="index.php?p=/ブルー・モーメント">ブルー・モーメント</a></div>

For an item's file listing:
<div style="line-height:1.5"><a href="/Japanese-Subtitles/光る君へ/光る君へ＃02.srt">光る君へ＃02.srt</a></div>

Basically the same idea.

There are a few caveats:

1) If the name starts with @ then it's an inner non-airing group so it should be skipped.
2) If the name is `..` then it's bringing you up a directory, so should be ignored.
3) There is no last_modified information so each scrape loop will force a full crawl either way.
*/

use std::{io::Write, path::PathBuf, sync::OnceLock};

use anyhow::{bail, Context};
use regex::Regex;
use time::OffsetDateTime;
use tokio::task::JoinSet;
use tracing::{info, warn};

use crate::{
    anilist::MediaTitle,
    audit::{AuditLogEntry, ScrapeDirectory, ScrapeResult, ScrapeSource},
    fixture::{commit_fixtures, Fixture},
    kitsunekko::USER_AGENT,
    tmdb, AppState,
};

const BASE_URL: &str = "https://jpsubbers.com";

fn regex() -> &'static Regex {
    static HTML_REGEX: OnceLock<Regex> = OnceLock::new();
    HTML_REGEX
        .get_or_init(|| Regex::new(r#"<a\s*href=\x{22}(?P<url>[^\x{22}]+)\x{22}\s*>(?P<name>[^<]+)</a>"#).unwrap())
}

#[derive(Debug, Clone)]
pub struct Directory {
    pub url: String,
    pub name: String,
    pub files: Vec<File>,
}

#[derive(Debug, Clone)]
pub struct File {
    pub url: String,
    pub name: String,
}

impl From<File> for Directory {
    fn from(value: File) -> Self {
        Self {
            url: value.url,
            name: value.name,
            files: Vec::new(),
        }
    }
}

impl File {
    async fn download(self, client: reqwest::Client, directory: PathBuf) -> anyhow::Result<bool> {
        let path = directory.join(&self.name);
        if path.exists() {
            return Ok(false);
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
        Ok(true)
    }

    fn is_supported(&self) -> bool {
        let extensions = [".zip", ".ass", ".srt", ".7z", ".rar", ".sup"];
        extensions.iter().any(|s| self.name.ends_with(s))
    }
}

impl Directory {
    /// Updates the `files` attribute with the file entries found for this entry.
    pub async fn find_files(&mut self, client: &reqwest::Client) -> anyhow::Result<()> {
        self.files = get_entries(client, &self.url).await?;
        self.files.retain(|f| f.is_supported());
        Ok(())
    }

    /// Concurrently downloads every file in this directory
    pub async fn download_files(self, client: &reqwest::Client, directory: PathBuf) -> anyhow::Result<usize> {
        let mut set = JoinSet::new();
        for file in self.files {
            set.spawn(file.download(client.clone(), directory.clone()));
        }
        let mut downloaded = 0;
        while let Some(result) = set.join_next().await {
            match result {
                Ok(Err(e)) => warn!(error = %e, "Could not download file"),
                Ok(Ok(true)) => downloaded += 1,
                _ => {}
            }
        }
        Ok(downloaded)
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
        .filter_map(|cap| {
            let captured_url = &cap["url"];
            let mut url = String::with_capacity(BASE_URL.len() + captured_url.len() + 1);
            url.push_str(BASE_URL);
            if !captured_url.starts_with('/') {
                url.push_str("/Japanese-Subtitles/");
            }
            url.push_str(captured_url);
            let name = &cap["name"];
            if name.starts_with('@') || name == ".." {
                None
            } else {
                Some(anyhow::Ok(File {
                    url,
                    name: sanitise_file_name::sanitise(name),
                }))
            }
        })
        .collect()
}

/// Cleans up a query by removing known problematic patterns in JPSubber names
///
/// Right now these are, e.g. [3rd], [4th], etc. and ・ in between kana.
///
/// There is another known pattern with numbers, e.g. JKと６法全書 vs JKと六法全書 but this one is not
/// as simple to fix, since e.g. ９ボーダー and ９５ are both fine.
fn prepare_query(haystack: &str) -> String {
    static KNOWN_PATTERNS: OnceLock<Regex> = OnceLock::new();
    let re = KNOWN_PATTERNS.get_or_init(|| Regex::new(r"(?:・|(?:\d|[０-９])+(?:st|nd|rd|th))").unwrap());
    re.replace_all(haystack, "").into_owned()
}

pub async fn scrape(state: &AppState) -> anyhow::Result<Vec<Fixture>> {
    let mut result = Vec::new();
    let directories = get_entries(&state.client, "https://jpsubbers.com/Japanese-Subtitles/")
        .await?
        .into_iter()
        .map(Directory::from)
        .collect::<Vec<_>>();

    let api_key = &state.config().tmdb_api_key;
    let subtitle_path = state.config().subtitle_path.as_path();
    let total = directories.len();
    for (index, mut entry) in directories.into_iter().enumerate() {
        entry.find_files(&state.client).await?;
        if entry.files.is_empty() {
            info!(
                "[{}/{}] skipping {:?} due to having no files",
                index + 1,
                total,
                &entry.name
            );
            continue;
        }

        let mut directory = subtitle_path.join(format!("jpsubbers_{}", &entry.name));
        let query = prepare_query(&entry.name);
        let fixture = if let Ok(Some(info)) = tmdb::find_match(&state.client, api_key, &query).await {
            // Check if it this AniList ID exists in the database already
            if let Some(path) = state.get_tmdb_directory_entry_path(info.id).await {
                directory = path;
            }
            Fixture {
                original_name: entry.name.clone(),
                path: directory.clone(),
                last_updated_at: OffsetDateTime::now_utc(),
                anilist_id: None,
                tmdb_id: Some(info.id),
                adult: info.is_adult(),
                movie: info.id.is_movie(),
                unverified: true,
                external: false,
                title: info.titles(),
            }
        } else {
            Fixture {
                path: directory.clone(),
                last_updated_at: OffsetDateTime::now_utc(),
                original_name: entry.name.clone(),
                anilist_id: None,
                tmdb_id: None,
                title: MediaTitle::new(entry.name.clone()),
                adult: false,
                movie: false,
                unverified: true,
                external: false,
            }
        };

        if !directory.exists() {
            if let Err(e) = std::fs::create_dir_all(&directory) {
                if e.kind() != std::io::ErrorKind::AlreadyExists {
                    return Err(e).with_context(|| format!("Could not create directory {}", directory.display()));
                }
            }
        }

        let name = entry.name.clone();
        let download_count = entry.download_files(&state.client, directory).await?;
        if download_count == 0 {
            info!(
                "[{}/{}] skipping {:?} due to having no new files",
                index + 1,
                total,
                name
            );
            continue;
        } else {
            info!("[{}/{}] finished downloading {:?}", index + 1, total, name);
            result.push(fixture);
        }
    }

    info!(
        "finished downloading {} entries ({} total, {} skipped)",
        result.len(),
        total,
        total - result.len()
    );
    Ok(result)
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
            .get_from_storage::<bool>("jpsubbers_scrape_enabled")
            .await
            .unwrap_or(true);

        if !enabled {
            break;
        }

        let result = scrape(&state).await;
        match result {
            Ok(fixtures) => {
                let mut scrape = ScrapeResult {
                    directories: fixtures
                        .iter()
                        .map(|f| ScrapeDirectory {
                            original_name: f.original_name.clone(),
                            name: f.title.romaji.clone(),
                            tmdb_id: f.tmdb_id,
                            anilist_id: None,
                        })
                        .collect(),
                    error: false,
                    date: None,
                    source: ScrapeSource::Jpsubbers,
                };
                if let Err(e) = commit_fixtures(&state, fixtures).await {
                    tracing::error!(error = %e, "Error occurred while committing fixtures");
                    scrape.error = true;
                } else if !scrape.directories.is_empty() {
                    let preview = crate::utils::join_iter(
                        "\n",
                        scrape.directories.iter().map(|x| format!("- {}", x.name)).take(25),
                    );
                    state.send_alert(
                        crate::discord::Alert::success("Scraped from JPSubbers")
                            .url("/logs")
                            .description(preview)
                            .field("Total", scrape.directories.len()),
                    );
                }
                state.audit(AuditLogEntry::new(scrape)).await;
            }
            Err(e) => {
                state.audit(AuditLogEntry::new(ScrapeResult::errored())).await;
                tracing::error!(error = %e, "Error occurred while scraping JPSubbers");
            }
        }

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
    async fn test_jpsubbers_parse() -> anyhow::Result<()> {
        let client = reqwest::Client::new();
        let url = "https://jpsubbers.com/Japanese-Subtitles/";
        let mut captures = get_entries(&client, url)
            .await?
            .into_iter()
            .map(Directory::from)
            .collect::<Vec<_>>();
        for dir in captures.iter_mut() {
            dir.find_files(&client).await?;
        }
        println!("got {} entries", captures.len());
        println!("{:?}", &captures[0..5]);
        Ok(())
    }
}
