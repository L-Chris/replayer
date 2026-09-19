mod api;
mod audio;
mod mp3;
mod scheduler;
pub use api::Config as LlmConfig;
pub mod style;
use crate::settings::Language;
use futures_util::StreamExt;
use std::time::Instant;

#[derive(Clone, Copy, Debug)]
pub struct Options {
    pub language: Language,
    pub concurrency: usize,
    pub start: f64,
    pub end: Option<f64>,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            language: Language::Chinese,
            concurrency: 2,
            start: 0.0,
            end: None,
        }
    }
}

use anyhow::{Context, Result, ensure};
use crossbeam_channel::{Receiver, Sender, unbounded};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Cue {
    pub start: f64,
    pub end: f64,
    pub text: String,
    #[serde(default)]
    pub source: Option<String>,
}

pub enum Event {
    Progress {
        through: f64,
        total: f64,
    },
    Cues(Vec<Cue>),
    Finished,
    Skipped {
        start: f64,
        end: f64,
        error: String,
    },
    Failed(String),
    Metrics {
        seconds: f64,
        peak: usize,
        fast: usize,
        short: usize,
    },
}
pub struct Job {
    pub events: Receiver<Event>,
    cancel: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    priority: Arc<std::sync::atomic::AtomicU64>,
}
impl Job {
    pub fn start_at(
        path: PathBuf,
        options: Options,
        position: f64,
        config: LlmConfig,
    ) -> Result<Self> {
        let (tx, events) = unbounded();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let priority = Arc::new(std::sync::atomic::AtomicU64::new(position.to_bits()));
        let worker_priority = priority.clone();
        let thread = std::thread::Builder::new()
            .name("replayer-subtitles".into())
            .spawn(move || {
                let result = scheduler::run(
                    &path,
                    &worker_cancel,
                    options,
                    &worker_priority,
                    match api::Client::new(config) {
                        Ok(client) => client,
                        Err(error) => {
                            let _ = tx.send(Event::Failed(error.to_string()));
                            return;
                        }
                    },
                    |event| {
                        let _ = tx.send(event);
                    },
                );
                let _ = tx.send(match result {
                    Ok(()) => Event::Finished,
                    Err(e) => Event::Failed(format!("{e:#}")),
                });
            })?;
        Ok(Self {
            events,
            cancel,
            thread: Some(thread),
            priority,
        })
    }
    pub fn prioritize(&self, position: f64) {
        if position.is_finite() && position >= 0.0 {
            self.priority.store(position.to_bits(), Ordering::Release);
        }
    }
}
impl Drop for Job {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = std::thread::Builder::new()
                .name("replayer-subtitle-reaper".into())
                .spawn(move || {
                    let _ = thread.join();
                });
        }
    }
}

