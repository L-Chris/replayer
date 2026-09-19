use crate::player::{Event, PlaybackState, Player};
use anyhow::{Result, bail, ensure};
use std::time::{Duration, Instant};

fn pump(p: &mut Player, millis: u64) -> Result<Vec<f64>> {
    let until = Instant::now() + Duration::from_millis(millis);
    let mut pts = Vec::new();
    while Instant::now() < until {
        while let Some(event) = p.poll_event() {
            if let Event::Error(e) = event {
                bail!(e);
            }
        }
        if let Some(f) = p.next_frame(p.sync_time()) {
            pts.push(f.pts);
        }
        std::thread::sleep(Duration::from_millis(3));
    }
    Ok(pts)
}
fn seek(p: &mut Player, target: f64) -> Result<()> {
    let id = p.seek(target);
    let start = Instant::now();
    let mut completed = false;
    let mut preview = None;
    while start.elapsed() < Duration::from_secs(10) {
        while let Some(event) = p.poll_event() {
            match event {
                Event::Error(e) => bail!(e),
                Event::SeekCompleted { id: done, position } if done == id => {
                    ensure!(
                        (position - target).abs() < 0.01,
                        "wrong seek completion position"
                    );
                    completed = true;
                }
                _ => {}
            }
        }
        if let Some(frame) = p.next_frame(p.sync_time()) {
            ensure!(frame.epoch == id, "stale generation after seek");
            ensure!(
                frame.pts + 0.001 >= target,
                "pre-target seek frame: {} < {target}",
                frame.pts
            );
            preview.get_or_insert(frame.pts);
        }
        if completed && (preview.is_some() || p.snapshot().state == PlaybackState::Ended) {
            break;
        }
        std::thread::sleep(Duration::from_millis(3));
    }
    ensure!(completed, "seek completion timed out: {:?}", p.snapshot());
    ensure!(
        preview.is_some() || p.snapshot().state == PlaybackState::Ended,
        "seek preview timed out: {:?}, pos={}, queued={}, pending={:?}",
        p.snapshot(),
        p.position(),
        p.queued(),
        p.pending_pts()
    );
    println!("seek {target:.3}: id={id} first={preview:?}");
    Ok(())
}
pub fn run(path: String) -> Result<()> {
    let open_start = Instant::now();
    let mut p = Player::open(path)?;
    p.set_volume(0.0);
    ensure!(
        open_start.elapsed() < Duration::from_millis(500),
        "open blocked caller"
    );
    let startup = Instant::now();
    while matches!(
        p.snapshot().state,
        PlaybackState::Opening | PlaybackState::Seeking
    ) && startup.elapsed() < Duration::from_secs(30)
    {
        pump(&mut p, 10)?;
    }
    ensure!(p.snapshot().state != PlaybackState::Failed, "open failed");
    println!("opened: {:?}", p.snapshot());
    let frames = pump(&mut p, 600)?;
    ensure!(!frames.is_empty(), "no video frames during playback");
    let before_stall = p.position();
    std::thread::sleep(Duration::from_millis(700));
    ensure!(
        p.position() > before_stall + 0.4,
        "stalled video consumer blocked playback clock"
    );
    pump(&mut p, 30)?;
    let duration = p.duration();
    ensure!(
        duration > 1.5,
        "selftest needs media longer than 1.5 seconds"
    );
    seek(&mut p, duration * 0.5)?;
    p.set_playing(false);
    pump(&mut p, 100)?;
    let paused = p.position();
    pump(&mut p, 200)?;
    ensure!((p.position() - paused).abs() < 0.005, "pause clock moved");
    seek(&mut p, duration * 0.25)?;
    ensure!(!p.is_playing(), "paused seek resumed unexpectedly");
    p.seek(duration * 0.7);
    p.seek(duration * 0.1);
    seek(&mut p, duration * 0.4)?;
    p.set_playing(true);
    pump(&mut p, 300)?;
    ensure!(
        p.position() > duration * 0.4 + 0.05,
        "resume did not advance"
    );
    seek(&mut p, (duration - 0.5).max(0.0))?;
    // The last video frame must survive a temporarily stalled presentation loop.
    std::thread::sleep(Duration::from_millis(800));
    let timeout = Instant::now();
    while p.snapshot().state != PlaybackState::Ended && timeout.elapsed() < Duration::from_secs(5) {
        pump(&mut p, 10)?;
    }
    ensure!(
        p.snapshot().state == PlaybackState::Ended,
        "EOF did not drain: {:?} pos={}",
        p.snapshot(),
        p.position()
    );
    ensure!(!p.is_playing(), "ended still playing");
    seek(&mut p, duration * 0.2)?;
    pump(&mut p, 200)?;
    ensure!(
        p.position() > duration * 0.2 + 0.05,
        "seek after EOF did not restart"
    );
    seek(&mut p, duration)?;
    pump(&mut p, 100)?;
    ensure!(
        p.snapshot().state == PlaybackState::Ended,
        "seek to EOF did not end"
    );
    p.toggle_play();
    let restarted = pump(&mut p, 400)?;
    ensure!(
        !restarted.is_empty() && p.position() < 1.0,
        "replay from end failed"
    );
    println!(
        "PASS: async open, playback, seek, pause, paused seek, rapid seek, drain, seek after EOF, replay; underruns={}",
        p.underruns()
    );
    Ok(())
}
