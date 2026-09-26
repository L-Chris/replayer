use anyhow::{Context, Result, bail, ensure};
use axum::{
    Router,
    body::Body,
    extract::{Path, State},
    http::{HeaderMap, Method, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
};
use librqbit::{
    AddTorrent, AddTorrentOptions, AddTorrentResponse, Api, ManagedTorrent, Session, SessionOptions,
};
use std::{
    path::{Component, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio_util::{io::ReaderStream, sync::CancellationToken};

#[derive(Clone, Debug)]
pub struct File {
    pub id: usize,
    pub name: String,
    pub size: u64,
    pub path: PathBuf,
}
#[derive(Clone, Debug, Default)]
pub struct Progress {
    pub downloaded: u64,
    pub total: u64,
    pub download_mbps: f64,
    pub finished: bool,
    pub paused: bool,
}
pub enum Event {
    Files(Vec<File>),
    Ready { url: String, file: File },
    Progress(Progress),
    Error(String),
}
enum Command {
    Select(usize),
    Pause(bool),
}
pub struct Job {
    pub events: crossbeam_channel::Receiver<Event>,
    tx: tokio::sync::mpsc::UnboundedSender<Command>,
    cancel: CancellationToken,
    pub cache_misses: Arc<AtomicU64>,
}
impl Job {
    pub fn start(magnet: String, cache: PathBuf) -> Result<Self> {
        let _ = rustls::crypto::ring::default_provider().install_default();
        ensure!(magnet.len() <= 32768, "Magnet link is too long");
        let parsed = librqbit::Magnet::parse(magnet.trim()).context("Invalid magnet link")?;
        let hash = parsed
            .as_id20()
            .context("Only v1/hybrid magnet links with btih are currently supported")?
            .as_string();
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let (events_tx, events) = crossbeam_channel::unbounded();
        let cancel = CancellationToken::new();
        let cache_misses = Arc::new(AtomicU64::new(0));
        let worker_misses = cache_misses.clone();
        let worker_cancel = cancel.clone();
        std::thread::Builder::new()
            .name("replayer-torrent".into())
            .spawn(move || {
                let result = (|| {
                    let rt = tokio::runtime::Builder::new_multi_thread()
                        .worker_threads(3)
                        .enable_all()
                        .build()?;
                    let result = rt.block_on(run(
                        magnet,
                        cache.join(hash),
                        rx,
                        &events_tx,
                        worker_cancel.clone(),
                        worker_misses,
                    ));
                    rt.shutdown_timeout(Duration::from_secs(2));
                    result
                })();
                if !worker_cancel.is_cancelled()
                    && let Err(error) = result
                {
                    let _ = events_tx.send(Event::Error(format!("{error:#}")));
                }
            })?;
        Ok(Self {
            events,
            tx,
            cancel,
            cache_misses,
        })
    }
    pub fn select(&self, id: usize) {
        let _ = self.tx.send(Command::Select(id));
    }
    pub fn pause(&self, paused: bool) {
        let _ = self.tx.send(Command::Pause(paused));
    }
}
impl Drop for Job {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}
pub fn cache_dir() -> PathBuf {
    std::env::var_os("REPLAYER_TORRENT_CACHE")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os("LOCALAPPDATA")
                .map(PathBuf::from)
                .unwrap_or_else(std::env::temp_dir)
                .join("replayer/torrents")
        })
}
pub fn is_media(name: &str) -> bool {
    crate::media::supported(name)
}
pub fn routing_rules(executable: &str, llm_base: &str) -> String {
    let name = if executable.contains([',', '\n', '\r', '/', '\\']) {
        "replayer.exe"
    } else {
        executable
    };
    let mut rules = Vec::new();
    // The local streaming server must stay direct, but BitTorrent peer and DHT
    // traffic needs the proxy egress on networks where UDP is otherwise blocked.
    rules.push("IP-CIDR,127.0.0.1/32,DIRECT,no-resolve".to_owned());
    rules.push(format!("PROCESS-NAME,{name},PROXY"));
    if let Ok(url) = reqwest::Url::parse(llm_base)
        && let Some(host) = url.host_str()
        && host != "localhost"
        && host.parse::<std::net::IpAddr>().is_err()
    {
        rules.push(format!("DOMAIN,{host},PROXY"));
    }
    rules.extend(
        [
            "DOMAIN,api.github.com,PROXY",
            "DOMAIN,github.com,PROXY",
            "DOMAIN-SUFFIX,githubusercontent.com,PROXY",
        ]
        .into_iter()
        .map(str::to_owned),
    );
    format!(
        "# Clash Verge Rev: 新建 Merge 类型配置并粘贴启用；将 PROXY 替换为你的策略组名\nprepend-rules:\n{}\n",
        rules
            .into_iter()
            .map(|rule| format!("  - {}", serde_json::to_string(&rule).unwrap()))
            .collect::<Vec<_>>()
            .join("\n")
    )
}
fn public_trackers() -> Vec<String> {
    [
        "udp://tracker.opentrackr.org:1337/announce",
        "udp://open.tracker.cl:1337/announce",
        "udp://tracker.torrent.eu.org:451/announce",
        "udp://exodus.desync.com:6969/announce",
        "udp://tracker.theoks.net:6969/announce",
        "udp://open.stealth.si:80/announce",
        "udp://tracker.tiny-vps.com:6969/announce",
        "udp://tracker.dler.org:6969/announce",
        "udp://tracker1.itzmx.com:8080/announce",
        "udp://p4p.arenabg.com:1337/announce",
        "udp://public.publicbt.one:6969/announce",
        "https://tracker.tamersunion.org:443/announce",
        "https://trackers.run:443/announce",
        "https://tracker.moeking.me:443/announce",
        "https://tr.cili001.com:7073/announce",
        "https://open.acgtracker.com:1096/announce",
        "https://tracker.gbitt.info:443/announce",
    ]
    .map(str::to_owned)
    .to_vec()
}
fn safe_path(components: &[String]) -> Result<PathBuf> {
    ensure!(!components.is_empty(), "Empty torrent filename");
    let mut path = PathBuf::new();
    for item in components {
        ensure!(
            !item.is_empty()
                && !item.contains(['/', '\\', ':', '\0', '<', '>', '"', '|', '?', '*'])
                && !item.ends_with(['.', ' '])
                && std::path::Path::new(item)
                    .components()
                    .all(|c| matches!(c, Component::Normal(_))),
            "Unsafe torrent filename"
        );
        let stem = item.split('.').next().unwrap_or("").to_ascii_uppercase();
        ensure!(
            !["CON", "PRN", "AUX", "NUL"].contains(&stem.as_str())
                && !((stem.starts_with("COM") || stem.starts_with("LPT"))
                    && stem.len() == 4
                    && stem.as_bytes()[3].is_ascii_digit()),
            "Reserved Windows filename"
        );
        path.push(item);
    }
    Ok(path)
}
async fn run(
    magnet: String,
    root: PathBuf,
    mut commands: tokio::sync::mpsc::UnboundedReceiver<Command>,
    events: &crossbeam_channel::Sender<Event>,
    cancel: CancellationToken,
    cache_misses: Arc<AtomicU64>,
) -> Result<()> {
    std::fs::create_dir_all(root.join("data"))?;
    let session = Session::new_with_opts(
        root.join("data"),
        SessionOptions {
            listen: Some(librqbit::ListenerOptions {
                mode: librqbit::ListenerMode::TcpAndUtp,
                ipv4_only: true,
                ..Default::default()
            }),
            dht: Some(librqbit::DhtSessionConfig {
                // A single unreachable bootstrap node stalls peer discovery;
                // try several well-known nodes and reuse the routing table.
                bootstrap_addrs: Some(
                    [
                        "router.bittorrent.com:6881",
                        "router.utorrent.com:6881",
                        "dht.transmissionbt.com:6881",
                        "dht.libtorrent.org:25401",
                    ]
                    .map(str::to_owned)
                    .to_vec(),
                ),
                persistence: Some(librqbit::dht::DhtPersistenceConfig {
                    dump_interval: None,
                    config_filename: root.parent().map(|parent| parent.join("dht-state.json")),
                }),
                ..Default::default()
            }),
            ipv4_only: true,
            cancellation_token: Some(cancel.child_token()),
            disable_local_service_discovery: true,
            ratelimits: librqbit::limits::LimitsConfig {
                upload_bps: std::num::NonZeroU32::new(256 * 1024),
                download_bps: None,
            },
            ..Default::default()
        },
    )
    .await?;
    let result = async {
        let metadata_path = root.join("metadata.torrent");
        let source = if metadata_path.exists() {
            AddTorrent::from_bytes(std::fs::read(&metadata_path)?)
        } else {
            AddTorrent::from_url(magnet.clone())
        };
        let listed = tokio::time::timeout(
            Duration::from_secs(180),
            session.add_torrent(
                source,
                Some(AddTorrentOptions {
                    list_only: true,
                    output_folder: Some(root.join("data").to_string_lossy().into_owned()),
                    trackers: Some(public_trackers()),
                    ..Default::default()
                }),
            ),
        )
        .await
        .context("Magnet metadata timed out; no reachable peers. Retry later, or apply the proxy routing rules from the magnet dialog.")??;
        let AddTorrentResponse::ListOnly(listed) = listed else {
            bail!("Expected torrent metadata");
        };
        let expected = librqbit::Magnet::parse(&magnet)?.as_id20().unwrap();
        ensure!(
            listed.info_hash == expected,
            "Cached torrent metadata does not match magnet"
        );
        let mut files = Vec::new();
        for (id, details) in listed.info.iter_file_details().enumerate() {
            let relative = safe_path(&details.filename.to_vec())?;
            let name = details.filename.to_string();
            if is_media(&name) && details.len > 0 {
                files.push(File {
                    id,
                    name,
                    size: details.len,
                    path: root.join("data").join(relative),
                });
            }
        }
        ensure!(
            !files.is_empty(),
            "No supported music or video files found in this torrent"
        );
        std::fs::write(metadata_path, &listed.torrent_bytes)?;
        let _ = events.send(Event::Files(files.clone()));
        let selected = loop {
            match commands.recv().await {
                Some(Command::Select(id)) => break id,
                Some(_) => {}
                None => return Ok(()),
            }
        };
        let first = files
            .iter()
            .find(|f| f.id == selected)
            .context("Invalid selected video")?
            .clone();
        let handle = session
            .add_torrent(
                AddTorrent::from_bytes(listed.torrent_bytes),
                Some(AddTorrentOptions {
                    only_files: Some(vec![selected]),
                    overwrite: true,
                    output_folder: Some(root.join("data").to_string_lossy().into_owned()),
                    initial_peers: Some(listed.seen_peers),
                    trackers: Some(public_trackers()),
                    ..Default::default()
                }),
            )
            .await?
            .into_handle()
            .context("Could not create download task")?;
        let api = Api::new(session.clone(), None);
        let active = Arc::new(AtomicUsize::new(selected));
        let token = uuid::Uuid::new_v4().simple().to_string();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let state = StreamState {
            handle: handle.clone(),
            active: active.clone(),
            token: token.clone(),
            api: api.clone(),
            cache_misses,
        };
        let router = Router::new()
            .route("/{token}/{file}", get(stream_file))
            .route("/{token}/{file}/cached", get(cached_file))
            .with_state(state);
        let server = tokio::spawn(async move { axum::serve(listener, router).await });
        let url = |id| format!("http://{address}/{token}/{id}");
        let mut pending = Some(first);
        let mut paused = false;
        let mut ticker = tokio::time::interval(Duration::from_millis(500));
        loop {
            tokio::select! {
                _=cancel.cancelled()=>break,
                command=commands.recv()=>match command {
                    Some(Command::Select(id))=>{
                        let file=files.iter().find(|f|f.id==id).context("Invalid video selection")?.clone();
                        session.update_only_files(&handle,&[id].into_iter().collect()).await?;
                        active.store(id,Ordering::Release);
                        if paused{session.unpause(&handle).await?;paused=false;}
                        pending=Some(file);
                    }
                    Some(Command::Pause(value))=>{
                        if value!=paused {if value {session.pause(&handle).await?;}else{session.unpause(&handle).await?;}paused=value;}
                    }
                    None=>break,
                },
                _=ticker.tick()=>{
                    let stats=handle.stats();
                    if let Some(error)=stats.error {bail!(error);}
                    if !matches!(stats.state,librqbit::TorrentStatsState::Initializing{..}) && let Some(file)=pending.take(){let _=events.send(Event::Ready{url:url(file.id),file});}
                    let id=active.load(Ordering::Acquire);
                    let total=files.iter().find(|f|f.id==id).map_or(0,|f|f.size);
                    let downloaded=stats.file_progress.get(id).copied().unwrap_or(0).min(total);
                    let _=events.send(Event::Progress(Progress{downloaded,total,finished:downloaded==total&&total>0,paused,download_mbps:stats.live.map_or(0.0,|s|s.download_speed.mbps)}));
                }
            }
        }
        server.abort();
        Ok(())
    };
    let outcome = tokio::select! {result=result=>result,_=cancel.cancelled()=>Ok(())};
    session.stop().await;
    outcome
}
#[derive(Clone)]
struct StreamState {
    handle: Arc<ManagedTorrent>,
    active: Arc<AtomicUsize>,
    token: String,
    api: Api,
    cache_misses: Arc<AtomicU64>,
}
async fn cached_file(
    State(state): State<StreamState>,
    Path((token, id)): Path<(String, usize)>,
    method: Method,
    headers: HeaderMap,
) -> Response {
    if token != state.token || id != state.active.load(Ordering::Acquire) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let result = async {
        let (file, length, offset, piece_len) = state
            .handle
            .with_metadata(|m| {
                m.file_infos.get(id).map(|f| {
                    (
                        state.handle.output_folder().join(&f.relative_filename),
                        f.len,
                        f.offset_in_torrent,
                        m.info.lengths().default_piece_length() as u64,
                    )
                })
            })?
            .context("Missing video metadata")?;
        let (start, end, partial) =
            match byte_range(headers.get("range").and_then(|h| h.to_str().ok()), length) {
                Ok(r) => r,
                Err(_) => {
                    return Ok(Response::builder()
                        .status(416)
                        .header("Content-Range", format!("bytes */{length}"))
                        .body(Body::empty())
                        .unwrap());
                }
            };
        let mut file = tokio::fs::File::open(file).await?;
        file.seek(std::io::SeekFrom::Start(start)).await?;
        let mut response = Response::builder()
            .status(if partial { 206 } else { 200 })
            .header("Accept-Ranges", "bytes")
            .header("Content-Type", "application/octet-stream")
            .header("Content-Length", end - start + 1);
        if partial {
            response = response.header("Content-Range", format!("bytes {start}-{end}/{length}"));
        }
        let stream = futures_util::stream::try_unfold(
            (file, start, state),
            move |(mut file, position, state)| async move {
                if position > end {
                    return Ok::<_, std::io::Error>(None);
                }
                let piece = (offset + position) / piece_len;
                let (haves, _) = state
                    .api
                    .api_dump_haves(state.handle.id().into())
                    .map_err(std::io::Error::other)?;
                if !haves.get(piece as usize).is_some_and(|bit| *bit) {
                    state.cache_misses.fetch_add(1, Ordering::AcqRel);
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::WouldBlock,
                        "Audio cache not ready",
                    ));
                }
                let n = (end - position + 1)
                    .min(piece_len - (offset + position) % piece_len)
                    .min(65536) as usize;
                let mut bytes = vec![0; n];
                file.read_exact(&mut bytes).await?;
                Ok(Some((bytes, (file, position + n as u64, state))))
            },
        );
        let body = if method == Method::HEAD {
            Body::empty()
        } else {
            Body::from_stream(stream)
        };
        Ok::<_, anyhow::Error>(response.body(body)?)
    }
    .await;
    result.unwrap_or_else(|_| StatusCode::SERVICE_UNAVAILABLE.into_response())
}
fn byte_range(header: Option<&str>, length: u64) -> Result<(u64, u64, bool)> {
    ensure!(length > 0, "Empty file");
    let Some(header) = header else {
        return Ok((0, length - 1, false));
    };
    let range = header.strip_prefix("bytes=").context("Unsupported range")?;
    ensure!(!range.contains(','), "Multiple ranges are unsupported");
    let (start, end) = range.split_once('-').context("Invalid range")?;
    let (start, end) = if start.is_empty() {
        let count = end.parse::<u64>()?;
        ensure!(count > 0, "Invalid suffix range");
        (length.saturating_sub(count), length - 1)
    } else {
        (
            start.parse::<u64>()?,
            if end.is_empty() {
                length - 1
            } else {
                end.parse::<u64>()?.min(length - 1)
            },
        )
    };
    ensure!(start < length && start <= end, "Range outside file");
    Ok((start, end, true))
}
async fn stream_file(
    State(state): State<StreamState>,
    Path((token, id)): Path<(String, usize)>,
    method: Method,
    headers: HeaderMap,
) -> Response {
    if token != state.token || id != state.active.load(Ordering::Acquire) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let result = async {
        let mut stream = state.handle.stream(id).await?;
        let length = stream.len();
        let (start, end, partial) =
            match byte_range(headers.get("range").and_then(|h| h.to_str().ok()), length) {
                Ok(r) => r,
                Err(_) => {
                    return Ok(Response::builder()
                        .status(StatusCode::RANGE_NOT_SATISFIABLE)
                        .header("Content-Range", format!("bytes */{length}"))
                        .body(Body::empty())
                        .unwrap());
                }
            };
        if std::env::var_os("REPLAYER_TORRENT_TRACE").is_some() {
            eprintln!("torrent HTTP streaming range {start}-{end}");
        }
        stream.seek(std::io::SeekFrom::Start(start)).await?;
        let mut response = Response::builder()
            .status(if partial {
                StatusCode::PARTIAL_CONTENT
            } else {
                StatusCode::OK
            })
            .header("Accept-Ranges", "bytes")
            .header("Content-Type", "application/octet-stream")
            .header("Content-Length", end - start + 1)
            .header("Cache-Control", "no-store");
        if partial {
            response = response.header("Content-Range", format!("bytes {start}-{end}/{length}"));
        }
        let body = if method == Method::HEAD {
            Body::empty()
        } else {
            Body::from_stream(ReaderStream::new(stream.take(end - start + 1)))
        };
        Ok::<_, anyhow::Error>(response.body(body)?)
    }
    .await;
    result.unwrap_or_else(|_| StatusCode::SERVICE_UNAVAILABLE.into_response())
}
pub fn selftest(magnet_file: &str) -> Result<()> {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                "librqbit=info,librqbit_dht=info,librqbit_tracker_comms=info".into()
            }),
        )
        .with_ansi(false)
        .try_init();
    use crate::player::{Event as PlayerEvent, Player};
    let start = std::time::Instant::now();
    let job = Job::start(std::fs::read_to_string(magnet_file)?, cache_dir())?;
    let mut player: Option<Player> = None;
    let mut first_frame = None;
    let mut frame_count = 0;
    let mut stage = 0;
    let mut seek_completed = 0;
    let mut last = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(240) {
        for event in job.events.try_iter() {
            match event {
                Event::Files(files) => {
                    println!(
                        "metadata {:.2}s: {} video files",
                        start.elapsed().as_secs_f64(),
                        files.len()
                    );
                    for file in &files {
                        println!("file {}: {} ({} bytes)", file.id, file.name, file.size);
                    }
                    job.select(files[0].id);
                }
                Event::Ready { url, file } => {
                    println!(
                        "stream ready {:.2}s: {}",
                        start.elapsed().as_secs_f64(),
                        file.name
                    );
                    let p = Player::open_progressive(url)?;
                    p.set_volume(0.0);
                    player = Some(p);
                }
                Event::Progress(progress) => {
                    if last.elapsed() > Duration::from_secs(5) {
                        println!(
                            "download {}/{} bytes {:.2} MiB/s",
                            progress.downloaded, progress.total, progress.download_mbps
                        );
                        last = std::time::Instant::now();
                    }
                }
                Event::Error(error) => bail!(error),
            }
        }
        if let Some(p) = &mut player {
            while let Some(event) = p.poll_event() {
                match event {
                    PlayerEvent::Error(error) => bail!(error),
                    PlayerEvent::SeekCompleted { id, position } => {
                        println!("seek completed id={id} target={position:.2}");
                        seek_completed += 1;
                    }
                    PlayerEvent::Opened => println!(
                        "opened {:.2}s duration {:.2}",
                        start.elapsed().as_secs_f64(),
                        p.duration()
                    ),
                    _ => {}
                }
            }
            if let Some(frame) = p.next_frame(p.sync_time()) {
                if first_frame.is_none() {
                    println!(
                        "first frame {:.2}s pts {:.3}",
                        start.elapsed().as_secs_f64(),
                        frame.pts
                    );
                    first_frame = Some(std::time::Instant::now());
                }
                frame_count += 1;
            }
            if let Some(first) = first_frame {
                if stage == 0 && first.elapsed() > Duration::from_secs(4) {
                    p.seek(300.0);
                    stage = 1;
                    println!("seek requested 300s");
                }
                if stage == 1 && seek_completed >= 1 {
                    p.seek(600.0);
                    p.seek(120.0);
                    p.seek(60.0);
                    stage = 2;
                    println!("rapid seeks requested 600,120,60");
                }
                if stage == 2 && seek_completed >= 2 && p.position() > 61.0 && frame_count > 20 {
                    println!(
                        "PASS: magnet metadata, selected-file playback, missing-piece seek, rapid seeks; frames={frame_count}, elapsed={:.2}s",
                        start.elapsed().as_secs_f64()
                    );
                    return Ok(());
                }
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    bail!(
        "Torrent test timed out (frames={frame_count}, completed seeks={seek_completed}); peer availability may be insufficient"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn loopback_magnet_resolves_multiple_files_and_streams_only_selected_video() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let root = std::env::temp_dir().join(format!("replayer-loopback-{}", uuid::Uuid::new_v4()));
        let seed = root.join("seed");
        let download = root.join("download");
        std::fs::create_dir_all(&seed).unwrap();
        std::fs::create_dir_all(&download).unwrap();
        std::fs::write(seed.join("a.mkv"), vec![7u8; 131072]).unwrap();
        std::fs::write(seed.join("b.mp4"), vec![19u8; 131072]).unwrap();
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(3)
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            tokio::time::timeout(Duration::from_secs(20), async {
                let torrent = librqbit::create_torrent(
                    &seed,
                    librqbit::CreateTorrentOptions {
                        piece_length: Some(16384),
                        ..Default::default()
                    },
                    &librqbit::spawn_utils::BlockingSpawner::new(1),
                )
                .await
                .unwrap();
                let bytes = torrent.as_bytes().unwrap();
                let magnet = format!("magnet:?xt=urn:btih:{}", torrent.info_hash().as_string());
                let seeder = Session::new_with_opts(
                    seed.clone(),
                    SessionOptions {
                        dht: None,
                        disable_trackers: true,
                        disable_local_service_discovery: true,
                        listen: Some(librqbit::ListenerOptions {
                            listen_addr: "127.0.0.1:0".parse().unwrap(),
                            ipv4_only: true,
                            ..Default::default()
                        }),
                        ..Default::default()
                    },
                )
                .await
                .unwrap();
                let seed_handle = seeder
                    .add_torrent(
                        AddTorrent::from_bytes(bytes),
                        Some(AddTorrentOptions {
                            overwrite: true,
                            output_folder: Some(seed.to_string_lossy().into_owned()),
                            ..Default::default()
                        }),
                    )
                    .await
                    .unwrap()
                    .into_handle()
                    .unwrap();
                while !seed_handle.stats().finished {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                let peer = seeder.listen_addr().unwrap();
                let leecher = Session::new_with_opts(
                    download.clone(),
                    SessionOptions {
                        dht: None,
                        disable_trackers: true,
                        disable_local_service_discovery: true,
                        ..Default::default()
                    },
                )
                .await
                .unwrap();
                let response = leecher
                    .add_torrent(
                        AddTorrent::from_url(magnet),
                        Some(AddTorrentOptions {
                            list_only: true,
                            initial_peers: Some(vec![peer]),
                            ..Default::default()
                        }),
                    )
                    .await
                    .unwrap();
                let AddTorrentResponse::ListOnly(list) = response else {
                    panic!("missing metadata")
                };
                let names = list
                    .info
                    .iter_file_details()
                    .map(|f| f.filename.to_string())
                    .collect::<Vec<_>>();
                assert_eq!(names.len(), 2);
                let selected = names.iter().position(|n| n == "b.mp4").unwrap();
                let other = 1 - selected;
                let handle = leecher
                    .add_torrent(
                        AddTorrent::from_bytes(list.torrent_bytes),
                        Some(AddTorrentOptions {
                            only_files: Some(vec![selected]),
                            initial_peers: Some(vec![peer]),
                            output_folder: Some(download.to_string_lossy().into_owned()),
                            ..Default::default()
                        }),
                    )
                    .await
                    .unwrap()
                    .into_handle()
                    .unwrap();
                while matches!(
                    handle.stats().state,
                    librqbit::TorrentStatsState::Initializing { .. }
                ) {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                let state = StreamState {
                    handle: handle.clone(),
                    active: Arc::new(AtomicUsize::new(selected)),
                    token: "test".into(),
                    api: Api::new(leecher.clone(), None),
                    cache_misses: Arc::new(AtomicU64::new(0)),
                };
                let mut headers = HeaderMap::new();
                headers.insert("range", "bytes=65536-65599".parse().unwrap());
                let response = stream_file(
                    State(state),
                    Path(("test".into(), selected)),
                    Method::GET,
                    headers,
                )
                .await;
                assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
                let content = axum::body::to_bytes(response.into_body(), 100)
                    .await
                    .unwrap();
                assert_eq!(&content[..], &[19u8; 64]);
                assert_eq!(handle.stats().file_progress[other], 0);
                leecher.stop().await;
                seeder.stop().await;
            })
            .await
            .expect("loopback torrent stalled");
        });
        drop(rt);
        assert!(root.starts_with(std::env::temp_dir()));
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn cached_http_never_serves_unverified_data_and_supports_ranges() {
        let root = std::env::temp_dir().join(format!("replayer-torrent-{}", uuid::Uuid::new_v4()));
        let seed = root.join("seed");
        let empty = root.join("empty");
        std::fs::create_dir_all(&seed).unwrap();
        std::fs::create_dir_all(&empty).unwrap();
        let data = (0..32768).map(|i| (i % 251) as u8).collect::<Vec<_>>();
        std::fs::write(seed.join("video.mkv"), &data).unwrap();
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let meta = librqbit::create_torrent(
                &seed.join("video.mkv"),
                librqbit::CreateTorrentOptions {
                    piece_length: Some(16384),
                    ..Default::default()
                },
                &librqbit::spawn_utils::BlockingSpawner::new(1),
            )
            .await
            .unwrap()
            .as_bytes()
            .unwrap();
            for (directory, complete) in [(&seed, true), (&empty, false)] {
                let session = Session::new_with_opts(
                    directory.clone(),
                    SessionOptions {
                        dht: None,
                        disable_trackers: true,
                        disable_local_service_discovery: true,
                        ..Default::default()
                    },
                )
                .await
                .unwrap();
                let handle = session
                    .add_torrent(
                        AddTorrent::from_bytes(meta.clone()),
                        Some(AddTorrentOptions {
                            paused: true,
                            overwrite: true,
                            output_folder: Some(directory.to_string_lossy().into_owned()),
                            ..Default::default()
                        }),
                    )
                    .await
                    .unwrap()
                    .into_handle()
                    .unwrap();
                let deadline = std::time::Instant::now() + Duration::from_secs(5);
                while matches!(
                    handle.stats().state,
                    librqbit::TorrentStatsState::Initializing { .. }
                ) {
                    assert!(std::time::Instant::now() < deadline);
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                let misses = Arc::new(AtomicU64::new(0));
                let state = StreamState {
                    handle,
                    active: Arc::new(AtomicUsize::new(0)),
                    token: "test-token".into(),
                    api: Api::new(session.clone(), None),
                    cache_misses: misses.clone(),
                };
                let mut headers = HeaderMap::new();
                headers.insert("range", "bytes=10-19".parse().unwrap());
                let response = cached_file(
                    State(state.clone()),
                    Path(("test-token".into(), 0)),
                    Method::GET,
                    headers,
                )
                .await;
                assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
                assert_eq!(response.headers()["content-range"], "bytes 10-19/32768");
                let body = axum::body::to_bytes(response.into_body(), 100).await;
                if complete {
                    assert_eq!(&body.unwrap()[..], &data[10..20]);
                    assert_eq!(misses.load(Ordering::Acquire), 0);
                } else {
                    assert!(body.is_err());
                    assert_eq!(misses.load(Ordering::Acquire), 1);
                }
                let wrong = cached_file(
                    State(state),
                    Path(("wrong-token".into(), 0)),
                    Method::GET,
                    HeaderMap::new(),
                )
                .await;
                assert_eq!(wrong.status(), StatusCode::NOT_FOUND);
                session.stop().await;
            }
        });
        drop(rt);
        assert!(root.starts_with(std::env::temp_dir()));
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn ranges_and_paths_are_bounded() {
        let rules = routing_rules("replayer.exe", "https://chat.rethinkos.com/v1");
        assert!(rules.starts_with("# Clash Verge Rev"));
        assert!(
            rules.find("IP-CIDR,127.0.0.1/32,DIRECT").unwrap()
                < rules.find("PROCESS-NAME,replayer.exe,PROXY").unwrap()
        );
        assert!(
            rules.find("PROCESS-NAME,replayer.exe,PROXY").unwrap()
                < rules.find("DOMAIN,chat.rethinkos.com,PROXY").unwrap()
        );
        assert!(!routing_rules("bad,INJECT.exe", "http://localhost:8000").contains("INJECT"));
        assert_eq!(
            byte_range(Some("bytes=10-19"), 100).unwrap(),
            (10, 19, true)
        );
        assert_eq!(byte_range(Some("bytes=80-"), 100).unwrap(), (80, 99, true));
        assert_eq!(byte_range(Some("bytes=-20"), 100).unwrap(), (80, 99, true));
        for range in ["bytes=100-", "bytes=9-2", "bytes=1-3,7-9", "bytes=-0"] {
            assert!(byte_range(Some(range), 100).is_err());
        }
        for component in ["..", "/etc", "C:evil", "a\\b", "."] {
            assert!(safe_path(&[component.into()]).is_err());
        }
        assert!(is_media("Season 1/EP01.MKV"));
        assert!(!is_media("video.exe"));
    }
}
