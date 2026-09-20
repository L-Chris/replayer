mod audio;
mod clock;
mod demux;
mod music;
#[cfg(test)]
mod music_tests;
#[cfg(test)]
mod network_tests;
mod session;
mod video;
pub use video::VideoFrame;

use anyhow::Result;
use audio::OutputControl;
use clock::Clock;
use crossbeam_channel::{Receiver, Sender, bounded, unbounded};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaybackState {
    Opening,
    Playing,
    Paused,
    Seeking,
    Buffering,
    Draining,
    Ended,
    Failed,
}
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub media: Option<Arc<crate::media::Info>>,
    pub state: PlaybackState,
    pub duration: f64,
    pub has_audio: bool,
    pub hardware: bool,
    pub epoch: u64,
}
#[derive(Debug)]
pub enum Event {
    MediaInfo(Arc<crate::media::Info>),
    Opened,
    SeekCompleted { id: u64, position: f64 },
    Ended,
    Warning(String),
    Error(String),
}
enum Command {
    Playing(bool),
    Seek { id: u64, target: f64 },
}
struct Shared {
    snapshot: Mutex<Snapshot>,
    clock: Arc<Clock>,
    requested: Arc<AtomicU64>,
    progressive: bool,
    source_key: Option<String>,
    desired_playing: AtomicBool,
    stop: Arc<AtomicBool>,
    output: Arc<OutputControl>,
    volume: Arc<AtomicU32>,
    presented: AtomicU64,
}
// Audio callback shares Clock directly, not the control/state mutex.
pub struct Player {
    commands: Sender<Command>,
    frames: Receiver<VideoFrame>,
    events: Receiver<Event>,
    shared: Arc<Shared>,
    pending: Option<VideoFrame>,
    shown_epoch: u64,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Player {
    /// Returns immediately. Opened/Error report asynchronous initialization.
    pub fn open(path: String) -> Result<Self> {
        Self::open_source(path, false, None)
    }
    pub fn open_progressive(path: String) -> Result<Self> {
        Self::open_source(path, true, None)
    }
    pub fn open_with_qq_key(path: String, ekey: String) -> Result<Self> {
        Self::open_source(path, false, Some(ekey))
    }
    fn open_source(path: String, progressive: bool, source_key: Option<String>) -> Result<Self> {
        let (commands, command_rx) = unbounded();
        let (frame_tx, frames) = bounded(3);
        let stale_frames = frames.clone();
        let (event_tx, events) = unbounded();
        let shared = Arc::new(Shared {
            snapshot: Mutex::new(Snapshot {
                media: None,
                state: PlaybackState::Opening,
                duration: 0.0,
                has_audio: false,
                hardware: false,
                epoch: 1,
            }),
            clock: Arc::new(Clock::new()),
            requested: Arc::new(AtomicU64::new(1)),
            progressive,
            source_key,
            desired_playing: AtomicBool::new(true),
            stop: Arc::new(AtomicBool::new(false)),
            output: Arc::new(OutputControl::new()),
            volume: Arc::new(AtomicU32::new(1.0f32.to_bits())),
            presented: AtomicU64::new(0.0f64.to_bits()),
        });
        let worker = shared.clone();
        let thread = std::thread::Builder::new()
            .name("replayer-session".into())
            .spawn(move || {
                if let Err(e) =
                    session::run(path, &worker, command_rx, frame_tx, stale_frames, &event_tx)
                {
                    worker.snapshot.lock().unwrap().state = PlaybackState::Failed;
                    worker.clock.set_playing(false);
                    let _ = event_tx.send(Event::Error(format!("{e:#}")));
                }
                worker.output.enabled.store(false, Ordering::Release);
                worker.stop.store(true, Ordering::Release);
            })?;
        Ok(Self {
            commands,
            frames,
            events,
            shared,
            pending: None,
            shown_epoch: 0,
            thread: Some(thread),
        })
    }
    pub fn snapshot(&self) -> Snapshot {
        self.shared.snapshot.lock().unwrap().clone()
    }
    pub fn poll_event(&self) -> Option<Event> {
        self.events.try_recv().ok()
    }
    pub fn duration(&self) -> f64 {
        self.snapshot().duration
    }
    pub fn has_audio(&self) -> bool {
        self.snapshot().has_audio
    }
    pub fn set_volume(&self, volume: f32) {
        self.shared.volume.store(
            if volume.is_finite() {
                volume.clamp(0.0, 1.0)
            } else {
                0.0
            }
            .to_bits(),
            Ordering::Relaxed,
        );
    }
    pub fn position(&self) -> f64 {
        let time = self.sync_time().max(0.0);
        let duration = self.duration();
        if duration > 0.0 {
            time.min(duration)
        } else {
            time
        }
    }
    pub fn sync_time(&self) -> f64 {
        self.shared.clock.now()
    }
    pub fn queued(&self) -> usize {
        self.frames.len()
    }
    pub fn pending_pts(&self) -> Option<f64> {
        self.pending.as_ref().map(|f| f.pts)
    }
    pub fn is_playing(&self) -> bool {
        matches!(
            self.snapshot().state,
            PlaybackState::Playing | PlaybackState::Draining | PlaybackState::Buffering
        )
    }
    pub fn set_playing(&self, playing: bool) {
        self.shared
            .desired_playing
            .store(playing, Ordering::Release);
        if playing && self.snapshot().state == PlaybackState::Ended {
            self.seek(0.0);
        }
        let _ = self.commands.send(Command::Playing(playing));
    }
    pub fn toggle_play(&self) {
        let ended = self.snapshot().state == PlaybackState::Ended;
        self.set_playing(ended || !self.shared.desired_playing.load(Ordering::Acquire));
    }
    pub fn seek(&self, target: f64) -> u64 {
        let duration = self.duration();
        let target = if target.is_finite() {
            target.max(0.0)
        } else {
            0.0
        };
        let target = if duration > 0.0 {
            target.min(duration)
        } else {
            target
        };
        let id = self.shared.requested.fetch_add(1, Ordering::AcqRel) + 1;
        self.shared.output.enabled.store(false, Ordering::Release);
        self.shared.output.epoch.store(id, Ordering::Release);
        let _ = self.commands.send(Command::Seek { id, target });
        id
    }
    pub fn next_frame(&mut self, now: f64) -> Option<VideoFrame> {
        let epoch = self.shared.requested.load(Ordering::Acquire);
        if self.pending.as_ref().is_some_and(|f| f.epoch != epoch) {
            self.pending = None;
        }
        let mut latest = None;
        loop {
            let frame = match self.pending.take().or_else(|| self.frames.try_recv().ok()) {
                Some(f) => f,
                None => break,
            };
            if frame.epoch != epoch {
                continue;
            }
            // A paused seek is allowed one preview frame at/after the requested time.
            if frame.pts <= now + 0.010 || self.shown_epoch != epoch {
                self.shown_epoch = epoch;
                latest = Some(frame);
            } else {
                self.pending = Some(frame);
                break;
            }
        }
        if let Some(f) = &latest {
            self.shared
                .presented
                .store(f.pts.to_bits(), Ordering::Release);
        }
        latest
    }
    pub fn underruns(&self) -> u64 {
        self.shared.output.underruns.load(Ordering::Relaxed)
    }
}
impl Drop for Player {
    fn drop(&mut self) {
        self.shared.output.enabled.store(false, Ordering::Release);
        self.shared.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            // Cancellation interrupts FFmpeg I/O; joining is kept off the UI thread.
            let _ = std::thread::Builder::new()
                .name("replayer-reaper".into())
                .spawn(move || {
                    let _ = thread.join();
                });
        }
    }
}
fn again(error: ffmpeg_next::Error) -> bool {
    matches!(error, ffmpeg_next::Error::Other { errno } if errno == ffmpeg_next::error::EAGAIN)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn open_errors_keep_the_real_cause() {
        let player = Player::open("replayer-intentionally-missing-file.mp4".into()).unwrap();
        let start = Instant::now();
        loop {
            if let Some(Event::Error(error)) = player.poll_event() {
                assert!(error.contains("replayer-intentionally-missing-file"));
                assert!(!error.contains("timed out opening"));
                break;
            }
            assert!(start.elapsed() < Duration::from_secs(5));
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn drop_cancels_blocked_network_open_and_reaps_threads() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (accepted_tx, accepted_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let server = std::thread::spawn(move || {
            let (_socket, _) = listener.accept().unwrap();
            accepted_tx.send(()).unwrap();
            let _ = release_rx.recv_timeout(Duration::from_secs(5));
        });
        let player = Player::open(format!("http://{address}/blocked.mp4")).unwrap();
        accepted_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        let shared = player.shared.clone();
        let start = Instant::now();
        drop(player);
        assert!(start.elapsed() < Duration::from_millis(200));
        while Arc::strong_count(&shared) > 1 && start.elapsed() < Duration::from_secs(3) {
            std::thread::sleep(Duration::from_millis(10));
        }
        let reaped = Arc::strong_count(&shared) == 1;
        let _ = release_tx.send(());
        server.join().unwrap();
        assert!(
            reaped,
            "session did not release its shared state after cancellation"
        );
    }
}