fn generate(
    path: &Path,
    cancel: &Arc<AtomicBool>,
    options: Options,
    report: impl FnMut(Event),
) -> Result<()> {
    let config = api::Config::load()?;
    generate_with_client(path, cancel, options, api::Client::new(config)?, report)
}
fn generate_with_client(
    path: &Path,
    cancel: &Arc<AtomicBool>,
    options: Options,
    client: api::Client,
    mut report: impl FnMut(Event),
) -> Result<()> {
    ensure!(
        (1..=6).contains(&options.concurrency),
        "concurrency must be between 1 and 6"
    );
    let started = Instant::now();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let mut audio = audio::AudioChunks::open(path, cancel.clone(), client.chunk_seconds())?;
    ensure!(
        audio.duration == 0.0 || options.start < audio.duration,
        "subtitle range starts beyond media duration"
    );
    audio.set_range(options.start, options.end)?;
    let end = options.end.map_or(audio.duration, |end| {
        if audio.duration > 0.0 {
            end.min(audio.duration)
        } else {
            end
        }
    });
    let total = (end - options.start).max(0.0);
    report(Event::Progress {
        through: 0.0,
        total,
    });
    // Blocking FFmpeg reads stay off the async executor. Both the decode queue
    // and the ordered request window are bounded by the configured concurrency.
    let (sender, receiver) = tokio::sync::mpsc::channel(options.concurrency);
    let decoder = std::thread::Builder::new()
        .name("replayer-subtitle-audio".into())
        .spawn(move || {
            loop {
                match audio.next() {
                    Ok(Some(chunk)) => {
                        if sender.blocking_send(Ok(chunk)).is_err() {
                            break;
                        }
                    }
                    Ok(None) => break,
                    Err(error) => {
                        let _ = sender.blocking_send(Err(error));
                        break;
                    }
                }
            }
        })?;
    let mut fast = 0;
    let mut short = 0;
    let outcome: Result<()> = runtime.block_on(async {
        let source = futures_util::stream::unfold(receiver, |mut receiver| async {
            receiver.recv().await.map(|item| (item, receiver))
        });
        let jobs = source
            .map(|item: Result<audio::Chunk>| {
                let client = &client;
                async move {
                    let chunk = item?;
                    ensure!(!cancel.load(Ordering::Acquire), "字幕生成已取消");
                    let end = chunk.start + chunk.duration();
                    let result = if chunk.samples.iter().all(|s| s.abs() < 0.0001) {
                        Ok(Vec::new())
                    } else {
                        client.transcribe(&chunk, cancel, options.language).await
                    };
                    ensure!(!cancel.load(Ordering::Acquire), "字幕生成已取消");
                    Ok::<_, anyhow::Error>((chunk.start, end, result))
                }
            })
            .buffered(options.concurrency);
        futures_util::pin_mut!(jobs);
        while let Some(result) = jobs.next().await {
            let (start, end, result) = result?;
            let cues = match result {
                Ok(cues) => style::prepare(cues, options.language, end),
                Err(error) => {
                    report(Event::Skipped {
                        start,
                        end,
                        error: error.to_string(),
                    });
                    Vec::new()
                }
            };
            let quality = style::quality(&cues, options.language);
            fast += quality.fast;
            short += quality.short;
            if !cues.is_empty() {
                report(Event::Cues(cues));
            }
            report(Event::Progress {
                through: (end - options.start).max(0.0),
                total,
            });
        }
        Ok(())
    });
    // All async requests/receiver have been dropped before joining the producer.
    cancel.store(true, Ordering::Release);
    let joined = decoder.join();
    report(Event::Metrics {
        seconds: started.elapsed().as_secs_f64(),
        peak: client.peak(),
        fast,
        short,
    });
    outcome?;
    ensure!(joined.is_ok(), "subtitle audio thread panicked");
    Ok(())
}

pub fn active(cues: &[Cue], position: f64) -> Option<&str> {
    let index = cues.partition_point(|cue| cue.start <= position);
    index
        .checked_sub(1)
        .and_then(|i| cues.get(i))
        .filter(|c| position < c.end)
        .map(|c| c.text.as_str())
}
pub fn to_srt(cues: &[Cue]) -> String {
    let mut output = String::new();
    for (index, cue) in cues.iter().enumerate() {
        output.push_str(&format!(
            "{}\n{} --> {}\n{}\n\n",
            index + 1,
            timestamp(cue.start),
            timestamp(cue.end),
            cue.text
        ));
    }
    output
}
fn timestamp(seconds: f64) -> String {
    let millis = (seconds.max(0.0) * 1000.0).round() as u64;
    format!(
        "{:02}:{:02}:{:02},{:03}",
        millis / 3_600_000,
        millis / 60_000 % 60,
        millis / 1000 % 60,
        millis % 1000
    )
}

