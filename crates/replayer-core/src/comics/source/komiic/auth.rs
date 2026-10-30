//! Memory-only, account-scoped cookies. Hosts own OS credential persistence.
use super::*;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use reqwest::{
    Client, StatusCode, Url,
    cookie::{CookieStore, Jar},
    header::HeaderValue,
};
use serde::{Deserialize, Serialize};
use std::sync::{
    RwLock,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use tokio::sync::Mutex;

#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AccountSettings {
    pub email: String,
    pub auto_login: bool,
    #[serde(skip)]
    pub password: String,
}
impl AccountSettings {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.auto_login || (self.email.trim().contains('@') && !self.password.is_empty()),
            "请填写 Komiic 邮箱和密码 / Enter Komiic email and password"
        );
        ensure!(
            self.email.len() <= 320 && self.password.len() <= 8192,
            "Komiic credentials exceed size limit"
        );
        Ok(())
    }
    pub fn credential_id(&self) -> String {
        crate::comics::digest(self.email.trim().as_bytes())
    }
}

#[derive(Clone, Serialize)]
pub struct AuthStatus {
    pub phase: &'static str,
    pub nickname: String,
    pub message: String,
    pub limit: Option<u64>,
    pub usage: Option<u64>,
}
impl AuthStatus {
    pub fn label(&self) -> &str {
        &self.message
    }
}

// Delegate parsing/scoping to reqwest, but redact Cookie headers in diagnostics.
#[derive(Default)]
struct PrivateJar(Jar);
impl CookieStore for PrivateJar {
    fn set_cookies(&self, headers: &mut dyn Iterator<Item = &HeaderValue>, url: &Url) {
        self.0.set_cookies(headers, url);
    }
    fn cookies(&self, url: &Url) -> Option<HeaderValue> {
        self.0.cookies(url).map(|mut header| {
            header.set_sensitive(true);
            header
        })
    }
}

pub(super) struct Sessions(RwLock<Arc<Session>>);
impl Sessions {
    pub fn new(base: &str) -> Result<Self> {
        Ok(Self(RwLock::new(Arc::new(Session::new(
            base,
            AccountSettings::default(),
        )?))))
    }
    pub fn current(&self) -> Arc<Session> {
        self.0.read().unwrap_or_else(|e| e.into_inner()).clone()
    }
    pub fn configure(&self, base: &str, mut settings: AccountSettings) -> Result<()> {
        settings.email = settings.email.trim().into();
        settings.validate()?;
        if self.current().settings == settings {
            return Ok(());
        }
        let next = Arc::new(Session::new(base, settings)?);
        *self.0.write().unwrap_or_else(|e| e.into_inner()) = next;
        Ok(())
    }
    pub fn check(&self, session: &Arc<Session>) -> Result<()> {
        ensure!(
            Arc::ptr_eq(session, &self.current()),
            "Komiic 账号已切换 / Account changed"
        );
        Ok(())
    }
}

