use anyhow::Result;
use serde_json::Value;

use super::util::read_json_value;
use crate::paths::ModelHubPaths as Paths;
use crate::store::{
    normalize_base_url, provider_endpoint_key, AgentBindings, AgentMode, AppConfig, ClaudeBinding,
    CodexBinding, OpencodeBinding, PiBinding, Protocol, Store, StoreService,
};
use fs_err as fs;

/// Read each agent config from disk and map into ModelHub AgentBindings
/// (matched against known providers/models in the store when possible).
pub fn read_live_bindings(svc: &StoreService, config: &AppConfig) -> Result<AgentBindings> {
    let store = svc.load_store()?;
    // Read each agent independently: one corrupt agent file must not blank the
    // other agents' drafts (which an earlier `?`-aggregated read did, leaving
    // Apply enabled over default/empty bindings). A per-agent parse failure falls
    // back to that agent's defaults here; preview_apply still re-reads disk and
    // reports a parse error for the broken agent, and Apply fails gracefully per
    // agent, so the user is never silently pushed into wiping good config.
    let claude = live_claude(config, &store).unwrap_or_default();
    let codex = live_codex(config, &store).unwrap_or_default();
    let opencode = live_opencode(config, &store).unwrap_or_default();
    let pi = live_pi(config, &store).unwrap_or_default();
    Ok(AgentBindings {
        claude,
        codex,
        opencode,
        pi,
    })
}

fn live_claude(config: &AppConfig, store: &Store) -> Result<ClaudeBinding> {
    let file = Paths::claude_settings(&config.paths)?;
    if !file.exists() {
        return Ok(ClaudeBinding::default());
    }
    let root = read_json_value(&file)?;
    let env = root
        .get("env")
        .cloned()
        .unwrap_or(Value::Object(Default::default()));
    let base = env
        .get("ANTHROPIC_BASE_URL")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let model = root
        .get("model")
        .and_then(|v| v.as_str())
        .or_else(|| env.get("ANTHROPIC_MODEL").and_then(|v| v.as_str()))
        .unwrap_or("")
        .to_string();

    if base.is_empty() {
        return Ok(ClaudeBinding {
            mode: AgentMode::Official,
            ..ClaudeBinding::default()
        });
    }

    let (provider_id, model_id) = match_provider_model(
        store,
        &base,
        Protocol::AnthropicMessages,
        if model.is_empty() { None } else { Some(&model) },
        true,
    );

    Ok(ClaudeBinding {
        mode: AgentMode::ThirdParty,
        provider_id,
        model_id,
        haiku_model_id: None,
        sonnet_model_id: None,
        opus_model_id: None,
    })
}

