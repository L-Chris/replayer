use anyhow::{Context, Result, ensure};
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
pub const RATE: u32 = 16000;
pub struct Chunk {
    pub start: f64,
    pub samples: Vec<f32>,
}
impl Chunk {
    pub fn duration(&self) -> f64 {
        self.samples.len() as f64 / RATE as f64
    }
    pub fn wav(&self) -> Vec<u8> {
        let bytes = (self.samples.len() * 2) as u32;
        let mut out = Vec::with_capacity(bytes as usize + 44);
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + bytes).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&RATE.to_le_bytes());
        out.extend_from_slice(&(RATE * 2).to_le_bytes());
        out.extend_from_slice(&2u16.to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&bytes.to_le_bytes());
        for s in &self.samples {
            out.extend_from_slice(&((s.clamp(-1., 1.) * 32767.).round() as i16).to_le_bytes());
        }
        out
    }
}
pub struct AudioChunks {
    session: remdia::Session,
    path: std::path::PathBuf,
    pub duration: f64,
    seconds: usize,
    next: f64,
    end: Option<f64>,
    cancel: Arc<AtomicBool>,
}
impl AudioChunks {
    pub fn open(path: &Path, cancel: Arc<AtomicBool>, seconds: usize) -> Result<Self> {
        ensure!((1..=120).contains(&seconds), "字幕音频分段必须为 1–120 秒");
        let engine = crate::media_backend::engine()?;
        let mut request = remdia::Request::playback(path.to_str().context("无效媒体路径")?);
        request.task = remdia::Task::Audio;
        request.audio_rate = RATE;
        request.audio_channels = 1;
        let session =
            unsafe { engine.open(request, remdia::Cancellation::from_flag(cancel.clone())) }?;
        let duration = session.info().duration_us.unwrap_or(0) as f64 / 1e6;
        Ok(Self {
            session,
            path: path.to_path_buf(),
            duration,
            seconds,
            next: 0.,
            end: None,
            cancel,
        })
    }
    pub fn set_range(&mut self, start: f64, end: Option<f64>) -> Result<()> {
        ensure!(
            start.is_finite() && start >= 0. && end.is_none_or(|e| e.is_finite() && e > start),
            "无效字幕范围"
        );
        self.next = start;
        self.end = end;
        Ok(())
    }
    pub fn next(&mut self) -> Result<Option<Chunk>> {
        ensure!(!self.cancel.load(Ordering::Acquire), "字幕生成已取消");
        let limit = self.end.unwrap_or(if self.duration > 0. {
            self.duration
        } else {
            f64::MAX
        });
        if self.next >= limit {
            return Ok(None);
        }
        let length = (self.seconds as f64).min(limit - self.next);
        let start = self.next;
        let chunk = self.session.audio_chunk(
            (start * 1e6).round() as i64,
            (length * 1e6).round().max(1.) as i64,
        )?;
        if chunk.samples.is_empty() {
            return Ok(None);
        }
        let samples = chunk.samples.clone();
        self.next = start + samples.len() as f64 / RATE as f64;
        Ok(Some(Chunk { start, samples }))
    }
}

pub struct Transfer {
    path: std::path::PathBuf,
    cancel: Arc<AtomicBool>,
    seconds: usize,
    next: f64,
    end: Option<f64>,
}
impl AudioChunks {
    pub fn into_transfer(self) -> Transfer {
        Transfer {
            path: self.path.clone(),
            cancel: self.cancel.clone(),
            seconds: self.seconds,
            next: self.next,
            end: self.end,
        }
    }
}
impl Transfer {
    pub fn open(self) -> Result<AudioChunks> {
        let mut audio = AudioChunks::open(&self.path, self.cancel, self.seconds)?;
        audio.set_range(self.next, self.end)?;
        Ok(audio)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires remdia component DLLs and native fixture corpus"]
    fn native_media_subtitle_range_and_mp3() {
        let path = std::path::PathBuf::from(std::env::var("REPLAYER_NATIVE_FIXTURES").unwrap())
            .join("audio.wav");
        let cancel = Arc::new(AtomicBool::new(false));
        let mut audio = AudioChunks::open(&path, cancel.clone(), 1).unwrap();
        audio.set_range(0.333, Some(1.456)).unwrap();
        let mut samples = Vec::new();
        while let Some(chunk) = audio.next().unwrap() {
            samples.extend(chunk.samples);
        }
        assert_eq!(samples.len(), 17968);
        let chunk = Chunk {
            start: 0.333,
            samples,
        };
        assert_eq!(chunk.wav().len(), 44 + 17968 * 2);
        let mp3 = super::super::mp3::encode(&chunk).unwrap();
        assert!(mp3.len() < chunk.wav().len());
        cancel.store(true, Ordering::Release);
        assert!(audio.next().is_err());
    }
}
