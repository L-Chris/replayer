use super::*;
#[cfg(test)]
use crate::pixels::test_support::TestImageOutput;
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::Mutex,
};
use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

type Requests = Arc<Mutex<Vec<String>>>;
fn server(responses: Vec<(u16, Vec<u8>)>) -> (String, Requests, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let requests = Arc::new(Mutex::new(Vec::new()));
    let captured = requests.clone();
    let thread = std::thread::spawn(move || {
        for (status, body) in responses {
            let start = std::time::Instant::now();
            let mut socket = loop {
                match listener.accept() {
                    Ok((socket, _)) => break socket,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(
                            start.elapsed() < Duration::from_secs(10),
                            "mock server did not receive request"
                        );
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(e) => panic!("{e}"),
                }
            };
            socket.set_nonblocking(false).unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut bytes = Vec::new();
            let mut buffer = [0; 4096];
            while !bytes.windows(4).any(|s| s == b"\r\n\r\n") {
                let n = socket.read(&mut buffer).unwrap();
                if n == 0 {
                    break;
                }
                bytes.extend_from_slice(&buffer[..n]);
            }
            captured
                .lock()
                .unwrap()
                .push(String::from_utf8_lossy(&bytes).into_owned());
            write!(
                socket,
                "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .unwrap();
            socket.write_all(&body).unwrap();
        }
    });
    (url, requests, thread)
}
fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}
fn json(value: serde_json::Value) -> (u16, Vec<u8>) {
    (200, serde_json::to_vec(&value).unwrap())
}
fn wav() -> Vec<u8> {
    let n = 16000u32;
    let mut out = Vec::new();
    out.extend(b"RIFF");
    out.extend((36 + n).to_le_bytes());
    out.extend(b"WAVEfmt ");
    out.extend(16u32.to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend(8000u32.to_le_bytes());
    out.extend(16000u32.to_le_bytes());
    out.extend(2u16.to_le_bytes());
    out.extend(16u16.to_le_bytes());
    out.extend(b"data");
    out.extend(n.to_le_bytes());
    out.resize(44 + n as usize, 0);
    out
}
#[test]
fn adapters_normalize_search_and_resolve_without_leaking_keys() {
    use serde_json::json;
    let replies = vec![
        json(
            json!({"code":200,"data":[{"id":123,"name":"song","artists":"artist","album":"album"}]}),
        ),
        json(json!({"code":200,"data":{"url":"https://media.example/a.flac"}})),
        json(
            json!({"code":200,"list":[{"mid":"mid123","name":"song","singer":"artist","album":"album"}]}),
        ),
        json(json!({"code":200,"url":"https://media.example/a.mp3","format":"mp3"})),
        json(
            json!({"code":200,"list":[{"id":"hash123","name":"song","singer":"artist","album":"album"}]}),
        ),
        json(json!({"code":200,"url":"https://media.example/a.flac"})),
    ];
    let (base, requests, worker) = server(replies);
    runtime().block_on(async {
        for source in chksz::sources(&base, "test-secret").unwrap() {
            let page = source.search("song name", 0).await.unwrap();
            assert_eq!(page.songs.len(), 1);
            assert!(!page.has_more);
            assert_eq!(page.songs[0].artist, "artist");
            let quality = source.descriptor().qualities[2].id;
            let track = source.resolve(&page.songs[0], quality).await.unwrap();
            assert_eq!(track.url.host_str(), Some("media.example"));
            assert!(!format!("{:?}", page.songs).contains("test-secret"));
        }
    });
    worker.join().unwrap();
    let requests = requests.lock().unwrap();
    assert!(requests[0].contains("offset=0"));
    assert!(requests[1].contains("id=123"));
    assert!(requests[3].contains("mid=mid123"));
    assert!(requests[5].contains("id=hash123"));
}
#[test]
fn errors_do_not_echo_credentials_or_upstream_bodies() {
    let (base, _, worker) = server(vec![
        (401, b"test-secret".to_vec()),
        json(serde_json::json!({"code":403,"msg":"test-secret"})),
    ]);
    runtime().block_on(async {
        let sources = chksz::sources(&base, "test-secret").unwrap();
        for _ in 0..2 {
            let error = sources[0].search("x", 0).await.err().unwrap().to_string();
            assert!(!error.contains("test-secret"));
            assert!(!error.contains(&base));
        }
        let sources = chksz::sources(&base, "").unwrap();
        assert!(
            sources[0]
                .search("x", 0)
                .await
                .err()
                .unwrap()
                .to_string()
                .contains("Music settings")
        );
    });
    worker.join().unwrap();
}
#[test]
fn download_is_local_playable_reusable_and_never_sends_api_key_to_cdn() {
    let mut cover = std::io::Cursor::new(Vec::new());
    crate::pixels::solid(24, 24, [30, 100, 200, 255])
        .write_to(&mut cover, "png")
        .unwrap();
    let cover = cover.into_inner();
    let (cdn, requests, worker) = server(vec![(200, wav()), (200, cover.clone()), (200, cover)]);
    let resolved = serde_json::json!({"code":200,"url":format!("{cdn}/song.wav"),"format":"wav","name":"Resolved title","singer":"Resolved artist","album":"Resolved album","cover":format!("{cdn}/cover.png"),"lrc":"[00:00.50]first line\n[00:01.50]second line"});
    let (base, _, api_worker) = server(vec![json(resolved.clone()), json(resolved)]);
    let dir = std::env::temp_dir().join(format!("replayer-music-test-{}", uuid::Uuid::new_v4()));
    runtime().block_on(async {
        let source = chksz::sources(&base, "test-secret").unwrap().remove(1);
        let song = Song {
            source: source.descriptor().id,
            id: "1".into(),
            title: "../CON: song".into(),
            artist: "a/b".into(),
            album: "".into(),
            cover: None,
        };
        let progress = Arc::new(AtomicU64::new(0));
        let path = download(source.as_ref(), &song, "128k", &dir, progress.clone())
            .await
            .unwrap();
        assert_eq!(path.parent(), Some(dir.as_path()));
        assert_eq!(std::fs::read(&path).unwrap(), wav());
        assert_eq!(progress.load(Ordering::Relaxed), wav().len() as u64);
        let info = crate::media::probe_info(path.to_str().unwrap()).unwrap();
        assert_eq!(info.title.as_deref(), Some("Resolved title"));
        assert_eq!(info.artist.as_deref(), Some("Resolved artist"));
        assert_eq!(info.album.as_deref(), Some("Resolved album"));
        let origin = crate::media::LocalMetadata::load(&path).online.unwrap();
        assert_eq!(origin.source, song.source);
        assert_eq!(origin.id, song.id);
        assert_eq!(origin.title, "Resolved title");
        assert!(origin.cover.is_some());
        assert!(info.artwork.is_some());
        let sidecar = path.with_extension("lrc");
        assert!(
            std::fs::read_to_string(&sidecar)
                .unwrap()
                .contains("first line")
        );
        // Simulate a legacy download with no companion metadata. The second
        // operation must repair it, without transferring the audio again.
        std::fs::remove_file(crate::media::companion_path(&path, ".replayer.json")).unwrap();
        std::fs::remove_file(crate::media::companion_path(&path, ".replayer-cover.png")).unwrap();
        progress.store(0, Ordering::Relaxed);
        assert_eq!(
            path,
            download(source.as_ref(), &song, "128k", &dir, progress.clone())
                .await
                .unwrap()
        );
        assert_eq!(progress.load(Ordering::Relaxed), 0);
        let info = crate::media::probe_info(path.to_str().unwrap()).unwrap();
        assert_eq!(info.title.as_deref(), Some("Resolved title"));
        assert!(info.artwork.is_some());
    });
    worker.join().unwrap();
    api_worker.join().unwrap();
    assert!(
        requests
            .lock()
            .unwrap()
            .iter()
            .all(|r| !r.contains("test-secret"))
    );
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 4);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn invalid_download_is_not_published_and_partial_is_cleaned() {
    let (cdn, _, worker) = server(vec![(200, b"<html>error</html>".to_vec())]);
    let (base, _, api_worker) = server(vec![json(
        serde_json::json!({"code":200,"url":format!("{cdn}/song.mp3")}),
    )]);
    let dir = std::env::temp_dir().join(format!("replayer-music-test-{}", uuid::Uuid::new_v4()));
    runtime().block_on(async {
        let source = chksz::sources(&base, "test-secret").unwrap().remove(1);
        let song = Song {
            source: source.descriptor().id,
            id: "1".into(),
            title: "song".into(),
            artist: "artist".into(),
            album: "".into(),
            cover: None,
        };
        assert!(
            download(
                source.as_ref(),
                &song,
                "128k",
                &dir,
                Arc::new(AtomicU64::new(0))
            )
            .await
            .is_err()
        );
    });
    worker.join().unwrap();
    api_worker.join().unwrap();
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
    std::fs::remove_dir_all(dir).unwrap();
}
fn live_settings() -> Settings {
    Settings {
        api_key: std::env::var("CHKSZ_API_KEY")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| {
                let path = std::env::var_os("REPLAYER_ENV_FILE")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| {
                        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.env")
                    });
                dotenvy::from_path_iter(&path).ok().and_then(|entries| {
                    entries.filter_map(Result::ok).find_map(|(name, value)| {
                        (name == "CHKSZ_API_KEY" && !value.trim().is_empty()).then_some(value)
                    })
                })
            })
            .unwrap_or_default(),
        ..Default::default()
    }
}
fn live_chksz(index: usize) {
    let settings = live_settings();
    let key = settings.key().unwrap();
    assert!(!key.is_empty(), "CHKSZ_API_KEY is missing");
    runtime().block_on(async {
        for source in sources(&key).unwrap().into_iter().skip(index).take(1) {
            let descriptor = source.descriptor();
            let result = async {
                let page = source.search("低潮期 丁世光", 0).await?;
                println!("{}: {} search results", descriptor.id, page.songs.len());
                let song = page
                    .songs
                    .first()
                    .ok_or_else(|| anyhow::anyhow!("No search results"))?;
                for quality in [descriptor.qualities[0].id, descriptor.default_quality] {
                    let start = std::time::Instant::now();
                    let path = download(
                        source.as_ref(),
                        song,
                        quality,
                        &settings.directory(),
                        Arc::new(AtomicU64::new(0)),
                    )
                    .await?;
                    println!(
                        "{}: downloaded and probed audio in {:.1}s: {}",
                        descriptor.id,
                        start.elapsed().as_secs_f64(),
                        path.display()
                    );
                    let info = crate::media::probe_info(path.to_str().unwrap())
                        .ok_or_else(|| anyhow::anyhow!("Cannot read downloaded metadata"))?;
                    println!(
                        "metadata: title={:?}, artist={:?}, album={:?}, cover={}",
                        info.title,
                        info.artist,
                        info.album,
                        info.artwork.is_some()
                    );
                    anyhow::ensure!(
                        info.title.is_some() && info.artist.is_some() && info.artwork.is_some(),
                        "Missing downloaded metadata or artwork"
                    );
                }
                Ok::<_, anyhow::Error>(())
            }
            .await;
            assert!(result.is_ok(), "{}: {}", descriptor.id, result.unwrap_err());
        }
    });
}

