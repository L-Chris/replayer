use super::*;
use futures_util::StreamExt;
use reqwest::{Client, Method, StatusCode, Url};
use std::{collections::BTreeMap, time::Duration};
const MAX_BODY: usize = 8 * 1024 * 1024;

pub struct WebDav {
    http: Client,
    root: Url,
    user: String,
    password: String,
    pace: Duration,
}
impl WebDav {
    pub fn new(settings: &Settings) -> Result<Self> {
        settings.validate()?;
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
        Ok(Self {
            http: Client::builder()
                .connect_timeout(Duration::from_secs(10))
                .timeout(Duration::from_secs(45))
                .redirect(reqwest::redirect::Policy::none())
                .user_agent("pordo WebDAV Zotero/7.0")
                .build()?,
            root: Url::parse(&format!(
                "{}/",
                settings.base_url.trim().trim_end_matches('/')
            ))?,
            user: settings.username.trim().into(),
            password: settings.password.clone(),
            pace: if settings.provider == Provider::CstCloud
                || settings.provider == Provider::Jianguoyun
            {
                Duration::from_millis(150)
            } else {
                Duration::ZERO
            },
        })
    }
    async fn request(
        &self,
        method: Method,
        path: &str,
        body: Option<Vec<u8>>,
        headers: &[(&str, &str)],
    ) -> Result<reqwest::Response> {
        tokio::time::sleep(self.pace).await;
        let mut request = self
            .http
            .request(method, self.root.join(path)?)
            .basic_auth(&self.user, Some(&self.password));
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        if let Some(body) = body {
            request = request.body(body);
        }
        let response = request.send().await.map_err(|error| {
            anyhow::Error::new(error).context("WebDAV 连接失败 / WebDAV connection failed")
        })?;
        if response.status() == StatusCode::TOO_MANY_REQUESTS {
            anyhow::bail!("WebDAV 请求过于频繁，请稍后重试 / WebDAV rate limited; try later");
        }
        Ok(response)
    }
    pub async fn prepare(&self) -> Result<()> {
        // Keep the v1 wire namespace shared with existing devices.
        for path in ["replayer/", "replayer/v1/", "replayer/v1/devices/"] {
            let response = self
                .request(Method::from_bytes(b"MKCOL")?, path, None, &[])
                .await?;
            ensure!(
                matches!(response.status().as_u16(), 201 | 405),
                "WebDAV 创建同步目录失败 / Cannot create sync directory: HTTP {}",
                response.status().as_u16()
            );
        }
        Ok(())
    }
    async fn bytes(response: reqwest::Response) -> Result<Vec<u8>> {
        ensure!(
            response
                .content_length()
                .is_none_or(|n| n <= MAX_BODY as u64),
            "Sync response exceeds size limit"
        );
        let mut bytes = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(part) = stream.next().await {
            let part =
                part.map_err(|_| anyhow::anyhow!("WebDAV 响应中断 / WebDAV response interrupted"))?;
            ensure!(
                bytes.len() + part.len() <= MAX_BODY,
                "Sync response exceeds size limit"
            );
            bytes.extend_from_slice(&part);
        }
        Ok(bytes)
    }
    pub async fn list(&self) -> Result<Vec<String>> {
        let response = self.request(Method::from_bytes(b"PROPFIND")?, "replayer/v1/devices/",
            Some(b"<?xml version=\"1.0\"?><d:propfind xmlns:d=\"DAV:\"><d:prop><d:getetag/></d:prop></d:propfind>".to_vec()),
            &[("Depth", "1"), ("Content-Type", "application/xml; charset=utf-8")]).await?;
        ensure!(
            response.status().as_u16() == 207,
            "WebDAV 无法读取设备列表 / Cannot list sync devices: HTTP {}",
            response.status().as_u16()
        );
        let xml = String::from_utf8(Self::bytes(response).await?)?;
        let directory = self.root.join("replayer/v1/devices/")?;
        list_paths(&xml, &directory)
    }
    async fn get(&self, path: &str) -> Result<Option<(Vec<u8>, Option<String>)>> {
        let response = self.request(Method::GET, path, None, &[]).await?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        ensure!(
            response.status().is_success(),
            "WebDAV 读取失败 / WebDAV read failed: HTTP {}",
            response.status().as_u16()
        );
        let etag = response
            .headers()
            .get("etag")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        Ok(Some((Self::bytes(response).await?, etag)))
    }
    pub(super) async fn put(&self, path: &str, bytes: Vec<u8>, etag: Option<&str>) -> Result<()> {
        ensure!(bytes.len() <= MAX_BODY, "Sync document exceeds size limit");
        let condition = etag.map_or(("If-None-Match", "*"), |e| ("If-Match", e));
        let response = self
            .request(
                Method::PUT,
                path,
                Some(bytes),
                &[condition, ("Content-Type", "application/json")],
            )
            .await?;
        ensure!(
            response.status().is_success(),
            "WebDAV 写入失败或版本冲突 / WebDAV write failed or version changed: HTTP {}",
            response.status().as_u16()
        );
        Ok(())
    }
}
pub fn list_paths(xml: &str, directory: &Url) -> Result<Vec<String>> {
    use quick_xml::{Reader, events::Event};
    let mut reader = Reader::from_str(xml);
    let mut paths = std::collections::BTreeSet::new();
    loop {
        match reader.read_event()? {
            Event::Eof => break,
            Event::Start(e) if e.local_name().as_ref() == "href" => {
                let text = reader.read_text(e.name())?;
                let text = quick_xml::escape::unescape(&text)?;
                let url = directory.join(&text)?;
                ensure!(
                    url.origin() == directory.origin(),
                    "Invalid sync device origin"
                );
                if let Some(name) = url.path().strip_prefix(directory.path())
                    && let Some(id) = name.strip_suffix(".json")
                    && uuid::Uuid::parse_str(id).is_ok()
                    && !name.contains('/')
                {
                    paths.insert(format!("replayer/v1/devices/{name}"));
                }
            }
            _ => {}
        }
    }
    ensure!(paths.len() <= 256, "Too many sync devices");
    Ok(paths.into_iter().collect())
}

