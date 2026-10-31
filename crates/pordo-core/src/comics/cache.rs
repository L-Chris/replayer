use super::*;
use anyhow::{Context, Result, anyhow, ensure};
use std::{
    path::{Path, PathBuf},
    sync::OnceLock,
    time::Duration,
};
#[cfg(test)]
mod tests;

pub use crate::pixels::{Bitmap, decode, read_image};
pub struct CachedImage {
    pub path: PathBuf,
    pub hash: String,
    pub bitmap: Bitmap,
}
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension(format!("{}.part", uuid::Uuid::new_v4()));
    let result = std::fs::write(&temporary, bytes).and_then(|()| std::fs::rename(&temporary, path));
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result?;
    Ok(())
}
pub fn load(path: PathBuf, width: u32) -> Result<CachedImage> {
    ensure!(
        std::fs::metadata(&path)?.len() <= 40 * 1024 * 1024,
        "图片超过限制 / Image too large"
    );
    let bytes = std::fs::read(&path)?;
    Ok(CachedImage {
        bitmap: decode(&bytes, width)?,
        hash: digest(&bytes),
        path,
    })
}
/// Bound CPU/memory-heavy image work independently of HTTP concurrency. A
/// cancelled waiter leaves its running worker holding the permit until done.
pub async fn background<T: Send + 'static>(
    work: impl FnOnce() -> Result<T> + Send + 'static,
) -> Result<T> {
    static SLOTS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);
    let permit = SLOTS.acquire().await?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        work()
    })
    .await?
}
fn client() -> Result<&'static reqwest::Client> {
    static CLIENT: OnceLock<Result<reqwest::Client, String>> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(8))
                // Image tickets and hotlink headers stay bound to their resolved URL.
                // Do not forward them through redirects.
                .redirect(reqwest::redirect::Policy::none())
                .user_agent(concat!("pordo/", env!("CARGO_PKG_VERSION")))
                .build()
                .map_err(|error| error.to_string())
        })
        .as_ref()
        .map_err(|error| anyhow!("{error}"))
}

#[derive(Debug)]
struct ImageHttpError(reqwest::StatusCode);
impl std::fmt::Display for ImageHttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "图片 HTTP {}，可刷新章节重试 / Image request failed",
            self.0.as_u16()
        )
    }
}
impl std::error::Error for ImageHttpError {}

async fn download(
    request: source::ImageRequest,
    width: u32,
    path: &Path,
) -> Result<(Vec<u8>, Bitmap)> {
    let headers = &request.headers;
    let client = client()?;
    let mut request = client
        .get(request.url)
        .headers(headers.clone())
        .timeout(Duration::from_secs(65));
    if !headers.contains_key(reqwest::header::ACCEPT) {
        request = request.header(
            reqwest::header::ACCEPT,
            "image/jpeg,image/webp;q=0.9,image/png;q=0.8,*/*;q=0.1",
        );
    }
    let response = request
        .send()
        .await
        .context("图片连接失败 / Image request failed")?;
    if !response.status().is_success() {
        let status = response.status();
        return Err(anyhow::Error::new(ImageHttpError(status)));
    }
    let bytes = source::limited_body(response, 40 * 1024 * 1024).await?;
    // Persist completed downloads before decoding. Cancelling a display job must
    // not discard bytes already received from a quota-limited source.
    let path = path.to_owned();
    let bytes = tokio::task::spawn_blocking(move || {
        atomic_write(&path, &bytes)?;
        Ok::<_, anyhow::Error>(bytes)
    })
    .await??;
    background(move || {
        let bitmap = decode(&bytes, width)?;
        Ok((bytes, bitmap))
    })
    .await
}

pub async fn fetch(
    directory: &Path,
    key: &str,
    request: impl Into<source::ImageRequest>,
    width: u32,
) -> Result<CachedImage> {
    let path = directory.join(format!("{}.img", digest(key.as_bytes())));
    let cached_path = path.clone();
    if let Some(image) = background(move || {
        if cached_path.exists() {
            match load(cached_path.clone(), width) {
                Ok(image) => return Ok(Some(image)),
                Err(_) => {
                    let _ = std::fs::remove_file(&cached_path);
                }
            }
        }
        Ok(None)
    })
    .await?
    {
        return Ok(image);
    }
    let request = request.into();
    let first = download(request.resolve().await?, width, &path).await;
    let (bytes, bitmap) = match first {
        Err(error)
            if request.can_refresh()
                && error
                    .downcast_ref::<ImageHttpError>()
                    .is_some_and(|e| e.0 == reqwest::StatusCode::FORBIDDEN) =>
        {
            // Only one re-resolution for an expired third-party image ticket.
            download(request.resolve().await?, width, &path).await?
        }
        other => other?,
    };
    tokio::task::spawn_blocking(move || {
        Ok(CachedImage {
            hash: digest(&bytes),
            path,
            bitmap,
        })
    })
    .await?
}
