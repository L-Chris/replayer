use super::*;
use anyhow::{Result, anyhow, ensure};
use std::{
    io::Cursor,
    path::{Path, PathBuf},
    sync::OnceLock,
    time::Duration,
};

pub struct Bitmap {
    pub width: usize,
    pub height: usize,
    pub tiles: Vec<image::RgbaImage>,
}
pub struct CachedImage {
    pub path: PathBuf,
    pub hash: String,
    pub bitmap: Bitmap,
}
pub fn read_image(bytes: &[u8]) -> Result<image::DynamicImage> {
    let mut reader = image::ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(16384);
    limits.max_image_height = Some(65536);
    limits.max_alloc = Some(256 * 1024 * 1024);
    reader.limits(limits);
    reader
        .decode()
        .map_err(|_| anyhow!("图片损坏或尺寸过大 / Invalid or oversized image"))
}
pub fn decode(bytes: &[u8], max_width: u32) -> Result<Bitmap> {
    let mut image = read_image(bytes)?;
    let scale = (max_width as f64 / image.width() as f64)
        .min(1.0)
        .min((16.0 * 1024.0 * 1024.0 / (image.width() as f64 * image.height() as f64)).sqrt());
    if scale < 1.0 {
        image = image.resize_exact(
            (image.width() as f64 * scale).max(1.0) as u32,
            (image.height() as f64 * scale).max(1.0) as u32,
            image::imageops::FilterType::Triangle,
        );
    }
    let width = image.width();
    let height = image.height();
    let tiles = (0..height)
        .step_by(2048)
        .map(|y| {
            image
                .crop_imm(0, y, width, (height - y).min(2048))
                .to_rgba8()
        })
        .collect();
    Ok(Bitmap {
        width: width as usize,
        height: height as usize,
        tiles,
    })
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
fn client() -> Result<&'static reqwest::Client> {
    static CLIENT: OnceLock<Result<reqwest::Client, String>> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(8))
                .timeout(Duration::from_secs(30))
                .redirect(reqwest::redirect::Policy::limited(4))
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

async fn download(url: reqwest::Url, timeout: Duration, width: u32) -> Result<(Vec<u8>, Bitmap)> {
    let response = client()?
        .get(url)
        .timeout(timeout)
        .send()
        .await
        .map_err(|_| anyhow!("图片连接失败或超时 / Image request timed out"))?;
    ensure!(
        response.status().is_success(),
        "图片 HTTP {}，可刷新章节重试 / Image request failed",
        response.status().as_u16()
    );
    let bytes = source::limited_body(response, 40 * 1024 * 1024).await?;
    tokio::task::spawn_blocking(move || {
        let bitmap = decode(&bytes, width)?;
        Ok((bytes, bitmap))
    })
    .await?
}

pub async fn fetch(
    directory: &Path,
    key: &str,
    request: impl Into<source::ImageRequest>,
    width: u32,
) -> Result<CachedImage> {
    let path = directory.join(format!("{}.img", digest(key.as_bytes())));
    let cached_path = path.clone();
    if let Some(image) = tokio::task::spawn_blocking(move || {
        if cached_path.exists() {
            match load(cached_path.clone(), width) {
                Ok(image) => return Some(image),
                Err(_) => {
                    let _ = std::fs::remove_file(&cached_path);
                }
            }
        }
        None
    })
    .await?
    {
        return Ok(image);
    }
    let request = request.into();
    let timeout = Duration::from_secs(if request.fallback.is_some() { 10 } else { 30 });
    let (bytes, bitmap) = match download(request.url, timeout, width).await {
        Ok(bytes) => bytes,
        Err(direct_error) => match request.fallback {
            Some(url) => download(url, Duration::from_secs(30), width)
                .await
                .map_err(|fallback_error| anyhow!("{direct_error}; {fallback_error}"))?,
            None => return Err(direct_error),
        },
    };
    tokio::task::spawn_blocking(move || {
        atomic_write(&path, &bytes)?;
        Ok(CachedImage {
            hash: digest(&bytes),
            path,
            bitmap,
        })
    })
    .await?
}
