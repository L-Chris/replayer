use super::*;
use axum::{
    Json, Router,
    extract::{Path, State},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Clone)]
struct Mock {
    book: Book,
    chapter: Chapter,
    prepared: Arc<AtomicUsize>,
    images: Arc<AtomicUsize>,
    bytes: Vec<u8>,
}
fn view(book: &Book) -> Value {
    let mut v = serde_json::to_value(book).unwrap();
    v["comicId"] = json!(book.key());
    v["coverAssetId"] = json!("cover-id");
    v
}
fn envelope(data: Value) -> Json<Value> {
    Json(json!({"data":data,"requestId":"offline-fixture"}))
}
async fn auth(request: axum::extract::Request, next: Next) -> Response {
    if request
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        != Some("Bearer fixture-token")
    {
        return (axum::http::StatusCode::UNAUTHORIZED,Json(json!({"error":{"code":"TOKEN_INVALID","message":"invalid","retryable":false},"requestId":"fixture"}))).into_response();
    }
    next.run(request).await
}
async fn fixture() -> (Arc<Sources>, Mock, tokio::task::JoinHandle<()>) {
    let mut bytes = std::io::Cursor::new(vec![]);
    use crate::pixels::test_support::TestImageOutput;
    crate::pixels::solid(12, 24, [11, 22, 33, 255])
        .write_to(&mut bytes, "png")
        .unwrap();
    let state = Mock {
        book: Book {
            provider: "mangayun".into(),
            site: "dm5".into(),
            title: "Existing favorite".into(),
            url: "https://upstream.example/book".into(),
            cover: "https://images.example/cover".into(),
            ..Default::default()
        },
        chapter: Chapter {
            id: String::new(),
            name: "Old chapter".into(),
            url: "https://upstream.example/chapter/25%20x?q=/&".into(),
            order: 1,
            kind: ChapterKind::Volume,
        },
        prepared: Arc::new(AtomicUsize::new(0)),
        images: Arc::new(AtomicUsize::new(0)),
        bytes: bytes.into_inner(),
    };
    let app=Router::new()
        .route("/api/v1/sources",get(||async{envelope(json!({"sources":[{"id":"mangayun","displayName":"MangaYun","kind":"comic","capabilities":{"search":true},"imagePolicy":null}]}))}))
        .route("/api/v1/sources/mangayun/sites",get(||async{envelope(json!({"sites":[{"siteId":"dm5","siteName":"DM5"}]}))}))
        .route("/api/v1/comics/search",get(|State(s):State<Mock>|async move{envelope(json!({"groups":[{"site":{"siteId":"mangayun:dm5","siteName":"DM5"},"books":[view(&s.book)],"error":null},{"site":{"siteId":"mangayun:other","siteName":"Unavailable"},"books":[],"error":"SOURCE_UNAVAILABLE"}]}))}))
        .route("/api/v1/comics/resolve",post(|State(s):State<Mock>,Json(body):Json<Value>|async move{
            let book:Book=serde_json::from_value(body["comics"][0].clone()).unwrap(); assert_eq!(book.key(),s.book.key());
            envelope(json!({"comics":[view(&book)]}))
        }))
        .route("/api/v1/comics/{id}",get(|State(s):State<Mock>,Path(id):Path<String>|async move{
            assert_eq!(id,s.book.key()); envelope(json!({"comic":view(&s.book),"chapters":[{"id":s.chapter.id,"url":s.chapter.url,"key":s.chapter.key(),"name":s.chapter.name,"order":1,"kind":"volume"}]}))
        }))
        .route("/api/v1/comics/{id}/chapters/{key}/pages",get(|State(s):State<Mock>,Path((id,key)):Path<(String,String)>|async move{
            assert_eq!(id,s.book.key()); assert_eq!(key,s.chapter.key());
            envelope(json!({"pages":[{"index":0,"url":"https://images.example/page","assetId":"page-id"}],"imagePolicy":{"concurrency":2,"ahead":1,"behind":0,"retainCacheOnRefresh":true}}))
        }))
        .route("/api/v1/assets/prepare",post(|State(s):State<Mock>,Json(body):Json<Value>|async move{
            s.prepared.fetch_add(1,Ordering::Relaxed);assert_eq!(body["items"][0]["provider"],s.book.provider);
            envelope(json!({"assets":[{"assetId":"prepared-id"}]}))
        }))
        .route("/api/v1/assets/{id}",get(|State(s):State<Mock>|async move{s.images.fetch_add(1,Ordering::Relaxed);([("content-type","image/png")],s.bytes)}))
        .layer(middleware::from_fn(auth)).with_state(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/mounted", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, Router::new().nest("/mounted", app))
            .await
            .unwrap()
    });
    let source = Sources::connect(&ServiceSettings {
        url: base,
        token: "fixture-token".into(),
    })
    .unwrap();
    (source, state, task)
}
fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}
#[test]
fn empty_service_opens_old_favorites_preserves_keys_and_reuses_local_bytes() {
    runtime().block_on(async {
        let (source, state, server) = fixture().await;
        let key = state.book.key();
        // No search/discovery precedes resolving this existing favorite.
        let detail = source.detail(&state.book).await.unwrap();
        assert_eq!(detail.book.key(), key);
        assert_eq!(detail.chapters[0].key(), state.chapter.key());
        assert_eq!(detail.chapters[0].url, state.chapter.url);
        let pages = source
            .pages(&state.book, &detail.chapters[0], false)
            .await
            .unwrap();
        let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/service-integration")
            .join(uuid::Uuid::new_v4().to_string());
        let cache_key = format!("{}:{}:0", key, state.chapter.key());
        let image = cache::fetch(
            &directory,
            &cache_key,
            source.book_image_request(&state.book, &pages[0]).unwrap(),
            1800,
        )
        .await
        .unwrap();
        assert_eq!((image.bitmap.width, image.bitmap.height), (12, 24));
        assert_eq!(std::fs::read(&image.path).unwrap(), state.bytes);
        assert_eq!(state.prepared.load(Ordering::Relaxed), 0);
        let mut settings = ServiceSettings {
            url: source.client.base.as_str().into(),
            token: "fixture-token".into(),
        };
        settings.token = "revoked-token".into();
        let replacement = Sources::connect(&settings).unwrap();
        // Restored local images do not need service access or another prepare request.
        let cached = cache::fetch(
            &directory,
            &cache_key,
            replacement
                .book_image_request(&state.book, &pages[0])
                .unwrap(),
            1800,
        )
        .await
        .unwrap();
        assert_eq!(image.hash, cached.hash);
        assert_eq!(state.images.load(Ordering::Relaxed), 1);
        std::fs::remove_dir_all(directory).unwrap();
        server.abort();
    })
}
#[test]
fn discovery_partial_search_and_revoked_tokens_use_the_service_contract() {
    runtime().block_on(async {
        let (source, state, server) = fixture().await;
        let sites = source.sites().await.unwrap();
        assert_eq!(sites[0].id, "mangayun:dm5");
        let groups = source.providers()[0].1.search("book").await.unwrap();
        assert_eq!(groups[0].site.id, "mangayun:dm5");
        assert!(groups[1].error.is_some());
        assert_eq!(groups[0].books[0].key(), state.book.key());
        let bad = Sources::connect(&ServiceSettings {
            url: source.client.base.as_str().into(),
            token: "revoked-token".into(),
        })
        .unwrap();
        let error = bad.sites().await.unwrap_err().to_string();
        assert!(error.contains("TOKEN_INVALID"));
        assert!(error.contains("撤销"));
        assert!(!error.contains("revoked-token"));
        server.abort();
    })
}
#[test]
fn configuration_rejects_embedded_credentials_query_parameters_and_header_injection() {
    for url in [
        "file:///private",
        "https://user:password@example.com",
        "https://example.com?token=secret",
        "https://example.com#token",
    ] {
        assert!(
            ServiceSettings {
                url: url.into(),
                token: "fixture".into()
            }
            .validate()
            .is_err()
        );
    }
    assert!(
        ServiceSettings {
            url: "https://example.com".into(),
            token: "bad\nheader".into()
        }
        .validate()
        .is_err()
    );
    let settings = ServiceSettings {
        url: "https://example.com".into(),
        token: "private-device-token".into(),
    };
    assert!(
        !serde_json::to_string(&settings)
            .unwrap()
            .contains("private-device-token")
    );
}

