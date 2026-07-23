use anyhow::Result;
use serde_json::{json, Map, Value};

use super::backup_before_write;
use super::util::{ensure_object, read_json_value, write_json_value};
use crate::paths::{ModelHubPaths, ModelHubPaths as Paths};
use crate::store::{
    find_provider, resolve_provider_write_key, resolve_upstream_model_id, AppConfig,
    ApplyAgentResult, Protocol, Secrets, Store, StoreService,
};

pub fn apply(
    svc: &StoreService,
    paths: &ModelHubPaths,
    config: &AppConfig,
    store: &Store,
    secrets: &Secrets,
    keep: u32,
) -> Result<ApplyAgentResult> {
    let file = Paths::opencode_config(&config.paths)?;
    let auth_file = Paths::opencode_auth(&config.paths)?;
    backup_before_write(paths, "opencode", &file, keep)?;
    if auth_file.exists() {
        backup_before_write(paths, "opencode", &auth_file, keep)?;
    }

    let mut root = read_json_value(&file)?;
    let obj = ensure_object(&mut root)?;

    let provider_map = obj
        .entry("provider".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    let provider_obj = ensure_object(provider_map)?;

    // Remove previously managed keys
    let managed_keys: Vec<String> = provider_obj
        .iter()
        .filter_map(|(k, v)| {
            v.get("_modelhub")
                .and_then(|m| m.get("managed"))
                .and_then(|b| b.as_bool())
                .filter(|b| *b)
                .map(|_| k.clone())
        })
        .collect();
    for k in managed_keys {
        provider_obj.remove(&k);
    }

    let enabled = svc.enabled_providers_with_models(store);
    let mut auth = if auth_file.exists() {
        read_json_value(&auth_file)?
    } else {
        Value::Object(Map::new())
    };
    let auth_obj = ensure_object(&mut auth)?;

    for (provider, models) in &enabled {
        let slug = resolve_provider_write_key(provider, provider_obj);
        let api_key = secrets
            .secrets
            .get(&provider.secret_ref)
            .map(|s| s.api_key.clone())
            .unwrap_or_default();

        let npm = match provider.protocol {
            Protocol::OpenaiCompletions => "@ai-sdk/openai-compatible",
            Protocol::OpenaiResponses => "@ai-sdk/openai",
            Protocol::AnthropicMessages => "@ai-sdk/anthropic",
        };

        let mut models_map = Map::new();
        for m in models {
            models_map.insert(
                m.model_id.clone(),
                json!({
                    "name": m.display_name,
                }),
            );
        }

        let mut options = Map::new();
        options.insert("baseURL".into(), Value::String(provider.base_url.clone()));
        if !provider.headers.is_empty() {
            let mut headers = Map::new();
            for (k, v) in &provider.headers {
                headers.insert(k.clone(), Value::String(v.clone()));
            }
            options.insert("headers".into(), Value::Object(headers));
        }

        let entry = json!({
            "npm": npm,
            "name": provider.name,
            "options": options,
            "models": models_map,
            "_modelhub": {
                "managed": true,
                "providerId": provider.id,
            }
        });
        provider_obj.insert(slug.clone(), entry);

        if !api_key.is_empty() {
            auth_obj.insert(
                slug.clone(),
                json!({
                    "type": "api",
                    "key": api_key,
                }),
            );
        }
    }

    let provider_keys = provider_obj.clone();
    if let (Some(pid), Some(mid)) = (
        store.agent_bindings.opencode.provider_id.as_deref(),
        store.agent_bindings.opencode.model_id.as_deref(),
    ) {
        if let (Some(p), Some(upstream)) = (
            find_provider(store, pid),
            resolve_upstream_model_id(store, mid),
        ) {
            let slug = resolve_provider_write_key(p, &provider_keys);
            obj.insert(
                "model".into(),
                Value::String(format!("{slug}/{upstream}")),
            );
        }
    }

    if let Some(small_id) = store.agent_bindings.opencode.small_model_id.as_deref() {
        if let Some(m) = store.models.iter().find(|x| x.id == small_id) {
            if let Some(p) = find_provider(store, &m.provider_id) {
                let slug = resolve_provider_write_key(p, &provider_keys);
                obj.insert(
                    "small_model".into(),
                    Value::String(format!("{slug}/{}", m.model_id)),
                );
            }
        }
    }

    write_json_value(&file, &root)?;
    write_json_value(&auth_file, &auth)?;

    Ok(ApplyAgentResult {
        agent: "opencode".into(),
        ok: true,
        message: format!(
            "已同步 {} 个 enabled Provider 到 OpenCode",
            enabled.len()
        ),
        files: vec![
            file.display().to_string(),
            auth_file.display().to_string(),
        ],
        restart_required: false,
    })
}
