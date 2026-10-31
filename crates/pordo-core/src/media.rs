use std::{
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
};

#[cfg(test)]
mod tests;

/// Metadata accompanying downloaded audio. No credentials or remote URLs are stored.
#[derive(Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct LocalMetadata {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub online: Option<crate::music_sources::OnlineSong>,
}
pub fn companion_path(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}
/// Prefer current metadata, while retaining downloads made before the rename.
pub fn companion_for_read(path: &Path, suffix: &str) -> PathBuf {
    let current = companion_path(path, &format!(".pordo{suffix}"));
    if current.exists() {
        current
    } else {
        companion_path(path, &format!(".replayer{suffix}"))
    }
}
fn read_bounded(path: &Path, limit: u64) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    (bytes.len() as u64 <= limit).then_some(bytes)
}
pub fn write_companion(path: &Path, suffix: &str, bytes: &[u8]) -> anyhow::Result<()> {
    let target = companion_path(path, suffix);
    write_atomic(&target, bytes)
}
pub fn write_atomic(target: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let temporary = companion_path(target, &format!(".{}.tmp", uuid::Uuid::new_v4()));
    let result =
        std::fs::write(&temporary, bytes).and_then(|()| std::fs::rename(&temporary, target));
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    Ok(result?)
}
impl LocalMetadata {
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        write_companion(path, ".pordo.json", &serde_json::to_vec(self)?)
    }
    pub fn load(path: &Path) -> Self {
        read_bounded(&companion_for_read(path, ".json"), 64 * 1024)
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }
}

pub const AUDIO_EXTENSIONS: &[&str] = &[
    "mp3", "flac", "wav", "m4a", "aac", "ogg", "opus", "aif", "aiff", "wma", "alac",
];
pub const VIDEO_EXTENSIONS: &[&str] = &[
    "mp4", "mkv", "webm", "mov", "m4v", "avi", "ts", "m2ts", "flv", "wmv", "mpg", "mpeg", "3gp",
    "ogv",
];
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Music,
    Video,
}
#[derive(Clone)]
pub struct Artwork {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}
impl std::fmt::Debug for Artwork {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Artwork({}x{})", self.width, self.height)
    }
}
#[derive(Clone, Debug)]
pub struct Info {
    pub kind: Kind,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub artwork: Option<Arc<Artwork>>,
}
pub fn supported(path: &str) -> bool {
    std::path::Path::new(path)
        .extension()
        .and_then(|s| s.to_str())
        .is_some_and(|s| {
            let s = s.to_ascii_lowercase();
            AUDIO_EXTENSIONS.contains(&s.as_str()) || VIDEO_EXTENSIONS.contains(&s.as_str())
        })
}
pub fn decode_artwork(bytes: &[u8]) -> Option<Arc<Artwork>> {
    if bytes.len() > 8 * 1024 * 1024 {
        return None;
    }
    let image = crate::pixels::thumbnail(bytes, 600, 600).ok()?;
    Some(Arc::new(Artwork {
        width: image.width() as usize,
        height: image.height() as usize,
        rgba: image.bytes().to_vec(),
    }))
}
pub fn remdia_info(session: &mut remdia::Session, path: &Path) -> Info {
    let media = session.info();
    let video = media
        .tracks
        .iter()
        .any(|t| t.kind == "video" && !t.attached_picture);
    let local = if video {
        LocalMetadata::default()
    } else {
        LocalMetadata::load(path)
    };
    let tag = |key: &str| {
        media
            .metadata
            .get(key)
            .filter(|v| !v.trim().is_empty())
            .cloned()
            .or_else(|| {
                media.tracks.iter().find_map(|t| {
                    t.metadata
                        .get(key)
                        .filter(|v| !v.trim().is_empty())
                        .cloned()
                })
            })
    };
    let (title, artist, album) = (tag("title"), tag("artist"), tag("album"));
    let nonempty = |v: String| (!v.trim().is_empty()).then_some(v);
    let artwork = if video {
        None
    } else {
        session
            .artwork()
            .ok()
            .flatten()
            .and_then(|f| decode_artwork(f.bytes()))
            .or_else(|| {
                read_bounded(&companion_for_read(path, "-cover.png"), 8 * 1024 * 1024)
                    .and_then(|b| decode_artwork(&b))
            })
    };
    Info {
        kind: if video { Kind::Video } else { Kind::Music },
        title: title.or_else(|| nonempty(local.title)),
        artist: artist.or_else(|| nonempty(local.artist)),
        album: album.or_else(|| nonempty(local.album)),
        artwork,
    }
}
/// Probe a plain local path or a remdia-supported URI. Hosts with special
/// source preparation (encrypted adapters, torrents) prepare first and keep
/// their own wrapper.
pub fn probe_info(path: &str) -> Option<Arc<Info>> {
    probe_info_at(path, Path::new(path))
}

/// Probe `prepared` while reading sidecar metadata and covers from `display`.
/// Prepared URIs (decrypted adapters, torrents) may not match the user-facing
/// path that owns the `.pordo.json`/`.pordo-cover.png` companions.
pub fn probe_info_at(prepared: &str, display: &Path) -> Option<Arc<Info>> {
    let engine = crate::media_backend::engine().ok()?;
    let mut request = remdia::Request::playback(prepared);
    request.task = remdia::Task::Probe;
    request.timeout_ms = 3000;
    let mut session =
        unsafe { engine.open(request.clone(), remdia::Cancellation::default()) }.ok()?;
    let mut metadata = remdia_info(&mut session, display);
    if metadata.kind == Kind::Video {
        let duration = session.info().duration_us.unwrap_or(0);
        drop(session);
        request.task = remdia::Task::Thumbnail;
        let mut thumbnail =
            unsafe { engine.open(request, remdia::Cancellation::default()) }.ok()?;
        let target = if duration > 10_000_000 {
            (duration / 10).min(30_000_000)
        } else {
            0
        };
        if target > 0 {
            let _ = thumbnail.seek(target);
        }
        let mut fallback = None;
        for _ in 0..12 {
            let Some(frame) = thumbnail.read().ok().flatten() else {
                break;
            };
            let remdia::FrameInfo::Image { image, .. } = frame.info() else {
                continue;
            };
            if image.format != "rgba8" {
                continue;
            }
            let (width, height) = (&image.width, &image.height);
            let mut rgba = Vec::with_capacity(*width as usize * *height as usize * 4);
            for row in frame.bytes().chunks(image.stride).take(*height as usize) {
                rgba.extend_from_slice(&row[..*width as usize * 4]);
            }
            let lit = rgba
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|p| u16::from(p[0]) + u16::from(p[1]) + u16::from(p[2]) > 72)
                .count();
            let art = Arc::new(Artwork {
                width: *width as usize,
                height: *height as usize,
                rgba,
            });
            fallback = Some(art);
            if lit > (*width as usize * *height as usize) / 100 {
                break;
            }
        }
        metadata.artwork = fallback;
    }
    Some(Arc::new(metadata))
}