#[test]
#[ignore = "Requires the opt-in pordo-service client-fixture.ts server and PORDO_SERVICE_FIXTURE_FILE"]
fn content_service_end_to_end() {
    runtime().block_on(async {
    let path=crate::environment::var("PORDO_SERVICE_FIXTURE_FILE").unwrap();
    let fixture:Value=serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let settings=ServiceSettings{url:fixture["url"].as_str().unwrap().into(),token:fixture["token"].as_str().unwrap().into()};
    let source=Sources::connect(&settings).unwrap();
    let old_book:Book=serde_json::from_value(fixture["book"].clone()).unwrap();
    let old_key=old_book.key();
    let detail=source.detail(&old_book).await.unwrap(); assert_eq!(detail.book.key(),old_key);
    let chapter_key=detail.chapters[0].key().to_owned();
    let dir=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/service-integration").join(uuid::Uuid::new_v4().to_string());
    let mut library=library::Library::open(&dir.join("library.sqlite")).unwrap();
    library.refresh(detail).unwrap();
    let mut entry=library.get(&old_key).unwrap().clone();entry.set_favorite(true);entry.mark_read(&chapter_key);
    entry.progress=Some(library::Progress{chapter:chapter_key.clone(),page:0,fraction:0.5,page_count:1});library.save(entry).unwrap();
    assert!(!source.sites().await.unwrap().is_empty());
    let groups=source.search("fixture").await.unwrap();assert!(!groups[0].books.is_empty());
    let updated=source.detail(&old_book).await.unwrap();
    let pages=source.pages(&updated.book,&updated.chapters[0],false).await.unwrap();
    let key=format!("{old_key}:{chapter_key}:0");
    let image=cache::fetch(&dir.join("pages"),&key,source.book_image_request(&old_book,&pages[0]).unwrap(),1800).await.unwrap();
    assert!(image.bitmap.width>0 && image.bitmap.height>0);
    library.refresh(updated).unwrap();drop(library);
    let library=library::Library::open(&dir.join("library.sqlite")).unwrap();let restored=library.get(&old_key).unwrap();
    assert!(restored.favorite && restored.read.contains(&chapter_key));assert_eq!(restored.progress.as_ref().unwrap().fraction,0.5);
    drop(library);std::fs::remove_dir_all(dir).unwrap();
    println!("PASS: real service discovery/search/legacy resolve/directory/pages/image and persisted favorite/read/progress");
})
}

