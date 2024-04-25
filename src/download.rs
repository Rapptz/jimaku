use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use std::path::{Component, Path, PathBuf};

/// Ensures the path is valid and does not traverse outside of the base path.
pub fn validate_path(base: &Path, requested: &str) -> Option<PathBuf> {
    let path = Path::new(requested.trim_start_matches('/'));
    let mut path_to_file = base.to_path_buf();
    for component in path.components() {
        match component {
            Component::Normal(cmp) => {
                if Path::new(&cmp).components().all(|c| matches!(c, Component::Normal(_))) {
                    path_to_file.push(cmp);
                } else {
                    return None;
                }
            }
            Component::CurDir => {}
            Component::Prefix(_) | Component::RootDir | Component::ParentDir => return None,
        }
    }
    Some(path_to_file)
}

/// A download response
pub enum DownloadResponse {
    /// The file that is being downloaded
    File(Response),
    /// The file is not found
    NotFound,
}

impl IntoResponse for DownloadResponse {
    fn into_response(self) -> Response {
        match self {
            DownloadResponse::File(r) => r,
            DownloadResponse::NotFound => StatusCode::NOT_FOUND.into_response(),
        }
    }
}
