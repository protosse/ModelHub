use anyhow::{Context, Result};
use serde_json::Value;

use crate::store::{find_provider, Protocol, RemoteModel, Secrets, Store};

pub async fn fetch_remote_models(
    store: &Store,
    secrets: &Secrets,
    provider_id: &str,
) -> Result<Vec<RemoteModel>> {
    let provider = find_provider(store, provider_id).context("provider not found")?;
    let api_key = secrets
        .secrets
        .get(&provider.secret_ref)
        .map(|s| s.api_key.as_str())
        .unwrap_or("");

    let base = provider.base_url.trim_end_matches('/');
    let url = if base.ends_with("/v1") {
        format!("{base}/models")
    } else {
        format!("{base}/v1/models")
    };

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()?;

    let mut req = client.get(&url);
    if !api_key.is_empty() {
        req = match provider.protocol {
            Protocol::AnthropicMessages => req
                .header("x-api-key", api_key)
                .header("anthropic-version", "2023-06-01")
                .header("Authorization", format!("Bearer {api_key}")),
            _ => req.header("Authorization", format!("Bearer {api_key}")),
        };
    }
    for (k, v) in &provider.headers {
        req = req.header(k, v);
    }

    let resp = req.send().await.with_context(|| format!("request {url}"))?;
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        anyhow::bail!(
            "HTTP {status}: {}",
            body.chars().take(200).collect::<String>()
        );
    }

    let json: Value = serde_json::from_str(&body).context("parse models response")?;
    let mut out = Vec::new();

    if let Some(arr) = json.get("data").and_then(|v| v.as_array()) {
        for item in arr {
            let id = item
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if id.is_empty() {
                continue;
            }
            let name = item
                .get("name")
                .or_else(|| item.get("display_name"))
                .and_then(|v| v.as_str())
                .unwrap_or(&id)
                .to_string();
            out.push(RemoteModel { id, name });
        }
    } else if let Some(arr) = json.as_array() {
        for item in arr {
            if let Some(id) = item.as_str() {
                out.push(RemoteModel {
                    id: id.to_string(),
                    name: id.to_string(),
                });
            } else if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
                let name = item
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or(id)
                    .to_string();
                out.push(RemoteModel {
                    id: id.to_string(),
                    name,
                });
            }
        }
    } else if let Some(obj) = json.get("models").and_then(|v| v.as_object()) {
        for (id, meta) in obj {
            let name = meta
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or(id)
                .to_string();
            out.push(RemoteModel {
                id: id.clone(),
                name,
            });
        }
    }

    out.sort_by(|a, b| a.id.cmp(&b.id));
    out.dedup_by(|a, b| a.id == b.id);
    Ok(out)
}
