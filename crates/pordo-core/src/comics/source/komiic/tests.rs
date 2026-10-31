use super::*;

#[test]
#[ignore = "Uses explicitly supplied KOMMIC_ACCOUNT/KOMMIC_PASSWORD against the real service"]
fn komiic_live_account_login_and_refresh() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let _ = dotenvy::from_path(root.join(".env"));
    let email = std::env::var("KOMMIC_ACCOUNT").expect("KOMMIC_ACCOUNT is required");
    let password = std::env::var("KOMMIC_PASSWORD").expect("KOMMIC_PASSWORD is required");
    runtime().block_on(async {
        let source = Komiic::at("https://komiic.com").unwrap();
        source
            .configure_account(AccountSettings {
                email,
                password,
                auto_login: true,
            })
            .unwrap();
        let status = source
            .login()
            .await
            .expect("Live sign-in failed (credentials are never printed)");
        assert_eq!(status.phase, "signed_in");
        let session = source.sessions.current();
        session
            .recover(&source.base, session.epoch())
            .await
            .expect("Live refresh failed");
        let value = source
            .query(
                "PordoVerifyAccount",
                "query PordoVerifyAccount { account { id } }",
                json!({}),
            )
            .await
            .unwrap();
        assert!(
            value["account"]["id"]
                .as_str()
                .is_some_and(|id| id != "0" && !id.is_empty())
        );
        // Discard only this test's local cookies. Do not sign the user out of other clients.
        source
            .configure_account(AccountSettings::default())
            .unwrap();
        assert_eq!(source.auth_status().phase, "guest");
    });
}
#[cfg(test)]
use crate::pixels::test_support::TestImageOutput;
use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::{get, post},
};
use std::{
    io::Cursor,
    sync::atomic::{AtomicUsize, Ordering},
};

const KID: &str = "6eb1e407-fad3-4891-a6fb-0489d8af89c1";
#[derive(Clone)]
struct Mock {
    base: String,
    tickets: Arc<AtomicUsize>,
    images: Arc<AtomicUsize>,
    mode: u8,
}
fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}
fn comic() -> Value {
    json!({"id":"217","title":"葬送的芙莉莲","status":"ONGOING","description":"简介","imageUrl":"https://public.komiic.com/cover.jpg","authors":[{"name":"作者"}]})
}
async fn query(State(state): State<Mock>, Json(body): Json<Value>) -> Json<Value> {
    let data = match body["operationName"].as_str().unwrap() {
        "Search" => {
            assert_eq!(body["variables"]["keyword"], "葬送");
            json!({"searchComicsAndAuthors":{"comics":[comic()]}})
        }
        "Detail" => {
            assert_eq!(body["variables"]["comicId"], "217");
            json!({"comicById":comic(),"chaptersByComicId":[
                {"id":"4","serial":"10","type":"chapter"},
                {"id":"5","serial":"01","type":"book"},
                {"id":"3","serial":"2.5","type":"chapter"},
                {"id":"2","serial":"02","type":"chapter"},
                {"id":"1","serial":"01","type":"chapter"},
                {"id":"1","serial":"01","type":"chapter"}
            ]})
        }
        "Pages" => {
            json!({"imagesByChapterId":[{"kid":KID}],"getImageLimit":{"limit":300,"usage":2,"resetInSeconds":"6200"}})
        }
        "ImageTicket" => {
            let n = state.tickets.fetch_add(1, Ordering::Relaxed);
            if state.mode == 3 {
                return Json(
                    json!({"errors":[{"message":"private info must not escape","extensions":{"code":"QUOTA_EXCEEDED"}}]}),
                );
            }
            assert_eq!(body["variables"]["kids"][0], KID);
            json!({"getImageTickets":[{"kid":KID,"url":format!("{}/image",state.base),"ticket":format!("ticket-{n}"),"expiresAt":now()+600}]})
        }
        _ => panic!("Unexpected operation"),
    };
    Json(json!({"data":data}))
}
async fn image(State(state): State<Mock>, headers: HeaderMap) -> (StatusCode, HeaderMap, Vec<u8>) {
    let n = state.images.fetch_add(1, Ordering::Relaxed);
    assert!(
        headers["x-image-ticket"]
            .to_str()
            .unwrap()
            .starts_with("ticket-")
    );
    let code = match state.mode {
        0 if n == 0 => StatusCode::FORBIDDEN,
        1 => StatusCode::FORBIDDEN,
        2 => StatusCode::PAYMENT_REQUIRED,
        _ => StatusCode::OK,
    };
    let image = crate::pixels::black(48, 64);
    let mut bytes = Cursor::new(Vec::new());
    image.write_to(&mut bytes, "jpeg").unwrap();
    let mut headers = HeaderMap::new();
    headers.insert("content-type", "application/octet-stream".parse().unwrap());
    (code, headers, bytes.into_inner())
}
async fn server(mode: u8) -> (Komiic, Mock, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let state = Mock {
        base: base.clone(),
        tickets: Arc::default(),
        images: Arc::default(),
        mode,
    };
    let app = Router::new()
        .route("/api/query", post(query))
        .route("/image", get(image))
        .with_state(state.clone());
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (Komiic::at(&base).unwrap(), state, task)
}
fn directory() -> std::path::PathBuf {
    std::path::Path::new("target/komiic-tests").join(uuid::Uuid::new_v4().to_string())
}

