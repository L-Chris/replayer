use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use super::clock::Clock;
use anyhow::Result;
#[cfg(not(test))]
use anyhow::{Context, bail};
#[cfg(not(test))]
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

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
    pub pending: VecDeque<Sample>,
    pub end: f64,
    pub submitted_end: Option<f64>,
    rate: u32,
    queued_until: f64,
    discard_before: f64,
}
impl AudioPipeline {
    pub fn new(rate: u32) -> Self {
        Self {
            pending: VecDeque::new(),
            end: 0.,
            submitted_end: None,
            rate,
            queued_until: 0.,
            discard_before: 0.,
        }
    }
    pub fn reset(&mut self, target: f64) {
        self.pending.clear();
        self.end = target;
        self.submitted_end = None;
        self.queued_until = target;
        self.discard_before = target;
    }
    pub fn append(&mut self, frame: &remdia::Frame, epoch: u64, target: f64) -> Result<()> {
        let remdia::FrameInfo::Audio {
            pts_us,
            rate,
            channels,
            samples,
            ..
        } = *frame.info()
        else {
            return Ok(());
        };
        anyhow::ensure!(
            rate == self.rate && channels == 2,
            "remdia/device PCM format mismatch"
        );
        let pts = pts_us as f64 / 1e6;
        let skip = ((target.max(self.discard_before) - pts).max(0.) * rate as f64).ceil() as usize;
        anyhow::ensure!(
            self.pending.len() + samples <= self.rate as usize * 2,
            "decoded audio exceeded two-second pending budget"
        );
        for (i, pair) in frame
            .bytes()
            .as_chunks::<8>()
            .0
            .iter()
            .enumerate()
            .skip(skip)
        {
            let left = f32::from_le_bytes(pair[..4].try_into().unwrap());
            let right = f32::from_le_bytes(pair[4..].try_into().unwrap());
            self.pending.push_back(Sample {
                epoch,
                pts: pts + i as f64 / rate as f64,
                pcm: [left, right],
            });
        }
        if samples > skip {
            self.end = pts + samples as f64 / rate as f64;
        }
        Ok(())
    }
    pub fn pump(&mut self, out: &mut AudioOut) {
        for _ in 0..out.producer.slots() {
            let Some(next) = self.pending.front() else {
                break;
            };
            let sample = if next.pts > self.queued_until + 1.5 / self.rate as f64 {
                Sample {
                    epoch: next.epoch,
                    pts: self.queued_until,
                    pcm: [0.; 2],
                }
            } else {
                self.pending.pop_front().unwrap()
            };
            self.queued_until = sample.pts + 1. / self.rate as f64;
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
