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
            if message.starts_with("invalid QQ music")
                || message.starts_with("QQ music file is truncated")
            {
                return "QQ 音乐文件或密钥格式无效，文件可能已损坏。".into();
            }
            return message.into();
        }
        let mut output = message.to_owned();
        for (zh, en) in [
            (
                "请在设置或 .env 中配置 API Key",
                "Set an API key in Settings or .env",
            ),
            ("模型名称不能为空", "Model name cannot be empty"),
            ("无法读取 .env 配置", "Cannot read .env configuration"),
            (
                "无法解析 .env，请检查 KEY=VALUE 格式",
                "Cannot parse .env; use KEY=VALUE syntax",
            ),
            (
                "请在 .env 中设置 LLM_API_KEY 或 OPENAI_API_KEY",
                "Set LLM_API_KEY or OPENAI_API_KEY in .env",
            ),
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
                "无法连接字幕 API，请检查网络、TLS 和 LLM_BASE_URL",
                "Cannot connect to subtitle API; check network, TLS and LLM_BASE_URL",
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
            ("LLM_BASE_URL 格式无效", "Invalid LLM_BASE_URL"),
            (
                "LLM_AUDIO_ENCODING 必须是 auto、data_url 或 base64",
                "LLM_AUDIO_ENCODING must be auto, data_url or base64",
            ),
            (
                "LLM_SUBTITLE_CHUNK_SECONDS 必须是整数",
                "LLM_SUBTITLE_CHUNK_SECONDS must be an integer",
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

/// Empty fields inherit the environment. Credentials are session-only.
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct LlmSettings {
    pub base_url: String,
    pub model: String,
    #[serde(skip)]
    pub api_key: String,
    pub audio_encoding: String,
    pub audio_format: String,
    pub bilingual: Option<bool>,
    pub reasoning_effort: String,
    pub chunk_seconds: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub auto_check_updates: bool,
    pub language: Language,
    pub subtitle_concurrency: usize,
    pub llm: LlmSettings,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            auto_check_updates: true,
            language: Language::Chinese,
            subtitle_concurrency: 2,
            llm: LlmSettings::default(),
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
        value
    }
    pub fn save(&self) -> Result<()> {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let temporary = path.with_extension("json.tmp");
        std::fs::write(&temporary, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(temporary, path).context("save settings")
    }
    fn path() -> PathBuf {
        Self::root().join("settings.json")
    }
    pub fn queue_path() -> PathBuf {
        Self::root().join("queue.json")
    }
    fn root() -> PathBuf {
        if let Some(path) = std::env::var_os("REPLAYER_SETTINGS_FILE") {
            return PathBuf::from(path).with_extension("");
        }
        if let Some(root) =
            std::env::var_os("LOCALAPPDATA").or_else(|| std::env::var_os("XDG_CONFIG_HOME"))
        {
            return PathBuf::from(root).join("replayer");
        }
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| ".".into())
            .join(".config/replayer")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn language_and_parallelism_roundtrip_without_credentials() {
        let s = Settings {
            auto_check_updates: true,
            language: Language::English,
            subtitle_concurrency: 4,
            llm: LlmSettings {
                api_key: "session-secret".into(),
                model: "custom-model".into(),
                ..Default::default()
            },
        };
        let bytes = serde_json::to_vec(&s).unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("session-secret"));
        let loaded: Settings = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(loaded.language, Language::English);
        assert_eq!(loaded.subtitle_concurrency, 4);
        assert!(loaded.llm.api_key.is_empty());
        assert_eq!(loaded.llm.model, "custom-model");
        assert_eq!(
            serde_json::from_str::<Settings>("{}").unwrap().language,
            Language::Chinese
        );
    }
}
