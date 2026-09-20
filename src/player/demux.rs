use anyhow::{Context, Result, bail};
use crossbeam_channel::{Receiver, SendTimeoutError, Sender, bounded, unbounded};
use ffmpeg::{codec, format, media};
use ffmpeg_next as ffmpeg;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

pub const PACKET_BUDGET: usize = 16 * 1024 * 1024;
pub struct Track {
    pub index: usize,
    pub params: codec::Parameters,
    pub tb: f64,
}
pub struct Header {
    pub duration: f64,
    pub origin: f64,
    pub video: Option<Track>,
    pub audio: Option<Track>,
    pub info: Arc<crate::media::Info>,
}
pub enum Message {
    Opened(Header),
    Packet(u64, ffmpeg::Packet),
    Seeked(u64),
    Eof(u64),
    Error(String),
}
pub struct Seek {
    pub epoch: u64,
    pub target: f64,
}
pub struct Demux {
    pub rx: Receiver<Message>,
    pub seeks: Sender<Seek>,
    pub waiting: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Demux {
    pub fn spawn(
        path: String,
        stop: Arc<AtomicBool>,
        requested: Arc<AtomicU64>,
        progressive: bool,
        source_key: Option<String>,
    ) -> Result<Self> {
        let (tx, rx) = bounded(4);
        let (seeks, commands) = unbounded();
        let cancelled = stop.clone();
        let waiting = Arc::new(AtomicBool::new(false));
        let worker_waiting = waiting.clone();
        let thread = std::thread::Builder::new()
            .name("replayer-demux".into())
            .spawn(move || {
                if let Err(e) = run(
                    path,
                    &tx,
                    commands,
                    &cancelled,
                    requested,
                    progressive,
                    &worker_waiting,
                    source_key,
                ) {
                    let _ = send(&tx, Message::Error(format!("{e:#}")), &cancelled);
                }
            })?;
        Ok(Self {
            rx,
            seeks,
            waiting,
            stop,
            thread: Some(thread),
        })
    }
}
impl Drop for Demux {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
fn send(tx: &Sender<Message>, mut message: Message, stop: &AtomicBool) -> bool {
    while !stop.load(Ordering::Acquire) {
        match tx.send_timeout(message, Duration::from_millis(5)) {
            Ok(()) => return true,
            Err(SendTimeoutError::Timeout(m)) => message = m,
            Err(SendTimeoutError::Disconnected(_)) => break,
        }
    }
    false
}
#[allow(clippy::too_many_arguments)]
fn run(
    path: String,
    tx: &Sender<Message>,
    seeks: Receiver<Seek>,
    stop: &Arc<AtomicBool>,
    requested: Arc<AtomicU64>,
    progressive: bool,
    waiting: &AtomicBool,
    source_key: Option<String>,
) -> Result<()> {
    // Keep the prepared source alive until FFmpeg closes its file handle.
    let prepared = crate::media_source::prepare(&path, stop, source_key.as_deref())?;
    let playback_path = prepared.path();
    ffmpeg::init()?;
    let base = Instant::now();
    let deadline = Arc::new(AtomicU64::new(if progressive { 300_000 } else { 30_000 }));
    let limit = deadline.clone();
    let cancelled = stop.clone();
    let active_epoch = Arc::new(AtomicU64::new(1));
    let observed = active_epoch.clone();
    let latest = requested.clone();
    let interrupt_seeks = Arc::new(AtomicBool::new(false));
    let interrupt = interrupt_seeks.clone();
    let mut options = ffmpeg::Dictionary::new();
    if progressive {
        options.set("probesize", "2097152");
        options.set("analyzeduration", "2000000");
        options.set("max_probe_packets", "512");
    }
    let mut input = format::input_with_interrupt_and_dictionary(
        &playback_path,
        move || {
            cancelled.load(Ordering::Acquire)
                || (interrupt.load(Ordering::Acquire)
                    && latest.load(Ordering::Acquire) > observed.load(Ordering::Acquire))
                || base.elapsed().as_millis() as u64 > limit.load(Ordering::Relaxed)
        },
        options,
    )
    .with_context(|| format!("open media '{path}'"))?;
    let video = input
        .streams()
        .filter(|stream| {
            stream.parameters().medium() == media::Type::Video
                && !stream
                    .disposition()
                    .contains(format::stream::Disposition::ATTACHED_PIC)
        })
        .max_by_key(|s| {
            s.disposition()
                .contains(format::stream::Disposition::DEFAULT)
        })
        .map(|s| Track {
            index: s.index(),
            tb: f64::from(s.time_base()),
            params: s.parameters().clone(),
        });
    let audio = input.streams().best(media::Type::Audio).map(|s| Track {
        index: s.index(),
        tb: f64::from(s.time_base()),
        params: s.parameters().clone(),
    });
    anyhow::ensure!(
        video.is_some() || audio.is_some(),
        "no playable audio or video stream"
    );
    let tag = |name: &str| {
        input
            .metadata()
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.to_owned())
            .or_else(|| {
                input.streams().best(media::Type::Audio).and_then(|s| {
                    s.metadata()
                        .iter()
                        .find(|(key, _)| key.eq_ignore_ascii_case(name))
                        .map(|(_, v)| v.to_owned())
                })
            })
            .filter(|s| !s.trim().is_empty())
    };
    let artwork = if video.is_none() {
        input
            .streams()
            .filter(|s| {
                s.disposition()
                    .contains(format::stream::Disposition::ATTACHED_PIC)
            })
            .find_map(|s| unsafe {
                let packet = &(*s.as_ptr()).attached_pic;
                if packet.data.is_null() || packet.size <= 0 || packet.size > 8 * 1024 * 1024 {
                    return None;
                }
                crate::media::decode_artwork(std::slice::from_raw_parts(
                    packet.data,
                    packet.size as usize,
                ))
            })
    } else {
        None
    };
    let info = Arc::new(crate::media::Info {
        kind: if video.is_some() {
            crate::media::Kind::Video
        } else {
            crate::media::Kind::Music
        },
        title: tag("title"),
        artist: tag("artist"),
        album: tag("album"),
        artwork,
    });
    // FFmpeg exposes format start_time only through its FFI context.
    let start = unsafe { (*input.as_ptr()).start_time };
    let origin = if start == ffmpeg::ffi::AV_NOPTS_VALUE {
        0.0
    } else {
        start as f64 / 1e6
    };
    let duration = (input.duration() as f64 / 1e6).max(0.0);
    let vi = video.as_ref().map(|v| v.index);
    let ai = audio.as_ref().map(|a| a.index);
    if !send(
        tx,
        Message::Opened(Header {
            duration,
            origin,
            video,
            audio,
            info,
        }),
        stop,
    ) {
        return Ok(());
    }
    let mut epoch = 1;
    interrupt_seeks.store(true, Ordering::Release);
    let mut eof = false;
    while !stop.load(Ordering::Acquire) {
        let mut seek = None;
        while let Ok(s) = seeks.try_recv() {
            seek = Some(s);
        }
        if let Some(s) = seek {
            active_epoch.store(s.epoch, Ordering::Release);
            deadline.store(
                base.elapsed().as_millis() as u64 + if progressive { 300_000 } else { 10_000 },
                Ordering::Relaxed,
            );
            let ts = ((s.target + origin) * 1e6) as i64;
            clear_io_error(&mut input);
            waiting.store(true, Ordering::Release);
            let result = input.seek(ts, ..ts);
            waiting.store(false, Ordering::Release);
            if requested.load(Ordering::Acquire) > s.epoch {
                continue;
            }
            result.context("seek input")?;
            epoch = s.epoch;
            eof = false;
            if !send(tx, Message::Seeked(epoch), stop) {
                break;
            }
        }
        if requested.load(Ordering::Acquire) > epoch {
            std::thread::sleep(Duration::from_millis(1));
            continue;
        }
        if eof {
            std::thread::sleep(Duration::from_millis(5));
            continue;
        }
        deadline.store(
            base.elapsed().as_millis() as u64 + if progressive { 300_000 } else { 10_000 },
            Ordering::Relaxed,
        );
        let mut packet = ffmpeg::Packet::empty();
        waiting.store(true, Ordering::Release);
        let result = packet.read(&mut input);
        waiting.store(false, Ordering::Release);
        if requested.load(Ordering::Acquire) > epoch {
            clear_io_error(&mut input);
            continue;
        }
        match result {
            Ok(()) => {
                if Some(packet.stream()) != vi && Some(packet.stream()) != ai {
                    continue;
                }
                if packet.size() > PACKET_BUDGET {
                    bail!("single packet exceeds 16 MiB input budget");
                }
                if !send(tx, Message::Packet(epoch, packet), stop) {
                    break;
                }
            }
            Err(ffmpeg::Error::Eof) => {
                eof = true;
                if !send(tx, Message::Eof(epoch), stop) {
                    break;
                }
            }
            Err(ffmpeg::Error::InvalidData) => continue,
            Err(e) => {
                return Err(e).context("read media packet (cancelled, timed out, or I/O failure)");
            }
        }
    }
    Ok(())
}
fn clear_io_error(input: &mut format::context::Input) {
    unsafe {
        let io = (*input.as_mut_ptr()).pb;
        if !io.is_null() {
            (*io).error = 0;
            (*io).eof_reached = 0;
        }
    }
}