#[derive(Serialize, Deserialize)]
pub struct Report {
    pub journal: Journal,
    pub secrets: BTreeMap<String, String>,
}
pub async fn run(
    settings: Settings,
    mut journal: Journal,
    secrets: BTreeMap<String, String>,
) -> Result<Report> {
    let http = WebDav::new(&settings)?;
    http.prepare().await?;
    if settings.sync_keys {
        let keys: std::collections::BTreeSet<_> = secrets
            .keys()
            .chain(journal.secret_hashes.keys())
            .cloned()
            .collect();
        for id in keys {
            let plain = secrets.get(&id).cloned().unwrap_or_default();
            let hash = crate::comics::digest(plain.as_bytes());
            if journal.secret_hashes.get(&id) == Some(&hash)
                || !journal.secret_hashes.contains_key(&id) && plain.is_empty()
            {
                continue;
            }
            let key = format!("secret/{id}");
            let phrase = settings.passphrase.clone();
            let identity = key.clone();
            let value = tokio::task::spawn_blocking(move || {
                crypto::seal(&phrase, &identity, plain.as_bytes())
            })
            .await??;
            journal.write(key, Some(serde_json::to_value(value)?));
            journal.secret_hashes.insert(id, hash);
        }
    }
    let own = format!("replayer/v1/devices/{}.json", settings.device_id);
    let mut own_etag = None;
    let mut own_exists = false;
    let mut paths = http.list().await?;
    if !paths.contains(&own) {
        paths.push(own.clone());
    }
    for path in paths {
        if let Some((bytes, etag)) = http.get(&path).await? {
            if path == own {
                own_exists = true;
                own_etag = etag;
            }
            let remote: Document = serde_json::from_slice(&bytes)
                .map_err(|_| anyhow::anyhow!("云端同步数据无效 / Invalid cloud sync data"))?;
            journal.merge(&remote)?;
        }
    }
    let mut decrypted = BTreeMap::new();
    if settings.sync_keys {
        for (id, value) in journal
            .values()
            .into_iter()
            .filter_map(|(k, v)| k.strip_prefix("secret/").map(|s| (s.to_owned(), v)))
        {
            let sealed: crypto::Sealed = serde_json::from_value(value)?;
            let phrase = settings.passphrase.clone();
            let identity = format!("secret/{id}");
            let bytes =
                tokio::task::spawn_blocking(move || crypto::open(&phrase, &identity, &sealed))
                    .await??;
            let plain = String::from_utf8(bytes)?;
            journal
                .secret_hashes
                .insert(id.clone(), crate::comics::digest(plain.as_bytes()));
            decrypted.insert(id, plain);
        }
    }
    let digest = journal.digest();
    if digest != journal.uploaded || !own_exists {
        ensure!(
            !own_exists || own_etag.is_some(),
            "WebDAV 服务未提供版本校验 / WebDAV server did not provide an ETag"
        );
        http.put(
            &own,
            serde_json::to_vec(&journal.document)?,
            own_etag.as_deref(),
        )
        .await?;
        journal.uploaded = digest;
    }
    journal.last_success = crate::comics::now();
    Ok(Report {
        journal,
        secrets: decrypted,
    })
}

pub async fn test(settings: Settings) -> Result<()> {
    let http = WebDav::new(&settings)?;
    http.prepare().await?;
    http.list().await?;
    // Check write/read access in an isolated probe. Never touch a device document.
    let path = format!("replayer/v1/probe-{}.json", uuid::Uuid::new_v4());
    http.put(&path, b"{}".to_vec(), None).await?;
    let read = http.get(&path).await?;
    let response = http.request(Method::DELETE, &path, None, &[]).await?;
    ensure!(
        response.status().is_success() && read.is_some_and(|(v, _)| v == b"{}"),
        "WebDAV 读写测试失败 / WebDAV read/write test failed"
    );
    Ok(())
}
