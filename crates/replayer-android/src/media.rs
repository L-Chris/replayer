//! remedia playback sessions. Sessions are thread-pinned (same constraint as
//! the remedia Java facade): Kotlin owns one worker thread per handle and must
//! open/read/seek/artwork/info/close on it. `nativeMediaRead` blocks until the
//! next audio frame, so the worker doubles as the playback pump feeding
//! AudioTrack (float PCM, source sample rate, stereo).
use crate::{container, failure, guard, json_string, text};
use anyhow::{Context, Result, anyhow};
use jni::{
    JNIEnv,
    objects::{JClass, JString},
    sys::{jbyteArray, jlong, jstring},
};
use replayer_core::media::{self, Info, Kind};
use serde_json::json;
use std::{
    cell::RefCell,
    collections::HashMap,
    path::Path,
    sync::atomic::{AtomicI64, Ordering},
};

struct Player {
    session: remdia::Session,
    generation: u64,
    info: Info,
    duration_us: i64,
}

thread_local! {
    static PLAYERS: RefCell<HashMap<i64, Player>> = RefCell::new(HashMap::new());
}
fn next_handle() -> i64 {
    static NEXT: AtomicI64 = AtomicI64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}
fn with_player<T>(handle: i64, work: impl FnOnce(&mut Player) -> Result<T>) -> Result<T> {
    PLAYERS
        .try_with(|players| {
            let mut players = players.borrow_mut();
            let player = players
                .get_mut(&handle)
                .ok_or_else(|| anyhow!("invalid media handle"))?;
            work(player)
        })
        .unwrap_or_else(|_| Err(anyhow!("media thread exited")))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_replayer_Native_nativeMediaOpen(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    path: JString<'_>,
) -> jlong {
    let result = guard(|| -> Result<jlong> {
        let path = text(&mut env, &path)?;
        let engine = replayer_core::media_backend::engine()?;
        // Request::playback keeps the source sample rate on Android
        // (audio_rate = 0); AudioTrack is created per stream instead.
        let request = remdia::Request::playback(path.clone());
        let mut session = unsafe { engine.open(request, remdia::Cancellation::default()) }
            .with_context(|| format!("open media '{path}' through remdia"))?;
        let info = media::remdia_info(&mut session, Path::new(&path));
        let duration_us = session.info().duration_us.unwrap_or(0);
        let handle = next_handle();
        PLAYERS
            .try_with(|players| {
                players.borrow_mut().insert(
                    handle,
                    Player {
                        session,
                        generation: 0,
                        info,
                        duration_us,
                    },
                )
            })
            .map_err(|_| anyhow!("media thread exited"))?;
        Ok(handle)
    });
    failure(&mut env, result)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_replayer_Native_nativeMediaInfo(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
) -> jstring {
    let result = guard(|| -> Result<jstring> {
        with_player(handle, |player| {
            json_string(
                &mut env,
                &json!({
                    "kind": match player.info.kind {
                        Kind::Music => "music",
                        Kind::Video => "video",
                    },
                    "title": player.info.title,
                    "artist": player.info.artist,
                    "album": player.info.album,
                    "durationUs": player.duration_us,
                    "hasArtwork": player.info.artwork.is_some(),
                }),
            )
        })
    });
    failure(&mut env, result)
}

/// Embedded or sidecar artwork as an image container, null when absent.
/// Artwork is a single decoded image, emitted as one full-cover tile so the
/// Kotlin side reassembles it through the same tiles protocol as comic pages.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_replayer_Native_nativeMediaArtwork(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
) -> jbyteArray {
    let result = guard(|| -> Result<jbyteArray> {
        with_player(handle, |player| match &player.info.artwork {
            Some(artwork) => container(
                &mut env,
                &json!({
                    "width": artwork.width,
                    "height": artwork.height,
                    "tiles": [{
                        "width": artwork.width,
                        "height": artwork.height,
                        "offset": 0,
                    }],
                }),
                &artwork.rgba,
            ),
            None => Ok(std::ptr::null_mut()),
        })
    });
    failure(&mut env, result)
}

/// Next float-PCM audio frame: container metadata `{ptsUs,rate,channels,
/// samples}`, payload F32LE interleaved. Stale frames from before the last
/// seek are dropped here; null means end of stream.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_replayer_Native_nativeMediaRead(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
) -> jbyteArray {
    let result = guard(|| -> Result<jbyteArray> {
        with_player(handle, |player| {
            loop {
                let Some(frame) = player.session.read()? else {
                    return Ok(std::ptr::null_mut());
                };
                if frame.generation() < player.generation {
                    continue;
                }
                let remdia::FrameInfo::Audio {
                    pts_us,
                    rate,
                    channels,
                    samples,
                    ..
                } = *frame.info()
                else {
                    continue;
                };
                return container(
                    &mut env,
                    &json!({
                        "ptsUs": pts_us,
                        "rate": rate,
                        "channels": channels,
                        "samples": samples,
                    }),
                    frame.bytes(),
                );
            }
        })
    });
    failure(&mut env, result)
}

/// Seek and return the new stream generation.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_replayer_Native_nativeMediaSeek(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
    target_us: jlong,
) -> jlong {
    let result = guard(|| -> Result<jlong> {
        with_player(handle, |player| {
            let generation = player.session.seek(target_us)?;
            player.generation = generation;
            Ok(generation as jlong)
        })
    });
    failure(&mut env, result)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_replayer_Native_nativeMediaClose(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
) {
    let result = guard(|| -> Result<()> {
        PLAYERS
            .try_with(|players| {
                players.borrow_mut().remove(&handle);
            })
            .map_err(|_| anyhow!("media thread exited"))?;
        Ok(())
    });
    failure(&mut env, result);
}
