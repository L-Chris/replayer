//! DM5's public reader protocol. Extract data from its packed image response;
//! never execute downloaded JavaScript. Page endpoints are resolved on demand.
use super::{http_url, limited_body};
use anyhow::{Result, anyhow, ensure};
use regex::Regex;
use reqwest::{Client, Url};
use std::{sync::OnceLock, time::Duration};

fn on_dm5(url: &Url) -> bool {
    matches!(url.host_str(), Some("www.dm5.com" | "dm5.com"))
}
pub(super) fn is_chapter(url: &Url) -> bool {
    on_dm5(url) && chapter_id(url.path()).is_some()
}
fn chapter_id(path: &str) -> Option<&str> {
    let id = path.strip_prefix("/m")?.strip_suffix('/')?;
    (!id.is_empty() && id.bytes().all(|b| b.is_ascii_digit())).then_some(id)
}
pub(super) fn is_page(url: &Url) -> bool {
    on_dm5(url)
        && url
            .path()
            .strip_suffix("chapterfun.ashx")
            .and_then(chapter_id)
            .is_some()
}

async fn text(client: &Client, url: Url, referer: &Url) -> Result<String> {
    let response = client
        .get(url)
        .header(reqwest::header::USER_AGENT, "Mozilla/5.0")
        .header(reqwest::header::REFERER, referer.as_str())
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .map_err(|_| anyhow!("动漫屋连接失败或超时 / DM5 request timed out"))?;
    ensure!(
        response.status().is_success(),
        "动漫屋 HTTP {} / DM5 unavailable",
        response.status().as_u16()
    );
    String::from_utf8(limited_body(response, 1024 * 1024).await?)
        .map_err(|_| anyhow!("动漫屋返回了无效文本 / Invalid DM5 response"))
}

pub(super) async fn pages(client: &Client, chapter: Url) -> Result<Vec<String>> {
    page_endpoints(&text(client, chapter.clone(), &chapter).await?, &chapter)
}

fn page_endpoints(html: &str, chapter: &Url) -> Result<Vec<String>> {
    let count: usize = variable(html, "DM5_IMAGE_COUNT")?.parse()?;
    ensure!(
        (1..=2000).contains(&count),
        "动漫屋未返回章节图片 / DM5 chapter has no images"
    );
    let cid = variable(html, "DM5_CID")?;
    let mid = variable(html, "DM5_MID")?;
    let dt = variable(html, "DM5_VIEWSIGN_DT")?;
    let sign = variable(html, "DM5_VIEWSIGN")?;
    ensure!(
        chapter_id(chapter.path()) == Some(cid.as_str()),
        "动漫屋章节信息不匹配 / DM5 chapter mismatch"
    );
    let mut endpoint = chapter.join("chapterfun.ashx")?;
    endpoint.set_query(None);
    endpoint.query_pairs_mut().extend_pairs([
        ("cid", cid.as_str()),
        ("key", ""),
        ("language", "1"),
        ("gtk", "6"),
        ("_cid", cid.as_str()),
        ("_mid", mid.as_str()),
        ("_dt", dt.as_str()),
        ("_sign", sign.as_str()),
    ]);
    Ok((1..=count)
        .map(|index| {
            let mut url = endpoint.clone();
            url.query_pairs_mut()
                .append_pair("page", &index.to_string());
            url.into()
        })
        .collect())
}

pub(super) async fn resolve(client: &Client, endpoint: Url) -> Result<(Url, Url)> {
    let referer = endpoint.join("./")?;
    let page = endpoint
        .query_pairs()
        .find(|(key, _)| key == "page")
        .and_then(|(_, value)| value.parse::<usize>().ok())
        .filter(|page| (1..=2000).contains(page))
        .ok_or_else(|| anyhow!("无效动漫屋页码 / Invalid DM5 page"))?;
    let result = text(client, endpoint, &referer)
        .await
        .and_then(|script| image_url(&script, &referer));
    let image = match result {
        Ok(image) => image,
        Err(_) => {
            // Reader signatures can expire while a chapter stays open. Refresh
            // the public chapter metadata once, preserving the requested page.
            let urls = pages(client, referer.clone()).await?;
            let endpoint = urls.get(page - 1).ok_or_else(|| {
                anyhow!("动漫屋页码已变化，请刷新章节 / DM5 page no longer exists")
            })?;
            image_url(
                &text(client, http_url(endpoint)?, &referer).await?,
                &referer,
            )?
        }
    };
    Ok((image, referer))
}

