use super::*;
#[cfg(test)]
use crate::pixels::test_support::TestImageOutput;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}
fn png(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = std::io::Cursor::new(Vec::new());
    crate::pixels::solid(width, height, [250, 250, 250, 255])
        .write_to(&mut bytes, "png")
        .unwrap();
    bytes.into_inner()
}
async fn serve(app: axum::Router) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (base, task)
}

#[test]
fn image_negotiation_preserves_original_bytes_and_explicit_provider_headers() {
    use axum::routing::get;
    runtime().block_on(async {
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        let bytes = png(32, 64);
        let expected = bytes.clone();
        let app = axum::Router::new().route(
            "/image",
            get(move |headers: axum::http::HeaderMap| {
                let calls = observed.clone();
                let bytes = bytes.clone();
                async move {
                    let accept = headers["accept"].to_str().unwrap();
                    if calls.fetch_add(1, Ordering::Relaxed) == 0 {
                        assert_eq!(
                            accept,
                            "image/jpeg,image/webp;q=0.9,image/png;q=0.8,*/*;q=0.1"
                        );
                    } else {
                        assert_eq!(accept, "image/png");
                    }
                    // Deliberately misleading MIME: decoding must inspect the actual bytes.
                    ([("content-type", "image/jpeg")], bytes)
                }
            }),
        );
        let (base, server) = serve(app).await;
        let directory =
            std::env::temp_dir().join(format!("pordo-format-negotiation-{}", uuid::Uuid::new_v4()));
        let url: reqwest::Url = format!("{base}/image").parse().unwrap();
        let original = cache::fetch(&directory, "page", url.clone(), 1800)
            .await
            .unwrap();
        assert_eq!(std::fs::read(&original.path).unwrap(), expected);
        assert_eq!(original.hash, digest(&expected));
        cache::fetch(&directory, "page", url.clone(), 1800)
            .await
            .unwrap();
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        let mut request = source::ImageRequest::from(url);
        request
            .headers
            .insert(reqwest::header::ACCEPT, "image/png".parse().unwrap());
        cache::fetch(&directory, "another-page", request, 1800)
            .await
            .unwrap();
        assert_eq!(calls.load(Ordering::Relaxed), 2);
        server.abort();
        std::fs::remove_dir_all(directory).unwrap();
    });
}

#[test]
fn library_updates_history_and_url_migration_survive_restart() {
    let path =
        std::env::temp_dir().join(format!("pordo-comics-test-{}.sqlite", uuid::Uuid::new_v4()));
    let book = Book {
        provider: "test".into(),
        site: "site".into(),
        title: "book".into(),
        url: "https://example.com/book".into(),
        ..Default::default()
    };
    let chapter = |id: &str, name: &str| Chapter {
        id: id.into(),
        name: name.into(),
        url: format!("https://example.com/{id}"),
        order: 0,
        ..Default::default()
    };
    let mut library = library::Library::open(&path).unwrap();
    library
        .refresh(Detail {
            book: book.clone(),
            chapters: vec![chapter("1", "第1话")],
        })
        .unwrap();
    let mut entry = library.get(&book.key()).unwrap().clone();
    entry.set_favorite(true);
    entry.progress = Some(library::Progress {
        chapter: "1".into(),
        page: 3,
        fraction: 0.5,
        page_count: 10,
    });
    entry.last_read = now();
    library.save(entry).unwrap();
    library
        .refresh(Detail {
            book: book.clone(),
            chapters: vec![chapter("1", "第1话"), chapter("2", "番外")],
        })
        .unwrap();
    assert_eq!(library.get(&book.key()).unwrap().new_unread.len(), 1);
    let mut entry = library.get(&book.key()).unwrap().clone();
    entry.mark_read("1");
    library.save(entry).unwrap();
    library
        .refresh(Detail {
            book: book.clone(),
            chapters: vec![chapter("new1", "第1话"), chapter("new2", "番外")],
        })
        .unwrap();
    drop(library);
    let library = library::Library::open(&path).unwrap();
    let entry = library.get(&book.key()).unwrap();
    assert!(entry.favorite);
    assert!(entry.read.contains("new1"));
    assert_eq!(entry.progress.as_ref().unwrap().chapter, "new1");
    assert_eq!(entry.progress.as_ref().unwrap().page, 3);
    assert_eq!(entry.new_unread.len(), 1);
    drop(library);
    let _ = std::fs::remove_file(path);
}

#[test]
fn favorite_order_is_independent_of_refresh_reading_and_restart() {
    let mut library = library::Library::open(std::path::Path::new(":memory:")).unwrap();
    for (url, at) in [("older", 100), ("newer", 200), ("legacy", 0)] {
        let mut entry = library::Entry {
            book: Book {
                url: url.into(),
                ..Default::default()
            },
            favorite: true,
            favorited_at: at,
            ..Default::default()
        };
        entry.refresh(Detail {
            book: entry.book.clone(),
            chapters: vec![],
        });
        entry.last_read = u64::MAX;
        library.save(entry).unwrap();
    }
    let mut entries = library.entries.clone();
    library::sort_favorites(&mut entries);
    assert_eq!(
        entries
            .iter()
            .map(|e| e.book.url.as_str())
            .collect::<Vec<_>>(),
        ["newer", "older", "legacy"]
    );
    assert_eq!(entries[1].favorited_at, 100);
    entries[1].set_favorite(true);
    assert_eq!(
        entries[1].favorited_at, 100,
        "refreshing an existing favorite must not move it"
    );
    entries[1].set_favorite(false);
    entries[1].set_favorite(true);
    library::sort_favorites(&mut entries);
    assert_eq!(entries[0].book.url, "older");
    let legacy: library::Entry = serde_json::from_str(r#"{"favorite":true}"#).unwrap();
    assert_eq!(legacy.favorited_at, 0);
}
