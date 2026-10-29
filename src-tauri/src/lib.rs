mod api;
mod commands;
mod error;
mod settings;

use std::sync::RwLock;

use tauri::Manager;

use commands::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let settings_path = app.path().app_config_dir()?.join("settings.json");
            let loaded = settings::load(&settings_path);
            app.manage(AppState {
                client: api::ModioClient::new(),
                settings: RwLock::new(loaded),
                settings_path,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::save_settings,
            commands::set_hide_mature,
            commands::list_games,
            commands::get_game,
            commands::list_mods,
            commands::get_mod,
            commands::list_mod_files,
            commands::list_dependencies,
        ])
        .run(tauri::generate_context!())
        .expect("erreur au lancement de ModStudio");
}
