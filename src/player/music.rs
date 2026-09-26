//! Audio-only driver: uses the same demuxer, audio pipeline and clock as video,
//! without a video decoder, frame queue or wall-clock fallback on audio failure.
use super::{
    Command, Event, PlaybackState, Shared,
    audio::{self, AudioPipeline},
    demux::{Demux, Header, Message, Seek},
};
use anyhow::{Context, Result, bail};
use crossbeam_channel::{Receiver, Sender};
use std::{
    collections::VecDeque,
    sync::{Arc, atomic::Ordering},
    time::{Duration, Instant},
};

/// A lost device freezes the music clock; retry reopening for this window
/// before surfacing the failure, so device switches resume transparently.
const DEVICE_RETRY_WINDOW: Duration = Duration::from_secs(10);
const DEVICE_RETRY_INTERVAL: Duration = Duration::from_millis(500);

pub(super) fn run(
    header: Header,
    demux: Demux,
    shared: &Arc<Shared>,
    commands: Receiver<Command>,
    events: &Sender<Event>,
) -> Result<()> {
    let track = header.audio.context("Music has no audio track")?;
    let params = track.params.clone();
    let tb = track.tb;
    let mut output = audio::open_output(
        shared.clock.clone(),
        shared.output.clone(),
        shared.volume.clone(),
    )
    .context("Unable to open audio output for music")?;
    let mut audio = AudioPipeline::new(track.params, track.tb, header.origin, output.rate)?;
    {
        let mut snapshot = shared.snapshot.lock().unwrap();
        snapshot.duration = header.duration;
        snapshot.has_audio = true;
    }
    let _ = events.send(Event::Opened);
    let mut epoch = 1;
    let mut target = 0.0;
    let mut seeking = true;
    let mut ack = true;
    let mut eof = false;
    let mut decoder_eof = false;
    let mut ended = false;
    let mut buffering = false;
    let mut deferred = VecDeque::<Command>::new();
    shared.clock.reset(epoch, target, false, true);
    while !shared.stop.load(Ordering::Acquire) {
        let mut seek = None;
        while let Some(command) = deferred.pop_front().or_else(|| commands.try_recv().ok()) {
            match command {
                // Internal recovery seeks carry newer ids; stale seeks are dropped
                // so the epoch never regresses below shared.requested.
                Command::Seek { id, target } if id > epoch => seek = Some((id, target)),
                Command::Playing(playing) if !seeking && !ended && !buffering => {
                    shared.clock.set_playing(playing);
                    shared.output.enabled.store(playing, Ordering::Release);
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
            decoder_eof = false;
            ended = false;
            buffering = false;
            shared.output.enabled.store(false, Ordering::Release);
            audio.reset(target)?;
            shared.clock.reset(epoch, target, false, true);
            demux
                .seeks
                .send(Seek { epoch, target })
                .context("send music seek")?;
        }
        if shared.output.failed.swap(false, Ordering::AcqRel) {
            shared.output.enabled.store(false, Ordering::Release);
            if ended {
                // The track already finished; do not fail or restart playback.
                return Ok(());
            }
            let position = shared.clock.now();
            let deadline = Instant::now() + DEVICE_RETRY_WINDOW;
            let reopened = loop {
                if shared.stop.load(Ordering::Acquire) {
                    break None;
                }
                match audio::open_output(
                    shared.clock.clone(),
                    shared.output.clone(),
                    shared.volume.clone(),
                )
                .and_then(|out| {
                    AudioPipeline::new(params.clone(), tb, header.origin, out.rate)
                        .map(|pipe| (out, pipe))
                }) {
                    Ok(pair) => break Some(pair),
                    Err(_) if Instant::now() < deadline => {
                        std::thread::sleep(DEVICE_RETRY_INTERVAL);
                    }
                    Err(_) => break None,
                }
            };
            let Some((next_output, next_pipeline)) = reopened else {
                bail!("Audio output device failed; reconnect the device and reopen the track");
            };
            output = next_output;
            audio = next_pipeline;
            let id = shared.requested.fetch_add(1, Ordering::AcqRel) + 1;
            shared.output.epoch.store(id, Ordering::Release);
            deferred.push_back(Command::Seek {
                id,
                target: position,
            });
            let _ = events.send(Event::Recovered);
        }
        audio.pump(&mut output);
        for _ in 0..32 {
            if !audio.pending.is_empty() || output.producer.slots() < output.capacity / 4 {
                break;
            }
            match demux.rx.try_recv() {
                Ok(Message::Packet(id, packet)) if id == epoch && ack => {
                    audio.decode(Some(&packet), epoch, target)?;
                    audio.pump(&mut output);
                }
                Ok(Message::Seeked(id)) if id == epoch => ack = true,
                Ok(Message::Eof(id)) if id == epoch => eof = true,
                Ok(Message::Error(error)) => bail!(error),
                Err(crossbeam_channel::TryRecvError::Disconnected) => {
                    bail!("Music demuxer exited unexpectedly")
                }
                Err(crossbeam_channel::TryRecvError::Empty) => break,
                _ => {}
            }
        }
        if eof && !decoder_eof && audio.pending.is_empty() {
            audio.decode(None, epoch, target)?;
            decoder_eof = true;
            audio.pump(&mut output);
        }
        let queued = output.capacity - output.producer.slots();
        if seeking
            && ack
            && ((audio.end > target && queued > 0) || decoder_eof)
            && epoch == shared.requested.load(Ordering::Acquire)
        {
            seeking = false;
            let playing = shared.desired_playing.load(Ordering::Acquire);
            shared.clock.reset(epoch, target, playing, true);
            shared.output.enabled.store(playing, Ordering::Release);
            if epoch > 1 {
                let _ = events.send(Event::SeekCompleted {
                    id: epoch,
                    position: target,
                });
            }
        }
        if !seeking && !ended {
            let drained = queued == 0
                && audio.pending.is_empty()
                && audio
                    .submitted_end
                    .is_none_or(|end| shared.clock.audio_drained(epoch, end));
            if decoder_eof && drained {
                ended = true;
                buffering = false;
                shared.output.enabled.store(false, Ordering::Release);
                shared.clock.reset(epoch, audio.end, false, true);
                let _ = events.send(Event::Ended);
            } else if !eof && drained && demux.waiting.load(Ordering::Acquire) && !buffering {
                buffering = true;
                shared.clock.set_playing(false);
                shared.output.enabled.store(false, Ordering::Release);
            } else if buffering && (queued >= output.rate as usize / 10 || eof) {
                buffering = false;
                let playing = shared.desired_playing.load(Ordering::Acquire);
                shared.clock.set_playing(playing);
                shared.output.enabled.store(playing, Ordering::Release);
            }
        }
        {
            let mut snapshot = shared.snapshot.lock().unwrap();
            snapshot.epoch = epoch;
            snapshot.state = if seeking {
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
        match commands.recv_timeout(Duration::from_millis(5)) {
            Ok(command) => deferred.push_back(command),
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
            _ => {}
        }
    }
    Ok(())
}
