use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::util::{
    find_existing_provider_entry, is_modelhub_managed_provider, read_json_value,
};
use crate::paths::ModelHubPaths as Paths;
use crate::store::{
    agent_write_base_url, assign_catalog_write_keys, find_provider, resolve_upstream_model_id,
    AgentMode, AppConfig, Secrets, Store, StoreService,
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
            "opencode" => out.push(preview_opencode(svc, config, store, secrets)?),
            "pi" => out.push(preview_pi(svc, config, store, secrets)?),
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
                kind: "same".into(),
                text: "= [model_providers.*] 原有 Provider 块保留".into(),
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
            let new_base = provider
                .map(|p| agent_write_base_url(&p.base_url, &p.protocol))
                .unwrap_or_default();
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
                &new_base,
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
        note: "只更新当前绑定与所选 Provider 的受管字段；其它 [model_providers.*] 及所选块的扩展字段保留".into(),
    })
}

fn preview_opencode(
    svc: &StoreService,
    config: &AppConfig,
    store: &Store,
    secrets: &Secrets,
) -> Result<AgentDiff> {
    let file = Paths::opencode_config(&config.paths)?;
    let auth_file = Paths::opencode_auth(&config.paths)?;
    let current = if file.exists() {
        read_json_value(&file).unwrap_or(Value::Object(Default::default()))
    } else {
        Value::Object(Default::default())
    };
    let auth = if auth_file.exists() {
        read_json_value(&auth_file).unwrap_or(Value::Object(Default::default()))
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

    let enabled = svc.catalog_providers_with_models(store, "opencode");
    let existing = current
        .get("provider")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default();
    // Same key map apply uses (name slug, de-duped) so preview matches disk.
    let key_map = assign_catalog_write_keys(
        &enabled.iter().map(|(p, _)| p.clone()).collect::<Vec<_>>(),
    );
    // Apply only writes `model` when BOTH provider and model are set; otherwise
    // leave disk value alone. Mirror that so an unset binding is not a phantom change.
    let active = match (
        store
            .agent_bindings
            .opencode
            .provider_id
            .as_deref()
            .and_then(|pid| key_map.get(pid).cloned()),
        store
            .agent_bindings
            .opencode
            .model_id
            .as_deref()
            .and_then(|mid| resolve_upstream_model_id(store, mid)),
    ) {
        (Some(key), Some(m)) => format!("{key}/{m}"),
        _ => cur_model.clone(),
    };

    let mut lines = vec![
        DiffLine {
            kind: "same".into(),
            text: format!(
                "同步目录：{} 个 Provider（文件现有 {cur_provider_count} 个 provider 条目）",
                enabled.len()
            ),
        },
        chg("model", &cur_model, &active),
    ];
    let mut write_keys = std::collections::HashSet::new();
    for (p, models) in &enabled {
        let key = key_map.get(&p.id).cloned().unwrap_or_default();
        write_keys.insert(key.clone());
        let matched = find_existing_provider_entry(&existing, &p.id, &key);
        if let Some((disk_key, _)) = matched {
            write_keys.insert(disk_key.to_string());
            if disk_key != key {
                lines.push(DiffLine {
                    kind: "change".into(),
                    text: format!("~ provider key `{disk_key}` → `{key}`（保留原有扩展配置）"),
                });
            }
        }
        lines.push(DiffLine {
            kind: "same".into(),
            text: format!(
                "provider key `{key}` ← {} ({}) 模型 {}",
                p.name,
                p.base_url,
                models.len()
            ),
        });
        // Disk model ids for this provider block: `provider[key].models` keys.
        let disk_ids: std::collections::HashSet<String> = matched
            .map(|(_, value)| value)
            .and_then(|v| v.get("models"))
            .and_then(|v| v.as_object())
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default();
        push_model_diff_lines(&mut lines, &disk_ids, models);

        // Key lives in auth.json (preferred) or legacy options.apiKey on the block.
        let new_key = secrets
            .secrets
            .get(&p.secret_ref)
            .map(|s| s.api_key.as_str())
            .unwrap_or("");
        let disk_key_name = matched.map(|(k, _)| k).unwrap_or(key.as_str());
        let cur_token = opencode_disk_token(&auth, matched.map(|(_, v)| v), disk_key_name, &key);
        push_secret_diff_line(
            &mut lines,
            &format!("auth.json[{key}]"),
            &cur_token,
            new_key,
        );
    }
    push_orphan_block_lines(&mut lines, &existing, &write_keys);

    Ok(AgentDiff {
        agent: "opencode".into(),
        file: format!("{} + {}", file.display(), auth_file.display()),
        lines,
        note: "同步目录控制 ModelHub 管理块；匹配 Provider/模型保留原有扩展配置，退出目录的受管块删除，未受管 Provider 与 mcp/plugin 等其它字段保留；密钥写入 auth.json".into(),
    })
}

/// (removed: superseded by push_model_diff_lines)
#[allow(dead_code)]
fn _append_model_diff_removed(lines: &mut Vec<DiffLine>, disk_ids: &[String], new_ids: &[String]) {
    let disk: std::collections::HashSet<&str> = disk_ids.iter().map(|s| s.as_str()).collect();
    let next: std::collections::HashSet<&str> = new_ids.iter().map(|s| s.as_str()).collect();
    for id in new_ids {
        if !disk.contains(id.as_str()) {
            lines.push(DiffLine {
                kind: "add".into(),
                text: format!("  + 模型 {id}"),
            });
        }
    }
    for id in disk_ids {
        if !next.contains(id.as_str()) {
            lines.push(DiffLine {
                kind: "remove".into(),
                text: format!("  - 模型 {id}"),
            });
        }
    }
}

fn preview_pi(
    svc: &StoreService,
    config: &AppConfig,
    store: &Store,
    secrets: &Secrets,
) -> Result<AgentDiff> {
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

    let enabled = svc.catalog_providers_with_models(store, "pi");
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
    // Same key map apply uses (name slug, de-duped) so preview matches disk.
    let key_map = assign_catalog_write_keys(
        &enabled.iter().map(|(p, _)| p.clone()).collect::<Vec<_>>(),
    );
    // Apply only writes defaultProvider/defaultModel when BOTH are set in the
    // draft; otherwise it leaves the disk values untouched. Mirror that here so
    // an unset binding shows "unchanged" instead of a phantom "→ —" change.
    let (new_p, new_m) = match (
        store
            .agent_bindings
            .pi
            .provider_id
            .as_deref()
            .and_then(|id| key_map.get(id).cloned()),
        store
            .agent_bindings
            .pi
            .model_id
            .as_deref()
            .and_then(|id| resolve_upstream_model_id(store, id)),
    ) {
        (Some(p), Some(m)) => (p, m),
        _ => (cur_p.clone(), cur_m.clone()),
    };

    let mut lines = vec![
        chg("defaultProvider", &cur_p, &new_p),
        chg("defaultModel", &cur_m, &new_m),
        DiffLine {
            kind: "same".into(),
            text: format!("models.json: 同步 {} 个 Provider", enabled.len()),
        },
    ];
    let mut write_keys = std::collections::HashSet::new();
    for (p, models) in &enabled {
        let key = key_map.get(&p.id).cloned().unwrap_or_default();
        write_keys.insert(key.clone());
        let matched = find_existing_provider_entry(&existing, &p.id, &key);
        if let Some((disk_key, _)) = matched {
            write_keys.insert(disk_key.to_string());
            if disk_key != key {
                lines.push(DiffLine {
                    kind: "change".into(),
                    text: format!("~ provider key `{disk_key}` → `{key}`（保留原有扩展配置）"),
                });
            }
        }
        lines.push(DiffLine {
            kind: "same".into(),
            text: format!(
                "provider key `{key}` ← {} ({}) 模型 {}",
                p.name,
                p.base_url,
                models.len()
            ),
        });
        // Model-level diff: disk block stores models as an array of {id, ...}.
        let disk_ids: std::collections::HashSet<String> = matched
            .map(|(_, value)| value)
            .and_then(|v| v.get("models"))
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|m| m.get("id").and_then(|v| v.as_str()).map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        push_model_diff_lines(&mut lines, &disk_ids, models);

        let new_key = secrets
            .secrets
            .get(&p.secret_ref)
            .map(|s| s.api_key.as_str())
            .unwrap_or("");
        let cur_token = matched
            .map(|(_, value)| value)
            .and_then(|v| v.get("apiKey"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        push_secret_diff_line(
            &mut lines,
            &format!("providers.{key}.apiKey"),
            cur_token,
            new_key,
        );
    }
    push_orphan_block_lines(&mut lines, &existing, &write_keys);

    Ok(AgentDiff {
        agent: "pi".into(),
        file: format!(
            "{} + {}",
            models_file.display(),
            settings_file.display()
        ),
        lines,
        note: "同步目录控制 ModelHub 管理块；匹配 Provider/模型保留 headers、compat 等扩展配置，退出目录的受管块删除，未受管 Provider 保留".into(),
    })
}

/// Append per-model add/remove lines comparing the disk model-id set against the
/// models about to be written. `disk_ids` are upstream model ids currently on
/// disk for this provider block; `writing` are the store models we'll write.
fn push_model_diff_lines(
    lines: &mut Vec<DiffLine>,
    disk_ids: &std::collections::HashSet<String>,
    writing: &[crate::store::Model],
) {
    let write_ids: std::collections::HashSet<&str> =
        writing.iter().map(|m| m.model_id.as_str()).collect();
    for m in writing {
        if !disk_ids.contains(&m.model_id) {
            lines.push(DiffLine {
                kind: "add".into(),
                text: format!("  + 模型 {}", m.model_id),
            });
        }
    }
    let mut removed: Vec<&String> = disk_ids
        .iter()
        .filter(|id| !write_ids.contains(id.as_str()))
        .collect();
    removed.sort();
    for id in removed {
        lines.push(DiffLine {
            kind: "remove".into(),
            text: format!("  - 模型 {id}（不再同步 → 将删除）"),
        });
    }
}

/// Report stale ModelHub-managed provider blocks that are no longer in the sync
/// catalog. Native/unmanaged blocks are outside ModelHub's ownership and remain.
fn push_orphan_block_lines(
    lines: &mut Vec<DiffLine>,
    existing: &serde_json::Map<String, Value>,
    writing_keys: &std::collections::HashSet<String>,
) {
    let mut orphans: Vec<&String> = existing
        .iter()
        .filter(|(k, value)| {
            !writing_keys.contains(k.as_str()) && is_modelhub_managed_provider(value)
        })
        .map(|(k, _)| k)
        .collect();
    orphans.sort();
    for key in orphans {
        lines.push(DiffLine {
            kind: "remove".into(),
            text: format!("- provider `{key}`（不在同步目录 → 将删除）"),
        });
    }
}


/// Resolve OpenCode's effective disk token for a catalog provider.
/// Prefer auth.json under the disk key (or the target write key); fall back to
/// legacy inline `options.apiKey` on the provider block (which Apply clears).
fn opencode_disk_token(
    auth: &Value,
    block: Option<&Value>,
    disk_key: &str,
    write_key: &str,
) -> String {
    let from_auth = |key: &str| -> Option<String> {
        auth.get(key)
            .and_then(|v| v.get("key"))
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
    };
    if let Some(t) = from_auth(disk_key) {
        return t;
    }
    if disk_key != write_key {
        if let Some(t) = from_auth(write_key) {
            return t;
        }
    }
    block
        .and_then(|v| v.get("options"))
        .and_then(|v| v.get("apiKey"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

/// Masked secret line: compares real tokens, never prints them.
fn push_secret_diff_line(lines: &mut Vec<DiffLine>, field: &str, cur: &str, new: &str) {
    if new.is_empty() {
        if cur.is_empty() {
            lines.push(DiffLine {
                kind: "same".into(),
                text: format!("= {field}: (empty)"),
            });
        } else {
            lines.push(DiffLine {
                kind: "remove".into(),
                text: format!("! {field}: API Key missing in ModelHub"),
            });
        }
    } else if cur.is_empty() {
        lines.push(DiffLine {
            kind: "add".into(),
            text: format!("+ {field} = ***"),
        });
    } else if cur != new {
        lines.push(DiffLine {
            kind: "change".into(),
            text: format!("~ {field} = *** (changed)"),
        });
    } else {
        lines.push(DiffLine {
            kind: "same".into(),
            text: format!("= {field}: unchanged"),
        });
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn model_removed_from_catalog_is_a_real_preview_change() {
        let disk_ids = std::collections::HashSet::from(["removed-model".to_string()]);
        let mut lines = Vec::new();

        push_model_diff_lines(&mut lines, &disk_ids, &[]);

        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].kind, "remove");
        assert!(lines[0].text.contains("removed-model"));
    }

    #[test]
    fn secret_diff_reports_key_only_changes() {
        let mut lines = Vec::new();
        push_secret_diff_line(&mut lines, "auth.json[foo]", "old-key", "new-key");
        assert_eq!(lines[0].kind, "change");
        assert!(lines[0].text.contains("***"));
        assert!(!lines[0].text.contains("old-key"));

        let mut lines = Vec::new();
        push_secret_diff_line(&mut lines, "auth.json[foo]", "same", "same");
        assert_eq!(lines[0].kind, "same");
        assert!(lines[0].text.contains("unchanged"));

        let mut lines = Vec::new();
        push_secret_diff_line(&mut lines, "auth.json[foo]", "", "fresh");
        assert_eq!(lines[0].kind, "add");
    }

    #[test]
    fn opencode_disk_token_prefers_auth_then_inline() {
        let auth = json!({
            "foo": { "type": "api", "key": "from-auth" }
        });
        let block = json!({
            "options": { "apiKey": "from-inline" }
        });
        assert_eq!(
            opencode_disk_token(&auth, Some(&block), "foo", "foo"),
            "from-auth"
        );
        let empty_auth = json!({});
        assert_eq!(
            opencode_disk_token(&empty_auth, Some(&block), "foo", "foo"),
            "from-inline"
        );
        // When the block was under an old slug, still accept auth under write key.
        let auth2 = json!({
            "new-slug": { "type": "api", "key": "renamed-auth" }
        });
        assert_eq!(
            opencode_disk_token(&auth2, Some(&block), "old-slug", "new-slug"),
            "renamed-auth"
        );
    }
}
