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
    let models_file = Paths::pi_models(&config.paths)?;
    let settings_file = Paths::pi_settings(&config.paths)?;
    backup_before_write(paths, "pi", &models_file, keep)?;
    if settings_file.exists() {
        backup_before_write(paths, "pi", &settings_file, keep)?;
    }

    let mut root = read_json_value(&models_file)?;
    let obj = ensure_object(&mut root)?;

    let providers_val = obj
        .entry("providers".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    let providers = ensure_object(providers_val)?;

    // remove managed
    let managed: Vec<String> = providers
        .iter()
        .filter_map(|(k, v)| {
            v.get("_modelhub")
                .and_then(|m| m.get("managed"))
                .and_then(|b| b.as_bool())
                .filter(|b| *b)
                .map(|_| k.clone())
        })
        .collect();
    for k in managed {
        providers.remove(&k);
    }

    let enabled = svc.enabled_providers_with_models(store);
    for (provider, models) in &enabled {
        let slug = resolve_provider_write_key(provider, providers);
        let api_key = secrets
            .secrets
            .get(&provider.secret_ref)
            .map(|s| s.api_key.clone())
            .unwrap_or_default();

        let api = match provider.protocol {
            Protocol::OpenaiCompletions => "openai-completions",
            Protocol::OpenaiResponses => "openai-responses",
            Protocol::AnthropicMessages => "anthropic-messages",
        };

        let model_arr: Vec<Value> = models
            .iter()
            .map(|m| {
                json!({
                    "id": m.model_id,
                    "name": m.display_name,
                    "reasoning": m.capabilities.reasoning,
                })
            })
            .collect();

        let mut entry = Map::new();
        entry.insert("baseUrl".into(), Value::String(provider.base_url.clone()));
        entry.insert("api".into(), Value::String(api.into()));
        if !api_key.is_empty() {
            entry.insert("apiKey".into(), Value::String(api_key));
        }
        if provider.protocol == Protocol::OpenaiCompletions
            || provider.protocol == Protocol::OpenaiResponses
        {
            entry.insert("authHeader".into(), Value::Bool(true));
        }
        if !provider.headers.is_empty() {
            let mut headers = Map::new();
            for (k, v) in &provider.headers {
                headers.insert(k.clone(), Value::String(v.clone()));
            }
            entry.insert("headers".into(), Value::Object(headers));
        }
        if !provider.compat.is_empty() {
            entry.insert(
                "compat".into(),
                Value::Object(provider.compat.clone().into_iter().collect()),
            );
        }
        entry.insert("models".into(), Value::Array(model_arr));

        providers.insert(slug, Value::Object(entry));
    }

    write_json_value(&models_file, &root)?;

    let mut settings = read_json_value(&settings_file)?;
    let settings_obj = ensure_object(&mut settings)?;
    if let (Some(pid), Some(mid)) = (
        store.agent_bindings.pi.provider_id.as_deref(),
        store.agent_bindings.pi.model_id.as_deref(),
    ) {
        if let (Some(p), Some(upstream)) = (
            find_provider(store, pid),
            resolve_upstream_model_id(store, mid),
        ) {
            let providers_map = root
                .get("providers")
                .and_then(|v| v.as_object())
                .cloned()
                .unwrap_or_default();
            let slug = resolve_provider_write_key(p, &providers_map);
            settings_obj.insert("defaultProvider".into(), Value::String(slug));
            settings_obj.insert("defaultModel".into(), Value::String(upstream));
        }
    }
    write_json_value(&settings_file, &settings)?;

    Ok(ApplyAgentResult {
        agent: "pi".into(),
        ok: true,
        message: format!("已同步 {} 个 enabled Provider 到 Pi", enabled.len()),
        files: vec![
            models_file.display().to_string(),
            settings_file.display().to_string(),
        ],
        restart_required: false,
    })
}
