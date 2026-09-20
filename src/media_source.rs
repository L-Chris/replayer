//! Local source adapters. Decoding stays outside the UI and the playback engine.
mod qq;
#[cfg(test)]
mod tests;

use anyhow::{Context, Result, bail, ensure};
use std::{
    fs::{self, File},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

pub const QQ_EXTENSIONS: &[&str] = &[
    "qmc0", "qmc2", "qmc3", "qmcflac", "qmcogg", "mflac", "mflac0", "mflac1", "mgg", "mgg0",
    "mgg1", "mggl", "mflach", "mmp4",
];

pub fn is_qq(path: &Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .is_some_and(|s| QQ_EXTENSIONS.contains(&s.to_ascii_lowercase().as_str()))
}

pub struct Prepared {
    path: String,
    temporary: Option<PathBuf>,
}
impl Prepared {
    pub fn path(&self) -> &str {
        &self.path
    }
}
impl Drop for Prepared {
    fn drop(&mut self) {
        if let Some(path) = &self.temporary {
            let _ = fs::remove_file(path);
        }
    }
}

fn cancelled(stop: &AtomicBool) -> Result<()> {
    ensure!(!stop.load(Ordering::Acquire), "media preparation cancelled");
    Ok(())
}

/// The cache is session-scoped: never overwrite the source or reuse stale keys.
pub fn prepare(source: &str, stop: &AtomicBool, ekey: Option<&str>) -> Result<Prepared> {
    let original = || Prepared {
        path: source.to_owned(),
        temporary: None,
    };
    if source.contains("://") || !is_qq(Path::new(source)) {
        return Ok(original());
    }
    cancelled(stop)?;
    let mut input = File::open(source).context("open QQ music file")?;
    let before = input.metadata()?;
    let length = before.len();
    ensure!(length >= 16, "QQ music file is truncated");
    let mut header = [0; 16];
    input.read_exact(&mut header)?;
    // Some downloads only have an unusual extension; do not decrypt plain audio.
    if audio_extension(&header).is_some() {
        return Ok(original());
    }
    let (mut cipher, payload) = qq::probe(&mut input, length, Path::new(source), ekey)?;
    ensure!(payload >= 16, "QQ music file has no audio payload");
    input.rewind()?;
    let root = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("replayer/music-cache");
    fs::create_dir_all(&root)?;
    let mut prepared = Prepared {
        path: String::new(),
        temporary: None,
    };
    let mut output = None;
    let mut offset = 0;
    let mut buffer = vec![0; 64 * 1024];
    while offset < payload {
        cancelled(stop)?;
        let count = (payload - offset).min(buffer.len() as u64) as usize;
        input
            .read_exact(&mut buffer[..count])
            .context("read QQ music payload")?;
        cipher.apply(&mut buffer[..count], offset);
        if output.is_none() {
            let Some(extension) = audio_extension(&buffer[..count]) else {
                bail!("QQ music key is incorrect or this encryption variant is unsupported");
            };
            let path = root.join(format!("{}.{}", uuid::Uuid::new_v4(), extension));
            let file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)?;
            prepared.path = path.to_string_lossy().into_owned();
            prepared.temporary = Some(path);
            output = Some(file);
        }
        output.as_mut().unwrap().write_all(&buffer[..count])?;
        offset += count as u64;
    }
    // Close before the Prepared guard, including on Windows error paths.
    if let Some(mut file) = output.take() {
        file.flush()?;
    }
    cancelled(stop)?;
    let after = input.metadata()?;
    ensure!(
        after.len() == length && before.modified()? == after.modified()?,
        "QQ music file changed during preparation"
    );
    Ok(prepared)
}

fn audio_extension(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"fLaC") {
        Some("flac")
    } else if bytes.starts_with(b"OggS") {
        Some("ogg")
    } else if bytes.starts_with(b"ID3")
        || (bytes.len() >= 2 && bytes[0] == 0xff && bytes[1] & 0xe0 == 0xe0)
    {
        Some("mp3")
    } else if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
        Some("m4a")
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WAVE" {
        Some("wav")
    } else {
        None
    }
}
