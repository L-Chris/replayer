//! Portable metadata only. Local paths, caches and transport credentials never enter a document.
pub mod crypto;
pub mod model;
pub mod portable;
#[cfg(test)]
mod tests;
pub mod webdav;
use anyhow::{Result, ensure};
pub use model::{Document, Journal};
use serde::{Deserialize, Serialize};

pub fn journal_file(
    directory: &std::path::Path,
    account: &str,
    device: &str,
) -> Result<std::path::PathBuf> {
    ensure!(
        account.len() == 64 && account.bytes().all(|c| c.is_ascii_hexdigit()),
        "Invalid sync account"
    );
    ensure!(uuid::Uuid::parse_str(device).is_ok(), "Invalid sync device");
    Ok(directory.join(account).join(format!("{device}.json")))
}

pub async fn bounded<T>(
    future: impl std::future::Future<Output = Result<T>>,
    timeout: std::time::Duration,
) -> Result<T> {
    tokio::time::timeout(timeout, future)
        .await
        .map_err(|_| anyhow::anyhow!("同步超时 / Sync timed out"))?
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Provider {
    CstCloud,
    Jianguoyun,
    InfiniCloud,
    Koofr,
    HiDrive,
    Yandex,
    #[default]
    Custom,
}
impl Provider {
    pub const ALL: [Self; 7] = [
        Self::CstCloud,
        Self::Jianguoyun,
        Self::InfiniCloud,
        Self::Koofr,
        Self::HiDrive,
        Self::Yandex,
        Self::Custom,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::CstCloud => "中国科技云",
            Self::Jianguoyun => "坚果云",
            Self::InfiniCloud => "InfiniCLOUD",
            Self::Koofr => "Koofr",
            Self::HiDrive => "HiDrive",
            Self::Yandex => "Yandex Disk",
            Self::Custom => "自定义 / Custom",
        }
    }
    pub fn url(self) -> &'static str {
        match self {
            Self::CstCloud => "https://data.cstcloud.cn/dav",
            Self::Jianguoyun => "https://dav.jianguoyun.com/dav",
            Self::InfiniCloud => "https://higa.teracloud.jp/dav",
            Self::Koofr => "https://app.koofr.net/dav/Koofr",
            Self::HiDrive => "https://webdav.hidrive.strato.com",
            Self::Yandex => "https://webdav.yandex.com",
            Self::Custom => "",
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Settings {
    pub enabled: bool,
    pub provider: Provider,
    pub base_url: String,
    pub username: String,
    pub device_id: String,
    pub device_name: String,
    pub sync_keys: bool,
    #[serde(skip)]
    pub password: String,
    #[serde(skip)]
    pub passphrase: String,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: Provider::Custom,
            base_url: String::new(),
            username: String::new(),
            device_id: uuid::Uuid::new_v4().to_string(),
            device_name: std::env::var("COMPUTERNAME").unwrap_or_else(|_| "Desktop".into()),
            sync_keys: false,
            password: String::new(),
            passphrase: String::new(),
        }
    }
}
impl Settings {
    pub fn validate(&self) -> Result<()> {
        let url = reqwest::Url::parse(self.base_url.trim())
            .map_err(|_| anyhow::anyhow!("WebDAV 地址无效 / Invalid WebDAV URL"))?;
        let local = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
        ensure!(
            (url.scheme() == "https" || url.scheme() == "http" && local)
                && url.host_str().is_some()
                && url.username().is_empty()
                && url.password().is_none()
                && url.query().is_none()
                && url.fragment().is_none(),
            "请使用 HTTPS WebDAV 地址 / Use an HTTPS WebDAV URL"
        );
        ensure!(
            !self.username.trim().is_empty() && !self.password.is_empty(),
            "请填写 WebDAV 用户名和密码 / Enter WebDAV credentials"
        );
        ensure!(
            uuid::Uuid::parse_str(&self.device_id).is_ok(),
            "设备标识无效 / Invalid device ID"
        );
        ensure!(
            !self.sync_keys || self.passphrase.chars().count() >= 12,
            "同步口令至少需要 12 个字符 / Sync passphrase needs at least 12 characters"
        );
        Ok(())
    }
    pub fn account(&self) -> String {
        crate::comics::digest(
            format!(
                "{}\0{}",
                self.base_url.trim().trim_end_matches('/'),
                self.username.trim()
            )
            .as_bytes(),
        )
    }
}
