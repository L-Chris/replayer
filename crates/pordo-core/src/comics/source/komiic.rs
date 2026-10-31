use super::*;
use anyhow::Context;
mod auth;
use auth::Sessions;
pub use auth::{AccountSettings, AuthStatus};

fn number(value: &Value) -> Option<u64> {
    value.as_u64().or_else(|| value.as_str()?.parse().ok())
}
#[cfg(test)]
mod auth_tests;
#[cfg(test)]
mod tests;

#[derive(Clone)]
pub struct Komiic {
    base: String,
    sessions: Arc<Sessions>,
}
impl Komiic {
    /// Single-provider entry point for hosts that do not bundle aggregators.
    pub fn connect() -> Result<Arc<dyn ComicSource>> {
        Ok(Arc::new(Self::at("https://komiic.com")?))
    }
    pub(super) fn at(base: &str) -> Result<Self> {
        Ok(Self {
            base: base.trim_end_matches('/').into(),
            sessions: Arc::new(Sessions::new(base)?),
        })
    }
    pub fn configure_account(&self, settings: AccountSettings) -> Result<()> {
        self.sessions.configure(&self.base, settings)
    }
    pub fn auth_status(&self) -> AuthStatus {
        self.sessions.current().status()
    }
    pub async fn login(&self) -> Result<AuthStatus> {
        let session = self.sessions.current();
        session.ensure_ready(&self.base, true).await?;
        self.sessions.check(&session)?;
        Ok(session.status())
    }
    pub async fn logout(&self) -> Result<()> {
        let old = self.sessions.current();
        self.configure_account(AccountSettings::default())?;
        old.logout(&self.base).await
    }
    #[cfg(test)]
    fn quota_block(&self, seconds: u64) {
        self.sessions.current().quota_block(seconds);
    }
    async fn query(&self, operation: &str, query: &str, variables: Value) -> Result<Value> {
        let session = self.sessions.current();
        session.ensure_ready(&self.base, false).await?;
        for attempt in 0..2 {
            self.sessions.check(&session)?;
            let epoch = session.epoch();
            let response = session
                .client
                .post(format!("{}/api/query", self.base))
                .header("Origin", &self.base)
                .header("Referer", format!("{}/", self.base))
                .json(&json!({"operationName":operation,"query":query,"variables":variables}))
                .send()
                .await
                .context("Komiic 连接失败 / Source request failed")?;
            let status = response.status();
            if status == reqwest::StatusCode::UNAUTHORIZED && attempt == 0 && session.configured() {
                session.recover(&self.base, epoch).await?;
                continue;
            }
            if status == reqwest::StatusCode::UNAUTHORIZED && session.configured() {
                session.reject(epoch);
            }
            if status == reqwest::StatusCode::PAYMENT_REQUIRED {
                session.quota_block(session.reset_seconds());
                session.quota_check()?;
            }
            ensure!(
                status.is_success(),
                "Komiic HTTP {} / Source unavailable",
                status.as_u16()
            );
            let bytes = limited_body(response, 8 * 1024 * 1024).await?;
            let value: Value = serde_json::from_slice(&bytes)
                .map_err(|_| anyhow!("Komiic 数据格式已变化 / Invalid source response"))?;
            self.sessions.check(&session)?;
            if auth::unauthorized(&value) && attempt == 0 && session.configured() {
                session.recover(&self.base, epoch).await?;
                continue;
            }
            if auth::unauthorized(&value) && session.configured() {
                session.reject(epoch);
            }
            session.quota_update(&value["data"]["getImageLimit"]);
            if let Some(errors) = value["errors"].as_array().filter(|e| !e.is_empty()) {
                if errors
                    .iter()
                    .any(|e| e["extensions"]["code"] == "QUOTA_EXCEEDED")
                {
                    session.quota_block(session.reset_seconds());
                    session.quota_check()?;
                }
                anyhow::bail!("Komiic 请求未成功 / Source rejected the request");
            }
            ensure!(
                value["data"].is_object(),
                "Komiic 未返回数据 / Missing source data"
            );
            return Ok(value["data"].clone());
        }
        anyhow::bail!("Komiic 会话已失效，请重新登录 / Sign in again")
    }
    fn id(url: &str, segment: &str) -> Result<String> {
        let url = http_url(url)?;
        ensure!(
            url.host_str() == Some("komiic.com"),
            "无效 Komiic 地址 / Invalid source URL"
        );
        let parts: Vec<_> = url.path_segments().into_iter().flatten().collect();
        let id = parts
            .windows(2)
            .find(|p| p[0] == segment)
            .map(|p| p[1])
            .unwrap_or("");
        ensure!(
            !id.is_empty() && id.len() <= 20 && id.bytes().all(|b| b.is_ascii_digit()),
            "无效漫画编号 / Invalid comic ID"
        );
        Ok(id.into())
    }
    fn book(value: &Value) -> Result<Book> {
        let id = value["id"].as_str().unwrap_or("");
        ensure!(
            !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()),
            "无效漫画编号 / Invalid comic ID"
        );
        let title = value["title"].as_str().unwrap_or("").trim();
        ensure!(!title.is_empty(), "缺少漫画名称 / Missing comic title");
        Ok(Book {
            provider: "komiic".into(),
            site: "komiic".into(),
            site_name: "Komiic".into(),
            title: title.into(),
            author: value["authors"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|a| a["name"].as_str())
                .collect::<Vec<_>>()
                .join(" / "),
            description: value["description"].as_str().unwrap_or("").into(),
            status: match value["status"].as_str() {
                Some("ONGOING") => "连载中",
                Some("END") => "已完结",
                _ => "",
            }
            .into(),
            cover: value["imageUrl"].as_str().unwrap_or("").into(),
            url: format!("https://komiic.com/comic/{id}"),
        })
    }
    fn ticket(&self, kid: String, refreshable: bool) -> BoxFuture<'_, Result<ImageRequest>> {
        Box::pin(async move {
            let session = self.sessions.current();
            session.ensure_ready(&self.base, false).await?;
            self.sessions.check(&session)?;
            session.quota_check()?;
            let value = self.query("ImageTicket", "query ImageTicket($kids:[String!]!){getImageTickets(kids:$kids){kid url ticket expiresAt}}", json!({"kids":[kid]})).await?;
            self.sessions.check(&session)?;
            let ticket = value["getImageTickets"]
                .as_array()
                .and_then(|a| a.iter().find(|v| v["kid"] == kid))
                .ok_or_else(|| anyhow!("Komiic 图片凭证不可用 / Missing image ticket"))?;
            let url = http_url(ticket["url"].as_str().unwrap_or(""))?;
            // Tickets must never be forwarded to an arbitrary host or through redirects.
            ensure!(
                (url.scheme() == "https" && url.host_str() == Some("img.komiic.com"))
                    || (cfg!(test) && url.origin() == http_url(&self.base)?.origin()),
                "无效图片服务器 / Invalid image server"
            );
            let mut request: ImageRequest = url.into();
            let mut header =
                reqwest::header::HeaderValue::from_str(ticket["ticket"].as_str().unwrap_or(""))
                    .map_err(|_| anyhow!("无效图片凭证 / Invalid image ticket"))?;
            ensure!(!header.is_empty(), "缺少图片凭证 / Missing image ticket");
            header.set_sensitive(true);
            request.headers.insert("x-image-ticket", header);
            let sessions = self.sessions.clone();
            request.on_status = Some(Arc::new(move |status| {
                sessions.check(&session)?;
                if status == reqwest::StatusCode::PAYMENT_REQUIRED {
                    session.quota_block(session.reset_seconds());
                    session.quota_check()?;
                }
                Ok(())
            }));
            if refreshable {
                let source = self.clone();
                request.refresh = Some(Box::pin(async move { source.ticket(kid, false).await }));
            }
            Ok(request)
        })
    }
}
impl ComicSource for Komiic {
    fn image_policy(&self, _book: &Book) -> ImagePolicy {
        ImagePolicy {
            concurrency: 2,
            ahead: 1,
            behind: 0,
            retain_cache_on_refresh: true,
        }
    }
    fn sites(&self) -> BoxFuture<'_, Result<Vec<Site>>> {
        Box::pin(async {
            Ok(vec![Site {
                id: "komiic".into(),
                name: "Komiic".into(),
            }])
        })
    }
    fn search<'a>(&'a self, keyword: &'a str) -> BoxFuture<'a, Result<Vec<SearchGroup>>> {
        Box::pin(async move {
            ensure!(
                !keyword.trim().is_empty() && keyword.len() <= 512,
                "请输入漫画名称 / Enter a comic title"
            );
            let value = self.query("Search", "query Search($keyword:String!){searchComicsAndAuthors(keyword:$keyword){comics{id title status imageUrl authors{name}}}}", json!({"keyword":keyword.trim()})).await?;
            let records = value["searchComicsAndAuthors"]["comics"]
                .as_array()
                .ok_or_else(|| anyhow!("Komiic 搜索数据已变化 / Invalid search response"))?;
            let mut seen = std::collections::HashSet::new();
            let books = records
                .iter()
                .take(200)
                .filter_map(|v| Self::book(v).ok())
                .filter(|b| seen.insert(b.key()))
                .collect();
            Ok(vec![SearchGroup {
                site: Site {
                    id: "komiic".into(),
                    name: "Komiic".into(),
                },
                books,
                error: None,
            }])
        })
    }
    fn detail<'a>(&'a self, book: &'a Book) -> BoxFuture<'a, Result<Detail>> {
        Box::pin(async move {
            let id = Self::id(&book.url, "comic")?;
            let value = self.query("Detail", "query Detail($comicId:ID!){comicById(comicId:$comicId){id title description status imageUrl authors{name}} chaptersByComicId(comicId:$comicId){id serial type size}}", json!({"comicId":id})).await?;
            let normalized = Self::book(&value["comicById"])?;
            ensure!(
                normalized.url == book.url,
                "漫画编号不一致 / Comic ID mismatch"
            );
            let records = value["chaptersByComicId"]
                .as_array()
                .ok_or_else(|| anyhow!("Komiic 目录格式已变化 / Invalid chapter list"))?;
            let mut seen = std::collections::HashSet::new();
            let mut chapters = Vec::new();
            for record in records.iter().take(10000) {
                let cid = record["id"].as_str().unwrap_or("");
                let serial = record["serial"].as_str().unwrap_or("");
                let kind = match record["type"].as_str() {
                    Some("chapter") => ChapterKind::Chapter,
                    Some("book") => ChapterKind::Volume,
                    _ => continue,
                };
                if cid.is_empty()
                    || !cid.bytes().all(|b| b.is_ascii_digit())
                    || !seen.insert(cid.to_owned())
                {
                    continue;
                }
                chapters.push(Chapter {
                    id: cid.into(),
                    name: format!(
                        "第 {serial} {}",
                        if kind == ChapterKind::Volume {
                            "卷"
                        } else {
                            "话"
                        }
                    ),
                    url: format!("https://komiic.com/comic/{id}/chapter/{cid}/images/all"),
                    order: 0,
                    kind,
                });
            }
            // Numeric serials, including decimals, sort naturally within each edition.
            chapters.sort_by(|a, b| {
                let serial = |c: &Chapter| {
                    c.name
                        .split_whitespace()
                        .nth(1)
                        .unwrap_or("")
                        .parse::<f64>()
                        .ok()
                        .filter(|n| n.is_finite())
                };
                (a.kind == ChapterKind::Volume)
                    .cmp(&(b.kind == ChapterKind::Volume))
                    .then_with(|| match (serial(a), serial(b)) {
                        (Some(a), Some(b)) => a.total_cmp(&b),
                        (Some(_), None) => std::cmp::Ordering::Less,
                        (None, Some(_)) => std::cmp::Ordering::Greater,
                        _ => a.name.cmp(&b.name),
                    })
                    .then_with(|| a.id.cmp(&b.id))
            });
            for (index, chapter) in chapters.iter_mut().enumerate() {
                chapter.order = index as i64;
            }
            ensure!(
                !chapters.is_empty(),
                "Komiic 暂无可读章节 / No readable chapters"
            );
            Ok(Detail {
                book: normalized,
                chapters,
            })
        })
    }
    fn pages<'a>(
        &'a self,
        book: &'a Book,
        chapter: &'a Chapter,
        _refresh: bool,
    ) -> BoxFuture<'a, Result<Vec<String>>> {
        Box::pin(async move {
            ensure!(
                Self::id(&book.url, "comic")? == Self::id(&chapter.url, "comic")?,
                "章节不属于该漫画 / Chapter belongs to another comic"
            );
            let id = Self::id(&chapter.url, "chapter")?;
            let value = self.query("Pages", "query Pages($chapterId:ID!){imagesByChapterId(chapterId:$chapterId){kid} getImageLimit{limit usage resetInSeconds}}", json!({"chapterId":id})).await?;
            // Keep the page list usable offline even when the remote image quota is exhausted.
            let records = value["imagesByChapterId"]
                .as_array()
                .ok_or_else(|| anyhow!("Komiic 图片列表已变化 / Invalid page list"))?;
            let mut pages = Vec::new();
            for record in records.iter().take(2000) {
                let kid = record["kid"].as_str().unwrap_or("");
                ensure!(
                    uuid::Uuid::parse_str(kid).is_ok(),
                    "无效图片编号 / Invalid image ID"
                );
                pages.push(format!("{}/api/image/{kid}", self.base));
            }
            ensure!(
                !pages.is_empty() && records.len() <= 2000,
                "Komiic 无有效图片 / No valid pages"
            );
            Ok(pages)
        })
    }
    fn image_url(&self, _site: &str, url: &str) -> Result<reqwest::Url> {
        http_url(url)
    }
    fn image_request(&self, _site: &str, url: &str) -> Result<ImageRequest> {
        let url = http_url(url)?;
        if url.origin() == http_url(&self.base)?.origin()
            && let Some(kid) = url.path().strip_prefix("/api/image/")
        {
            ensure!(
                uuid::Uuid::parse_str(kid).is_ok(),
                "无效图片编号 / Invalid image ID"
            );
            let kid = kid.to_owned();
            let source = self.clone();
            return Ok(ImageRequest::deferred(
                url,
                Box::pin(async move { source.ticket(kid, true).await }),
            ));
        }
        ensure!(
            url.scheme() == "https" && url.host_str() == Some("public.komiic.com"),
            "无效 Komiic 图片地址 / Invalid image URL"
        );
        Ok(url.into())
    }
}
