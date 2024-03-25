use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::PathBuf,
    sync::OnceLock,
};

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::key::SecretKey;
use crate::{cli::PROGRAM_NAME, discord::Webhook};

/// The server configuration.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    /// Whether the server is running a production build or not
    #[serde(default)]
    pub production: bool,
    /// Whether to run the server in Let's Encrypt's production directory.
    #[serde(default)]
    pub lets_encrypt_production: bool,
    /// The location that subtitles can be found under in the filesystem.
    ///
    /// Note that due to zipping being a significant use case, S3 storage is not used.
    pub subtitle_path: PathBuf,
    /// The contact emails for Let's Encrypt.
    ///
    /// Required for production use. Do not prefix this with e.g. `mailto`.
    #[serde(default)]
    pub contact_emails: Vec<String>,
    /// The domains that are registered to this server.
    ///
    /// These must *not* have any schemes.
    #[serde(default)]
    pub domains: Vec<String>,
    /// The Discord webhook URL for audit log announcements.
    #[serde(rename = "discord_webhook_url")]
    #[serde(default)]
    pub webhook: Option<Webhook>,
    /// The server IP and port configuration
    #[serde(default)]
    pub server: ServerConfig,
    /// The secret key used for all crypto related functionality in the server.
    ///
    /// Microbenching makes it evident that cloning this without an Arc is around ~4x faster.
    pub secret_key: SecretKey,
}

impl Config {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self {
            production: false,
            lets_encrypt_production: false,
            subtitle_path: std::env::current_dir().expect("could not get current working directory"),
            domains: Vec::new(),
            contact_emails: Vec::new(),
            webhook: None,
            server: ServerConfig::default(),
            secret_key: SecretKey::random()?,
        })
    }

    pub fn path() -> anyhow::Result<PathBuf> {
        let mut path = dirs::config_dir().context("could not find a config directory for the current user")?;
        path.push(PROGRAM_NAME);
        path.push("config.json");
        Ok(path)
    }

    pub fn load() -> anyhow::Result<Self> {
        let path = Self::path()?;
        if path.exists() {
            let file = std::fs::read_to_string(path).context("could not read config file")?;
            serde_json::from_str(&file).context("could not parse config file")
        } else {
            let config = Self::new()?;
            let parent = path.parent().unwrap();
            if !parent.exists() {
                std::fs::create_dir(parent).context("could not create config directory")?;
            }

            let file = std::fs::File::create(path).context("could not create config file")?;
            serde_json::to_writer_pretty(file, &config)?;
            Ok(config)
        }
    }

    /// Checks if the string is a valid configured hostname.
    ///
    /// This does *not* include the scheme.
    pub fn is_valid_host(&self, host: &str) -> bool {
        if !self.production {
            return host == "localhost";
        }

        self.domains.iter().any(|s| s == host)
    }

    pub fn canonical_url(&self) -> String {
        let scheme = if self.server.port == 443 { "https://" } else { "http://" };
        let domain = self.domains.first().map(|x| x.as_str()).unwrap_or("localhost");
        let mut url = String::with_capacity(8 + domain.len());
        url.push_str(scheme);
        url.push_str(domain);
        if domain == "localhost" {
            url.push(':');
            url.push_str(&self.server.port.to_string());
        }
        url
    }

    pub fn url_to(&self, url: impl Into<std::borrow::Cow<'static, str>>) -> String {
        let mut base = self.canonical_url();
        base.push_str(&url.into());
        base
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ServerConfig {
    #[serde(default = "default_ip")]
    pub ip: IpAddr,
    #[serde(default = "default_port")]
    pub port: u16,
}

fn default_ip() -> IpAddr {
    IpAddr::V4(Ipv4Addr::UNSPECIFIED)
}

fn default_port() -> u16 {
    9510
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            ip: default_ip(),
            port: default_port(),
        }
    }
}

impl ServerConfig {
    pub fn address(&self) -> SocketAddr {
        SocketAddr::from((self.ip, self.port))
    }
}

/// A global variable for the loaded config.
///
/// Currently mainly used for templates
pub static CONFIG: OnceLock<Config> = OnceLock::new();
