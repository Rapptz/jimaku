//! A parser for the anime-relations file.
//!
//! The anime relation file has the following documentation:
//!
//! ```no_rust
//! Rules are sorted alphabetically by anime title. Rule syntax is:
//!
//!   10001|10002|10003:14-26 -> 20001|20002|20003:1-13!
//!   └─┬─┘ └─┬─┘ └─┬─┘ └─┬─┘    └─┬─┘ └─┬─┘ └─┬─┘ └─┬─┘
//!     1     2     3     4        1     2     3     4
//!
//!   (1) MyAnimeList ID
//!       <https://myanimelist.net/anime/{id}/{title}>
//!   (2) Kitsu ID
//!       <https://kitsu.io/api/edge/anime?filter[text]={title}>
//!   (3) AniList ID
//!       <https://anilist.co/anime/{id}/{title}>
//!   (4) Episode number or range
//!
//!   - "?" is used for unknown values.
//!   - "~" is used to repeat the source ID.
//!   - "!" suffix is shorthand for creating a new rule where destination ID is
//!     redirected to itself.
//! ```
//!
//! The only IDs we're interested in are the 3rd ones, the AniList ID.

use std::{collections::HashMap, str::FromStr};

use serde::Serialize;
use time::format_description::well_known::Iso8601;

pub const RELATIONS_URL: &str = "https://raw.githubusercontent.com/erengy/anime-relations/master/anime-relations.txt";

/// The relation ID that you can look up relations by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum RelationId {
    /// An external AniList ID
    AniList(u32),
    /// An unknown ID, denoted by ?
    Unknown,
    /// A repeated ID, denoted by ~
    Repeated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase", tag = "type")]
pub enum Range {
    Number { value: u16 },
    From { value: u16 },
    Inclusive { begin: u16, end: u16 },
}

impl Range {
    pub const fn begin(&self) -> u16 {
        match self {
            Range::Number { value } => *value,
            Range::From { value } => *value,
            Range::Inclusive { begin, .. } => *begin,
        }
    }

    pub const fn end(&self) -> u16 {
        match self {
            Range::Number { value } => *value,
            Range::From { .. } => u16::MAX,
            Range::Inclusive { end, .. } => *end,
        }
    }

    pub const fn contains(&self, number: u16) -> bool {
        match *self {
            Range::Number { value } => value == number,
            Range::From { value } => number >= value,
            Range::Inclusive { begin, end } => number >= begin && number <= end,
        }
    }

    #[must_use]
    pub const fn is_number(&self) -> bool {
        matches!(self, Self::Number { .. })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]

struct RuleComponent {
    id: RelationId,
    range: Range,
}

#[derive(Debug, Clone, Copy)]
struct ParseRuleComponentError;

impl std::fmt::Display for ParseRuleComponentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("invalid relation rule found")
    }
}

impl std::error::Error for ParseRuleComponentError {}

impl From<std::num::ParseIntError> for ParseRuleComponentError {
    fn from(_: std::num::ParseIntError) -> Self {
        Self
    }
}

impl FromStr for RuleComponent {
    type Err = ParseRuleComponentError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (_, s) = s.rsplit_once('|').ok_or(ParseRuleComponentError)?;
        let (id, episodes) = s.split_once(':').ok_or(ParseRuleComponentError)?;
        let id = match id {
            "?" => RelationId::Unknown,
            "~" => RelationId::Repeated,
            x => x.parse().map(RelationId::AniList)?,
        };

        // Episodes are essentially \d+(?:-(?:\d+|\?))?
        let range = match episodes.split_once('-') {
            Some((left, right)) => {
                let value = left.parse()?;
                if right == "?" {
                    Range::From { value }
                } else {
                    Range::Inclusive {
                        begin: value,
                        end: right.parse()?,
                    }
                }
            }
            None => Range::Number {
                value: episodes.parse()?,
            },
        };

        Ok(Self { id, range })
    }
}

/// The actual relation rule that maps from one source range to another destination range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Rule {
    /// The ID that this rule applies to
    #[serde(rename = "anilist_id")]
    pub id: u32,
    /// The source range of episodes that is being applied to
    pub source: Range,
    /// The destination range of episodes that is being applied to
    pub destination: Range,
}

type Relation = Vec<Rule>;

fn find_destination(rules: &[Rule], episode: u16) -> Option<(u32, u16)> {
    for rule in rules {
        if let Some(distance) = episode.checked_sub(rule.source.begin()) {
            if rule.source.end().checked_sub(episode).is_some() {
                let mut found = rule.destination.begin();
                if !rule.destination.is_number() {
                    found = found.saturating_add(distance);
                }
                if found <= rule.destination.end() {
                    return Some((rule.id, found));
                }
            }
        }
    }
    None
}

/// A mapping of an ID to a relation
#[derive(Debug, Clone, Serialize)]
pub struct Relations {
    pub last_modified: time::Date,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: time::OffsetDateTime,
    #[serde(rename = "relations")]
    data: HashMap<u32, Relation>,
}

impl Relations {
    pub fn new(s: &str) -> anyhow::Result<Self> {
        let mut data: HashMap<u32, Vec<Rule>> = HashMap::new();
        let mut last_modified = time::Date::MIN;

        for line in s.lines() {
            // Lines we're interested in are either `- last_modified` or `- A|B|C:D -> A|B|C:D`
            let Some(line) = line.strip_prefix("- ") else {
                continue;
            };

            if let Some(("last_modified", date)) = line.split_once(": ") {
                last_modified = time::Date::parse(date, &Iso8601::DATE)?;
                continue;
            }

            if let Some((left, right)) = line.split_once(" -> ") {
                let (right, is_redirected) = match right.strip_suffix('!') {
                    Some(l) => (l, true),
                    None => (right, false),
                };

                let left = left.parse::<RuleComponent>()?;
                let RelationId::AniList(source_id) = left.id else {
                    continue;
                };

                let right = right.parse::<RuleComponent>()?;
                let destination_id = match right.id {
                    RelationId::AniList(id) => id,
                    _ => source_id,
                };

                let rule = Rule {
                    id: destination_id,
                    source: left.range,
                    destination: right.range,
                };

                data.entry(source_id).or_default().push(rule);
                if is_redirected {
                    data.entry(destination_id).or_default().push(rule);
                }
            }
        }

        Ok(Self {
            last_modified,
            data,
            created_at: time::OffsetDateTime::now_utc(),
        })
    }

    /// Loads the relation data from the GitHub URL
    pub async fn load(client: &reqwest::Client) -> anyhow::Result<Self> {
        let data = client.get(RELATIONS_URL).send().await?.text().await?;
        Self::new(&data)
    }

    /// Finds the relation of a given ID and episode
    ///
    /// The first element of the tuple is the destination AniList ID
    /// and the second element of the tuple is the resulting episode.
    pub fn find(&self, anilist_id: u32, episode: u16) -> Option<(u32, u16)> {
        let rules = self.data.get(&anilist_id)?;
        find_destination(rules, episode)
    }
}

impl Default for Relations {
    fn default() -> Self {
        Self {
            last_modified: time::OffsetDateTime::UNIX_EPOCH.date(),
            created_at: time::OffsetDateTime::UNIX_EPOCH,
            data: HashMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_relations_lookup() {
        let client = reqwest::Client::new();
        let relations = Relations::load(&client).await.unwrap();

        println!("{:?}", relations.data.len());
        println!("{:?}", relations.find(153152, 13));
    }
}
