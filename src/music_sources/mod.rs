//! Provider-neutral discovery and download boundary. Playback receives local files only.
mod chksz;
mod download;
#[cfg(test)]
mod tests;

use anyhow::Result;
use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

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
pub struct ResolvedTrack {
    pub url: reqwest::Url,
    pub extension: String,
    pub metadata: crate::media::LocalMetadata,
    pub cover: Option<reqwest::Url>,
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
}
pub fn sources(key: &str) -> Result<Vec<Arc<dyn MusicSource>>> {
    chksz::sources("https://api.chksz.com", key)
}

#[derive(Clone, Serialize, Deserialize)]
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
        crate::env_config::get("CHKSZ_API_KEY").map(|v| v.unwrap_or_default())
    }
}

/// A cancellable background operation. Its receiver is owned by one UI request,
/// so replacing it cannot apply an older search result to a newer search.
pub struct Job<T> {
    result: crossbeam_channel::Receiver<Result<T>>,
    cancel: Arc<AtomicBool>,
    pub bytes: Arc<AtomicU64>,
}
impl<T: Send + 'static> Job<T> {
    pub fn spawn<F, Fut>(work: F) -> Self
    where
        F: FnOnce(Arc<AtomicU64>) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<T>> + Send + 'static,
    {
        let (tx, result) = crossbeam_channel::bounded(1);
        let cancel = Arc::new(AtomicBool::new(false));
        let bytes = Arc::new(AtomicU64::new(0));
        let stop = cancel.clone();
        let progress = bytes.clone();
        std::thread::spawn(move || {
            let result = tokio::runtime::Builder::new_current_thread().enable_all().build()
                .map_err(anyhow::Error::from).and_then(|runtime| runtime.block_on(async {
                    tokio::select! {
                        value = work(progress) => value,
                        _ = async { while !stop.load(Ordering::Relaxed) { tokio::time::sleep(Duration::from_millis(50)).await; } } => anyhow::bail!("Cancelled"),
                    }
                }));
            let _ = tx.send(result);
        });
        Self {
            result,
            cancel,
            bytes,
        }
    }
    pub fn poll(&self) -> Option<Result<T>> {
        match self.result.try_recv() {
            Ok(value) => Some(value),
            Err(crossbeam_channel::TryRecvError::Empty) => None,
            Err(_) => Some(Err(anyhow::anyhow!("Music worker stopped"))),
        }
    }
}
impl<T> Drop for Job<T> {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}
