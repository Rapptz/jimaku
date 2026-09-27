//! A check of shows and of uploaded subtitles against the names of video releases.
//!
//! Debrid services, and the search tools around them (Debrid Media Manager, Torrentio,
//! Zilean, Prowlarr), find a video by the name of its release. These names also tell that a
//! show exists, and which of its episodes exist, when AniList and TMDB do not list the show
//! or the episode. This module sends a query to one search of release names: an RSS feed such
//! as the one of nyaa.si, or the Torznab API of Prowlarr, Jackett or Zilean. Then it compares
//! the names with the names of an entry, and with the names of uploaded subtitle files.
//!
//! A release name is evidence, not proof: anybody can publish a torrent with any name. Thus
//! an entry that only releases name is unverified until an editor looks at it, its first
//! release must be some days old, and a subtitle file that no release names gets a note, not
//! a refusal. The module keeps no infohash, magnet link or torrent link, and it downloads no
//! torrent.

use std::{
    sync::{Arc, OnceLock},
    time::{Duration, Instant},
};

use anitomy::ElementKind;
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use quick_xml::{Reader, escape::resolve_predefined_entity, events::Event};
use serde::{Deserialize, Serialize};
use time::{OffsetDateTime, format_description::well_known::Rfc2822};

/// The time between two queries to the search. nyaa.si asks crawlers to wait 5 seconds.
const SPACING: Duration = Duration::from_secs(5);
/// How long the answer to a query is kept.
const KEEP: Duration = Duration::from_secs(60 * 60);
/// A search that returns this many releases or more can have left older releases out.
const FULL_PAGE: usize = 50;
/// The number of extra queries for the episodes of one upload.
const MAX_EPISODE_QUERIES: usize = 3;
/// A shorter title of an episode is too common to tell two episodes apart.
const MIN_TITLE_CHARS: usize = 4;
/// A longer range of episodes in a name is not a range of episodes.
const MAX_RANGE: u32 = 500;

/// A search of release names. See the module documentation.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ReleaseIndex {
    /// The URL of an RSS search, with `{query}` where the search terms go. For example
    /// `https://nyaa.si/?page=rss&c=0_0&f=0&q={query}`, or the Torznab URL of an indexer
    /// in Prowlarr: `http://127.0.0.1:9696/1/api?t=search&apikey=KEY&q={query}`.
    pub url: String,
    /// The number of releases that must name a show before a user who is not an editor
    /// can add the show without an AniList or TMDB page.
    #[serde(default = "default_min_releases")]
    pub min_releases: usize,
    /// The age in days that the first of those releases must have.
    #[serde(default = "default_min_age_days")]
    pub min_age_days: u32,
}

fn default_min_releases() -> usize {
    2
}

fn default_min_age_days() -> u32 {
    7
}

/// A release that the search found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub name: String,
    pub published: Option<OffsetDateTime>,
}

/// The releases that one query found.
#[derive(Debug, Clone, Default)]
pub struct Found {
    pub releases: Vec<Release>,
    /// True when the search returned as many releases as a page holds. Then older releases
    /// can be missing, and the absence of a release tells nothing.
    pub full: bool,
}

/// Reads the items of an RSS feed (RSS 2.0, which Torznab uses too): the title of each item
/// is the name of a release. An answer that is not a feed is an error, not an empty search.
pub fn parse_feed(xml: &str) -> anyhow::Result<Vec<Release>> {
    let mut reader = Reader::from_str(xml);
    let mut path: Vec<Vec<u8>> = Vec::new();
    let mut is_feed = false;
    let mut releases = Vec::new();
    let (mut title, mut date) = (String::new(), String::new());
    loop {
        let text = match reader.read_event()? {
            Event::Start(element) => {
                let name = element.local_name().as_ref().to_vec();
                is_feed |= name == b"channel";
                if name == b"item" {
                    title.clear();
                    date.clear();
                }
                path.push(name);
                continue;
            }
            Event::End(_) => {
                if path.pop().as_deref() == Some(b"item") && !title.trim().is_empty() {
                    releases.push(Release {
                        name: title.trim().to_owned(),
                        published: parse_date(&date),
                    });
                }
                continue;
            }
            Event::Text(text) => text.xml10_content()?.into_owned(),
            Event::CData(data) => data.decode()?.into_owned(),
            Event::GeneralRef(reference) => match reference.resolve_char_ref()? {
                Some(c) => c.to_string(),
                None => resolve_predefined_entity(&reference.decode()?)
                    .unwrap_or_default()
                    .to_owned(),
            },
            Event::Eof => break,
            _ => continue,
        };
        match path.as_slice() {
            [.., item, field] if item == b"item" && field == b"title" => title.push_str(&text),
            [.., item, field] if item == b"item" && field == b"pubDate" => date.push_str(&text),
            _ => {}
        }
    }
    anyhow::ensure!(is_feed, "the search did not answer with an RSS feed");
    Ok(releases)
}