fn variable(script: &str, name: &str) -> Result<String> {
    let re = Regex::new(&format!(
        r#"\bvar\s+{}\s*=\s*("(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'|\d+)"#,
        regex::escape(name)
    ))?;
    let value = re
        .captures(script)
        .and_then(|c| c.get(1))
        .ok_or_else(|| anyhow!("动漫屋阅读数据不完整 / Missing DM5 reader data: {name}"))?
        .as_str();
    if value.starts_with(['\'', '"']) {
        unescape(&value[1..value.len() - 1])
    } else {
        Ok(value.into())
    }
}

fn unescape(value: &str) -> Result<String> {
    let mut result = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            result.push(c);
            continue;
        }
        let escaped = chars
            .next()
            .ok_or_else(|| anyhow!("Invalid DM5 string escape"))?;
        result.push(match escaped {
            'n' => '\n',
            'r' => '\r',
            't' => '\t',
            'x' | 'u' => {
                let digits = if escaped == 'x' { 2 } else { 4 };
                let hex: String = chars.by_ref().take(digits).collect();
                ensure!(hex.len() == digits, "Invalid DM5 character escape");
                char::from_u32(u32::from_str_radix(&hex, 16)?)
                    .ok_or_else(|| anyhow!("Invalid DM5 character"))?
            }
            other => other,
        });
    }
    Ok(result)
}

