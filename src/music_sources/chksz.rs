use super::*;
use anyhow::{anyhow, ensure};
use futures_util::StreamExt;
use serde_json::Value;
use std::time::Duration;

const NETEASE_QUALITY: &[Quality] = &[
    Quality {
        id: "standard",
        name: "MP3 128k",
    },
    Quality {
        id: "exhigh",
        name: "MP3 320k",
    },
    Quality {
        id: "lossless",
        name: "FLAC",
    },
    Quality {
        id: "hires",
        name: "Hi-Res",
    },
];
const QUALITY: &[Quality] = &[
    Quality {
        id: "128k",
        name: "MP3 128k",
    },
    Quality {
        id: "320k",
        name: "MP3 320k",
    },
    Quality {
        id: "flac",
        name: "FLAC",
    },
    Quality {
        id: "hires",
        name: "Hi-Res",
    },
];
#[derive(Clone, Copy)]
enum Platform {
    Netease,
    Qq,
    Kugou,
}
struct Chksz {
    base: String,
    key: String,
    platform: Platform,
    client: reqwest::Client,
}
pub(super) fn sources(base: &str, key: &str) -> Result<Vec<Arc<dyn MusicSource>>> {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(60))
        .build()?;
    Ok([Platform::Netease, Platform::Qq, Platform::Kugou]
        .into_iter()
        .map(|platform| {
            Arc::new(Chksz {
                base: base.trim_end_matches('/').into(),
                key: key.into(),
                platform,
                client: client.clone(),
            }) as Arc<dyn MusicSource>
        })
        .collect())
}
impl Chksz {
    async fn request(&self, endpoint: &str, params: &[(&str, String)]) -> Result<Value> {
        ensure!(
            !self.key.trim().is_empty(),
            "Set CHKSZ_API_KEY in .env or Music sources settings"
        );
        // Never return reqwest's error: its URL contains the API key.
        let response = self
            .client
            .get(format!("{}/api/{endpoint}", self.base))
            .query(params)
            .query(&[("apikey", &self.key)])
            .send()
            .await
            .map_err(|_| anyhow!("Music API connection failed or timed out"))?;
        ensure!(
            response.status().is_success(),
            "Music API HTTP {}",
            response.status().as_u16()
        );
        let mut stream = response.bytes_stream();
        let mut bytes = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| anyhow!("Music API response interrupted"))?;
            ensure!(
                bytes.len() + chunk.len() <= 2 * 1024 * 1024,
                "Music API response too large"
            );
            bytes.extend_from_slice(&chunk);
        }
        let data: Value =
            serde_json::from_slice(&bytes).map_err(|_| anyhow!("Invalid music API response"))?;
        // Upstream messages can echo credentials/URLs; expose only the status code.
        let code = data["code"]
            .as_i64()
            .or_else(|| data["code"].as_str()?.parse().ok())
            .unwrap_or(0);
        ensure!(code == 200, "Music API code {code}");
        Ok(data)
    }
}
fn field(v: &Value, key: &str) -> String {
    match &v[key] {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}
fn cover_url(value: &str) -> Option<reqwest::Url> {
    let url = reqwest::Url::parse(value).ok()?;
    (matches!(url.scheme(), "http" | "https")
        && url.username().is_empty()
        && url.password().is_none())
    .then_some(url)
}
impl MusicSource for Chksz {
    fn descriptor(&self) -> Descriptor {
        match self.platform {
            Platform::Netease => Descriptor {
                id: "chksz-163",
                name: "网易云",
                qualities: NETEASE_QUALITY,
                default_quality: "lossless",
            },
            Platform::Qq => Descriptor {
                id: "chksz-qq",
                name: "QQ音乐",
                qualities: QUALITY,
                default_quality: "flac",
            },
            Platform::Kugou => Descriptor {
                id: "chksz-kugou",
                name: "酷狗",
                qualities: QUALITY,
                default_quality: "flac",
            },
        }
    }
    fn search<'a>(&'a self, keyword: &'a str, page: usize) -> BoxFuture<'a, Result<SearchPage>> {
        Box::pin(async move {
            ensure!(!keyword.trim().is_empty(), "Enter a song or artist");
            let (endpoint, params) = match self.platform {
                Platform::Netease => (
                    "163_search",
                    vec![
                        ("keyword", keyword.into()),
                        ("limit", "20".into()),
                        ("offset", page.saturating_mul(20).to_string()),
                    ],
                ),
                Platform::Qq => {
                    ensure!(page == 0, "This source does not support pagination");
                    (
                        "qq_music",
                        vec![("msg", keyword.into()), ("num", "20".into())],
                    )
                }
                Platform::Kugou => {
                    ensure!(page == 0, "This source does not support pagination");
                    ("kugou_music", vec![("msg", keyword.into())])
                }
            };
            let data = self.request(endpoint, &params).await?;
            let (list_key, id_key, artist_key) = match self.platform {
                Platform::Netease => ("data", "id", "artists"),
                Platform::Qq => ("list", "mid", "singer"),
                Platform::Kugou => ("list", "id", "singer"),
            };
            let list = data[list_key]
                .as_array()
                .or_else(|| data[list_key]["songs"].as_array())
                .ok_or_else(|| anyhow!("Invalid music search response"))?;
            let songs = list
                .iter()
                .take(100)
                .filter_map(|v| {
                    let id = field(v, id_key);
                    let title = field(v, "name");
                    (!id.is_empty() && !title.is_empty()).then(|| Song {
                        source: self.descriptor().id,
                        id,
                        title,
                        artist: field(v, artist_key),
                        album: field(v, "album"),
                        cover: cover_url(&field(v, "picUrl"))
                            .or_else(|| cover_url(&field(v, "cover"))),
                    })
                })
                .collect();
            Ok(SearchPage {
                songs,
                has_more: matches!(self.platform, Platform::Netease)
                    && data["data"]["total"]
                        .as_u64()
                        .map_or(list.len() >= 20, |total| {
                            (page.saturating_add(1).saturating_mul(20) as u64) < total
                        }),
            })
        })
    }
    fn resolve<'a>(
        &'a self,
        song: &'a Song,
        quality: &'a str,
    ) -> BoxFuture<'a, Result<ResolvedTrack>> {
        Box::pin(async move {
            let descriptor = self.descriptor();
            ensure!(
                song.source == descriptor.id,
                "Song belongs to another music source"
            );
            ensure!(
                descriptor.qualities.iter().any(|q| q.id == quality),
                "Unsupported music quality"
            );
            let (endpoint, id_key, quality_key) = match self.platform {
                Platform::Netease => ("163_music", "id", "level"),
                Platform::Qq => ("qq_music", "mid", "size"),
                Platform::Kugou => ("kugou_music", "id", "size"),
            };
            let result = self
                .request(
                    endpoint,
                    &[
                        (id_key, song.id.clone()),
                        (quality_key, quality.into()),
                        ("type", "json".into()),
                    ],
                )
                .await?;
            let data = if matches!(self.platform, Platform::Netease) {
                &result["data"]
            } else {
                &result
            };
            let url = reqwest::Url::parse(&field(data, "url"))
                .map_err(|_| anyhow!("No playable URL at this quality; try another quality"))?;
            ensure!(
                matches!(url.scheme(), "http" | "https")
                    && url.username().is_empty()
                    && url.password().is_none(),
                "Unsupported music download URL"
            );
            let format = field(data, "format").to_ascii_lowercase();
            let extension = [format.as_str(), url.path().rsplit('.').next().unwrap_or("")]
                .into_iter()
                .find(|ext| ["mp3", "flac", "m4a", "aac", "ogg", "opus", "wav"].contains(ext))
                .unwrap_or(
                    if ["standard", "exhigh", "128k", "320k"].contains(&quality) {
                        "mp3"
                    } else {
                        "flac"
                    },
                )
                .to_owned();
            let preferred = |name: &str, fallback: &str| {
                let value = field(data, name);
                if value.trim().is_empty() {
                    fallback.to_owned()
                } else {
                    value
                }
            };
            Ok(ResolvedTrack {
                url,
                extension,
                metadata: crate::media::LocalMetadata {
                    title: preferred("name", &song.title),
                    artist: preferred(
                        if matches!(self.platform, Platform::Netease) {
                            "artist"
                        } else {
                            "singer"
                        },
                        &song.artist,
                    ),
                    album: preferred("album", &song.album),
                },
                cover: cover_url(&field(data, "picUrl"))
                    .or_else(|| cover_url(&field(data, "cover")))
                    .or_else(|| song.cover.clone()),
                lyrics: {
                    let text = field(data, "lrc");
                    (!text.trim().is_empty()).then_some(text)
                },
            })
        })
    }
    fn lyrics<'a>(&'a self, song: &'a Song) -> BoxFuture<'a, Result<Option<String>>> {
        Box::pin(async move {
            // QQ/Kugou detail responses already carry `lrc`; NetEase does not.
            if !matches!(self.platform, Platform::Netease) {
                return Ok(None);
            }
            let data = self
                .request("163_lyric", &[("id", song.id.clone())])
                .await?;
            let text = field(&data["data"], "lrc");
            Ok((!text.trim().is_empty()).then_some(text))
        })
    }
}
