mod account;
mod api;
mod archive;
mod auth;
mod commands;
mod error;
mod games;
mod install_commands;
mod installer;
mod modio_sdk;
mod nexus;
mod settings;
mod toggle;
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
            // Mises à jour de ModStudio : signées, publiées dans les releases GitHub.
            #[cfg(desktop)]
            {
                app.handle().plugin(tauri_plugin_updater::Builder::new().build())?;
                app.handle().plugin(tauri_plugin_process::init())?;
            }
            let settings_path = app.path().app_config_dir()?.join("settings.json");
            let loaded = settings::load(&settings_path);
            let secrets = Box::new(auth::ChunkedStore::new(auth::KeyringBackend));
            let nexus_secrets = Box::new(auth::ChunkedStore::for_account(auth::KeyringBackend, nexus::KEYRING_ACCOUNT));
            app.manage(AppState::new(loaded, settings_path, secrets, nexus_secrets));
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
            commands::open_modio_window,
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
            install_commands::set_mod_enabled,
            install_commands::apply_mod_profile,
            install_commands::edit_mod_profiles,
            install_commands::reveal_install,
            nexus::nexus_status,
            nexus::nexus_set_key,
            nexus::nexus_forget_key,
            nexus::nexus_list_games,
            nexus::nexus_get_game,
            nexus::nexus_list_mods,
            nexus::nexus_get_mod,
            nexus::nexus_list_files,
            nexus::nexus_open_download,
            nexus::nexus_open_login,
            nexus::nexus_site_bounds,
            nexus::nexus_site_close,
            nexus::nexus_overview,
            nexus::nexus_found_loaders,
            nexus::nexus_game_setup,
            nexus::nexus_detect_games,
            nexus::nexus_set_enabled,
            nexus::nexus_apply_profile,
            nexus::nexus_edit_profiles,
            nexus::nexus_install_tool,
            nexus::nexus_cancel_tool,
            nexus::nexus_uninstall_tool,
            nexus::nexus_configure_game,
            nexus::nexus_install,
            nexus::nexus_cancel_install,
            nexus::nexus_fomod_finish,
            nexus::nexus_fomod_cancel,
            nexus::nexus_uninstall,
            nexus::nexus_updates,
            nexus::nexus_reveal,
        ])
        .run(tauri::generate_context!())
        .expect("erreur au lancement de ModStudio");
}
