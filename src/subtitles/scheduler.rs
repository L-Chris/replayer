use super::{Cue, Event, Options, api, audio, style};
use anyhow::{Context, Result, ensure};
use futures_util::{StreamExt, stream::FuturesUnordered};
use std::{
    collections::HashMap,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Pending,
    Running,
    Done,
}
// Completed intervals (including silence) survive seeks. Superseded requests
// return to Pending only after cancellation has released their concurrency slot.
struct Schedule {
    claimed: Vec<State>,
    start: f64,
    end: f64,
    step: f64,
}
impl Schedule {
    fn new(start: f64, end: f64, step: f64) -> Self {
        Self {
            claimed: vec![State::Pending; ((end - start) / step).ceil() as usize],
            start,
            end,
            step,
        }
    }
    fn take(&mut self, position: f64) -> Option<(f64, f64)> {
        let first = (((position - self.start).max(0.0) / self.step) as usize)
            .min(self.claimed.len().saturating_sub(1));
        // Two minutes ahead, then fill the rest chronologically.
        let ahead = (120.0 / self.step).ceil() as usize;
        let index = (first..(first + ahead).min(self.claimed.len()))
            .chain(0..self.claimed.len())
            .find(|&i| self.claimed[i] == State::Pending)?;
        self.claimed[index] = State::Running;
        let start = self.start + index as f64 * self.step;
        Some((start, (start + self.step).min(self.end)))
    }
    fn index(&self, position: f64) -> usize {
        (((position - self.start).max(0.0) / self.step) as usize).min(self.claimed.len() - 1)
    }
    fn needs_priority(&self, position: f64) -> bool {
        self.claimed[self.index(position)] == State::Pending
    }
    fn finish(&mut self, start: f64, completed: bool) {
        let index = self.index(start);
        self.claimed[index] = if completed {
            State::Done
        } else {
            State::Pending
        };
    }
}

