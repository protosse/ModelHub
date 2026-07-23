use anyhow::{Context, Result};
use fs_err as fs;
use toml::value::{Table, Value};

use super::backup_before_write;
use crate::paths::{ModelHubPaths, ModelHubPaths as Paths};
use crate::store::{
    find_provider, resolve_upstream_model_id, AgentMode, AppConfig, ApplyAgentResult, Protocol,
    Secrets, Store, StoreService,
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
    backup_before_write(paths, "codex", &file, keep)?;

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
            remove_managed_provider(table, MANAGED_KEY_DEFAULT);
            remove_managed_provider(table, &store.agent_bindings.codex.provider_key);
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

    let mut warn = String::new();
    if provider.protocol != Protocol::OpenaiResponses {
        warn = format!(
            "警告：Provider 协议为 {}，Codex 通常需要 openai-responses。",
            provider.protocol.as_str()
        );
    }

    remove_managed_provider(table, MANAGED_KEY_DEFAULT);
    remove_managed_provider(table, &provider_key);

    table.insert("model".into(), Value::String(model_id.clone()));
    table.insert(
        "model_provider".into(),
        Value::String(provider_key.clone()),
    );

    let providers = table
        .entry("model_providers".to_string())
        .or_insert_with(|| Value::Table(Table::new()))
        .as_table_mut()
        .context("model_providers must be table")?;

    let mut block = Table::new();
    block.insert("name".into(), Value::String(provider.name.clone()));
    block.insert("base_url".into(), Value::String(provider.base_url.clone()));
    block.insert("wire_api".into(), Value::String("responses".into()));
    // Provider-scoped key; leave auth.json alone (scheme B / cc-switch preserve path).
    block.insert(
        "experimental_bearer_token".into(),
        Value::String(api_key),
    );
    providers.insert(provider_key.clone(), Value::Table(block));

    write_toml_atomic(&file, &root)?;
    let _ = paths;

    let mut message = format!(
        "已写入 Codex Active：{} / {}（key 在 config.toml experimental_bearer_token，未改 auth.json）",
        provider.name, model_id
    );
    if !warn.is_empty() {
        message = format!("{warn} {message}");
    }
    message.push_str(" 请重启 Codex 后生效。");

    Ok(ApplyAgentResult {
        agent: "codex".into(),
        ok: true,
        message,
        files: vec![file.display().to_string()],
        restart_required: true,
    })
}

fn remove_managed_provider(table: &mut Table, key: &str) {
    if key.is_empty() {
        return;
    }
    if let Some(Value::Table(providers)) = table.get_mut("model_providers") {
        providers.remove(key);
    }
}

fn write_toml_atomic(path: &std::path::Path, value: &Value) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let text = toml::to_string_pretty(value).context("serialize toml")?;
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, text)?;
    fs::rename(&tmp, path)?;
    Ok(())
}
