//! A single disk cache serves both progressive playback and the durable download.
//! Only verified, written ranges are served; a growing file's EOF is never exposed.
use super::download::{MAX_BYTES, enrich, filename, resolve_track, validate};
use super::*;
use anyhow::{Context, anyhow, ensure};
use axum::{
    Router,
    body::Body,
    extract::State,
    http::{HeaderMap, Method, Response, StatusCode},
    response::IntoResponse,
    routing::get,
};
use futures_util::StreamExt;
use std::{
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::sync::{Mutex as AsyncMutex, Notify, Semaphore};
use tokio_util::sync::CancellationToken;

const BLOCK: u64 = 512 * 1024;

pub enum Prepared {
    Local(PathBuf),
    Stream(Stream),
}
pub struct Stream {
    pub url: String,
    pub song: OnlineSong,
    pub bytes: Arc<AtomicU64>,
    completion: Job<PathBuf>,
    cache: Arc<Cache>,
    server: tokio::task::JoinHandle<()>,
}
impl Stream {
    pub fn length(&self) -> Option<u64> {
        self.cache.length
    }

    pub fn poll(&self) -> Option<Result<PathBuf>> {
        self.completion.poll()
    }
}
impl Drop for Stream {
    fn drop(&mut self) {
        self.cache.cancel.cancel();
        self.server.abort();
    }
}
struct Cache {
    path: PathBuf,
    length: Option<u64>,
    remote: reqwest::Url,
    validator: Option<String>,
    client: reqwest::Client,
    ranged: bool,
    blocks: Mutex<Vec<bool>>,
    prefix: AtomicU64,
    bytes: Arc<AtomicU64>,
    complete: std::sync::atomic::AtomicBool,
    error: Mutex<Option<String>>,
    fetch: Vec<Arc<AsyncMutex<()>>>,
    slots: Semaphore,
    changed: Notify,
    cancel: CancellationToken,
}
impl Drop for Cache {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
impl Cache {
    fn fail(&self, message: String) {
        *self.error.lock().unwrap() = Some(message);
        self.changed.notify_waiters();
    }
    fn available(&self, offset: u64) -> u64 {
        if self.ranged {
            let blocks = self.blocks.lock().unwrap();
            if blocks.get((offset / BLOCK) as usize) == Some(&true) {
                return (BLOCK - offset % BLOCK).min(self.length.unwrap() - offset);
            }
            0
        } else {
            self.prefix.load(Ordering::Acquire).saturating_sub(offset)
        }
    }
    async fn fetch_block(&self, index: usize) -> Result<()> {
        let _lock = self.fetch[index].lock().await;
        if self.blocks.lock().unwrap()[index] {
            return Ok(());
        }
        ensure!(!self.cancel.is_cancelled(), "Cancelled");
        let _permit = self.slots.acquire().await?;
        let start = index as u64 * BLOCK;
        let end = (start + BLOCK).min(self.length.unwrap()) - 1;
        let mut request = self
            .client
            .get(self.remote.clone())
            .header("Range", format!("bytes={start}-{end}"));
        if let Some(validator) = &self.validator {
            request = request.header("If-Range", validator);
        }
        let response = request
            .send()
            .await
            .map_err(|_| anyhow!("Music download connection failed or timed out"))?;
        ensure!(
            response.status() == StatusCode::PARTIAL_CONTENT,
            "Music server changed or stopped supporting ranges"
        );
        ensure!(
            content_range(response.headers())? == (start, end, self.length.unwrap()),
            "Invalid music content range"
        );
        self.write_block(index, response).await
    }
    async fn write_block(&self, index: usize, response: reqwest::Response) -> Result<()> {
        let start = index as u64 * BLOCK;
        let size = BLOCK.min(self.length.unwrap() - start);
        let mut data = Vec::with_capacity(size as usize);
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| anyhow!("Music download interrupted or timed out"))?;
            ensure!(
                data.len() as u64 + chunk.len() as u64 <= size,
                "Invalid music range length"
            );
            data.extend_from_slice(&chunk);
        }
        ensure!(data.len() as u64 == size, "Incomplete music download");
        let mut file = OpenOptions::new().write(true).open(&self.path)?;
        file.seek(SeekFrom::Start(start))?;
        file.write_all(&data)
            .context("Cannot write music download")?;
        self.blocks.lock().unwrap()[index] = true;
        self.bytes.fetch_add(size, Ordering::Release);
        self.changed.notify_waiters();
        Ok(())
    }
    async fn sequential(&self, response: reqwest::Response) -> Result<()> {
        let mut file = OpenOptions::new().write(true).open(&self.path)?;
        let mut stream = response.bytes_stream();
        let mut count = 0;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| anyhow!("Music download interrupted or timed out"))?;
            count += chunk.len() as u64;
            ensure!(count <= MAX_BYTES, "Music file exceeds 512 MiB");
            ensure!(
                self.length.is_none_or(|n| count <= n),
                "Invalid music response length"
            );
            file.write_all(&chunk)
                .context("Cannot write music download")?;
            self.prefix.store(count, Ordering::Release);
            self.bytes.store(count, Ordering::Release);
            self.changed.notify_waiters();
        }
        ensure!(
            count > 0 && self.length.is_none_or(|n| n == count),
            "Incomplete music download"
        );
        Ok(())
    }
    async fn read(&self, offset: u64, size: usize) -> Result<Vec<u8>> {
        loop {
            let changed = self.changed.notified();
            // Register before observing state so completion cannot lose a wakeup.
            tokio::pin!(changed);
            changed.as_mut().enable();
            ensure!(!self.cancel.is_cancelled(), "Cancelled");
            if let Some(error) = self.error.lock().unwrap().clone() {
                return Err(anyhow!(error));
            }
            let available = self.available(offset);
            if available > 0 {
                let mut data = vec![0; size.min(available as usize)];
                let mut file = File::open(&self.path)?;
                file.seek(SeekFrom::Start(offset))?;
                file.read_exact(&mut data)?;
                return Ok(data);
            }
            if self.complete.load(Ordering::Acquire) {
                return Ok(Vec::new());
            }
            if self.ranged {
                // Reserve capacity for an on-demand seek while the one background
                // fetch is in flight. Per-block locks prevent duplicate requests.
                let result = tokio::select! {
                    result = self.fetch_block((offset / BLOCK) as usize) => result,
                    _ = self.cancel.cancelled() => Err(anyhow!("Cancelled")),
                };
                if let Err(error) = result {
                    self.fail(error.to_string());
                    return Err(error);
                }
            } else {
                tokio::select! {
                    _ = &mut changed => {},
                    _ = self.cancel.cancelled() => return Err(anyhow!("Cancelled")),
                }
            }
        }
    }
}
fn content_range(headers: &HeaderMap) -> Result<(u64, u64, u64)> {
    let parsed = || {
        let value = headers
            .get("Content-Range")?
            .to_str()
            .ok()?
            .strip_prefix("bytes ")?;
        let (range, total) = value.split_once('/')?;
        let (start, end) = range.split_once('-')?;
        Some((start.parse().ok()?, end.parse().ok()?, total.parse().ok()?))
    };
    parsed().ok_or_else(|| anyhow!("Invalid music content range"))
}