/// Reads an RFC 2822 date. RSS writes "-0000" for UTC, which RFC 2822 calls an unknown zone.
fn parse_date(date: &str) -> Option<OffsetDateTime> {
    let date = date.trim();
    let date = match date.strip_suffix("-0000") {
        Some(rest) => format!("{rest}+0000"),
        None => date.to_owned(),
    };
    OffsetDateTime::parse(&date, &Rfc2822).ok()
}

fn cache() -> &'static quick_cache::sync::Cache<String, (Instant, Arc<Found>)> {
    static CACHE: OnceLock<quick_cache::sync::Cache<String, (Instant, Arc<Found>)>> = OnceLock::new();
    CACHE.get_or_init(|| quick_cache::sync::Cache::new(256))
}

/// Waits until the previous query is [`SPACING`] old.
async fn wait_for_turn() {
    static LAST: OnceLock<tokio::sync::Mutex<Option<Instant>>> = OnceLock::new();
    let mut last = LAST.get_or_init(Default::default).lock().await;
    if let Some(at) = *last {
        tokio::time::sleep_until((at + SPACING).into()).await;
    }
    *last = Some(Instant::now());
}

impl ReleaseIndex {
    /// The releases that the search finds for the words. An answer is kept for an hour.
    pub async fn search(&self, client: &reqwest::Client, terms: &str) -> anyhow::Result<Arc<Found>> {
        let url = self
            .url
            .replace("{query}", &utf8_percent_encode(terms, NON_ALPHANUMERIC).to_string());
        if let Some((at, found)) = cache().get(&url)
            && at.elapsed() < KEEP
        {
            return Ok(found);
        }
        wait_for_turn().await;
        let body = client
            .get(&url)
            .header(
                reqwest::header::USER_AGENT,
                concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION")),
            )
            .timeout(Duration::from_secs(20))
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;
        let releases = parse_feed(&body)?;
        let found = Arc::new(Found {
            full: releases.len() >= FULL_PAGE,
            releases,
        });
        cache().insert(url, (Instant::now(), found.clone()));
        Ok(found)
    }

    /// Searches the releases of the show that has this name.
    pub async fn find_show(&self, client: &reqwest::Client, name: &str) -> anyhow::Result<ShowEvidence> {
        let found = self.search(client, &terms(name)).await?;
        Ok(show_evidence(&found.releases, &Show::new([name])))
    }

    /// Says whether the releases are enough to add a show that has no AniList or TMDB page.
    /// The `Ok` value is the note for the new entry. The `Err` value tells the user why not.
    pub fn judge(&self, name: &str, evidence: ShowEvidence, now: OffsetDateTime) -> Result<String, String> {
        let fallback = "Give the AniList or TMDB page of the show, or ask an editor to add it.";
        if evidence.releases == 0 {
            return Err(format!("The search found no release of \"{name}\". {fallback}"));
        }
        if evidence.releases < self.min_releases {
            let found = match evidence.releases {
                1 => String::from("1 release"),
                n => format!("{n} releases"),
            };
            return Err(format!(
                "The search found only {found} of \"{name}\", and {} are necessary. {fallback}",
                self.min_releases
            ));
        }
        let first = evidence.first.map(|date| date.date());
        let old_enough = evidence
            .first
            .is_some_and(|date| now - date >= time::Duration::days(self.min_age_days.into()));
        if !old_enough {
            let when = first.map_or_else(|| String::from("of an unknown date"), |date| format!("from {date}"));
            return Err(format!(
                "The first release of \"{name}\" is {when}. It must be {} days old. {fallback}",
                self.min_age_days
            ));
        }
        let first = first.map(|date| date.to_string()).unwrap_or_default();
        Ok(format!(
            "Added from the release check: {} releases name this show, the first on {first}. An editor did not verify it yet.",
            evidence.releases
        ))
    }
}

/// A text as the comparisons see it: letters and digits only, in lower case.
/// So "Can't", "Can’t" and "CANT" are the same.
pub fn key(text: &str) -> String {
    text.chars()
        .flat_map(char::to_lowercase)
        .filter(|c| c.is_alphanumeric())
        .collect()
}

/// The words of a title, as [`key`] writes each of them.
fn words(title: &str) -> Vec<String> {
    title
        .split(|c: char| !c.is_alphanumeric())
        .map(key)
        .filter(|word| !word.is_empty())
        .collect()
}

fn is_digits(word: &str) -> bool {
    !word.is_empty() && word.bytes().all(|b| b.is_ascii_digit())
}

