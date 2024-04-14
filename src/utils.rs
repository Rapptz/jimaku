use std::{path::PathBuf, str::FromStr, sync::OnceLock};

use regex::Regex;
use serde::{de::DeserializeOwned, Deserialize, Deserializer};

/// The maximum amount of bytes an upload can have, in bytes.
pub const MAX_UPLOAD_SIZE: u64 = 1024 * 1024 * 16;
pub const MAX_BODY_SIZE: usize = MAX_UPLOAD_SIZE as usize;

/// This is mainly for use in forms.
///
/// Since forms always receive strings, this uses FromStr for the internal type.
pub fn generic_empty_string_is_none<'de, D, T>(de: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: FromStr,
    T::Err: std::error::Error,
{
    let opt = Option::<String>::deserialize(de)?;
    let opt = opt.as_deref();
    match opt {
        None | Some("") => Ok(None),
        Some(s) => s.parse::<T>().map(Some).map_err(serde::de::Error::custom),
    }
}

pub fn empty_string_is_none<'de, D: Deserializer<'de>>(de: D) -> Result<Option<String>, D::Error> {
    let opt: Option<String> = Option::deserialize(de)?;
    Ok(opt.filter(|s| !s.is_empty()))
}

pub fn inner_json<'de, D, T>(de: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned,
{
    let s = crate::borrowed::MaybeBorrowedString::deserialize(de)?;
    serde_json::from_str(&s).map_err(serde::de::Error::custom)
}

fn anilist_id_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| Regex::new(r#"https?://anilist.co/(?:anime|manga)/([0-9]+)(?:/.*)?"#).unwrap())
}

pub fn get_anilist_id(url: &str) -> Option<u32> {
    anilist_id_regex().captures(url)?.get(1)?.as_str().parse().ok()
}

pub fn is_over_length<T: AsRef<str>>(opt: &Option<T>, length: usize) -> bool {
    opt.as_ref().map(|x| x.as_ref().len() >= length).unwrap_or_default()
}

pub fn join_iter<T: ToString>(sep: impl AsRef<str>, mut iter: impl Iterator<Item = T>) -> String {
    let mut buffer = String::new();
    if let Some(item) = iter.next() {
        buffer.push_str(&item.to_string());
    }
    for item in iter {
        buffer.push_str(sep.as_ref());
        buffer.push_str(&item.to_string());
    }
    buffer
}

/// Returns the directory where logs are stored
pub fn logs_directory() -> PathBuf {
    dirs::state_dir()
        .map(|p| p.join(crate::PROGRAM_NAME))
        .unwrap_or_else(|| PathBuf::from("./logs"))
}

/// This is mainly used for serde defaults
pub const fn default_true() -> bool {
    true
}

pub const fn is_false(s: &bool) -> bool {
    !*s
}

pub mod base64_bytes {
    use base64::{prelude::BASE64_STANDARD, Engine};
    use serde::{Deserialize, Deserializer, Serializer};

    use crate::borrowed::MaybeBorrowedString;

    pub fn serialize<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        let b64 = BASE64_STANDARD.encode(bytes);
        serializer.serialize_str(&b64)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        let s = MaybeBorrowedString::deserialize(deserializer)?;
        BASE64_STANDARD.decode(s.as_bytes()).map_err(serde::de::Error::custom)
    }
}