#[test]
fn authenticated_assets_never_follow_redirects_or_forward_the_device_token() {
    runtime().block_on(async {
        let observed = Arc::new(AtomicUsize::new(0));
        let calls = observed.clone();
        let foreign = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let target = format!("http://{}/foreign", foreign.local_addr().unwrap());
        let foreign_task = tokio::spawn(async move {
            axum::serve(
                foreign,
                Router::new().route(
                    "/foreign",
                    get(move || {
                        let calls = calls.clone();
                        async move {
                            calls.fetch_add(1, Ordering::Relaxed);
                            "should not be reached"
                        }
                    }),
                ),
            )
            .await
            .unwrap()
        });
        let server = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/asset", server.local_addr().unwrap());
        let server_task = tokio::spawn(async move {
            axum::serve(
                server,
                Router::new().route(
                    "/asset",
                    get(move || {
                        let target = target.clone();
                        async move {
                            (
                                axum::http::StatusCode::TEMPORARY_REDIRECT,
                                [("location", target)],
                            )
                        }
                    }),
                ),
            )
            .await
            .unwrap()
        });
        let mut request = ImageRequest::from(url.parse::<reqwest::Url>().unwrap());
        request.headers.insert(
            reqwest::header::AUTHORIZATION,
            "Bearer fixture-token".parse().unwrap(),
        );
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/service-integration")
            .join(uuid::Uuid::new_v4().to_string());
        assert!(cache::fetch(&dir, "redirect", request, 1800).await.is_err());
        assert_eq!(observed.load(Ordering::Relaxed), 0);
        if dir.exists() {
            std::fs::remove_dir_all(dir).unwrap();
        }
        server_task.abort();
        foreign_task.abort();
    })
}
