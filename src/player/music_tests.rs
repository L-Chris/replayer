use super::*;
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
fn ffmpeg(args: &[&str], output: &Path) {
    let exe = PathBuf::from(std::env::var_os("FFMPEG_DIR").unwrap()).join("bin/ffmpeg.exe");
    assert!(
        std::process::Command::new(exe)
            .args(["-v", "error", "-y"])
            .args(args)
            .arg(output)
            .status()
            .unwrap()
            .success()
    );
}
fn await_state(player: &Player, check: impl Fn(&Snapshot) -> bool) {
    let start = Instant::now();
    while !check(&player.snapshot()) {
        if player.snapshot().state == PlaybackState::Failed {
            let mut events = Vec::new();
            while let Some(event) = player.poll_event() {
                events.push(event);
            }
            panic!("{events:?}");
        }
        assert!(
            start.elapsed() < Duration::from_secs(6),
            "{:?}",
            player.snapshot()
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}
#[test]
fn music_formats_tags_cover_and_audio_eof() {
    let root = std::env::temp_dir().join(format!("replayer-music-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let cover = root.join("cover.png");
    image::RgbaImage::from_pixel(32, 32, image::Rgba([30, 100, 200, 255]))
        .save(&cover)
        .unwrap();
    for (name, codec, has_cover) in [
        ("song.wav", "pcm_s16le", false),
        ("song.flac", "flac", false),
        ("song.mp3", "libmp3lame", true),
        ("song.m4a", "aac", true),
    ] {
        let path = root.join(name);
        let mut args = vec![
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=44100:duration=0.7",
        ];
        if has_cover {
            args.extend([
                "-i",
                cover.to_str().unwrap(),
                "-map",
                "0:a",
                "-map",
                "1:v",
                "-c:v",
                "copy",
                "-disposition:v",
                "attached_pic",
            ]);
        }
        args.extend([
            "-c:a",
            codec,
            "-metadata",
            "title=Test song",
            "-metadata",
            "artist=Test artist",
            "-metadata",
            "album=Test album",
        ]);
        ffmpeg(&args, &path);
        let player = Player::open(path.to_string_lossy().into_owned()).unwrap();
        await_state(&player, |s| s.state == PlaybackState::Ended);
        let snapshot = player.snapshot();
        let info = snapshot.media.as_ref().unwrap();
        assert_eq!(info.kind, crate::media::Kind::Music);
        assert_eq!(info.title.as_deref(), Some("Test song"));
        assert_eq!(info.artist.as_deref(), Some("Test artist"));
        assert_eq!(info.album.as_deref(), Some("Test album"));
        assert_eq!(info.artwork.is_some(), has_cover);
        assert!(!snapshot.hardware);
        assert_eq!(player.queued(), 0);
        assert!(
            (player.position() - 0.7).abs() < 0.05,
            "{} {}",
            name,
            player.position()
        );
        drop(player);
        std::thread::sleep(Duration::from_millis(30));
    }
    assert!(root.starts_with(std::env::temp_dir()));
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn music_with_corrupt_frames_and_trailing_garbage_still_ends() {
    let root =
        std::env::temp_dir().join(format!("replayer-music-corrupt-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("song.mp3");
    ffmpeg(
        &[
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=44100:duration=0.7",
            "-c:a",
            "libmp3lame",
        ],
        &path,
    );
    let mut bytes = std::fs::read(&path).unwrap();
    // Corrupt one frame in the middle of the stream.
    let middle = bytes.len() / 2;
    for slot in &mut bytes[middle + 8..middle + 64] {
        *slot = slot.wrapping_mul(31).wrapping_add(7);
    }
    // Append a frame sync with a garbage payload, then trailing junk.
    bytes.extend_from_slice(&[0xFF, 0xFB, 0x90, 0x64]);
    bytes.extend((0..413u32).map(|i| (i % 251) as u8));
    bytes.extend((0..300u32).map(|i| (i.wrapping_mul(7) % 256) as u8));
    std::fs::write(&path, &bytes).unwrap();
    let player = Player::open(path.to_string_lossy().into_owned()).unwrap();
    await_state(&player, |s| s.state == PlaybackState::Ended);
    assert!(
        (player.position() - 0.7).abs() < 0.15,
        "{}",
        player.position()
    );
    drop(player);
    std::thread::sleep(Duration::from_millis(30));
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn music_pause_seek_replay_and_device_recovery() {
    let path =
        std::env::temp_dir().join(format!("replayer-music-seek-{}.wav", uuid::Uuid::new_v4()));
    ffmpeg(
        &[
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000:duration=2.5",
        ],
        &path,
    );
    let player = Player::open(path.to_string_lossy().into_owned()).unwrap();
    await_state(&player, |s| s.state == PlaybackState::Playing);
    player.set_playing(false);
    await_state(&player, |s| s.state == PlaybackState::Paused);
    let position = player.position();
    std::thread::sleep(Duration::from_millis(80));
    assert!((player.position() - position).abs() < 0.03);
    player.seek(1.8);
    let id = player.seek(0.5);
    await_state(&player, |s| {
        s.epoch == id && s.state == PlaybackState::Paused
    });
    assert!((player.position() - 0.5).abs() < 0.01);
    player.set_playing(true);
    player.seek(2.3);
    await_state(&player, |s| s.state == PlaybackState::Ended);
    player.toggle_play();
    await_state(&player, |s| s.state == PlaybackState::Playing);
    let epoch_before = player.snapshot().epoch;
    player.shared.output.failed.store(true, Ordering::Release);
    // The device is reopened and an internal seek resyncs the pipeline.
    await_state(&player, |s| {
        s.state == PlaybackState::Playing && s.epoch > epoch_before
    });
    assert!(player.snapshot().has_audio);
    let before = player.position();
    std::thread::sleep(Duration::from_millis(120));
    assert!(
        player.position() > before,
        "recovered audio did not resume the clock"
    );
    drop(player);
    std::thread::sleep(Duration::from_millis(50));
    std::fs::remove_file(path).unwrap();
}
