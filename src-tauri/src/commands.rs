use crate::adapters;
use crate::backup::{self, BackupEntry};
use crate::paths::ModelHubPaths;
use crate::store::{
    AgentBindings, AppConfig, ApplyRequest, ApplyResult, FullState, ImportPreview, ImportRequest,
    ImportResult, Model, ModelInput, ModelTestResult, Provider, ProviderInput, RemoteModel,
    StoreService, TestConnectionRequest, TestConnectionResult, TestPrompt, TestPromptInput,
};

fn svc() -> Result<(StoreService, ModelHubPaths), String> {
    let paths = ModelHubPaths::default_location().map_err(|e| e.to_string())?;
    let svc = StoreService::new(paths.clone());
    svc.ensure_dirs().map_err(|e| e.to_string())?;
    Ok((svc, paths))
}

#[tauri::command]
pub fn get_state() -> Result<FullState, String> {
    let (svc, _) = svc()?;
    svc.full_state().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_app_config(config: AppConfig) -> Result<(), String> {
    let (svc, _) = svc()?;
    svc.save_config(&config).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn add_provider(input: ProviderInput) -> Result<Provider, String> {
    let (svc, _) = svc()?;
    svc.add_provider(input).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_provider(id: String, input: ProviderInput) -> Result<Provider, String> {
    let (svc, _) = svc()?;
    svc.update_provider(&id, input).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_provider(id: String) -> Result<(), String> {
    let (svc, _) = svc()?;
    svc.delete_provider(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn clone_provider(id: String, new_name: String, new_api_key: String) -> Result<Provider, String> {
    let (svc, _) = svc()?;
    svc.clone_provider(&id, &new_name, &new_api_key)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_provider_enabled(id: String, enabled: bool) -> Result<(), String> {
    let (svc, _) = svc()?;
    svc.set_provider_enabled(&id, enabled)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn add_model(input: ModelInput) -> Result<Model, String> {
    let (svc, _) = svc()?;
    svc.add_model(input).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn add_models(inputs: Vec<ModelInput>) -> Result<Vec<Model>, String> {
    let (svc, _) = svc()?;
    svc.add_models(inputs).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_model(id: String, input: ModelInput) -> Result<Model, String> {
    let (svc, _) = svc()?;
    svc.update_model(&id, input).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_model(id: String) -> Result<(), String> {
    let (svc, _) = svc()?;
    svc.delete_model(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_bindings(bindings: AgentBindings) -> Result<(), String> {
    let (svc, _) = svc()?;
    svc.save_bindings(bindings).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn read_live_bindings() -> Result<AgentBindings, String> {
    let (svc, _) = svc()?;
    let config = svc.load_config().map_err(|e| e.to_string())?;
    adapters::read_live_bindings(&svc, &config).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn apply_config(request: ApplyRequest) -> Result<ApplyResult, String> {
    let (svc, paths) = svc()?;
    adapters::apply_all(&svc, &paths, request).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn preview_apply(request: ApplyRequest) -> Result<adapters::ApplyPreview, String> {
    let (svc, _) = svc()?;
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
    let (svc, _) = svc()?;
    let config = svc.load_config().map_err(|e| e.to_string())?;
    adapters::preview_import(&svc, &config).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn run_import(request: ImportRequest) -> Result<ImportResult, String> {
    let (svc, _) = svc()?;
    let config = svc.load_config().map_err(|e| e.to_string())?;
    adapters::import_from_agents(&svc, &config, &request).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_provider_names() -> Result<Vec<String>, String> {
    let (svc, _) = svc()?;
    let store = svc.load_store().map_err(|e| e.to_string())?;
    Ok(store.providers.into_iter().map(|p| p.name).collect())
}

#[tauri::command]
pub fn list_backups() -> Result<Vec<BackupEntry>, String> {
    let (_, paths) = svc()?;
    backup::list_backups(&paths).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn reveal_api_key(secret_ref: String) -> Result<String, String> {
    let (svc, _) = svc()?;
    svc.get_api_key(&secret_ref).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn fetch_provider_models(provider_id: String) -> Result<Vec<RemoteModel>, String> {
    let (svc, _) = svc()?;
    let store = svc.load_store().map_err(|e| e.to_string())?;
    let secrets = svc.load_secrets().map_err(|e| e.to_string())?;
    adapters::fetch_remote_models(&store, &secrets, &provider_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_providers(ids: Vec<String>) -> Result<usize, String> {
    let (svc, _) = svc()?;
    let mut n = 0usize;
    for id in ids {
        svc.delete_provider(&id).map_err(|e| e.to_string())?;
        n += 1;
    }
    Ok(n)
}

#[tauri::command]
pub fn list_test_prompts() -> Result<Vec<TestPrompt>, String> {
    let (svc, _) = svc()?;
    svc.list_test_prompts().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn upsert_test_prompt(input: TestPromptInput) -> Result<TestPrompt, String> {
    let (svc, _) = svc()?;
    svc.upsert_test_prompt(input).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_test_prompt(id: String) -> Result<(), String> {
    let (svc, _) = svc()?;
    svc.delete_test_prompt(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_default_test_prompt(id: String) -> Result<TestPrompt, String> {
    let (svc, _) = svc()?;
    svc.set_default_test_prompt(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn record_model_test_result(
    model_id: String,
    ok: bool,
    latency_ms: Option<u64>,
    tested_at: Option<String>,
) -> Result<ModelTestResult, String> {
    let (svc, _) = svc()?;
    svc.record_model_test_result(&model_id, ok, latency_ms, tested_at)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn test_model_connection(
    app: tauri::AppHandle,
    request: TestConnectionRequest,
) -> Result<TestConnectionResult, String> {
    let (svc, _) = svc()?;
    let store = svc.load_store().map_err(|e| e.to_string())?;
    let secrets = svc.load_secrets().map_err(|e| e.to_string())?;
    let run_id = request
        .run_id
        .clone()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    adapters::test_model_connection(
        Some(app),
        &run_id,
        &store,
        &secrets,
        &request.model_id,
        &request.prompt,
        request.timeout_secs,
        request.extra_headers.as_ref(),
    )
    .await
    .map_err(|e| e.to_string())
}
