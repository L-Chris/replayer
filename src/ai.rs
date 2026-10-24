//! Shared AI connections. Business settings reference a provider/model pair.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ModelRef {
    pub provider: String,
    pub model: String,
}

#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Settings {
    pub providers: Vec<Provider>,
}

// Never derive Debug: the in-memory connection contains a credential.
#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Provider {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub models: Vec<String>,
    #[serde(skip)]
    pub api_key: String,
}

impl Provider {
    pub fn new() -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            ..Default::default()
        }
    }
    pub fn same_connection(&self, other: &Self) -> bool {
        self.id == other.id && self.base_url == other.base_url && self.api_key == other.api_key
    }
}

pub struct Connection {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
}

pub fn endpoint(base: &str) -> Result<reqwest::Url> {
    let url = reqwest::Url::parse(base.trim())
        .map_err(|_| anyhow::anyhow!("服务地址无效 / Invalid service endpoint"))?;
    ensure!(
        matches!(url.scheme(), "https" | "http")
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none(),
        "服务地址必须是 HTTP(S) 根地址 / Use an HTTP(S) API base URL"
    );
    Ok(url)
}

impl Settings {
    pub fn resolve(&self, selected: &ModelRef) -> Result<Connection> {
        ensure!(
            !selected.provider.is_empty() && !selected.model.is_empty(),
            "请在设置中选择 AI 模型 / Select an AI model in Settings"
        );
        let provider = self
            .providers
            .iter()
            .find(|p| p.id == selected.provider)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "AI 提供商已移除，请重新选择 / AI provider was removed; select another"
                )
            })?;
        ensure!(
            provider.models.contains(&selected.model),
            "AI 模型已移除，请重新选择 / AI model was removed; select another"
        );
        endpoint(&provider.base_url)?;
        ensure!(
            !provider.api_key.trim().is_empty(),
            "请在 AI 提供商设置中填写密钥 / Set an API key in AI providers"
        );
        Ok(Connection {
            base_url: provider.base_url.trim().trim_end_matches('/').into(),
            api_key: provider.api_key.trim().into(),
            model: selected.model.clone(),
        })
    }
    pub fn validate(&self) -> Result<()> {
        let mut ids = std::collections::HashSet::new();
        for provider in &self.providers {
            ensure!(
                !provider.id.is_empty() && ids.insert(&provider.id),
                "提供商 ID 无效 / Invalid provider ID"
            );
            ensure!(
                !provider.name.trim().is_empty(),
                "请填写提供商名称 / Enter a provider name"
            );
            if !provider.base_url.trim().is_empty() {
                endpoint(&provider.base_url)?;
            }
            ensure!(
                provider.models.iter().all(|m| !m.trim().is_empty()),
                "模型 ID 不能为空 / Model ID cannot be empty"
            );
        }
        Ok(())
    }
}

/// No automatic probes: called only by the provider's Fetch models action.
pub async fn fetch_models(provider: &Provider) -> Result<Vec<String>> {
    endpoint(&provider.base_url)?;
    let url = format!("{}/models", provider.base_url.trim().trim_end_matches('/'));
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let response = http
        .get(url)
        .bearer_auth(provider.api_key.trim())
        .send()
        .await
        .map_err(|_| {
            anyhow::anyhow!("获取模型失败，请检查网络 / Cannot fetch models; check the connection")
        })?;
    ensure!(
        response.status().is_success(),
        "获取模型失败 HTTP {} / Cannot fetch models: HTTP {}",
        response.status().as_u16(),
        response.status().as_u16()
    );
    use futures_util::StreamExt;
    let mut body = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk
            .map_err(|_| anyhow::anyhow!("模型列表响应中断 / Model list response interrupted"))?;
        ensure!(
            body.len() + chunk.len() <= 2 * 1024 * 1024,
            "模型列表过大 / Model list too large"
        );
        body.extend_from_slice(&chunk);
    }
    let value: serde_json::Value = serde_json::from_slice(&body)
        .map_err(|_| anyhow::anyhow!("模型列表格式无效 / Invalid model list"))?;
    let data = value["data"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("模型列表格式无效 / Invalid model list"))?;
    let mut models: Vec<String> = data
        .iter()
        .filter_map(|m| m["id"].as_str())
        .filter(|m| !m.trim().is_empty())
        .map(str::to_owned)
        .collect();
    models.sort();
    models.dedup();
    Ok(models)
}

#[cfg(test)]
mod tests;