/// True if the word gives a part, a season or an episode: "part", "part3", "season",
/// "s2", "2nd", "e28", "ep5".
fn is_part(word: &str) -> bool {
    matches!(word, "part" | "season" | "cour" | "episode")
        || ["part", "season", "cour", "episode", "ep", "s", "e"]
            .iter()
            .any(|marker| word.strip_prefix(marker).is_some_and(is_digits))
        || ["st", "nd", "rd", "th"]
            .iter()
            .any(|suffix| word.strip_suffix(suffix).is_some_and(is_digits))
}

/// The words without a part or a season at their end: releases add "Part 3", "2nd Season"
/// or "S2" to the title of a show, and entries do not. At least one word stays.
fn without_part(mut words: Vec<String>) -> Vec<String> {
    loop {
        let end = match words.as_slice() {
            [.., marker, number] if matches!(marker.as_str(), "part" | "season" | "cour") && is_digits(number) => 2,
            [.., last] if is_part(last) => 1,
            _ => 0,
        };
        if end == 0 || end >= words.len() {
            return words;
        }
        words.truncate(words.len() - end);
    }
}

/// The title of a show as the comparisons see it: the letters and digits of its words,
/// without a part or a season at its end. "Show: Part 3" and "SHOW" have the same key.
pub fn show_key(title: &str) -> String {
    without_part(words(title)).concat()
}

/// A show as the comparisons see it: the words of each of its names.
#[derive(Debug, Clone, Default)]
pub struct Show(Vec<Vec<String>>);

impl Show {
    pub fn new<'a>(names: impl IntoIterator<Item = &'a str>) -> Self {
        let mut show: Vec<Vec<String>> = Vec::new();
        for name in names {
            let name = without_part(words(name));
            if !name.is_empty() && !show.contains(&name) {
                show.push(name);
            }
        }
        Self(show)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// True if the title (the words of [`Name::title`]) is of this show: it has the key of a
    /// name of the show, or the words of a name and then a part, a season or an episode.
    /// The parser can leave those in a title: "Terrace House-Tokyo 2019-2020 Part3 E28 2019".
    pub fn has(&self, title: &[String]) -> bool {
        let key = without_part(title.to_vec()).concat();
        self.0.iter().any(|name| {
            name.concat() == key || (title.len() > name.len() && title.starts_with(name) && is_part(&title[name.len()]))
        })
    }
}

/// What a release name, or the name of a subtitle file, tells about the video.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Name {
    /// The words of the title of the show.
    pub title: Vec<String>,
    pub season: Option<u32>,
    /// The episodes: one, all the episodes of a range, or none for a season or a film.
    pub episodes: Vec<u32>,
    /// The title of the episode, as [`key`] writes it.
    pub episode_title: Option<String>,
}

impl Name {
    pub fn parse(name: &str) -> Self {
        let mut parsed = Self::default();
        let mut numbers = Vec::new();
        for element in anitomy::parse(name) {
            match element.kind() {
                ElementKind::Title => parsed.title = words(element.value()),
                ElementKind::Season if parsed.season.is_none() => parsed.season = element.value().parse().ok(),
                ElementKind::Episode => numbers.extend(element.value().parse::<u32>().ok()),
                ElementKind::EpisodeTitle => {
                    parsed.episode_title = Some(key(element.value())).filter(|t| t.chars().count() >= MIN_TITLE_CHARS)
                }
                _ => {}
            }
        }
        // Two numbers are a range: "01-12" is each episode from 1 to 12.
        parsed.episodes = match numbers[..] {
            [first, last] if first < last && last - first <= MAX_RANGE => (first..=last).collect(),
            _ => numbers,
        };
        parsed
    }

    /// True if the name tells which episode it is for.
    pub fn has_episode(&self) -> bool {
        !self.episodes.is_empty() || self.episode_title.is_some()
    }

    /// True if the release names the episode that this file is for: it has the same number
    /// in the same season (or one of the two names has no season), or the same title.
    ///
    /// One episode can have three numbers. "Terrace House: Tokyo 2019-2020" has Netflix's
    /// S03E25, TMDB's S03E01 and the S01E25 of some releases. The title of the episode is
    /// the same in all of them, so it is compared too. A release can put more words in front
    /// of the title ("Week25 The Girls Can't Do It"), so one title may end with the other.
    pub fn is_named_by(&self, release: &Name) -> bool {
        let same_season = match (self.season, release.season) {
            (Some(ours), Some(theirs)) => ours == theirs,
            _ => true,
        };
        let same_number = same_season && self.episodes.iter().any(|e| release.episodes.contains(e));
        let same_title = match (&self.episode_title, &release.episode_title) {
            (Some(ours), Some(theirs)) => ours.ends_with(theirs.as_str()) || theirs.ends_with(ours.as_str()),
            _ => false,
        };
        same_number || same_title
    }
}

