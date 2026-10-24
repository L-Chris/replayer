use super::*;
use source::ComicSource;
use std::sync::{
    Arc,
    atomic::{AtomicU64, AtomicUsize, Ordering},
};

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}
fn png(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(width, height, image::Rgba([250, 250, 250, 255]))
        .write_to(&mut bytes, image::ImageFormat::Png)
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
fn adapter_signs_requests_preserves_partial_failures_and_caches_images() {
    use axum::{
        Json,
        routing::{get, post},
    };
    use serde_json::json;
    runtime().block_on(async {
        let images=Arc::new(AtomicUsize::new(0));let observed=images.clone();
        let app=axum::Router::new()
            .route("/api/sites",get(||async{Json(json!({"data":[{"siteId":"a","siteName":"Source A"}]}))}))
            .route("/api/search",post(|headers:axum::http::HeaderMap,body:String|async move{
                let ts=headers["x-ym-ts"].to_str().unwrap();let nonce=headers["x-ym-nonce"].to_str().unwrap();
                assert_eq!(headers["x-ym-sign"],digest(format!("/api/search|{body}|{ts}|{nonce}|ym-salt-883a0f7e-29f1-4b72-9ad2").as_bytes()));
                assert_eq!(serde_json::from_str::<serde_json::Value>(&body).unwrap()["keyword"],"book");
                Json(json!({"data":[{"siteId":"a","siteName":"Source A","results":[{"title":"book","detailUrl":"https://upstream.example/book","coverUrl":"https://img.example/cover.jpg"}]},{"siteId":"b","siteName":"Source B","results":[],"error":"403"}]}))
            }))
            .route("/api/details",post(||async{Json(json!({"data":{"title":"Book title","author":"Author","sourceUrl":"https://upstream.example/book","chapters":[{"id":"1","name":"First","url":"https://upstream.example/1","order":0},{"id":"1","name":"Duplicate","url":"https://upstream.example/1","order":1}]}}))}))
            .route("/api/chapter-images",post(||async{Json(json!({"data":{"images":["https://img.example/page.jpg"]}}))}))
            .route("/api/image",get(move ||{let observed=observed.clone();async move{observed.fetch_add(1,Ordering::Relaxed);png(64,128)}}));
        let(base,task)=serve(app).await;let source=source::MangaYun::at(&base).unwrap();
        assert_eq!(source.sites().await.unwrap().len(),1);
        let groups=source.search(" book ").await.unwrap();assert_eq!(groups.len(),2);assert!(groups[1].error.is_some());
        let detail=source.detail(&groups[0].books[0]).await.unwrap();assert_eq!(detail.book.author,"Author");assert_eq!(detail.chapters.len(),1);
        let pages=source.pages(&detail.book,&detail.chapters[0],false).await.unwrap();assert_eq!(pages.len(),1);
        let url=source.image_url("dm5",&pages[0]).unwrap();assert_eq!(url.path(),"/api/image");
        let directory=std::env::temp_dir().join(format!("replayer-comic-cache-{}",uuid::Uuid::new_v4()));
        let first=cache::fetch(&directory,"stable-page",url.clone(),240).await.unwrap();
        let second=cache::fetch(&directory,"stable-page",url,240).await.unwrap();
        assert_eq!(first.hash,second.hash);assert_eq!(images.load(Ordering::Relaxed),1);assert_eq!(first.bitmap.height,128);
        assert!(source::http_url("file:///C:/secret").is_err());assert!(source::http_url("https://user:password@example.com").is_err());
        std::fs::remove_dir_all(directory).unwrap();task.abort();
    });
}

#[test]
fn image_routes_match_web_client_and_preserve_proxy_requests() {
    let source = source::MangaYun::at("https://mangayun.com").unwrap();
    let direct = source
        .image_request("manhuazhijia", "https://static-tw.baozimh.com/page.jpg")
        .unwrap();
    assert_eq!(direct.url.host_str(), Some("static-tw.baozimh.com"));
    assert_eq!(direct.fallback.unwrap().path(), "/api/image");
    for (site, url) in [
        ("dm5", "https://cdn.example/page.jpg"),
        ("other", "https://cdn.hamreus.com/page.jpg"),
    ] {
        let request = source.image_request(site, url).unwrap();
        assert_eq!(request.url.path(), "/api/image");
        assert!(request.fallback.is_none());
    }
    let proxied =
        "https://mangayun.com/api/image?url=https%3A%2F%2Fexample.com%2Fpage.jpg&siteId=dm5";
    let request = source.image_request("dm5", proxied).unwrap();
    assert_eq!(request.url.as_str(), proxied);
    assert!(request.fallback.is_none());
    assert!(source.image_request("a", "file:///C:/secret").is_err());
}

#[test]
fn image_relay_fallback_is_cached_and_does_not_repeat_failed_direct_requests() {
    use axum::routing::get;
    runtime().block_on(async {
        let direct = Arc::new(AtomicUsize::new(0));
        let relayed = Arc::new(AtomicUsize::new(0));
        let direct_count = direct.clone();
        let invalid_count = direct.clone();
        let relay_count = relayed.clone();
        let app = axum::Router::new()
            .route(
                "/blocked",
                get(move || {
                    let count = direct_count.clone();
                    async move {
                        count.fetch_add(1, Ordering::Relaxed);
                        axum::http::StatusCode::FORBIDDEN
                    }
                }),
            )
            .route(
                "/invalid",
                get(move || {
                    let count = invalid_count.clone();
                    async move {
                        count.fetch_add(1, Ordering::Relaxed);
                        "<html>not an image</html>"
                    }
                }),
            )
            .route(
                "/image",
                get(move || {
                    let count = relay_count.clone();
                    async move {
                        count.fetch_add(1, Ordering::Relaxed);
                        png(64, 128)
                    }
                }),
            );
        let (base, server) = serve(app).await;
        let directory =
            std::env::temp_dir().join(format!("replayer-image-fallback-{}", uuid::Uuid::new_v4()));
        for path in ["blocked", "invalid"] {
            for _ in 0..2 {
                let image = cache::fetch(
                    &directory,
                    path,
                    source::ImageRequest::from(
                        format!("{base}/{path}").parse::<reqwest::Url>().unwrap(),
                    )
                    .with_fallback(format!("{base}/image").parse().unwrap()),
                    240,
                )
                .await
                .unwrap();
                assert_eq!(image.bitmap.height, 128);
            }
        }
        assert_eq!(direct.load(Ordering::Relaxed), 2);
        assert_eq!(relayed.load(Ordering::Relaxed), 2);
        std::fs::remove_dir_all(directory).unwrap();
        server.abort();
    });
}

#[test]
fn image_edit_uses_multipart_returns_a_cached_image_and_never_retries_400() {
    use axum::{Json, routing::post};
    use base64::{Engine, engine::general_purpose::STANDARD};
    use serde_json::json;
    runtime().block_on(async {
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = calls.clone();
        let app = axum::Router::new().route(
            "/v1/images/edits",
            post(
                move |headers: axum::http::HeaderMap, body: axum::body::Bytes| {
                    let seen = seen.clone();
                    async move {
                        seen.fetch_add(1, Ordering::Relaxed);
                        assert_eq!(headers["authorization"], "Bearer mock-only");
                        assert!(
                            headers["content-type"]
                                .to_str()
                                .unwrap()
                                .starts_with("multipart/form-data")
                        );
                        let text = String::from_utf8_lossy(&body);
                        assert!(text.contains("name=\"image\""));
                        assert!(text.contains("Translate every readable"));
                        assert!(text.contains("English"));
                        assert!(!text.contains("reasoning_effort"));
                        Json(json!({"data":[{"b64_json":STANDARD.encode(png(96,160))}]}))
                    }
                },
            ),
        );
        let (base, task) = serve(app).await;
        let settings = Settings {
            image_base_url: format!("{base}/v1"),
            image_model: "image-editor".into(),
            image_key: "mock-only".into(),
            ..Default::default()
        };
        let config =
            || translation::Config::load(&settings, crate::settings::Language::English).unwrap();
        let directory = std::env::temp_dir().join(format!(
            "replayer-comic-translation-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let original = directory.join("original.png");
        let original_bytes = png(96, 160);
        std::fs::write(&original, &original_bytes).unwrap();
        let hash = digest(&original_bytes);
        let output = translation::translate(
            config(),
            &original,
            &hash,
            "A comic",
            &directory.join("translated"),
            Arc::new(AtomicU64::new(0)),
        )
        .await
        .unwrap();
        assert!(output.exists());
        assert_ne!(output, original);
        assert_eq!(std::fs::read(&original).unwrap(), original_bytes);
        assert_eq!(
            translation::translate(
                config(),
                &original,
                &hash,
                "A comic",
                &directory.join("translated"),
                Arc::new(AtomicU64::new(0))
            )
            .await
            .unwrap(),
            output
        );
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        let changed = Settings {
            target: "zh-CN".into(),
            ..settings.clone()
        };
        assert_ne!(
            config().cache_key(&hash, "A comic"),
            translation::Config::load(&changed, crate::settings::Language::English)
                .unwrap()
                .cache_key(&hash, "A comic")
        );
        task.abort();
        let failed = Arc::new(AtomicUsize::new(0));
        let observed = failed.clone();
        let app = axum::Router::new().route(
            "/v1/images/edits",
            post(move || {
                let observed = observed.clone();
                async move {
                    observed.fetch_add(1, Ordering::Relaxed);
                    (
                        axum::http::StatusCode::BAD_REQUEST,
                        "mock-only secret must not be echoed",
                    )
                }
            }),
        );
        let (base, task) = serve(app).await;
        let settings = Settings {
            image_base_url: format!("{base}/v1"),
            ..settings
        };
        let error = translation::translate(
            translation::Config::load(&settings, crate::settings::Language::English).unwrap(),
            &original,
            &hash,
            "Other comic",
            &directory,
            Arc::new(AtomicU64::new(0)),
        )
        .await
        .err()
        .unwrap()
        .to_string();
        assert!(error.contains("400"));
        assert!(!error.contains("mock-only"));
        assert_eq!(failed.load(Ordering::Relaxed), 1);
        assert_eq!(std::fs::read(&original).unwrap(), original_bytes);
        std::fs::remove_dir_all(directory).unwrap();
        task.abort();
    });
}

#[test]
fn tall_images_are_tiled_and_translation_preserves_canvas_dimensions() {
    use futures_util::future::BoxFuture;
    struct Echo(Arc<AtomicUsize>);
    impl translation::ImageTranslationProvider for Echo {
        fn cache_key(&self, _: &str, _: &str) -> String {
            "echo".into()
        }
        fn edit<'a>(
            &'a self,
            image: image::DynamicImage,
            _: &'a str,
        ) -> BoxFuture<'a, anyhow::Result<image::DynamicImage>> {
            self.0.fetch_add(1, Ordering::Relaxed);
            Box::pin(async { Ok(image) })
        }
    }
    let image = cache::read_image(&png(500, 5200)).unwrap();
    let bands = translation::bands(&image);
    assert_eq!(bands.first().unwrap().0, 0);
    assert_eq!(bands.last().unwrap().1, 5200);
    assert!(bands.windows(2).all(|b| b[0].1 == b[1].0));
    let bitmap = cache::decode(&png(500, 5200), 1800).unwrap();
    assert_eq!(bitmap.tiles.len(), 3);
    assert!(bitmap.tiles.iter().all(|t| t.height() <= 2048));
    let directory =
        std::env::temp_dir().join(format!("replayer-comic-bands-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("original.png");
    let bytes = png(500, 5200);
    std::fs::write(&path, &bytes).unwrap();
    let count = Arc::new(AtomicUsize::new(0));
    let output = runtime()
        .block_on(translation::translate(
            Echo(count.clone()),
            &path,
            &digest(&bytes),
            "test",
            &directory.join("translated"),
            Arc::new(AtomicU64::new(0)),
        ))
        .unwrap();
    let image = cache::read_image(&std::fs::read(output).unwrap()).unwrap();
    assert_eq!((image.width(), image.height()), (500, 5200));
    assert_eq!(count.load(Ordering::Relaxed), bands.len());
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
#[ignore = "Contacts MangaYun/DM5 and downloads three public pages from the regression chapter"]
fn live_dm5_empty_aggregator_falls_back_and_loads_first_middle_last_pages() {
    runtime().block_on(async {
        let source = source::MangaYun::connect().unwrap();
        let book = Book {
            provider: "mangayun".into(),
            site: "dm5".into(),
            ..Default::default()
        };
        let chapter = Chapter {
            url: "https://www.dm5.com/m1845402/".into(),
            ..Default::default()
        };
        let started = std::time::Instant::now();
        let pages = source.pages(&book, &chapter, false).await.unwrap();
        assert_eq!(pages.len(), 56);
        println!(
            "DM5 chapter: {} pages, list loaded in {:?}",
            pages.len(),
            started.elapsed()
        );
        let directory = std::path::Path::new("target/comic-review/live-cache");
        for index in [0, 27, 55] {
            let started = std::time::Instant::now();
            let image = cache::fetch(
                directory,
                &format!("dm5-1845402:{index}"),
                source.image_request("dm5", &pages[index]).unwrap(),
                1800,
            )
            .await
            .unwrap();
            assert!(image.bitmap.width > 100 && image.bitmap.height > 100);
            println!(
                "Page {}: {}x{} in {:?}",
                index + 1,
                image.bitmap.width,
                image.bitmap.height,
                started.elapsed()
            );
            std::fs::copy(
                image.path,
                format!("target/comic-review/page-{:02}.jpg", index + 1),
            )
            .unwrap();
        }
    });
}

#[test]
#[ignore = "Contacts MangaYun and downloads one public page"]
fn live_mangayun_search_details_and_page() {
    runtime().block_on(async {
        let source = source::MangaYun::connect().unwrap();
        let groups = source.search("鬼灭之刃").await.unwrap();
        println!(
            "{} sources, {} results",
            groups.len(),
            groups.iter().map(|g| g.books.len()).sum::<usize>()
        );
        let book = groups
            .iter()
            .find(|g| g.site.id == "manhuazhijia")
            .and_then(|g| g.books.iter().find(|b| b.title == "鬼灭之刃"))
            .expect("No test comic");
        let detail = source.detail(book).await.unwrap();
        let list_started = std::time::Instant::now();
        let pages = source
            .pages(&detail.book, &detail.chapters[0], false)
            .await
            .unwrap();
        println!("page list: {:?}", list_started.elapsed());
        let load_started = std::time::Instant::now();
        let image = cache::fetch(
            &root().join("cache/pages"),
            &format!("{}:{}:0", book.key(), detail.chapters[0].key()),
            source.image_request(&book.site, &pages[0]).unwrap(),
            1800,
        )
        .await
        .unwrap();
        println!("first load (may be cached): {:?}", load_started.elapsed());
        let decode_started = std::time::Instant::now();
        let cached = cache::load(image.path.clone(), 1800).unwrap();
        println!("cached image read/decode: {:?}", decode_started.elapsed());
        assert_eq!(cached.hash, image.hash);
        println!(
            "{}: {} chapters, {} pages; image {}x{}",
            detail.book.title,
            detail.chapters.len(),
            pages.len(),
            image.bitmap.width,
            image.bitmap.height
        );
        assert!(detail.chapters.len() > 1 && pages.len() > 1);
    });
}

#[test]
#[ignore = "Uses IMAGE_MODEL and uploads one public manga page to the configured image editing service"]
fn live_comic_image_translation() {
    runtime().block_on(async {
        let source = source::MangaYun::connect().unwrap();
        let book = Book {
            provider: "mangayun".into(),
            site: "manhuazhijia".into(),
            title: "鬼灭之刃".into(),
            url: "https://www.manhuazhijia.cc/comic/guimiezhiren".into(),
            ..Default::default()
        };
        let detail = source.detail(&book).await.unwrap();
        let pages = source
            .pages(&book, &detail.chapters[0], false)
            .await
            .unwrap();
        let image = cache::fetch(
            &root().join("cache/pages"),
            &format!("{}:{}:2", book.key(), detail.chapters[0].key()),
            source.image_url(&book.site, &pages[2]).unwrap(),
            1800,
        )
        .await
        .unwrap();
        let configured = crate::test_config::settings().unwrap();
        let config = translation::Config::load_with(
            &configured.comics,
            &configured.ai,
            crate::settings::Language::English,
        )
        .unwrap();
        println!("Configured image model: {}", config.model_name());
        let start = std::time::Instant::now();
        let path = translation::translate(
            config,
            &image.path,
            &image.hash,
            &book.title,
            &root().join("translations"),
            Arc::new(AtomicU64::new(0)),
        )
        .await
        .unwrap();
        println!(
            "Translated public page into English in {:.1}s\nOriginal: {}\nTranslated: {}",
            start.elapsed().as_secs_f64(),
            image.path.display(),
            path.display()
        );
        assert!(cache::load(path, 1800).is_ok());
    });
}
#[test]
fn library_updates_history_and_url_migration_survive_restart() {
    let path = std::env::temp_dir().join(format!(
        "replayer-comics-test-{}.sqlite",
        uuid::Uuid::new_v4()
    ));
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
