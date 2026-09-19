use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

/// Single writer (audio callback), coherent readers. The callback never waits.
#[derive(Default)]
pub struct AudioProgress {
    serial: AtomicU64,
    epoch: AtomicU64,
    media_end: AtomicU64,
    wall_end: AtomicU64,
}
impl AudioProgress {
    pub fn publish(&self, epoch: u64, media_end: f64, wall_end: f64) {
        // SeqCst makes payload/version ordering explicit on every platform.
        self.serial.fetch_add(1, Ordering::SeqCst);
        self.epoch.store(epoch, Ordering::SeqCst);
        self.media_end.store(media_end.to_bits(), Ordering::SeqCst);
        self.wall_end.store(wall_end.to_bits(), Ordering::SeqCst);
        self.serial.fetch_add(1, Ordering::SeqCst);
    }
    pub(super) fn read(&self) -> (u64, u64, f64, f64) {
        loop {
            let a = self.serial.load(Ordering::SeqCst);
            if a & 1 != 0 {
                std::hint::spin_loop();
                continue;
            }
            let epoch = self.epoch.load(Ordering::SeqCst);
            let media = f64::from_bits(self.media_end.load(Ordering::SeqCst));
            let wall = f64::from_bits(self.wall_end.load(Ordering::SeqCst));
            if a == self.serial.load(Ordering::SeqCst) {
                return (a, epoch, media, wall);
            }
        }
    }
}
struct Anchor {
    epoch: u64,
    media: f64,
    wall: f64,
    playing: bool,
    audio: bool,
    serial: u64,
}
pub struct Clock {
    base: Instant,
    anchor: Mutex<Anchor>,
    pub progress: AudioProgress,
}
impl Clock {
    pub fn new() -> Self {
        Self {
            base: Instant::now(),
            progress: AudioProgress::default(),
            anchor: Mutex::new(Anchor {
                epoch: 1,
                media: 0.0,
                wall: 0.0,
                playing: false,
                audio: false,
                serial: 0,
            }),
        }
    }
    pub fn wall(&self) -> f64 {
        self.base.elapsed().as_secs_f64()
    }
    fn time(&self, a: &Anchor) -> f64 {
        if !a.playing {
            return a.media;
        }
        if a.audio {
            let (serial, epoch, end, wall_end) = self.progress.read();
            if serial > a.serial && epoch == a.epoch {
                return (end - (wall_end - self.wall()).max(0.0))
                    .max(a.media)
                    .min(end.max(a.media));
            }
            a.media
        } else {
            a.media + (self.wall() - a.wall).max(0.0)
        }
    }
    pub fn now(&self) -> f64 {
        self.time(&self.anchor.lock().unwrap())
    }
    pub fn reset(&self, epoch: u64, media: f64, playing: bool, audio: bool) {
        let mut a = self.anchor.lock().unwrap();
        *a = Anchor {
            epoch,
            media,
            wall: self.wall(),
            playing,
            audio,
            serial: self.progress.read().0,
        };
    }
    pub fn set_playing(&self, playing: bool) {
        let mut a = self.anchor.lock().unwrap();
        if a.playing != playing {
            a.media = self.time(&a);
            a.wall = self.wall();
            a.serial = self.progress.read().0;
            a.playing = playing;
        }
    }
    pub fn use_wall_clock(&self) {
        let mut a = self.anchor.lock().unwrap();
        a.media = self.time(&a);
        a.wall = self.wall();
        a.audio = false;
    }
    pub fn audio_drained(&self, epoch: u64, end: f64) -> bool {
        let (_, e, media, wall) = self.progress.read();
        e == epoch && media + 0.002 >= end && self.wall() >= wall
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn old_epoch_cannot_move_seek_clock() {
        let c = Clock::new();
        c.reset(2, 10.0, true, true);
        c.progress.publish(1, 90.0, c.wall());
        assert_eq!(c.now(), 10.0);
        c.progress.publish(2, 10.1, c.wall());
        assert_eq!(c.now(), 10.1);
    }
    #[test]
    fn underrun_freezes_and_pause_ignores_callbacks() {
        let c = Clock::new();
        c.reset(1, 0.0, true, true);
        c.progress.publish(1, 0.1, c.wall());
        assert_eq!(c.now(), 0.1);
        c.set_playing(false);
        c.progress.publish(1, 0.2, c.wall());
        assert_eq!(c.now(), 0.1);
    }
    #[test]
    fn publication_is_coherent_under_contention() {
        let p = std::sync::Arc::new(AudioProgress::default());
        let writer = p.clone();
        let t = std::thread::spawn(move || {
            for n in 1..20_000 {
                writer.publish(n, n as f64, n as f64 * 2.0);
            }
        });
        for _ in 0..20_000 {
            let (_, e, m, w) = p.read();
            assert_eq!(m, e as f64);
            assert_eq!(w, m * 2.0);
        }
        t.join().unwrap();
    }
}