pub(super) struct Session {
    pub client: Client,
    settings: AccountSettings,
    jar: Arc<PrivateJar>,
    status: RwLock<AuthStatus>,
    gate: Mutex<()>,
    epoch: AtomicU64,
    refresh_at: AtomicU64,
    retry_after: AtomicU64,
    terminal: AtomicBool,
    manual_login: AtomicBool,
    pub(super) blocked_until: AtomicU64,
    quota_reset_at: AtomicU64,
}
impl Session {
    fn new(base: &str, settings: AccountSettings) -> Result<Self> {
        let url = Url::parse(base)?;
        ensure!(
            url.scheme() == "https" || cfg!(test) && url.scheme() == "http",
            "Komiic requires HTTPS"
        );
        let jar = Arc::new(PrivateJar::default());
        let client = Client::builder()
            .cookie_provider(jar.clone())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(25))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(concat!("replayer/", env!("CARGO_PKG_VERSION")))
            .build()?;
        let enabled = settings.auto_login;
        Ok(Self {
            client,
            settings,
            jar,
            status: RwLock::new(AuthStatus {
                phase: if enabled { "pending" } else { "guest" },
                nickname: String::new(),
                message: if enabled {
                    "等待自动登录 / Waiting to sign in"
                } else {
                    "游客 / Guest"
                }
                .into(),
                limit: None,
                usage: None,
            }),
            gate: Mutex::new(()),
            epoch: AtomicU64::new(0),
            refresh_at: AtomicU64::new(0),
            retry_after: AtomicU64::new(0),
            terminal: AtomicBool::new(false),
            manual_login: AtomicBool::new(false),
            blocked_until: AtomicU64::new(0),
            quota_reset_at: AtomicU64::new(0),
        })
    }
    pub fn configured(&self) -> bool {
        self.settings.auto_login || self.manual_login.load(Ordering::Acquire)
    }
    pub fn status(&self) -> AuthStatus {
        self.status
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
    fn phase(&self, phase: &'static str, message: &str) {
        let mut status = self.status.write().unwrap_or_else(|e| e.into_inner());
        status.phase = phase;
        status.message = message.into();
    }
    pub fn epoch(&self) -> u64 {
        self.epoch.load(Ordering::Acquire)
    }
    fn ready(&self) -> bool {
        self.status().phase == "signed_in" && now() < self.refresh_at.load(Ordering::Acquire)
    }
    pub async fn ensure_ready(&self, base: &str, manual: bool) -> Result<()> {
        if manual {
            ensure!(
                self.settings.email.contains('@') && !self.settings.password.is_empty(),
                "请填写 Komiic 邮箱和密码 / Enter Komiic credentials"
            );
            self.manual_login.store(true, Ordering::Release);
        }
        if !self.configured() {
            return Ok(());
        }
        if self.ready() {
            return Ok(());
        }
        let _guard = self.gate.lock().await;
        if self.ready() {
            return Ok(());
        }
        self.check_retry(manual)?;
        self.authenticate(base, self.epoch() != 0).await
    }
    fn check_retry(&self, manual: bool) -> Result<()> {
        ensure!(
            manual || !self.terminal.load(Ordering::Acquire),
            "{}",
            self.status().message
        );
        ensure!(
            manual || now() >= self.retry_after.load(Ordering::Acquire),
            "{}",
            self.status().message
        );
        Ok(())
    }
    pub async fn recover(&self, base: &str, epoch: u64) -> Result<()> {
        let _guard = self.gate.lock().await;
        if self.epoch() != epoch && self.status().phase == "signed_in" {
            return Ok(());
        }
        self.check_retry(false)?;
        self.authenticate(base, true).await
    }
    pub fn reject(&self, epoch: u64) {
        if self.epoch() == epoch {
            self.terminal.store(true, Ordering::Release);
            self.phase("error", "Komiic 会话无效，请重新登录 / Sign in again");
        }
    }
    async fn authenticate(&self, base: &str, refresh: bool) -> Result<()> {
        self.phase("signing_in", "正在登录 Komiic… / Signing in…");
        let result = self.authenticate_inner(base, refresh).await;
        if let Err(error) = &result {
            let failure = error.downcast_ref::<AuthFailure>();
            self.terminal
                .store(failure.is_some_and(|e| e.terminal), Ordering::Release);
            self.retry_after.store(
                now().saturating_add(failure.map_or(60, |e| e.delay)),
                Ordering::Release,
            );
            self.phase("error", &format!("{error}"));
        }
        result
    }
    async fn authenticate_inner(&self, base: &str, refresh: bool) -> Result<()> {
        let mut refreshed = false;
        if refresh {
            let response = self
                .post(base, "/auth/refresh")
                .send()
                .await
                .context("Komiic 会话刷新连接失败 / Session refresh connection failed")?;
            if response.status().is_success() {
                refreshed = true;
            } else if !matches!(response.status().as_u16(), 400 | 401 | 403) {
                return Err(http_failure(response).await);
            }
        }
        if !refreshed {
            let response = self
                .post(base, "/api/login")
                .json(&json!({"email":self.settings.email,"password":self.settings.password}))
                .send()
                .await
                .context("Komiic 登录连接失败 / Sign-in connection failed")?;
            if !response.status().is_success() {
                return Err(http_failure(response).await);
            }
        }
        let account = self.account(base).await;
        // Some servers acknowledge refresh but report a guest account afterwards.
        // Only an explicit authentication failure permits another password login;
        // network failures must respect backoff instead.
        let fallback = refreshed
            && match &account {
                Ok(value) => !has_account(value),
                Err(error) => error
                    .downcast_ref::<AuthFailure>()
                    .is_some_and(|e| e.unauthorized),
            };
        let value = if fallback {
            let response = self
                .post(base, "/api/login")
                .json(&json!({"email":self.settings.email,"password":self.settings.password}))
                .send()
                .await
                .context("Komiic 登录连接失败 / Sign-in connection failed")?;
            if !response.status().is_success() {
                return Err(http_failure(response).await);
            }
            self.account(base).await?
        } else {
            account?
        };
        ensure!(
            has_account(&value),
            AuthFailure::new(
                "登录未成功，请检查账号 / Account verification failed",
                true,
                0
            )
        );
        {
            let mut status = self.status.write().unwrap_or_else(|e| e.into_inner());
            status.phase = "signed_in";
            status.nickname = value["data"]["account"]["nickname"]
                .as_str()
                .unwrap_or_default()
                .into();
            status.message = "已登录 / Signed in".into();
            status.limit = None;
            status.usage = None;
        }
        self.blocked_until.store(0, Ordering::Release);
        self.quota_reset_at.store(0, Ordering::Release);
        self.quota_update(&value["data"]["getImageLimit"]);
        self.terminal.store(false, Ordering::Release);
        self.retry_after.store(0, Ordering::Release);
        self.refresh_at.store(
            self.expiry(base)
                .unwrap_or_else(|| now() + 900)
                .saturating_sub(120)
                .max(now() + 30),
            Ordering::Release,
        );
        self.epoch.fetch_add(1, Ordering::AcqRel);
        Ok(())
    }
    fn post(&self, base: &str, path: &str) -> reqwest::RequestBuilder {
        self.client
            .post(format!("{base}{path}"))
            .header("Origin", base)
            .header("Referer", format!("{base}/"))
            .header("Content-Type", "application/json")
    }
    async fn account(&self, base: &str) -> Result<Value> {
        let response = self.post(base, "/api/query").json(&json!({"operationName":"ReplayerAccount",
            "query":"query ReplayerAccount { account { id nickname } getImageLimit { limit usage resetInSeconds } }","variables":{}}))
            .send().await.context("Komiic 账号查询失败 / Account lookup failed")?;
        if !response.status().is_success() {
            return Err(http_failure(response).await);
        }
        let value = serde_json::from_slice(&limited_body(response, 512 * 1024).await?)
            .map_err(|_| anyhow!("Komiic 账号响应无效 / Invalid account response"))?;
        if unauthorized(&value) {
            let mut failure = AuthFailure::new("Komiic 会话已过期 / Session expired", true, 0);
            failure.unauthorized = true;
            return Err(failure.into());
        }
        Ok(value)
    }
    fn expiry(&self, base: &str) -> Option<u64> {
        // Unverified JWT expiry is only a refresh hint; account() authenticates the session.
        let url = Url::parse(base).ok()?;
        let header = self.jar.cookies(&url)?;
        let token = header
            .to_str()
            .ok()?
            .split(';')
            .find_map(|c| c.trim().strip_prefix("komiic-access-token="))?;
        let payload = URL_SAFE_NO_PAD.decode(token.split('.').nth(1)?).ok()?;
        serde_json::from_slice::<Value>(&payload).ok()?["exp"].as_u64()
    }
    pub async fn logout(&self, base: &str) -> Result<()> {
        if self.status().phase != "signed_in" {
            return Ok(());
        }
        let response = self
            .post(base, "/auth/logout")
            .send()
            .await
            .context("本机已退出，服务器退出失败 / Signed out locally; server logout failed")?;
        ensure!(
            response.status().is_success(),
            "本机已退出，服务器退出失败 / Server logout failed"
        );
        Ok(())
    }
    pub fn quota_check(&self) -> Result<()> {
        ensure!(
            now() >= self.blocked_until.load(Ordering::Acquire),
            "Komiic 图片额度已用尽，请稍后重试 / Image quota exhausted; retry later"
        );
        Ok(())
    }
    pub fn quota_block(&self, seconds: u64) {
        self.blocked_until
            .fetch_max(now() + seconds.clamp(60, 86400), Ordering::AcqRel);
    }
    pub fn reset_seconds(&self) -> u64 {
        self.quota_reset_at
            .load(Ordering::Acquire)
            .checked_sub(now())
            .filter(|s| *s > 0)
            .unwrap_or(3600)
    }
    pub fn quota_update(&self, limit: &Value) {
        if let Some(seconds) = number(&limit["resetInSeconds"]) {
            self.quota_reset_at
                .store(now() + seconds, Ordering::Release);
        }
        if let (Some(max), Some(usage)) = (number(&limit["limit"]), number(&limit["usage"])) {
            let mut status = self.status.write().unwrap_or_else(|e| e.into_inner());
            status.limit = Some(max);
            status.usage = Some(usage);
            if max > 0 && usage >= max {
                self.quota_block(self.reset_seconds());
            }
        }
    }
}

