//! Playback coordination is retained in the host; codec work runs in remdia's DAG.
use super::{
    Command, Event, PlaybackState, Shared, VideoFrame,
    audio::{self, AudioPipeline},
    demux::{Demux, Message, Seek},
};
use anyhow::{Context, Result, bail};
use crossbeam_channel::{Receiver, Sender, TrySendError};
use std::{
    collections::VecDeque,
    sync::{Arc, atomic::Ordering},
    time::{Duration, Instant},
};
pub(super) fn run(
    path: String,
    s: &Arc<Shared>,
    commands: Receiver<Command>,
    frames: Sender<VideoFrame>,
    stale: Receiver<VideoFrame>,
    events: &Sender<Event>,
) -> Result<()> {
    let setup = audio::open_output(s.clock.clone(), s.output.clone(), s.volume.clone());
    let rate = setup.as_ref().map_or(48000, |o| o.rate);
    let mut output = setup.ok();
    let demux = Demux::spawn(path, s, rate)?;
    let header = loop {
        if s.stop.load(Ordering::Acquire) {
            return Ok(());
        }
        match demux.rx.recv_timeout(Duration::from_millis(10)) {
            Ok(Message::Opened(h)) => break h,
            Ok(Message::Error(e)) => bail!(e),
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                bail!("remdia worker exited during open")
            }
            _ => {}
        }
    };
    let music = !header.has_video;
    if header.has_audio && output.is_none() {
        if music {
            bail!("Unable to open audio output for music");
        }
        let _ = events.send(Event::Warning(
            "audio unavailable; continuing with video".into(),
        ));
    }
    let mut audio = if header.has_audio {
        output.as_ref().map(|o| AudioPipeline::new(o.rate))
    } else {
        output = None;
        None
    };
    {
        let mut snapshot = s.snapshot.lock().unwrap();
        snapshot.media = Some(header.info.clone());
        snapshot.duration = header.duration;
        snapshot.has_audio = audio.is_some();
    }
    let _ = events.send(Event::MediaInfo(header.info));
    let _ = events.send(Event::Opened);
    let (mut epoch, mut target) = (1u64, 0f64);
    let mut completed_seek = 0;
    let (mut seeking, mut ack, mut eof, mut ended, mut buffering, mut audio_finished) =
        (true, true, false, false, false, false);
    let mut video = VecDeque::<VideoFrame>::new();
    let (mut video_end, mut frame_duration) = (0., 1. / 30.);
    let mut next_video_pts = 0.;
    let mut deferred = VecDeque::new();
    let mut starving = None;
    let mut audio_fallback = false;
    let mut reopen_due: Option<Instant> = None;
    let mut reopen_deadline: Option<Instant> = None;
    let mut reopens = 0;
    let mut pending_rate = None;
    s.clock.reset(epoch, target, false, audio.is_some());
    while !s.stop.load(Ordering::Acquire) {
        let mut seek: Option<(u64, f64)> = None;
        while let Some(command) = deferred.pop_front().or_else(|| commands.try_recv().ok()) {
            match command {
                Command::Seek { id, target }
                    if id > epoch && seek.is_none_or(|(latest, _)| id > latest) =>
                {
                    seek = Some((id, target));
                }
                Command::Playing(playing) if !seeking && !ended && !buffering => {
                    s.clock.set_playing(playing);
                    s.output.enabled.store(
                        playing && audio.is_some() && !audio_finished && !audio_fallback,
                        Ordering::Release,
                    );
                }
                _ => {}
            }
        }
        if let Some((id, position)) = seek {
            epoch = id;
            target = position;
            seeking = true;
            ack = false;
            eof = false;
            ended = false;
            buffering = false;
            audio_finished = false;
            audio_fallback = false;
            starving = None;
            video.clear();
            video_end = position;
            next_video_pts = position;
            while stale.try_recv().is_ok() {}
            if let Some(a) = &mut audio {
                a.reset(position);
            }
            s.output.enabled.store(false, Ordering::Release);
            s.clock.reset(epoch, target, false, audio.is_some());
            s.presented.store((-1f64).to_bits(), Ordering::Release);
            demux
                .seeks
                .send(Seek {
                    epoch,
                    target,
                    rate: pending_rate.take(),
                })
                .context("send remdia seek")?;
        }
        if s.output.failed.swap(false, Ordering::AcqRel) {
            s.output.enabled.store(false, Ordering::Release);
            output = None;
            audio = None;
            if music {
                s.clock.set_playing(false);
            } else {
                s.clock.use_wall_clock();
            }
            if !ended && header.has_audio && reopens < 8 {
                reopen_due = Some(Instant::now());
                reopen_deadline = Some(Instant::now() + Duration::from_secs(10));
            } else if music && !ended {
                bail!("Music audio output device failed");
            }
        }
        if reopen_due.is_some_and(|due| Instant::now() >= due) {
            reopen_due = None;
            match audio::open_output(s.clock.clone(), s.output.clone(), s.volume.clone()) {
                Ok(out) => {
                    pending_rate = Some(out.rate);
                    audio = Some(AudioPipeline::new(out.rate));
                    output = Some(out);
                    reopens += 1;
                    reopen_deadline = None;
                    // A device reset must not supersede a pending user's target.
                    // Allocate under the same lock as Player::seek so a newer
                    // user command always wins, regardless of queue order.
                    let (id, position) = {
                        let intent = s.seek_intent.lock().unwrap();
                        let position = if intent.0 > completed_seek {
                            intent.1
                        } else {
                            s.clock.now()
                        };
                        (s.requested.fetch_add(1, Ordering::AcqRel) + 1, position)
                    };
                    s.output.epoch.fetch_max(id, Ordering::AcqRel);
                    s.cancel.interrupt();
                    deferred.push_back(Command::Seek {
                        id,
                        target: position,
                    });
                    let _ = events.send(Event::Recovered);
                }
                Err(_) if reopen_deadline.is_some_and(|due| Instant::now() < due) => {
                    reopen_due = Some(Instant::now() + Duration::from_millis(500))
                }
                Err(_) => {
                    reopen_deadline = None;
                    if music {
                        bail!("Audio output device failed; reconnect and reopen the track");
                    }
                    s.snapshot.lock().unwrap().has_audio = false;
                    let _ = events.send(Event::Warning(
                        "audio device unavailable; continuing with video".into(),
                    ));
                }
            }
        }
        if !audio_fallback && let (Some(a), Some(out)) = (&mut audio, &mut output) {
            a.pump(out);
        }
        // Read decoded outputs while audio needs them, even if the video consumer has
        // stalled. The host keeps a bounded presentation window and drops stale video.
        let now = s.clock.now();
        let wants_audio = audio.as_ref().zip(output.as_ref()).is_some_and(|(a, out)| {
            a.pending.len() < out.rate as usize / 2 && out.producer.slots() > out.capacity / 4
        });
        let wants_video = header.has_video && video_end <= now + 0.5;
        if !ended && (seeking || wants_audio || wants_video || eof) {
            for _ in 0..32 {
                if audio
                    .as_ref()
                    .is_some_and(|a| a.pending.len() > rate as usize / 2)
                {
                    break;
                }
                match demux.rx.try_recv() {
                    Ok(Message::Frame(id, frame)) if id == epoch && ack => match frame.info() {
                        remdia::FrameInfo::Audio { .. } => {
                            if let Some(a) = &mut audio {
                                a.append(&frame, epoch, target)?;
                                if !audio_fallback && let Some(out) = &mut output {
                                    a.pump(out);
                                }
                            }
                        }
                        remdia::FrameInfo::Video {
                            pts_us,
                            duration_us,
                            hardware_decoded,
                            ..
                        } => {
                            let pts = pts_us.map_or(next_video_pts, |v| v as f64 / 1e6);
                            if *duration_us > 0 {
                                frame_duration = *duration_us as f64 / 1e6;
                            }
                            next_video_pts = pts + frame_duration;
                            video_end = next_video_pts;
                            s.snapshot.lock().unwrap().hardware = *hardware_decoded;
                            let f = VideoFrame::from_remdia(epoch, pts, frame);
                            let bytes =
                                f.image().width() as usize * f.image().height() as usize * 4;
                            let limit = (64 * 1024 * 1024 / bytes.max(1)).clamp(1, 8);
                            if video.len() >= limit {
                                video.pop_front();
                            }
                            video.push_back(f);
                            // Publish the target preview before reading ahead to prime audio.
                            if seeking {
                                break;
                            }
                        }
                        _ => {}
                    },
                    Ok(Message::Seeked(id)) if id == epoch => ack = true,
                    Ok(Message::Eof(id)) if id == epoch => eof = true,
                    Ok(Message::Error(e)) => bail!(e),
                    Err(crossbeam_channel::TryRecvError::Disconnected) => {
                        if s.stop.load(Ordering::Acquire) {
                            return Ok(());
                        }
                        bail!("remdia worker exited unexpectedly");
                    }
                    Err(crossbeam_channel::TryRecvError::Empty) => break,
                    _ => {}
                }
                if !seeking && !wants_audio && video_end > now + 0.5 {
                    break;
                }
            }
        }
        if !audio_fallback && let (Some(a), Some(out)) = (&mut audio, &mut output) {
            a.pump(out);
        }
        let now = s.clock.now();
        while video.len() > 1 && video.front().is_some_and(|f| f.pts + 0.1 < now) {
            video.pop_front();
        }
        while let Some(f) = video.pop_front() {
            match frames.try_send(f) {
                Ok(()) => {}
                Err(TrySendError::Full(f)) => {
                    if f.pts + 0.1 < now && !seeking {
                        let _ = stale.try_recv();
                        video.push_front(f);
                        continue;
                    }
                    video.push_front(f);
                    break;
                }
                Err(TrySendError::Disconnected(_)) => return Ok(()),
            }
        }
        if !seeking
            && !ended
            && let (Some(a), Some(out)) = (&mut audio, &output)
        {
            let drained = a.pending.is_empty()
                && out.producer.slots() == out.capacity
                && a.submitted_end
                    .is_none_or(|end| s.clock.audio_drained(epoch, end));
            if drained && !eof {
                if !audio_fallback
                    && starving.get_or_insert_with(Instant::now).elapsed()
                        >= Duration::from_millis(250)
                {
                    if music || s.progressive {
                        if !buffering {
                            buffering = true;
                            s.clock.set_playing(false);
                            s.output.enabled.store(false, Ordering::Release);
                        }
                    } else {
                        s.clock.use_wall_clock();
                        audio_fallback = true;
                        s.output.enabled.store(false, Ordering::Release);
                    }
                }
            } else {
                starving = None;
            }
            // Keep new PCM out of the device queue until its timestamps have
            // caught up with the video wall clock after a sparse audio gap.
            if audio_fallback && !a.pending.is_empty() {
                let position = s.clock.now();
                a.catch_up(position);
                if !a.pending.is_empty() {
                    s.clock.reset(
                        epoch,
                        position,
                        s.desired_playing.load(Ordering::Acquire),
                        true,
                    );
                    s.output
                        .enabled
                        .store(s.desired_playing.load(Ordering::Acquire), Ordering::Release);
                    audio_fallback = false;
                }
            }
        }
        let video_ready = !header.has_video || video_end > target || eof;
        let audio_ready = audio.as_ref().is_none_or(|a| a.end > target || eof);
        if seeking
            && ack
            && video_ready
            && audio_ready
            && epoch == s.requested.load(Ordering::Acquire)
        {
            seeking = false;
            let active_audio = audio.as_ref().is_some_and(|a| a.end > target);
            audio_fallback = audio.is_some() && !active_audio;
            let playing = s.desired_playing.load(Ordering::Acquire);
            s.clock.reset(epoch, target, playing, active_audio);
            s.output
                .enabled
                .store(playing && active_audio, Ordering::Release);
            let intent = s.seek_intent.lock().unwrap();
            if intent.0 > completed_seek && intent.0 <= epoch {
                let _ = events.send(Event::SeekCompleted {
                    id: intent.0,
                    position: target,
                });
                completed_seek = intent.0;
            }
        }
        if buffering {
            let ready = output
                .as_ref()
                .is_some_and(|out| out.capacity - out.producer.slots() >= out.rate as usize / 10)
                || eof
                || video_end > s.clock.now() + 0.25;
            if ready {
                buffering = false;
                let playing = s.desired_playing.load(Ordering::Acquire);
                s.clock.set_playing(playing);
                s.output
                    .enabled
                    .store(playing && audio.is_some(), Ordering::Release);
            }
        }
        if s.progressive
            && !music
            && !seeking
            && !ended
            && !eof
            && demux.waiting.load(Ordering::Acquire)
            && video_end <= s.clock.now() + 0.05
            && video.is_empty()
            && frames.is_empty()
            && !buffering
        {
            buffering = true;
            s.clock.set_playing(false);
            s.output.enabled.store(false, Ordering::Release);
        }
        if eof && !seeking && !ended {
            audio_finished = audio.as_ref().zip(output.as_ref()).is_none_or(|(a, out)| {
                a.pending.is_empty()
                    && out.producer.slots() == out.capacity
                    && a.submitted_end
                        .is_none_or(|end| s.clock.audio_drained(epoch, end))
            });
            if audio_finished {
                if !music {
                    s.clock.use_wall_clock();
                }
                s.output.enabled.store(false, Ordering::Release);
            }
            let presented = f64::from_bits(s.presented.load(Ordering::Acquire));
            let video_finished = !header.has_video
                || (video.is_empty()
                    && frames.is_empty()
                    && (video_end <= target || presented + frame_duration + 0.001 >= video_end)
                    && s.clock.now() + 0.002 >= video_end);
            if audio_finished && video_finished {
                ended = true;
                let end = video_end.max(audio.as_ref().map_or(target, |a| a.end));
                s.clock.reset(epoch, end, false, false);
                let _ = events.send(Event::Ended);
            }
        }
        {
            let mut snapshot = s.snapshot.lock().unwrap();
            snapshot.epoch = epoch;
            snapshot.has_audio = audio.is_some();
            snapshot.state = if seeking {
                PlaybackState::Seeking
            } else if ended {
                PlaybackState::Ended
            } else if !s.desired_playing.load(Ordering::Acquire) {
                PlaybackState::Paused
            } else if buffering {
                PlaybackState::Buffering
            } else if eof {
                PlaybackState::Draining
            } else {
                PlaybackState::Playing
            };
        }
        let idle = ended || (!seeking && !s.desired_playing.load(Ordering::Acquire));
        match commands.recv_timeout(Duration::from_millis(if idle { 100 } else { 2 })) {
            Ok(c) => deferred.push_back(c),
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => return Ok(()),
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
        }
    }
    Ok(())
}
