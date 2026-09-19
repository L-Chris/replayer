mod api;
mod audio;
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
            concurrency: 3,
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
}

pub enum Event {
    Progress {
        through: f64,
        total: f64,
    },
    Cues(Vec<Cue>),
    Finished,
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
}
impl Job {
    pub fn start(path: PathBuf, options: Options) -> Result<Self> {
        let (tx, events) = unbounded();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let thread = std::thread::Builder::new()
            .name("replayer-subtitles".into())
            .spawn(move || {
                let result = generate(&path, &worker_cancel, options, |event| {
                    let _ = tx.send(event);
                });
                let _ = tx.send(match result {
                    Ok(()) => Event::Finished,
                    Err(e) => Event::Failed(format!("{e:#}")),
                });
            })?;
        Ok(Self {
            events,
            cancel,
            thread: Some(thread),
        })
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
                    let cues = if chunk.samples.iter().all(|s| s.abs() < 0.0001) {
                        Vec::new()
                    } else {
                        client.transcribe(&chunk, cancel, options.language).await?
                    };
                    Ok::<_, anyhow::Error>((end, style::prepare(cues, options.language, end)))
                }
            })
            .buffered(options.concurrency);
        futures_util::pin_mut!(jobs);
        while let Some(result) = jobs.next().await {
            let (end, cues) = result?;
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

fn parse_cues(content: &str, start: f64, duration: f64) -> Result<Vec<Cue>> {
    #[derive(Deserialize)]
    struct Segment {
        #[serde(flatten)]
        cue: Cue,
        #[serde(default)]
        source: String,
    }
    #[derive(Deserialize)]
    struct Transcript {
        segments: Vec<Segment>,
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
    ensure!(result.segments.len() <= 256, "模型返回了过多字幕片段");
    let mut cues: Vec<Cue> = result
        .segments
        .into_iter()
        .map(|segment| {
            let mut cue = segment.cue;
            if segment
                .source
                .trim_end_matches(['"', '”', ' '])
                .ends_with('?')
            {
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
            cue
        })
        .collect();
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
    fn concurrent_pipeline_is_bounded_and_delivers_in_media_order() {
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
        generate_with_client(
            &path,
            &Arc::new(AtomicBool::new(false)),
            Options {
                language: Language::English,
                concurrency: 3,
                ..Default::default()
            },
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
        assert_eq!(starts, vec![0.0, 5.0, 10.0]);
        assert_eq!(peak, 3);
        assert_eq!(accepted.load(Ordering::Relaxed), 3);
    }
}
