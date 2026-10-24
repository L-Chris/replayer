//! Provider-neutral discovery and download boundary. Playback receives local files only.
mod chksz;
mod download;
#[cfg(test)]
mod tests;

use anyhow::Result;
use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, sync::Arc};

pub use download::download;

#[derive(Clone, Debug)]
pub struct Song {
    pub source: &'static str,
    pub id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub cover: Option<reqwest::Url>,
}
pub struct SearchPage {
    pub songs: Vec<Song>,
    pub has_more: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct OnlineSong {
    pub source: String,
    pub id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub cover: Option<String>,
}
impl OnlineSong {
    pub fn key(&self) -> String {
        let platform = match self.source.as_str() {
            "chksz-163" => "netease",
            "chksz-qq" => "qq",
            "chksz-kugou" => "kugou",
            other => other,
        };
        crate::comics::digest(format!("{platform}\0{}", self.id).as_bytes())
    }
    pub fn song(&self) -> Result<Song> {
        let source = sources("")?
            .into_iter()
            .map(|s| s.descriptor().id)
            .find(|id| *id == self.source)
            .ok_or_else(|| anyhow::anyhow!("音乐源暂不可用 / Music source is unavailable"))?;
        Ok(Song {
            source,
            id: self.id.clone(),
            title: self.title.clone(),
            artist: self.artist.clone(),
            album: self.album.clone(),
            cover: self.cover.as_ref().and_then(|v| v.parse().ok()),
        })
    }
}
impl From<&Song> for OnlineSong {
    fn from(song: &Song) -> Self {
        Self {
            source: song.source.into(),
            id: song.id.clone(),
            title: song.title.clone(),
            artist: song.artist.clone(),
            album: song.album.clone(),
            cover: song.cover.as_ref().map(ToString::to_string),
        }
    }
}
pub struct ResolvedTrack {
    pub url: reqwest::Url,
    pub extension: String,
    pub metadata: crate::media::LocalMetadata,
    pub cover: Option<reqwest::Url>,
    /// Raw LRC text when the resolve response already carries lyrics.
    pub lyrics: Option<String>,
}
pub struct Quality {
    pub id: &'static str,
    pub name: &'static str,
}
pub struct Descriptor {
    pub id: &'static str,
    pub name: &'static str,
    pub qualities: &'static [Quality],
    pub default_quality: &'static str,
}
/// Implement this trait and register a provider to add another music service.
/// Credentials remain private to each adapter, never in Song or queue entries.
pub trait MusicSource: Send + Sync {
    fn descriptor(&self) -> Descriptor;
    fn search<'a>(&'a self, keyword: &'a str, page: usize) -> BoxFuture<'a, Result<SearchPage>>;
    fn resolve<'a>(
        &'a self,
        song: &'a Song,
        quality: &'a str,
    ) -> BoxFuture<'a, Result<ResolvedTrack>>;
    /// Optional raw LRC text for a song. Providers without a lyric endpoint
    /// keep the default; failures here must never fail a download.
    fn lyrics<'a>(&'a self, song: &'a Song) -> BoxFuture<'a, Result<Option<String>>> {
        let _ = song;
        Box::pin(async { Ok(None) })
    }
}
pub fn sources(key: &str) -> Result<Vec<Arc<dyn MusicSource>>> {
    chksz::sources("https://api.chksz.com", key)
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Settings {
    pub source: String,
    pub quality: String,
    pub directory: String,
    #[serde(skip)]
    pub api_key: String,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            source: "chksz-163".into(),
            quality: "lossless".into(),
            directory: String::new(),
            api_key: String::new(),
        }
    }
}
impl Settings {
    pub fn directory(&self) -> PathBuf {
        if !self.directory.trim().is_empty() {
            return PathBuf::from(self.directory.trim());
        }
        crate::settings::Settings::queue_path().with_file_name("music")
    }
    pub fn key(&self) -> Result<String> {
        if !self.api_key.trim().is_empty() {
            return Ok(self.api_key.trim().into());
        }
        Ok(String::new())
    }
}

pub use crate::jobs::Job;
