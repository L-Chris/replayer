use super::*;
use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
};
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicUsize, Ordering};
use tokio::sync::Notify;

#[derive(Default)]
struct Mock {
    logins: AtomicUsize,
    refreshes: AtomicUsize,
    queries: AtomicUsize,
    login_status: AtomicU16,
    refresh_mode: AtomicU16,
    query_status: AtomicU16,
    delay_login: AtomicBool,
    entered: Notify,
    resume: Notify,
}
fn settings(automatic: bool) -> AccountSettings {
    AccountSettings {
        email: "reader@example.test".into(),
        password: " password with spaces ".into(),
        auto_login: automatic,
    }
}
async fn login(State(state): State<Arc<Mock>>, Json(body): Json<Value>) -> Response {
    state.logins.fetch_add(1, Ordering::Relaxed);
    assert_eq!(body["password"].as_str(), Some(" password with spaces "));
    state.entered.notify_one();
    if state.delay_login.load(Ordering::Relaxed) {
        state.resume.notified().await;
    }
    match state.login_status.load(Ordering::Relaxed) {
        400 => (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"account_not_activated"})),
        )
            .into_response(),
        401 => (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error":"never echo private password or email"})),
        )
            .into_response(),
        429 => (
            StatusCode::TOO_MANY_REQUESTS,
            [("retry-after", "30")],
            "private body",
        )
            .into_response(),
        _ => (
            [("set-cookie", "session=reader; HttpOnly; Path=/")],
            Json(json!({})),
        )
            .into_response(),
    }
}
async fn refresh(State(state): State<Arc<Mock>>) -> Response {
    state.refreshes.fetch_add(1, Ordering::Relaxed);
    match state.refresh_mode.load(Ordering::Relaxed) {
        1 => StatusCode::UNAUTHORIZED.into_response(),
        2 => ([("set-cookie", "session=expired; HttpOnly; Path=/")], "").into_response(),
        3 => StatusCode::SERVICE_UNAVAILABLE.into_response(),
        4 => ([("set-cookie", "session=graphql; HttpOnly; Path=/")], "").into_response(),
        5 => ([("set-cookie", "session=blocked; HttpOnly; Path=/")], "").into_response(),
        6 => ([("set-cookie", "session=guest; HttpOnly; Path=/")], "").into_response(),
        _ => ([("set-cookie", "session=reader; HttpOnly; Path=/")], "").into_response(),
    }
}
async fn query(
    State(state): State<Arc<Mock>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let cookie = headers
        .get("cookie")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    if body["operationName"] == "PordoAccount" {
        if cookie.contains("session=expired") {
            return StatusCode::UNAUTHORIZED.into_response();
        }
        if cookie.contains("session=graphql") {
            return Json(json!({"errors":[{"extensions":{"code":"UNAUTHENTICATED"}}]}))
                .into_response();
        }
        if cookie.contains("session=blocked") {
            return StatusCode::FORBIDDEN.into_response();
        }
        return Json(json!({"data":{"account":{"id":if cookie.contains("session=reader") {"123"} else {"0"},"nickname":"Fixture"},
            "getImageLimit":{"limit":1000,"usage":2,"resetInSeconds":500}}})).into_response();
    }
    state.queries.fetch_add(1, Ordering::Relaxed);
    match state.query_status.load(Ordering::Relaxed) {
        401 => StatusCode::UNAUTHORIZED.into_response(),
        402 => StatusCode::PAYMENT_REQUIRED.into_response(),
        403 => StatusCode::FORBIDDEN.into_response(),
        2001 => Json(json!({"errors":[{"extensions":{"code":"UNAUTHENTICATED"}}]})).into_response(),
        _ => Json(json!({"data":{"ok":true}})).into_response(),
    }
}
struct Fixture {
    source: Komiic,
    state: Arc<Mock>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Fixture {
    async fn new() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let source = Komiic::at(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
        let state = Arc::new(Mock::default());
        let app = Router::new()
            .route("/api/login", post(login))
            .route("/auth/refresh", post(refresh))
            .route("/auth/logout", post(|| async { StatusCode::OK }))
            .route("/api/query", post(query))
            .with_state(state.clone());
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        Self {
            source,
            state,
            task,
        }
    }
    async fn request(&self) -> Result<Value> {
        self.source
            .query("Protected", "query Protected { ok }", json!({}))
            .await
    }
    fn counts(&self) -> (usize, usize) {
        (
            self.state.logins.load(Ordering::Relaxed),
            self.state.refreshes.load(Ordering::Relaxed),
        )
    }
}

#[tokio::test]
async fn concurrent_requests_and_recovery_share_one_authentication() {
    let fixture = Fixture::new().await;
    fixture.source.quota_block(500); // A guest's exhausted quota must not survive sign-in.
    fixture.source.configure_account(settings(true)).unwrap();
    let results = futures_util::future::join_all((0..20).map(|_| fixture.request())).await;
    assert!(results.iter().all(Result::is_ok));
    assert_eq!(fixture.counts(), (1, 0));
    let session = fixture.source.sessions.current();
    session.quota_check().unwrap();
    assert_eq!(session.status().limit, Some(1000));
    let epoch = session.epoch();
    session.quota_block(500);
    let results = futures_util::future::join_all(
        (0..20).map(|_| session.recover(&fixture.source.base, epoch)),
    )
    .await;
    assert!(results.iter().all(Result::is_ok));
    assert_eq!(fixture.counts(), (1, 1));
    session.quota_check().unwrap();
}

#[tokio::test]
async fn refresh_expiration_falls_back_once_but_network_failure_does_not() {
    for mode in [1, 2, 3, 4, 5, 6] {
        let fixture = Fixture::new().await;
        fixture.source.configure_account(settings(true)).unwrap();
        fixture.source.login().await.unwrap();
        fixture.state.refresh_mode.store(mode, Ordering::Relaxed);
        let session = fixture.source.sessions.current();
        let result = session.recover(&fixture.source.base, session.epoch()).await;
        let failed = mode == 3 || mode == 5;
        assert_eq!(result.is_err(), failed);
        assert_eq!(fixture.counts(), (if failed { 1 } else { 2 }, 1));
        if failed {
            assert!(fixture.request().await.is_err());
            assert_eq!(fixture.counts(), (1, 1));
        }
    }
}

#[tokio::test]
async fn failed_login_is_redacted_and_pauses_automatic_retries() {
    for status in [400, 401, 429] {
        let fixture = Fixture::new().await;
        fixture.state.login_status.store(status, Ordering::Relaxed);
        fixture.source.configure_account(settings(true)).unwrap();
        let error = fixture.request().await.unwrap_err().to_string();
        assert!(!error.contains("private"));
        assert!(!error.contains("reader@example"));
        if status == 400 {
            assert!(error.contains("未激活"));
        }
        assert!(fixture.request().await.is_err());
        assert_eq!(fixture.counts(), (1, 0));
        fixture.state.login_status.store(200, Ordering::Relaxed);
        fixture.source.login().await.unwrap(); // Explicit retry is allowed after editing credentials.
        assert_eq!(fixture.counts(), (2, 0));
    }
    let fixture = Fixture::new().await;
    fixture.source.configure_account(settings(false)).unwrap();
    fixture.state.login_status.store(401, Ordering::Relaxed);
    assert!(fixture.source.login().await.is_err());
    // A failed manual sign-in must not leak partially issued cookies into guest requests.
    assert!(fixture.request().await.is_err());
    assert_eq!(fixture.counts(), (1, 0));
    assert_eq!(fixture.state.queries.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn quota_and_forbidden_are_not_authentication_failures() {
    for code in [402, 403] {
        let fixture = Fixture::new().await;
        fixture.source.configure_account(settings(true)).unwrap();
        fixture.state.query_status.store(code, Ordering::Relaxed);
        assert!(fixture.request().await.is_err());
        assert_eq!(fixture.counts(), (1, 0));
        assert_eq!(fixture.source.auth_status().phase, "signed_in");
        if code == 402 {
            assert!(fixture.source.sessions.current().quota_check().is_err());
        }
    }
}

#[tokio::test]
async fn persistent_unauthorized_recovers_once_then_requires_user_action() {
    for code in [401, 2001] {
        let fixture = Fixture::new().await;
        fixture.source.configure_account(settings(true)).unwrap();
        fixture.state.query_status.store(code, Ordering::Relaxed);
        assert!(fixture.request().await.is_err());
        assert_eq!(fixture.counts(), (1, 1));
        assert!(fixture.request().await.is_err());
        assert_eq!(fixture.counts(), (1, 1));
        assert_eq!(fixture.state.queries.load(Ordering::Relaxed), 2);
    }
}

#[tokio::test]
async fn account_change_invalidates_an_inflight_login() {
    let fixture = Fixture::new().await;
    fixture.source.configure_account(settings(true)).unwrap();
    fixture.state.delay_login.store(true, Ordering::Relaxed);
    let source = fixture.source.clone();
    let login = tokio::spawn(async move { source.login().await });
    fixture.state.entered.notified().await;
    fixture
        .source
        .configure_account(AccountSettings::default())
        .unwrap();
    fixture.state.resume.notify_one();
    assert!(
        login
            .await
            .unwrap()
            .err()
            .unwrap()
            .to_string()
            .contains("Account changed")
    );
    assert_eq!(fixture.source.auth_status().phase, "guest");
    fixture.request().await.unwrap();
    assert_eq!(fixture.counts(), (1, 0));
}

#[tokio::test]
async fn manual_login_without_auto_login_lasts_only_for_this_session() {
    let fixture = Fixture::new().await;
    fixture.source.configure_account(settings(false)).unwrap();
    fixture.request().await.unwrap();
    assert_eq!(fixture.counts(), (0, 0));
    fixture.source.login().await.unwrap();
    fixture.request().await.unwrap();
    assert_eq!(fixture.counts(), (1, 0));
    fixture.source.logout().await.unwrap();
    assert_eq!(fixture.source.auth_status().phase, "guest");
    fixture.request().await.unwrap();
    assert_eq!(fixture.counts(), (1, 0));
    assert!(
        !serde_json::to_string(&settings(true))
            .unwrap()
            .contains("password")
    );
}