pub(super) fn run(
    path: &Path,
    cancel: &Arc<AtomicBool>,
    options: Options,
    priority: &AtomicU64,
    client: api::Client,
    mut report: impl FnMut(Event),
) -> Result<()> {
    ensure!(
        (1..=6).contains(&options.concurrency),
        "concurrency must be between 1 and 6"
    );
    let started = Instant::now();
    let mut audio = audio::AudioChunks::open(path, cancel.clone(), client.chunk_seconds())?;
    let end = options.end.unwrap_or(audio.duration);
    if end <= 0.0 {
        // Unknown-length inputs retain the bounded sequential pipeline.
        drop(audio);
        return super::generate_with_client(path, cancel, options, client, report);
    }
    let end = if audio.duration > 0.0 {
        end.min(audio.duration)
    } else {
        end
    };
    ensure!(
        options.start.is_finite() && end.is_finite() && options.start >= 0.0 && options.start < end,
        "invalid subtitle range"
    );
    let total = end - options.start;
    let mut schedule = Schedule::new(options.start, end, client.chunk_seconds() as f64);
    type Decode = (
        f64,
        f64,
        tokio::sync::oneshot::Sender<Result<Vec<audio::Chunk>>>,
    );
    let (tx, rx) = crossbeam_channel::unbounded::<Decode>();
    let worker_cancel = cancel.clone();
    let decoder = std::thread::Builder::new()
        .name("replayer-subtitle-audio".into())
        .spawn(move || {
            while let Ok((start, end, reply)) = rx.recv() {
                if worker_cancel.load(Ordering::Acquire) {
                    break;
                }
                if reply.is_closed() {
                    continue;
                }
                let result = (|| {
                    audio.set_range(start, Some(end))?;
                    let mut chunks = Vec::new();
                    while let Some(chunk) = audio.next()? {
                        if reply.is_closed() {
                            break;
                        }
                        chunks.push(chunk);
                    }
                    Ok(chunks)
                })();
                let _ = reply.send(result);
            }
        })?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let mut completed = 0.0;
    let mut fast = 0;
    let mut short = 0;
    report(Event::Progress {
        through: 0.0,
        total,
    });
    let outcome: Result<()> = runtime.block_on(async {
        let mut jobs = FuturesUnordered::new();
        let mut running: HashMap<u64, Arc<AtomicBool>> = HashMap::new();
        loop {
            ensure!(!cancel.load(Ordering::Acquire), "字幕生成已取消");
            while jobs.len() < options.concurrency {
                let position = f64::from_bits(priority.load(Ordering::Acquire));
                let Some((start, end)) = schedule.take(position) else {
                    break;
                };
                let (reply, decoded) = tokio::sync::oneshot::channel();
                tx.send((start, end, reply))
                    .context("subtitle decoder stopped")?;
                let client = &client;
                let task_cancel = Arc::new(AtomicBool::new(false));
                running.insert(start.to_bits(), task_cancel.clone());
                jobs.push(async move {
                    let work = async {
                    let chunks = decoded.await.context("subtitle decoder stopped")??;
                    let mut cues: Vec<Cue> = Vec::new();
                    let mut skipped = Vec::new();
                    for chunk in chunks {
                        ensure!(!cancel.load(Ordering::Acquire), "字幕生成已取消");
                        if chunk.samples.iter().any(|s| s.abs() >= 0.0001) {
                            let limit = chunk.start + chunk.duration();
                            match client.transcribe(&chunk, &task_cancel, options.language).await {
                                Ok(result) => cues.extend(style::prepare(result, options.language, limit)),
                                Err(error) => {
                                    ensure!(!cancel.load(Ordering::Acquire) && !task_cancel.load(Ordering::Acquire), "字幕生成已取消");
                                    skipped.push((chunk.start, limit, error.to_string()));
                                }
                            }
                        }
                    }
                    Ok::<_, anyhow::Error>((cues, skipped))
                    };
                    let result = tokio::select! {
                        result = work => result,
                        _ = async {
                            while !task_cancel.load(Ordering::Acquire) && !cancel.load(Ordering::Acquire) {
                                tokio::time::sleep(Duration::from_millis(20)).await;
                            }
                        } => Err(anyhow::anyhow!("字幕生成已取消")),
                    };
                    (start, end, task_cancel.load(Ordering::Acquire), result)
                });
            }
            if jobs.is_empty() { break; }
            let next = tokio::select! {
                result = jobs.next() => result,
                _ = tokio::time::sleep(Duration::from_millis(25)) => {
                    let position = f64::from_bits(priority.load(Ordering::Acquire));
                    if schedule.needs_priority(position)
                        && !running.values().any(|flag| flag.load(Ordering::Acquire)) {
                        // Free only one slot. Keep the other request and all completed results.
                        if let Some((_, flag)) = running.iter().max_by(|(a, _), (b, _)| {
                            (f64::from_bits(**a)-position).abs().total_cmp(&(f64::from_bits(**b)-position).abs())
                        }) { flag.store(true, Ordering::Release); }
                    }
                    continue;
                }
            };
            let Some((start, end, superseded, result)) = next else { break; };
            running.remove(&start.to_bits());
            // If a response won the race with cancellation, retain it.
            if superseded && result.is_err() {
                schedule.finish(start, false);
                continue;
            }
            let (cues, skipped) = result?;
            schedule.finish(start, true);
            for (start, end, error) in skipped {
                report(Event::Skipped { start, end, error });
            }
            let quality = style::quality(&cues, options.language);
            fast += quality.fast;
            short += quality.short;
            if !cues.is_empty() {
                report(Event::Cues(cues));
            }
            completed += end-start;
            report(Event::Progress {
                through: completed.min(total),
                total,
            });
        }
        Ok(())
    });
    cancel.store(true, Ordering::Release);
    drop(tx);
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn seek_frees_a_slot_without_waiting_for_old_http_responses() {
        use std::{
            io::{Read, Write},
            net::TcpListener,
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let cancel = Arc::new(AtomicBool::new(false));
        let priority = Arc::new(AtomicU64::new(0.0f64.to_bits()));
        let server_cancel = cancel.clone();
        let server_priority = priority.clone();
        let server = std::thread::spawn(move || {
            let mut handlers = Vec::new();
            let deadline = Instant::now() + Duration::from_secs(4);
            for index in 0..3 {
                let (mut socket, _) = loop {
                    match listener.accept() {
                        Ok(connection) => break connection,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(
                                Instant::now() < deadline,
                                "target request remained blocked by old requests"
                            );
                            std::thread::sleep(Duration::from_millis(5));
                        }
                        Err(error) => panic!("{error}"),
                    }
                };
                let cancel = server_cancel.clone();
                handlers.push(std::thread::spawn(move || {
                    socket.set_nonblocking(false).unwrap();
                    socket.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
                    if index < 2 {
                        // Deliberately never answer the two pre-seek requests.
                        while !cancel.load(Ordering::Acquire) && Instant::now() < deadline {
                            std::thread::sleep(Duration::from_millis(5));
                        }
                        return;
                    }
                    let mut request = Vec::new();
                    loop {
                        let mut buffer = [0; 8192];
                        let n = socket.read(&mut buffer).unwrap();
                        assert!(n > 0);
                        request.extend_from_slice(&buffer[..n]);
                        if let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                            let header = String::from_utf8_lossy(&request[..end]).to_lowercase();
                            let length: usize = header.lines().find_map(|line| line.strip_prefix("content-length:")).unwrap().trim().parse().unwrap();
                            if request.len() >= end+4+length { break; }
                        }
                    }
                    let body = serde_json::json!({"choices":[{"message":{"content":"{\"segments\":[{\"start\":0,\"end\":1,\"text\":\"Target\"}]}"},"finish_reason":"stop"}]}).to_string();
                    write!(socket,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body).unwrap();
                }));
                if index == 1 {
                    server_priority.store(20.0f64.to_bits(), Ordering::Release);
                }
            }
            for handler in handlers {
                handler.join().unwrap();
            }
        });
        let path =
            std::env::temp_dir().join(format!("replayer-seek-priority-{}.wav", std::process::id()));
        std::fs::write(
            &path,
            audio::Chunk {
                start: 0.0,
                samples: vec![0.2; audio::RATE as usize * 30],
            }
            .wav(),
        )
        .unwrap();
        let mut received_target = false;
        let result = run(
            &path,
            &cancel,
            Options {
                concurrency: 2,
                ..Default::default()
            },
            &priority,
            api::Client::mock(format!("http://{address}/v1")),
            |event| {
                if let Event::Cues(cues) = event {
                    assert_eq!(cues[0].start, 20.0);
                    received_target = true;
                    cancel.store(true, Ordering::Release);
                }
            },
        );
        cancel.store(true, Ordering::Release);
        server.join().unwrap();
        std::fs::remove_file(path).unwrap();
        assert!(received_target);
        assert!(result.is_err()); // Explicit cancellation after the target result.
    }
    #[test]
    fn seeks_prioritize_new_position_without_duplicates_and_fill_gaps() {
        let mut s = Schedule::new(0.0, 305.0, 20.0);
        assert_eq!(s.take(0.0), Some((0.0, 20.0)));
        assert_eq!(s.take(240.0), Some((240.0, 260.0)));
        assert_eq!(s.take(40.0), Some((40.0, 60.0)));
        assert_eq!(s.take(240.0), Some((260.0, 280.0)));
        let mut all = vec![0.0, 240.0, 40.0, 260.0];
        while let Some((start, end)) = s.take(300.0) {
            assert!(!all.contains(&start));
            assert!(end <= 305.0);
            all.push(start);
        }
        assert_eq!(all.len(), 16);
        assert!(s.take(0.0).is_none());
    }
    #[test]
    fn cancelled_intervals_are_requeued_but_completed_intervals_are_preserved() {
        let mut s = Schedule::new(0.0, 180.0, 60.0);
        assert_eq!(s.take(0.0), Some((0.0, 60.0)));
        s.finish(0.0, true);
        assert!(!s.needs_priority(10.0));
        assert_eq!(s.take(60.0), Some((60.0, 120.0)));
        s.finish(60.0, false);
        assert!(s.needs_priority(65.0));
        assert_eq!(s.take(120.0), Some((120.0, 180.0)));
        assert_eq!(s.take(0.0), Some((60.0, 120.0)));
        assert!(s.take(0.0).is_none());
    }
}
