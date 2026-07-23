mod adapters;
mod backup;
mod commands;
mod paths;
mod store;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            commands::get_state,
            commands::save_app_config,
            commands::add_provider,
            commands::update_provider,
            commands::delete_provider,
            commands::clone_provider,
            commands::set_provider_enabled,
            commands::add_model,
            commands::update_model,
            commands::delete_model,
            commands::save_bindings,
            commands::read_live_bindings,
            commands::apply_config,
            commands::preview_apply,
            commands::preview_import,
            commands::run_import,
            commands::list_backups,
            commands::reveal_api_key,
            commands::fetch_provider_models,
            commands::delete_providers,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
