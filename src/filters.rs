//! Askama filters

use std::fmt::Display;
use time::OffsetDateTime;

#[repr(transparent)]
pub struct OptionalDisplay<'a, T>(&'a Option<T>);

pub fn maybe_display<T: Display>(opt: &Option<T>) -> askama::Result<OptionalDisplay<'_, T>> {
    Ok(OptionalDisplay(opt))
}

impl<'a, T: Display> Display for OptionalDisplay<'a, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(value) = &self.0 {
            value.fmt(f)
        } else {
            Ok(())
        }
    }
}

pub fn isoformat(dt: &OffsetDateTime) -> askama::Result<String> {
    let (hours, minutes, _) = dt.offset().as_hms();
    Ok(format!(
        "{}-{:02}-{:02} {:02}:{:02}:{:02}{:+03}:{:02}",
        dt.year(),
        dt.month() as u8,
        dt.day(),
        dt.hour(),
        dt.minute(),
        dt.second(),
        hours,
        minutes.abs()
    ))
}

/// Returns a canonical URL to the given path
pub fn canonical_url(url: impl Display) -> askama::Result<String> {
    let path = url.to_string();
    let mut url = crate::CONFIG.get().unwrap().canonical_url();
    url.push_str(&path);
    Ok(url)
}

pub fn maybe_tmdb_url(opt: &Option<crate::tmdb::Id>) -> askama::Result<String> {
    Ok(opt.as_ref().map(|x| x.url()).unwrap_or_default())
}

pub fn maybe_anilist_url(opt: &Option<u32>) -> askama::Result<String> {
    Ok(opt
        .as_ref()
        .map(|x| format!("https://anilist.co/anime/{x}"))
        .unwrap_or_default())
}

/// HTML input pattern for TMDB URLs
pub const TMDB_PATTERN: &str =
    r#"https:\/\/(?:www\.)?themoviedb\.org\/(?:tv|movie)\/(\d+)(?:-[a-zA-Z0-9\-]+)?(\/.*)?(?:\?.*)?"#;

/// HTML input pattern for AniList URLs
pub const ANILIST_PATTERN: &str = r#"https:\/\/anilist\.co\/anime\/(\d+)(?:\/.*)?"#;
