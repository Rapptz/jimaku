//! Support for Discord's webhook

use std::{borrow::Cow, str::FromStr};

use regex::Regex;
use serde::{ser::SerializeMap, Deserialize, Deserializer, Serialize};
use std::sync::OnceLock;

use crate::models::Account;

#[derive(Debug, Clone, Copy)]
pub struct InvalidWebhookUrl;

impl std::fmt::Display for InvalidWebhookUrl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("invalid discord webhook URL")
    }
}

fn discord_webhook_url_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| Regex::new(
        r#"^https://(?:canary\.|ptb\.)?discord(?:app)?\.com/api/webhooks/(?P<id>[0-9]{17,20})/(?P<token>[A-Za-z0-9\.\-\_]{60,})"#
    ).unwrap())
}

/// A Discord Webhook to send to.
#[derive(Debug, Clone)]
pub struct Webhook {
    url: reqwest::Url,
}

impl<'de> Deserialize<'de> for Webhook {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        FromStr::from_str(&s).map_err(|_| serde::de::Error::custom("invalid discord webhook URL"))
    }
}

impl Serialize for Webhook {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.url.as_str())
    }
}

impl FromStr for Webhook {
    type Err = InvalidWebhookUrl;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if !discord_webhook_url_regex().is_match(s) {
            return Err(InvalidWebhookUrl);
        }
        Ok(Webhook {
            url: s.parse().map_err(|_| InvalidWebhookUrl)?,
        })
    }
}

pub struct PreparedWebhookRequest<T> {
    json: T,
    url: reqwest::Url,
}

impl Webhook {
    /// Prepare a request for sending with the given JSON value.
    pub fn prepare<T: Serialize>(&self, json: T) -> PreparedWebhookRequest<T> {
        PreparedWebhookRequest {
            json,
            url: self.url.clone(),
        }
    }
}

impl<T: Serialize> PreparedWebhookRequest<T> {
    /// Sends the request to Discord
    pub async fn send(self, client: &reqwest::Client) -> Option<reqwest::StatusCode> {
        client
            .post(self.url)
            .json(&self.json)
            .header(
                reqwest::header::USER_AGENT,
                reqwest::header::HeaderValue::from_static("DiscordBot (https://github.com/Rapptz/jimaku, 0.1)"),
            )
            .send()
            .await
            .map(|r| r.status())
            .ok()
    }
}

#[derive(Serialize)]
struct AlertField {
    name: String,
    value: String,
    inline: bool,
}

#[derive(Serialize)]
struct AlertAuthor {
    name: String,
    url: String,
}

/// An actual structured alert to send to Discord.
///
/// This is basically just an embed builder.
pub struct Alert {
    title: Cow<'static, str>,
    url: Option<Cow<'static, str>>,
    author: Option<AlertAuthor>,
    fields: Vec<AlertField>,
    description: Option<Cow<'static, str>>,
    color: u32,
    username: Cow<'static, str>,
}

impl Alert {
    /// The alert color for info.
    pub const INFO: u32 = 0x1c7379;
    /// The alert color for success.
    pub const SUCCESS: u32 = 0x1c7951;
    /// The alert color for error.
    pub const ERROR: u32 = 0xa4392f;

    const fn new_with(color: u32, title: Cow<'static, str>) -> Self {
        Self {
            title,
            url: None,
            author: None,
            fields: Vec::new(),
            description: None,
            color,
            username: Cow::Borrowed("Jimaku"),
        }
    }

    pub fn info(title: impl Into<Cow<'static, str>>) -> Self {
        Self::new_with(Self::INFO, title.into())
    }

    pub fn success(title: impl Into<Cow<'static, str>>) -> Self {
        Self::new_with(Self::SUCCESS, title.into())
    }

    pub fn error(title: impl Into<Cow<'static, str>>) -> Self {
        Self::new_with(Self::ERROR, title.into())
    }

    pub fn color(mut self, color: u32) -> Self {
        self.color = color;
        self
    }

    pub fn external_url(mut self, url: impl Into<Cow<'static, str>>) -> Self {
        self.url = Some(url.into());
        self
    }

    pub fn url(mut self, url: impl Into<Cow<'static, str>>) -> Self {
        let url = crate::CONFIG.get().unwrap().url_to(url);
        self.url = Some(Cow::Owned(url));
        self
    }

    pub fn description(mut self, description: impl Into<Cow<'static, str>>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn name(mut self, username: impl Into<Cow<'static, str>>) -> Self {
        self.username = username.into();
        self
    }

    pub fn field(mut self, name: impl Into<String>, value: impl ToString) -> Self {
        self.fields.push(AlertField {
            name: name.into(),
            value: value.to_string(),
            inline: false,
        });
        self
    }

    pub fn inline_field(mut self, name: impl Into<String>, value: impl ToString) -> Self {
        self.fields.push(AlertField {
            name: name.into(),
            value: value.to_string(),
            inline: true,
        });
        self
    }

    pub fn empty_inline_field(mut self) -> Self {
        self.fields.push(AlertField {
            name: String::new(),
            value: String::new(),
            inline: true,
        });
        self
    }

    pub fn account(mut self, account: Account) -> Self {
        let mut url = crate::CONFIG.get().unwrap().canonical_url();
        url.push_str("/user/");
        url.push_str(&account.name);
        self.author = Some(AlertAuthor {
            name: account.name,
            url,
        });
        self
    }
}

struct InnerEmbed<'a> {
    embed: &'a Alert,
}

impl<'a> Serialize for InnerEmbed<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("title", &self.embed.title)?;
        if let Some(url) = self.embed.url.as_deref() {
            map.serialize_entry("url", url)?;
        }
        if let Some(description) = self.embed.description.as_deref() {
            map.serialize_entry("description", description)?;
        }
        if let Some(author) = &self.embed.author {
            map.serialize_entry("author", author)?;
        }
        map.serialize_entry("color", &self.embed.color)?;
        map.serialize_entry("fields", &self.embed.fields)?;
        map.end()
    }
}

impl Serialize for Alert {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("username", &self.username)?;
        map.serialize_entry("embeds", &[InnerEmbed { embed: self }])?;
        map.end()
    }
}
