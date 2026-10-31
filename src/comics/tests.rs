use super::*;
#[cfg(test)]
use crate::pixels::test_support::TestImageOutput;
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
                        assert!(text.contains("name=\"output_format\"\r\n\r\njpeg\r\n"));
                        assert!(text.contains("filename=\"page.jpg\""));
                        assert!(
                            text.to_ascii_lowercase()
                                .contains("content-type: image/jpeg")
                        );
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
        let directory =
            std::env::temp_dir().join(format!("pordo-comic-translation-{}", uuid::Uuid::new_v4()));
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
        assert_eq!(output.extension().unwrap(), "jpg");
        assert_eq!(
            crate::pixels::format(&std::fs::read(&output).unwrap()).unwrap(),
            "jpeg"
        );
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
fn image_edit_uploads_jpeg_and_normalizes_provider_results_to_jpeg() {
    use axum::{Json, routing::post};
    use base64::{Engine, engine::general_purpose::STANDARD};
    runtime().block_on(async {
        let directory =
            std::env::temp_dir().join(format!("pordo-edit-formats-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let original = directory.join("original.png");
        let original_bytes = png(32, 64);
        std::fs::write(&original, &original_bytes).unwrap();
        let hash = digest(&original_bytes);
        // Some compatible gateways ignore output_format. They must still yield
        // a JPEG cache after canvas reconstruction, without a second API call.
        for (name, format) in [("webp", "webp"), ("jpeg", "jpeg"), ("png", "png")] {
            if format == "webp" && !crate::pixels::test_support::webp_supported() {
                assert!(
                    crate::pixels::read_image(include_bytes!(
                        "../../tests/fixtures/images/legacy-white.webp"
                    ))
                    .is_err()
                );
                continue;
            }
            let mut response = std::io::Cursor::new(Vec::new());
            if format == "webp" {
                response.get_mut().extend_from_slice(include_bytes!(
                    "../../tests/fixtures/images/legacy-white.webp"
                ));
            } else {
                crate::pixels::black(32, 64)
                    .write_to(&mut response, format)
                    .unwrap();
            }
            let calls = Arc::new(AtomicUsize::new(0));
            let observed = calls.clone();
            let app = axum::Router::new().route(
                "/v1/images/edits",
                post(move |body: axum::body::Bytes| {
                    let bytes = response.get_ref().clone();
                    let calls = observed.clone();
                    async move {
                        calls.fetch_add(1, Ordering::Relaxed);
                        let text = String::from_utf8_lossy(&body);
                        assert!(text.contains("name=\"output_format\"\r\n\r\njpeg\r\n"));
                        assert!(
                            text.to_ascii_lowercase()
                                .contains("content-type: image/jpeg")
                        );
                        let marker = b"filename=\"page.jpg\"";
                        let header = body
                            .windows(marker.len())
                            .position(|p| p == marker)
                            .unwrap();
                        let start = header
                            + body[header..]
                                .windows(4)
                                .position(|p| p == b"\r\n\r\n")
                                .unwrap()
                            + 4;
                        assert_eq!(
                            &body[start..start + 3],
                            &[0xff, 0xd8, 0xff],
                            "Upload bytes must be real JPEG"
                        );
                        assert!(!text.contains("output_compression"));
                        Json(serde_json::json!({"data":[{"b64_json":STANDARD.encode(bytes)}]}))
                    }
                }),
            );
            let (base, server) = serve(app).await;
            let settings = Settings {
                image_base_url: format!("{base}/v1"),
                image_model: "image-editor".into(),
                image_key: "mock-only".into(),
                ..Default::default()
            };
            let config = || {
                translation::Config::load(&settings, crate::settings::Language::English).unwrap()
            };
            let translated = translation::translate(
                config(),
                &original,
                &hash,
                "comic",
                &directory.join(name),
                Arc::new(AtomicU64::new(0)),
            )
            .await
            .unwrap();
            assert_eq!(translated.extension().unwrap(), "jpg");
            let bytes = std::fs::read(&translated).unwrap();
            assert_eq!(crate::pixels::format(&bytes).unwrap(), "jpeg");
            let image = cache::read_image(&bytes).unwrap();
            assert_eq!((image.width(), image.height()), (32, 64));
            assert_eq!(
                translation::translate(
                    config(),
                    &original,
                    &hash,
                    "comic",
                    &directory.join(name),
                    Arc::new(AtomicU64::new(0))
                )
                .await
                .unwrap(),
                translated
            );
            assert_eq!(calls.load(Ordering::Relaxed), 1);
            server.abort();
        }
        assert_eq!(std::fs::read(&original).unwrap(), original_bytes);
        std::fs::remove_dir_all(directory).unwrap();
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
            image: crate::pixels::Image,
            _: &'a str,
        ) -> BoxFuture<'a, anyhow::Result<crate::pixels::Image>> {
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
        std::env::temp_dir().join(format!("pordo-comic-bands-{}", uuid::Uuid::new_v4()));
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
#[ignore = "Uses IMAGE_MODEL and uploads one public manga page to the configured image editing service"]
fn live_comic_image_translation() {
    runtime().block_on(async {
        let configured = crate::test_config::settings().unwrap();
        let source = source::Sources::connect(&configured.comics.service).unwrap();
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
            source.book_image_request(&book, &pages[2]).unwrap(),
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
