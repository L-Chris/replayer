//! Explicit live-test fixtures. This module is never compiled into the client.
use anyhow::{Result, anyhow};
use std::path::PathBuf;

/// Fixture generation uses a full external FFmpeg, never the packaged runtime.
pub fn ffmpeg() -> PathBuf {
    if let Some(path) = std::env::var_os("REPLAYER_TEST_FFMPEG") {
        let path = PathBuf::from(path);
        assert!(
            path.is_file(),
            "REPLAYER_TEST_FFMPEG must name a full FFmpeg executable"
        );
        return path;
    }
    let runtime = std::env::var_os("FFMPEG_DIR").map(PathBuf::from);
    let name = if cfg!(windows) {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    };
    for directory in std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()) {
        let path = directory.join(name);
        if path.is_file() && !runtime.as_ref().is_some_and(|root| path.starts_with(root)) {
            return path;
        }
    }
    panic!(
        "Install full FFmpeg for fixture generation and set REPLAYER_TEST_FFMPEG to its executable"
    );
}

pub fn get(name: &str) -> Result<Option<String>> {
    if let Ok(value) = std::env::var(name)
        && !value.trim().is_empty()
    {
        return Ok(Some(value));
    }
    let file = Some(
        std::env::var_os("REPLAYER_ENV_FILE")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".env")),
    );
    let mut value = None;
    if let Some(path) = file {
        for entry in dotenvy::from_path_iter(path).map_err(|_| anyhow!("Cannot read .env"))? {
            let (key, item) = entry.map_err(|_| anyhow!("Cannot parse .env"))?;
            if key == name && !item.trim().is_empty() {
                value = Some(item);
            }
        }
    }
    Ok(value)
}

/// Test credentials are injected explicitly, never consulted by runtime resolvers.
pub fn settings() -> Result<crate::settings::Settings> {
    let mut settings = crate::settings::Settings::default();
    let get_any = |names: &[&str]| -> Result<String> {
        for name in names {
            if let Some(value) = get(name)? {
                return Ok(value);
            }
        }
        Ok(String::new())
    };
    let audio_model = get_any(&["LLM_MODEL", "OPENAI_MODEL"])?;
    settings.ai.providers.push(crate::ai::Provider {
        id: "test-audio".into(),
        name: "Test audio".into(),
        models: vec![audio_model.clone()],
        base_url: get_any(&["LLM_BASE_URL", "OPENAI_BASE_URL"])?,
        api_key: get_any(&["LLM_API_KEY", "OPENAI_API_KEY"])?,
    });
    settings.llm.selected = crate::ai::ModelRef {
        provider: "test-audio".into(),
        model: audio_model,
    };
    for (name, value) in [
        ("LLM_AUDIO_FORMAT", &mut settings.llm.audio_format),
        ("LLM_AUDIO_ENCODING", &mut settings.llm.audio_encoding),
        ("LLM_REASONING_EFFORT", &mut settings.llm.reasoning_effort),
        (
            "LLM_SUBTITLE_CHUNK_SECONDS",
            &mut settings.llm.chunk_seconds,
        ),
    ] {
        if let Some(configured) = get(name)? {
            *value = configured;
        }
    }
    if let Some(value) = get("LLM_SUBTITLE_BILINGUAL")? {
        settings.llm.bilingual = Some(match value.as_str() {
            "true" | "1" => true,
            "false" | "0" => false,
            _ => anyhow::bail!("Invalid test bilingual option"),
        });
    }
    let image_model = get_any(&["IMAGE_MODEL"])?;
    settings.ai.providers.push(crate::ai::Provider {
        id: "test-images".into(),
        name: "Test images".into(),
        models: vec![image_model.clone()],
        base_url: get_any(&["IMAGE_BASE_URL"])?,
        api_key: get_any(&["IMAGE_API_KEY"])?,
    });
    settings.comics.image_selected = crate::ai::ModelRef {
        provider: "test-images".into(),
        model: image_model,
    };
    settings.music.api_key = get_any(&["CHKSZ_API_KEY"])?;
    Ok(settings)
}
