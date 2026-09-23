use anyhow::{Context, Result, ensure};
use ffmpeg::{ChannelLayout, codec, format, frame, media, software};
use ffmpeg_next as ffmpeg;
use std::{
    collections::VecDeque,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Instant,
};

pub const RATE: u32 = 16_000;
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
        for &sample in &self.samples {
            out.extend_from_slice(
                &((sample.clamp(-1.0, 1.0) * 32767.0).round() as i16).to_le_bytes(),
            );
        }
        out
    }
}
/// Independent, bounded offline decoding with reusable range seeking.
pub struct AudioChunks {
    input: format::context::Input,
    dec: ffmpeg::decoder::Audio,
    resampler: Option<software::resampling::Context>,
    index: usize,
    tb: f64,
    origin: f64,
    pub duration: f64,
    max_samples: usize,
    pending: VecDeque<f32>,
    pending_start: f64,
    output_end: f64,
    next_input: f64,
    deferred: Option<Chunk>,
    boundary: bool,
    input_eof: bool,
    decoder_eof: bool,
    done: bool,
    cancel: Arc<AtomicBool>,
    deadline: Arc<AtomicU64>,
    base: Instant,
    range_start: f64,
    range_end: Option<f64>,
}
impl AudioChunks {
    pub fn open(path: &Path, cancel: Arc<AtomicBool>, seconds: usize) -> Result<Self> {
        ffmpeg::init()?;
        let base = Instant::now();
        let deadline = Arc::new(AtomicU64::new(30_000));
        let limit = deadline.clone();
        let cancelled = cancel.clone();
        let input = format::input_with_interrupt(path, move || {
            cancelled.load(Ordering::Acquire)
                || base.elapsed().as_millis() as u64 > limit.load(Ordering::Relaxed)
        })
        .context("读取字幕源文件失败")?;
        let track = input
            .streams()
            .best(media::Type::Audio)
            .context("当前文件没有音轨，无法生成字幕")?;
        let index = track.index();
        let tb = f64::from(track.time_base());
        let mut dec = codec::context::Context::from_parameters(track.parameters())?
            .decoder()
            .audio()?;
        dec.set_packet_time_base(track.time_base());
        // Same timeline origin as the playback demuxer.
        let start = unsafe { (*input.as_ptr()).start_time };
        let origin = if start == ffmpeg::ffi::AV_NOPTS_VALUE {
            0.0
        } else {
            start as f64 / 1e6
        };
        let duration = (input.duration() as f64 / 1e6).max(0.0);
        Ok(Self {
            input,
            dec,
            resampler: None,
            index,
            tb,
            origin,
            duration,
            max_samples: seconds * RATE as usize,
            pending: VecDeque::new(),
            pending_start: 0.0,
            output_end: 0.0,
            next_input: 0.0,
            deferred: None,
            boundary: false,
            input_eof: false,
            decoder_eof: false,
            done: false,
            cancel,
            deadline,
            base,
            range_start: 0.0,
            range_end: None,
        })
    }
    pub fn set_range(&mut self, start: f64, end: Option<f64>) -> Result<()> {
        ensure!(
            start.is_finite() && start >= 0.0 && end.is_none_or(|e| e.is_finite() && e > start),
            "invalid subtitle range"
        );
        self.range_start = start;
        self.range_end = end;
        {
            self.deadline.store(
                self.base.elapsed().as_millis() as u64 + 10_000,
                Ordering::Relaxed,
            );
            let ts = ((start + self.origin) * 1e6) as i64;
            unsafe {
                let io = (*self.input.as_mut_ptr()).pb;
                if !io.is_null() {
                    (*io).error = 0;
                    (*io).eof_reached = 0;
                }
            }
            self.input.seek(ts, ..ts)?;
            self.dec.flush();
            self.resampler = None;
            self.pending.clear();
            self.next_input = start;
            self.output_end = start;
            self.pending_start = start;
            self.deferred = None;
            self.boundary = false;
            self.input_eof = false;
            self.decoder_eof = false;
            self.done = false;
        }
        Ok(())
    }
    pub fn next(&mut self) -> Result<Option<Chunk>> {
        ensure!(!self.cancel.load(Ordering::Acquire), "字幕生成已取消");
        if self.pending.is_empty()
            && let Some(chunk) = self.deferred.take()
        {
            self.pending_start = chunk.start;
            self.pending.extend(chunk.samples);
            self.boundary = false;
        }
        while self.pending.len() < self.max_samples && !self.done && !self.boundary {
            self.read_more()?;
        }
        if self.pending.is_empty() {
            return Ok(None);
        }
        let mut count = self.pending.len().min(self.max_samples);
        // Prefer a quiet 40ms window in the last two seconds to avoid splitting a word.
        // Scheduled ranges already have a fixed boundary. Splitting them at a
        // quiet point would create a second tiny request and delay publication
        // of the entire interval until that request also finishes.
        if count == self.max_samples && self.range_end.is_none() {
            let window = RATE as usize / 25;
            let lower = count.saturating_sub(2 * RATE as usize).max(window);
            for end in (lower..=count).rev().step_by(window) {
                let energy = self
                    .pending
                    .range(end - window..end)
                    .map(|s| s * s)
                    .sum::<f32>()
                    / window as f32;
                if energy < 0.000025 {
                    count = end;
                    break;
                }
            }
        }
        let start = self.pending_start;
        let samples = self.pending.drain(..count).collect();
        self.pending_start += count as f64 / RATE as f64;
        Ok(Some(Chunk { start, samples }))
    }
    fn read_more(&mut self) -> Result<()> {
        loop {
            ensure!(!self.cancel.load(Ordering::Acquire), "字幕生成已取消");
            if self.decoder_eof {
                let Some(resampler) = &mut self.resampler else {
                    self.done = true;
                    return Ok(());
                };
                let mut output = frame::Audio::new(
                    format::Sample::F32(format::sample::Type::Packed),
                    4096,
                    ChannelLayout::MONO,
                );
                resampler.flush(&mut output)?;
                if output.samples() == 0 {
                    self.done = true;
                } else {
                    self.append(self.output_end, &output);
                }
                return Ok(());
            }
            let mut decoded = frame::Audio::empty();
            match self.dec.receive_frame(&mut decoded) {
                Ok(()) => {
                    if decoded.channel_layout().is_empty() {
                        decoded
                            .set_channel_layout(ChannelLayout::default(decoded.channels() as i32));
                    }
                    // Corrupt frames can arrive with new parameters; rebuild instead
                    // of letting swr fail with INPUT_CHANGED.
                    if let Some(resampler) = &self.resampler {
                        let current = *resampler.input();
                        if current.format != decoded.format()
                            || current.rate != decoded.rate()
                            || current.channel_layout != decoded.channel_layout()
                        {
                            self.resampler = None;
                        }
                    }
                    if self.resampler.is_none() {
                        self.resampler = Some(software::resampling::Context::get(
                            decoded.format(),
                            decoded.channel_layout(),
                            decoded.rate(),
                            format::Sample::F32(format::sample::Type::Packed),
                            ChannelLayout::MONO,
                            RATE,
                        )?);
                    }
                    let resampler = self.resampler.as_mut().unwrap();
                    let pts = decoded
                        .timestamp()
                        .or(decoded.pts())
                        .map_or(self.next_input, |p| p as f64 * self.tb - self.origin);
                    self.next_input = pts + decoded.samples() as f64 / decoded.rate().max(1) as f64;
                    let delay = resampler.delay().map_or(0, |d| d.output.max(0) as usize);
                    let count = (decoded.samples() as f64 * RATE as f64
                        / decoded.rate().max(1) as f64)
                        .ceil() as usize
                        + delay
                        + 256;
                    let mut output = frame::Audio::new(
                        format::Sample::F32(format::sample::Type::Packed),
                        count,
                        ChannelLayout::MONO,
                    );
                    resampler
                        .run(&decoded, &mut output)
                        .context("字幕音频重采样失败")?;
                    if output.samples() > 0 {
                        self.append(pts - delay as f64 / RATE as f64, &output);
                    }
                    return Ok(());
                }
                Err(ffmpeg::Error::Eof) => {
                    self.decoder_eof = true;
                    continue;
                }
                Err(ffmpeg::Error::Other { errno }) if errno == ffmpeg::error::EAGAIN => {}
                // Corrupt frames are skipped; during drain the decoder still reaches EOF.
                Err(ffmpeg::Error::InvalidData) if self.input_eof => continue,
                Err(ffmpeg::Error::InvalidData) => {}
                Err(e) => return Err(e).context("字幕音轨解码失败"),
            }
            ensure!(!self.input_eof, "音频解码器在 EOF 后未完成排空");
            loop {
                ensure!(!self.cancel.load(Ordering::Acquire), "字幕生成已取消");
                self.deadline.store(
                    self.base.elapsed().as_millis() as u64 + 10_000,
                    Ordering::Relaxed,
                );
                let mut packet = ffmpeg::Packet::empty();
                match packet.read(&mut self.input) {
                    Ok(()) if packet.stream() == self.index => {
                        match self.dec.send_packet(&packet) {
                            Ok(()) | Err(ffmpeg::Error::InvalidData) => break,
                            Err(e) => return Err(e).context("字幕音轨解码失败"),
                        }
                    }
                    Ok(()) | Err(ffmpeg::Error::InvalidData) => continue,
                    Err(ffmpeg::Error::Eof) => {
                        self.input_eof = true;
                        self.dec.send_eof()?;
                        break;
                    }
                    Err(e) => return Err(e).context("读取字幕音轨失败或已取消"),
                }
            }
        }
    }
    fn append(&mut self, mut start: f64, frame: &frame::Audio) {
        let floats: &[f32] = bytemuck::cast_slice(&frame.data(0)[..frame.samples() * 4]);
        let skip = ((self.range_start - start).max(0.0) * RATE as f64).ceil() as usize;
        if skip >= floats.len() {
            return;
        }
        start += skip as f64 / RATE as f64;
        let mut samples = &floats[skip..];
        if let Some(end) = self.range_end {
            let remaining = ((end - start).max(0.0) * RATE as f64).floor() as usize;
            if samples.len() >= remaining {
                samples = &samples[..remaining];
                self.done = true;
            }
        }
        if samples.is_empty() {
            return;
        }
        self.output_end = start + samples.len() as f64 / RATE as f64;
        let expected = self.pending_start + self.pending.len() as f64 / RATE as f64;
        if !self.pending.is_empty() && (start - expected).abs() > 0.05 {
            // Keep timestamp gaps without allocating minutes of silent PCM.
            self.deferred = Some(Chunk {
                start,
                samples: samples.to_vec(),
            });
            self.boundary = true;
        } else {
            if self.pending.is_empty() {
                self.pending_start = start;
            }
            self.pending.extend(samples.iter().copied());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_range_does_not_create_a_second_request_for_a_short_tail() {
        let mut samples = vec![0.2; RATE as usize * 5];
        samples[RATE as usize * 3..RATE as usize * 4].fill(0.0);
        let path =
            std::env::temp_dir().join(format!("replayer-bounded-audio-{}.wav", std::process::id()));
        std::fs::write(
            &path,
            Chunk {
                start: 0.0,
                samples,
            }
            .wav(),
        )
        .unwrap();
        let mut reader = AudioChunks::open(&path, Arc::new(AtomicBool::new(false)), 5).unwrap();
        reader.set_range(0.0, Some(5.0)).unwrap();
        let chunk = reader.next().unwrap().unwrap();
        assert_eq!(chunk.samples.len(), RATE as usize * 5);
        assert!(reader.next().unwrap().is_none());
        drop(reader);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn wave_roundtrip_is_bounded_and_has_no_lost_tail() {
        let input = Chunk {
            start: 0.0,
            samples: vec![0.2; RATE as usize * 3],
        };
        let path =
            std::env::temp_dir().join(format!("replayer-audio-test-{}.wav", std::process::id()));
        std::fs::write(&path, input.wav()).unwrap();
        let mut reader = AudioChunks::open(&path, Arc::new(AtomicBool::new(false)), 2).unwrap();
        let mut total = 0;
        while let Some(chunk) = reader.next().unwrap() {
            assert!((chunk.start - total as f64 / RATE as f64).abs() < 0.002);
            assert!(chunk.samples.len() <= RATE as usize * 2);
            total += chunk.samples.len();
        }
        assert_eq!(total, RATE as usize * 3);
        // Reuse after EOF, seek backwards to zero, and recover after a clipped range.
        for (start, end) in [(2.0, 3.0), (0.0, 1.0), (1.0, 2.0)] {
            reader.set_range(start, Some(end)).unwrap();
            let mut samples = 0;
            while let Some(chunk) = reader.next().unwrap() {
                assert!((chunk.start - start - samples as f64 / RATE as f64).abs() < 0.002);
                samples += chunk.samples.len();
            }
            assert_eq!(samples, RATE as usize);
        }
        drop(reader);
        std::fs::remove_file(path).unwrap();
    }
}
