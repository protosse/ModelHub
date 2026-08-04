use std::sync::{LazyLock, Mutex, MutexGuard};

use crate::adapters;
use crate::backup::{self, BackupEntry, BackupSnapshotRef, RestoreBackupResult};
use crate::paths::ModelHubPaths;
use crate::store::{
    AgentBindings, AgentMode, ApplyRequest, ApplyResult, CatalogEntry, FetchModelsInput, FullState,
    ImportPreview, ImportRequest, ImportResult, Model, ModelInput, ModelTestResult, Provider,
    ProviderInput, QuickAddRequest, QuickAddResult, RemoteModel, StoreService,
    TestConnectionRequest, TestConnectionResult, TestPrompt, TestPromptInput,
};

static COMMAND_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

fn svc() -> Result<(StoreService, ModelHubPaths, MutexGuard<'static, ()>), String> {
    let guard = COMMAND_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let paths = ModelHubPaths::default_location().map_err(|e| e.to_string())?;
    let svc = StoreService::new(paths.clone());
    svc.ensure_dirs().map_err(|e| e.to_string())?;
    Ok((svc, paths, guard))
}

#[tauri::command]
pub fn get_state() -> Result<FullState, String> {
    let (svc, _, _guard) = svc()?;
    svc.full_state().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_backup_keep_count(backup_keep_count: u32) -> Result<(), String> {
    let (svc, _, _guard) = svc()?;
    let mut config = svc.load_config().map_err(|e| e.to_string())?;
    config.backup_keep_count = backup_keep_count;
    svc.save_config(&config).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn add_provider(input: ProviderInput) -> Result<Provider, String> {
    let (svc, _, _guard) = svc()?;
    svc.add_provider(input).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_provider(id: String, input: ProviderInput) -> Result<Provider, String> {
    let (svc, _, _guard) = svc()?;
    svc.update_provider(&id, input).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_provider(id: String) -> Result<(), String> {
    let (svc, _, _guard) = svc()?;
    svc.delete_provider(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn clone_provider(
    id: String,
    new_name: String,
    new_api_key: String,
) -> Result<Provider, String> {
    let (svc, _, _guard) = svc()?;
    svc.clone_provider(&id, &new_name, &new_api_key)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn add_model(input: ModelInput) -> Result<Model, String> {
    let (svc, _, _guard) = svc()?;
    svc.add_model(input).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn add_models(inputs: Vec<ModelInput>) -> Result<Vec<Model>, String> {
    let (svc, _, _guard) = svc()?;
    svc.add_models(inputs).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_model(id: String, input: ModelInput) -> Result<Model, String> {
    let (svc, _, _guard) = svc()?;
    svc.update_model(&id, input).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_model(id: String) -> Result<(), String> {
    let (svc, _, _guard) = svc()?;
    svc.delete_model(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_models(ids: Vec<String>) -> Result<usize, String> {
    let (svc, _, _guard) = svc()?;
    svc.delete_models(&ids).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn read_live_bindings() -> Result<AgentBindings, String> {
    let (svc, _, _guard) = svc()?;
    let config = svc.load_config().map_err(|e| e.to_string())?;
    adapters::read_live_bindings(&svc, &config).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn apply_config(request: ApplyRequest) -> Result<ApplyResult, String> {
    let (svc, paths, _guard) = svc()?;
    adapters::apply_all(&svc, &paths, request).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn preview_apply(request: ApplyRequest) -> Result<adapters::ApplyPreview, String> {
    let (svc, _, _guard) = svc()?;
    let config = svc.load_config().map_err(|e| e.to_string())?;
    let mut store = svc.load_store().map_err(|e| e.to_string())?;
    if let Some(bindings) = request.bindings {
        store.agent_bindings = bindings;
    }
    let secrets = svc.load_secrets().map_err(|e| e.to_string())?;
    adapters::preview_apply(&svc, &config, &store, &secrets, &request.agents)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn preview_import() -> Result<ImportPreview, String> {
    let (svc, _, _guard) = svc()?;
    let config = svc.load_config().map_err(|e| e.to_string())?;
    adapters::preview_import(&svc, &config).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn run_import(request: ImportRequest) -> Result<ImportResult, String> {
    let (svc, _, _guard) = svc()?;
    let config = svc.load_config().map_err(|e| e.to_string())?;
    adapters::import_from_agents(&svc, &config, &request).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_backups() -> Result<Vec<BackupEntry>, String> {
    let (_svc, paths, _guard) = svc()?;
    backup::list_backups(&paths).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_backups(items: Vec<BackupSnapshotRef>) -> Result<usize, String> {
    let (_svc, paths, _guard) = svc()?;
    backup::delete_snapshots(&paths, &items).map_err(|e| e.to_string())
}

/// Restore one backup snapshot (agent + stamp) onto current live Agent paths.
/// Creates a safety backup of current live files first.
#[tauri::command]
pub fn restore_backup(agent: String, stamp: String) -> Result<RestoreBackupResult, String> {
    let (svc, paths, _guard) = svc()?;
    let config = svc.load_config().map_err(|e| e.to_string())?;
    backup::restore_snapshot(&paths, &config, &agent, &stamp).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn reveal_api_key(secret_ref: String) -> Result<String, String> {
    let (svc, _, _guard) = svc()?;
    svc.get_api_key(&secret_ref).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn fetch_provider_models(provider_id: String) -> Result<Vec<RemoteModel>, String> {
    let (store, secrets) = {
        let (svc, _, _guard) = svc()?;
        (
            svc.load_store().map_err(|e| e.to_string())?,
            svc.load_secrets().map_err(|e| e.to_string())?,
        )
    };
    adapters::fetch_remote_models(&store, &secrets, &provider_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn fetch_models_from_provider_input(
    input: FetchModelsInput,
) -> Result<Vec<RemoteModel>, String> {
    adapters::fetch_remote_models_from_input(&input.base_url, &input.protocol, &input.api_key)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn quick_add_and_apply(request: QuickAddRequest) -> Result<QuickAddResult, String> {
    let (svc, paths, _guard) = svc()?;
    let valid_agents = ["claude", "codex", "opencode", "pi"];
    if request.agents.is_empty() {
        return Err("请至少选择一个 Agent".into());
    }
    if let Some(agent) = request
        .agents
        .iter()
        .find(|agent| !valid_agents.contains(&agent.as_str()))
    {
        return Err(format!("unknown agent: {agent}"));
    }

    let config = svc.load_config().map_err(|e| e.to_string())?;
    let mut bindings = match request.bindings {
        Some(bindings) => bindings,
        None => adapters::read_live_bindings(&svc, &config).map_err(|e| e.to_string())?,
    };
    let default_model_id = request.default_model_id.trim().to_string();
    if default_model_id.is_empty()
        || !request
            .models
            .iter()
            .any(|model| model.model_id.trim() == default_model_id)
    {
        return Err("默认模型不在待添加模型中".into());
    }
    let agents = request.agents;
    let (provider, models) = svc
        .quick_add_provider(request.provider, request.models, &agents)
        .map_err(|e| e.to_string())?;
    let model_row_id = models
        .iter()
        .find(|model| model.model_id == default_model_id)
        .map(|model| model.id.clone())
        .ok_or_else(|| "默认模型不在待添加模型中".to_string())?;
    let selected = |agent: &str| agents.iter().any(|item| item == agent);

    if selected("claude") {
        bindings.claude.mode = AgentMode::ThirdParty;
        bindings.claude.provider_id = Some(provider.id.clone());
        bindings.claude.model_id = Some(model_row_id.clone());
    }
    if selected("codex") {
        bindings.codex.mode = AgentMode::ThirdParty;
        bindings.codex.provider_id = Some(provider.id.clone());
        bindings.codex.model_id = Some(model_row_id.clone());
    }
    if selected("opencode") {
        bindings.opencode.provider_id = Some(provider.id.clone());
        bindings.opencode.model_id = Some(model_row_id.clone());
    }
    if selected("pi") {
        bindings.pi.provider_id = Some(provider.id.clone());
        bindings.pi.model_id = Some(model_row_id);
    }

    let apply = adapters::apply_all(
        &svc,
        &paths,
        ApplyRequest {
            agents,
            bindings: Some(bindings.clone()),
        },
    )
    .map_err(|e| e.to_string())?;

    Ok(QuickAddResult {
        provider,
        models,
        bindings,
        apply,
    })
}

#[tauri::command]
pub fn set_agent_catalog(agent: String, entries: Vec<CatalogEntry>) -> Result<(), String> {
    let (svc, _, _guard) = svc()?;
    svc.set_agent_catalog(&agent, &entries)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_providers(ids: Vec<String>) -> Result<usize, String> {
    let (svc, _, _guard) = svc()?;
    svc.delete_providers(&ids).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_test_prompts() -> Result<Vec<TestPrompt>, String> {
    let (svc, _, _guard) = svc()?;
    svc.list_test_prompts().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn upsert_test_prompt(input: TestPromptInput) -> Result<TestPrompt, String> {
    let (svc, _, _guard) = svc()?;
    svc.upsert_test_prompt(input).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_test_prompt(id: String) -> Result<(), String> {
    let (svc, _, _guard) = svc()?;
    svc.delete_test_prompt(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_default_test_prompt(id: String) -> Result<TestPrompt, String> {
    let (svc, _, _guard) = svc()?;
    svc.set_default_test_prompt(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn record_model_test_result(
    model_id: String,
    ok: bool,
    latency_ms: Option<u64>,
    tested_at: Option<String>,
) -> Result<ModelTestResult, String> {
    let (svc, _, _guard) = svc()?;
    svc.record_model_test_result(&model_id, ok, latency_ms, tested_at)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn test_model_connection(
    app: tauri::AppHandle,
    request: TestConnectionRequest,
) -> Result<TestConnectionResult, String> {
    let (store, secrets) = {
        let (svc, _, _guard) = svc()?;
        (
            svc.load_store().map_err(|e| e.to_string())?,
            svc.load_secrets().map_err(|e| e.to_string())?,
        )
    };
    let run_id = request
        .run_id
        .clone()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    adapters::test_model_connection(adapters::TestConnectionParams {
        app: Some(app),
        run_id: &run_id,
        store: &store,
        secrets: &secrets,
        model_row_id: &request.model_id,
        prompt: &request.prompt,
        timeout_secs: request.timeout_secs,
        extra_headers: request.extra_headers.as_ref(),
    })
    .await
    .map_err(|e| e.to_string())
}
