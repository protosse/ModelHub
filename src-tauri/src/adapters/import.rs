use anyhow::{bail, Result};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

use super::util::read_json_value;
use crate::paths::ModelHubPaths as Paths;
use crate::store::{
    normalize_base_url, now_iso, provider_endpoint_key, AppConfig, ImportAction, ImportPreview,
    ImportPreviewItem, ImportRequest, ImportResult, Model, ModelCapabilities, Protocol, Provider,
    SecretEntry, StoreService,
};
use fs_err as fs;

#[derive(Clone)]
struct Candidate {
    source: String,
    name: String,
    base_url: String,
    protocol: Protocol,
    api_key: String,
    models: Vec<(String, String, bool)>, // id, display, reasoning
}

fn collect_candidates(config: &AppConfig) -> Result<(Vec<Candidate>, Vec<String>)> {
    let mut raw: Vec<Candidate> = Vec::new();
    let mut notes: Vec<String> = Vec::new();

    if let Ok(path) = Paths::opencode_config(&config.paths) {
        if path.exists() {
            match read_json_value(&path) {
                Err(e) => notes.push(format!("OpenCode 配置解析失败：{e}")),
                Ok(root) => {
                    if let Some(providers) = root.get("provider").and_then(|v| v.as_object()) {
                        let auth = Paths::opencode_auth(&config.paths)
                            .ok()
                            .and_then(|p| read_json_value(&p).ok());
                        for (key, val) in providers {
                            let base = val
                                .pointer("/options/baseURL")
                                .or_else(|| val.pointer("/options/baseUrl"))
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            if base.is_empty() {
                                continue;
                            }
                            let protocol = guess_protocol_from_npm(
                                val.get("npm").and_then(|v| v.as_str()).unwrap_or(""),
                            );
                            let api_key = extract_opencode_key(auth.as_ref(), key, val);
                            let name = val
                                .get("name")
                                .and_then(|v| v.as_str())
                                .unwrap_or(key)
                                .to_string();
                            let mut models = Vec::new();
                            if let Some(map) = val.get("models").and_then(|m| m.as_object()) {
                                for (mid, meta) in map {
                                    let display = meta
                                        .get("name")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or(mid)
                                        .to_string();
                                    models.push((mid.clone(), display, false));
                                }
                            }
                            raw.push(Candidate {
                                source: "opencode".into(),
                                name,
                                base_url: base,
                                protocol,
                                api_key,
                                models,
                            });
                        }
                    }
                }
            }
        }
    }

    if let Ok(path) = Paths::pi_models(&config.paths) {
        if path.exists() {
            match read_json_value(&path) {
                Err(e) => notes.push(format!("Pi models.json 解析失败：{e}")),
                Ok(root) => {
                    if let Some(providers) = root.get("providers").and_then(|v| v.as_object()) {
                        for (key, val) in providers {
                            let base = val
                                .get("baseUrl")
                                .or_else(|| val.get("baseURL"))
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            if base.is_empty() {
                                continue;
                            }
                            let protocol = guess_protocol_from_api(
                                val.get("api").and_then(|v| v.as_str()).unwrap_or(""),
                            );
                            let api_key = val
                                .get("apiKey")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            let mut models = Vec::new();
                            if let Some(arr) = val.get("models").and_then(|m| m.as_array()) {
                                for m in arr {
                                    let mid = m
                                        .get("id")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("")
                                        .to_string();
                                    if mid.is_empty() {
                                        continue;
                                    }
                                    let display = m
                                        .get("name")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or(&mid)
                                        .to_string();
                                    let reasoning = m
                                        .get("reasoning")
                                        .and_then(|v| v.as_bool())
                                        .unwrap_or(false);
                                    models.push((mid, display, reasoning));
                                }
                            }
                            raw.push(Candidate {
                                source: "pi".into(),
                                name: key.clone(),
                                base_url: base,
                                protocol,
                                api_key,
                                models,
                            });
                        }
                    }
                }
            }
        }
    }

    if let Ok(path) = Paths::claude_settings(&config.paths) {
        if path.exists() {
            match read_json_value(&path) {
                Err(e) => notes.push(format!("Claude settings.json 解析失败：{e}")),
                Ok(root) => {
                    let base = root
                        .pointer("/env/ANTHROPIC_BASE_URL")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let key = root
                        .pointer("/env/ANTHROPIC_AUTH_TOKEN")
                        .or_else(|| root.pointer("/env/ANTHROPIC_API_KEY"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let model = root
                        .get("model")
                        .and_then(|v| v.as_str())
                        .or_else(|| {
                            root.pointer("/env/ANTHROPIC_MODEL")
                                .and_then(|v| v.as_str())
                        })
                        .unwrap_or("default")
                        .to_string();
                    if !base.is_empty() {
                        raw.push(Candidate {
                            source: "claude".into(),
                            name: "claude-current".into(),
                            base_url: base,
                            protocol: Protocol::AnthropicMessages,
                            api_key: key,
                            models: vec![(model.clone(), model, false)],
                        });
                    }
                }
            }
        }
    }

    if let Ok(path) = Paths::codex_config(&config.paths) {
        if path.exists() {
            match fs::read_to_string(&path) {
                Err(e) => notes.push(format!("Codex config.toml 读取失败：{e}")),
                Ok(text) => match text.parse::<toml::Value>() {
                    Err(e) => notes.push(format!("Codex config.toml 解析失败：{e}")),
                    Ok(val) => {
                        let active_model = val
                            .get("model")
                            .and_then(|v| v.as_str())
                            .unwrap_or("default")
                            .to_string();
                        let auth_openai_key = read_codex_auth_openai_key();
                        if let Some(table) = val.get("model_providers").and_then(|v| v.as_table()) {
                            for (key, block) in table {
                                let base = block
                                    .get("base_url")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string();
                                if base.is_empty() {
                                    continue;
                                }
                                let name = block
                                    .get("name")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or(key)
                                    .to_string();
                                let wire = block
                                    .get("wire_api")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("responses");
                                let protocol = if wire == "chat" {
                                    Protocol::OpenaiCompletions
                                } else {
                                    Protocol::OpenaiResponses
                                };
                                let api_key = extract_codex_key(block, auth_openai_key.as_deref());
                                raw.push(Candidate {
                                    source: "codex".into(),
                                    name,
                                    base_url: base,
                                    protocol,
                                    api_key,
                                    models: vec![(active_model.clone(), active_model.clone(), false)],
                                });
                            }
                        }
                    }
                },
            }
        }
    }

    Ok((merge_candidates(raw), notes))
}

fn read_codex_auth_openai_key() -> Option<String> {
    let path = Paths::home().ok()?.join(".codex").join("auth.json");
    if !path.exists() {
        return None;
    }
    let root = read_json_value(&path).ok()?;
    root.get("OPENAI_API_KEY")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn extract_codex_key(block: &toml::Value, auth_openai_key: Option<&str>) -> String {
    if let Some(k) = block
        .get("experimental_bearer_token")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return k.to_string();
    }
    let requires_openai_auth = block
        .get("requires_openai_auth")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if requires_openai_auth {
        if let Some(k) = auth_openai_key.filter(|s| !s.is_empty()) {
            return k.to_string();
        }
    }
    if let Some(k) = auth_openai_key.filter(|s| !s.is_empty()) {
        return k.to_string();
    }
    String::new()
}

fn extract_opencode_key(auth: Option<&Value>, provider_key: &str, provider_val: &Value) -> String {
    if let Some(k) = provider_val
        .pointer("/options/apiKey")
        .or_else(|| provider_val.pointer("/options/api_key"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    {
        return k.to_string();
    }
    if let Some(auth) = auth {
        if let Some(entry) = auth.get(provider_key) {
            if let Some(s) = entry.as_str().filter(|s| !s.is_empty()) {
                return s.to_string();
            }
            if let Some(k) = entry
                .get("key")
                .or_else(|| entry.get("apiKey"))
                .or_else(|| entry.get("token"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
            {
                return k.to_string();
            }
        }
    }
    String::new()
}

fn merge_candidates(raw: Vec<Candidate>) -> Vec<Candidate> {
    let mut map: HashMap<String, Candidate> = HashMap::new();
    for c in raw {
        let ek = provider_endpoint_key(&c.base_url, &c.protocol);
        match map.get_mut(&ek) {
            None => {
                map.insert(ek, c);
            }
            Some(exist) => {
                if !exist.source.split('+').any(|s| s == c.source) {
                    exist.source = format!("{}+{}", exist.source, c.source);
                }
                if exist.api_key.is_empty() && !c.api_key.is_empty() {
                    exist.api_key = c.api_key;
                }
                if exist.name.len() < c.name.len() {
                    exist.name = c.name;
                }
                let mut seen: HashSet<String> =
                    exist.models.iter().map(|(id, _, _)| id.clone()).collect();
                for m in c.models {
                    if seen.insert(m.0.clone()) {
                        exist.models.push(m);
                    }
                }
            }
        }
    }
    let mut out: Vec<_> = map.into_values().collect();
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    out
}

pub fn preview_import(svc: &StoreService, config: &AppConfig) -> Result<ImportPreview> {
    let store = svc.load_store()?;
    let (candidates, scan_notes) = collect_candidates(config)?;

    let mut endpoint_to_existing: HashMap<String, (String, String)> = HashMap::new();
    for p in &store.providers {
        endpoint_to_existing.insert(
            provider_endpoint_key(&p.base_url, &p.protocol),
            (p.id.clone(), p.name.clone()),
        );
    }
    let existing_names: HashSet<String> = store
        .providers
        .iter()
        .map(|p| p.name.to_lowercase())
        .collect();

    let mut items = Vec::new();
    let mut name_counts: HashMap<String, usize> = HashMap::new();
    for c in &candidates {
        *name_counts
            .entry(c.name.to_lowercase())
            .or_insert(0) += 1;
    }

    for c in candidates {
        let ek = provider_endpoint_key(&c.base_url, &c.protocol);
        let existing = endpoint_to_existing.get(&ek);
        let name_conflict = existing_names.contains(&c.name.to_lowercase())
            && existing
                .as_ref()
                .map(|(_, n)| n.to_lowercase() != c.name.to_lowercase())
                .unwrap_or(true);
        let batch_dup = name_counts.get(&c.name.to_lowercase()).copied().unwrap_or(0) > 1;

        let model_ids: Vec<String> = c.models.iter().map(|(id, _, _)| id.clone()).collect();
        let (new_model_ids, existing_model_ids, extra_model_count) =
            if let Some((pid, _)) = existing {
                let have: HashSet<String> = store
                    .models
                    .iter()
                    .filter(|m| m.provider_id == *pid)
                    .map(|m| m.model_id.clone())
                    .collect();
                let mut neu = Vec::new();
                let mut old = Vec::new();
                for (id, _, _) in &c.models {
                    if have.contains(id) {
                        old.push(id.clone());
                    } else {
                        neu.push(id.clone());
                    }
                }
                let n = neu.len();
                (neu, old, n)
            } else {
                (model_ids.clone(), Vec::new(), 0)
            };
        items.push(ImportPreviewItem {
            id: format!("ep|{ek}"),
            source: c.source,
            name: c.name,
            base_url: normalize_base_url(&c.base_url),
            protocol: c.protocol,
            model_count: c.models.len(),
            model_ids,
            extra_model_count,
            new_model_ids,
            existing_model_ids,
            already_exists: existing.is_some(),
            existing_provider_id: existing.map(|(id, _)| id.clone()),
            existing_name: existing.map(|(_, n)| n.clone()),
            name_conflict: name_conflict || batch_dup,
            has_api_key: !c.api_key.is_empty(),
        });
    }

        items.sort_by(|a, b| {
        let score = |it: &ImportPreviewItem| -> i32 {
            if !it.already_exists && it.has_api_key {
                0
            } else if !it.already_exists {
                1
            } else if it.extra_model_count > 0 {
                2
            } else {
                3
            }
        };
        score(a)
            .cmp(&score(b))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    Ok(ImportPreview {
        items,
        scan_notes,
    })
}

pub fn import_from_agents(
    svc: &StoreService,
    config: &AppConfig,
    req: &ImportRequest,
) -> Result<ImportResult> {
    if req.items.is_empty() {
        bail!("未选择要导入的提供商");
    }

    let (candidates, _) = collect_candidates(config)?;
    let by_endpoint: HashMap<String, Candidate> = candidates
        .into_iter()
        .map(|c| (provider_endpoint_key(&c.base_url, &c.protocol), c))
        .collect();

    // Single load + single save for the whole batch (atomic from disk's perspective).
    let mut store = svc.load_store()?;
    let mut secrets = svc.load_secrets()?;
    let mut name_to_id: HashMap<String, String> = store
        .providers
        .iter()
        .map(|p| (p.name.to_lowercase(), p.id.clone()))
        .collect();
    let mut endpoint_to_id: HashMap<String, String> = store
        .providers
        .iter()
        .map(|p| {
            (
                provider_endpoint_key(&p.base_url, &p.protocol),
                p.id.clone(),
            )
        })
        .collect();

    // Validate final names unique among decisions
    let mut final_names: HashSet<String> = HashSet::new();
    for d in &req.items {
        if matches!(d.action, ImportAction::Skip) {
            continue;
        }
        let n = d.name.trim();
        if n.is_empty() {
            bail!("提供商名称不能为空");
        }
        let key = n.to_lowercase();
        if !final_names.insert(key) {
            bail!("导入列表中存在重复名称：{n}");
        }
    }

    let mut imported_providers = 0usize;
    let mut imported_models = 0usize;
    let mut skipped = 0usize;
    let mut overridden = 0usize;
    let mut secrets_dirty = false;
    let now = now_iso();

    for d in &req.items {
        match d.action {
            ImportAction::Skip => {
                skipped += 1;
                continue;
            }
            ImportAction::Import | ImportAction::Override => {}
        }

        let endpoint = d
            .id
            .strip_prefix("ep|")
            .unwrap_or(d.id.as_str())
            .to_string();
        let Some(c) = by_endpoint.get(&endpoint) else {
            skipped += 1;
            continue;
        };
        let final_name = d.name.trim().to_string();
        let name_key = final_name.to_lowercase();

        let existing_by_endpoint = endpoint_to_id.get(&endpoint).cloned();
        let existing_by_name = name_to_id.get(&name_key).cloned();

        if matches!(d.action, ImportAction::Import) {
            if existing_by_endpoint.is_some() {
                bail!("「{final_name}」对应端点已存在，请选择覆盖，或跳过");
            }
            if existing_by_name.is_some() {
                bail!("名称「{final_name}」已被占用，请改名或选择覆盖");
            }
        }

        let target_id = match d.action {
            ImportAction::Override => existing_by_endpoint
                .or(existing_by_name)
                .ok_or_else(|| anyhow::anyhow!("覆盖目标不存在：{final_name}"))?,
            ImportAction::Import => {
                let secret_ref = format!("sec_{}", uuid::Uuid::new_v4());
                let provider = Provider {
                    id: format!("prov_{}", uuid::Uuid::new_v4()),
                    name: final_name.clone(),
                    base_url: normalize_base_url(&c.base_url),
                    protocol: c.protocol.clone(),
                    headers: HashMap::new(),
                    compat: HashMap::new(),
                    enabled: true,
                    notes: format!("imported from {}", c.source),
                    secret_ref: secret_ref.clone(),
                    created_at: now.clone(),
                    updated_at: now.clone(),
                };
                // Always create the secret slot; empty scanned key stays empty
                // (does not overwrite an existing secret because this is a new ref).
                secrets.secrets.insert(
                    secret_ref,
                    SecretEntry {
                        api_key: c.api_key.clone(),
                        updated_at: now.clone(),
                    },
                );
                secrets_dirty = true;
                let id = provider.id.clone();
                name_to_id.insert(name_key.clone(), id.clone());
                endpoint_to_id.insert(endpoint.clone(), id.clone());
                store.providers.push(provider);
                imported_providers += 1;
                id
            }
            ImportAction::Skip => continue,
        };

        if matches!(d.action, ImportAction::Override) {
            let provider = store
                .providers
                .iter_mut()
                .find(|p| p.id == target_id)
                .ok_or_else(|| anyhow::anyhow!("覆盖目标不存在：{final_name}"))?;

            // If renaming, refuse collision with a *different* provider.
            if let Some(other_id) = name_to_id.get(&name_key) {
                if other_id != &target_id {
                    bail!("名称「{final_name}」已被其它提供商占用，请改名");
                }
            }

            let old_name_key = provider.name.to_lowercase();
            // Preserve headers / compat / enabled / user notes; only update
            // identity fields + non-empty api key + source note if empty.
            provider.name = final_name.clone();
            provider.base_url = normalize_base_url(&c.base_url);
            provider.protocol = c.protocol.clone();
            provider.updated_at = now.clone();
            if provider.notes.trim().is_empty() {
                provider.notes = format!("imported from {}", c.source);
            }

            if !c.api_key.trim().is_empty() {
                let secret_ref = provider.secret_ref.clone();
                secrets.secrets.insert(
                    secret_ref,
                    SecretEntry {
                        api_key: c.api_key.clone(),
                        updated_at: now.clone(),
                    },
                );
                secrets_dirty = true;
            }

            if old_name_key != name_key {
                name_to_id.remove(&old_name_key);
            }
            name_to_id.insert(name_key, target_id.clone());
            // Endpoint key may change if base_url/protocol updated.
            endpoint_to_id.retain(|_, id| id != &target_id);
            endpoint_to_id.insert(
                provider_endpoint_key(&normalize_base_url(&c.base_url), &c.protocol),
                target_id.clone(),
            );
            overridden += 1;
        }

        let existing_model_ids: HashSet<String> = store
            .models
            .iter()
            .filter(|m| m.provider_id == target_id)
            .map(|m| m.model_id.clone())
            .collect();

        for (mid, display, reasoning) in &c.models {
            if existing_model_ids.contains(mid) {
                continue;
            }
            store.models.push(Model {
                id: format!("mdl_{}", uuid::Uuid::new_v4()),
                provider_id: target_id.clone(),
                model_id: mid.clone(),
                display_name: display.clone(),
                enabled: true,
                capabilities: ModelCapabilities {
                    reasoning: *reasoning,
                    vision: false,
                },
                created_at: now.clone(),
                updated_at: now.clone(),
            });
            imported_models += 1;
        }
    }

    if secrets_dirty {
        svc.save_secrets(&secrets)?;
    }
    // Always persist store after a successful batch (even if only counters moved).
    if imported_providers > 0 || imported_models > 0 || overridden > 0 {
        svc.save_store(&store)?;
    }

    Ok(ImportResult {
        imported_providers,
        imported_models,
        skipped,
        overridden,
    })
}

fn guess_protocol_from_npm(npm: &str) -> Protocol {
    if npm.contains("anthropic") {
        Protocol::AnthropicMessages
    } else if npm.ends_with("/openai") && !npm.contains("compatible") {
        Protocol::OpenaiResponses
    } else {
        Protocol::OpenaiCompletions
    }
}

fn guess_protocol_from_api(api: &str) -> Protocol {
    match api {
        "anthropic-messages" => Protocol::AnthropicMessages,
        "openai-responses" => Protocol::OpenaiResponses,
        _ => Protocol::OpenaiCompletions,
    }
}
