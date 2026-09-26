//! RSS feeds. Each listing has a feed of its entries with the newest files, so that a feed
//! reader shows each new upload.

use askama::Template;
use axum::{
    Router,
    extract::State,
    http::{StatusCode, header::CONTENT_TYPE},
    response::{IntoResponse, Response},
    routing::get,
};

use crate::{AppState, filters, models::DirectoryEntry};

/// The largest number of entries in a feed.
const MAX_ITEMS: usize = 50;

#[derive(Template)]
#[template(path = "feed.xml")]
struct FeedTemplate<'a> {
    /// The name of the listing: "Anime" or "Live Action".
    name: &'static str,
    /// The canonical URL of the site, with no slash at the end.
    base: String,
    /// The path of the page of the listing.
    page: &'static str,
    /// The path of the feed.
    path: &'static str,
    /// The entries of the listing with the newest files, the newest first.
    entries: Vec<&'a DirectoryEntry>,
}

impl<'a> FeedTemplate<'a> {
    fn new(base: String, anime: bool, entries: impl Iterator<Item = &'a DirectoryEntry>) -> Self {
        let mut entries: Vec<_> = entries.filter(|e| e.flags.is_anime() == anime).collect();
        entries.sort_by_key(|e| std::cmp::Reverse(e.last_updated_at));
        entries.truncate(MAX_ITEMS);
        let (name, page, path) = if anime {
            ("Anime", "/", "/feed.xml")
        } else {
            ("Live Action", "/dramas", "/dramas/feed.xml")
        };
        Self {
            name,
            base,
            page,
            path,
            entries,
        }
    }
}

async fn respond(state: &AppState, anime: bool) -> Response {
    let entries = state.directory_entries().await;
    match FeedTemplate::new(state.config().canonical_url(), anime, entries.iter()).render() {
        Ok(xml) => ([(CONTENT_TYPE, "application/rss+xml; charset=utf-8")], xml).into_response(),
        Err(error) => {
            tracing::error!(%error, "Failed to render a feed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

async fn anime_feed(State(state): State<AppState>) -> Response {
    respond(&state, true).await
}

async fn dramas_feed(State(state): State<AppState>) -> Response {
    respond(&state, false).await
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/feed.xml", get(anime_feed))
        .route("/dramas/feed.xml", get(dramas_feed))
}

#[cfg(test)]
mod tests {
    use time::OffsetDateTime;

    use super::*;

    fn entry(id: i64, name: &str, seconds: i64, anime: bool) -> DirectoryEntry {
        let mut entry = DirectoryEntry::temporary(name.to_owned());
        entry.id = id;
        entry.last_updated_at = OffsetDateTime::from_unix_timestamp(seconds).unwrap();
        entry.flags.set_anime(anime);
        entry
    }

    fn render(anime: bool, entries: &[DirectoryEntry]) -> String {
        FeedTemplate::new(String::from("https://jimaku.cc"), anime, entries.iter())
            .render()
            .unwrap()
    }

    #[test]
    fn a_feed_has_the_entries_of_its_listing_with_the_newest_first() {
        let entries = [
            entry(1, "Old Anime", 1_700_000_000, true),
            entry(2, "New Anime", 1_800_000_000, true),
            entry(3, "A Drama", 1_900_000_000, false),
        ];
        let xml = render(true, &entries);
        assert!(xml.starts_with("<?xml"));
        assert!(xml.contains("<title>Jimaku: Anime</title>"));
        assert!(xml.contains("<link>https://jimaku.cc/</link>"));
        assert!(xml.contains(r#"<atom:link href="https://jimaku.cc/feed.xml""#));
        let new = xml.find("<link>https://jimaku.cc/entry/2</link>").unwrap();
        let old = xml.find("<link>https://jimaku.cc/entry/1</link>").unwrap();
        assert!(new < old, "the newest entry is first");
        assert!(!xml.contains("/entry/3<"), "a drama is not in the anime feed");
        assert!(xml.contains(r#"<guid isPermaLink="false">https://jimaku.cc/entry/2#1800000000</guid>"#));
        assert!(xml.contains("<pubDate>Fri, 15 Jan 2027 08:00:00 +0000</pubDate>"));

        let xml = render(false, &entries);
        assert!(xml.contains("<title>Jimaku: Live Action</title>"));
        assert!(xml.contains("<link>https://jimaku.cc/dramas</link>"));
        assert!(xml.contains("/entry/3</link>") && !xml.contains("/entry/1</link>"));
    }

    #[test]
    fn a_name_cannot_break_the_xml() {
        let xml = render(true, &[entry(1, "Tom & Jerry <TV>\u{1}", 1_800_000_000, true)]);
        assert!(xml.contains("<title>Tom &#38; Jerry &#60;TV&#62; </title>"), "{xml}");
    }

    #[test]
    fn a_feed_holds_at_most_the_newest_entries() {
        let entries: Vec<_> = (0..MAX_ITEMS as i64 + 10)
            .map(|id| entry(id, "Anime", 1_800_000_000 + id, true))
            .collect();
        let xml = render(true, &entries);
        assert_eq!(xml.matches("<item>").count(), MAX_ITEMS);
        assert!(xml.contains(&format!("/entry/{}</link>", MAX_ITEMS + 9)));
        assert!(
            !xml.contains("/entry/9</link>"),
            "the oldest entries are not in the feed"
        );
    }
}