/// What the releases tell about a show before it has an entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShowEvidence {
    /// The number of releases of the show.
    pub releases: usize,
    /// The date of the first of them.
    pub first: Option<OffsetDateTime>,
}

/// Counts the releases of the show.
pub fn show_evidence(releases: &[Release], show: &Show) -> ShowEvidence {
    let ours: Vec<&Release> = releases
        .iter()
        .filter(|release| show.has(&Name::parse(&release.name).title))
        .collect();
    ShowEvidence {
        releases: ours.len(),
        first: ours.iter().filter_map(|release| release.published).min(),
    }
}

/// The result of the check of one uploaded file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileCheck {
    /// This many releases of the show name the episode.
    Named(usize),
    /// No release of the show names the episode, and the search was complete.
    NotNamed,
    /// The name of the file tells no episode, the search failed, or the check stopped early.
    NotChecked,
}

/// Checks one file against the releases of its show.
pub fn check_file(file: &Name, releases: &[Name], full: bool) -> FileCheck {
    if !file.has_episode() {
        return FileCheck::NotChecked;
    }
    match releases.iter().filter(|release| file.is_named_by(release)).count() {
        0 if full => FileCheck::NotChecked,
        0 => FileCheck::NotNamed,
        n => FileCheck::Named(n),
    }
}

