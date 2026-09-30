use std::{
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
};

/// Metadata accompanying downloaded audio. No credentials or remote URLs are stored.
#[derive(Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct LocalMetadata {
    pub title: String,
    pub artist: String,
    pub album: String,
}
pub fn companion_path(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
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
    let temporary = companion_path(&target, &format!(".{}.tmp", uuid::Uuid::new_v4()));
    let result =
        std::fs::write(&temporary, bytes).and_then(|()| std::fs::rename(&temporary, target));
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    Ok(result?)
}
impl LocalMetadata {
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        write_companion(path, ".replayer.json", &serde_json::to_vec(self)?)
    }
    fn load(path: &Path) -> Self {
        read_bounded(&companion_path(path, ".replayer.json"), 64 * 1024)
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
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    let image = reader.decode().ok()?.thumbnail(600, 600).to_rgba8();
    Some(Arc::new(Artwork {
        width: image.width() as usize,
        height: image.height() as usize,
        rgba: image.into_raw(),
    }))
}
/// One metadata reader for playback and queue entries: embedded tags take
/// precedence, downloaded companion data fills omissions, then the UI uses names.
pub fn info(input: &ffmpeg_next::format::context::Input, path: &Path) -> Info {
    use ffmpeg_next::{format::stream::Disposition, media::Type};
    let video = input.streams().any(|s| {
        s.parameters().medium() == Type::Video
            && !s.disposition().contains(Disposition::ATTACHED_PIC)
    });
    let local = if video {
        LocalMetadata::default()
    } else {
        LocalMetadata::load(path)
    };
    let nonempty = |value: String| (!value.trim().is_empty()).then_some(value);
    let tag = |name: &str| {
        input
            .metadata()
            .iter()
            .find(|(key, value)| key.eq_ignore_ascii_case(name) && !value.trim().is_empty())
            .map(|(_, value)| value.to_owned())
            .or_else(|| {
                input.streams().best(Type::Audio).and_then(|s| {
                    s.metadata()
                        .iter()
                        .find(|(key, value)| {
                            key.eq_ignore_ascii_case(name) && !value.trim().is_empty()
                        })
                        .map(|(_, value)| value.to_owned())
                })
            })
    };
    let artwork = if video {
        None
    } else {
        input
            .streams()
            .filter(|s| s.disposition().contains(Disposition::ATTACHED_PIC))
            .find_map(|s| {
                let packet = unsafe { &(*s.as_ptr()).attached_pic };
                if packet.data.is_null() || packet.size <= 0 || packet.size > 8 * 1024 * 1024 {
                    return None;
                }
                decode_artwork(unsafe {
                    std::slice::from_raw_parts(packet.data, packet.size as usize)
                })
            })
            .or_else(|| {
                read_bounded(
                    &companion_path(path, ".replayer-cover.png"),
                    8 * 1024 * 1024,
                )
                .and_then(|bytes| decode_artwork(&bytes))
            })
    };
    Info {
        kind: if video { Kind::Video } else { Kind::Music },
        title: tag("title").or_else(|| nonempty(local.title)),
        artist: tag("artist").or_else(|| nonempty(local.artist)),
        album: tag("album").or_else(|| nonempty(local.album)),
        artwork,
    }
}
/// Reads tags and artwork without playback, off the UI thread, including files
/// that have never been selected. Bound probing so one bad file cannot stall a queue.
pub fn probe_info(path: &str) -> Option<Arc<Info>> {
    let stop = std::sync::atomic::AtomicBool::new(false);
    let prepared = crate::media_source::prepare(path, &stop, None).ok()?;
    ffmpeg_next::init().ok()?;
    let started = std::time::Instant::now();
    let input = ffmpeg_next::format::input_with_interrupt_and_dictionary(
        prepared.path(),
        move || started.elapsed() > std::time::Duration::from_secs(3),
        ffmpeg_next::Dictionary::new(),
    )
    .ok()?;
    Some(Arc::new(info(&input, Path::new(path))))
}