fn unpack(script: &str) -> Result<String> {
    static PACKER: OnceLock<Regex> = OnceLock::new();
    static WORD: OnceLock<Regex> = OnceLock::new();
    let re = PACKER.get_or_init(|| Regex::new(r#"(?s)\}\s*\(\s*'(.*)'\s*,\s*(\d+)\s*,\s*(\d+)\s*,\s*'([^']*)'\.split\('\|'\)\s*,\s*0\s*,\s*\{\}\s*\)\s*\)"#).unwrap());
    let Some(caps) = re.captures(script) else {
        ensure!(
            !script.contains("eval("),
            "动漫屋图片脚本格式已变化 / Unsupported DM5 image script"
        );
        return Ok(script.into());
    };
    let radix: u32 = caps[2].parse()?;
    let count: usize = caps[3].parse()?;
    ensure!(
        (2..=62).contains(&radix) && count <= 10000,
        "Invalid DM5 symbol table"
    );
    let words: Vec<_> = caps[4].split('|').collect();
    ensure!(words.len() == count, "Invalid DM5 symbol count");
    // Some responses escape backslashes twice, including the quoted key.
    let payload = if caps[1].contains(r"\\'") {
        caps[1].replace(r"\\", r"\")
    } else {
        caps[1].into()
    };
    let payload = unescape(&payload)?;
    let re = WORD.get_or_init(|| Regex::new(r"\b[0-9A-Za-z_]+\b").unwrap());
    let mut output = String::new();
    let mut end = 0;
    for token in re.find_iter(&payload) {
        let number = token.as_str().bytes().try_fold(0usize, |value, digit| {
            let digit = match digit {
                b'0'..=b'9' => digit - b'0',
                b'a'..=b'z' => digit - b'a' + 10,
                b'A'..=b'Z' => digit - b'A' + 36,
                _ => return None,
            };
            if u32::from(digit) >= radix {
                return None;
            }
            value
                .checked_mul(radix as usize)?
                .checked_add(digit as usize)
        });
        let word = number
            .and_then(|n| words.get(n))
            .filter(|s| !s.is_empty())
            .copied()
            .unwrap_or(token.as_str());
        ensure!(
            output.len() + token.start() - end + word.len() <= 2 * 1024 * 1024,
            "DM5 image script is too large"
        );
        output.push_str(&payload[end..token.start()]);
        output.push_str(word);
        end = token.end();
    }
    output.push_str(&payload[end..]);
    Ok(output)
}

fn image_url(script: &str, referer: &Url) -> Result<Url> {
    static VALUES: OnceLock<Regex> = OnceLock::new();
    let script = unpack(script)?;
    let re = VALUES.get_or_init(|| Regex::new(r"(?s)\bvar\s+pvalue\s*=\s*(\[[^;]*?\])").unwrap());
    let values = re
        .captures(&script)
        .ok_or_else(|| anyhow!("动漫屋未返回图片地址 / DM5 returned no image URL"))?;
    let values: Vec<String> = serde_json::from_str(&values[1])?;
    let first = values
        .first()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("动漫屋图片列表为空 / Empty DM5 image list"))?;
    let url =
        if first.starts_with("https://") || first.starts_with("http://") || first.starts_with("//")
        {
            referer.join(first)?
        } else {
            let mut url = http_url(&format!("{}{first}", variable(&script, "pix")?))?;
            url.query_pairs_mut()
                .append_pair("cid", &variable(&script, "cid")?)
                .append_pair("key", &variable(&script, "key")?);
            url
        };
    http_url(url.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::comics::{cache, source::ImageRequest};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    fn html(count: usize) -> String {
        format!(
            r#"var DM5_CID=123; var DM5_MID=99; var DM5_IMAGE_COUNT={count}; var DM5_VIEWSIGN="fresh"; var DM5_VIEWSIGN_DT="2026-10-22 09:00:00";"#
        )
    }

    #[test]
    fn chapter_metadata_yields_ordered_lazy_endpoints_and_rejects_invalid_chapters() {
        let chapter: Url = "https://www.dm5.com/m123/".parse().unwrap();
        let pages = page_endpoints(&html(56), &chapter).unwrap();
        assert_eq!(pages.len(), 56);
        for (index, url) in pages.iter().enumerate() {
            let url = Url::parse(url).unwrap();
            assert!(is_page(&url));
            let pairs: std::collections::HashMap<_, _> = url.query_pairs().collect();
            assert_eq!(pairs["page"], (index + 1).to_string());
            assert_eq!(pairs["_cid"], "123");
            assert_eq!(pairs["_sign"], "fresh");
            assert_eq!(pairs["_dt"], "2026-10-22 09:00:00");
        }
        assert!(is_chapter(&chapter));
        assert!(!is_chapter(
            &Url::parse("https://other.example/m123/").unwrap()
        ));
        assert!(!is_chapter(
            &Url::parse("https://www.dm5.com/login/").unwrap()
        ));
        assert!(page_endpoints(&html(0), &chapter).is_err());
        assert!(page_endpoints(&html(2001), &chapter).is_err());
        assert!(page_endpoints("Please log in", &chapter).is_err());
        assert!(
            page_endpoints(&html(56), &Url::parse("https://www.dm5.com/m456/").unwrap()).is_err()
        );
    }

    #[test]
    fn image_scripts_decode_both_escape_formats_without_executing_code() {
        let payload = r#"0 1="https://cdn.example/chapter";0 2=["/page-a.jpg","/page-b.jpg"];0 3=123;0 4='token';"#;
        let escaped = payload.replace('\\', r"\\").replace('\'', r"\'");
        let chapter = Url::parse("https://www.dm5.com/m123/").unwrap();
        for payload in [escaped.clone(), escaped.replace('\\', r"\\")] {
            let packed = format!(
                "eval(function(p,a,c,k,e,d){{return p;}}('{payload}',10,5,'var|pix|pvalue|cid|key'.split('|'),0,{{}}))"
            );
            let image = image_url(&packed, &chapter).unwrap();
            assert_eq!(
                image.as_str(),
                "https://cdn.example/chapter/page-a.jpg?cid=123&key=token"
            );
        }
        assert_eq!(
            image_url(r#"var pvalue=["//cdn.example/page.jpg"]"#, &chapter)
                .unwrap()
                .as_str(),
            "https://cdn.example/page.jpg"
        );
        for invalid in [
            "",
            "eval(fetch('https://other.example'))",
            "var pvalue=[]",
            r#"var pvalue=["file:///secret"]"#,
        ] {
            assert!(image_url(invalid, &chapter).is_err());
        }
        let mut words: Vec<_> = (0..62).map(|_| String::new()).collect();
        words[61] = "pvalue".into();
        let script = format!(
            "eval(function(p,a,c,k,e,d){{return p;}}('var Z=[\"https://cdn.example/leaf.jpg\"]',62,62,'{}'.split('|'),0,{{}}))",
            words.join("|")
        );
        assert_eq!(image_url(&script, &chapter).unwrap().path(), "/leaf.jpg");
    }

    #[test]
    fn expired_signature_refreshes_the_requested_page_and_cached_images_skip_resolution() {
        use axum::{Router, extract::Query, http::HeaderMap, routing::get};
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let script_calls = Arc::new(AtomicUsize::new(0));
                let image_calls = Arc::new(AtomicUsize::new(0));
                let chapter_calls = Arc::new(AtomicUsize::new(0));
                let scripts = script_calls.clone();
                let images = image_calls.clone();
                let chapters = chapter_calls.clone();
                let app = Router::new()
                    .route(
                        "/m123/",
                        get(move || {
                            let n = chapters.clone();
                            async move {
                                n.fetch_add(1, Ordering::Relaxed);
                                html(2)
                            }
                        }),
                    )
                    .route(
                        "/m123/chapterfun.ashx",
                        get(
                            move |headers: HeaderMap,
                                  Query(query): Query<
                                std::collections::HashMap<String, String>,
                            >| {
                                let n = scripts.clone();
                                async move {
                                    n.fetch_add(1, Ordering::Relaxed);
                                    assert_eq!(query["page"], "2");
                                    let host = headers["host"].to_str().unwrap();
                                    assert_eq!(
                                        headers["referer"].to_str().unwrap(),
                                        format!("http://{host}/m123/")
                                    );
                                    if query["_sign"] == "expired" {
                                        String::new()
                                    } else {
                                        format!(r#"var pvalue=["http://{host}/image"]"#)
                                    }
                                }
                            },
                        ),
                    )
                    .route(
                        "/image",
                        get(move |headers: HeaderMap| {
                            let n = images.clone();
                            async move {
                                n.fetch_add(1, Ordering::Relaxed);
                                assert!(headers["referer"].to_str().unwrap().ends_with("/m123/"));
                                let mut bytes = std::io::Cursor::new(Vec::new());
                                image::RgbaImage::from_pixel(
                                    32,
                                    48,
                                    image::Rgba([100, 120, 140, 255]),
                                )
                                .write_to(&mut bytes, image::ImageFormat::Png)
                                .unwrap();
                                bytes.into_inner()
                            }
                        }),
                    );
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                let chapter =
                    Url::parse(&format!("http://{}/m123/", listener.local_addr().unwrap()))
                        .unwrap();
                let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
                let mut endpoint = Url::parse(
                    &page_endpoints(&html(2).replace("fresh", "expired"), &chapter).unwrap()[1],
                )
                .unwrap();
                endpoint.set_fragment(None);
                let directory =
                    std::env::temp_dir().join(format!("replayer-dm5-{}", uuid::Uuid::new_v4()));
                for _ in 0..2 {
                    let url = endpoint.clone();
                    let request = ImageRequest::deferred(
                        url.clone(),
                        Box::pin(async move {
                            let (image, referer) = resolve(&Client::new(), url).await?;
                            let mut request = ImageRequest::from(image);
                            request.referer = Some(referer);
                            Ok(request)
                        }),
                    );
                    let image = cache::fetch(&directory, "page-2", request, 1800)
                        .await
                        .unwrap();
                    assert_eq!((image.bitmap.width, image.bitmap.height), (32, 48));
                }
                assert_eq!(script_calls.load(Ordering::Relaxed), 2);
                assert_eq!(chapter_calls.load(Ordering::Relaxed), 1);
                assert_eq!(image_calls.load(Ordering::Relaxed), 1);
                std::fs::remove_dir_all(directory).unwrap();
                server.abort();
            });
    }
}
