use std::time::Duration;

use anyhow::bail;
use reqwest::header::{HeaderValue, ACCEPT, CONTENT_TYPE, RETRY_AFTER};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use tracing::warn;

const FIXTURE_SEARCH_QUERY: &str = r#"
query ($id: Int, $page: Int, $perPage: Int, $search: String) {
  Page (page: $page, perPage: $perPage) {
    media (id: $id, search: $search, type: ANIME) {
      id
      title {
        romaji
        english
        native
      }
    }
  }
}
"#;

#[allow(clippy::declare_interior_mutable_const)]
const APPLICATION_JSON: HeaderValue = HeaderValue::from_static("application/json");

#[derive(Debug, Clone, Deserialize)]
struct GraphQlError {
    message: String,
}

#[derive(Debug, Clone, Deserialize)]
struct GraphQlResult<T> {
    data: Option<T>,
    #[serde(default)]
    errors: Vec<GraphQlError>,
}

impl<T> GraphQlResult<T> {
    fn into_result(self) -> anyhow::Result<T> {
        match self.data {
            None => {
                let msg = self
                    .error_string()
                    .unwrap_or_else(|| String::from("Unknown GraphQL error"));
                anyhow::bail!("{}", msg);
            }
            Some(s) => Ok(s),
        }
    }

    fn error_string(&self) -> Option<String> {
        if self.errors.is_empty() {
            None
        } else {
            let capacity: usize = self.errors.iter().map(|s| s.message.len() + 1).sum();
            let mut str =
                self.errors
                    .iter()
                    .map(|s| s.message.as_str())
                    .fold(String::with_capacity(capacity), |mut a, b| {
                        a.push_str(b);
                        a.push('\n');
                        a
                    });
            if str.ends_with('\n') {
                str.pop();
            }
            Some(str)
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
struct PageResult {
    #[serde(rename = "Page")]
    page: SearchQueryResult,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaTitle {
    pub romaji: String,
    pub english: Option<String>,
    pub native: Option<String>,
}

impl MediaTitle {
    pub fn new(romaji: String) -> Self {
        Self {
            romaji,
            english: None,
            native: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Media {
    pub id: u32,
    pub title: MediaTitle,
}

#[derive(Debug, Clone, Deserialize)]
struct SearchQueryResult {
    media: Vec<Media>,
}

#[derive(Serialize)]
struct GraphQlBody<'a, T> {
    query: &'a str,
    variables: T,
}

impl<'a, T> GraphQlBody<'a, T>
where
    T: Serialize,
{
    async fn send(&self, client: &reqwest::Client) -> reqwest::Result<reqwest::Response> {
        client
            .post("https://graphql.anilist.co")
            .header(CONTENT_TYPE, APPLICATION_JSON)
            .header(ACCEPT, APPLICATION_JSON)
            .json(&self)
            .send()
            .await
    }
}

/// Represents an empty variable list for [`send_request`].
#[derive(Debug, Copy, Clone, Deserialize, Serialize)]
pub struct EmptyVariable {}

/// A constant that represents an empty variable list for GraphQL requests
pub const NO_VARIABLES: EmptyVariable = EmptyVariable {};

/// Sends a GraphQL request, while respecting rate limits, to the anilist unauthenticated API.
pub async fn send_request<T>(client: &reqwest::Client, query: &str, variables: impl Serialize) -> anyhow::Result<T>
where
    T: DeserializeOwned,
{
    let body = GraphQlBody { query, variables };
    let mut response = body.send(client).await?;
    if response.status().is_server_error() {
        bail!("anilist returned a server error: {}", response.status())
    }
    if let Some(header) = response.headers().get(RETRY_AFTER) {
        if let Ok(seconds) = header.to_str() {
            if let Ok(seconds) = seconds.parse::<u64>() {
                warn!("rate limited by anilist API for {} seconds", seconds);
                tokio::time::sleep(Duration::from_secs(seconds)).await;
                response = body.send(client).await?;
            }
        }
    }

    response.json::<GraphQlResult<T>>().await?.into_result()
}

#[derive(Debug, Serialize)]
struct SearchQueryVariables {
    search: String,
    page: u8,
    #[serde(rename = "perPage")]
    per_page: u8,
}

#[derive(Debug, Serialize)]
struct SearchByIdQueryVariables {
    id: u32,
    page: u8,
    #[serde(rename = "perPage")]
    per_page: u8,
}

/// Searches the AniList API for the first page of media that matches the query
pub async fn search(client: &reqwest::Client, query: impl Into<String>) -> anyhow::Result<Vec<Media>> {
    Ok(send_request::<PageResult>(
        client,
        FIXTURE_SEARCH_QUERY,
        SearchQueryVariables {
            search: query.into(),
            page: 1,
            per_page: 50,
        },
    )
    .await?
    .page
    .media)
}

/// Searches the AniList API for the media that matches the given ID
pub async fn search_by_id(client: &reqwest::Client, id: u32) -> anyhow::Result<Option<Media>> {
    Ok(send_request::<PageResult>(
        client,
        FIXTURE_SEARCH_QUERY,
        SearchByIdQueryVariables {
            id,
            page: 1,
            per_page: 50,
        },
    )
    .await?
    .page
    .media
    .into_iter()
    .next())
}
