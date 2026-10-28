use super::*;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
fn fixture(name: &str) -> String {
    PathBuf::from(std::env::var("REPLAYER_NATIVE_FIXTURES").expect("set REPLAYER_NATIVE_FIXTURES"))
        .join(name)
        .to_string_lossy()
        .into_owned()
}
fn pump(player: &mut Player) -> Vec<Event> {
    let mut events = Vec::new();
    while let Some(e) = player.poll_event() {
        if let Event::Error(message) = &e {
            panic!("{message}");
        }
        events.push(e);
    }
    player.next_frame(player.sync_time());
    events
}
fn until(
    player: &mut Player,
    limit: Duration,
    mut ready: impl FnMut(&mut Player, &[Event]) -> bool,
) {
    let start = Instant::now();
    loop {
        let events = pump(player);
        if ready(player, &events) {
            return;
        }
        assert!(
            start.elapsed() < limit,
            "state={:?}, position={}",
            player.snapshot(),
            player.position()
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}
#[test]
#[ignore = "requires staged remdia component DLLs and native fixtures"]
fn native_media_music_pause_seek_eof_and_replay() {
    for source in ["audio.wav", "cover.flac", "audio.mp3", "audio.opus"] {
        let mut p = Player::open(fixture(source)).unwrap();
        until(&mut p, Duration::from_secs(5), |p, _| p.position() > 0.1);
        assert_eq!(
            p.snapshot().media.as_ref().unwrap().kind,
            crate::media::Kind::Music
        );
        assert_eq!(p.queued(), 0);
        if source == "cover.flac" {
            assert!(p.snapshot().media.as_ref().unwrap().artwork.is_some());
        }
        p.set_playing(false);
        until(&mut p, Duration::from_secs(2), |p, _| {
            p.snapshot().state == PlaybackState::Paused
        });
        let paused = p.position();
        std::thread::sleep(Duration::from_millis(100));
        assert!((p.position() - paused).abs() < 0.03);
        p.seek(1.5);
        let id = p.seek(0.3);
        until(&mut p, Duration::from_secs(5), |p, e| {
            e.iter()
                .any(|e| matches!(e,Event::SeekCompleted {id:done,..} if *done==id))
                && p.snapshot().state == PlaybackState::Paused
        });
        p.seek(2.8);
        p.set_playing(true);
        until(&mut p, Duration::from_secs(5), |p, _| {
            p.snapshot().state == PlaybackState::Ended
        });
        p.toggle_play();
        until(&mut p, Duration::from_secs(5), |p, _| {
            p.position() > 0.1 && p.position() < 1. && p.snapshot().state != PlaybackState::Ended
        });
    }
}
#[test]
#[ignore = "requires staged remdia component DLLs and native fixtures"]
fn native_media_device_recovery() {
    let mut p = Player::open(fixture("audio.wav")).unwrap();
    until(&mut p, Duration::from_secs(5), |p, _| p.position() > 0.15);
    let epoch = p.snapshot().epoch;
    p.shared.output.failed.store(true, Ordering::Release);
    let mut recovered = false;
    until(&mut p, Duration::from_secs(6), |p, e| {
        recovered |= e.iter().any(|e| matches!(e, Event::Recovered));
        recovered && p.snapshot().epoch > epoch && p.position() > 0.2
    });
}
#[test]
#[ignore = "requires staged remdia component DLLs"]
fn native_media_drop_interrupts_blocked_network_open() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (accepted, ready) = std::sync::mpsc::channel();
    let (release, wait) = std::sync::mpsc::channel();
    let server = std::thread::spawn(move || {
        let (_socket, _) = listener.accept().unwrap();
        accepted.send(()).unwrap();
        let _ = wait.recv_timeout(Duration::from_secs(5));
    });
    let p = Player::open(format!("http://{address}/blocked.mp4")).unwrap();
    ready.recv_timeout(Duration::from_secs(3)).unwrap();
    let shared = p.shared.clone();
    let start = Instant::now();
    drop(p);
    assert!(start.elapsed() < Duration::from_millis(200));
    while Arc::strong_count(&shared) > 1 && start.elapsed() < Duration::from_secs(3) {
        std::thread::sleep(Duration::from_millis(10));
    }
    let reaped = Arc::strong_count(&shared) == 1;
    let _ = release.send(());
    server.join().unwrap();
    assert!(reaped, "native graph worker was not reclaimed");
}
#[cfg(any(windows, target_os = "macos"))]
#[test]
#[ignore = "requires staged platform video components"]
fn native_media_video_playback_pause_seek_and_drain() {
    let mut p = Player::open(fixture("av.mp4")).unwrap();
    until(&mut p, Duration::from_secs(8), |p, _| p.position() > 0.15);
    assert_eq!(
        p.snapshot().media.as_ref().unwrap().kind,
        crate::media::Kind::Video
    );
    p.set_playing(false);
    let id = p.seek(1.25);
    let mut preview = false;
    let start = Instant::now();
    let mut completed = false;
    while start.elapsed() < Duration::from_secs(5) {
        for e in pump(&mut p) {
            if matches!(e,Event::SeekCompleted {id:done,..} if done==id) {
                completed = true;
            }
        }
        if let Some(f) = p.next_frame(p.sync_time()) {
            assert_eq!(f.epoch, id);
            assert!(f.pts >= 1.25 && f.pts < 1.25 + 1. / 24. + 0.001);
            assert_eq!((f.image().width(), f.image().height()), (160, 90));
            preview = true;
        }
        if completed {
            let presented = f64::from_bits(p.shared.presented.load(Ordering::Acquire));
            assert!(presented < 1.25 + 1. / 24. + 0.001);
            preview |= presented >= 1.25;
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(completed && preview);
    p.set_playing(true);
    p.seek(2.8);
    until(&mut p, Duration::from_secs(5), |p, _| {
        p.snapshot().state == PlaybackState::Ended
    });
    p.toggle_play();
    until(&mut p, Duration::from_secs(5), |p, _| {
        p.position() > 0.1 && p.position() < 1.
    });
}

#[test]
#[ignore = "requires staged remdia platform components"]
fn native_media_video_open_and_paused_seek_survive_audio_preroll() {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/av-audio-preroll.mkv")
        .to_string_lossy()
        .into_owned();
    let mut player = Player::open(source).unwrap();
    until(&mut player, Duration::from_secs(6), |p, _| {
        p.snapshot().state == PlaybackState::Playing && p.position() > 0.1
    });
    player.set_playing(false);
    let seek = player.seek(0.25);
    until(&mut player, Duration::from_secs(6), |p, events| {
        p.snapshot().state == PlaybackState::Paused
            && events
                .iter()
                .any(|e| matches!(e,Event::SeekCompleted {id,..} if *id==seek))
    });
    assert!((player.position() - 0.25).abs() < 0.03);
}

#[test]
#[ignore = "requires remdia component DLLs and native fixtures"]
fn native_media_progressive_audio_buffers_and_new_seek_preempts_missing_data() {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::atomic::AtomicUsize,
    };
    let data = Arc::new(std::fs::read(fixture("audio.wav")).unwrap());
    let available = Arc::new(AtomicUsize::new(80_000));
    let stop = Arc::new(AtomicBool::new(false));
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let server_data = data.clone();
    let server_available = available.clone();
    let server_stop = stop.clone();
    let server = std::thread::spawn(move || {
        let mut children = Vec::new();
        while !server_stop.load(Ordering::Acquire) {
            let (mut socket, _) = match listener.accept() {
                Ok(v) => v,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(5));
                    continue;
                }
                Err(_) => break,
            };
            let data = server_data.clone();
            let available = server_available.clone();
            let stop = server_stop.clone();
            children.push(std::thread::spawn(move||{
                let _=socket.set_read_timeout(Some(Duration::from_secs(2)));
                let _=socket.set_write_timeout(Some(Duration::from_secs(1)));
                let mut request=Vec::new();let mut part=[0;1024];
                while request.len()<16384&&!request.ends_with(b"\r\n\r\n"){
                    match socket.read(&mut part){Ok(0)|Err(_)=>return,Ok(n)=>request.extend_from_slice(&part[..n])}
                }
                let request=String::from_utf8_lossy(&request);
                let offset=request.lines().find_map(|line|line.to_ascii_lowercase().strip_prefix("range: bytes=").and_then(|v|v.split('-').next()).and_then(|v|v.parse::<usize>().ok())).unwrap_or(0);
                if offset>=data.len(){let _=write!(socket,"HTTP/1.1 416 Range Not Satisfiable\r\nContent-Length: 0\r\n\r\n");return;}
                if write!(socket,"HTTP/1.1 206 Partial Content\r\nAccept-Ranges: bytes\r\nContent-Type: audio/wav\r\nContent-Range: bytes {offset}-{}/{}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",data.len()-1,data.len(),data.len()-offset).is_err(){return;}
                let mut position=offset;
                while position<data.len()&&!stop.load(Ordering::Acquire){
                    let end=(position+8192).min(available.load(Ordering::Acquire)).min(data.len());
                    if end<=position{std::thread::sleep(Duration::from_millis(5));continue;}
                    if socket.write_all(&data[position..end]).is_err(){return;}position=end;
                }
            }));
        }
        for child in children {
            let _ = child.join();
        }
    });
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut player = Player::open_progressive(format!("http://{address}/audio.wav")).unwrap();
        until(&mut player, Duration::from_secs(8), |p, _| {
            p.snapshot().state == PlaybackState::Buffering
        });
        let frozen = player.position();
        std::thread::sleep(Duration::from_millis(150));
        pump(&mut player);
        assert!((player.position() - frozen).abs() < 0.03);
        player.seek(2.5);
        std::thread::sleep(Duration::from_millis(150));
        let id = player.seek(0.);
        until(&mut player, Duration::from_secs(3), |_, events| {
            events
                .iter()
                .any(|e| matches!(e,Event::SeekCompleted {id:done,..} if *done==id))
        });
        // Recover while a second distant seek is blocked. Recovery gets a new
        // media generation but must retain the user's target and acknowledgement.
        player.set_playing(false);
        let pending = player.seek(2.5);
        until(&mut player, Duration::from_secs(3), |p, _| {
            p.snapshot().epoch == pending && p.snapshot().state == PlaybackState::Seeking
        });
        player
            .shared
            .output
            .simulated_rate
            .store(44100, Ordering::Release);
        player.shared.output.failed.store(true, Ordering::Release);
        until(&mut player, Duration::from_secs(3), |p, events| {
            events.iter().any(|e| matches!(e, Event::Recovered))
                && p.shared.requested.load(Ordering::Acquire) > pending
        });
        available.store(data.len(), Ordering::Release);
        until(&mut player, Duration::from_secs(5), |p, events| {
            events.iter().any(|e| {
                matches!(e, Event::SeekCompleted { id, position }
                if *id == pending && (*position - 2.5).abs() < 0.001)
            }) && p.snapshot().state == PlaybackState::Paused
        });
        assert!((player.position() - 2.5).abs() < 0.03);
        player.set_playing(true);
        until(&mut player, Duration::from_secs(5), |p, _| {
            p.position() > 2.6
        });
    }));
    stop.store(true, Ordering::Release);
    server.join().unwrap();
    if let Err(error) = result {
        std::panic::resume_unwind(error);
    }
}
