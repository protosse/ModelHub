use anyhow::{Context, Result};
use serde::Serialize;
use serde_json::{json, Value};
use std::time::Instant;
use tauri::{AppHandle, Emitter};

use crate::store::{
    find_model, find_provider, mask_key, normalize_base_url, Protocol, Secrets, Store,
    TestConnectionResult,
};

const BODY_TRUNCATE: usize = 8000;
const LOG_BODY_TRUNCATE: usize = 4000;
const MAX_TOKENS: u32 = 64;
const DEFAULT_TIMEOUT_SECS: u64 = 30;
const MIN_TIMEOUT_SECS: u64 = 5;
const MAX_TIMEOUT_SECS: u64 = 300;

fn clamp_timeout_secs(v: Option<u64>) -> u64 {
    let n = v.unwrap_or(DEFAULT_TIMEOUT_SECS);
    n.clamp(MIN_TIMEOUT_SECS, MAX_TIMEOUT_SECS)
}

/// Frontend listens on this event for live connection-test logs.
pub const TEST_CONNECTION_LOG_EVENT: &str = "test-connection-log";

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct LogEvent {
    run_id: String,
    line: String,
}

struct LogSink {
    app: Option<AppHandle>,
    run_id: String,
    lines: Vec<String>,
}

impl LogSink {
    fn new(app: Option<AppHandle>, run_id: String) -> Self {
        Self {
            app,
            run_id,
            lines: Vec::new(),
        }
    }

    fn push(&mut self, line: impl Into<String>) {
        let line = line.into();
        self.lines.push(line.clone());
        if let Some(app) = &self.app {
            let _ = app.emit(
                TEST_CONNECTION_LOG_EVENT,
                LogEvent {
                    run_id: self.run_id.clone(),
                    line,
                },
            );
        }
    }

    fn into_lines(self) -> Vec<String> {
        self.lines
    }
}

