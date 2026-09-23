use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use super::clock::Clock;
use anyhow::Result;
#[cfg(not(test))]
use anyhow::{Context, bail};
#[cfg(not(test))]
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ffmpeg::{ChannelLayout, codec, format, frame, software};
use ffmpeg_next as ffmpeg;
use rtrb::{Producer, RingBuffer};

#[derive(Clone, Copy)]
pub struct Sample {
    pub epoch: u64,
    pub pts: f64,
    pub pcm: [f32; 2],
}

/// Control is read only by the callback. A generation change invalidates queued audio.
pub struct OutputControl {
    pub epoch: AtomicU64,
    pub enabled: AtomicBool,
    pub failed: AtomicBool,
    pub underruns: AtomicU64,
}
impl OutputControl {
    pub fn new() -> Self {
        Self {
            epoch: AtomicU64::new(1),
            enabled: AtomicBool::new(false),
            failed: AtomicBool::new(false),
            underruns: AtomicU64::new(0),
        }
    }
}
pub struct AudioOut {
    pub rate: u32,
    pub producer: Producer<Sample>,
    pub capacity: usize,
    _stream: Option<cpal::Stream>,
    #[cfg(test)]
    simulation_stop: Arc<AtomicBool>,
}
#[cfg(not(test))]
pub fn open_output(
    clock: Arc<Clock>,
    control: Arc<OutputControl>,
    volume: Arc<AtomicU32>,
) -> Result<AudioOut> {
    let device = cpal::default_host()
        .default_output_device()
        .context("no output device")?;
    let supported = device.default_output_config()?;
    let rate = supported.sample_rate();
    let channels = supported.channels() as usize;
    let config = supported.config();
    let capacity = (rate as usize / 2).max(1024);
    let (producer, consumer) = RingBuffer::<Sample>::new(capacity);
    let err_control = control.clone();
    let err_cb = move |_error| {
        err_control.failed.store(true, Ordering::Release);
    };

    macro_rules! build {
        ($ty:ty) => {{
            let mut consumer = consumer;
            device.build_output_stream(
                config,
                move |data: &mut [$ty], info: &cpal::OutputCallbackInfo| {
                    use cpal::Sample as _;
                    let epoch = control.epoch.load(Ordering::Acquire);
                    // Only stale generations are discarded; pause retains current PCM.
                    for _ in 0..capacity {
                        if consumer.peek().is_ok_and(|s| s.epoch < epoch) {
                            let _ = consumer.pop();
                        } else {
                            break;
                        }
                    }
                    let enabled = control.enabled.load(Ordering::Acquire);
                    let gain = f32::from_bits(volume.load(Ordering::Relaxed));
                    let mut last = None;
                    let mut produced = 0;
                    for (i, dst) in data.chunks_mut(channels).enumerate() {
                        let sample = if enabled && consumer.peek().is_ok_and(|s| s.epoch == epoch) {
                            consumer.pop().ok()
                        } else {
                            None
                        };
                        if let Some(s) = sample {
                            for (ch, v) in dst.iter_mut().enumerate() {
                                let value = if channels == 1 {
                                    (s.pcm[0] + s.pcm[1]) * 0.5
                                } else if ch < 2 {
                                    s.pcm[ch]
                                } else {
                                    0.0
                                };
                                *v = value.mul_add(gain, 0.0).clamp(-1.0, 1.0).to_sample::<$ty>();
                            }
                            last = Some((s.pts + 1.0 / rate as f64, i + 1));
                            produced += 1;
                        } else {
                            dst.fill(<$ty as cpal::Sample>::EQUILIBRIUM);
                        }
                    }
                    if enabled && produced < data.len() / channels {
                        control.underruns.fetch_add(1, Ordering::Relaxed);
                    }
                    if let Some((end, offset)) = last {
                        let ts = info.timestamp();
                        let latency = ts
                            .playback
                            .as_nanos()
                            .saturating_sub(ts.callback.as_nanos())
                            as f64
                            / 1e9;
                        clock.progress.publish(
                            epoch,
                            end,
                            clock.wall() + latency + offset as f64 / rate as f64,
                        );
                    }
                },
                err_cb,
                None,
            )?
        }};
    }
    let stream = match supported.sample_format() {
        cpal::SampleFormat::F32 => build!(f32),
        cpal::SampleFormat::I16 => build!(i16),
        cpal::SampleFormat::U16 => build!(u16),
        cpal::SampleFormat::I32 => build!(i32),
        cpal::SampleFormat::F64 => build!(f64),
        other => bail!("unsupported audio device format: {other}"),
    };
    stream.play()?;
    Ok(AudioOut {
        rate,
        producer,
        capacity,
        _stream: Some(stream),
    })
}
#[cfg(test)]
pub fn open_output(
    clock: Arc<Clock>,
    control: Arc<OutputControl>,
    _volume: Arc<AtomicU32>,
) -> Result<AudioOut> {
    let rate = 48000;
    let capacity = 24000;
    let (producer, mut consumer) = RingBuffer::<Sample>::new(capacity);
    let stop = Arc::new(AtomicBool::new(false));
    let worker_stop = stop.clone();
    std::thread::spawn(move || {
        while !worker_stop.load(Ordering::Acquire) {
            let epoch = control.epoch.load(Ordering::Acquire);
            while consumer.peek().is_ok_and(|s| s.epoch < epoch) {
                let _ = consumer.pop();
            }
            if control.enabled.load(Ordering::Acquire) {
                let mut last = None;
                for _ in 0..240 {
                    if consumer.peek().is_ok_and(|s| s.epoch == epoch) {
                        last = consumer.pop().ok();
                    } else {
                        break;
                    }
                }
                if let Some(sample) = last {
                    assert!(sample.pcm.iter().all(|v| v.is_finite()));
                    clock.progress.publish(
                        epoch,
                        sample.pts + 1.0 / rate as f64,
                        clock.wall() + 0.005,
                    );
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    });
    Ok(AudioOut {
        rate,
        capacity,
        producer,
        _stream: None,
        simulation_stop: stop,
    })
}
#[cfg(test)]
impl Drop for AudioOut {
    fn drop(&mut self) {
        self.simulation_stop.store(true, Ordering::Release);
    }
}

pub struct AudioPipeline {
    dec: ffmpeg::decoder::Audio,
    resampler: software::resampling::Context,
    tb: f64,
    origin: f64,
    rate: u32,
    next_pts: Option<f64>,
    pub pending: VecDeque<Sample>,
    pub end: f64,
    pub submitted_end: Option<f64>,
    queued_until: f64,
    discard_before: f64,
}
impl AudioPipeline {
    pub fn new(params: codec::Parameters, tb: f64, origin: f64, rate: u32) -> Result<Self> {
        let mut dec = codec::context::Context::from_parameters(params)?
            .decoder()
            .audio()?;
        dec.set_packet_time_base(ffmpeg::Rational::from(tb));
        let layout = if !dec.channel_layout().is_empty() {
            dec.channel_layout()
        } else {
            ChannelLayout::default(dec.channels() as i32)
        };
        let resampler = software::resampling::Context::get(
            dec.format(),
            layout,
            dec.rate(),
            format::Sample::F32(format::sample::Type::Packed),
            ChannelLayout::STEREO,
            rate,
        )?;
        Ok(Self {
            dec,
            resampler,
            tb,
            origin,
            rate,
            next_pts: None,
            pending: VecDeque::new(),
            end: 0.0,
            submitted_end: None,
            queued_until: 0.0,
            discard_before: 0.0,
        })
    }
    pub fn reset(&mut self, target: f64) -> Result<()> {
        self.dec.flush();
        let input = *self.resampler.input();
        self.resampler = software::resampling::Context::get(
            input.format,
            input.channel_layout,
            input.rate,
            format::Sample::F32(format::sample::Type::Packed),
            ChannelLayout::STEREO,
            self.rate,
        )?;
        self.pending.clear();
        self.next_pts = None;
        self.end = target;
        self.submitted_end = None;
        self.queued_until = target;
        self.discard_before = target;
        Ok(())
    }
    pub fn decode(
        &mut self,
        packet: Option<&ffmpeg::Packet>,
        epoch: u64,
        target: f64,
    ) -> Result<()> {
        let sent = match packet {
            Some(p) => self.dec.send_packet(p),
            None => self.dec.send_eof(),
        };
        if let Err(e) = sent {
            if super::again(e) {
                self.receive(epoch, target)?;
                match packet {
                    Some(p) => self.dec.send_packet(p)?,
                    None => self.dec.send_eof()?,
                }
            } else if packet.is_none() || e != ffmpeg::Error::InvalidData {
                // A corrupt packet is skipped; the decoder resyncs downstream.
                return Err(e.into());
            }
        }
        self.receive(epoch, target)?;
        if packet.is_none() {
            loop {
                let mut output = self.output(4096);
                self.resampler.flush(&mut output)?;
                if output.samples() == 0 {
                    break;
                }
                self.append(&output, epoch, target);
            }
        }
        Ok(())
    }
    fn output(&self, samples: usize) -> frame::Audio {
        frame::Audio::new(
            format::Sample::F32(format::sample::Type::Packed),
            samples,
            ChannelLayout::STEREO,
        )
    }
    /// Corrupt streams can decode a frame with new parameters; swr would fail
    /// with INPUT_CHANGED, so rebuild the resampler around the change instead.
    fn match_resampler(&mut self, input: &frame::Audio) -> Result<()> {
        let current = *self.resampler.input();
        if current.format == input.format()
            && current.rate == input.rate()
            && current.channel_layout == input.channel_layout()
        {
            return Ok(());
        }
        self.resampler = software::resampling::Context::get(
            input.format(),
            input.channel_layout(),
            input.rate(),
            format::Sample::F32(format::sample::Type::Packed),
            ChannelLayout::STEREO,
            self.rate,
        )?;
        Ok(())
    }
    fn receive(&mut self, epoch: u64, target: f64) -> Result<()> {
        let mut input = frame::Audio::empty();
        loop {
            match self.dec.receive_frame(&mut input) {
                Ok(()) => {}
                Err(e) if super::again(e) || e == ffmpeg::Error::Eof => break,
                Err(ffmpeg::Error::InvalidData) => continue,
                Err(e) => return Err(e.into()),
            }
            if input.channel_layout().is_empty() {
                input.set_channel_layout(ChannelLayout::default(input.channels() as i32));
            }
            self.match_resampler(&input)?;
            let pts = input
                .timestamp()
                .or(input.pts())
                .map(|p| p as f64 * self.tb - self.origin);
            // Preserve normal resampler delay, but follow genuine timestamp discontinuities.
            if self.next_pts.is_none()
                || pts
                    .zip(self.next_pts)
                    .is_some_and(|(a, b)| (a - b).abs() > 0.1)
            {
                self.next_pts = Some(pts.unwrap_or(target));
            }
            let delay = self
                .resampler
                .delay()
                .map_or(0, |d| d.output.max(0) as usize);
            let count = (input.samples() as f64 * self.rate as f64 / input.rate().max(1) as f64)
                .ceil() as usize
                + delay
                + 256;
            let mut output = self.output(count);
            self.resampler.run(&input, &mut output)?;
            self.append(&output, epoch, target);
        }
        Ok(())
    }
    fn append(&mut self, output: &frame::Audio, epoch: u64, target: f64) {
        let target = target.max(self.discard_before);
        let pts = self.next_pts.unwrap_or(target);
        let n = output.samples();
        self.next_pts = Some(pts + n as f64 / self.rate as f64);
        let bytes = &output.data(0)[..n * 8];
        let pcm: &[f32] = bytemuck::cast_slice(bytes);
        let skip = ((target - pts).max(0.0) * self.rate as f64).ceil() as usize;
        for (i, pair) in pcm.as_chunks::<2>().0.iter().enumerate().skip(skip) {
            self.pending.push_back(Sample {
                epoch,
                pts: pts + i as f64 / self.rate as f64,
                pcm: [pair[0], pair[1]],
            });
        }
        if n > skip {
            self.end = self.next_pts.unwrap();
        }
    }
    pub fn pump(&mut self, out: &mut AudioOut) {
        for _ in 0..out.producer.slots() {
            let Some(next) = self.pending.front() else {
                break;
            };
            // Timestamp gaps become bounded, lazily generated silence. No huge
            // allocation when an audio track starts minutes after the video.
            let sample = if next.pts > self.queued_until + 1.5 / self.rate as f64 {
                Sample {
                    epoch: next.epoch,
                    pts: self.queued_until,
                    pcm: [0.0; 2],
                }
            } else {
                self.pending.pop_front().unwrap()
            };
            self.queued_until = sample.pts + 1.0 / self.rate as f64;
            let result = out.producer.push(sample);
            debug_assert!(result.is_ok());
            self.submitted_end = Some(self.queued_until);
        }
    }
    pub fn catch_up(&mut self, position: f64) {
        self.discard_before = position;
        self.queued_until = position;
        while self.pending.front().is_some_and(|s| s.pts < position) {
            self.pending.pop_front();
        }
    }
}
