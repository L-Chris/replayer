//! Online music sources, cancellable downloads with progress polling, lyrics
//! and cover art. Download jobs run on the shared core worker runtime; the
//! Kotlin side polls for progress instead of receiving native callbacks.
use crate::{failure, guard, image_container, json_string, text};
use anyhow::{Context, Result, anyhow, ensure};
use jni::{
    JNIEnv,
    objects::{JClass, JString},
    sys::{jbyteArray, jint, jlong, jstring},
};
use pordo_core::{
    comics::{cache, source::http_url},
    jobs::{self, Job},
    lyrics::Lyrics,
    music_sources::{self, MusicSource, OnlineSong, Settings, Song},
    paths,
};
use serde_json::json;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, MutexGuard, OnceLock,
        atomic::{AtomicI64, Ordering},
    },
};

fn downloads() -> &'static Mutex<HashMap<i64, Job<PathBuf>>> {
    static JOBS: OnceLock<Mutex<HashMap<i64, Job<PathBuf>>>> = OnceLock::new();
    JOBS.get_or_init(Default::default)
}
fn next_handle() -> i64 {
    static NEXT: AtomicI64 = AtomicI64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}
fn locked<T>(guard: &Mutex<T>) -> Result<MutexGuard<'_, T>> {
    guard
        .lock()
        .map_err(|_| anyhow!("download registry lock poisoned"))
}
fn find(key: &str, id: &str) -> Result<Arc<dyn MusicSource>> {
    music_sources::sources(key)?
        .into_iter()
        .find(|source| source.descriptor().id == id)
        .with_context(|| format!("音乐源暂不可用 / Music source is unavailable: {id}"))
}
fn song(value: &str, id: &str) -> Result<Song> {
    let online: OnlineSong =
        serde_json::from_str(value).map_err(|_| anyhow!("无效歌曲记录 / Invalid song"))?;
    ensure!(
        online.source == id,
        "歌曲与音乐源不匹配 / Song does not match the source"
    );
    online.song()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_pordo_Native_nativeMusicSources(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    api_key: JString<'_>,
) -> jstring {
    let result = guard(|| -> Result<jstring> {
        let descriptors = music_sources::sources(&text(&mut env, &api_key)?)?
            .iter()
            .map(|source| {
                let descriptor = source.descriptor();
                json!({
                    "id": descriptor.id,
                    "name": descriptor.name,
                    "qualities": descriptor
                        .qualities
                        .iter()
                        .map(|quality| json!({"id": quality.id, "name": quality.name}))
                        .collect::<Vec<_>>(),
                    "defaultQuality": descriptor.default_quality,
                })
            })
            .collect::<Vec<_>>();
        json_string(&mut env, &json!(descriptors))
    });
    failure(&mut env, result)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_pordo_Native_nativeMusicSearch(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    api_key: JString<'_>,
    source_id: JString<'_>,
    keyword: JString<'_>,
    page: jint,
) -> jstring {
    let result = guard(|| -> Result<jstring> {
        let key = text(&mut env, &api_key)?;
        let id = text(&mut env, &source_id)?;
        let keyword = text(&mut env, &keyword)?;
        let source = find(&key, &id)?;
        let result = jobs::block_on(source.search(&keyword, page.max(0) as usize))?;
        let songs = result
            .songs
            .iter()
            .map(OnlineSong::from)
            .map(serde_json::to_value)
            .collect::<serde_json::Result<Vec<_>>>()?;
        json_string(
            &mut env,
            &json!({"songs": songs, "hasMore": result.has_more}),
        )
    });
    failure(&mut env, result)
}

/// Start resolve+download+enrich (probe, cover and lyric sidecars). Returns a
/// job handle for `nativeMusicDownloadPoll`; dropping via cancel removes the
/// partial file exactly like the desktop job lifecycle.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_pordo_Native_nativeMusicDownloadStart(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    api_key: JString<'_>,
    source_id: JString<'_>,
    value: JString<'_>,
    quality: JString<'_>,
    directory: JString<'_>,
) -> jlong {
    let result = guard(|| -> Result<jlong> {
        let key = text(&mut env, &api_key)?;
        let id = text(&mut env, &source_id)?;
        let track = song(&text(&mut env, &value)?, &id)?;
        let quality = text(&mut env, &quality)?;
        let directory = text(&mut env, &directory)?;
        ensure!(!quality.trim().is_empty(), "invalid quality");
        let directory = if directory.trim().is_empty() {
            Settings::default().directory()
        } else {
            PathBuf::from(directory)
        };
        let job = Job::spawn(move |progress| async move {
            let source = find(&key, &id)?;
            music_sources::download(source.as_ref(), &track, &quality, &directory, progress).await
        });
        let handle = next_handle();
        locked(downloads())?.insert(handle, job);
        Ok(handle)
    });
    failure(&mut env, result)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_pordo_Native_nativeMusicDownloadPoll(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
) -> jstring {
    let result = guard(|| -> Result<jstring> {
        let mut jobs = locked(downloads())?;
        let value = match jobs.get(&handle) {
            None => json!({"state": "error", "error": "unknown download"}),
            Some(job) => match job.poll() {
                None => json!({"state": "running", "bytes": job.bytes.load(Ordering::Relaxed)}),
                Some(Ok(path)) => {
                    jobs.remove(&handle);
                    json!({"state": "done", "path": path.display().to_string()})
                }
                Some(Err(error)) => {
                    jobs.remove(&handle);
                    json!({"state": "error", "error": format!("{error:#}")})
                }
            },
        };
        json_string(&mut env, &value)
    });
    failure(&mut env, result)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_pordo_Native_nativeMusicDownloadCancel(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    handle: jlong,
) {
    let result = guard(|| -> Result<()> {
        locked(downloads())?.remove(&handle);
        Ok(())
    });
    failure(&mut env, result);
}

/// Parsed `<stem>.lrc` sidecar of a downloaded or local track, else null.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_pordo_Native_nativeMusicLyrics(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    path: JString<'_>,
) -> jstring {
    let result = guard(|| -> Result<jstring> {
        let value = match Lyrics::load_for(Path::new(&text(&mut env, &path)?)) {
            Some(lyrics) => json!({
                "synced": lyrics.synced,
                "lines": lyrics
                    .lines
                    .iter()
                    .map(|line| json!({"time": line.time, "text": line.text}))
                    .collect::<Vec<_>>(),
            }),
            None => json!(null),
        };
        json_string(&mut env, &value)
    });
    failure(&mut env, result)
}

/// Online cover through the shared music-cover cache (desktop-compatible keys).
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_pordo_Native_nativeMusicCover(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    url: JString<'_>,
    width: jint,
) -> jbyteArray {
    let result = guard(|| -> Result<jbyteArray> {
        let url = http_url(&text(&mut env, &url)?)?;
        ensure!(width > 0, "invalid cover width");
        let key = url.to_string();
        let image = jobs::block_on(cache::fetch(
            &paths::root().join("music-covers"),
            &key,
            url,
            width as u32,
        ))?;
        image_container(&mut env, &image)
    });
    failure(&mut env, result)
}