#[cfg(test)]
fn parse_cues(content: &str, start: f64, duration: f64) -> Result<Vec<Cue>> {
    parse_cues_mode(content, start, duration, false)
}
fn parse_cues_mode(content: &str, start: f64, duration: f64, bilingual: bool) -> Result<Vec<Cue>> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Row {
        s: f64,
        e: f64,
        t: String,
        #[serde(default)]
        o: Option<String>,
    }
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Transcript {
        Compact { c: Vec<Row> },
        Legacy { segments: Vec<Cue> },
    }
    let text = content.trim();
    let text = if text.starts_with("```") {
        text.split_once('\n')
            .and_then(|(_, body)| body.rsplit_once("```").map(|(json, _)| json))
            .context("字幕 JSON 代码块不完整")?
    } else {
        text
    };
    let result: Transcript = serde_json::from_str(text)
        .context("模型没有返回有效的字幕 JSON，请重试或更换支持音频的模型")?;
    let mut cues = match result {
        Transcript::Compact { c: rows } => rows
            .into_iter()
            .map(|row| Cue {
                start: row.s,
                end: row.e,
                text: row.t,
                source: row.o,
            })
            .collect::<Vec<_>>(),
        Transcript::Legacy { segments } => segments,
    };
    ensure!(cues.len() <= 256, "模型返回了过多字幕片段");
    for cue in &mut cues {
        if let Some(source) = &mut cue.source {
            *source = source.trim().replace('\r', "");
            ensure!(
                source.chars().count() <= 1000,
                "模型返回的字幕文本为空或过长"
            );
            // Compatibility with older responses that used the original as a question hint.
            if source.trim_end_matches(['"', '”', ' ']).ends_with('?') {
                let text = cue.text.trim();
                let core = text.trim_end_matches(['"', '”', '’', '」', '』']);
                if !core.ends_with(['？', '?']) {
                    cue.text = format!(
                        "{}?{}",
                        core.trim_end_matches(['。', '.', ' ']),
                        &text[core.len()..]
                    );
                }
            }
        }
        if !bilingual
            || cue
                .source
                .as_ref()
                .is_some_and(|s| s.is_empty() || s.trim() == cue.text.trim())
        {
            cue.source = None;
        }
    }
    for cue in &mut cues {
        ensure!(
            cue.start.is_finite()
                && cue.end.is_finite()
                && cue.start >= 0.0
                && cue.end > cue.start
                && cue.start < duration
                && cue.end <= duration + 0.75,
            "模型返回的字幕时间戳超出音频片段范围"
        );
        cue.text = cue.text.trim().replace('\r', "");
        ensure!(
            !cue.text.is_empty() && cue.text.chars().count() <= 1000,
            "模型返回的字幕文本为空或过长"
        );
        cue.start += start;
        cue.end = cue.end.min(duration) + start;
    }
    cues.sort_by(|a, b| a.start.total_cmp(&b.start));
    Ok(cues)
}

pub fn run_cli(path: &Path, output: &Path, options: Options) -> Result<()> {
    ensure!(!output.exists(), "SRT 目标文件已存在，请选择新文件名");
    let mut cues = Vec::new();
    generate(
        path,
        &Arc::new(AtomicBool::new(false)),
        options,
        |event| match event {
            Event::Cues(new) => cues.extend(new),
            Event::Skipped { start, end, error } => {
                eprintln!("Skipped {start:.1}–{end:.1}s: {error}")
            }
            Event::Progress { through, total } => {
                println!(
                    "{}: {through:.1} / {total:.1} s",
                    options.language.text("字幕进度", "Subtitle progress")
                )
            }
            Event::Metrics {
                seconds,
                peak,
                fast,
                short,
            } => println!(
                "metrics: elapsed={seconds:.2}s peak_requests={peak} fast_cues={fast} short_cues={short}"
            ),
            _ => {}
        },
    )?;
    // CLI never silently overwrites existing user subtitles.
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .context("创建 SRT 文件失败（目标文件可能已存在）")?;
    use std::io::Write;
    file.write_all(to_srt(&cues).as_bytes())?;
    println!("已生成 {} 条字幕: {}", cues.len(), output.display());
    Ok(())
}

