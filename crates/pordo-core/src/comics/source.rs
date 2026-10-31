//! Versioned content-service client. Source accounts and scraping live on the server.
use super::*;
use anyhow::{Context, Result, anyhow, ensure};
use futures_util::{StreamExt, future::BoxFuture};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, RwLock},
    time::Duration,
};

#[cfg(test)]
mod tests;

/// UI and persisted records retain their original provider/site/URL identities.
pub trait ComicSource: Send + Sync {
    fn image_policy(&self, _book: &Book) -> ImagePolicy {
        ImagePolicy::default()
    }
    fn sites(&self) -> BoxFuture<'_, Result<Vec<Site>>>;
    fn search<'a>(&'a self, keyword: &'a str) -> BoxFuture<'a, Result<Vec<SearchGroup>>>;
    fn detail<'a>(&'a self, book: &'a Book) -> BoxFuture<'a, Result<Detail>>;
    fn pages<'a>(
        &'a self,
        book: &'a Book,
        chapter: &'a Chapter,
        refresh: bool,
    ) -> BoxFuture<'a, Result<Vec<String>>>;
    fn book_image_request(&self, book: &Book, url: &str) -> Result<ImageRequest>;
}

#[derive(Clone, Copy, Debug, serde::Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ImagePolicy {
    pub concurrency: usize,
    pub ahead: usize,
    pub behind: usize,
    pub retain_cache_on_refresh: bool,
}
impl Default for ImagePolicy {
    fn default() -> Self {
        Self {
            concurrency: 4,
            ahead: 3,
            behind: 1,
            retain_cache_on_refresh: false,
        }
    }
}

pub struct ImageRequest {
    pub url: reqwest::Url,
    pub headers: reqwest::header::HeaderMap,
    resolver: Option<BoxFuture<'static, Result<ImageRequest>>>,
}
impl From<reqwest::Url> for ImageRequest {
    fn from(url: reqwest::Url) -> Self {
        Self {
            url,
            headers: Default::default(),
            resolver: None,
        }
    }
}
impl ImageRequest {
    fn deferred(url: reqwest::Url, resolver: BoxFuture<'static, Result<Self>>) -> Self {
        Self {
            resolver: Some(resolver),
            ..url.into()
        }
    }
    /// Local cache hits never require a network roundtrip or an image ticket.
    pub async fn resolve(mut self) -> Result<Self> {
        if let Some(resolve) = self.resolver.take() {
            resolve.await
        } else {
            Ok(self)
        }
    }
}

/// Only the address is serialized. The per-device token belongs in the host vault.
#[derive(Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ServiceSettings {
    pub url: String,
    #[serde(skip)]
    pub token: String,
}
impl ServiceSettings {
    pub fn validate(&self) -> Result<()> {
        if self.url.trim().is_empty() {
            ensure!(
                self.token.is_empty(),
                "请填写内容服务地址 / Enter the content service URL"
            );
            return Ok(());
        }
        let url = http_url(self.url.trim())?;
        ensure!(
            url.query().is_none() && url.fragment().is_none(),
            "服务地址不能包含查询参数 / Use a service base URL"
        );
        ensure!(
            self.token.len() <= 8192,
            "服务 Token 过长 / Service token is too long"
        );
        reqwest::header::HeaderValue::from_str(&format!("Bearer {}", self.token))
            .context("无效服务 Token / Invalid service token")?;
        Ok(())
    }
    pub fn credential_id(&self) -> String {
        digest(self.url.trim().trim_end_matches('/').as_bytes())
    }
}

