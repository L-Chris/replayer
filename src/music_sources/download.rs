use super::*;
use anyhow::{Context, anyhow, ensure};
use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use std::{
    fs::{File, OpenOptions},
    io::Write,
    path::Path,
};
use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

const MAX_BYTES: u64 = 512 * 1024 * 1024;
struct Partial {
    file: Option<File>,
    path: PathBuf,
}
impl Drop for Partial {
    fn drop(&mut self) {
        drop(self.file.take());
        let _ = std::fs::remove_file(&self.path);
    }
}
fn filename(song: &Song, quality: &str, extension: &str) -> String {
    let text = format!("{} - {}", song.artist, song.title);
    let safe: String = text
        .chars()
        .map(|c| {
            if c.is_control() || "<>:\"/\\|?*".contains(c) {
                '_'
            } else {
                c
            }
        })
        .take(70)
        .collect();
    let hash = Sha256::digest(format!("{}\0{}\0{quality}", song.source, song.id).as_bytes());
    format!(
        "{} [{}].{}",
        safe.trim_matches([' ', '.']),
        &format!("{hash:x}")[..16],
        extension
    )
}
fn validate(path: &Path) -> Result<()> {
    // Opening only completed local files avoids signed URLs entering FFmpeg logs.
    let media = ffmpeg_next::format::input(path)
        .map_err(|_| anyhow!("Downloaded file is not playable audio"))?;
    ensure!(
        media
            .streams()
            .best(ffmpeg_next::media::Type::Audio)
            .is_some(),
        "Downloaded file has no audio track"
    );
    Ok(())
}
async fn enrich(path: &Path, track: &ResolvedTrack) -> Result<()> {
    track
        .metadata
        .save(path)
        .context("Cannot save music metadata")?;
    if let Some(text) = &track.lyrics {
        // Standard sidecar name, interoperable with other players; optional data.
        let _ = std::fs::write(path.with_extension("lrc"), text.as_bytes());
    }
    if let Some(url) = &track.cover {
        // Preserve embedded/cached art; cover failure must not discard audio.
        if crate::media::probe_info(&path.to_string_lossy())
            .is_some_and(|info| info.artwork.is_some())
        {
            return Ok(());
        }
        let _ = cache_cover(path, url.clone()).await;
    }
    Ok(())
}
async fn cache_cover(path: &Path, url: reqwest::Url) -> Result<()> {
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()?;
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|_| anyhow!("Cover unavailable"))?;
    ensure!(response.status().is_success(), "Cover unavailable");
    ensure!(
        response
            .content_length()
            .is_none_or(|n| n <= 8 * 1024 * 1024),
        "Cover too large"
    );
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| anyhow!("Cover unavailable"))?;
        ensure!(
            bytes.len() + chunk.len() <= 8 * 1024 * 1024,
            "Cover too large"
        );
        bytes.extend_from_slice(&chunk);
    }
    let art = crate::media::decode_artwork(&bytes).ok_or_else(|| anyhow!("Invalid cover"))?;
    let image = image::RgbaImage::from_raw(art.width as u32, art.height as u32, art.rgba.clone())
        .ok_or_else(|| anyhow!("Invalid cover"))?;
    let mut output = std::io::Cursor::new(Vec::new());
    image.write_to(&mut output, image::ImageFormat::Png)?;
    crate::media::write_companion(path, ".replayer-cover.png", &output.into_inner())
}
pub async fn download(
    source: &dyn MusicSource,
    song: &Song,
    quality: &str,
    directory: &Path,
    progress: Arc<AtomicU64>,
) -> Result<PathBuf> {
    let mut track = source.resolve(song, quality).await?;
    let mut online = OnlineSong::from(song);
    if !track.metadata.title.is_empty() {
        online.title = track.metadata.title.clone();
    }
    if !track.metadata.artist.is_empty() {
        online.artist = track.metadata.artist.clone();
    }
    if !track.metadata.album.is_empty() {
        online.album = track.metadata.album.clone();
    }
    online.cover = track
        .cover
        .as_ref()
        .map(ToString::to_string)
        .or(online.cover);
    track.metadata.online = Some(online);
    if track.lyrics.is_none() {
        // Lyrics are best-effort: an unavailable endpoint never fails playback.
        track.lyrics = source.lyrics(song).await.ok().flatten();
    }
    ensure!(
        ["mp3", "flac", "m4a", "aac", "ogg", "opus", "wav"].contains(&track.extension.as_str()),
        "Unsupported audio format"
    );
    std::fs::create_dir_all(directory).context("Cannot create music directory")?;
    let target = directory.join(filename(song, quality, &track.extension));
    if target.exists() {
        validate(&target)?;
        enrich(&target, &track).await?;
        return Ok(target);
    }
    // Separate client: API credentials and cookies never reach the media host.
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(300))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()?;
    let response = client
        .get(track.url.clone())
        .send()
        .await
        .map_err(|_| anyhow!("Music download connection failed or timed out"))?;
    ensure!(
        response.status().is_success(),
        "Music download HTTP {}",
        response.status().as_u16()
    );
    let expected = response.content_length();
    ensure!(
        expected.is_none_or(|n| n <= MAX_BYTES),
        "Music file exceeds 512 MiB"
    );
    let path = directory.join(format!(".replayer-{}.part", uuid::Uuid::new_v4()));
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .context("Cannot create music download")?;
    let mut partial = Partial {
        file: Some(file),
        path,
    };
    let mut stream = response.bytes_stream();
    let mut count = 0u64;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| anyhow!("Music download interrupted or timed out"))?;
        count += chunk.len() as u64;
        ensure!(count <= MAX_BYTES, "Music file exceeds 512 MiB");
        partial
            .file
            .as_mut()
            .unwrap()
            .write_all(&chunk)
            .context("Cannot write music download")?;
        progress.store(count, Ordering::Relaxed);
    }
    ensure!(
        count > 0 && expected.is_none_or(|n| n == count),
        "Incomplete music download"
    );
    partial.file.take().unwrap().sync_all()?;
    validate(&partial.path)?;
    // An atomic, non-overwriting publish on the same filesystem. Keep downloads
    // durable: the queue never depends on a provider's expiring media URL.
    std::fs::hard_link(&partial.path, &target).context("Cannot publish downloaded music file")?;
    enrich(&target, &track).await?;
    Ok(target)
}
