mod account;
mod api;
mod auth;
mod commands;
mod error;
mod install_commands;
mod installer;
mod modio_sdk;
mod settings;
#[cfg(test)]
mod testutil;

use tauri::Manager;

use commands::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let settings_path = app.path().app_config_dir()?.join("settings.json");
            let loaded = settings::load(&settings_path);
            let secrets = Box::new(auth::ChunkedStore::new(auth::KeyringBackend));
            app.manage(AppState::new(loaded, settings_path, secrets));
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
            install_commands::install_overview,
            install_commands::set_game_folder,
            install_commands::folder_suggestion,
            install_commands::inspect_folder,
            modio_sdk::detect_modio_install,
            install_commands::install_mod,
            install_commands::cancel_install,
            install_commands::uninstall_mod,
            install_commands::latest_installed_files,
            install_commands::reveal_install,
        ])
        .run(tauri::generate_context!())
        .expect("erreur au lancement de ModStudio");
}