#[derive(Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct SourceView {
    id: String,
    display_name: String,
    kind: String,
    #[serde(default)]
    capabilities: HashMap<String, bool>,
    image_policy: Option<ImagePolicy>,
}
struct Client {
    http: reqwest::Client,
    base: reqwest::Url,
    authorization: reqwest::header::HeaderValue,
    sources: RwLock<Vec<SourceView>>,
    assets: Mutex<HashMap<String, String>>,
    policies: RwLock<HashMap<String, ImagePolicy>>,
}
#[derive(Clone)]
pub struct Sources {
    client: Arc<Client>,
    scope: Option<String>,
}
impl Sources {
    pub fn connect(settings: &ServiceSettings) -> Result<Arc<Self>> {
        settings.validate()?;
        ensure!(
            !settings.url.trim().is_empty(),
            "请在设置中配置内容服务 / Configure a content service in Settings"
        );
        ensure!(
            !settings.token.is_empty(),
            "请填写设备 Token / Enter a device token"
        );
        let base = http_url(&format!("{}/", settings.url.trim().trim_end_matches('/')))?;
        let mut authorization =
            reqwest::header::HeaderValue::from_str(&format!("Bearer {}", settings.token))?;
        authorization.set_sensitive(true);
        Ok(Arc::new(Self {
            client: Arc::new(Client {
                http: reqwest::Client::builder()
                    .connect_timeout(Duration::from_secs(12))
                    .timeout(Duration::from_secs(65))
                    .redirect(reqwest::redirect::Policy::none())
                    .user_agent(concat!("pordo/", env!("CARGO_PKG_VERSION")))
                    .build()?,
                base,
                authorization,
                sources: RwLock::new(vec![]),
                assets: Mutex::new(HashMap::new()),
                policies: RwLock::new(HashMap::new()),
            }),
            scope: None,
        }))
    }
    pub fn providers(&self) -> Vec<(Site, Arc<dyn ComicSource>)> {
        let sources = self.client.sources.read().unwrap();
        if sources.is_empty() {
            return vec![(
                Site {
                    id: "service".into(),
                    name: "Content service".into(),
                },
                Arc::new(self.clone()),
            )];
        }
        sources
            .iter()
            .filter(|s| s.kind == "comic" && s.capabilities.get("search") == Some(&true))
            .map(|s| {
                (
                    Site {
                        id: s.id.clone(),
                        name: s.display_name.clone(),
                    },
                    Arc::new(Self {
                        client: self.client.clone(),
                        scope: Some(s.id.clone()),
                    }) as Arc<dyn ComicSource>,
                )
            })
            .collect()
    }
    pub fn qualify(provider: &str, groups: &mut [SearchGroup]) {
        for group in groups {
            group.site.id = format!("{provider}:{}", group.site.id);
        }
    }
    async fn request(
        &self,
        method: reqwest::Method,
        path: &str,
        query: &[(&str, &str)],
        body: Option<Value>,
    ) -> Result<Value> {
        let url = self.client.base.join(path)?;
        ensure!(
            url.origin() == self.client.base.origin(),
            "内容服务请求越界 / Service origin mismatch"
        );
        let mut request = self.client.http.request(method, url).query(query).header(
            reqwest::header::AUTHORIZATION,
            self.client.authorization.clone(),
        );
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request
            .send()
            .await
            .context("无法连接内容服务 / Cannot connect to content service")?;
        let status = response.status();
        let bytes = limited_body(response, 8 * 1024 * 1024).await?;
        let value: Value = serde_json::from_slice(&bytes)
            .context("内容服务响应无效 / Invalid content service response")?;
        if !status.is_success() {
            return Err(service_error(status, &value));
        }
        ensure!(
            value.get("data").is_some(),
            "内容服务缺少数据 / Missing service data"
        );
        Ok(value["data"].clone())
    }
    async fn resolve(&self, book: &Book) -> Result<()> {
        // Empty legacy providers historically selected MangaYun. Keep the original
        // record untouched; the server has explicit support for its old identity.
        self.request(
            reqwest::Method::POST,
            "api/v1/comics/resolve",
            &[],
            Some(json!({"comics":[book]})),
        )
        .await?;
        Ok(())
    }
    fn remember_asset(&self, book: &Book, url: &str, asset: &str) {
        let mut assets = self.client.assets.lock().unwrap();
        if assets.len() >= 2048 {
            assets.clear();
        }
        assets.insert(
            format!("{}\0{}\0{url}", book.provider, book.site),
            asset.into(),
        );
    }
    fn decode_book(&self, value: &Value) -> Result<Book> {
        let book: Book = serde_json::from_value(value.clone())?;
        ensure!(
            value["comicId"].as_str() == Some(book.key().as_str()),
            "内容服务漫画标识不匹配 / Comic identity mismatch"
        );
        if let Some(asset) = value["coverAssetId"].as_str() {
            self.remember_asset(&book, &book.cover, asset);
        }
        Ok(book)
    }
    fn asset_request(&self, asset_id: &str) -> Result<ImageRequest> {
        let mut url = self.client.base.join("api/v1/assets/")?;
        url.path_segments_mut()
            .map_err(|()| anyhow!("Invalid service URL"))?
            .pop_if_empty()
            .push(asset_id);
        let mut request: ImageRequest = url.into();
        request.headers.insert(
            reqwest::header::AUTHORIZATION,
            self.client.authorization.clone(),
        );
        Ok(request)
    }
}
impl ComicSource for Sources {
    fn image_policy(&self, book: &Book) -> ImagePolicy {
        self.client
            .policies
            .read()
            .unwrap()
            .get(if book.provider.is_empty() {
                "mangayun"
            } else {
                &book.provider
            })
            .copied()
            .unwrap_or_default()
    }
    fn sites(&self) -> BoxFuture<'_, Result<Vec<Site>>> {
        Box::pin(async move {
            let data = self
                .request(reqwest::Method::GET, "api/v1/sources", &[], None)
                .await?;
            let sources: Vec<SourceView> = serde_json::from_value(data["sources"].clone())?;
            for source in &sources {
                if let Some(policy) = source.image_policy {
                    self.client
                        .policies
                        .write()
                        .unwrap()
                        .insert(source.id.clone(), policy);
                }
            }
            *self.client.sources.write().unwrap() = sources.clone();
            let results =
                futures_util::future::join_all(sources.iter().filter(|s| s.kind == "comic").map(
                    |source| async move {
                        let mut url = self.client.base.join("api/v1/sources/")?;
                        url.path_segments_mut()
                            .map_err(|()| anyhow!("Invalid service URL"))?
                            .pop_if_empty()
                            .push(&source.id)
                            .push("sites");
                        let data = self
                            .request(reqwest::Method::GET, url.as_str(), &[], None)
                            .await?;
                        let children: Vec<Site> = serde_json::from_value(data["sites"].clone())?;
                        Ok::<_, anyhow::Error>(
                            children
                                .into_iter()
                                .map(|s| Site {
                                    id: format!("{}:{}", source.id, s.id),
                                    name: s.name,
                                })
                                .collect::<Vec<_>>(),
                        )
                    },
                ))
                .await;
            let sites = results
                .into_iter()
                .filter_map(Result::ok)
                .flatten()
                .collect();
            Ok(sites)
        })
    }
    fn search<'a>(&'a self, keyword: &'a str) -> BoxFuture<'a, Result<Vec<SearchGroup>>> {
        Box::pin(async move {
            ensure!(
                !keyword.trim().is_empty() && keyword.len() <= 512,
                "请输入漫画名称 / Enter a title"
            );
            let mut query = vec![("q", keyword.trim())];
            if let Some(scope) = &self.scope {
                query.push(("sourceId", scope));
            }
            let data = self
                .request(reqwest::Method::GET, "api/v1/comics/search", &query, None)
                .await?;
            data["groups"]
                .as_array()
                .context("Invalid search groups")?
                .iter()
                .map(|group| {
                    Ok(SearchGroup {
                        site: serde_json::from_value(group["site"].clone())?,
                        books: group["books"]
                            .as_array()
                            .context("Invalid search books")?
                            .iter()
                            .map(|b| self.decode_book(b))
                            .collect::<Result<_>>()?,
                        error: group["error"].as_str().map(str::to_owned),
                    })
                })
                .collect()
        })
    }
    fn detail<'a>(&'a self, book: &'a Book) -> BoxFuture<'a, Result<Detail>> {
        Box::pin(async move {
            self.resolve(book).await?;
            let data = self
                .request(
                    reqwest::Method::GET,
                    &format!("api/v1/comics/{}", book.key()),
                    &[],
                    None,
                )
                .await?;
            let detail_book = self.decode_book(&data["comic"])?;
            ensure!(
                detail_book.key() == book.key(),
                "内容服务改变了漫画标识 / Comic identity changed"
            );
            let chapters = data["chapters"]
                .as_array()
                .context("Invalid chapter list")?
                .iter()
                .map(|v| {
                    let kind = if v["kind"] == "volume" {
                        ChapterKind::Volume
                    } else {
                        ChapterKind::Chapter
                    };
                    let chapter = Chapter {
                        id: v["id"].as_str().unwrap_or_default().into(),
                        url: v["url"]
                            .as_str()
                            .or_else(|| v["key"].as_str())
                            .unwrap_or_default()
                            .into(),
                        name: v["name"].as_str().unwrap_or_default().into(),
                        order: v["order"].as_i64().unwrap_or_default(),
                        kind,
                    };
                    ensure!(
                        v["key"].as_str() == Some(chapter.key()),
                        "章节标识不匹配 / Chapter identity mismatch"
                    );
                    Ok(chapter)
                })
                .collect::<Result<Vec<_>>>()?;
            Ok(Detail {
                book: detail_book,
                chapters,
            })
        })
    }
    fn pages<'a>(
        &'a self,
        book: &'a Book,
        chapter: &'a Chapter,
        refresh: bool,
    ) -> BoxFuture<'a, Result<Vec<String>>> {
        Box::pin(async move {
            self.resolve(book).await?;
            let mut url = self
                .client
                .base
                .join(&format!("api/v1/comics/{}/chapters/", book.key()))?;
            url.path_segments_mut()
                .map_err(|()| anyhow!("Invalid service URL"))?
                .pop_if_empty()
                .push(chapter.key())
                .push("pages");
            let data = self
                .request(
                    reqwest::Method::GET,
                    url.as_str(),
                    &[("refresh", if refresh { "1" } else { "0" })],
                    None,
                )
                .await?;
            if let Some(policy) = data.get("imagePolicy") {
                let mut policy: ImagePolicy = serde_json::from_value(policy.clone())?;
                policy.concurrency = policy.concurrency.clamp(1, 16);
                policy.ahead = policy.ahead.min(10);
                policy.behind = policy.behind.min(10);
                self.client.policies.write().unwrap().insert(
                    if book.provider.is_empty() {
                        "mangayun".into()
                    } else {
                        book.provider.clone()
                    },
                    policy,
                );
            }
            let pages = data["pages"].as_array().context("Invalid page list")?;
            ensure!(
                !pages.is_empty() && pages.len() <= 2000,
                "无有效漫画页面 / No valid comic pages"
            );
            pages
                .iter()
                .enumerate()
                .map(|(index, p)| {
                    ensure!(
                        p["index"].as_u64() == Some(index as u64),
                        "Invalid page order"
                    );
                    let url = p["url"].as_str().context("Missing page URL")?;
                    let asset = p["assetId"].as_str().context("Missing asset id")?;
                    self.remember_asset(book, url, asset);
                    Ok(url.to_owned())
                })
                .collect()
        })
    }
    fn book_image_request(&self, book: &Book, url: &str) -> Result<ImageRequest> {
        let original = http_url(url)?;
        let book = book.clone();
        let url = url.to_owned();
        let source = self.clone();
        Ok(ImageRequest::deferred(
            original,
            Box::pin(async move {
                let key = format!("{}\0{}\0{url}", book.provider, book.site);
                let cached = source.client.assets.lock().unwrap().get(&key).cloned();
                let asset = if let Some(asset) = cached {
                    asset
                } else {
                    let data = source.request(reqwest::Method::POST,"api/v1/assets/prepare",&[],Some(json!({"items":[{"provider":book.provider,"siteId":book.site,"url":url}]}))).await?;
                    let asset = data["assets"][0]["assetId"]
                        .as_str()
                        .context("Missing asset id")?
                        .to_owned();
                    source.remember_asset(&book, &url, &asset);
                    asset
                };
                source.asset_request(&asset)
            }),
        ))
    }
}

