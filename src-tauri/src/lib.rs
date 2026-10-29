mod account;
mod api;
mod auth;
mod commands;
mod error;
mod settings;
#[cfg(test)]
mod testutil;

use tauri::Manager;

use commands::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let settings_path = app.path().app_config_dir()?.join("settings.json");
            let loaded = settings::load(&settings_path);
            app.manage(AppState::new(loaded, settings_path, Box::new(auth::ChunkedStore::new(auth::KeyringBackend))));
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
            account::auth_status,
            account::auth_request_code,
            account::auth_exchange_code,
            account::auth_use_token,
            account::auth_logout,
            account::list_my_mods,
            account::list_subscribed_ids,
            account::subscribe_mod,
            account::unsubscribe_mod,
        ])
        .run(tauri::generate_context!())
        .expect("erreur au lancement de ModStudio");
}
