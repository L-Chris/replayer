use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Language {
    #[default]
    #[serde(rename = "zh-CN")]
    Chinese,
    #[serde(rename = "en-US")]
    English,
}
impl Language {
    pub fn text(self, chinese: &'static str, english: &'static str) -> &'static str {
        match self {
            Self::Chinese => chinese,
            Self::English => english,
        }
    }
    pub fn target(self) -> &'static str {
        self.text("Simplified Chinese (zh-CN)", "English (en-US)")
    }
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "zh" | "zh-CN" => Ok(Self::Chinese),
            "en" | "en-US" => Ok(Self::English),
            _ => anyhow::bail!("language must be zh-CN or en-US"),
        }
    }
    pub fn error(self, message: &str) -> String {
        if let Some((chinese, english)) = message.split_once(" / ") {
            return match self {
                Self::Chinese => chinese,
                Self::English => english,
            }
            .into();
        }
        if self == Self::English
            && message == "字幕 API 返回 HTTP 400: 请求无效，已跳过该片段（不重试）"
        {
            return "Subtitle API returned HTTP 400: invalid request; skipped this segment without retrying".into();
        }
        if self == Self::Chinese {
            if message.starts_with("QQ music needs a song key") {
                return "该 QQ 音乐文件没有内嵌密钥，请导入这首歌对应的 ekey。".into();
            }
            if message.starts_with("QQ music EncV2") {
                return "暂不支持此 EncV2 密钥封装版本。".into();
            }
            if message.starts_with("QQ music key is incorrect") {
                return "歌曲密钥不匹配，或此加密变体尚不支持。".into();
            }
            if message.starts_with("unsupported QQ music") {
                return "暂不支持此 QQ 音乐文件结构或加密版本。".into();
            }
            if message.starts_with("Magnet metadata timed out") {
                return "磁链元数据解析超时：没有可连通的 peer。稍后重试；若使用代理，请在磁链弹窗复制并应用路由规则后重试。".into();
            }
            if message.starts_with("audio device failed") {
                return "音频设备失效，将继续无声播放".into();
            }
            if message.starts_with("audio device lost") {
                return "音频设备已断开，正在等待设备恢复…".into();
            }
            if message.starts_with("Audio output device failed") {
                return "音频输出设备失效；请重新连接设备后重新打开该曲目".into();
            }
            if message.starts_with("invalid QQ music")
                || message.starts_with("QQ music file is truncated")
            {
                return "QQ 音乐文件或密钥格式无效，文件可能已损坏。".into();
            }
            return message.into();
        }
        let mut output = message.to_owned();
        for (zh, en) in [
            ("模型名称不能为空", "Model name cannot be empty"),
            ("字幕生成已取消", "Subtitle generation cancelled"),
            (
                "当前文件没有音轨，无法生成字幕",
                "This file has no audio track",
            ),
            ("字幕音频重采样失败", "Audio resampling failed"),
            ("读取字幕源文件失败", "Cannot open subtitle source"),
            ("字幕音轨解码失败", "Audio decoding failed"),
            (
                "读取字幕音轨失败或已取消",
                "Audio read failed or was cancelled",
            ),
            (
                "字幕 API 请求超时（120 秒），可取消后重试",
                "Subtitle API timed out (120 seconds); retry later",
            ),
            (
                "无法连接字幕 API，请检查网络、TLS 和 服务地址",
                "Cannot connect to subtitle API; check network, TLS and endpoint",
            ),
            (
                "字幕 API 连接中断或响应读取失败",
                "Subtitle API connection interrupted",
            ),
            ("字幕 API 返回 HTTP", "Subtitle API returned HTTP"),
            (
                "请检查接口地址、模型名称、余额和音频输入支持",
                "Check endpoint, model, credit and audio-input support",
            ),
            (
                "模型没有返回有效的字幕 JSON，请重试或更换支持音频的模型",
                "Model did not return valid subtitle JSON; retry or use an audio-capable model",
            ),
            (
                "模型返回的字幕时间戳超出音频片段范围",
                "Model timestamps exceed the audio chunk duration",
            ),
            (
                "模型返回的字幕文本为空或过长",
                "Model subtitle text is empty or too long",
            ),
            (
                "字幕 API 流未完整结束或没有返回文本",
                "Subtitle API stream ended early or returned no text",
            ),
            ("模型提前结束生成", "Model stopped generation early"),
            ("字幕 API 错误", "Subtitle API error"),
            ("字幕 API 没有返回文本", "Subtitle API returned no text"),
            (
                "字幕 JSON 代码块不完整",
                "Incomplete subtitle JSON code block",
            ),
            ("模型返回了过多字幕片段", "Model returned too many segments"),
            ("字幕 API 返回了无效 SSE 数据", "Invalid SSE response"),
            ("字幕 API 返回了无效 UTF-8", "Invalid UTF-8 response"),
            (
                "字幕 API 响应超过 2 MiB 上限",
                "Subtitle API response exceeds 2 MiB",
            ),
            (
                "字幕音频分段长度必须在 5..120 秒之间",
                "Chunk duration must be between 5 and 120 seconds",
            ),
        ] {
            output = output.replace(zh, en);
        }
        output
    }
}

