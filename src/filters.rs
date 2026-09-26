//! Askama filters

use std::fmt::Display;
use time::OffsetDateTime;

#[repr(transparent)]
pub struct OptionalDisplay<'a, T>(&'a Option<T>);

pub fn maybe_display<'a, T: Display>(
    opt: &'a Option<T>,
    _: &dyn askama::Values,
) -> askama::Result<OptionalDisplay<'a, T>> {
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

pub fn isoformat(dt: &OffsetDateTime, _: &dyn askama::Values) -> askama::Result<String> {
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

/// Formats the date as RFC 2822, which RSS uses: "Fri, 25 Sep 2026 12:00:00 +0000".
pub fn rfc2822(dt: &OffsetDateTime, _: &dyn askama::Values) -> askama::Result<String> {
    dt.to_offset(time::UtcOffset::UTC)
        .format(&time::format_description::well_known::Rfc2822)
        .map_err(askama::Error::custom)
}

/// Replaces each control character with a space. XML 1.0 does not allow most control characters,
/// or U+FFFE and U+FFFF, so one bad entry name could otherwise break a whole feed.
pub fn xml_text(s: impl AsRef<str>, _: &dyn askama::Values) -> askama::Result<String> {
    Ok(s.as_ref()
        .chars()
        .map(|c| {
            if c.is_control() || c == '\u{FFFE}' || c == '\u{FFFF}' {
                ' '
            } else {
                c
            }
        })
        .collect())
}

/// Returns a canonical URL to the given path
pub fn canonical_url(url: impl Display, _: &dyn askama::Values) -> askama::Result<String> {
    let path = url.to_string();
    let mut url = crate::CONFIG.get().unwrap().canonical_url();
    url.push_str(&path);
    Ok(url)
}

pub fn maybe_tmdb_url(opt: &Option<crate::tmdb::Id>, _: &dyn askama::Values) -> askama::Result<String> {
    Ok(opt.as_ref().map(|x| x.url()).unwrap_or_default())
}

pub fn maybe_anilist_url(opt: &Option<u32>, _: &dyn askama::Values) -> askama::Result<String> {
    Ok(opt
        .as_ref()
        .map(|x| format!("https://anilist.co/anime/{x}"))
        .unwrap_or_default())
}

pub fn markdown(s: impl AsRef<str>, _: &dyn askama::Values) -> askama::Result<askama::filters::Safe<String>> {
    let mut opts = comrak::Options::default();
    opts.extension.strikethrough = true;
    opts.extension.tagfilter = true;
    opts.extension.table = true;
    opts.extension.autolink = true;
    opts.render.escape = true;

    let s = comrak::markdown_to_html(s.as_ref(), &opts);
    Ok(askama::filters::Safe(s))
}

/// HTML input pattern for TMDB URLs
pub const TMDB_PATTERN: &str =
    r#"https:\/\/(?:www\.)?themoviedb\.org\/(?:tv|movie)\/(\d+)(?:-[a-zA-Z0-9\-]+)?(\/.*)?(?:\?.*)?"#;

/// HTML input pattern for AniList URLs
pub const ANILIST_PATTERN: &str = r#"https:\/\/anilist\.co\/anime\/(\d+)(?:\/.*)?"#;
