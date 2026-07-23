use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::util::read_json_value;
use crate::paths::ModelHubPaths as Paths;
use crate::store::{
    find_provider, resolve_provider_write_key, resolve_upstream_model_id, AgentMode, AppConfig,
    Secrets, Store, StoreService,
};
use fs_err as fs;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffLine {
    pub kind: String, // same | add | remove | change
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentDiff {
    pub agent: String,
    pub file: String,
    pub lines: Vec<DiffLine>,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyPreview {
    pub agents: Vec<AgentDiff>,
}

pub fn preview_apply(
    svc: &StoreService,
    config: &AppConfig,
    store: &Store,
    secrets: &Secrets,
    agents: &[String],
) -> Result<ApplyPreview> {
    let selected: Vec<&str> = if agents.is_empty() {
        vec!["claude", "codex", "opencode", "pi"]
    } else {
        agents.iter().map(|s| s.as_str()).collect()
    };

    let mut out = Vec::new();
    for agent in selected {
        match agent {
            "claude" => out.push(preview_claude(config, store, secrets)?),
            "codex" => out.push(preview_codex(config, store, secrets)?),
            "opencode" => out.push(preview_opencode(svc, config, store)?),
            "pi" => out.push(preview_pi(svc, config, store)?),
            _ => {}
        }
    }
    Ok(ApplyPreview { agents: out })
}

fn preview_claude(config: &AppConfig, store: &Store, secrets: &Secrets) -> Result<AgentDiff> {
    let file = Paths::claude_settings(&config.paths)?;
    let current = if file.exists() {
        read_json_value(&file).unwrap_or(Value::Object(Default::default()))
    } else {
        Value::Object(Default::default())
    };
    let env = current.get("env").cloned().unwrap_or(Value::Object(Default::default()));
    let cur_base = env
        .get("ANTHROPIC_BASE_URL")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let cur_model = current
        .get("model")
        .and_then(|v| v.as_str())
        .or_else(|| env.get("ANTHROPIC_MODEL").and_then(|v| v.as_str()))
        .unwrap_or("")
        .to_string();
    let cur_has_token = env
        .get("ANTHROPIC_AUTH_TOKEN")
        .or_else(|| env.get("ANTHROPIC_API_KEY"))
        .and_then(|v| v.as_str())
        .map(|s| !s.is_empty())
        .unwrap_or(false);

    let mut lines = Vec::new();
    match store.agent_bindings.claude.mode {
        AgentMode::Official => {
            lines.push(chg(
                "mode",
                if cur_base.is_empty() {
                    "official"
                } else {
                    "third_party"
                },
                "official",
            ));
            if !cur_base.is_empty() {
                lines.push(DiffLine {
                    kind: "remove".into(),
                    text: format!("- ANTHROPIC_BASE_URL = {cur_base}"),
                });
            }
            if cur_has_token {
                lines.push(DiffLine {
                    kind: "remove".into(),
                    text: "- ANTHROPIC_AUTH_TOKEN / API_KEY (cleared)".into(),
                });
            }
        }
        AgentMode::ThirdParty => {
            let pid = store.agent_bindings.claude.provider_id.as_deref();
            let mid = store.agent_bindings.claude.model_id.as_deref();
            let provider = pid.and_then(|id| find_provider(store, id));
            let model = mid.and_then(|id| resolve_upstream_model_id(store, id));
            let new_base = provider.map(|p| p.base_url.as_str()).unwrap_or("");
            let new_model = model.as_deref().unwrap_or("");
            let new_name = provider.map(|p| p.name.as_str()).unwrap_or("?");
            let new_key = provider
                .and_then(|p| secrets.secrets.get(&p.secret_ref))
                .map(|s| s.api_key.as_str())
                .unwrap_or("");
            let cur_token = env
                .get("ANTHROPIC_AUTH_TOKEN")
                .or_else(|| env.get("ANTHROPIC_API_KEY"))
                .and_then(|v| v.as_str())
                .unwrap_or("");

            lines.push(DiffLine {
                kind: "same".into(),
                text: format!("provider: {new_name}"),
            });
            lines.push(chg("ANTHROPIC_BASE_URL", &cur_base, new_base));
            lines.push(chg("model", &cur_model, new_model));
            if new_key.is_empty() {
                lines.push(DiffLine {
                    kind: "remove".into(),
                    text: "! API Key missing in ModelHub".into(),
                });
            } else if cur_token.is_empty() {
                lines.push(DiffLine {
                    kind: "add".into(),
                    text: "+ ANTHROPIC_AUTH_TOKEN = ***".into(),
                });
            } else if cur_token != new_key {
                lines.push(DiffLine {
                    kind: "change".into(),
                    text: "~ ANTHROPIC_AUTH_TOKEN = *** (changed)".into(),
                });
            } else {
                lines.push(DiffLine {
                    kind: "same".into(),
                    text: "= ANTHROPIC_AUTH_TOKEN: unchanged".into(),
                });
            }
        }
    }

    Ok(AgentDiff {
        agent: "claude".into(),
        file: file.display().to_string(),
        lines,
        note: "只改 settings.json 的 env/model".into(),
    })
}

fn preview_codex(config: &AppConfig, store: &Store, secrets: &Secrets) -> Result<AgentDiff> {
    let file = Paths::codex_config(&config.paths)?;
    let (cur_provider, cur_model, cur_base) = if file.exists() {
        let text = fs::read_to_string(&file).unwrap_or_default();
        let val = text.parse::<toml::Value>().unwrap_or(toml::Value::Table(Default::default()));
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
        let base = val
            .get("model_providers")
            .and_then(|v| v.get(&mp))
            .and_then(|v| v.get("base_url"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        (mp, model, base)
    } else {
        (String::new(), String::new(), String::new())
    };

    let mut lines = Vec::new();
    match store.agent_bindings.codex.mode {
        AgentMode::Official => {
            lines.push(chg("model_provider", &cur_provider, "openai"));
            lines.push(DiffLine {
                kind: "remove".into(),
                text: "- [model_providers.modelhub] (removed if present)".into(),
            });
        }
        AgentMode::ThirdParty => {
            let pid = store.agent_bindings.codex.provider_id.as_deref();
            let mid = store.agent_bindings.codex.model_id.as_deref();
            let provider = pid.and_then(|id| find_provider(store, id));
            let model = mid.and_then(|id| resolve_upstream_model_id(store, id));
            let key = if store.agent_bindings.codex.provider_key.is_empty() {
                "modelhub"
            } else {
                store.agent_bindings.codex.provider_key.as_str()
            };
            let new_base = provider.map(|p| p.base_url.as_str()).unwrap_or("");
            let new_model = model.as_deref().unwrap_or("");
            let new_name = provider.map(|p| p.name.as_str()).unwrap_or("?");
            let new_key = provider
                .and_then(|p| secrets.secrets.get(&p.secret_ref))
                .map(|s| s.api_key.as_str())
                .unwrap_or("");
            let cur_token = if file.exists() {
                let text = fs::read_to_string(&file).unwrap_or_default();
                text.parse::<toml::Value>()
                    .ok()
                    .and_then(|v| {
                        v.get("model_providers")
                            .and_then(|p| p.get(key))
                            .and_then(|b| b.get("experimental_bearer_token"))
                            .and_then(|t| t.as_str())
                            .map(|s| s.to_string())
                    })
                    .unwrap_or_default()
            } else {
                String::new()
            };

            lines.push(chg("model_provider", &cur_provider, key));
            lines.push(chg("model", &cur_model, new_model));
            lines.push(chg(
                &format!("model_providers.{key}.base_url"),
                &cur_base,
                new_base,
            ));
            lines.push(DiffLine {
                kind: "same".into(),
                text: format!("provider name: {new_name}"),
            });
            if new_key.is_empty() {
                lines.push(DiffLine {
                    kind: "remove".into(),
                    text: "! API Key missing in ModelHub".into(),
                });
            } else if cur_token.is_empty() {
                lines.push(DiffLine {
                    kind: "add".into(),
                    text: format!("+ model_providers.{key}.experimental_bearer_token = ***"),
                });
            } else if cur_token != new_key {
                lines.push(DiffLine {
                    kind: "change".into(),
                    text: format!("~ model_providers.{key}.experimental_bearer_token = *** (changed)"),
                });
            } else {
                lines.push(DiffLine {
                    kind: "same".into(),
                    text: format!("= model_providers.{key}.experimental_bearer_token: unchanged"),
                });
            }
        }
    }

    Ok(AgentDiff {
        agent: "codex".into(),
        file: file.display().to_string(),
        lines,
        note: "其它 [model_providers.*] 会保留；运行时只看 model_provider".into(),
    })
}

fn preview_opencode(svc: &StoreService, config: &AppConfig, store: &Store) -> Result<AgentDiff> {
    let file = Paths::opencode_config(&config.paths)?;
    let current = if file.exists() {
        read_json_value(&file).unwrap_or(Value::Object(Default::default()))
    } else {
        Value::Object(Default::default())
    };
    let cur_model = current
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let cur_provider_count = current
        .get("provider")
        .and_then(|v| v.as_object())
        .map(|m| m.len())
        .unwrap_or(0);

    let enabled = svc.enabled_providers_with_models(store);
    let existing = current
        .get("provider")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default();
    let active = store
        .agent_bindings
        .opencode
        .provider_id
        .as_deref()
        .and_then(|pid| find_provider(store, pid))
        .zip(
            store
                .agent_bindings
                .opencode
                .model_id
                .as_deref()
                .and_then(|mid| resolve_upstream_model_id(store, mid)),
        )
        .map(|(p, m)| {
            let key = resolve_provider_write_key(p, &existing);
            format!("{key}/{m}")
        })
        .unwrap_or_default();

    let mut lines = vec![
        DiffLine {
            kind: "same".into(),
            text: format!(
                "enabled providers to sync: {} (file currently has {cur_provider_count} provider entries)",
                enabled.len()
            ),
        },
        chg("model", &cur_model, &active),
    ];
    for (p, models) in &enabled {
        let key = resolve_provider_write_key(p, &existing);
        lines.push(DiffLine {
            kind: "same".into(),
            text: format!(
                "provider key `{key}` ← {} ({}) models={}",
                p.name,
                p.base_url,
                models.len()
            ),
        });
    }

    Ok(AgentDiff {
        agent: "opencode".into(),
        file: file.display().to_string(),
        lines,
        note: "会合并写入 enabled providers；mcp/plugin 不动".into(),
    })
}

fn preview_pi(svc: &StoreService, config: &AppConfig, store: &Store) -> Result<AgentDiff> {
    let models_file = Paths::pi_models(&config.paths)?;
    let settings_file = Paths::pi_settings(&config.paths)?;
    let settings = if settings_file.exists() {
        read_json_value(&settings_file).unwrap_or(Value::Object(Default::default()))
    } else {
        Value::Object(Default::default())
    };
    let cur_p = settings
        .get("defaultProvider")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let cur_m = settings
        .get("defaultModel")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let enabled = svc.enabled_providers_with_models(store);
    let existing = if models_file.exists() {
        read_json_value(&models_file)
            .ok()
            .and_then(|v| {
                v.get("providers")
                    .and_then(|p| p.as_object())
                    .cloned()
            })
            .unwrap_or_default()
    } else {
        Default::default()
    };
    let new_p = store
        .agent_bindings
        .pi
        .provider_id
        .as_deref()
        .and_then(|id| find_provider(store, id))
        .map(|p| resolve_provider_write_key(p, &existing))
        .unwrap_or_default();
    let new_m = store
        .agent_bindings
        .pi
        .model_id
        .as_deref()
        .and_then(|id| resolve_upstream_model_id(store, id))
        .unwrap_or_default();

    let mut lines = vec![
        chg("defaultProvider", &cur_p, &new_p),
        chg("defaultModel", &cur_m, &new_m),
        DiffLine {
            kind: "same".into(),
            text: format!("models.json: sync {} enabled providers", enabled.len()),
        },
    ];
    for (p, models) in &enabled {
        let key = resolve_provider_write_key(p, &existing);
        lines.push(DiffLine {
            kind: "same".into(),
            text: format!(
                "provider key `{key}` ← {} ({}) models={}",
                p.name,
                p.base_url,
                models.len()
            ),
        });
    }

    Ok(AgentDiff {
        agent: "pi".into(),
        file: format!(
            "{} + {}",
            models_file.display(),
            settings_file.display()
        ),
        lines,
        note: "会合并写入 enabled providers；defaultProvider 优先复用磁盘已有 key".into(),
    })
}

fn chg(field: &str, old: &str, new: &str) -> DiffLine {
    if old == new {
        DiffLine {
            kind: "same".into(),
            text: format!("= {field}: {}", if new.is_empty() { "—" } else { new }),
        }
    } else if old.is_empty() {
        DiffLine {
            kind: "add".into(),
            text: format!("+ {field}: {new}"),
        }
    } else if new.is_empty() {
        DiffLine {
            kind: "remove".into(),
            text: format!("- {field}: {old}"),
        }
    } else {
        DiffLine {
            kind: "change".into(),
            text: format!("~ {field}: {old} → {new}"),
        }
    }
}

