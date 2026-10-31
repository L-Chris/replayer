//! Comic discovery, page/cover image fetching and the bookshelf library.
use crate::{failure, guard, image_container, json_string, text};
use anyhow::{Result, anyhow, ensure};
use jni::{
    JNIEnv,
    objects::{JClass, JString},
    sys::{jbyteArray, jint, jstring},
};
use pordo_core::{
    comics::{
        self, Book, Chapter, Detail, cache,
        library::Library,
        source::{ComicSource, Sources},
    },
    jobs,
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, RwLock};

fn connection() -> &'static RwLock<Option<Arc<Sources>>> {
    static SOURCE: OnceLock<RwLock<Option<Arc<Sources>>>> = OnceLock::new();
    SOURCE.get_or_init(|| RwLock::new(None))
}
fn source() -> Result<Arc<Sources>> {
    connection()
        .read()
        .map_err(|_| anyhow!("Content service lock poisoned"))?
        .clone()
        .ok_or_else(|| {
            anyhow!("请在源设置中配置内容服务 / Configure a content service in Source settings")
        })
}
pub(crate) fn library() -> Result<&'static Mutex<Library>> {
    static LIBRARY: OnceLock<Result<Mutex<Library>, String>> = OnceLock::new();
    LIBRARY
        .get_or_init(|| {
            Library::open(&comics::root().join("library.sqlite"))
                .map(Mutex::new)
                .map_err(|error| error.to_string())
        })
        .as_ref()
        .map_err(|error| anyhow!("{error}"))
}
pub(crate) fn locked<T>(guard: &Mutex<T>) -> Result<MutexGuard<'_, T>> {
    guard
        .lock()
        .map_err(|_| anyhow!("comic library lock poisoned"))
}
fn parse_book(value: &str) -> Result<Book> {
    serde_json::from_str(value).map_err(|_| anyhow!("无效漫画记录 / Invalid comic book"))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_pordo_Native_nativeComicServiceConfigure(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    config: JString<'_>,
) -> jstring {
    let result = guard(|| {
        let value: Value = serde_json::from_str(&text(&mut env, &config)?)?;
        let mut settings: comics::source::ServiceSettings = serde_json::from_value(value.clone())?;
        settings.token = value["token"].as_str().unwrap_or_default().into();
        settings.validate()?;
        let source = if settings.url.trim().is_empty() {
            None
        } else {
            Some(Sources::connect(&settings)?)
        };
        *connection()
            .write()
            .map_err(|_| anyhow!("Content service lock poisoned"))? = source;
        json_string(
            &mut env,
            &json!({"configured": !settings.url.trim().is_empty()}),
        )
    });
    failure(&mut env, result)
}
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_pordo_Native_nativeComicServiceTest(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
) -> jstring {
    let result = guard(|| {
        let sites = jobs::block_on(source()?.sites())?;
        json_string(
            &mut env,
            &json!({"sites": sites.iter().map(|site|json!({"siteId":site.id,"siteName":site.name})).collect::<Vec<_>>()}),
        )
    });
    failure(&mut env, result)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_pordo_Native_nativeComicSearch(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    keyword: JString<'_>,
) -> jstring {
    let result = guard(|| -> Result<jstring> {
        let keyword = text(&mut env, &keyword)?;
        ensure!(!keyword.trim().is_empty(), "请输入漫画名称 / Enter a title");
        let groups = jobs::block_on(source()?.search(&keyword))?;
        let value = groups
            .iter()
            .map(|group| -> Result<Value> {
                Ok(json!({
                    "site": {"siteId": group.site.id, "siteName": group.site.name},
                    "books": serde_json::to_value(&group.books)?,
                    "error": group.error,
                }))
            })
            .collect::<Result<Vec<_>>>()?;
        json_string(&mut env, &Value::Array(value))
    });
    failure(&mut env, result)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_pordo_Native_nativeComicDetail(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    value: JString<'_>,
) -> jstring {
    let result = guard(|| -> Result<jstring> {
        let book = parse_book(&text(&mut env, &value)?)?;
        let detail = jobs::block_on(source()?.detail(&book))?;
        json_string(
            &mut env,
            &json!({
                "book": serde_json::to_value(&detail.book)?,
                "chapters": serde_json::to_value(&detail.chapters)?,
            }),
        )
    });
    failure(&mut env, result)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_pordo_Native_nativeComicPages(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    value: JString<'_>,
    chapter: JString<'_>,
    refresh: u8,
) -> jstring {
    let result = guard(|| -> Result<jstring> {
        let book = parse_book(&text(&mut env, &value)?)?;
        let chapter: Chapter = serde_json::from_str(&text(&mut env, &chapter)?)
            .map_err(|_| anyhow!("无效章节记录 / Invalid comic chapter"))?;
        let pages = jobs::block_on(source()?.pages(&book, &chapter, refresh != 0))?;
        json_string(&mut env, &json!(pages))
    });
    failure(&mut env, result)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_pordo_Native_nativeComicImagePolicy(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    value: JString<'_>,
) -> jstring {
    let result = guard(|| -> Result<jstring> {
        let policy = source()?.image_policy(&parse_book(&text(&mut env, &value)?)?);
        json_string(
            &mut env,
            &json!({
                "concurrency": policy.concurrency,
                "ahead": policy.ahead,
                "behind": policy.behind,
                "retainCacheOnRefresh": policy.retain_cache_on_refresh,
            }),
        )
    });
    failure(&mut env, result)
}

/// Decode-at-width page fetch through the shared content-addressed cache.
/// Key scheme matches the desktop reader so caches stay interchangeable.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_pordo_Native_nativeComicFetchPage(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    value: JString<'_>,
    chapter: JString<'_>,
    index: jint,
    url: JString<'_>,
    width: jint,
) -> jbyteArray {
    let result = guard(|| -> Result<jbyteArray> {
        let book = parse_book(&text(&mut env, &value)?)?;
        let chapter: Chapter = serde_json::from_str(&text(&mut env, &chapter)?)
            .map_err(|_| anyhow!("无效章节记录 / Invalid comic chapter"))?;
        let url = text(&mut env, &url)?;
        ensure!(width > 0, "invalid page width");
        let key = format!("{}:{}:{}", book.key(), chapter.key(), index);
        let request = source()?.book_image_request(&book, &url)?;
        let image = jobs::block_on(cache::fetch(
            &comics::root().join("cache/pages"),
            &key,
            request,
            width as u32,
        ))?;
        image_container(&mut env, &image)
    });
    failure(&mut env, result)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_pordo_Native_nativeComicFetchCover(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    value: JString<'_>,
    width: jint,
) -> jbyteArray {
    let result = guard(|| -> Result<jbyteArray> {
        let book = parse_book(&text(&mut env, &value)?)?;
        ensure!(!book.cover.is_empty(), "没有封面 / No cover");
        ensure!(width > 0, "invalid cover width");
        let key = format!("cover:{}", book.cover);
        let request = source()?.book_image_request(&book, &book.cover)?;
        let image = jobs::block_on(cache::fetch(
            &comics::root().join("cache/covers"),
            &key,
            request,
            width as u32,
        ))?;
        image_container(&mut env, &image)
    });
    failure(&mut env, result)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_pordo_Native_nativeLibraryEntries(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
) -> jstring {
    let result = guard(|| -> Result<jstring> {
        let entries = &locked(library()?)?.entries;
        json_string(&mut env, &serde_json::to_value(entries)?)
    });
    failure(&mut env, result)
}

/// Apply only explicitly edited fields to the latest row, so reader writes do
/// not restore stale favorites/read sets after a concurrent cloud pull.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_pordo_Native_nativeLibraryUpdate(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    book: JString<'_>,
    changes: JString<'_>,
) {
    let result = guard(|| -> Result<()> {
        let book = parse_book(&text(&mut env, &book)?)?;
        let changes: Value = serde_json::from_str(&text(&mut env, &changes)?)?;
        let mut library = locked(library()?)?;
        let mut entry = library
            .get(&book.key())
            .cloned()
            .ok_or_else(|| anyhow!("Comic is not shelved"))?;
        if let Some(favorite) = changes["favorite"].as_bool() {
            entry.set_favorite(favorite);
        }
        if let Some(progress) = changes.get("progress") {
            let progress: comics::library::Progress = serde_json::from_value(progress.clone())?;
            ensure!(
                progress.fraction.is_finite() && (0.0..=1.0).contains(&progress.fraction),
                "Invalid comic progress"
            );
            if changes["completed"].as_bool().unwrap_or(false) {
                entry.mark_read(&progress.chapter);
            }
            entry.progress = Some(progress);
            entry.last_read = comics::now();
        }
        library.save(entry)
    });
    failure(&mut env, result);
}

/// Merge freshly fetched details into the shelf, preserving read state.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_pordo_Native_nativeLibraryRefresh(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    book: JString<'_>,
    chapters: JString<'_>,
) {
    let result = guard(|| -> Result<()> {
        let detail = Detail {
            book: parse_book(&text(&mut env, &book)?)?,
            chapters: serde_json::from_str(&text(&mut env, &chapters)?)
                .map_err(|_| anyhow!("无效章节记录 / Invalid comic chapters"))?,
        };
        locked(library()?)?.refresh(detail)
    });
    failure(&mut env, result);
}

/// Comics data directory (library database and image caches).
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_pordo_Native_nativeLibraryRoot(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
) -> jstring {
    let result = guard(|| -> Result<jstring> {
        json_string(
            &mut env,
            &Value::String(comics::root().display().to_string()),
        )
    });
    failure(&mut env, result)
}