/// The search terms for a name: its words, without punctuation.
fn terms(name: &str) -> String {
    name.split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Checks the uploaded files of an entry against the releases of the show. `names` are the
/// names of the entry, the first one first. A file that the search of the show cannot
/// settle gets a search of its own episode, up to [`MAX_EPISODE_QUERIES`] for each upload.
pub async fn check_files(
    client: &reqwest::Client,
    index: &ReleaseIndex,
    names: &[&str],
    files: &[String],
) -> Vec<FileCheck> {
    let show = Show::new(names.iter().copied());
    let parsed: Vec<Name> = files.iter().map(|file| Name::parse(file)).collect();
    if show.is_empty() || !parsed.iter().any(Name::has_episode) {
        return vec![FileCheck::NotChecked; files.len()];
    }

    let of_show = |found: &Found| -> Vec<Name> {
        found
            .releases
            .iter()
            .map(|release| Name::parse(&release.name))
            .filter(|release| show.has(&release.title))
            .collect()
    };

    // The search of the show: the first name that finds releases of the show. When no
    // search finds the show, a missing episode tells nothing.
    let mut searched: Option<(String, Vec<Name>, bool)> = None;
    let mut seen_terms: Vec<String> = Vec::new();
    for name in names {
        let words = terms(name);
        if words.is_empty() || seen_terms.contains(&words) {
            continue;
        }
        seen_terms.push(words.clone());
        match index.search(client, &words).await {
            Ok(found) => {
                let ours = of_show(&found);
                if !ours.is_empty() {
                    searched = Some((words, ours, found.full));
                    break;
                }
            }
            Err(e) => tracing::warn!(error = %e, "the release search did not answer"),
        }
    }
    let Some((words, releases, full)) = searched else {
        return vec![FileCheck::NotChecked; files.len()];
    };

    let mut queries = 0;
    let mut checks = Vec::with_capacity(files.len());
    for file in &parsed {
        let mut check = check_file(file, &releases, full);
        if check == FileCheck::NotChecked && file.has_episode() && queries < MAX_EPISODE_QUERIES {
            let episode = match (file.season, file.episodes.first()) {
                (Some(season), Some(episode)) => Some(format!("S{season:02}E{episode:02}")),
                (None, Some(episode)) => Some(format!("{episode:02}")),
                _ => None,
            };
            if let Some(episode) = episode {
                queries += 1;
                match index.search(client, &format!("{words} {episode}")).await {
                    Ok(found) => check = check_file(file, &of_show(&found), found.full),
                    Err(e) => tracing::warn!(error = %e, "the release search did not answer"),
                }
            }
        }
        checks.push(check);
    }
    checks
}

/// Notes for the uploader about the result of [`check_files`].
pub fn notes(files: &[String], checks: &[FileCheck]) -> Vec<String> {
    let checked = checks.iter().filter(|check| **check != FileCheck::NotChecked).count();
    if checked == 0 {
        return Vec::new();
    }
    let named = checks
        .iter()
        .filter(|check| matches!(check, FileCheck::Named(_)))
        .count();
    let mut notes = vec![format!(
        "Release check: {named} of {checked} files match a release of the show."
    )];
    for (file, check) in files.iter().zip(checks) {
        if *check == FileCheck::NotNamed {
            notes.push(format!(
                "{file}: no release of the show names this episode. Make sure that the season and the episode are correct."
            ));
        }
    }
    notes
}

#[cfg(test)]
mod tests {
    use quick_xml::escape::escape;
    use time::macros::datetime;

    use super::*;

    /// Release names of "Terrace House: Tokyo 2019-2020" from nyaa.si (2026-09-26), with
    /// three numbering schemes: S03E01 with Week25, Part3 E27, and S01E25. The range of
    /// years is not in these names, because anitomy reads "2019-2020" as the episodes 2019
    /// and 2020 until it keeps a range of years for last.
    const TERRACE_HOUSE: &[&str] = &[
        "Terrace.House.Tokyo.S04E06.Week42.Woman.Who.Makes.Everyone.Dream.1080p",
        "Terrace.House.Tokyo.S04E05.Week41.Life-Threatening.Date.1080p",
        "Terrace.House.Tokyo.S04E04.Week40.Never.Forgive.Luigi.1080p",
        "Terrace.House.Tokyo.S04E03.Week39.Always.Remembered.1080p",
        "Terrace.House.Tokyo.S04E02.Week38.Case.of.The.Costume.Incident.1080p",
        "Terrace.House.Tokyo.S04E01.Week37.Another.Terrace!!.1080p",
        "Terrace.House.Tokyo.S03E12.Week36.Angel.1080p",
        "Terrace.House.Tokyo.S03E11.Week35.The.Monster.in.the.Hallway.1080p",
        "Terrace.House.Tokyo.S03E10.Week34.Case.of.The.Bottled.Beer.Incident.1080p",
        "Terrace.House.Tokyo.S03E09.Week33.Half.Blue.1080p",
        "Terrace.House.Tokyo.S03E08.Week32.I.Hate.You.1080p",
        "Terrace.House.Tokyo.S03E07.Week31.Publicity.Stunt.1080p (REPACK)",
        "Terrace.House.Tokyo.S03E06.Week30.Not Guilty.1080p",
        "Terrace House-Tokyo Part3 E29 2019 1080p NF WEB-DL DDP2.0 x264-IRENEBRO",
        "Terrace.House.Tokyo.S03E05.Week29.About.Love.1080p",
        "Terrace.House-Tokyo.Part3.E28.2019.1080p.NF.WEB-DL.DDP2.0.x264-IRENEBRO",
        "Terrace.House.Tokyo.S03E04.Week28.Starving.for.Affection.1080p",
        "Terrace.House-Tokyo.Part3.E27.2019.1080p.NF.WEB-DL.DDP2.0.x264-IRENEBRO",
        "Terrace.House.Tokyo.S03E03.Week27.I.Can't.Be.Here.1080p",
        "Terrace.House.Tokyo.S01E26.Internationalization.at.Once.1080p.NF.WEB-DL.DDP2.0.x264.mkv",
        "Terrace.House.Tokyo.S03E02.Week26.1080p",
        "Terrace.House.Tokyo.S01E25.The.Girls.Can't.Do.It.1080p.NF.WEB-DL.DDP2.0.x264.mkv",
        "Terrace.House.Tokyo.S03E01.Week25.1080p",
    ];

    /// The Netflix titles of Part 3 and Part 4 (episodes 25 to 42), as TMDB lists them.
    const PARTS_3_AND_4: &[(u32, u32, &str)] = &[
        (3, 25, "The.Girls.Can't.Do.It"),
        (3, 26, "Internationalization.at.Once"),
        (3, 27, "I.Can't.Be.Here"),
        (3, 28, "Starving.for.Affection"),
        (3, 29, "About.Love"),
        (3, 30, "Not.Guilty"),
        (3, 31, "Publicity.Stunt"),
        (3, 32, "I.Hate.You"),
        (3, 33, "Half.Blue"),
        (3, 34, "Case.of.The.Bottled.Beer.Incident"),
        (3, 35, "The.Monster.in.the.Hallway"),
        (3, 36, "Angel"),
        (4, 37, "Another.Terrace!!"),
        (4, 38, "Case.of.The.Costume.Incident"),
        (4, 39, "Always.Remembered"),
        (4, 40, "Never.Forgive.Luigi"),
        (4, 41, "Life-Threatening.Date"),
        (4, 42, "Woman.Who.Makes.Everyone.Dream"),
    ];

    /// A subtitle file named as on jimaku.cc entry 9561, without the range of years.
    fn netflix_file(season: u32, episode: u32, title: &str) -> String {
        format!("テラスハウス_.Tokyo.S{season:02}E{episode:02}.{title}.WEBRip.Netflix.ja[cc].srt")
    }

    fn releases(names: &[&str]) -> Vec<Name> {
        names.iter().map(|name| Name::parse(name)).collect()
    }

    fn rss(items: &[(String, Option<OffsetDateTime>)]) -> String {
        let mut xml = String::from(
            r#"<?xml version="1.0" encoding="UTF-8"?><rss version="2.0" xmlns:nyaa="https://nyaa.si/xmlns/nyaa"><channel><title>search</title>"#,
        );
        for (name, date) in items {
            xml.push_str("<item><title>");
            xml.push_str(&escape(name.as_str()));
            xml.push_str("</title>");
            if let Some(date) = date {
                xml.push_str("<pubDate>");
                xml.push_str(&date.format(&Rfc2822).unwrap());
                xml.push_str("</pubDate>");
            }
            xml.push_str("<nyaa:seeders>3</nyaa:seeders></item>");
        }
        xml.push_str("</channel></rss>");
        xml
    }

    #[test]
    fn each_file_of_parts_3_and_4_matches_a_release() {
        let found = releases(TERRACE_HOUSE);
        for (season, episode, title) in PARTS_3_AND_4 {
            let file = Name::parse(&netflix_file(*season, *episode, title));
            assert!(
                matches!(check_file(&file, &found, false), FileCheck::Named(_)),
                "S{season:02}E{episode:02} {title}: {file:?}"
            );
        }
    }

    #[test]
    fn an_episode_that_was_never_released_matches_no_release() {
        let found = releases(TERRACE_HOUSE);
        let file = Name::parse(&netflix_file(4, 43, "A.Girl.With.Pure.Fortune"));
        assert_eq!(check_file(&file, &found, false), FileCheck::NotNamed);
        // A search that can have left releases out says nothing about the episode.
        assert_eq!(check_file(&file, &found, true), FileCheck::NotChecked);
    }

    #[test]
    fn a_file_without_an_episode_is_not_checked() {
        let found = releases(TERRACE_HOUSE);
        let file = Name::parse("テラスハウス.srt");
        assert_eq!(check_file(&file, &found, false), FileCheck::NotChecked);
    }

    #[test]
    fn the_same_number_in_another_season_is_another_episode() {
        let found = releases(&["Show.S01E05.1080p.WEB-DL.mkv"]);
        assert_eq!(
            check_file(&Name::parse("Show.S02E05.ja.srt"), &found, false),
            FileCheck::NotNamed
        );
        assert_eq!(
            check_file(&Name::parse("Show.S01E05.ja.srt"), &found, false),
            FileCheck::Named(1)
        );
        // A name without a season (the numbers of an anime) can be of any season.
        assert_eq!(
            check_file(&Name::parse("[Group] Show - 05 [1080p].ass"), &found, false),
            FileCheck::Named(1)
        );
    }

    #[test]
    fn a_batch_names_each_episode_of_its_range() {
        let found = releases(&["[Group] Show 01-12 [1080p]"]);
        assert_eq!(
            check_file(&Name::parse("Show - 07.srt"), &found, false),
            FileCheck::Named(1)
        );
        assert_eq!(
            check_file(&Name::parse("Show - 13.srt"), &found, false),
            FileCheck::NotNamed
        );
    }

    #[test]
    fn a_show_is_the_show_without_its_part() {
        let show = Show::new(["Terrace House: Tokyo", "テラスハウス Tokyo"]);
        for name in TERRACE_HOUSE {
            assert!(
                show.has(&Name::parse(name).title),
                "{name}: {:?}",
                Name::parse(name).title
            );
        }
        for other in [
            "Terrace.House.Opening.New.Doors.S01E01.1080p",
            "Terrace House Aloha State S01E01",
            "Terrace.House.Tokyo.Partner.1080p",
        ] {
            assert!(!show.has(&Name::parse(other).title), "{other}");
        }
        assert_eq!(show_key("Show 2nd Season"), "show");
        assert_eq!(show_key("Show Season 2"), "show");
        assert_eq!(show_key("Show: Part 3"), "show");
        assert_eq!(show_key("Show S2"), "show");
        assert_eq!(show_key("Mob Psycho 100"), "mobpsycho100");
        assert_eq!(show_key("Season 2"), "season2", "a title is not only a part");
    }

    #[test]
    fn the_evidence_for_a_show_counts_its_releases_only() {
        let items: Vec<Release> = [
            (
                "Terrace.House.Tokyo.S03E01.Week25.1080p",
                Some(datetime!(2019-12-10 12:00 UTC)),
            ),
            (
                "Terrace.House.Tokyo.S03E02.Week26.1080p",
                Some(datetime!(2019-12-17 12:00 UTC)),
            ),
            (
                "Terrace.House.Opening.New.Doors.S01E01.1080p",
                Some(datetime!(2017-06-01 12:00 UTC)),
            ),
            ("Terrace.House.Tokyo.S03E03.Week27.1080p", None),
        ]
        .into_iter()
        .map(|(name, published)| Release {
            name: name.to_owned(),
            published,
        })
        .collect();
        let evidence = show_evidence(&items, &Show::new(["Terrace House: Tokyo"]));
        assert_eq!(
            evidence,
            ShowEvidence {
                releases: 3,
                first: Some(datetime!(2019-12-10 12:00 UTC)),
            }
        );
    }

    #[test]
    fn a_show_needs_enough_releases_that_are_old_enough() {
        let index = ReleaseIndex {
            url: String::new(),
            min_releases: 2,
            min_age_days: 7,
        };
        let now = datetime!(2026-09-26 12:00 UTC);
        let judge = |releases, first| index.judge("Show", ShowEvidence { releases, first }, now);
        assert!(
            judge(0, None)
                .unwrap_err()
                .starts_with("The search found no release of \"Show\"")
        );
        assert!(
            judge(1, Some(datetime!(2020-01-01 0:00 UTC)))
                .unwrap_err()
                .contains("2 are necessary")
        );
        let why = judge(3, Some(datetime!(2026-09-24 0:00 UTC))).unwrap_err();
        assert!(why.contains("from 2026-09-24") && why.contains("7 days old"), "{why}");
        assert!(judge(3, None).unwrap_err().contains("unknown date"));
        let note = judge(3, Some(datetime!(2019-12-10 0:00 UTC))).unwrap();
        assert!(note.contains("3 releases") && note.contains("2019-12-10"), "{note}");
    }

    #[test]
    fn a_feed_of_nyaa_and_of_torznab_is_read() {
        let nyaa = r#"<?xml version="1.0" encoding="utf-8"?>
<rss xmlns:atom="http://www.w3.org/2005/Atom" xmlns:nyaa="https://nyaa.si/xmlns/nyaa" version="2.0">
	<channel>
		<title>Nyaa - "terrace house" - Torrent File RSS</title>
		<item>
			<title>Terrace.House.Tokyo.S03E03.Week27.I.Can&#39;t.Be.Here.1080p</title>
			<link>https://nyaa.si/download/1.torrent</link>
			<pubDate>Tue, 24 Dec 2019 01:02:03 -0000</pubDate>
			<nyaa:seeders>2</nyaa:seeders>
		</item>
		<item><title>A &amp; B</title></item>
	</channel>
</rss>"#;
        let found = parse_feed(nyaa).unwrap();
        assert_eq!(
            found,
            vec![
                Release {
                    name: String::from("Terrace.House.Tokyo.S03E03.Week27.I.Can't.Be.Here.1080p"),
                    published: Some(datetime!(2019-12-24 01:02:03 UTC)),
                },
                Release {
                    name: String::from("A & B"),
                    published: None,
                },
            ]
        );
        let torznab = r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:torznab="http://torznab.com/schemas/2015/feed"><channel>
<item><title><![CDATA[Show S01E01 1080p]]></title><pubDate>Mon, 01 Jan 2024 10:00:00 +0900</pubDate>
<torznab:attr name="seeders" value="4" /></item></channel></rss>"#;
        assert_eq!(
            parse_feed(torznab).unwrap(),
            vec![Release {
                name: String::from("Show S01E01 1080p"),
                published: Some(datetime!(2024-01-01 01:00 UTC)),
            }]
        );
    }

    #[test]
    fn a_page_that_is_not_a_feed_is_an_error() {
        assert!(parse_feed("<!doctype html><html><body>Checking your browser</body></html>").is_err());
        assert!(parse_feed("").is_err());
        assert!(parse_feed(r#"{"results": []}"#).is_err());
        assert_eq!(parse_feed(&rss(&[])).unwrap(), Vec::new());
    }

    #[test]
    fn the_notes_name_each_file_that_no_release_names() {
        let files = vec![String::from("a.srt"), String::from("b.srt"), String::from("c.srt")];
        let notes = notes(
            &files,
            &[FileCheck::Named(2), FileCheck::NotNamed, FileCheck::NotChecked],
        );
        assert_eq!(notes.len(), 2);
        assert_eq!(notes[0], "Release check: 1 of 2 files match a release of the show.");
        assert!(notes[1].starts_with("b.srt: no release"));
        assert!(super::notes(&files, &[FileCheck::NotChecked; 3]).is_empty());
    }

    /// A small random generator (xorshift), so that the tests below try many inputs. The
    /// seed is fixed, so each run tries the same inputs.
    struct Random(u64);

    impl Random {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }

        fn below(&mut self, n: u64) -> u64 {
            self.next() % n
        }

        fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
            items[self.below(items.len() as u64) as usize]
        }

        fn text(&mut self, pieces: &[&str], max: u64) -> String {
            (0..self.below(max + 1)).map(|_| self.pick(pieces)).collect()
        }
    }

    /// Pieces of text with the cases that break a comparison or an XML document: case,
    /// punctuation, letters that change length in lower case, and characters outside the BMP.
    const PIECES: &[&str] = &[
        "a",
        "Z",
        "7",
        " ",
        "-",
        ".",
        ":",
        "'",
        "’",
        "é",
        "É",
        "İ",
        "ß",
        "𝕊",
        "テラス",
        "猫",
        "&",
        "<",
        ">",
        "\"",
        "]]>",
        "ｱ",
        "Ⅻ",
        "\u{2028}",
    ];

    /// Words that the parser does not take for a keyword (a source, a type, a codec).
    const WORDS: &[&str] = &[
        "Alpha", "Bravo", "Charlie", "Delta", "Echo", "Foxtrot", "Golf", "Hotel", "India", "Juliet", "Kilo", "Lima",
        "Mike", "November", "Oscar", "Papa", "Quebec", "Romeo", "Sierra", "Tango", "Uniform", "Victor", "Whiskey",
    ];

    /// A key has only letters and digits, in lower case, and the key of a key is the same
    /// key. ("𝕊" is an upper-case letter that has no lower case.)
    #[test]
    fn a_key_is_stable() {
        let mut random = Random(0x2545F4914F6CDD1D);
        for _ in 0..3000 {
            let text = random.text(PIECES, 12);
            let once = key(&text);
            assert_eq!(key(&once), once, "{text:?}");
            assert!(once.chars().all(char::is_alphanumeric), "{text:?}");
            assert_eq!(once.to_lowercase(), once, "{text:?}");
        }
    }

    /// Punctuation, case and a part at the end do not change the show.
    #[test]
    fn a_show_key_ignores_the_form_of_the_title() {
        let mut random = Random(0x9E3779B97F4A7C15);
        for _ in 0..3000 {
            let words: Vec<&str> = (0..1 + random.below(4)).map(|_| random.pick(WORDS)).collect();
            let separator = random.pick(&[" ", ".", "_", ":", "-", " - "]);
            let plain = words.join(" ");
            let mut other = words
                .iter()
                .map(|w| w.to_uppercase())
                .collect::<Vec<_>>()
                .join(separator);
            match random.below(3) {
                0 => other.push_str(&format!("{separator}Part{}", 1 + random.below(20))),
                1 => other.push_str(&format!("{separator}Season {}", 1 + random.below(9))),
                _ => {}
            }
            assert_eq!(show_key(&plain), show_key(&other), "{plain:?} {other:?}");
        }
    }

    /// A subtitle file named after a release matches that release, and does not match a
    /// release of another episode with another title.
    #[test]
    fn a_file_matches_the_release_it_is_named_after() {
        let mut random = Random(0xD1B54A32D192ED03);
        for _ in 0..1000 {
            let season = 1 + random.below(9);
            let episode = 1 + random.below(199);
            let title: Vec<&str> = (0..1 + random.below(3)).map(|_| random.pick(WORDS)).collect();
            let other_title: Vec<&str> = (0..1 + random.below(3)).map(|_| random.pick(WORDS)).collect();
            let (title, other_title) = (title.join("."), other_title.join("."));
            // A title that ends with the other title is the same title (see `is_named_by`).
            if key(&title).ends_with(&key(&other_title)) || key(&other_title).ends_with(&key(&title)) {
                continue;
            }
            let release = Name::parse(&format!(
                "Show.Name.S{season:02}E{episode:02}.{title}.1080p.WEB-DL.x264-GRP.mkv"
            ));
            let file = Name::parse(&format!(
                "Show.Name.S{season:02}E{episode:02}.{title}.WEBRip.Netflix.ja[cc].srt"
            ));
            assert!(file.is_named_by(&release), "{file:?} {release:?}");
            let other = Name::parse(&format!(
                "Show.Name.S{season:02}E{:02}.{other_title}.1080p.WEB-DL.x264-GRP.mkv",
                episode + 1
            ));
            assert!(!file.is_named_by(&other), "{file:?} {other:?}");
        }
    }

    /// The titles and the dates of a feed are read back as they were written.
    #[test]
    fn a_feed_is_read_back() {
        let mut random = Random(0xA0761D6478BD642F);
        for _ in 0..500 {
            let items: Vec<(String, Option<OffsetDateTime>)> = (0..random.below(8))
                .map(|_| {
                    let name = format!("x{}", random.text(PIECES, 20)).trim().to_owned();
                    let seconds = random.below(2_000_000_000) as i64;
                    let date = (random.below(4) != 0).then(|| OffsetDateTime::from_unix_timestamp(seconds).unwrap());
                    (name, date)
                })
                .collect();
            let expected: Vec<Release> = items
                .iter()
                .map(|(name, published)| Release {
                    name: name.clone(),
                    published: *published,
                })
                .collect();
            assert_eq!(parse_feed(&rss(&items)).unwrap(), expected);
        }
    }
}
