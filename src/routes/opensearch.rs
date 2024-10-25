use axum::{
    extract::{Query, State},
    response::{IntoResponse, Redirect, Response},
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};

use crate::AppState;

fn generate_opensearch_xml(state: &AppState, anime: bool) -> Response {
    let base_url = state.config().canonical_url();
    let suggestions_url = format!("{base_url}/opensearch/suggest?anime={anime}&amp;query={{searchTerms}}",);

    let search_kind = if anime { "Anime" } else { "Drama" };

    let xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<OpenSearchDescription xmlns="http://a9.com/-/spec/opensearch/1.1/">
    <ShortName>Jimaku {search_kind} Search</ShortName>
    <Description>Jimaku: Japanese Subtitles</Description>
    <Image height="48" width="48" type="image/x-icon">{base_url}/favicon.ico</Image>
    <Url type="text/html" method="get" template="{base_url}/opensearch/redirect?anime={anime}&amp;query={{searchTerms}}" />
    <Url type="application/x-suggestions+json" template="{suggestions_url}" />
</OpenSearchDescription>
"#
    );

    ([(axum::http::header::CONTENT_TYPE, "application/xml")], xml).into_response()
}

async fn opensearch_anime(State(state): State<AppState>) -> Response {
    generate_opensearch_xml(&state, true)
}

async fn opensearch_dramas(State(state): State<AppState>) -> Response {
    generate_opensearch_xml(&state, false)
}

// According to this:
// https://github.com/dewitt/opensearch/blob/master/mediawiki/Specifications/OpenSearch/Extensions/Suggestions/1.1/Draft%201.wiki#json-formatted-search-suggestion-responses
// The suggestions have to be a four element array containing arrays of completions, descriptions, and the actual URL
// However, in reality all of these are essentially unused except the descriptions on Firefox
//
// Since only a simple string can be used, a redirect service has to be used as a middle-man to power
// the "search engine"
struct Suggestion(isize, String);

// Rather inefficient due to mutating a dynamically allocated type and then returning it
// But hopefully the Rust compiler knows my intention here and elides the allocation
fn truncate_string(mut buf: String, count: usize) -> String {
    if buf.len() > count {
        buf.truncate(count - 3);
        buf.push_str("...");
    }
    buf
}

#[derive(Deserialize)]
struct SuggestionQuery {
    query: String,
    #[serde(default)]
    anime: bool,
}

impl From<SuggestionQuery> for Redirect {
    fn from(value: SuggestionQuery) -> Self {
        if value.anime {
            Self::to(&format!("/?query={}", value.query))
        } else {
            Self::to(&format!("/dramas?query={}", value.query))
        }
    }
}

// The endpoint expects [query, [titles...], [], []]
#[derive(Serialize)]
struct SuggestionResult(String, Vec<String>, Vec<String>, Vec<String>);

async fn get_suggestions(state: &AppState, anime: bool, query: String) -> SuggestionResult {
    // This uses the API "backend" to do the search filter, so it's somewhat consistent...
    // The client uses a different algorithm though so it's not 1:1
    let search = super::SearchQuery {
        query: Some(query),
        anime,
        ..Default::default()
    };

    let mut suggestions = state
        .directory_entries()
        .await
        .iter()
        .filter_map(|s| {
            search
                .apply(s)
                .map(|score| Suggestion(score, truncate_string(format!("{}: {}", s.id, s.name), 128)))
        })
        .collect::<Vec<_>>();

    suggestions.sort_by_key(|s| std::cmp::Reverse(s.0));
    suggestions.truncate(10);

    let titles = suggestions.into_iter().map(|s| s.1).collect();

    SuggestionResult(search.query.unwrap(), titles, Vec::new(), Vec::new())
}

async fn suggest_entries(
    State(state): State<AppState>,
    Query(params): Query<SuggestionQuery>,
) -> Json<SuggestionResult> {
    Json(get_suggestions(&state, params.anime, params.query).await)
}

async fn redirect_search(Query(params): Query<SuggestionQuery>) -> Redirect {
    match params.query.split_once(':') {
        Some((left, _)) => match left.parse::<i64>() {
            Ok(id) => Redirect::to(&format!("/entry/{id}")),
            _ => params.into(),
        },
        None => params.into(),
    }
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/opensearch/anime.xml", get(opensearch_anime))
        .route("/opensearch/dramas.xml", get(opensearch_dramas))
        .route("/opensearch/suggest", get(suggest_entries))
        .route("/opensearch/redirect", get(redirect_search))
}