pub async fn test_model_connection(
    app: Option<AppHandle>,
    run_id: &str,
    store: &Store,
    secrets: &Secrets,
    model_row_id: &str,
    prompt: &str,
    timeout_secs: Option<u64>,
    extra_headers: Option<&std::collections::HashMap<String, String>>,
) -> Result<TestConnectionResult> {
    let mut log = LogSink::new(app, run_id.to_string());
    let prompt = prompt.trim();
    if prompt.is_empty() {
        anyhow::bail!("提示词不能为空");
    }
    let timeout_secs = clamp_timeout_secs(timeout_secs);

    log.push(format!("run_id={run_id}"));
    log.push(format!("start connection test timeout={timeout_secs}s"));

    let model = find_model(store, model_row_id).context("model not found")?;
    let provider = find_provider(store, &model.provider_id).context("provider not found")?;
    log.push(format!(
        "resolve model row={} upstream={} provider={} protocol={}",
        model.id,
        model.model_id,
        provider.name,
        provider.protocol.as_str()
    ));
    log.push(format!("base_url={}", provider.base_url));

    let api_key = secrets
        .secrets
        .get(&provider.secret_ref)
        .map(|s| s.api_key.as_str())
        .unwrap_or("");
    if api_key.is_empty() {
        anyhow::bail!("该提供商未配置 API Key");
    }
    log.push(format!(
        "auth secret_ref={} key_mask={}",
        provider.secret_ref,
        mask_key(api_key)
    ));

    let base = normalize_base_url(&provider.base_url);
    let (url, body) = build_request(&base, &provider.protocol, &model.model_id, prompt)?;
    let request_body = serde_json::to_string_pretty(&body).unwrap_or_else(|_| body.to_string());

    // Merge order: provider.headers, then per-run extra_headers (same key overwrites).
    let mut merged_headers = provider.headers.clone();
    if let Some(extra) = extra_headers {
        for (k, v) in extra {
            let key = k.trim();
            if key.is_empty() {
                continue;
            }
            merged_headers.insert(key.to_string(), v.clone());
        }
    }
    let request_headers =
        build_request_header_log(&provider.protocol, api_key, &merged_headers);

    log.push(format!(
        "timeout={}s token_limit={}",
        timeout_secs,
        match provider.protocol {
            // Completions/Anthropic send max_tokens; Responses omits max_output_tokens
            // for broader third-party gateway compatibility.
            Protocol::OpenaiResponses => "none (responses)".to_string(),
            _ => MAX_TOKENS.to_string(),
        }
    ));
    if !merged_headers.is_empty() {
        log.push(format!(
            "extra/provider headers: {}",
            merged_headers.len()
        ));
    }
    log.push(format!("POST {url}"));
    for h in &request_headers {
        log.push(format!("req header: {h}"));
    }
    log.push(format!(
        "req body ({} chars):\n{}",
        request_body.chars().count(),
        truncate(&request_body, LOG_BODY_TRUNCATE)
    ));

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .build()?;

    let mut req = client.post(&url).json(&body);
    req = apply_auth(req, &provider.protocol, api_key);
    for (k, v) in &merged_headers {
        req = req.header(k, v);
    }

    let started = Instant::now();
    log.push("sending request…");
    let resp = match req.send().await {
        Ok(r) => r,
        Err(e) => {
            let latency_ms = started.elapsed().as_millis() as u64;
            log.push(format!("transport error after {latency_ms}ms: {e}"));
            if e.is_timeout() {
                log.push("hint: request timed out — check network, baseUrl, or proxy");
            }
            if e.is_connect() {
                log.push("hint: connection failed — DNS / TLS / host unreachable");
            }
            return Ok(TestConnectionResult {
                ok: false,
                latency_ms,
                http_status: None,
                protocol: provider.protocol.clone(),
                request_url: url,
                response_text: None,
                error: Some(format!("请求失败：{e}")),
                logs: log.into_lines(),
                request_method: "POST".into(),
                request_headers,
                request_body: Some(truncate(&request_body, BODY_TRUNCATE)),
                response_headers: vec![],
                response_body: None,
            });
        }
    };

    let latency_ms = started.elapsed().as_millis() as u64;
    let status = resp.status();
    let status_code = status.as_u16();
    let response_headers = resp
        .headers()
        .iter()
        .map(|(k, v)| {
            let val = v.to_str().unwrap_or("<binary>");
            format!("{k}: {val}")
        })
        .collect::<Vec<_>>();

    log.push(format!("response status={status_code} latency={latency_ms}ms"));
    for h in &response_headers {
        log.push(format!("resp header: {h}"));
    }

    log.push("reading response body…");
    let raw = resp.text().await.unwrap_or_default();
    let raw_chars = raw.chars().count();
    log.push(format!("resp body length={raw_chars} chars"));
    let response_body = truncate(&raw, BODY_TRUNCATE);
    log.push(format!(
        "resp body:\n{}",
        truncate(&raw, LOG_BODY_TRUNCATE)
    ));

    if !status.is_success() {
        let err_snip: String = raw.chars().take(300).collect();
        log.push(format!("failed: non-2xx HTTP {status_code}"));
        return Ok(TestConnectionResult {
            ok: false,
            latency_ms,
            http_status: Some(status_code),
            protocol: provider.protocol.clone(),
            request_url: url,
            response_text: Some(truncate(&raw, BODY_TRUNCATE)),
            error: Some(format!("HTTP {status_code}: {err_snip}")),
            logs: log.into_lines(),
            request_method: "POST".into(),
            request_headers,
            request_body: Some(truncate(&request_body, BODY_TRUNCATE)),
            response_headers,
            response_body: Some(response_body),
        });
    }

    let parsed = extract_assistant_text(&provider.protocol, &raw);
    let text = match parsed {
        Some(t) => {
            log.push(format!(
                "parsed assistant text ({} chars)",
                t.chars().count()
            ));
            t
        }
        None => {
            log.push("warn: could not parse assistant text; showing raw body snippet");
            truncate(&raw, BODY_TRUNCATE)
        }
    };

    log.push("ok");
    Ok(TestConnectionResult {
        ok: true,
        latency_ms,
        http_status: Some(status_code),
        protocol: provider.protocol.clone(),
        request_url: url,
        response_text: Some(truncate(&text, BODY_TRUNCATE)),
        error: None,
        logs: log.into_lines(),
        request_method: "POST".into(),
        request_headers,
        request_body: Some(truncate(&request_body, BODY_TRUNCATE)),
        response_headers,
        response_body: Some(response_body),
    })
}

