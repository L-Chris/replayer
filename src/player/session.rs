use super::audio::{self, AudioPipeline};
use super::demux::{Demux, Message, PACKET_BUDGET, Seek};
use super::video::VideoPipeline;
use super::{Command, Event, PlaybackState, Shared, VideoFrame};
use anyhow::{Context, Result, bail};
use crossbeam_channel::{Receiver, Sender, TrySendError};
use ffmpeg_next as ffmpeg;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

pub(super) fn run(
    path: String,
    shared: &Arc<Shared>,
    commands: Receiver<Command>,
    frames: Sender<VideoFrame>,
    stale_frames: Receiver<VideoFrame>,
    events: &Sender<Event>,
) -> Result<()> {
    let demux = Demux::spawn(
        path,
        shared.stop.clone(),
        shared.requested.clone(),
        shared.progressive,
        shared.source_key.clone(),
    )?;
    let header = loop {
        if shared.stop.load(Ordering::Acquire) {
            return Ok(());
        }
        match demux.rx.recv_timeout(Duration::from_millis(10)) {
            Ok(Message::Opened(h)) => break h,
            Ok(Message::Error(e)) => bail!(e),
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                bail!("demux thread exited during open")
            }
            _ => {}
        }
    };
    shared.snapshot.lock().unwrap().media = Some(header.info.clone());
    let _ = events.send(Event::MediaInfo(header.info.clone()));
    if header.video.is_none() {
        return super::music::run(header, demux, shared, commands, events);
    }
    let track = header.video.as_ref().unwrap();
    let vi = track.index;
    let mut video = VideoPipeline::new(track.params.clone(), track.tb, header.origin)?;
    let mut output = None;
    let mut audio = None;
    let ai = header.audio.as_ref().map(|a| a.index);
    if let Some(track) = header.audio {
        let setup = audio::open_output(
            shared.clock.clone(),
            shared.output.clone(),
            shared.volume.clone(),
        )
        .and_then(|out| {
            AudioPipeline::new(track.params, track.tb, header.origin, out.rate)
                .map(|pipe| (out, pipe))
        });
        match setup {
            Ok((out, pipe)) => {
                output = Some(out);
                audio = Some(pipe);
            }
            Err(e) => {
                let _ = events.send(Event::Warning(format!("audio disabled: {e:#}")));
            }
        }
    }
    {
        let mut s = shared.snapshot.lock().unwrap();
        s.duration = header.duration;
        s.has_audio = audio.is_some();
        s.hardware = video.hardware;
    }
    let _ = events.send(Event::Opened);
    let mut epoch = 1;
    let mut target = 0.0;
    let mut seeking = true;
    let mut seeking_since = Instant::now();
    let mut seek_ack = true;
    let mut eof = false;
    let mut video_eof = false;
    let mut audio_eof = false;
    let mut ended = false;
    let mut audio_finished = false;
    let mut audio_fallback = false;
    let mut buffering = false;
    let mut starving_since: Option<Instant> = None;
    let mut deferred_command = None;
    let mut vpackets = VecDeque::<ffmpeg::Packet>::new();
    let mut apackets = VecDeque::<ffmpeg::Packet>::new();
    let mut packet_bytes = 0usize;
    let stats = std::env::var_os("REPLAYER_STATS").is_some();
    let mut last_stats = Instant::now();
    shared.clock.reset(epoch, target, false, audio.is_some());

    while !shared.stop.load(Ordering::Acquire) {
        let mut seek = None;
        while let Some(command) = deferred_command.take().or_else(|| commands.try_recv().ok()) {
            match command {
                Command::Playing(playing) => {
                    if !seeking && !ended && !buffering {
                        shared.clock.set_playing(playing);
                        shared.output.enabled.store(
                            playing && !audio_finished && !audio_fallback,
                            Ordering::Release,
                        );
                    }
                }
                Command::Seek { id, target } => seek = Some((id, target)),
            }
        }
        if let Some((id, position)) = seek {
            epoch = id;
            target = position;
            seeking = true;
            seeking_since = Instant::now();
            seek_ack = false;
            eof = false;
            video_eof = false;
            audio_eof = false;
            ended = false;
            audio_finished = false;
            audio_fallback = false;
            buffering = false;
            starving_since = None;
            vpackets.clear();
            apackets.clear();
            packet_bytes = 0;
            video.reset(target);
            while stale_frames.try_recv().is_ok() {}
            if let Some(a) = &mut audio {
                a.reset(target)?;
            }
            shared.output.enabled.store(false, Ordering::Release);
            shared.clock.reset(epoch, target, false, audio.is_some());
            shared
                .presented
                .store((-1.0f64).to_bits(), Ordering::Release);
            demux
                .seeks
                .send(Seek { epoch, target })
                .context("send seek to demux")?;
        }
        if shared.output.failed.swap(false, Ordering::AcqRel) {
            shared.output.enabled.store(false, Ordering::Release);
            output = None;
            audio = None;
            apackets.clear();
            packet_bytes = vpackets.iter().map(ffmpeg::Packet::size).sum();
            shared.snapshot.lock().unwrap().has_audio = false;
            shared.clock.use_wall_clock();
            let _ = events.send(Event::Warning(
                "audio device failed; continuing without audio".into(),
            ));
        }

        // Read-ahead is bounded in compressed bytes. Audio and video are then
        // scheduled independently; a full presentation queue never blocks here.
        while packet_bytes < PACKET_BUDGET {
            match demux.rx.try_recv() {
                Ok(Message::Packet(id, p)) if id == epoch && seek_ack => {
                    if p.stream() == vi {
                        packet_bytes += p.size();
                        vpackets.push_back(p);
                    } else if Some(p.stream()) == ai && audio.is_some() {
                        packet_bytes += p.size();
                        apackets.push_back(p);
                    }
                }
                Ok(Message::Seeked(id)) if id == epoch => seek_ack = true,
                Ok(Message::Eof(id)) if id == epoch => eof = true,
                Ok(Message::Error(e)) => bail!(e),
                Err(crossbeam_channel::TryRecvError::Disconnected) => {
                    bail!("demux thread exited unexpectedly")
                }
                Err(crossbeam_channel::TryRecvError::Empty) => break,
                _ => {}
            }
        }

        if let (Some(a), Some(out)) = (&mut audio, &mut output) {
            if audio_fallback && resume_audio(a, shared, epoch) {
                audio_fallback = false;
                starving_since = None;
            }
            a.pump(out);
            for _ in 0..16 {
                if !a.pending.is_empty() || out.producer.slots() <= out.capacity / 4 {
                    break;
                }
                if let Some(packet) = apackets.pop_front() {
                    packet_bytes -= packet.size();
                    a.decode(Some(&packet), epoch, target)?;
                } else if eof && !audio_eof {
                    a.decode(None, epoch, target)?;
                    audio_eof = true;
                } else {
                    break;
                }
                if audio_fallback && resume_audio(a, shared, epoch) {
                    audio_fallback = false;
                    starving_since = None;
                }
                a.pump(out);
            }
            // A sparse/ended audio track must not freeze video before the
            // demuxer can reach global EOF through its bounded read-ahead.
            if !seeking
                && !audio_eof
                && !audio_fallback
                && a.pending.is_empty()
                && apackets.is_empty()
                && out.producer.slots() == out.capacity
                && a.submitted_end
                    .is_none_or(|end| shared.clock.audio_drained(epoch, end))
            {
                if starving_since.get_or_insert_with(Instant::now).elapsed()
                    >= Duration::from_millis(250)
                    && !(shared.progressive
                        && !eof
                        && demux.waiting.load(Ordering::Acquire)
                        && video.end <= shared.clock.now() + 0.1)
                {
                    shared.clock.use_wall_clock();
                    audio_fallback = true;
                    shared.output.enabled.store(false, Ordering::Release);
                }
            } else {
                starving_since = None;
            }
        } else {
            audio_eof = eof;
        }

        let now = shared.clock.now();
        // Drop already-late pending video rather than starving the audio path.
        while video.pending.len() > 1 && video.pending.front().is_some_and(|f| f.pts + 0.1 < now) {
            video.pending.pop_front();
        }
        while let Some(frame) = video.pending.pop_front() {
            match frames.try_send(frame) {
                Ok(()) => {}
                Err(TrySendError::Full(frame)) => {
                    if frame.pts + 0.1 < now && !seeking {
                        // Replace stale queued frames, keeping the most recent
                        // frame (including the final frame) available to the UI.
                        let _ = stale_frames.try_recv();
                        video.pending.push_front(frame);
                        continue;
                    }
                    video.pending.push_front(frame);
                    break;
                }
                Err(TrySendError::Disconnected(_)) => return Ok(()),
            }
        }
        if video.pending.len() < video.queue_limit && (!ended) {
            if let Some(packet) = vpackets.pop_front() {
                packet_bytes -= packet.size();
                video.decode(Some(&packet), epoch, target)?;
            } else if eof && !video_eof {
                video.decode(None, epoch, target)?;
                video_eof = true;
            }
        }
        let video_ready = video.end > target || video_eof;
        // A track may have ended before this seek position, or have a long
        // gap. Do not wait for global EOF behind a full video read-ahead queue.
        let audio_ready = audio.as_ref().is_none_or(|a| a.end > target || audio_eof)
            || (apackets.is_empty() && seeking_since.elapsed() >= Duration::from_millis(100));
        if shared.progressive && !seeking && !ended {
            let now = shared.clock.now();
            let stalled = !eof
                && demux.waiting.load(Ordering::Acquire)
                && video.end <= now + 0.05
                && video.pending.is_empty()
                && frames.is_empty()
                && vpackets.is_empty();
            if stalled && !buffering {
                buffering = true;
                shared.clock.set_playing(false);
                shared.output.enabled.store(false, Ordering::Release);
            } else if buffering && (eof || video.end > now + 0.25) {
                buffering = false;
                let playing = shared.desired_playing.load(Ordering::Acquire);
                shared.clock.set_playing(playing);
                shared.output.enabled.store(
                    playing && !audio_fallback && !audio_finished,
                    Ordering::Release,
                );
            }
        }
        if seeking
            && seek_ack
            && video_ready
            && audio_ready
            && epoch == shared.requested.load(Ordering::Acquire)
        {
            seeking = false;
            let playing = shared.desired_playing.load(Ordering::Acquire);
            let active_audio = audio.as_ref().is_some_and(|a| a.end > target);
            audio_fallback = audio.is_some() && !active_audio;
            shared.clock.reset(epoch, target, playing, active_audio);
            shared
                .output
                .enabled
                .store(playing && active_audio, Ordering::Release);
            if epoch > 1 {
                let _ = events.send(Event::SeekCompleted {
                    id: epoch,
                    position: target,
                });
            }
        }
        if !seeking && !ended {
            if let (Some(a), Some(out)) = (&audio, &mut output) {
                if audio_eof
                    && a.pending.is_empty()
                    && out.producer.slots() == out.capacity
                    && a.submitted_end
                        .is_none_or(|end| shared.clock.audio_drained(epoch, end))
                    && !audio_finished
                {
                    audio_finished = true;
                    shared.output.enabled.store(false, Ordering::Release);
                    shared.clock.use_wall_clock();
                }
            } else {
                audio_finished = true;
            }
            let presented = f64::from_bits(shared.presented.load(Ordering::Acquire));
            if video_eof
                && audio_eof
                && audio_finished
                && video.pending.is_empty()
                && frames.is_empty()
                && (video.end <= target || presented + video.frame_duration + 0.001 >= video.end)
                && shared.clock.now() + 0.002 >= video.end
            {
                ended = true;
                shared.clock.reset(
                    epoch,
                    video.end.max(audio.as_ref().map_or(target, |a| a.end)),
                    false,
                    false,
                );
                shared.output.enabled.store(false, Ordering::Release);
                let _ = events.send(Event::Ended);
            }
        }
        {
            let mut s = shared.snapshot.lock().unwrap();
            s.epoch = epoch;
            s.hardware = video.hardware;
            s.state = if seeking {
                PlaybackState::Seeking
            } else if ended {
                PlaybackState::Ended
            } else if !shared.desired_playing.load(Ordering::Acquire) {
                PlaybackState::Paused
            } else if buffering {
                PlaybackState::Buffering
            } else if eof {
                PlaybackState::Draining
            } else {
                PlaybackState::Playing
            };
        }
        let idle = ended || (!seeking && !shared.desired_playing.load(Ordering::Acquire));
        if stats && last_stats.elapsed() >= Duration::from_secs(1) {
            last_stats = Instant::now();
            eprintln!(
                "[session] epoch={epoch} seeking={seeking} eof={eof} video_eof={video_eof} video_end={} audio_eof={audio_eof} audio_finished={audio_finished} fallback={audio_fallback} time={} audio={:?} device={:?}",
                video.end,
                shared.clock.now(),
                audio.as_ref().map(|a| (
                    a.end,
                    a.submitted_end,
                    a.pending.len(),
                    output.as_mut().map(|o| o.capacity - o.producer.slots())
                )),
                shared.clock.progress.read()
            );
        }
        match commands.recv_timeout(Duration::from_millis(if idle { 100 } else { 2 })) {
            Ok(command) => deferred_command = Some(command),
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => return Ok(()),
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
        }
    }
    Ok(())
}

fn resume_audio(audio: &mut AudioPipeline, shared: &Shared, epoch: u64) -> bool {
    if audio.pending.is_empty() {
        return false;
    }
    let position = shared.clock.now();
    audio.catch_up(position);
    if audio.pending.is_empty() {
        return false;
    }
    shared.clock.reset(
        epoch,
        position,
        shared.desired_playing.load(Ordering::Acquire),
        true,
    );
    shared.output.enabled.store(
        shared.desired_playing.load(Ordering::Acquire),
        Ordering::Release,
    );
    true
}
