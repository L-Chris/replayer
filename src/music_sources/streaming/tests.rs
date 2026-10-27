use super::*;
use axum::{extract::State, http::HeaderMap};
use std::sync::atomic::{AtomicBool, AtomicUsize};

struct Provider {
    url: reqwest::Url,
    extension: &'static str,
}
impl MusicSource for Provider {
    fn descriptor(&self) -> Descriptor {
        Descriptor {
            id: "test",
            name: "Test",
            qualities: &[],
            default_quality: "default",
        }
    }
    fn search<'a>(&'a self, _: &'a str, _: usize) -> BoxFuture<'a, Result<SearchPage>> {
        Box::pin(async { anyhow::bail!("unused") })
    }
    fn resolve<'a>(&'a self, _: &'a Song, _: &'a str) -> BoxFuture<'a, Result<ResolvedTrack>> {
        Box::pin(async {
            Ok(ResolvedTrack {
                url: self.url.clone(),
                extension: self.extension.into(),
                metadata: crate::media::LocalMetadata {
                    title: "Cached track".into(),
                    ..Default::default()
                },
                cover: None,
                lyrics: Some("[00:00]Lyrics".into()),
            })
        })
    }
}
struct Remote {
    data: Vec<u8>,
    ranges: bool,
    unknown_length: bool,
    broken: bool,
    requests: Mutex<Vec<u64>>,
    released: AtomicBool,
    changed: Notify,
    closed: AtomicUsize,
}
impl Remote {
    fn release(&self) {
        self.released.store(true, Ordering::Release);
        self.changed.notify_waiters();
    }
    async fn wait(&self) {
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if self.released.load(Ordering::Acquire) {
                return;
            }
            changed.await;
        }
    }
}
async fn remote(State(state): State<Arc<Remote>>, headers: HeaderMap) -> axum::response::Response {
    let length = state.data.len() as u64;
    let (start, end, _) = if state.ranges {
        crate::torrent::byte_range(headers.get("range").and_then(|s| s.to_str().ok()), length)
            .unwrap()
    } else {
        (0, length - 1, false)
    };
    state.requests.lock().unwrap().push(start);
    if state.broken && start > 0 {
        return Response::builder().status(403).body(Body::empty()).unwrap();
    }
    let mut response = Response::builder().status(if state.ranges { 206 } else { 200 });
    if !state.unknown_length {
        response = response.header("Content-Length", end - start + 1);
    }
    if state.ranges {
        response = response.header("Content-Range", format!("bytes {start}-{end}/{length}"));
    }
    response
        .body(Body::from_stream(futures_util::stream::try_unfold(
            (state, start),
            move |(state, pos)| async move {
                if pos > end {
                    state.closed.fetch_add(1, Ordering::Relaxed);
                    return Ok::<_, std::io::Error>(None);
                }
                // Block the sequential downloader while allowing a tail seek to proceed.
                if (BLOCK..BLOCK * 2).contains(&pos) {
                    state.wait().await;
                }
                let next = (pos + 32768).min(end + 1);
                let data = state.data[pos as usize..next as usize].to_vec();
                Ok(Some((data, (state, next))))
            },
        )))
        .unwrap()
}
fn wav() -> Vec<u8> {
    let n = (BLOCK * 6) as u32;
    let mut data = Vec::new();
    data.extend(b"RIFF");
    data.extend((36 + n).to_le_bytes());
    data.extend(b"WAVEfmt ");
    data.extend(16u32.to_le_bytes());
    data.extend(1u16.to_le_bytes());
    data.extend(1u16.to_le_bytes());
    data.extend(44100u32.to_le_bytes());
    data.extend(88200u32.to_le_bytes());
    data.extend(2u16.to_le_bytes());
    data.extend(16u16.to_le_bytes());
    data.extend(b"data");
    data.extend(n.to_le_bytes());
    data.extend((0..n).map(|i| (i % 239) as u8));
    data
}
async fn fixture(
    ranges: bool,
    unknown_length: bool,
    broken: bool,
) -> (
    Arc<Remote>,
    Arc<dyn MusicSource>,
    Song,
    PathBuf,
    tokio::task::JoinHandle<()>,
) {
    fixture_audio(wav(), "wav", ranges, unknown_length, broken).await
}
async fn fixture_audio(
    data: Vec<u8>,
    extension: &'static str,
    ranges: bool,
    unknown_length: bool,
    broken: bool,
) -> (
    Arc<Remote>,
    Arc<dyn MusicSource>,
    Song,
    PathBuf,
    tokio::task::JoinHandle<()>,
) {
    let remote_state = Arc::new(Remote {
        data,
        ranges,
        unknown_length,
        broken,
        requests: Mutex::new(Vec::new()),
        released: false.into(),
        changed: Notify::new(),
        closed: 0.into(),
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/audio", listener.local_addr().unwrap())
        .parse()
        .unwrap();
    let app = Router::new()
        .route("/audio", get(remote))
        .with_state(remote_state.clone());
    let server = tokio::spawn(async {
        axum::serve(listener, app).await.unwrap();
    });
    let song = Song {
        source: "test",
        id: "track-1".into(),
        title: "Track".into(),
        artist: "Artist".into(),
        album: String::new(),
        cover: None,
    };
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target/test-data")
        .join(uuid::Uuid::new_v4().to_string());
    (
        remote_state,
        Arc::new(Provider { url, extension }),
        song,
        directory,
        server,
    )
}
async fn prepared(source: Arc<dyn MusicSource>, song: Song, directory: PathBuf) -> Stream {
    match prepare(source, song, "default".into(), directory)
        .await
        .unwrap()
    {
        Prepared::Stream(stream) => stream,
        _ => panic!("expected stream"),
    }
}
async fn completion(stream: &Stream) -> Result<PathBuf> {
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            if let Some(result) = stream.poll() {
                return result;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("completion timeout")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ranged_stream_plays_before_completion_prioritizes_seek_and_reuses_every_block() {
    let (remote, source, song, directory, server) = fixture(true, false, false).await;
    let stream = prepared(source.clone(), song.clone(), directory.clone()).await;
    let client = reqwest::Client::new();
    let first = client
        .get(&stream.url)
        .header("Range", "bytes=0-99")
        .send()
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap();
    assert_eq!(&first[..], &remote.data[..100]);
    assert!(stream.bytes.load(Ordering::Acquire) < remote.data.len() as u64);
    assert!(
        stream.poll().is_none(),
        "must serve audio before full download"
    );
    let offset = BLOCK * 5 + 20;
    let tail = tokio::time::timeout(
        Duration::from_secs(3),
        client
            .get(&stream.url)
            .header("Range", format!("bytes={offset}-{}", offset + 31))
            .send(),
    )
    .await
    .unwrap()
    .unwrap();
    let tail = tokio::time::timeout(Duration::from_secs(3), tail.bytes())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        &tail[..],
        &remote.data[offset as usize..offset as usize + 32]
    );
    // Exercise the real decoder and simulated audio output before releasing the download.
    let player = crate::player::Player::open_progressive(stream.url.clone()).unwrap();
    tokio::time::timeout(Duration::from_secs(6), async {
        loop {
            while let Some(event) = player.poll_event() {
                if let crate::player::Event::Error(error) = event {
                    panic!("{error}");
                }
            }
            if player.position() > 0.1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("audio should start before the whole file is downloaded");
    assert!(stream.poll().is_none());
    drop(player);
    remote.release();
    let target = completion(&stream).await.unwrap();
    assert_eq!(std::fs::read(&target).unwrap(), remote.data);
    assert!(crate::media::LocalMetadata::load(&target).online.is_some());
    assert!(target.with_extension("lrc").exists());
    let requests = remote.requests.lock().unwrap().clone();
    for index in 0..(remote.data.len() as u64).div_ceil(BLOCK) {
        assert_eq!(
            requests.iter().filter(|p| **p == index * BLOCK).count(),
            1,
            "a block was downloaded twice"
        );
    }
    server.abort();
    assert!(
        matches!(
            prepare(source, song, "default".into(), directory.clone())
                .await
                .unwrap(),
            Prepared::Local(_)
        ),
        "completed audio must be reusable offline"
    );
    let part = stream.cache.path.clone();
    drop(stream);
    tokio::time::sleep(Duration::from_millis(80)).await;
    assert!(!part.exists());
    std::fs::remove_dir_all(directory).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sequential_and_unknown_length_sources_serve_only_available_bytes() {
    for unknown in [false, true] {
        let (remote, source, song, directory, server) = fixture(false, unknown, false).await;
        let stream = prepared(source, song, directory.clone()).await;
        let cache_path = stream.cache.path.clone();
        let response = reqwest::Client::new()
            .get(&stream.url)
            .send()
            .await
            .unwrap();
        let mut body = response.bytes_stream();
        let first = tokio::time::timeout(Duration::from_secs(3), body.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(&first[..], &remote.data[..first.len()]);
        assert!(stream.poll().is_none());
        remote.release();
        let mut received = first.to_vec();
        while let Some(chunk) = body.next().await {
            received.extend_from_slice(&chunk.unwrap());
        }
        assert_eq!(received, remote.data);
        completion(&stream).await.unwrap();
        assert_eq!(
            remote.requests.lock().unwrap().len(),
            1,
            "range fallback must share the original response"
        );
        drop(stream);
        server.abort();
        tokio::time::sleep(Duration::from_millis(80)).await;
        assert!(!cache_path.exists());
        std::fs::remove_dir_all(directory).unwrap();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancellation_and_failed_ranges_never_publish_incomplete_audio() {
    for broken in [false, true] {
        let (remote, source, song, directory, server) = fixture(true, false, broken).await;
        let stream = prepared(source, song, directory.clone()).await;
        let cache_path = stream.cache.path.clone();
        if broken {
            assert!(completion(&stream).await.is_err());
        } else {
            tokio::time::timeout(Duration::from_secs(3), async {
                while stream.bytes.load(Ordering::Acquire) == 0 {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .unwrap();
        }
        drop(stream);
        remote.release();
        server.abort();
        tokio::time::timeout(Duration::from_secs(3), async {
            while cache_path.exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 0);
        std::fs::remove_dir_all(directory).unwrap();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mp3_flac_and_tail_indexed_m4a_start_while_background_download_is_blocked() {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target/test-data")
        .join(uuid::Uuid::new_v4().to_string());
    std::fs::create_dir_all(&directory).unwrap();
    for (extension, codec, bitrate) in [
        ("mp3", "libmp3lame", "320k"),
        ("flac", "flac", "320k"),
        ("m4a", "aac", "320k"),
    ] {
        let file = directory.join(format!("sample.{extension}"));
        let ffmpeg = crate::test_config::ffmpeg();
        let status = std::process::Command::new(ffmpeg)
            .args([
                "-v",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "anoisesrc=color=white:sample_rate=44100:duration=70",
                "-c:a",
                codec,
                "-b:a",
                bitrate,
            ])
            .arg(&file)
            .status()
            .unwrap();
        assert!(status.success());
        let data = std::fs::read(&file).unwrap();
        assert!(data.len() as u64 > BLOCK * 3);
        let (remote, source, song, cache, server) =
            fixture_audio(data, extension, true, false, false).await;
        let stream = prepared(source, song, cache.clone()).await;
        let player = crate::player::Player::open_progressive(stream.url.clone()).unwrap();
        tokio::time::timeout(Duration::from_secs(8), async {
            loop {
                while let Some(event) = player.poll_event() {
                    if let crate::player::Event::Error(error) = event {
                        panic!("{extension}: {error}");
                    }
                }
                if player.position() > 0.1 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("{extension} waited for the entire download"));
        assert!(stream.poll().is_none());
        // A seek far past the blocked sequential block uses a separate range.
        let seek = player.seek(60.0);
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(event) = player.poll_event() {
                    match event {
                        crate::player::Event::SeekCompleted { id, .. } if id == seek => break,
                        crate::player::Event::Error(error) => panic!("{extension}: {error}"),
                        _ => {}
                    }
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap_or_else(|_| {
            panic!(
                "{extension} seek blocked by background download: {:?}, requests {:?}",
                player.snapshot(),
                remote.requests.lock().unwrap()
            )
        });
        remote.release();
        completion(&stream).await.unwrap();
        drop(player);
        drop(stream);
        server.abort();
        tokio::time::sleep(Duration::from_millis(80)).await;
        std::fs::remove_dir_all(cache).unwrap();
    }
    std::fs::remove_dir_all(directory).unwrap();
}