async fn serve(
    State(cache): State<Arc<Cache>>,
    method: Method,
    headers: HeaderMap,
) -> axum::response::Response {
    let (start, end, partial) = if let Some(length) = cache.length {
        match crate::torrent::byte_range(headers.get("Range").and_then(|s| s.to_str().ok()), length)
        {
            Ok(range) => range,
            Err(_) => {
                return Response::builder()
                    .status(416)
                    .header("Content-Range", format!("bytes */{length}"))
                    .body(Body::empty())
                    .unwrap();
            }
        }
    } else {
        // Unknown-length, non-range sources remain a forward-only stream.
        (0, MAX_BYTES - 1, false)
    };
    let mut response = Response::builder()
        .status(if partial { 206 } else { 200 })
        .header("Content-Type", "application/octet-stream");
    if let Some(length) = cache.length {
        response = response
            .header("Accept-Ranges", "bytes")
            .header("Content-Length", end - start + 1);
        if partial {
            response = response.header("Content-Range", format!("bytes {start}-{end}/{length}"));
        }
    }
    let body = if method == Method::HEAD {
        Body::empty()
    } else {
        Body::from_stream(futures_util::stream::try_unfold(
            (cache, start),
            move |(cache, position)| async move {
                if position > end {
                    return Ok::<_, std::io::Error>(None);
                }
                let data = cache
                    .read(position, (end - position + 1).min(65536) as usize)
                    .await
                    .map_err(std::io::Error::other)?;
                if data.is_empty() {
                    return Ok(None);
                }
                let next = position + data.len() as u64;
                Ok(Some((data, (cache, next))))
            },
        ))
    };
    response.body(body).unwrap().into_response()
}