fn live_codex(config: &AppConfig, store: &Store) -> Result<CodexBinding> {
    let file = Paths::codex_config(&config.paths)?;
    if !file.exists() {
        return Ok(CodexBinding::default());
    }
    let text = fs::read_to_string(&file)?;
    let val = text
        .parse::<toml::Value>()
        .unwrap_or(toml::Value::Table(Default::default()));
    let mp = val
        .get("model_provider")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let model = val
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    if mp.is_empty() || mp == "openai" {
        return Ok(CodexBinding {
            mode: AgentMode::Official,
            provider_key: "modelhub".into(),
            ..CodexBinding::default()
        });
    }

    let base = val
        .get("model_providers")
        .and_then(|v| v.get(&mp))
        .and_then(|v| v.get("base_url"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let wire = val
        .get("model_providers")
        .and_then(|v| v.get(&mp))
        .and_then(|v| v.get("wire_api"))
        .and_then(|v| v.as_str())
        .unwrap_or("responses");
    let protocol = if wire == "chat" {
        Protocol::OpenaiCompletions
    } else {
        Protocol::OpenaiResponses
    };

    let (provider_id, model_id) = if base.is_empty() {
        (None, None)
    } else {
        match_provider_model(
            store,
            &base,
            protocol,
            if model.is_empty() { None } else { Some(&model) },
            true,
        )
    };

    Ok(CodexBinding {
        mode: AgentMode::ThirdParty,
        provider_id,
        model_id,
        provider_key: if mp.is_empty() { "modelhub".into() } else { mp },
    })
}

fn live_opencode(config: &AppConfig, store: &Store) -> Result<OpencodeBinding> {
    let file = Paths::opencode_config(&config.paths)?;
    let root = if file.exists() {
        read_json_value(&file)?
    } else {
        Value::Object(Default::default())
    };

    let mut model_str = root
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let mut slug_hint = String::new();
    let mut upstream_hint = String::new();

    if model_str.is_empty() {
        if let Some((provider_id, model_id)) = read_opencode_recent_model() {
            slug_hint = provider_id;
            upstream_hint = model_id;
            model_str = format!("{slug_hint}/{upstream_hint}");
        }
    }

    if model_str.is_empty() {
        return Ok(OpencodeBinding::default());
    }

    let (slug, upstream) = if !slug_hint.is_empty() {
        (slug_hint, upstream_hint)
    } else {
        split_provider_model(&model_str)
    };

    let providers_map = root.get("provider").and_then(|v| v.as_object());
    let provider_block = providers_map.and_then(|m| m.get(&slug));
    let base = provider_block
        .and_then(|p| {
            p.pointer("/options/baseURL")
                .or_else(|| p.pointer("/options/baseUrl"))
                .and_then(|v| v.as_str())
        })
        .unwrap_or("")
        .to_string();
    let protocol = provider_block
        .and_then(|p| p.get("npm"))
        .and_then(|v| v.as_str())
        .map(protocol_from_npm)
        .unwrap_or(Protocol::OpenaiCompletions);

    let (provider_id, model_id) =
        if let Some(matched) = match_managed_provider_model(store, provider_block, &upstream) {
            matched
        } else if !base.is_empty() {
            match_provider_model(
                store,
                &base,
                protocol,
                if upstream.is_empty() {
                    None
                } else {
                    Some(&upstream)
                },
                true,
            )
        } else {
            match_by_name_and_model(store, &slug, &upstream)
        };

    let small = root
        .get("small_model")
        .and_then(|v| v.as_str())
        .map(|s| {
            let (s_slug, s_up) = split_provider_model(s);
            if s_up.is_empty() {
                None
            } else if let Some(pid) = &provider_id {
                store
                    .models
                    .iter()
                    .find(|m| m.provider_id == *pid && m.model_id == s_up)
                    .map(|m| m.id.clone())
            } else {
                let (_, mid) = match_by_name_and_model(store, &s_slug, &s_up);
                mid
            }
        })
        .flatten();

    Ok(OpencodeBinding {
        provider_id,
        model_id,
        small_model_id: small,
    })
}

fn read_opencode_recent_model() -> Option<(String, String)> {
    let home = dirs::home_dir()?;
    let path = home.join(".local/state/opencode/model.json");
    if !path.exists() {
        return None;
    }
    let root = read_json_value(&path).ok()?;
    let recent = root.get("recent")?.as_array()?;
    let first = recent.first()?;
    let provider = first.get("providerID")?.as_str()?.to_string();
    let model = first.get("modelID")?.as_str()?.to_string();
    if provider.is_empty() || model.is_empty() {
        return None;
    }
    Some((provider, model))
}

fn live_pi(config: &AppConfig, store: &Store) -> Result<PiBinding> {
    let settings_file = Paths::pi_settings(&config.paths)?;
    let models_file = Paths::pi_models(&config.paths)?;
    if !settings_file.exists() {
        return Ok(PiBinding::default());
    }
    let settings = read_json_value(&settings_file)?;
    let def_p = settings
        .get("defaultProvider")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let def_m = settings
        .get("defaultModel")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if def_p.is_empty() && def_m.is_empty() {
        return Ok(PiBinding::default());
    }

    let mut base = String::new();
    let mut protocol = Protocol::OpenaiCompletions;
    let mut provider_block = None;
    if models_file.exists() {
        if let Ok(root) = read_json_value(&models_file) {
            if let Some(p) = root
                .get("providers")
                .and_then(|v| v.as_object())
                .and_then(|m| m.get(&def_p))
            {
                provider_block = Some(p.clone());
                base = p
                    .get("baseUrl")
                    .or_else(|| p.get("baseURL"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                protocol = p
                    .get("api")
                    .and_then(|v| v.as_str())
                    .map(protocol_from_api)
                    .unwrap_or(Protocol::OpenaiCompletions);
            }
        }
    }

    let (provider_id, model_id) = if let Some(matched) =
        match_managed_provider_model(store, provider_block.as_ref(), &def_m)
    {
        matched
    } else if !base.is_empty() {
        match_provider_model(
            store,
            &base,
            protocol,
            if def_m.is_empty() { None } else { Some(&def_m) },
            true,
        )
    } else {
        match_by_name_and_model(store, &def_p, &def_m)
    };

    Ok(PiBinding {
        provider_id,
        model_id,
    })
}

fn match_managed_provider_model(
    store: &Store,
    provider_block: Option<&Value>,
    upstream_model: &str,
) -> Option<(Option<String>, Option<String>)> {
    let provider_id = provider_block?
        .get("_modelhub")?
        .get("providerId")?
        .as_str()?;
    let provider = store.providers.iter().find(|p| p.id == provider_id)?;
    let model_id = if upstream_model.is_empty() {
        None
    } else {
        store
            .models
            .iter()
            .find(|m| m.provider_id == provider.id && m.model_id == upstream_model)
            .map(|m| m.id.clone())
    };
    Some((Some(provider.id.clone()), model_id))
}

fn match_provider_model(
    store: &Store,
    base_url: &str,
    protocol: Protocol,
    upstream_model: Option<&str>,
    loose_protocol: bool,
) -> (Option<String>, Option<String>) {
    let base = normalize_base_url(base_url);
    let endpoint = provider_endpoint_key(&base, &protocol);
    let comparable_base = provider_base_for_match(&base, &protocol);

    // Match in distinct passes so iteration order never picks the wrong provider:
    //   1. exact endpoint (base + protocol)
    //   2. same protocol after stripping a `/v1` suffix on OpenAI-style URLs
    //      (Apply auto-appends `/v1`, so disk may carry it while the Store does
    //      not)
    //   3. loose base-only fallback (only when explicitly requested) — used when
    //      the on-disk protocol is a guess that may not match the Store value.
    // Previously all three checks lived in one `.find()`, so a completions
    // provider sorted before a responses provider could win the loose branch
    // even when the disk protocol was responses, returning the wrong API key.
    let provider = store
        .providers
        .iter()
        .find(|p| provider_endpoint_key(&p.base_url, &p.protocol) == endpoint)
        .or_else(|| {
            store.providers.iter().find(|p| {
                p.protocol == protocol
                    && provider_base_for_match(&p.base_url, &p.protocol) == comparable_base
            })
        })
        .or_else(|| {
            if loose_protocol {
                store.providers.iter().find(|p| {
                    provider_base_for_match(&p.base_url, &p.protocol) == comparable_base
                })
            } else {
                None
            }
        });

    let provider_id = provider.map(|p| p.id.clone());
    let model_id = match (provider, upstream_model) {
        (Some(p), Some(m)) => store
            .models
            .iter()
            .find(|x| x.provider_id == p.id && x.model_id == m)
            .map(|x| x.id.clone())
            .or_else(|| {
                // partial match
                store
                    .models
                    .iter()
                    .find(|x| {
                        x.provider_id == p.id && (x.model_id.contains(m) || m.contains(&x.model_id))
                    })
                    .map(|x| x.id.clone())
            }),
        _ => None,
    };
    (provider_id, model_id)
}

fn provider_base_for_match(base_url: &str, protocol: &Protocol) -> String {
    let base = normalize_base_url(base_url);
    if matches!(
        protocol,
        Protocol::OpenaiCompletions | Protocol::OpenaiResponses
    ) {
        base.strip_suffix("/v1").unwrap_or(&base).to_string()
    } else {
        base
    }
}

fn match_by_name_and_model(
    store: &Store,
    name_or_slug: &str,
    upstream: &str,
) -> (Option<String>, Option<String>) {
    let needle = name_or_slug.to_lowercase();
    let provider = store.providers.iter().find(|p| {
        p.name.to_lowercase() == needle
            || p.name.to_lowercase().replace(' ', "-") == needle
            || p.id.to_lowercase().contains(&needle)
    });
    let provider_id = provider.map(|p| p.id.clone());
    let model_id = match (provider, upstream.is_empty()) {
        (Some(p), false) => store
            .models
            .iter()
            .find(|m| m.provider_id == p.id && m.model_id == upstream)
            .map(|m| m.id.clone()),
        _ => None,
    };
    (provider_id, model_id)
}

fn split_provider_model(s: &str) -> (String, String) {
    if let Some(i) = s.find('/') {
        (s[..i].to_string(), s[i + 1..].to_string())
    } else {
        (s.to_string(), String::new())
    }
}

fn protocol_from_npm(npm: &str) -> Protocol {
    if npm.contains("anthropic") {
        Protocol::AnthropicMessages
    } else if npm.ends_with("/openai") && !npm.contains("compatible") {
        Protocol::OpenaiResponses
    } else {
        Protocol::OpenaiCompletions
    }
}

fn protocol_from_api(api: &str) -> Protocol {
    match api {
        "anthropic-messages" => Protocol::AnthropicMessages,
        "openai-responses" => Protocol::OpenaiResponses,
        _ => Protocol::OpenaiCompletions,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{Model, Provider};
    use serde_json::json;

    #[test]
    fn managed_provider_id_restores_binding_even_when_disk_url_has_v1() {
        let mut store = Store::default();
        store.providers.push(Provider {
            id: "prov_1".into(),
            name: "baqi".into(),
            base_url: "https://ai.example.com".into(),
            protocol: Protocol::OpenaiCompletions,
            enabled: true,
            notes: String::new(),
            secret_ref: "sec_1".into(),
            created_at: "now".into(),
            updated_at: "now".into(),
        });
        store.models.push(Model {
            id: "mdl_1".into(),
            provider_id: "prov_1".into(),
            model_id: "glm-test".into(),
            display_name: "GLM Test".into(),
            created_at: "now".into(),
            updated_at: "now".into(),
        });
        let disk_block = json!({
            "baseUrl": "https://ai.example.com/v1",
            "_modelhub": { "managed": true, "providerId": "prov_1" }
        });

        let matched = match_managed_provider_model(&store, Some(&disk_block), "glm-test");

        assert_eq!(matched, Some((Some("prov_1".into()), Some("mdl_1".into()))));
    }

    #[test]
    fn codex_provider_matches_store_url_without_written_v1_suffix() {
        let mut store = Store::default();
        store.providers.push(Provider {
            id: "prov_zzzcoding".into(),
            name: "zzzcoding".into(),
            base_url: "https://api.zzzcoding.org".into(),
            protocol: Protocol::OpenaiResponses,
            enabled: true,
            notes: String::new(),
            secret_ref: "sec_zzzcoding".into(),
            created_at: "now".into(),
            updated_at: "now".into(),
        });
        store.models.push(Model {
            id: "mdl_sol".into(),
            provider_id: "prov_zzzcoding".into(),
            model_id: "gpt-5.6-sol".into(),
            display_name: "gpt-5.6-sol".into(),
            created_at: "now".into(),
            updated_at: "now".into(),
        });

        let matched = match_provider_model(
            &store,
            "https://api.zzzcoding.org/v1",
            Protocol::OpenaiResponses,
            Some("gpt-5.6-sol"),
            true,
        );

        assert_eq!(
            matched,
            (Some("prov_zzzcoding".into()), Some("mdl_sol".into()))
        );
    }

    #[test]
    fn match_provider_model_prefers_exact_protocol_regardless_of_order() {
        // Two providers share the same base URL but differ by protocol. The
        // disk block is responses. The completions provider sorts first in the
        // store. The old single-`.find()` loose branch would wrongly return the
        // completions provider; the two-pass match must pick responses.
        let mut store = Store::default();
        store.providers.push(Provider {
            id: "prov_completions".into(),
            name: "Completions".into(),
            base_url: "https://shared.example.com".into(),
            protocol: Protocol::OpenaiCompletions,
            enabled: true,
            notes: String::new(),
            secret_ref: "sec_c".into(),
            created_at: "now".into(),
            updated_at: "now".into(),
        });
        store.providers.push(Provider {
            id: "prov_responses".into(),
            name: "Responses".into(),
            base_url: "https://shared.example.com".into(),
            protocol: Protocol::OpenaiResponses,
            enabled: true,
            notes: String::new(),
            secret_ref: "sec_r".into(),
            created_at: "now".into(),
            updated_at: "now".into(),
        });

        let matched = match_provider_model(
            &store,
            "https://shared.example.com",
            Protocol::OpenaiResponses,
            None,
            true,
        );

        assert_eq!(matched.0.as_deref(), Some("prov_responses"));
    }
}
