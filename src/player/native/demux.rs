//! A dedicated SDK worker executes the public demux/decoder/filter DAG.
use super::Shared;
use anyhow::{Context, Result};
use crossbeam_channel::{Receiver, SendTimeoutError, Sender, bounded, unbounded};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
pub struct Header {
    pub duration: f64,
    pub has_audio: bool,
    pub has_video: bool,
    pub info: Arc<crate::media::Info>,
}
pub enum Message {
    Opened(Header),
    Frame(u64, remdia::Frame),
    Seeked(u64),
    Eof(u64),
    Error(String),
}
pub struct Seek {
    pub epoch: u64,
    pub target: f64,
    pub rate: Option<u32>,
}
pub struct Demux {
    pub rx: Receiver<Message>,
    pub seeks: Sender<Seek>,
    pub waiting: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    cancel: remdia::Cancellation,
}
impl Demux {
    pub fn spawn(path: String, shared: &Arc<Shared>, rate: u32) -> Result<Self> {
        let (tx, rx) = bounded(4);
        let (seeks, commands) = unbounded();
        let waiting = Arc::new(AtomicBool::new(false));
        let worker_waiting = waiting.clone();
        let worker = shared.clone();
        let stop = shared.stop.clone();
        let thread = std::thread::Builder::new()
            .name("replayer-remdia-graph".into())
            .spawn(move || {
                if let Err(e) = run(&path, &worker, rate, &tx, commands, &worker_waiting)
                    && !worker.stop.load(Ordering::Acquire)
                {
                    let _ = send(&tx, Message::Error(format!("{e:#}")), &worker);
                }
            })?;
        Ok(Self {
            rx,
            seeks,
            waiting,
            stop,
            thread: Some(thread),
            cancel: shared.cancel.clone(),
        })
    }
}
impl Drop for Demux {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.cancel.cancel();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
fn send(tx: &Sender<Message>, mut message: Message, s: &Shared) -> bool {
    while !s.stop.load(Ordering::Acquire) {
        if matches!(&message,Message::Frame(id,_) if *id<s.requested.load(Ordering::Acquire)) {
            return true;
        }
        match tx.send_timeout(message, Duration::from_millis(5)) {
            Ok(()) => return true,
            Err(SendTimeoutError::Timeout(m)) => message = m,
            Err(SendTimeoutError::Disconnected(_)) => break,
        }
    }
    false
}
fn run(
    path: &str,
    s: &Shared,
    rate: u32,
    tx: &Sender<Message>,
    commands: Receiver<Seek>,
    waiting: &AtomicBool,
) -> Result<()> {
    let prepared = crate::media_source::prepare(path, &s.stop, s.source_key.as_deref())?;
    let engine = crate::media_backend::engine()?;
    let mut request = remdia::Request::playback(prepared.path());
    request.audio_rate = rate;
    request.timeout_ms = if s.progressive { 300000 } else { 30000 };
    request.hardware = std::env::var_os("REPLAYER_SOFTWARE").is_none();
    let mut session = unsafe { engine.open(request.clone(), s.cancel.clone()) }
        .with_context(|| format!("open media '{path}' through remdia"))?;
    let header = Header {
        duration: session.info().duration_us.unwrap_or(0) as f64 / 1e6,
        has_audio: session.info().tracks.iter().any(|t| t.kind == "audio"),
        has_video: session
            .info()
            .tracks
            .iter()
            .any(|t| t.kind == "video" && !t.attached_picture),
        info: Arc::new(crate::media::remdia_info(
            &mut session,
            std::path::Path::new(path),
        )),
    };
    if !send(tx, Message::Opened(header), s) {
        return Ok(());
    }
    s.cancel.acknowledge_generation(1);
    let mut epoch = 1;
    let mut eof = false;
    while !s.stop.load(Ordering::Acquire) {
        let mut seek = None;
        while let Ok(next) = commands.try_recv() {
            seek = Some(next);
        }
        if let Some(next) = seek {
            if s.requested.load(Ordering::Acquire) > next.epoch {
                continue;
            }
            s.cancel.acknowledge_generation(next.epoch);
            if let Some(rate) = next.rate
                && rate != request.audio_rate
            {
                request.audio_rate = rate;
                s.cancel.begin();
                session = unsafe { engine.open(request.clone(), s.cancel.clone()) }?;
            }
            waiting.store(true, Ordering::Release);
            let result = session.seek((next.target * 1e6).round() as i64);
            waiting.store(false, Ordering::Release);
            if s.requested.load(Ordering::Acquire) > next.epoch {
                continue;
            }
            result.context("seek remdia graph")?;
            epoch = next.epoch;
            eof = false;
            if !send(tx, Message::Seeked(epoch), s) {
                break;
            }
        }
        if s.requested.load(Ordering::Acquire) > epoch || eof {
            std::thread::sleep(Duration::from_millis(2));
            continue;
        }
        waiting.store(true, Ordering::Release);
        let result = session.read();
        waiting.store(false, Ordering::Release);
        if s.stop.load(Ordering::Acquire) {
            break;
        }
        if s.requested.load(Ordering::Acquire) > epoch {
            continue;
        }
        match result? {
            Some(frame) => {
                if !send(tx, Message::Frame(epoch, frame), s) {
                    break;
                }
            }
            None => {
                eof = true;
                if !send(tx, Message::Eof(epoch), s) {
                    break;
                }
            }
        }
    }
    Ok(())
}