/// Business options reference a shared AI model. Legacy connection fields only migrate old settings.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct LlmSettings {
    pub selected: crate::ai::ModelRef,
    #[serde(skip_serializing)]
    pub base_url: String,
    #[serde(skip_serializing)]
    pub model: String,
    #[serde(skip)]
    pub api_key: String,
    pub audio_encoding: String,
    pub audio_format: String,
    pub bilingual: Option<bool>,
    pub reasoning_effort: String,
    pub chunk_seconds: String,
}
impl Default for LlmSettings {
    fn default() -> Self {
        Self {
            selected: Default::default(),
            base_url: String::new(),
            model: String::new(),
            api_key: String::new(),
            audio_encoding: "auto".into(),
            audio_format: "mp3".into(),
            bilingual: Some(false),
            reasoning_effort: "default".into(),
            chunk_seconds: "120".into(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Settings {
    pub ai: crate::ai::Settings,
    pub sync: crate::sync::Settings,
    #[serde(skip)]
    pub credential_error: Option<String>,
    pub auto_check_updates: bool,
    pub language: Language,
    pub subtitle_concurrency: usize,
    pub llm: LlmSettings,
    pub music: crate::music_sources::Settings,
    pub comics: crate::comics::Settings,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            ai: Default::default(),
            sync: Default::default(),
            credential_error: None,
            auto_check_updates: true,
            language: Language::Chinese,
            subtitle_concurrency: 2,
            llm: LlmSettings::default(),
            music: Default::default(),
            comics: Default::default(),
        }
    }
}
impl Settings {
    pub fn load() -> Self {
        let mut value: Self = std::fs::read(Self::path())
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        value.subtitle_concurrency = value.subtitle_concurrency.clamp(1, 6);
        value.migrate();
        if let Err(error) = value.load_credentials(&crate::credentials::SystemVault) {
            value.credential_error = Some(error.to_string());
        }
        value
    }
    pub fn save(&self) -> Result<()> {
        self.save_with(&crate::credentials::SystemVault, &Self::path())
    }
    fn secret_target(path: &std::path::Path, id: &str) -> String {
        format!(
            // Persisted OS credential targets are independent of the display brand.
            "replayer/{}/{}",
            crate::comics::digest(path.to_string_lossy().as_bytes()),
            id
        )
    }
    pub fn load_credentials(&mut self, vault: &impl crate::credentials::Vault) -> Result<()> {
        let path = Self::path();
        self.load_credentials_from(vault, &path)
    }
    fn load_credentials_from(
        &mut self,
        vault: &impl crate::credentials::Vault,
        path: &std::path::Path,
    ) -> Result<()> {
        for provider in &mut self.ai.providers {
            provider.api_key = vault
                .get(&Self::secret_target(path, &format!("ai/{}", provider.id)))?
                .unwrap_or_default();
        }
        self.music.api_key = vault
            .get(&Self::secret_target(path, "music/chksz"))?
            .unwrap_or_default();
        self.comics.service.token = vault
            .get(&Self::secret_target(
                path,
                &format!(
                    "comic/service/{}/token",
                    self.comics.service.credential_id()
                ),
            ))?
            .unwrap_or_default();
        self.credential_error = None;
        self.sync.password = vault
            .get(&Self::secret_target(
                path,
                &format!("sync/{}/password", self.sync.account()),
            ))?
            .unwrap_or_default();
        self.sync.passphrase = vault
            .get(&Self::secret_target(
                path,
                &format!("sync/{}/passphrase", self.sync.account()),
            ))?
            .unwrap_or_default();
        Ok(())
    }
    fn save_with(
        &self,
        vault: &impl crate::credentials::Vault,
        path: &std::path::Path,
    ) -> Result<()> {
        anyhow::ensure!(
            self.credential_error.is_none(),
            "无法读取密钥，请重新加载后保存 / Reload credentials before saving"
        );
        self.ai.validate()?;
        self.comics.service.validate()?;
        if self.sync.enabled {
            self.sync.validate()?;
        }
        let mut changes = vec![(
            Self::secret_target(path, "music/chksz"),
            self.music.api_key.clone(),
        )];
        changes.push((
            Self::secret_target(
                path,
                &format!(
                    "comic/service/{}/token",
                    self.comics.service.credential_id()
                ),
            ),
            self.comics.service.token.clone(),
        ));
        for provider in &self.ai.providers {
            // AI keys remain in the credential vault even when cloud sync is enabled.
            changes.push((
                Self::secret_target(path, &format!("ai/{}", provider.id)),
                provider.api_key.clone(),
            ));
        }
        if let Ok(bytes) = std::fs::read(path)
            && let Ok(old) = serde_json::from_slice::<Self>(&bytes)
        {
            if old.comics.service.credential_id() != self.comics.service.credential_id() {
                changes.push((
                    Self::secret_target(
                        path,
                        &format!("comic/service/{}/token", old.comics.service.credential_id()),
                    ),
                    String::new(),
                ));
            }
            for provider in old.ai.providers {
                if !self.ai.providers.iter().any(|p| p.id == provider.id) {
                    changes.push((
                        Self::secret_target(path, &format!("ai/{}", provider.id)),
                        String::new(),
                    ));
                }
            }
        }
        changes.push((
            Self::secret_target(path, &format!("sync/{}/password", self.sync.account())),
            self.sync.password.clone(),
        ));
        changes.push((
            Self::secret_target(path, &format!("sync/{}/passphrase", self.sync.account())),
            self.sync.passphrase.clone(),
        ));
        crate::credentials::transaction(vault, &changes, || self.write_json(path))
    }
    fn write_json(&self, path: &std::path::Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let temporary = path.with_extension("json.tmp");
        std::fs::write(&temporary, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(temporary, path).context("save settings")
    }
    fn migrate(&mut self) {
        // An empty old field is not a reason to search the environment.
        let mut add =
            |id: &str, name: &str, base: &str, model: &str, selected: &mut crate::ai::ModelRef| {
                if selected.provider.is_empty() && (!base.is_empty() || !model.is_empty()) {
                    if !self.ai.providers.iter().any(|p| p.id == id) {
                        self.ai.providers.push(crate::ai::Provider {
                            id: id.into(),
                            name: name.into(),
                            base_url: base.into(),
                            models: if model.is_empty() {
                                vec![]
                            } else {
                                vec![model.into()]
                            },
                            api_key: String::new(),
                        });
                    }
                    *selected = crate::ai::ModelRef {
                        provider: id.into(),
                        model: model.into(),
                    };
                }
            };
        add(
            "legacy-subtitles",
            "字幕服务",
            &self.llm.base_url,
            &self.llm.model,
            &mut self.llm.selected,
        );
        let image_base = if self.comics.image_base_url.is_empty() {
            &self.llm.base_url
        } else {
            &self.comics.image_base_url
        };
        if !self.comics.image_base_url.is_empty() || !self.comics.image_model.is_empty() {
            add(
                "legacy-images",
                "图像服务",
                image_base,
                &self.comics.image_model,
                &mut self.comics.image_selected,
            );
        }
        self.llm.base_url.clear();
        self.llm.model.clear();
        self.llm.api_key.clear();
        self.comics.image_base_url.clear();
        self.comics.image_model.clear();
        self.comics.image_key.clear();
        if self.llm.audio_encoding.is_empty() {
            self.llm.audio_encoding = "auto".into();
        }
        if self.llm.audio_format.is_empty() {
            self.llm.audio_format = "mp3".into();
        }
        if self.llm.chunk_seconds.is_empty() {
            self.llm.chunk_seconds = "120".into();
        }
        if self.llm.reasoning_effort.is_empty() {
            self.llm.reasoning_effort = "default".into();
        }
    }
    fn path() -> PathBuf {
        Self::root().join("settings.json")
    }
    pub fn queue_path() -> PathBuf {
        Self::root().join("queue.json")
    }
    fn root() -> PathBuf {
        pordo_core::paths::root()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn old_settings_migrate_once_without_reusing_business_connections() {
        let mut settings: Settings = serde_json::from_str(r#"{"llm":{"base_url":"https://audio.example/v1","model":"audio","audio_format":""},"comics":{"image_model":"edit"}}"#).unwrap();
        settings.migrate();
        assert_eq!(settings.ai.providers.len(), 2);
        assert_ne!(
            settings.llm.selected.provider,
            settings.comics.image_selected.provider
        );
        assert_eq!(settings.llm.selected.model, "audio");
        assert_eq!(settings.comics.image_selected.model, "edit");
        assert_eq!(settings.llm.audio_format, "mp3");
        settings.migrate();
        assert_eq!(settings.ai.providers.len(), 2);
        assert!(settings.llm.base_url.is_empty() && settings.comics.image_model.is_empty());
        let mut fresh = Settings::default();
        fresh.migrate();
        assert!(fresh.ai.providers.is_empty());
    }

    #[test]
    fn saved_settings_restore_keys_separately_and_remove_deleted_provider_keys() {
        let vault = crate::credentials::tests::MemoryVault::default();
        let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join(format!("settings-test-{}", uuid::Uuid::new_v4()));
        let path = directory.join("settings.json");
        let mut settings = Settings::default();
        settings.ai.providers.push(crate::ai::Provider {
            id: "my-ai".into(),
            name: "AI".into(),
            base_url: "https://example.com/v1".into(),
            models: vec!["audio".into()],
            api_key: "ai-test-key".into(),
        });
        settings.music.api_key = "music-test-key".into();
        settings.comics.service = crate::comics::source::ServiceSettings {
            url: "https://content.example.test".into(),
            token: "device-test-key".into(),
        };
        settings.sync.password = "webdav-test-key".into();
        settings.sync.passphrase = "passphrase-test-key".into();
        settings.save_with(&vault, &path).unwrap();
        let json = std::fs::read_to_string(&path).unwrap();
        assert!(!json.contains("test-key"));
        let mut loaded: Settings = serde_json::from_str(&json).unwrap();
        loaded.load_credentials_from(&vault, &path).unwrap();
        assert!(loaded.ai.providers[0].api_key == "ai-test-key");
        assert!(loaded.music.api_key == "music-test-key");
        assert!(loaded.comics.service.token == "device-test-key");
        assert!(loaded.sync.password == "webdav-test-key");
        assert!(loaded.sync.passphrase == "passphrase-test-key");
        let old_target = Settings::secret_target(
            &path,
            &format!(
                "comic/service/{}/token",
                loaded.comics.service.credential_id()
            ),
        );
        loaded.comics.service.url = "https://another.example.test".into();
        loaded.save_with(&vault, &path).unwrap();
        assert!(!vault.values.borrow().contains_key(&old_target));
        loaded.ai.providers.clear();
        loaded.music.api_key.clear();
        loaded.comics.service.token.clear();
        loaded.sync.password.clear();
        loaded.sync.passphrase.clear();
        loaded.save_with(&vault, &path).unwrap();
        assert!(vault.values.borrow().is_empty());
        let before = std::fs::read(&path).unwrap();
        loaded.credential_error = Some("Vault is unavailable".into());
        loaded.music.api_key = "must-not-be-saved".into();
        assert!(loaded.save_with(&vault, &path).is_err());
        assert!(vault.values.borrow().is_empty());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert!(
            directory.starts_with(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target"))
        );
        std::fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn language_and_parallelism_roundtrip_without_credentials() {
        let s = Settings {
            sync: crate::sync::Settings {
                password: "webdav-session-secret".into(),
                passphrase: "sync-session-secret".into(),
                ..Default::default()
            },
            comics: crate::comics::Settings {
                image_key: "image-session-secret".into(),
                ..Default::default()
            },
            music: crate::music_sources::Settings {
                api_key: "music-session-secret".into(),
                ..Default::default()
            },
            auto_check_updates: true,
            language: Language::English,
            subtitle_concurrency: 4,
            llm: LlmSettings {
                api_key: "session-secret".into(),
                model: "custom-model".into(),
                ..Default::default()
            },
            ..Default::default()
        };
        let bytes = serde_json::to_vec(&s).unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("session-secret"));
        let loaded: Settings = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(loaded.language, Language::English);
        assert_eq!(loaded.subtitle_concurrency, 4);
        assert!(loaded.llm.api_key.is_empty());
        assert!(loaded.llm.model.is_empty());
        assert_eq!(
            serde_json::from_str::<Settings>("{}").unwrap().language,
            Language::Chinese
        );
    }
}