#[test]
fn komiic_search_directory_lazy_ticket_refresh_and_offline_cache() {
    runtime().block_on(async {
        let (source, state, task) = server(0).await;
        let groups = source.search(" 葬送 ").await.unwrap();
        let book = &groups[0].books[0];
        let detail = source.detail(book).await.unwrap();
        assert_eq!(book.key(), detail.book.key());
        assert_eq!(detail.book.author, "作者");
        assert_eq!(
            detail
                .chapters
                .iter()
                .map(|c| c.id.as_str())
                .collect::<Vec<_>>(),
            vec!["1", "2", "3", "4", "5"]
        );
        assert_eq!(detail.chapters[4].kind, ChapterKind::Volume);
        let pages = source
            .pages(book, &detail.chapters[0], false)
            .await
            .unwrap();
        let request = source.image_request(&book.site, &pages[0]).unwrap();
        assert_eq!(state.tickets.load(Ordering::Relaxed), 0);
        let directory = directory();
        let first = cache::fetch(&directory, "stable-page", request, 240)
            .await
            .unwrap();
        assert_eq!(first.bitmap.height, 64);
        assert_eq!(state.tickets.load(Ordering::Relaxed), 2);
        assert_eq!(state.images.load(Ordering::Relaxed), 2);
        source.quota_block(1000);
        let cached = cache::fetch(
            &directory,
            "stable-page",
            source.image_request(&book.site, &pages[0]).unwrap(),
            240,
        )
        .await
        .unwrap();
        assert_eq!(first.hash, cached.hash);
        assert!(
            cache::fetch(
                &directory,
                "uncached-page",
                source.image_request(&book.site, &pages[0]).unwrap(),
                240
            )
            .await
            .err()
            .unwrap()
            .to_string()
            .contains("quota")
        );
        assert_eq!(state.tickets.load(Ordering::Relaxed), 2);
        assert!(
            source
                .image_request("komiic", "https://evil.example/api/image/123")
                .is_err()
        );
        assert!(Komiic::id("https://evil.example/comic/217", "comic").is_err());
        assert!(
            !std::fs::read(&first.path)
                .unwrap()
                .windows(7)
                .any(|w| w == b"ticket-")
        );
        std::fs::remove_dir_all(directory).unwrap();
        task.abort();
    });
}

#[test]
fn komiic_expired_tickets_retry_once_and_402_stops_following_requests() {
    runtime().block_on(async {
        for mode in [1, 2, 3] {
            let (source, state, task) = server(mode).await;
            let book = Komiic::book(&comic()).unwrap();
            let detail = source.detail(&book).await.unwrap();
            let pages = source
                .pages(&book, &detail.chapters[0], false)
                .await
                .unwrap();
            let directory = directory();
            let result = cache::fetch(
                &directory,
                "page",
                source.image_request("komiic", &pages[0]).unwrap(),
                240,
            )
            .await;
            let error = match result {
                Err(error) => error,
                Ok(_) => panic!("Expected an image error"),
            };
            if mode == 1 {
                assert!(error.to_string().contains("403"));
                assert_eq!(state.tickets.load(Ordering::Relaxed), 2);
                assert_eq!(state.images.load(Ordering::Relaxed), 2);
            } else {
                assert!(error.to_string().contains("quota"));
                assert!(!error.to_string().contains("private info"));
                assert!(
                    source
                        .sessions
                        .current()
                        .blocked_until
                        .load(Ordering::Relaxed)
                        >= now() + 6000
                );
                assert!(
                    cache::fetch(
                        &directory,
                        "next",
                        source.image_request("komiic", &pages[0]).unwrap(),
                        240
                    )
                    .await
                    .is_err()
                );
                assert_eq!(state.tickets.load(Ordering::Relaxed), 1);
                assert_eq!(state.images.load(Ordering::Relaxed), usize::from(mode == 2));
                // Quota exhaustion must not disable metadata/search.
                assert_eq!(source.search("葬送").await.unwrap()[0].books.len(), 1);
            }
            task.abort();
        }
    });
}

#[test]
#[ignore = "Uses the public Komiic guest API and downloads one page"]
fn live_komiic_search_detail_and_cached_page() {
    runtime().block_on(async {
        let source = Komiic::at("https://komiic.com").unwrap();
        let groups = source.search("葬送").await.unwrap();
        let book = groups[0]
            .books
            .iter()
            .find(|b| b.url.ends_with("/217"))
            .unwrap();
        let detail = source.detail(book).await.unwrap();
        assert!(
            detail
                .chapters
                .iter()
                .any(|c| c.kind == ChapterKind::Volume)
        );
        let pages = source
            .pages(book, &detail.chapters[0], false)
            .await
            .unwrap();
        assert_eq!(pages.len(), 39);
        let directory = std::path::Path::new("target/komiic-research/live-cache");
        let key = format!("{}:{}:0", book.key(), detail.chapters[0].key());
        let image = cache::fetch(
            directory,
            &key,
            source.image_request(&book.site, &pages[0]).unwrap(),
            1800,
        )
        .await
        .unwrap();
        assert!(image.bitmap.height > 1000);
        source.quota_block(1000);
        let cached = cache::fetch(
            directory,
            &key,
            source.image_request(&book.site, &pages[0]).unwrap(),
            1800,
        )
        .await
        .unwrap();
        assert_eq!(image.hash, cached.hash);
        println!(
            "Komiic: {} results, {} chapters, {} pages; image {}x{}, offline cache verified",
            groups[0].books.len(),
            detail.chapters.len(),
            pages.len(),
            image.bitmap.width,
            image.bitmap.height
        );
    });
}