pub fn export_dialog(
    cues: &[Cue],
    name: String,
    result: Sender<Result<PathBuf, String>>,
    language: Language,
) {
    let data = to_srt(cues);
    std::thread::spawn(move || {
        if let Some(path) = rfd::FileDialog::new()
            .set_title(language.text("导出字幕", "Export subtitles"))
            .add_filter(language.text("SubRip 字幕", "SubRip subtitles"), &["srt"])
            .set_file_name(name)
            .save_file()
        {
            let saved = std::fs::write(&path, data).map(|_| path).map_err(|e| {
                format!(
                    "{}: {e}",
                    language.text("导出字幕失败", "Failed to export subtitles")
                )
            });
            let _ = result.send(saved);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compact_protocol_is_strict_and_bilingual_is_opt_in() {
        let mono =
            parse_cues_mode(r#"{"c":[{"s":0.25,"e":1.5,"t":"你好"}]}"#, 60.0, 2.0, false).unwrap();
        assert_eq!(mono[0].start, 60.25);
        assert_eq!(mono[0].text, "你好");
        assert!(mono[0].source.is_none());
        let dual = r#"{"c":[{"s":0,"e":1,"t":"你好","o":"Hello."}]}"#;
        assert_eq!(
            parse_cues_mode(dual, 0.0, 2.0, true).unwrap()[0]
                .source
                .as_deref(),
            Some("Hello.")
        );
        assert!(
            parse_cues_mode(dual, 0.0, 2.0, false).unwrap()[0]
                .source
                .is_none()
        );
        assert!(
            parse_cues_mode(r#"{"c":[]}"#, 0.0, 2.0, false)
                .unwrap()
                .is_empty()
        );
        for invalid in [
            r#"{"c":[{"s":0,"e":1}]}"#,
            r#"{"c":[{"s":0,"e":1,"t":7}]}"#,
            r#"{"c":[{"s":2,"e":3,"t":"late"}]}"#,
            r#"{"c":[{"s":0,"e":1,"t":"ok","extra":1}]}"#,
            r#"[[0,1]]"#,
            r#"[[0,1,"ok","source","extra"]]"#,
            r#"[[0,1,7]]"#,
            r#"[[2,3,"late"]]"#,
        ] {
            assert!(parse_cues_mode(invalid, 0.0, 2.0, false).is_err());
        }
        assert!(
            parse_cues_mode(
                r#"{"c":[{"s":0,"e":1,"t":"Hello","o":"Hello"}]}"#,
                0.0,
                2.0,
                true
            )
            .unwrap()[0]
                .source
                .is_none()
        );
    }
    #[test]
    fn exhausted_retries_skip_one_segment_and_continue_in_both_pipelines() {
        use std::io::{Read, Write};
        for prioritized in [false, true] {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let address = listener.local_addr().unwrap();
            let server = std::thread::spawn(move || {
                let deadline = Instant::now() + std::time::Duration::from_secs(12);
                for attempt in 0..4 {
                    let (mut socket, _) = loop {
                        match listener.accept() {
                            Ok(connection) => break connection,
                            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                                assert!(Instant::now() < deadline, "later segment never requested");
                                std::thread::sleep(std::time::Duration::from_millis(5));
                            }
                            Err(error) => panic!("{error}"),
                        }
                    };
                    socket.set_nonblocking(false).unwrap();
                    socket
                        .set_read_timeout(Some(std::time::Duration::from_secs(3)))
                        .unwrap();
                    let mut request = Vec::new();
                    loop {
                        let mut bytes = [0; 8192];
                        let n = socket.read(&mut bytes).unwrap();
                        assert!(n > 0);
                        request.extend_from_slice(&bytes[..n]);
                        if let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                            let headers = String::from_utf8_lossy(&request[..end]).to_lowercase();
                            let length: usize = headers
                                .lines()
                                .find_map(|line| line.strip_prefix("content-length:"))
                                .unwrap()
                                .trim()
                                .parse()
                                .unwrap();
                            if request.len() >= end + 4 + length {
                                break;
                            }
                        }
                    }
                    let (status, body) = if attempt < 3 {
                        (
                            "500 Internal Server Error",
                            r#"{"error":{"message":"persistent failure test-key"}}"#.to_owned(),
                        )
                    } else {
                        ("200 OK", serde_json::json!({"choices":[{"message":{"content":"{\"segments\":[{\"start\":0,\"end\":0.5,\"text\":\"Later\"}]}"},"finish_reason":"stop"}]}).to_string())
                    };
                    write!(socket,"HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
                }
            });
            let path = std::env::temp_dir().join(format!(
                "replayer-skip-{}-{prioritized}.wav",
                std::process::id()
            ));
            std::fs::write(
                &path,
                audio::Chunk {
                    start: 0.0,
                    samples: vec![0.2; audio::RATE as usize * 6],
                }
                .wav(),
            )
            .unwrap();
            let cancel = Arc::new(AtomicBool::new(false));
            let options = Options {
                concurrency: 1,
                ..Default::default()
            };
            let client = api::Client::mock(format!("http://{address}/v1"));
            let mut skipped = Vec::new();
            let mut cues = Vec::new();
            let mut through = 0.0;
            let report = |event| match event {
                Event::Skipped { start, end, error } => skipped.push((start, end, error)),
                Event::Cues(new) => cues.extend(new),
                Event::Progress { through: value, .. } => through = value,
                _ => {}
            };
            let result = if prioritized {
                scheduler::run(
                    &path,
                    &cancel,
                    options,
                    &std::sync::atomic::AtomicU64::new(0.0f64.to_bits()),
                    client,
                    report,
                )
            } else {
                generate_with_client(&path, &cancel, options, client, report)
            };
            server.join().unwrap();
            std::fs::remove_file(path).unwrap();
            result.unwrap();
            assert_eq!(skipped.len(), 1);
            assert_eq!((skipped[0].0, skipped[0].1), (0.0, 5.0));
            assert!(!skipped[0].2.contains("test-key"));
            assert_eq!(cues[0].start, 5.0);
            assert_eq!(through, 6.0);
        }
    }
    #[test]
    fn timestamps_are_offset_clamped_and_seekable() {
        let cues = parse_cues(
            r#"{"segments":[{"start":0.2,"end":2.2,"text":"测试"}]}"#,
            20.0,
            2.0,
        )
        .unwrap();
        assert_eq!(active(&cues, 20.0), None);
        assert_eq!(active(&cues, 20.2), Some("测试"));
        assert_eq!(active(&cues, 22.0), None);
        assert!(to_srt(&cues).contains("00:00:20,200 --> 00:00:22,000"));
    }
    #[test]
    fn invalid_timestamps_are_not_silently_accepted() {
        assert!(
            parse_cues(
                r#"{"segments":[{"start":9,"end":10,"text":"bad"}]}"#,
                0.0,
                2.0
            )
            .is_err()
        );
        assert!(
            parse_cues(
                r#"{"segments":[{"start":1,"end":0,"text":"bad"}]}"#,
                0.0,
                2.0
            )
            .is_err()
        );
        assert!(parse_cues("not json", 0.0, 2.0).is_err());
        let cues = parse_cues(
            r#"{"segments":[{"start":0,"end":1,"source":"Help you?","text":"“需要帮忙。”"}]}"#,
            0.0,
            2.0,
        )
        .unwrap();
        assert_eq!(
            style::normalize(&cues[0].text, Language::Chinese),
            "“需要帮忙？”"
        );
    }
    #[test]
    fn fenced_json_and_empty_speech_are_supported() {
        assert!(
            parse_cues("```json\n{\"segments\":[]}\n```", 0.0, 2.0)
                .unwrap()
                .is_empty()
        );
        assert_eq!(timestamp(59.9996), "00:01:00,000");
    }

    #[test]
    fn priority_pipeline_is_bounded_and_preserves_all_timestamps() {
        use std::{
            io::{Read, Write},
            net::TcpListener,
            sync::atomic::AtomicUsize,
            time::Duration,
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let ready = Arc::new(AtomicUsize::new(0));
        let accepted = Arc::new(AtomicUsize::new(0));
        let accepted_worker = accepted.clone();
        let server = std::thread::spawn(move || {
            let mut threads = Vec::new();
            let deadline = Instant::now() + Duration::from_secs(5);
            for index in 0..3 {
                let (mut socket, _) = loop {
                    match listener.accept() {
                        Ok(connection) => break connection,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(
                                Instant::now() < deadline,
                                "three concurrent requests did not arrive"
                            );
                            std::thread::sleep(Duration::from_millis(5));
                        }
                        Err(error) => panic!("{error}"),
                    }
                };
                let ready = ready.clone();
                accepted_worker.fetch_add(1, Ordering::Relaxed);
                threads.push(std::thread::spawn(move || {
                    socket.set_nonblocking(false).unwrap();
                    socket.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                    let mut request=Vec::new();
                    let (offset,length)=loop {
                        let mut buffer=[0;8192]; let n=socket.read(&mut buffer).unwrap(); assert!(n>0); request.extend_from_slice(&buffer[..n]);
                        if let Some(end)=request.windows(4).position(|w|w==b"\r\n\r\n") {
                            let headers=std::str::from_utf8(&request[..end]).unwrap().to_lowercase();
                            let size=headers.lines().find_map(|l|l.strip_prefix("content-length:")).unwrap().trim().parse::<usize>().unwrap();
                            break(end+4,size);
                        }
                    };
                    while request.len()<offset+length { let mut buffer=[0;8192]; let n=socket.read(&mut buffer).unwrap();assert!(n>0);request.extend_from_slice(&buffer[..n]); }
                    let body:serde_json::Value=serde_json::from_slice(&request[offset..offset+length]).unwrap();
                    let prompt=body.pointer("/messages/0/content/0/text").unwrap().as_str().unwrap();
                    assert!(prompt.contains("English (en-US)"));
                    // All three HTTP requests must arrive before any response.
                    ready.fetch_add(1,Ordering::SeqCst);
                    while ready.load(Ordering::SeqCst)<3 {
                        assert!(Instant::now()<deadline,"request concurrency was below three");
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    std::thread::sleep(Duration::from_millis((2-index)*40));
                    let data=serde_json::json!({"choices":[{"message":{"content":"{\"segments\":[{\"start\":0,\"end\":0.9,\"text\":\"Hello.\"}]}"},"finish_reason":"stop"}]}).to_string();
                    write!(socket,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",data.len(),data).unwrap();
                }));
            }
            for thread in threads {
                thread.join().unwrap();
            }
        });
        let path =
            std::env::temp_dir().join(format!("replayer-concurrency-{}.wav", std::process::id()));
        std::fs::write(
            &path,
            audio::Chunk {
                start: 0.0,
                samples: vec![0.2; audio::RATE as usize * 11],
            }
            .wav(),
        )
        .unwrap();
        let mut starts = Vec::new();
        let mut peak = 0;
        scheduler::run(
            &path,
            &Arc::new(AtomicBool::new(false)),
            Options {
                language: Language::English,
                concurrency: 3,
                ..Default::default()
            },
            &std::sync::atomic::AtomicU64::new(10.0f64.to_bits()),
            api::Client::mock(format!("http://{address}/v1")),
            |event| match event {
                Event::Cues(cues) => starts.extend(cues.into_iter().map(|c| c.start)),
                Event::Metrics { peak: n, .. } => peak = n,
                _ => {}
            },
        )
        .unwrap();
        server.join().unwrap();
        std::fs::remove_file(path).unwrap();
        starts.sort_by(f64::total_cmp);
        assert_eq!(starts, vec![0.0, 5.0, 10.0]);
        assert_eq!(peak, 3);
        assert_eq!(accepted.load(Ordering::Relaxed), 3);
    }
}