#[test]
#[ignore = "Uses CHKSZ_API_KEY and downloads real audio"]
fn live_chksz_netease() {
    live_chksz(0);
}
#[test]
#[ignore = "Uses CHKSZ_API_KEY and downloads real audio"]
fn live_chksz_qq() {
    live_chksz(1);
}
#[test]
#[ignore = "Uses CHKSZ_API_KEY and downloads real audio"]
fn live_chksz_kugou() {
    live_chksz(2);
}

#[test]
fn netease_actual_nested_response_respects_total_and_page_offset() {
    let songs: Vec<_> = (0..20)
        .map(|id| serde_json::json!({"id":id,"name":"song","artists":"artist"}))
        .collect();
    let (base, requests, worker) = server(vec![json(
        serde_json::json!({"code":200,"data":{"songs":songs,"total":40}}),
    )]);
    runtime().block_on(async {
        let source = chksz::sources(&base, "test-secret").unwrap().remove(0);
        let page = source.search("song", 1).await.unwrap();
        assert_eq!(page.songs.len(), 20);
        assert!(!page.has_more);
    });
    worker.join().unwrap();
    assert!(requests.lock().unwrap()[0].contains("offset=20"));
}

#[test]
fn cancelling_a_running_download_removes_its_partial_file() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let cdn = format!("http://{}/song.wav", listener.local_addr().unwrap());
    let (release, release_rx) = crossbeam_channel::bounded::<()>(1);
    let worker = std::thread::spawn(move || {
        listener.set_nonblocking(true).unwrap();
        let start = std::time::Instant::now();
        let mut socket = loop {
            if let Ok((socket, _)) = listener.accept() {
                break socket;
            }
            if start.elapsed() > Duration::from_secs(5) {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        socket.set_nonblocking(false).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let _ = socket.read(&mut [0u8; 4096]);
        let _ = write!(
            socket,
            "HTTP/1.1 200 OK\r\nContent-Length: 16044\r\nConnection: close\r\n\r\n"
        );
        let _ = socket.write_all(&wav()[..1024]);
        let _ = release_rx.recv_timeout(Duration::from_secs(5));
    });
    let (base, _, api_worker) = server(vec![json(
        serde_json::json!({"code":200,"url":cdn,"format":"wav"}),
    )]);
    let dir = std::env::temp_dir().join(format!("replayer-music-test-{}", uuid::Uuid::new_v4()));
    let destination = dir.clone();
    let source = chksz::sources(&base, "test-secret").unwrap().remove(1);
    let song = Song {
        source: source.descriptor().id,
        id: "1".into(),
        title: "song".into(),
        artist: "artist".into(),
        album: "".into(),
        cover: None,
    };
    let job = Job::spawn(move |progress| async move {
        download(source.as_ref(), &song, "128k", &destination, progress).await
    });
    let start = std::time::Instant::now();
    while job.bytes.load(Ordering::Relaxed) == 0 {
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "download never started"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    drop(job);
    let start = std::time::Instant::now();
    while std::fs::read_dir(&dir).unwrap().count() != 0 {
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "cancelled partial file was not removed"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let _ = release.send(());
    worker.join().unwrap();
    api_worker.join().unwrap();
    std::fs::remove_dir_all(dir).unwrap();
}
