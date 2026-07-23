use anyhow::{Context, Result};
use serde_json::Value;

use super::backup_before_write;
use super::util::{ensure_object, read_json_value, remove_path, set_string_path, write_json_value};
use crate::paths::{ModelHubPaths, ModelHubPaths as Paths};
use crate::store::{
    find_provider, resolve_upstream_model_id, AgentMode, AppConfig, ApplyAgentResult, Secrets,
    Store, StoreService,
};

pub fn apply(
    svc: &StoreService,
    paths: &ModelHubPaths,
    config: &AppConfig,
    store: &Store,
    secrets: &Secrets,
    keep: u32,
) -> Result<ApplyAgentResult> {
    let file = Paths::claude_settings(&config.paths)?;
    backup_before_write(paths, "claude", &file, keep)?;

    let mut root = read_json_value(&file)?;
    let obj = ensure_object(&mut root)?;

    match store.agent_bindings.claude.mode {
        AgentMode::Official => {
            if let Some(Value::Object(env)) = obj.get_mut("env") {
                env.remove("ANTHROPIC_BASE_URL");
                env.remove("ANTHROPIC_AUTH_TOKEN");
                env.remove("ANTHROPIC_API_KEY");
                env.remove("ANTHROPIC_MODEL");
                env.remove("ANTHROPIC_DEFAULT_HAIKU_MODEL");
                env.remove("ANTHROPIC_DEFAULT_SONNET_MODEL");
                env.remove("ANTHROPIC_DEFAULT_OPUS_MODEL");
            }
            obj.remove("model");
            obj.remove("_modelhub");
            write_json_value(&file, &root)?;
            return Ok(ApplyAgentResult {
                agent: "claude".into(),
                ok: true,
                message: "已切换为官方模式（已清除第三方 BASE_URL/TOKEN）".into(),
                files: vec![file.display().to_string()],
                restart_required: true,
            });
        }
        AgentMode::ThirdParty => {}
    }

    let provider_id = store
        .agent_bindings
        .claude
        .provider_id
        .as_deref()
        .context("Claude 未选择 Provider")?;
    let model_rec_id = store
        .agent_bindings
        .claude
        .model_id
        .as_deref()
        .context("Claude 未选择 Model")?;
    let provider = find_provider(store, provider_id).context("Claude Provider 不存在")?;
    let model_id = resolve_upstream_model_id(store, model_rec_id).context("Claude Model 不存在")?;
    let api_key = secrets
        .secrets
        .get(&provider.secret_ref)
        .map(|s| s.api_key.clone())
        .context("Claude Provider 密钥不存在")?;

    set_string_path(
        obj,
        &["env", "ANTHROPIC_BASE_URL"],
        provider.base_url.clone(),
    );
    set_string_path(obj, &["env", "ANTHROPIC_AUTH_TOKEN"], api_key);
    // avoid dual auth confusion
    remove_path(obj, &["env", "ANTHROPIC_API_KEY"]);
    set_string_path(obj, &["env", "ANTHROPIC_MODEL"], model_id.clone());
    obj.insert("model".into(), Value::String(model_id.clone()));

    if let Some(id) = store.agent_bindings.claude.haiku_model_id.as_deref() {
        if let Some(m) = resolve_upstream_model_id(store, id) {
            set_string_path(obj, &["env", "ANTHROPIC_DEFAULT_HAIKU_MODEL"], m);
        }
    }
    if let Some(id) = store.agent_bindings.claude.sonnet_model_id.as_deref() {
        if let Some(m) = resolve_upstream_model_id(store, id) {
            set_string_path(obj, &["env", "ANTHROPIC_DEFAULT_SONNET_MODEL"], m);
        }
    }
    if let Some(id) = store.agent_bindings.claude.opus_model_id.as_deref() {
        if let Some(m) = resolve_upstream_model_id(store, id) {
            set_string_path(obj, &["env", "ANTHROPIC_DEFAULT_OPUS_MODEL"], m);
        }
    }

    obj.remove("_modelhub");

    write_json_value(&file, &root)?;
    let _ = svc;
    Ok(ApplyAgentResult {
        agent: "claude".into(),
        ok: true,
        message: format!(
            "已写入 Claude：{} / {}（仅改 settings.json 的 env/model）",
            provider.name, model_id
        ),
        files: vec![file.display().to_string()],
        restart_required: true,
    })
}
