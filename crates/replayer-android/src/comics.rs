//! Comic discovery, page/cover image fetching and the bookshelf library.
use crate::{failure, guard, image_container, json_string, text};
use anyhow::{Result, anyhow, ensure};
use jni::{
    JNIEnv,
    objects::{JClass, JString},
    sys::{jbyteArray, jint, jstring},
};
use replayer_core::{
    comics::{self, Book, Chapter, Detail, cache, library::Library, source::ComicSource},
    jobs,
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

/// Android ships Komiic only; aggregator providers stay desktop-exclusive.
fn source() -> Result<&'static Arc<dyn ComicSource>> {
    static SOURCE: OnceLock<Result<Arc<dyn ComicSource>, String>> = OnceLock::new();
    SOURCE
        .get_or_init(|| comics::source::Komiic::connect().map_err(|error| error.to_string()))
        .as_ref()
        .map_err(|error| anyhow!("{error}"))
}
fn library() -> Result<&'static Mutex<Library>> {
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
fn locked<T>(guard: &Mutex<T>) -> Result<MutexGuard<'_, T>> {
    guard
        .lock()
        .map_err(|_| anyhow!("comic library lock poisoned"))
}
fn parse_book(value: &str) -> Result<Book> {
    serde_json::from_str(value).map_err(|_| anyhow!("无效漫画记录 / Invalid comic book"))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_replayer_Native_nativeComicSearch(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    keyword: JString<'_>,
) -> jstring {
    let result = guard(|| -> Result<jstring> {
        let keyword = text(&mut env, &keyword)?;
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
pub extern "system" fn Java_dev_replayer_Native_nativeComicDetail(
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
pub extern "system" fn Java_dev_replayer_Native_nativeComicPages(
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
pub extern "system" fn Java_dev_replayer_Native_nativeComicImagePolicy(
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
pub extern "system" fn Java_dev_replayer_Native_nativeComicFetchPage(
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
pub extern "system" fn Java_dev_replayer_Native_nativeComicFetchCover(
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
pub extern "system" fn Java_dev_replayer_Native_nativeLibraryEntries(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
) -> jstring {
    let result = guard(|| -> Result<jstring> {
        let entries = &locked(library()?)?.entries;
        json_string(&mut env, &serde_json::to_value(entries)?)
    });
    failure(&mut env, result)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_replayer_Native_nativeLibrarySave(
    mut env: JNIEnv<'_>,
    _: JClass<'_>,
    value: JString<'_>,
) {
    let result = guard(|| -> Result<()> {
        let entry = serde_json::from_str(&text(&mut env, &value)?)
            .map_err(|_| anyhow!("无效书架记录 / Invalid library entry"))?;
        locked(library()?)?.save(entry)
    });
    failure(&mut env, result);
}

/// Merge freshly fetched details into the shelf, preserving read state.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_replayer_Native_nativeLibraryRefresh(
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
pub extern "system" fn Java_dev_replayer_Native_nativeLibraryRoot(
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
