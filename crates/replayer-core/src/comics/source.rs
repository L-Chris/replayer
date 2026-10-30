use super::*;
use anyhow::{Context, Result, anyhow, ensure};
use futures_util::{StreamExt, future::BoxFuture};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};

mod dm5;
mod komiic;
mod sources;
pub use komiic::Komiic;
pub use sources::Sources;

/// UI and storage use normalized records, never provider-specific JSON.
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
    fn image_url(&self, site: &str, url: &str) -> Result<reqwest::Url>;
    fn image_request(&self, site: &str, url: &str) -> Result<ImageRequest> {
        Ok(self.image_url(site, url)?.into())
    }
    fn book_image_request(&self, book: &Book, url: &str) -> Result<ImageRequest> {
        self.image_request(&book.site, url)
    }
}

#[derive(Clone, Copy)]
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
    pub fallback: Option<reqwest::Url>,
    pub referer: Option<reqwest::Url>,
    pub headers: reqwest::header::HeaderMap,
    pub refresh: Option<BoxFuture<'static, Result<ImageRequest>>>,
    pub on_status: Option<Arc<dyn Fn(reqwest::StatusCode) -> Result<()> + Send + Sync>>,
    resolver: Option<BoxFuture<'static, Result<ImageRequest>>>,
}
impl From<reqwest::Url> for ImageRequest {
    fn from(url: reqwest::Url) -> Self {
        Self {
            url,
            fallback: None,
            referer: None,
            headers: reqwest::header::HeaderMap::new(),
            refresh: None,
            on_status: None,
            resolver: None,
        }
    }
}
impl ImageRequest {
    pub fn with_fallback(mut self, url: reqwest::Url) -> Self {
        self.fallback = Some(url);
        self
    }
    fn deferred(url: reqwest::Url, resolver: BoxFuture<'static, Result<Self>>) -> Self {
        Self {
            resolver: Some(resolver),
            ..url.into()
        }
    }
    /// Resolve only after checking the image cache, inside the cancellable page job.
    pub async fn resolve(mut self) -> Result<Self> {
        if let Some(resolve) = self.resolver.take() {
            let request = resolve.await?;
            ensure!(
                request.resolver.is_none(),
                "无效图片解析结果 / Invalid image resolution"
            );
            Ok(request)
        } else {
            Ok(self)
        }
    }
}
#[derive(Clone)]
pub struct MangaYun {
    base: String,
    client: reqwest::Client,
}
impl MangaYun {
    pub fn connect() -> Result<Arc<dyn ComicSource>> {
        Ok(Arc::new(Self::at("https://mangayun.com")?))
    }
    pub(super) fn at(base: &str) -> Result<Self> {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(12))
            .timeout(Duration::from_secs(65))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(concat!(
                "replayer/",
                env!("CARGO_PKG_VERSION"),
                " comic-reader"
            ))
            .build()?;
        Ok(Self {
            base: base.trim_end_matches('/').into(),
            client,
        })
    }
    async fn request(&self, path: &str, body: Option<Value>) -> Result<Value> {
        let body = body.map(|b| b.to_string()).unwrap_or_default();
        let ts = (now() * 1000).to_string();
        let nonce = uuid::Uuid::new_v4().simple().to_string();
        // Public web-client protocol; isolate it here when the website changes.
        let sign = digest(
            format!("{path}|{body}|{ts}|{nonce}|ym-salt-883a0f7e-29f1-4b72-9ad2").as_bytes(),
        );
        let url = format!("{}{path}", self.base);
        let request = if body.is_empty() {
            self.client.get(url)
        } else {
            self.client.post(url).body(body)
        };
        let response = request
            .header("Content-Type", "application/json")
            .header("x-ym-ts", ts)
            .header("x-ym-nonce", nonce)
            .header("x-ym-sign", sign)
            .send()
            .await
            .context("漫画源连接失败 / Comic source request failed")?;
        let status = response.status();
        ensure!(
            status.is_success(),
            "漫画源 HTTP {} / Comic source unavailable",
            status.as_u16()
        );
        let bytes = limited_body(response, 8 * 1024 * 1024).await?;
        let value: Value = serde_json::from_slice(&bytes)
            .map_err(|_| anyhow!("漫画源数据格式已变化 / Invalid comic response"))?;
        ensure!(
            value.get("data").is_some(),
            "漫画源未返回数据 / Missing comic data"
        );
        Ok(value["data"].clone())
    }
}
pub async fn limited_body(response: reqwest::Response, max: usize) -> Result<Vec<u8>> {
    ensure!(
        response.content_length().is_none_or(|n| n <= max as u64),
        "响应超过大小限制 / Response too large"
    );
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| anyhow!("响应中断或超时 / Response interrupted"))?;
        ensure!(
            bytes.len() + chunk.len() <= max,
            "响应超过大小限制 / Response too large"
        );
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}
pub fn http_url(url: &str) -> Result<reqwest::Url> {
    let url = reqwest::Url::parse(url).map_err(|_| anyhow!("无效图片地址 / Invalid image URL"))?;
    ensure!(
        matches!(url.scheme(), "https" | "http")
            && url.username().is_empty()
            && url.password().is_none(),
        "不支持的图片地址 / Unsupported URL"
    );
    Ok(url)
}
impl ComicSource for MangaYun {
    fn sites(&self) -> BoxFuture<'_, Result<Vec<Site>>> {
        Box::pin(async {
            Ok(serde_json::from_value(
                self.request("/api/sites?v=3", None).await?,
            )?)
        })
    }
    fn search<'a>(&'a self, keyword: &'a str) -> BoxFuture<'a, Result<Vec<SearchGroup>>> {
        Box::pin(async move {
            ensure!(
                !keyword.trim().is_empty() && keyword.len() <= 512,
                "请输入漫画名称 / Enter a title"
            );
            let data = self
                .request("/api/search", Some(json!({"keyword":keyword.trim()})))
                .await?;
            let groups = data
                .as_array()
                .ok_or_else(|| anyhow!("无效搜索结果 / Invalid search response"))?;
            let mut result = Vec::new();
            for group in groups.iter().take(40) {
                let site: Site = serde_json::from_value(group.clone())?;
                let mut books = Vec::new();
                if let Some(list) = group["results"].as_array() {
                    for item in list.iter().take(100) {
                        let mut book: Book = serde_json::from_value(item.clone())?;
                        book.provider = "mangayun".into();
                        book.site = site.id.clone();
                        book.site_name = site.name.clone();
                        if !book.title.is_empty() && !book.url.is_empty() {
                            books.push(book);
                        }
                    }
                }
                result.push(SearchGroup {
                    site,
                    books,
                    error: group["error"]
                        .as_str()
                        .filter(|e| !e.is_empty())
                        .map(|_| "来源暂时不可用 / Source unavailable".into()),
                });
            }
            Ok(result)
        })
    }
    fn detail<'a>(&'a self, book: &'a Book) -> BoxFuture<'a, Result<Detail>> {
        Box::pin(async move {
            let data = self
                .request(
                    "/api/details",
                    Some(json!({"siteId":book.site,"detailUrl":book.url})),
                )
                .await?;
            let mut details: Book = serde_json::from_value(data.clone())?;
            details.provider = book.provider.clone();
            details.site = book.site.clone();
            details.site_name = book.site_name.clone();
            details.url = book.url.clone();
            if details.cover.is_empty() {
                details.cover = book.cover.clone();
            }
            let mut chapters: Vec<Chapter> = serde_json::from_value(data["chapters"].clone())?;
            chapters.retain(|c| !c.url.is_empty());
            chapters.sort_by_key(|c| c.order);
            let mut seen = std::collections::HashSet::new();
            chapters.retain(|c| seen.insert(c.key().to_owned()));
            ensure!(!chapters.is_empty(), "没有可读章节 / No chapters available");
            Ok(Detail {
                book: details,
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
            ensure!(
                book.site != "comicbox",
                "暂不支持此来源的阅读，请选择其他来源 / Reading this source is not supported yet"
            );
            let result = self
                .request(
                    "/api/chapter-images",
                    Some(json!({"siteId":book.site,"chapterUrl":chapter.url,"refresh":refresh})),
                )
                .await;
            let pages = result.and_then(|data| {
                let images: Vec<String> = serde_json::from_value(data["images"].clone())?;
                ensure!(
                    !images.is_empty() && images.len() <= 2000,
                    "来源未返回章节图片 / Source returned no chapter images"
                );
                Ok(images)
            });
            if book.site == "dm5" && pages.is_err() {
                let chapter_url = http_url(&chapter.url)?;
                ensure!(
                    dm5::is_chapter(&chapter_url),
                    "无效动漫屋章节地址 / Invalid DM5 chapter URL"
                );
                return dm5::pages(&self.client, chapter_url).await;
            }
            pages
        })
    }
    fn image_url(&self, site: &str, url: &str) -> Result<reqwest::Url> {
        Ok(self.image_request(site, url)?.url)
    }
    fn image_request(&self, site: &str, url: &str) -> Result<ImageRequest> {
        let absolute = reqwest::Url::parse(&self.base)?
            .join(url)
            .map_err(|_| anyhow!("无效图片地址 / Invalid image URL"))?;
        http_url(absolute.as_str())?;
        if site == "dm5" && dm5::is_page(&absolute) {
            let source = self.clone();
            return Ok(ImageRequest::deferred(
                absolute.clone(),
                Box::pin(async move {
                    let (image, referer) = dm5::resolve(&source.client, absolute).await?;
                    let relay = source.image_request("dm5", image.as_str())?.url;
                    let mut request = ImageRequest::from(image).with_fallback(relay);
                    request.referer = Some(referer);
                    Ok(request)
                }),
            ));
        }
        if absolute.origin() == reqwest::Url::parse(&self.base)?.origin()
            && absolute.path() == "/api/image"
        {
            return Ok(absolute.into());
        }
        let mut proxy = reqwest::Url::parse(&format!("{}/api/image", self.base))?;
        proxy
            .query_pairs_mut()
            .append_pair("url", absolute.as_str())
            .append_pair("siteId", site);
        // Match the public web client: only hotlink-protected sources need
        // the relay initially. Other CDNs are faster directly, with a relay
        // fallback if the CDN rejects this client or is unavailable locally.
        let host = absolute.host_str().unwrap_or_default();
        let protected = [
            "hamreus.com",
            "cdndm5.com",
            "ccavbox.com",
            "bgm.tv",
            "tuku.cc",
        ]
        .iter()
        .any(|domain| host == *domain || host.ends_with(&format!(".{domain}")))
            || matches!(site, "manhuagui" | "comicbox" | "manben" | "dm5" | "tuku");
        Ok(if protected {
            proxy.into()
        } else {
            ImageRequest::from(absolute).with_fallback(proxy)
        })
    }
}