pub async fn prepare(
    source: Arc<dyn MusicSource>,
    song: Song,
    quality: String,
    directory: PathBuf,
) -> Result<Prepared> {
    let mut track = resolve_track(source.as_ref(), &song, &quality).await?;
    ensure!(
        ["mp3", "flac", "m4a", "aac", "ogg", "opus", "wav"].contains(&track.extension.as_str()),
        "Unsupported audio format"
    );
    std::fs::create_dir_all(&directory).context("Cannot create music directory")?;
    let target = directory.join(filename(&song, &quality, &track.extension));
    if target.is_file() {
        validate(&target)?;
        track.metadata.save(&target)?;
        return Ok(Prepared::Local(target));
    }
    // A separate client never forwards provider API credentials to the media host.
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(300))
        .read_timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()?;
    let response = client
        .get(track.url.clone())
        .header("Range", format!("bytes=0-{}", BLOCK - 1))
        .send()
        .await
        .map_err(|_| anyhow!("Music download connection failed or timed out"))?;
    ensure!(
        response.status().is_success(),
        "Music download HTTP {}",
        response.status().as_u16()
    );
    let ranged = response.status() == StatusCode::PARTIAL_CONTENT;
    let length = if ranged {
        let (start, end, total) = content_range(response.headers())?;
        ensure!(
            total > 0 && start == 0 && end == BLOCK.min(total) - 1,
            "Invalid music content range"
        );
        Some(total)
    } else {
        response.content_length()
    };
    ensure!(
        length.is_none_or(|n| n > 0 && n <= MAX_BYTES),
        "Music file exceeds 512 MiB"
    );
    let validator = response
        .headers()
        .get("ETag")
        .filter(|v| !v.as_bytes().starts_with(b"W/"))
        .or_else(|| response.headers().get("Last-Modified"))
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let path = directory.join(format!(".replayer-{}.part", uuid::Uuid::new_v4()));
    OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&path)
        .context("Cannot create music download")?;
    let bytes = Arc::new(AtomicU64::new(0));
    let cache = Arc::new(Cache {
        path,
        length,
        remote: track.url.clone(),
        validator,
        client,
        ranged,
        blocks: Mutex::new(vec![
            false;
            if ranged {
                length.unwrap().div_ceil(BLOCK) as usize
            } else {
                0
            }
        ]),
        prefix: AtomicU64::new(0),
        bytes: bytes.clone(),
        complete: false.into(),
        error: Mutex::new(None),
        fetch: (0..if ranged {
            length.unwrap().div_ceil(BLOCK) as usize
        } else {
            0
        })
            .map(|_| Arc::new(AsyncMutex::new(())))
            .collect(),
        slots: Semaphore::new(2),
        changed: Notify::new(),
        cancel: CancellationToken::new(),
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let token = uuid::Uuid::new_v4();
    let route = format!("/{token}/audio.{}", track.extension);
    // Acquire before publishing the server, including when preparation is cancelled.
    let initial = if ranged {
        Some(cache.fetch[0].clone().lock_owned().await)
    } else {
        None
    };
    let router = Router::new()
        .route(&route, get(serve))
        .with_state(cache.clone());
    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    let worker = cache.clone();
    let online = track.metadata.online.clone().unwrap();
    let completion = Job::spawn(move |_| async move {
        let download = async {
            if worker.ranged {
                worker.write_block(0, response).await?;
                drop(initial);
                let block_count = worker.blocks.lock().unwrap().len();
                for index in 1..block_count {
                    worker.fetch_block(index).await?;
                    tokio::task::yield_now().await;
                }
            } else {
                drop(initial);
                worker.sequential(response).await?;
            }
            OpenOptions::new()
                .write(true)
                .open(&worker.path)?
                .sync_all()?;
            validate(&worker.path)?;
            // Keep the cache path alive for active FFmpeg readers after publication.
            if let Err(error) = std::fs::hard_link(&worker.path, &target) {
                if error.kind() != std::io::ErrorKind::AlreadyExists {
                    return Err(error).context("Cannot publish downloaded music file");
                }
                validate(&target)?;
            }
            track
                .metadata
                .save(&target)
                .context("Cannot save music metadata")?;
            worker.complete.store(true, Ordering::Release);
            worker.changed.notify_waiters();
            if track.lyrics.is_none() {
                track.lyrics = source.lyrics(&song).await.ok().flatten();
            }
            enrich(&target, &track).await?;
            Ok(target)
        };
        let result = tokio::select! {
            result = download => result,
            _ = worker.cancel.cancelled() => Err(anyhow!("Cancelled")),
        };
        if let Err(error) = &result {
            worker.fail(error.to_string());
        }
        result
    });
    Ok(Prepared::Stream(Stream {
        url: format!("http://{address}{route}"),
        song: online,
        bytes,
        completion,
        cache,
        server,
    }))
}

#[cfg(test)]
mod tests;
