use anyhow::Result;
use serde_json::{json, Map, Value};

use super::backup_before_write;
use super::util::{ensure_object, read_json_value, write_json_value};
use crate::paths::{ModelHubPaths, ModelHubPaths as Paths};
use crate::store::{
    agent_write_base_url, assign_catalog_write_keys, resolve_upstream_model_id, AppConfig,
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

    // Full overwrite: ModelHub owns the entire `provider` map. Clear all blocks
    // (managed or not) and rewrite only the sync catalog. `mcp` / `plugin` and
    // other top-level keys are untouched.
    provider_obj.clear();

    let enabled = svc.catalog_providers_with_models(store, "opencode");
    // Unique key per provider (names are globally unique); same map used by
    // preview. Prevents two providers sharing a base_url (diff protocol) from
    // colliding onto one key and overwriting each other.
    let write_keys = assign_catalog_write_keys(
        &enabled.iter().map(|(p, _)| p.clone()).collect::<Vec<_>>(),
    );
    let mut auth = if auth_file.exists() {
        read_json_value(&auth_file)?
    } else {
        Value::Object(Map::new())
    };
    let auth_obj = ensure_object(&mut auth)?;

    for (provider, models) in &enabled {
        let slug = write_keys.get(&provider.id).cloned().unwrap_or_default();
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
        options.insert(
            "baseURL".into(),
            Value::String(agent_write_base_url(&provider.base_url, &provider.protocol)),
        );
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

    if let (Some(pid), Some(mid)) = (
        store.agent_bindings.opencode.provider_id.as_deref(),
        store.agent_bindings.opencode.model_id.as_deref(),
    ) {
        if let (Some(slug), Some(upstream)) = (
            write_keys.get(pid),
            resolve_upstream_model_id(store, mid),
        ) {
            obj.insert(
                "model".into(),
                Value::String(format!("{slug}/{upstream}")),
            );
        }
    }

    if let Some(small_id) = store.agent_bindings.opencode.small_model_id.as_deref() {
        if let Some(m) = store.models.iter().find(|x| x.id == small_id) {
            if let Some(slug) = write_keys.get(&m.provider_id) {
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
            "已同步 {} 个同步目录 Provider 到 OpenCode",
            enabled.len()
        ),
        files: vec![
            file.display().to_string(),
            auth_file.display().to_string(),
        ],
        restart_required: false,
    })
}
