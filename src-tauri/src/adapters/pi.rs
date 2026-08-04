use anyhow::Result;
use serde_json::{json, Map, Value};

use super::backup_before_write;
use super::util::{
    ensure_object, find_existing_provider_entry, read_json_value,
    retain_unmanaged_provider_entries, serialize_json_value, unmanaged_provider_keys,
};
use crate::backup::new_stamp;
use crate::file_io::write_atomic_group;
use crate::paths::{ModelHubPaths, ModelHubPaths as Paths};
use crate::store::{
    agent_write_base_url, assign_catalog_write_keys_with_reserved, find_provider,
    resolve_upstream_model_id, AppConfig, ApplyAgentResult, Protocol, Secrets, Store, StoreService,
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

    // Read and validate every target before creating backups or replacing any
    // file, so a malformed settings file cannot leave models.json updated.
    let mut root = read_json_value(&models_file)?;
    let obj = ensure_object(&mut root)?;

    let providers_val = obj
        .entry("providers".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    let providers = ensure_object(providers_val)?;

    // Remove only stale blocks previously managed by ModelHub. Native/unmanaged
    // providers stay untouched; catalog entries are merged into matching blocks.
    let existing_providers = providers.clone();
    retain_unmanaged_provider_entries(providers);

    let enabled = svc.catalog_providers_with_models(store, "pi");
    // Unique key per provider (names are globally unique); same map used by
    // preview. Prevents two providers sharing a base_url (diff protocol) from
    // colliding onto one key and overwriting each other. Keys also avoid native
    // (unmanaged) disk keys so a ModelHub provider never takes over a user block
    // that merely shares the slug.
    let native_keys = unmanaged_provider_keys(&existing_providers);
    let write_keys = assign_catalog_write_keys_with_reserved(
        &enabled.iter().map(|(p, _)| p.clone()).collect::<Vec<_>>(),
        &native_keys,
    );
    for (provider, models) in &enabled {
        let slug = write_keys.get(&provider.id).cloned().unwrap_or_default();
        let api_key = secrets
            .secrets
            .get(&provider.secret_ref)
            .map(|s| s.api_key.clone())
            .unwrap_or_default();

        let existing = find_existing_provider_entry(&existing_providers, &provider.id, &slug)
            .map(|(_, value)| value);
        providers.insert(
            slug,
            merge_provider_entry(existing, provider, models, &api_key),
        );
    }

    let mut settings = read_json_value(&settings_file)?;
    let settings_obj = ensure_object(&mut settings)?;
    if let (Some(pid), Some(mid)) = (
        store.agent_bindings.pi.provider_id.as_deref(),
        store.agent_bindings.pi.model_id.as_deref(),
    ) {
        // Only write defaults when the model is part of this provider's synced
        // subset, so Pi never points at a model that is never written into the
        // block (self-contradictory config).
        let in_subset = enabled
            .iter()
            .find(|(p, _)| p.id == pid)
            .map(|(_, models)| models.iter().any(|m| m.id == mid))
            .unwrap_or(false);
        if in_subset {
            if let (Some(p), Some(upstream)) = (
                find_provider(store, pid),
                resolve_upstream_model_id(store, mid),
            ) {
                let slug = write_keys.get(&p.id).cloned().unwrap_or_default();
                settings_obj.insert("defaultProvider".into(), Value::String(slug));
                settings_obj.insert("defaultModel".into(), Value::String(upstream));
            }
        }
    }
    let models_bytes = serialize_json_value(&root)?;
    let settings_bytes = serialize_json_value(&settings)?;
    let stamp = new_stamp();
    backup_before_write(paths, "pi", &models_file, keep, &stamp)?;
    if settings_file.exists() {
        backup_before_write(paths, "pi", &settings_file, keep, &stamp)?;
    }
    write_atomic_group(&[
        (models_file.clone(), models_bytes),
        (settings_file.clone(), settings_bytes),
    ])?;

    Ok(ApplyAgentResult {
        agent: "pi".into(),
        ok: true,
        message: format!("已同步 {} 个同步目录 Provider 到 Pi", enabled.len()),
        files: vec![
            models_file.display().to_string(),
            settings_file.display().to_string(),
        ],
        restart_required: false,
    })
}

fn merge_provider_entry(
    existing: Option<&Value>,
    provider: &crate::store::Provider,
    models: &[crate::store::Model],
    api_key: &str,
) -> Value {
    let mut entry = existing
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();

    let api = match provider.protocol {
        Protocol::OpenaiCompletions => "openai-completions",
        Protocol::OpenaiResponses => "openai-responses",
        Protocol::AnthropicMessages => "anthropic-messages",
    };
    entry.insert(
        "baseUrl".into(),
        Value::String(agent_write_base_url(&provider.base_url, &provider.protocol)),
    );
    entry.insert("api".into(), Value::String(api.into()));
    if api_key.is_empty() {
        entry.remove("apiKey");
    } else {
        entry.insert("apiKey".into(), Value::String(api_key.into()));
    }
    if provider.protocol == Protocol::OpenaiCompletions
        || provider.protocol == Protocol::OpenaiResponses
    {
        entry.insert("authHeader".into(), Value::Bool(true));
    } else {
        entry.remove("authHeader");
    }

    // Preserve native headers. Only seed Pi's default UA when the local block
    // does not already specify one.
    let mut headers = entry
        .get("headers")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    if !headers
        .keys()
        .any(|key| key.eq_ignore_ascii_case("User-Agent"))
    {
        headers.insert("User-Agent".into(), Value::String("pi-coding-agent".into()));
    }
    entry.insert("headers".into(), Value::Object(headers));

    let existing_models: std::collections::HashMap<String, &Value> = entry
        .get("models")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|model| {
            model
                .get("id")
                .and_then(Value::as_str)
                .map(|id| (id.to_string(), model))
        })
        .collect();
    let model_arr: Vec<Value> = models
        .iter()
        .map(|model| {
            let mut model_entry = existing_models
                .get(&model.model_id)
                .and_then(|value| value.as_object())
                .cloned()
                .unwrap_or_default();
            model_entry.insert("id".into(), Value::String(model.model_id.clone()));
            model_entry.insert("name".into(), Value::String(model.display_name.clone()));
            Value::Object(model_entry)
        })
        .collect();
    entry.insert("models".into(), Value::Array(model_arr));
    entry.insert(
        "_modelhub".into(),
        json!({ "managed": true, "providerId": provider.id }),
    );
    Value::Object(entry)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{Model, Provider};

    fn provider() -> Provider {
        Provider {
            id: "prov_1".into(),
            name: "Pi Provider".into(),
            base_url: "https://new.example.com".into(),
            protocol: Protocol::OpenaiResponses,
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
            model_id: "pi-test".into(),
            display_name: "Pi Test New".into(),
            created_at: "now".into(),
            updated_at: "now".into(),
        }
    }

    #[test]
    fn preserves_native_compat_headers_provider_and_model_fields() {
        let existing = json!({
            "baseUrl": "https://old.example.com/v1",
            "api": "old-api",
            "customProviderField": { "keep": true },
            "headers": {
                "user-agent": "native-pi-client",
                "X-Native": "keep",
                "X-Managed": "old"
            },
            "compat": { "nativeCompat": true, "managedCompat": "old" },
            "models": [
                {
                    "id": "pi-test",
                    "name": "Old model name",
                    "reasoning": false,
                    "contextWindow": 200000,
                    "customModelField": "keep"
                },
                { "id": "removed-model", "name": "Removed" }
            ]
        });

        let merged = merge_provider_entry(Some(&existing), &provider(), &[model()], "secret");

        assert_eq!(merged["customProviderField"]["keep"], true);
        assert_eq!(merged["headers"]["X-Native"], "keep");
        assert_eq!(merged["headers"]["X-Managed"], "old");
        assert_eq!(merged["headers"]["user-agent"], "native-pi-client");
        assert!(merged["headers"].get("User-Agent").is_none());
        assert_eq!(merged["compat"]["nativeCompat"], true);
        assert_eq!(merged["compat"]["managedCompat"], "old");
        assert_eq!(merged["baseUrl"], "https://new.example.com/v1");
        assert_eq!(merged["apiKey"], "secret");
        assert_eq!(merged["models"][0]["contextWindow"], 200000);
        assert_eq!(merged["models"][0]["customModelField"], "keep");
        assert_eq!(merged["models"][0]["name"], "Pi Test New");
        assert_eq!(merged["models"][0]["reasoning"], false);
        assert_eq!(merged["models"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn new_pi_provider_gets_default_user_agent() {
        let merged = merge_provider_entry(None, &provider(), &[model()], "secret");

        assert_eq!(merged["headers"]["User-Agent"], "pi-coding-agent");
    }
}
