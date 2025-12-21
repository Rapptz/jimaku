use base64::{prelude::BASE64_URL_SAFE, Engine};
use hyper::header::{AUTHORIZATION, CONTENT_LENGTH};
use reqwest::Body;
use serde::{Deserialize, Deserializer, Serialize};
use tokio_util::codec::{BytesCodec, FramedRead};

/// Represents an account ID for Buzzheavier
///
/// This is a type-safe wrapper around it that allows you to do operations with it.
#[derive(Debug, Clone)]
pub struct Buzzheavier {
    account_id: String,
}

impl<'de> Deserialize<'de> for Buzzheavier {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let account_id = String::deserialize(deserializer)?;
        Ok(Self { account_id })
    }
}

impl Serialize for Buzzheavier {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.account_id)
    }
}

pin_project_lite::pin_project! {
    /// A wrapped tokio::fs::File that also prints read progrss to CLI
    struct TrackingFile {
        #[pin]
        file: tokio::fs::File,
        progress: usize,
        total: usize,
    }
}

// impl TrackingFile {
//     async fn new(file: tokio::fs::File) -> Body {
//         let total = file.metadata().await.map(|m| m.len() as usize).unwrap_or_default();
//         let tracked = Self {
//             file,
//             progress: 0,
//             total,
//         };
//         let stream = FramedRead::new(tracked, BytesCodec::new());
//         Body::wrap_stream(stream)
//     }
// }

// impl AsyncRead for TrackingFile {
//     fn poll_read(
//         self: std::pin::Pin<&mut Self>,
//         cx: &mut std::task::Context<'_>,
//         buf: &mut tokio::io::ReadBuf<'_>,
//     ) -> std::task::Poll<std::io::Result<()>> {
//         let buf_size = self.file.max_buf_size();
//         let me = self.project();
//         let poll = me.file.poll_read(cx, buf);
//         if poll.is_ready() {
//             *me.progress += std::cmp::min(buf.remaining(), buf_size);
//         }
//         crate::cli::print_progress_bar(*me.progress, *me.total, Some("Uploading"));
//         eprintln!(" bytes\n");
//         poll
//     }
// }

fn file_to_body(file: tokio::fs::File) -> Body {
    let stream = FramedRead::new(file, BytesCodec::new());
    Body::wrap_stream(stream)
}

#[derive(Deserialize)]
struct ResponseData {
    id: String,
}

#[derive(Deserialize)]
struct UploadResponse {
    data: ResponseData,
}

impl Buzzheavier {
    async fn get_root_directory(&self, client: &reqwest::Client) -> anyhow::Result<String> {
        let response = client
            .get("https://buzzheavier.com/api/fs")
            .header(AUTHORIZATION, format!("Bearer {}", self.account_id))
            .send()
            .await?
            .error_for_status()?;

        let json: UploadResponse = response.json().await?;
        Ok(json.data.id)
    }

    pub async fn upload(&self, client: &reqwest::Client, file: tokio::fs::File) -> anyhow::Result<String> {
        let content_length = file.metadata().await?.len() as usize;
        let directory = self.get_root_directory(client).await?;
        let note =
            BASE64_URL_SAFE.encode("Unpacking this ZIP file requires a 7zip >= 24.01 or support for ZSTD ZIP files");
        let response = client
            .put(format!(
                "https://w.buzzheavier.com/{directory}/jimaku_backup.zip?note={note}"
            ))
            .body(file_to_body(file))
            .header(AUTHORIZATION, format!("Bearer {}", self.account_id))
            .header(CONTENT_LENGTH, content_length.to_string())
            .send()
            .await?
            .error_for_status()?;

        let json: UploadResponse = response.json().await?;
        let mut url = String::from("https://buzzheavier.com/");
        url.push_str(&json.data.id);
        Ok(url)
    }
}
