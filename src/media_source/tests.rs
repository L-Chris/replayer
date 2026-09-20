use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use sha2::{Digest, Sha256};

const EKEY: &str = "AQIDBAUGBwg0Wk04/J1L8KaXldnG4EdQS2sMVkKhC+rTp5SGmq1apIGuO4VHCjB4Bt9wRyyCbwAa6lV5AfcbKeeekgfYj07Pz5yAouQXPPV/zAUxuYNf/KfA69CO18uPOXVS0E7jccgaM9IW2nNUcM8w/hrKLGQL4WovxZ1QME079wChZb2XdBrEw/IGWQOdG9en0jEat26weXlK+YqetsvCa2CNqXt0qQzUq4xqWA17xfKfPZ7Z5cf3FM8KV+98G8e0rqacSFX1wqpQGvzBiLxOt0InI+JfNaoTcIDj62mnoD78GsRi1zCXtMURClQHPu9LPgKErmB7/FPSAftf3g3FYG+dwWLTZm+0/Nji3dw=";
fn key(length: usize) -> Vec<u8> {
    (0..length).map(|i| (i % 255 + 1) as u8).collect()
}

#[test]
fn independent_qmc2_vectors_and_unaligned_chunks() {
    // Produced by libtakiyasha, not by this implementation. See scripts/qq-reference-vectors.py.
    for (length, expected) in [
        (
            256,
            "8b5049b15fd4f5bbda68340af4be34c95f84327065a29afa05402913682f07b2",
        ),
        (
            512,
            "07445e3a88e7a6f982d0b09027baf64ede5495f4817f6b9bbcc90ca52acdcab6",
        ),
    ] {
        let mut cipher = qq::Cipher::from_key(key(length)).unwrap();
        let mut bytes = vec![0; 140000];
        for (i, chunk) in bytes.chunks_mut(777).enumerate() {
            cipher.apply(chunk, (i * 777) as u64);
        }
        assert_eq!(format!("{:x}", Sha256::digest(&bytes)), expected);
    }
    assert_eq!(qq::unwrap_key(EKEY.as_bytes()).unwrap(), key(256));
}

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!("replayer-qq-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&p).unwrap();
        Self(p)
    }
    fn file(&self, name: &str, data: &[u8]) -> PathBuf {
        let p = self.0.join(name);
        fs::write(&p, data).unwrap();
        p
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn encode_key(raw: &[u8]) -> String {
    let mut tea_key = [0; 16];
    for i in 0..8 {
        tea_key[i * 2] = ((106.0 + i as f64 * 0.1).tan().abs() * 100.0) as u8;
        tea_key[i * 2 + 1] = raw[i];
    }
    let mut result = raw[..8].to_vec();
    result.extend(tc_tea::encrypt(&raw[8..], tea_key).unwrap());
    STANDARD.encode(result)
}

#[test]
fn wrappers_cache_cleanup_plain_files_and_bad_inputs() {
    let fixture = Fixture::new();
    let mut plain = vec![0x59; 140000];
    plain[..4].copy_from_slice(b"fLaC");
    for (name, kind) in [
        ("a.qmcflac", 0),
        ("b.mflac", 1),
        ("c.mflac0", 2),
        ("d.mgg1", 3),
        ("e.mflac", 4),
    ] {
        let mut encoded = plain.clone();
        let mut cipher = if kind == 0 {
            qq::Cipher::legacy()
        } else {
            qq::Cipher::from_key(key(512)).unwrap()
        };
        cipher.apply(&mut encoded, 0);
        let ekey = encode_key(&key(512));
        match kind {
            1 => {
                encoded.extend(ekey.as_bytes());
                encoded.extend((ekey.len() as u32).to_le_bytes());
            }
            2 => {
                let tag = format!("{ekey},123,2");
                encoded.extend(tag.as_bytes());
                encoded.extend((tag.len() as u32).to_be_bytes());
                encoded.extend(b"QTag");
            }
            3 => {
                let tag = b"123,2,mid";
                encoded.extend(tag);
                encoded.extend((tag.len() as u32).to_be_bytes());
                encoded.extend(b"STag");
            }
            4 => {
                encoded.extend([0; 192]);
                encoded.extend(192_u32.to_le_bytes());
                encoded.extend(1_u32.to_le_bytes());
                encoded.extend(b"musicex\0");
            }
            _ => {}
        }
        let path = fixture.file(name, &encoded);
        if kind >= 3 {
            let error = prepare(path.to_str().unwrap(), &AtomicBool::new(false), None)
                .err()
                .unwrap()
                .to_string();
            assert!(error.contains("needs a song key"), "{error}");
            fixture.file(&format!("{name}.ekey"), ekey.as_bytes());
        }
        let ready = prepare(path.to_str().unwrap(), &AtomicBool::new(false), None).unwrap();
        assert_eq!(fs::read(ready.path()).unwrap(), plain);
        let cache = PathBuf::from(ready.path());
        drop(ready);
        assert!(!cache.exists());
        assert_eq!(fs::read(path).unwrap(), encoded);
    }
    let original = fixture.file("plain.mflac", &plain);
    let ready = prepare(original.to_str().unwrap(), &AtomicBool::new(false), None).unwrap();
    assert_eq!(ready.path(), original.to_str().unwrap());
    drop(ready);
    assert!(original.exists());
    let bad = fixture.file("bad.mflac", &[0; 32]);
    assert!(prepare(bad.to_str().unwrap(), &AtomicBool::new(false), None).is_err());
    assert!(prepare(original.to_str().unwrap(), &AtomicBool::new(true), None).is_err());
    let truncated = fixture.file("short.qmc0", b"short");
    assert!(prepare(truncated.to_str().unwrap(), &AtomicBool::new(false), None).is_err());
    assert!(qq::unwrap_key(b"invalid-secret-never-echo").is_err());
    for footer in [
        [u32::MAX.to_be_bytes().as_slice(), b"QTag"].concat(),
        [
            u32::MAX.to_le_bytes().as_slice(),
            1_u32.to_le_bytes().as_slice(),
            b"musicex\0",
        ]
        .concat(),
        [
            192_u32.to_le_bytes().as_slice(),
            9_u32.to_le_bytes().as_slice(),
            b"musicex\0",
        ]
        .concat(),
    ] {
        let mut data = vec![0; 256];
        data.extend(footer);
        let path = fixture.file("malformed.mflac", &data);
        assert!(prepare(path.to_str().unwrap(), &AtomicBool::new(false), None).is_err());
    }
    let v2 = STANDARD.encode(b"QQMusic EncV2,Key:unsupported-test-envelope");
    assert!(
        qq::unwrap_key(v2.as_bytes())
            .unwrap_err()
            .to_string()
            .contains("EncV2")
    );
}

#[test]
fn encrypted_music_plays_seeks_and_finishes() {
    use crate::player::{PlaybackState, Player};
    use std::time::{Duration, Instant};
    let fixture = Fixture::new();
    let flac = fixture.0.join("plain.flac");
    let ffmpeg = PathBuf::from(std::env::var_os("FFMPEG_DIR").unwrap()).join("bin/ffmpeg.exe");
    assert!(
        std::process::Command::new(ffmpeg)
            .args([
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "sine=duration=4:sample_rate=44100",
                "-metadata",
                "title=QQ adapter test",
                "-c:a",
                "flac"
            ])
            .arg(&flac)
            .status()
            .unwrap()
            .success()
    );
    let plain = fs::read(flac).unwrap();
    for legacy in [true, false] {
        let mut encoded = plain.clone();
        let mut cipher = if legacy {
            qq::Cipher::legacy()
        } else {
            qq::Cipher::from_key(key(512)).unwrap()
        };
        cipher.apply(&mut encoded, 0);
        if !legacy {
            let ekey = encode_key(&key(512));
            encoded.extend(ekey.as_bytes());
            encoded.extend((ekey.len() as u32).to_le_bytes());
        }
        let path = fixture.file(if legacy { "song.qmcflac" } else { "song.mflac" }, &encoded);
        let player = Player::open(path.to_string_lossy().into_owned()).unwrap();
        let wait = |check: &dyn Fn() -> bool| {
            let start = Instant::now();
            while !check() {
                assert_ne!(
                    player.snapshot().state,
                    PlaybackState::Failed,
                    "{:?}",
                    player.poll_event()
                );
                assert!(start.elapsed() < Duration::from_secs(8));
                std::thread::sleep(Duration::from_millis(5));
            }
        };
        wait(&|| player.snapshot().media.is_some());
        assert_eq!(
            player.snapshot().media.unwrap().title.as_deref(),
            Some("QQ adapter test")
        );
        player.seek(3.6);
        wait(&|| player.snapshot().state == PlaybackState::Ended);
        player.seek(0.2);
        player.set_playing(false);
        wait(&|| player.snapshot().state == PlaybackState::Paused);
        assert!((player.position() - 0.2).abs() < 0.05);
        drop(player);
    }
}
