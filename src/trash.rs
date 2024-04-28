//! A minimal re-implementation of the FreeDesktop.org trash spec.
//!
//! This differs from it though in that it's entirely managed by this program
//! so other programs can't really mess with it. This means there's more
//! metadata specific to this server that it can leverage.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use anyhow::Context;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

/// A trash can that sort of implements the FreeDesktop.org trash spec
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trash {
    files: PathBuf,
    info: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrashInfo {
    pub path: PathBuf,
    #[serde(with = "time::serde::timestamp")]
    pub deletion_date: OffsetDateTime,
    pub size: u64,
    pub entry_id: i64,
    #[serde(default)]
    pub reason: Option<String>,
}

pub type TrashListing = HashMap<PathBuf, TrashInfo>;

fn create_directory(path: &Path) -> anyhow::Result<()> {
    if path.exists() {
        return Ok(());
    }

    if let Err(e) = std::fs::create_dir(path) {
        if e.kind() != std::io::ErrorKind::AlreadyExists {
            return Err(e).with_context(|| format!("could not create directory {}", path.display()));
        }
    }
    Ok(())
}

impl Trash {
    pub fn new() -> anyhow::Result<Self> {
        let mut base = dirs::data_dir().context("could not find a data directory for current user")?;
        base.push(crate::PROGRAM_NAME);
        create_directory(&base)?;
        let files = base.join("files");
        let info = base.join("info");
        create_directory(&files)?;
        create_directory(&info)?;
        Ok(Self { files, info })
    }

    pub fn files_path(&self) -> &Path {
        &self.files
    }

    pub fn info_path(&self) -> &Path {
        &self.info
    }

    /// Puts the file in the trash.
    ///
    /// This does not trash directories.
    pub async fn put(&self, path: PathBuf, entry_id: i64, reason: Option<String>) -> std::io::Result<()> {
        let (new_location, info_location) = match path.file_name().and_then(|s| s.to_str()) {
            Some(filename) => {
                let filename = format!("{entry_id}_{filename}");
                (self.files.join(&filename), self.info.join(&filename))
            }
            None => return Err(std::io::Error::other("path has no filename")),
        };

        let info = TrashInfo {
            path: path.canonicalize()?,
            deletion_date: OffsetDateTime::now_utc(),
            size: path.metadata()?.len(),
            entry_id,
            reason,
        };

        tokio::task::spawn_blocking(move || -> std::io::Result<()> {
            let info_file = std::fs::File::create(info_location)?;
            serde_json::to_writer(info_file, &info).map_err(std::io::Error::other)?;
            std::fs::rename(path, new_location)
        })
        .await
        .map_err(std::io::Error::other)?
    }

    /// Returns everything that is in the trash
    pub async fn list(&self) -> std::io::Result<TrashListing> {
        let reader = self.info.read_dir()?;
        tokio::task::spawn_blocking(move || {
            let mut map = HashMap::new();
            for file in reader {
                let Ok(entry) = file else { continue };
                let path = entry.path();
                let Some(filename) = path.file_name().map(|x| Path::new(x).to_path_buf()) else {
                    continue;
                };
                let json = std::fs::read_to_string(&path)?;
                let value: TrashInfo = serde_json::from_str(&json).map_err(std::io::Error::other)?;
                map.insert(filename, value);
            }
            Ok(map)
        })
        .await
        .map_err(std::io::Error::other)?
    }

    /// Permanently deletes the file
    ///
    /// The path must be the filename of the deleted file, e.g. `foo.zip` if `foo.zip` is in the trash.
    /// This is equivalent to the key in the return value for [`Self::list`].
    pub async fn delete(&self, filename: PathBuf) -> std::io::Result<()> {
        let trash_path = self.files.join(&filename);
        let info_path = self.info.join(&filename);

        tokio::task::spawn_blocking(move || {
            std::fs::remove_file(info_path)?;
            std::fs::remove_file(trash_path)
        })
        .await
        .map_err(std::io::Error::other)?
    }

    /// Restores the file
    pub async fn restore(&self, filename: PathBuf) -> std::io::Result<()> {
        let trash_path = self.files.join(&filename);
        let info_path = self.info.join(&filename);
        tokio::task::spawn_blocking(move || {
            let json = std::fs::read_to_string(&info_path)?;
            let value: TrashInfo = serde_json::from_str(&json).map_err(std::io::Error::other)?;
            std::fs::rename(trash_path, value.path)?;
            std::fs::remove_file(info_path)
        })
        .await
        .map_err(std::io::Error::other)?
    }

    /// Returns a URL to the trash item
    pub fn url_to(&self, filename: &Path) -> String {
        format!(
            "/admin/trash/download/{}",
            percent_encoding::percent_encode(filename.as_os_str().as_encoded_bytes(), crate::utils::FRAGMENT)
        )
    }
}
