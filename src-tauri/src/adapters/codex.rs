use anyhow::{Context, Result};
use fs_err as fs;
use toml::value::{Table, Value};

use super::backup_before_write;
use crate::backup::new_stamp;
use crate::file_io::write_atomic;
use crate::paths::{ModelHubPaths, ModelHubPaths as Paths};
use crate::store::{
    agent_write_base_url, find_provider, resolve_upstream_model_id, AgentMode, AppConfig,
    ApplyAgentResult, Protocol, Secrets, Store, StoreService,
};

const MANAGED_KEY_DEFAULT: &str = "modelhub";

pub fn apply(
    _svc: &StoreService,
    paths: &ModelHubPaths,
    config: &AppConfig,
    store: &Store,
    secrets: &Secrets,
    keep: u32,
) -> Result<ApplyAgentResult> {
    let file = Paths::codex_config(&config.paths)?;
    let stamp = new_stamp();
    backup_before_write(paths, "codex", &file, keep, &stamp)?;

    let mut root = if file.exists() {
        let text = fs::read_to_string(&file)?;
        text.parse::<Value>().context("parse codex config.toml")?
    } else {
        Value::Table(Table::new())
    };

    let table = root
        .as_table_mut()
        .context("codex config root must be a table")?;

    // Never touch ~/.codex/auth.json (preserve ChatGPT/OAuth login cache).
    table.remove("experimental_bearer_token");

    match store.agent_bindings.codex.mode {
        AgentMode::Official => {
            table.insert("model_provider".into(), Value::String("openai".into()));
            write_toml_atomic(&file, &root)?;
            return Ok(ApplyAgentResult {
                agent: "codex".into(),
                ok: true,
                message: "已切换 Codex 为官方 openai provider（未修改 auth.json）".into(),
                files: vec![file.display().to_string()],
                restart_required: true,
            });
        }
        AgentMode::ThirdParty => {}
    }

    let provider_id = store
        .agent_bindings
        .codex
        .provider_id
        .as_deref()
        .context("Codex 未选择 Provider")?;
    let model_rec_id = store
        .agent_bindings
        .codex
        .model_id
        .as_deref()
        .context("Codex 未选择 Model")?;
    let provider = find_provider(store, provider_id).context("Codex Provider 不存在")?;
    let model_id = resolve_upstream_model_id(store, model_rec_id).context("Codex Model 不存在")?;
    let api_key = secrets
        .secrets
        .get(&provider.secret_ref)
        .map(|s| s.api_key.clone())
        .unwrap_or_default();
    if api_key.trim().is_empty() {
        anyhow::bail!("Codex Provider 未配置 API Key，请先在提供商详情中填写密钥");
    }

    let provider_key = if store.agent_bindings.codex.provider_key.is_empty() {
        MANAGED_KEY_DEFAULT.to_string()
    } else {
        store.agent_bindings.codex.provider_key.clone()
    };

    let wire_api = codex_wire_api(&provider.protocol)?;

    table.insert("model".into(), Value::String(model_id.clone()));
    table.insert("model_provider".into(), Value::String(provider_key.clone()));

    let providers = table
        .entry("model_providers".to_string())
        .or_insert_with(|| Value::Table(Table::new()))
        .as_table_mut()
        .context("model_providers must be table")?;

    let existing = providers.get(&provider_key).and_then(Value::as_table);
    let block = merge_provider_block(existing, provider, api_key, wire_api);
    providers.insert(provider_key.clone(), Value::Table(block));

    write_toml_atomic(&file, &root)?;
    let _ = paths;

    let mut message = format!(
        "已写入 Codex Active：{} / {}（wire_api={wire_api}，key 在 config.toml experimental_bearer_token，未改 auth.json）",
        provider.name, model_id
    );
    message.push_str(" 请重启 Codex 后生效。");

    Ok(ApplyAgentResult {
        agent: "codex".into(),
        ok: true,
        message,
        files: vec![file.display().to_string()],
        restart_required: true,
    })
}

fn codex_wire_api(protocol: &Protocol) -> Result<&'static str> {
    match protocol {
        Protocol::OpenaiCompletions => Ok("chat"),
        Protocol::OpenaiResponses => Ok("responses"),
        Protocol::AnthropicMessages => {
            anyhow::bail!("Codex 不支持 anthropic-messages Provider，请选择 OpenAI 协议")
        }
    }
}

fn merge_provider_block(
    existing: Option<&Table>,
    provider: &crate::store::Provider,
    api_key: String,
    wire_api: &str,
) -> Table {
    let mut block = existing.cloned().unwrap_or_default();
    block.insert("name".into(), Value::String(provider.name.clone()));
    block.insert(
        "base_url".into(),
        Value::String(agent_write_base_url(&provider.base_url, &provider.protocol)),
    );
    block.insert("wire_api".into(), Value::String(wire_api.into()));
    // Provider-scoped key; leave auth.json alone (scheme B / cc-switch preserve path).
    block.insert("experimental_bearer_token".into(), Value::String(api_key));
    block
}

fn write_toml_atomic(path: &std::path::Path, value: &Value) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let text = toml::to_string_pretty(value).context("serialize toml")?;
    write_atomic(path, text.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Provider;

    #[test]
    fn preserves_unknown_fields_in_existing_provider_block() {
        let provider = Provider {
            id: "prov_1".into(),
            name: "Welfare".into(),
            base_url: "https://welfare.example.com".into(),
            protocol: Protocol::OpenaiResponses,
            enabled: true,
            notes: String::new(),
            secret_ref: "sec_1".into(),
            created_at: "now".into(),
            updated_at: "now".into(),
        };
        let existing = toml::toml! {
            base_url = "https://old.example.com/v1"
            wire_api = "responses"
            custom_flag = true
            [http_headers]
            "X-Native" = "keep"
        };

        let merged = merge_provider_block(Some(&existing), &provider, "secret".into(), "responses");

        assert_eq!(
            merged.get("custom_flag").and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(
            merged
                .get("http_headers")
                .and_then(Value::as_table)
                .and_then(|t| t.get("X-Native"))
                .and_then(Value::as_str),
            Some("keep")
        );
        assert_eq!(
            merged.get("base_url").and_then(Value::as_str),
            Some("https://welfare.example.com/v1")
        );
        assert_eq!(
            merged
                .get("experimental_bearer_token")
                .and_then(Value::as_str),
            Some("secret")
        );
    }

    #[test]
    fn completions_provider_uses_chat_wire_api() {
        let provider = Provider {
            id: "prov_1".into(),
            name: "Chat Gateway".into(),
            base_url: "https://chat.example.com".into(),
            protocol: Protocol::OpenaiCompletions,
            enabled: true,
            notes: String::new(),
            secret_ref: "sec_1".into(),
            created_at: "now".into(),
            updated_at: "now".into(),
        };

        assert_eq!(codex_wire_api(&provider.protocol).unwrap(), "chat");
        let merged = merge_provider_block(
            None,
            &provider,
            "secret".into(),
            codex_wire_api(&provider.protocol).unwrap(),
        );

        assert_eq!(merged.get("wire_api").and_then(Value::as_str), Some("chat"));
        assert!(codex_wire_api(&Protocol::AnthropicMessages).is_err());
    }
}