pub fn service_error(status: reqwest::StatusCode, value: &Value) -> anyhow::Error {
    let code = value["error"]["code"].as_str().unwrap_or("SERVICE_ERROR");
    let message = match status.as_u16() {
        401 => "设备 Token 无效或已撤销，请检查内容服务设置 / Device token is invalid or revoked",
        403 => "设备 Token 无权访问此资源 / Device token cannot access this resource",
        _ => value["error"]["message"]
            .as_str()
            .unwrap_or("内容服务请求失败 / Content service request failed"),
    };
    let retry = value["error"]["retryAfter"]
        .as_u64()
        .map(|v| format!("; retry after {v}s"))
        .unwrap_or_default();
    anyhow!("{code}: {message}{retry}")
}
pub async fn limited_body(response: reqwest::Response, max: usize) -> Result<Vec<u8>> {
    ensure!(
        response.content_length().is_none_or(|n| n <= max as u64),
        "响应超过大小限制 / Response too large"
    );
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("响应中断或超时 / Response interrupted")?;
        ensure!(
            bytes.len() + chunk.len() <= max,
            "响应超过大小限制 / Response too large"
        );
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}
pub fn http_url(url: &str) -> Result<reqwest::Url> {
    let url = reqwest::Url::parse(url).context("无效服务或资源地址 / Invalid URL")?;
    ensure!(
        matches!(url.scheme(), "https" | "http")
            && url.username().is_empty()
            && url.password().is_none(),
        "不支持的地址 / Unsupported URL"
    );
    Ok(url)
}
