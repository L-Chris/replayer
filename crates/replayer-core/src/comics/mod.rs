pub mod cache;
pub mod library;
pub mod source;
#[cfg(test)]
mod tests;

use serde::{Deserialize, Serialize};

pub fn digest(value: &[u8]) -> String {
    crate::crypto::sha256_hex(value)
}
pub fn root() -> std::path::PathBuf {
    crate::paths::root().join("comics")
}
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Book {
    pub provider: String,
    #[serde(rename = "siteId")]
    pub site: String,
    #[serde(rename = "siteName")]
    pub site_name: String,
    pub title: String,
    pub author: String,
    pub description: String,
    pub status: String,
    #[serde(rename = "coverUrl")]
    pub cover: String,
    #[serde(rename = "detailUrl", alias = "sourceUrl")]
    pub url: String,
}
impl Book {
    pub fn key(&self) -> String {
        digest(format!("{}\0{}\0{}", self.provider, self.site, self.url).as_bytes())
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Chapter {
    pub id: String,
    pub name: String,
    pub url: String,
    pub order: i64,
    pub kind: ChapterKind,
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum ChapterKind {
    #[default]
    Chapter,
    Volume,
}
impl Chapter {
    pub fn key(&self) -> &str {
        if self.id.is_empty() {
            &self.url
        } else {
            &self.id
        }
    }
}
#[derive(Clone, Debug)]
pub struct Detail {
    pub book: Book,
    pub chapters: Vec<Chapter>,
}
#[derive(Clone, Debug, Deserialize)]
pub struct Site {
    #[serde(rename = "siteId")]
    pub id: String,
    #[serde(rename = "siteName")]
    pub name: String,
}
#[derive(Clone, Debug)]
pub struct SearchGroup {
    pub site: Site,
    pub books: Vec<Book>,
    pub error: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum ReadingMode {
    #[default]
    #[serde(alias = "LeftToRight", alias = "RightToLeft")]
    Vertical,
}
