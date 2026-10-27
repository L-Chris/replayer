use super::*;
use anyhow::{Result, anyhow, ensure};
use std::{
    path::{Path, PathBuf},
    sync::OnceLock,
    time::Duration,
};
#[cfg(test)]
mod tests;

pub use replayer_image::{Bitmap, decode, read_image};
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
fn client(tickets: bool) -> Result<&'static reqwest::Client> {
    static CLIENT: OnceLock<Result<reqwest::Client, String>> = OnceLock::new();
    static TICKET_CLIENT: OnceLock<Result<reqwest::Client, String>> = OnceLock::new();
    (if tickets { &TICKET_CLIENT } else { &CLIENT })
        .get_or_init(|| {
            reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(8))
                .timeout(Duration::from_secs(30))
                .redirect(if tickets {
                    reqwest::redirect::Policy::none()
                } else {
                    reqwest::redirect::Policy::limited(4)
                })
                .user_agent(concat!(
                    "replayer/",
                    env!("CARGO_PKG_VERSION"),
                    " comic-reader"
                ))
                .build()
                .map_err(|error| error.to_string())
        })
        .as_ref()
        .map_err(|error| anyhow!("{error}"))
}

async fn download(
    url: reqwest::Url,
    referer: Option<&reqwest::Url>,
    headers: &reqwest::header::HeaderMap,
    timeout: Duration,
    width: u32,
    path: &Path,
) -> Result<(Vec<u8>, Bitmap)> {
    let client = client(headers.contains_key("x-image-ticket"))?;
    let mut request = client.get(url).headers(headers.clone()).timeout(timeout);
    if !headers.contains_key(reqwest::header::ACCEPT) {
        request = request.header(
            reqwest::header::ACCEPT,
            "image/jpeg,image/webp;q=0.9,image/png;q=0.8,*/*;q=0.1",
        );
    }
    if let Some(referer) = referer {
        request = request.header(reqwest::header::REFERER, referer.as_str());
    }
    let response = request
        .send()
        .await
        .map_err(|_| anyhow!("图片连接失败或超时 / Image request timed out"))?;
    if !response.status().is_success() {
        return Err(ImageStatus(response.status()).into());
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

#[derive(Debug)]
struct ImageStatus(reqwest::StatusCode);
impl std::fmt::Display for ImageStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "图片 HTTP {}，可刷新章节重试 / Image request failed",
            self.0.as_u16()
        )
    }
}
impl std::error::Error for ImageStatus {}

async fn download_request(
    mut request: source::ImageRequest,
    width: u32,
    path: &Path,
) -> Result<(Vec<u8>, Bitmap)> {
    // A ticket can expire while queued. Refresh once on 403, never on other failures.
    for attempt in 0..2 {
        let timeout = Duration::from_secs(if request.fallback.is_some() { 10 } else { 30 });
        match download(
            request.url.clone(),
            request.referer.as_ref(),
            &request.headers,
            timeout,
            width,
            path,
        )
        .await
        {
            Ok(image) => return Ok(image),
            Err(error) => {
                if let Some(status) = error.downcast_ref::<ImageStatus>() {
                    if let Some(callback) = &request.on_status {
                        callback(status.0)?;
                    }
                    if status.0 == reqwest::StatusCode::FORBIDDEN
                        && attempt == 0
                        && let Some(refresh) = request.refresh.take()
                    {
                        request = refresh.await?;
                        continue;
                    }
                }
                if let Some(url) = request.fallback {
                    return download(
                        url,
                        None,
                        &Default::default(),
                        Duration::from_secs(30),
                        width,
                        path,
                    )
                    .await
                    .map_err(|fallback| anyhow!("{error}; {fallback}"));
                }
                return Err(error);
            }
        }
    }
    anyhow::bail!("图片凭证已失效 / Image ticket expired")
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
    let request = request.into().resolve().await?;
    let (bytes, bitmap) = download_request(request, width, &path).await?;
    tokio::task::spawn_blocking(move || {
        Ok(CachedImage {
            hash: digest(&bytes),
            path,
            bitmap,
        })
    })
    .await?
}
