use super::*;
use std::{
    io::{Read, Write},
    net::TcpListener,
    time::{Duration, Instant},
};

fn pump(player: &mut Player) -> Vec<Event> {
    let mut events = Vec::new();
    while let Some(event) = player.poll_event() {
        if let Event::Error(error) = &event {
            panic!("{error}");
        }
        events.push(event);
    }
    player.next_frame(player.sync_time());
    events
}

#[test]
fn progressive_http_freezes_while_buffering_and_interrupts_old_seek() {
    let root = std::env::temp_dir().join(format!("replayer-network-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("sample.mp4");
    let ffmpeg =
        std::path::PathBuf::from(std::env::var_os("FFMPEG_DIR").unwrap()).join("bin/ffmpeg.exe");
    let status = std::process::Command::new(ffmpeg)
        .args([
            "-v",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=320x180:rate=24:duration=8",
            "-c:v",
            "mpeg4",
            "-g",
            "24",
            "-q:v",
            "4",
            "-movflags",
            "+faststart",
        ])
        .arg(&path)
        .status()
        .unwrap();
    assert!(status.success());
    let data = Arc::new(std::fs::read(&path).unwrap());
    let available = Arc::new(std::sync::atomic::AtomicUsize::new(data.len() / 3));
    let stop = Arc::new(AtomicBool::new(false));
    let server_stop = stop.clone();
    let server_available = available.clone();
    let server_data = data.clone();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let mut children = Vec::new();
        while !server_stop.load(Ordering::Acquire) {
            let (mut socket, _) = match listener.accept() {
                Ok(c) => c,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(5));
                    continue;
                }
                Err(e) => panic!("{e}"),
            };
            let stop = server_stop.clone();
            let data = server_data.clone();
            let available = server_available.clone();
            children.push(std::thread::spawn(move||{
                socket.set_nonblocking(false).unwrap();socket.set_read_timeout(Some(Duration::from_secs(2))).unwrap();socket.set_write_timeout(Some(Duration::from_secs(2))).unwrap();
                let mut request=Vec::new();
                while !request.windows(4).any(|w|w==b"\r\n\r\n"){
                    let mut bytes=[0;4096];let Ok(n)=socket.read(&mut bytes)else{return;};if n==0{return;}request.extend_from_slice(&bytes[..n]);
                }
                let headers=String::from_utf8_lossy(&request).to_lowercase();
                let offset=headers.lines().find_map(|line|line.strip_prefix("range: bytes=")).and_then(|s|s.split('-').next()).and_then(|s|s.parse::<usize>().ok()).unwrap_or(0);
                if offset>=data.len(){let _=write!(socket,"HTTP/1.1 416 Range Not Satisfiable\r\nContent-Length: 0\r\n\r\n");return;}
                if write!(socket,"HTTP/1.1 206 Partial Content\r\nAccept-Ranges: bytes\r\nContent-Type: video/mp4\r\nContent-Range: bytes {offset}-{}/{}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",data.len()-1,data.len(),data.len()-offset).is_err(){return;}
                let mut position=offset;
                while position<data.len()&&!stop.load(Ordering::Acquire){
                    let end=(position+8192).min(available.load(Ordering::Acquire)).min(data.len());
                    if end<=position{std::thread::sleep(Duration::from_millis(5));continue;}
                    if socket.write_all(&data[position..end]).is_err(){return;}
                    position=end;
                }
            }));
        }
        for child in children {
            child.join().unwrap();
        }
    });
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut player = Player::open_progressive(format!("http://{address}/sample.mp4")).unwrap();
        let deadline = Instant::now() + Duration::from_secs(12);
        loop {
            pump(&mut player);
            if player.snapshot().state == PlaybackState::Buffering {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "never entered buffering: {:?}",
                player.snapshot()
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        let frozen = player.position();
        std::thread::sleep(Duration::from_millis(250));
        pump(&mut player);
        assert!((player.position() - frozen).abs() < 0.03);
        player.seek(6.0);
        std::thread::sleep(Duration::from_millis(150));
        let latest = player.seek(0.0);
        let deadline = Instant::now() + Duration::from_secs(4);
        let mut done = false;
        while Instant::now() < deadline {
            if pump(&mut player)
                .iter()
                .any(|event| matches!(event,Event::SeekCompleted{id,..}if *id==latest))
            {
                done = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(done, "new seek was blocked by old missing-data read");
        available.store(data.len(), Ordering::Release);
        let deadline = Instant::now() + Duration::from_secs(6);
        while player.position() < 3.0 {
            pump(&mut player);
            assert!(Instant::now() < deadline, "did not resume");
            std::thread::sleep(Duration::from_millis(5));
        }
    }));
    stop.store(true, Ordering::Release);
    server.join().unwrap();
    std::thread::sleep(Duration::from_millis(100));
    assert!(root.starts_with(std::env::temp_dir()));
    std::fs::remove_dir_all(root).unwrap();
    if let Err(error) = result {
        std::panic::resume_unwind(error);
    }
}
