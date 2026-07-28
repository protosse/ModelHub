use anyhow::Result;
use serde_json::{json, Map, Value};

use super::backup_before_write;
use super::util::{ensure_object, find_existing_provider_entry, read_json_value, write_json_value};
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

    // ModelHub owns the `provider` directory membership: rewrite only the sync
    // catalog, but merge matched on-disk blocks so native/unknown settings on
    // those providers and models survive. Other top-level keys stay untouched.
    let existing_providers = provider_obj.clone();
    provider_obj.clear();

    let enabled = svc.catalog_providers_with_models(store, "opencode");
    // Unique key per provider (names are globally unique); same map used by
    // preview. Prevents two providers sharing a base_url (diff protocol) from
    // colliding onto one key and overwriting each other.
    let write_keys =
        assign_catalog_write_keys(&enabled.iter().map(|(p, _)| p.clone()).collect::<Vec<_>>());
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

        let existing = find_existing_provider_entry(&existing_providers, &provider.id, &slug)
            .map(|(_, value)| value);
        let entry = merge_provider_entry(existing, provider, models);
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
        if let (Some(slug), Some(upstream)) =
            (write_keys.get(pid), resolve_upstream_model_id(store, mid))
        {
            obj.insert("model".into(), Value::String(format!("{slug}/{upstream}")));
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
        message: format!("已同步 {} 个同步目录 Provider 到 OpenCode", enabled.len()),
        files: vec![file.display().to_string(), auth_file.display().to_string()],
        restart_required: false,
    })
}

fn merge_provider_entry(
    existing: Option<&Value>,
    provider: &crate::store::Provider,
    models: &[crate::store::Model],
) -> Value {
    let mut entry = existing
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();

    let npm = match provider.protocol {
        Protocol::OpenaiCompletions => "@ai-sdk/openai-compatible",
        Protocol::OpenaiResponses => "@ai-sdk/openai",
        Protocol::AnthropicMessages => "@ai-sdk/anthropic",
    };
    entry.insert("npm".into(), Value::String(npm.into()));
    entry.insert("name".into(), Value::String(provider.name.clone()));

    let mut options = entry
        .get("options")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    options.insert(
        "baseURL".into(),
        Value::String(agent_write_base_url(&provider.base_url, &provider.protocol)),
    );
    if !provider.headers.is_empty() {
        let mut headers = options
            .get("headers")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        for (k, v) in &provider.headers {
            headers.insert(k.clone(), Value::String(v.clone()));
        }
        options.insert("headers".into(), Value::Object(headers));
    }
    entry.insert("options".into(), Value::Object(options));

    let existing_models = entry
        .get("models")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mut models_map = Map::new();
    for model in models {
        let mut model_entry = existing_models
            .get(&model.model_id)
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        model_entry.insert("name".into(), Value::String(model.display_name.clone()));
        models_map.insert(model.model_id.clone(), Value::Object(model_entry));
    }
    entry.insert("models".into(), Value::Object(models_map));
    entry.insert(
        "_modelhub".into(),
        json!({ "managed": true, "providerId": provider.id }),
    );
    Value::Object(entry)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{Model, ModelCapabilities, Provider};
    use std::collections::HashMap;

    fn provider() -> Provider {
        Provider {
            id: "prov_1".into(),
            name: "Renamed Provider".into(),
            base_url: "https://new.example.com".into(),
            protocol: Protocol::OpenaiCompletions,
            headers: HashMap::from([("X-Managed".into(), "new".into())]),
            compat: HashMap::new(),
            enabled: true,
            notes: String::new(),
            secret_ref: "sec_1".into(),
            created_at: "now".into(),
            updated_at: "now".into(),
        }
    }

    fn model() -> Model {
        Model {
            id: "mdl_1".into(),
            provider_id: "prov_1".into(),
            model_id: "gpt-test".into(),
            display_name: "GPT Test New".into(),
            capabilities: ModelCapabilities::default(),
            created_at: "now".into(),
            updated_at: "now".into(),
        }
    }

    #[test]
    fn preserves_native_provider_options_and_model_fields() {
        let existing = json!({
            "npm": "old",
            "name": "Old",
            "customProviderField": { "keep": true },
            "options": {
                "baseURL": "https://old.example.com/v1",
                "timeout": 90000,
                "headers": { "X-Native": "keep", "X-Managed": "old" }
            },
            "models": {
                "gpt-test": {
                    "name": "Old model name",
                    "limit": { "context": 200000 },
                    "customModelField": "keep"
                },
                "removed-model": { "name": "Removed" }
            }
        });

        let merged = merge_provider_entry(Some(&existing), &provider(), &[model()]);

        assert_eq!(merged["customProviderField"]["keep"], true);
        assert_eq!(merged["options"]["timeout"], 90000);
        assert_eq!(merged["options"]["headers"]["X-Native"], "keep");
        assert_eq!(merged["options"]["headers"]["X-Managed"], "new");
        assert_eq!(merged["options"]["baseURL"], "https://new.example.com/v1");
        assert_eq!(merged["models"]["gpt-test"]["limit"]["context"], 200000);
        assert_eq!(merged["models"]["gpt-test"]["customModelField"], "keep");
        assert_eq!(merged["models"]["gpt-test"]["name"], "GPT Test New");
        assert!(merged["models"].get("removed-model").is_none());
    }
}
