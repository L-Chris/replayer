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
    pub video: Track,
    pub audio: Option<Track>,
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
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Demux {
    pub fn spawn(path: String, stop: Arc<AtomicBool>) -> Result<Self> {
        let (tx, rx) = bounded(4);
        let (seeks, commands) = unbounded();
        let cancelled = stop.clone();
        let thread = std::thread::Builder::new()
            .name("replayer-demux".into())
            .spawn(move || {
                if let Err(e) = run(path, &tx, commands, &cancelled) {
                    let _ = send(&tx, Message::Error(format!("{e:#}")), &cancelled);
                }
            })?;
        Ok(Self {
            rx,
            seeks,
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
fn run(
    path: String,
    tx: &Sender<Message>,
    seeks: Receiver<Seek>,
    stop: &Arc<AtomicBool>,
) -> Result<()> {
    ffmpeg::init()?;
    let base = Instant::now();
    let deadline = Arc::new(AtomicU64::new(30_000));
    let limit = deadline.clone();
    let cancelled = stop.clone();
    let mut input = format::input_with_interrupt(&path, move || {
        cancelled.load(Ordering::Acquire)
            || base.elapsed().as_millis() as u64 > limit.load(Ordering::Relaxed)
    })
    .with_context(|| format!("open media '{path}'"))?;
    let video = input
        .streams()
        .best(media::Type::Video)
        .context("no video stream")?;
    let video = Track {
        index: video.index(),
        tb: f64::from(video.time_base()),
        params: video.parameters().clone(),
    };
    let audio = input.streams().best(media::Type::Audio).map(|s| Track {
        index: s.index(),
        tb: f64::from(s.time_base()),
        params: s.parameters().clone(),
    });
    // FFmpeg exposes format start_time only through its FFI context.
    let start = unsafe { (*input.as_ptr()).start_time };
    let origin = if start == ffmpeg::ffi::AV_NOPTS_VALUE {
        0.0
    } else {
        start as f64 / 1e6
    };
    let duration = (input.duration() as f64 / 1e6).max(0.0);
    let vi = video.index;
    let ai = audio.as_ref().map(|a| a.index);
    if !send(
        tx,
        Message::Opened(Header {
            duration,
            origin,
            video,
            audio,
        }),
        stop,
    ) {
        return Ok(());
    }
    let mut epoch = 1;
    let mut eof = false;
    while !stop.load(Ordering::Acquire) {
        let mut seek = None;
        while let Ok(s) = seeks.try_recv() {
            seek = Some(s);
        }
        if let Some(s) = seek {
            deadline.store(
                base.elapsed().as_millis() as u64 + 10_000,
                Ordering::Relaxed,
            );
            let ts = ((s.target + origin) * 1e6) as i64;
            input.seek(ts, ..ts).context("seek input")?;
            epoch = s.epoch;
            eof = false;
            if !send(tx, Message::Seeked(epoch), stop) {
                break;
            }
        }
        if eof {
            std::thread::sleep(Duration::from_millis(5));
            continue;
        }
        deadline.store(
            base.elapsed().as_millis() as u64 + 10_000,
            Ordering::Relaxed,
        );
        let mut packet = ffmpeg::Packet::empty();
        match packet.read(&mut input) {
            Ok(()) => {
                if packet.stream() != vi && Some(packet.stream()) != ai {
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
