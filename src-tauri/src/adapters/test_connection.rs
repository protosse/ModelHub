use anyhow::{Context, Result};
use reqwest::header::{
    HeaderMap, HeaderName, HeaderValue, AUTHORIZATION, CONTENT_TYPE, COOKIE, SET_COOKIE,
};
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

pub struct TestConnectionParams<'a> {
    pub app: Option<AppHandle>,
    pub run_id: &'a str,
    pub store: &'a Store,
    pub secrets: &'a Secrets,
    pub model_row_id: &'a str,
    pub prompt: &'a str,
    pub timeout_secs: Option<u64>,
    pub extra_headers: Option<&'a std::collections::HashMap<String, String>>,
}

pub async fn test_model_connection(
    params: TestConnectionParams<'_>,
) -> Result<TestConnectionResult> {
    let TestConnectionParams {
        app,
        run_id,
        store,
        secrets,
        model_row_id,
        prompt,
        timeout_secs,
        extra_headers,
    } = params;
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

    // Build one case-insensitive HeaderMap. User values are inserted last, so
    // same-name headers replace protocol/auth defaults instead of being appended.
    let request_header_map = build_request_headers(&provider.protocol, api_key, extra_headers)?;
    let request_headers = build_header_log(&request_header_map);

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
    log.push(format!(
        "header merge: auth/protocol defaults → run extra ({} header(s))",
        request_header_map.len()
    ));
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

    let req = client.post(&url).json(&body).headers(request_header_map);

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
    let response_headers = build_header_log(resp.headers());

    log.push(format!(
        "response status={status_code} latency={latency_ms}ms"
    ));
    for h in &response_headers {
        log.push(format!("resp header: {h}"));
    }

    log.push("reading response body…");
    let raw = resp.text().await.unwrap_or_default();
    let raw_chars = raw.chars().count();
    log.push(format!("resp body length={raw_chars} chars"));
    let response_body = truncate(&raw, BODY_TRUNCATE);
    log.push(format!("resp body:\n{}", truncate(&raw, LOG_BODY_TRUNCATE)));

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

/// Client identity defaults so gateways that check User-Agent accept the probe.
fn protocol_default_headers(protocol: &Protocol) -> HeaderMap {
    let mut headers = HeaderMap::new();
    match protocol {
        Protocol::AnthropicMessages => {
            headers.insert("user-agent", HeaderValue::from_static("claude-cli/2.1.79"));
            headers.insert("x-app", HeaderValue::from_static("cli"));
            // Many Claude Code relays require this opt-in for the 1M context window.
            headers.insert(
                "anthropic-beta",
                HeaderValue::from_static("context-1m-2025-08-07"),
            );
        }
        Protocol::OpenaiCompletions => {
            headers.insert("user-agent", HeaderValue::from_static("openai-node"));
        }
        Protocol::OpenaiResponses => {
            headers.insert(
                "user-agent",
                HeaderValue::from_static("codex_cli_rs/0.144.4"),
            );
        }
    }
    headers
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

fn build_request_headers(
    protocol: &Protocol,
    api_key: &str,
    extra: Option<&std::collections::HashMap<String, String>>,
) -> Result<HeaderMap> {
    let mut headers = protocol_default_headers(protocol);
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {api_key}"))
            .context("invalid Authorization header value")?,
    );
    if protocol == &Protocol::AnthropicMessages {
        headers.insert(
            "x-api-key",
            HeaderValue::from_str(api_key).context("invalid x-api-key header value")?,
        );
        headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
    }

    if let Some(extra) = extra {
        for (raw_name, raw_value) in extra {
            let name = raw_name.trim();
            if name.is_empty() {
                continue;
            }
            let name = HeaderName::from_bytes(name.as_bytes())
                .with_context(|| format!("invalid header name: {name}"))?;
            let value = HeaderValue::from_str(raw_value)
                .with_context(|| format!("invalid value for header {name}"))?;
            headers.insert(name, value);
        }
    }
    Ok(headers)
}

fn is_sensitive_header(name: &HeaderName) -> bool {
    let lower = name.as_str();
    name == AUTHORIZATION
        || name == COOKIE
        || name == SET_COOKIE
        || lower == "proxy-authorization"
        || lower.contains("key")
        || lower.contains("token")
        || lower.contains("secret")
        || lower.contains("signature")
}

fn build_header_log(headers: &HeaderMap) -> Vec<String> {
    headers
        .iter()
        .map(|(name, value)| {
            let value = value.to_str().unwrap_or("<binary>");
            let displayed = if is_sensitive_header(name) {
                "***".to_string()
            } else {
                value.to_string()
            };
            format!("{name}: {displayed}")
        })
        .collect()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_agent_specific_user_agents_for_openai_protocols() {
        let completions = protocol_default_headers(&Protocol::OpenaiCompletions);
        let responses = protocol_default_headers(&Protocol::OpenaiResponses);

        assert_eq!(
            completions
                .get("user-agent")
                .and_then(|value| value.to_str().ok()),
            Some("openai-node")
        );
        assert_eq!(
            responses
                .get("user-agent")
                .and_then(|value| value.to_str().ok()),
            Some("codex_cli_rs/0.144.4")
        );
    }

    #[test]
    fn extra_headers_replace_defaults_case_insensitively() {
        let extra = std::collections::HashMap::from([
            ("user-agent".to_string(), "custom-client".to_string()),
            ("AUTHORIZATION".to_string(), "Bearer custom".to_string()),
        ]);
        let headers =
            build_request_headers(&Protocol::OpenaiCompletions, "store-key", Some(&extra)).unwrap();

        assert_eq!(headers.get_all("user-agent").iter().count(), 1);
        assert_eq!(headers["user-agent"], "custom-client");
        assert_eq!(headers.get_all(AUTHORIZATION).iter().count(), 1);
        assert_eq!(headers[AUTHORIZATION], "Bearer custom");
    }

    #[test]
    fn header_logs_redact_cookie_and_signature_values() {
        let headers = HeaderMap::from_iter([
            (SET_COOKIE, HeaderValue::from_static("session=raw-secret")),
            (
                HeaderName::from_static("x-signature"),
                HeaderValue::from_static("signature-secret"),
            ),
        ]);
        let log = build_header_log(&headers).join("\n");

        assert!(!log.contains("raw-secret"));
        assert!(!log.contains("signature-secret"));
    }
}