fn has_account(value: &Value) -> bool {
    value["data"]["account"]["id"]
        .as_str()
        .is_some_and(|id| !id.is_empty() && id != "0")
}
pub(super) fn unauthorized(value: &Value) -> bool {
    value["errors"].as_array().is_some_and(|errors| {
        errors.iter().any(|error| {
            matches!(
                error["extensions"]["code"].as_str(),
                Some("UNAUTHENTICATED" | "UNAUTHORIZED" | "AUTHENTICATION_REQUIRED")
            )
        })
    })
}

#[derive(Debug)]
struct AuthFailure {
    message: &'static str,
    terminal: bool,
    delay: u64,
    unauthorized: bool,
}
impl AuthFailure {
    fn new(message: &'static str, terminal: bool, delay: u64) -> Self {
        Self {
            message,
            terminal,
            delay,
            unauthorized: false,
        }
    }
}
impl std::fmt::Display for AuthFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message)
    }
}
impl std::error::Error for AuthFailure {}
async fn http_failure(response: reqwest::Response) -> anyhow::Error {
    let status = response.status();
    let delay = response
        .headers()
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok())
        .unwrap_or(60u64)
        .clamp(1, 3600);
    let bytes = limited_body(response, 64 * 1024).await.unwrap_or_default();
    let code = serde_json::from_slice::<Value>(&bytes)
        .ok()
        .and_then(|v| v["error"].as_str().map(str::to_owned));
    let (message, terminal) = match (status, code.as_deref()) {
        (_, Some("account_not_activated")) => (
            "Komiic 账号未激活，请先验证邮箱 / Verify your email first",
            true,
        ),
        (StatusCode::TOO_MANY_REQUESTS, _) => (
            "Komiic 登录过于频繁，请稍后重试 / Sign-in rate limited",
            false,
        ),
        (StatusCode::FORBIDDEN, _) => (
            "Komiic 登录被拒绝或需要额外验证 / Sign-in blocked; verification may be required",
            true,
        ),
        (StatusCode::BAD_REQUEST | StatusCode::UNAUTHORIZED, _) => {
            ("Komiic 邮箱或密码错误 / Incorrect email or password", true)
        }
        _ => (
            "Komiic 登录服务暂不可用 / Sign-in service unavailable",
            false,
        ),
    };
    let mut failure = AuthFailure::new(message, terminal, delay);
    failure.unauthorized = status == StatusCode::UNAUTHORIZED;
    failure.into()
}