fn build_request(
    base: &str,
    protocol: &Protocol,
    upstream_model_id: &str,
    prompt: &str,
) -> Result<(String, Value)> {
    let url = match protocol {
        Protocol::OpenaiCompletions => {
            format!("{}/chat/completions", api_root(base))
        }
        Protocol::OpenaiResponses => format!("{}/responses", api_root(base)),
        Protocol::AnthropicMessages => format!("{}/messages", api_root(base)),
    };

    let body = match protocol {
        Protocol::OpenaiCompletions => json!({
            "model": upstream_model_id,
            "messages": [{ "role": "user", "content": prompt }],
            "max_tokens": MAX_TOKENS,
            "temperature": 0,
        }),
        // Omit max_output_tokens: official OpenAI accepts it, but many third-party
        // OpenAI-compatible /responses gateways reject it (HTTP 400 Unsupported parameter).
        // Connectivity tests only need a minimal valid body.
        Protocol::OpenaiResponses => json!({
            "model": upstream_model_id,
            "input": prompt,
        }),
        Protocol::AnthropicMessages => json!({
            "model": upstream_model_id,
            "max_tokens": MAX_TOKENS,
            "messages": [{ "role": "user", "content": prompt }],
        }),
    };

    Ok((url, body))
}

/// Prefer `{base}/v1/...` unless base already ends with `/v1`.
fn api_root(base: &str) -> String {
    let b = base.trim_end_matches('/');
    if b.ends_with("/v1") {
        b.to_string()
    } else {
        format!("{b}/v1")
    }
}

fn apply_auth(
    req: reqwest::RequestBuilder,
    protocol: &Protocol,
    api_key: &str,
) -> reqwest::RequestBuilder {
    match protocol {
        Protocol::AnthropicMessages => req
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01")
            .header("Authorization", format!("Bearer {api_key}"))
            .header("Content-Type", "application/json"),
        _ => req
            .header("Authorization", format!("Bearer {api_key}"))
            .header("Content-Type", "application/json"),
    }
}

fn build_request_header_log(
    protocol: &Protocol,
    api_key: &str,
    extra: &std::collections::HashMap<String, String>,
) -> Vec<String> {
    let mask = mask_key(api_key);
    let mut headers = match protocol {
        Protocol::AnthropicMessages => vec![
            format!("x-api-key: {mask}"),
            "anthropic-version: 2023-06-01".into(),
            format!("Authorization: Bearer {mask}"),
            "Content-Type: application/json".into(),
        ],
        _ => vec![
            format!("Authorization: Bearer {mask}"),
            "Content-Type: application/json".into(),
        ],
    };
    for (k, v) in extra {
        let lower = k.to_ascii_lowercase();
        let val = if lower.contains("auth") || lower.contains("key") || lower.contains("token") {
            mask_key(v)
        } else {
            v.clone()
        };
        headers.push(format!("{k}: {val}"));
    }
    headers
}

fn extract_assistant_text(protocol: &Protocol, raw: &str) -> Option<String> {
    let json: Value = serde_json::from_str(raw).ok()?;
    match protocol {
        Protocol::OpenaiCompletions => {
            if let Some(s) = json
                .pointer("/choices/0/message/content")
                .and_then(|v| v.as_str())
            {
                return Some(s.to_string());
            }
            if let Some(arr) = json
                .pointer("/choices/0/message/content")
                .and_then(|v| v.as_array())
            {
                let joined = arr
                    .iter()
                    .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
                    .collect::<Vec<_>>()
                    .join("");
                if !joined.is_empty() {
                    return Some(joined);
                }
            }
            None
        }
        Protocol::OpenaiResponses => {
            if let Some(s) = json.get("output_text").and_then(|v| v.as_str()) {
                return Some(s.to_string());
            }
            if let Some(output) = json.get("output").and_then(|v| v.as_array()) {
                let mut parts = Vec::new();
                for item in output {
                    if let Some(content) = item.get("content").and_then(|v| v.as_array()) {
                        for c in content {
                            if let Some(t) = c.get("text").and_then(|v| v.as_str()) {
                                parts.push(t.to_string());
                            }
                        }
                    }
                }
                if !parts.is_empty() {
                    return Some(parts.join(""));
                }
            }
            None
        }
        Protocol::AnthropicMessages => {
            if let Some(content) = json.get("content").and_then(|v| v.as_array()) {
                let joined = content
                    .iter()
                    .filter_map(|b| {
                        if b.get("type").and_then(|t| t.as_str()) == Some("text") {
                            b.get("text").and_then(|t| t.as_str())
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("");
                if !joined.is_empty() {
                    return Some(joined);
                }
            }
            None
        }
    }
}

fn truncate(s: &str, max: usize) -> String {
    let count = s.chars().count();
    if count <= max {
        return s.to_string();
    }
    let head: String = s.chars().take(max).collect();
    format!("{head}…")
}
