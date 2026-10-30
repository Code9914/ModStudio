//! Nexus Mods : deuxième source de mods, à côté de mod.io.
//!
//! - Parcourir (jeux, mods, fichiers) passe par l'API GraphQL publique.
//! - Télécharger demande la clé API personnelle de l'utilisateur. Sans compte
//!   Premium, chaque téléchargement passe par la page du fichier sur le site
//!   (voir `window.rs`), comme le veut Nexus Mods.
//! - Les mods installés sont suivis dans `installed-nexus.json`, séparé du
//!   manifeste mod.io.

pub mod account;
pub mod browse;
pub mod client;
pub mod install;
pub mod models;
pub mod rules;
pub mod window;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, RwLock};

use tauri::ipc::Channel;
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

use crate::api::models::Page;
use crate::auth::SecretStore;
use crate::commands::AppState;
use crate::error::{AppError, AppResult};
use crate::installer::{GameInstall, InstallEvent, Layout, Library};
use account::{NexusAuth, NexusAuthView};
use client::NexusClient;
use install::{GameSetup, InstallRequest, NexusInstalled, UpdateInfo};
use models::{NexusFile, NexusGame, NexusMod, NexusModDetail};

/// Nom de l'entrée du trousseau qui garde la clé API Nexus.
pub const KEYRING_ACCOUNT: &str = "nexusmods";

pub struct NexusState {
    pub client: NexusClient,
    pub secrets: Box<dyn SecretStore>,
    pub auth: RwLock<NexusAuth>,
    pub library: Mutex<Library>,
    pub library_path: PathBuf,
    /// Installations en cours : drapeau d'annulation par mod.
    pub jobs: Mutex<HashMap<i64, Arc<AtomicBool>>>,
}

impl NexusState {
    pub fn new(base: &str, secrets: Box<dyn SecretStore>, library_path: PathBuf) -> Self {
        Self {
            client: NexusClient::new(base),
            secrets,
            auth: RwLock::new(NexusAuth::default()),
            library: Mutex::new(Library::load(&library_path)),
            library_path,
            jobs: Mutex::new(HashMap::new()),
        }
    }
}

// ---------------------------------------------------------------- Compte

#[tauri::command]
pub async fn nexus_status(state: State<'_, AppState>) -> AppResult<NexusAuthView> {
    state.nexus.status().await
}

#[tauri::command]
pub async fn nexus_set_key(state: State<'_, AppState>, key: String) -> AppResult<NexusAuthView> {
    state.nexus.set_key(&key).await
}

#[tauri::command]
pub fn nexus_forget_key(state: State<'_, AppState>) -> AppResult<NexusAuthView> {
    state.nexus.forget_key()
}

// ---------------------------------------------------------------- Parcourir

#[tauri::command]
pub async fn nexus_list_games(
    state: State<'_, AppState>,
    query: Option<String>,
    sort: Option<String>,
    offset: u32,
    limit: u32,
) -> AppResult<Page<NexusGame>> {
    state.nexus.list_games(query, sort, offset, limit).await
}

#[tauri::command]
pub async fn nexus_get_game(state: State<'_, AppState>, domain: String) -> AppResult<NexusGame> {
    state.nexus.get_game(&domain).await
}

#[tauri::command]
pub async fn nexus_list_mods(
    state: State<'_, AppState>,
    game_domain: String,
    query: Option<String>,
    sort: Option<String>,
    offset: u32,
    limit: u32,
) -> AppResult<Page<NexusMod>> {
    let hide_adult = state.snapshot().hide_mature;
    state.nexus.list_mods(&game_domain, query, sort, hide_adult, offset, limit).await
}

#[tauri::command]
pub async fn nexus_get_mod(state: State<'_, AppState>, game_id: i64, mod_id: i64) -> AppResult<NexusModDetail> {
    state.nexus.get_mod(game_id, mod_id).await
}

#[tauri::command]
pub async fn nexus_list_files(state: State<'_, AppState>, game_id: i64, mod_id: i64) -> AppResult<Vec<NexusFile>> {
    state.nexus.list_files(game_id, mod_id).await
}

/// Page d'un fichier sur le site, dans ModStudio. Renvoie `embedded` si la
/// page s'affiche dans la fenêtre principale (aux coordonnées `bounds`),
/// `window` si elle s'est ouverte dans une fenêtre séparée.
///
/// Doit rester `async` : sous Windows, une vue web créée depuis une commande
/// synchrone (exécutée sur le fil principal) reste blanche (wry#583).
#[tauri::command]
pub async fn nexus_open_download(
    app: AppHandle,
    domain: String,
    mod_id: i64,
    file_id: i64,
    bounds: Option<window::Bounds>,
) -> AppResult<window::Placement> {
    let url = window::file_page(&domain, mod_id, file_id)?;
    window::show(&app, &url, "Télécharger sur Nexus Mods", bounds)
}

/// Première connexion au site, dans ModStudio : la session y reste ouverte
/// pour les téléchargements suivants, et la clé API s'y copie.
#[tauri::command]
pub async fn nexus_open_login(app: AppHandle, bounds: Option<window::Bounds>) -> AppResult<window::Placement> {
    window::show(&app, window::LOGIN_PAGE, "Connexion à Nexus Mods", bounds)
}

/// La zone d'affichage de la page Nexus a changé (fenêtre redimensionnée).
#[tauri::command]
pub async fn nexus_site_bounds(app: AppHandle, bounds: window::Bounds) -> AppResult<()> {
    window::set_bounds(&app, bounds)
}

/// Ferme la page Nexus (bouton « Annuler » ou « Fermer »).
#[tauri::command]
pub async fn nexus_site_close(app: AppHandle) {
    window::close_all(&app)
}

// ---------------------------------------------------------------- Installation

#[tauri::command]
pub fn nexus_overview(state: State<'_, AppState>) -> Vec<GameInstall> {
    state.nexus.overview()
}

#[tauri::command]
pub fn nexus_game_setup(state: State<'_, AppState>, game_id: i64) -> GameSetup {
    state.nexus.setup(game_id)
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn nexus_configure_game(
    state: State<'_, AppState>,
    game_id: i64,
    domain: String,
    game_name: String,
    icon: Option<String>,
    folder: String,
    layout: Layout,
    create: Option<bool>,
    game_dir: Option<String>,
) -> AppResult<GameInstall> {
    state
        .nexus
        .configure(game_id, &domain, game_name, icon, folder, layout, create.unwrap_or(false), game_dir)
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn nexus_install(
    state: State<'_, AppState>,
    game_id: i64,
    game_domain: String,
    mod_id: i64,
    file_id: i64,
    nxm_key: Option<String>,
    nxm_expires: Option<i64>,
    on_event: Channel<InstallEvent>,
) -> AppResult<NexusInstalled> {
    let emit: Arc<dyn Fn(InstallEvent) + Send + Sync> = Arc::new(move |e| {
        let _ = on_event.send(e);
    });
    let nxm = match (nxm_key, nxm_expires) {
        (Some(k), Some(e)) if !k.is_empty() => Some((k, e)),
        _ => None,
    };
    // Les fichiers des mods installés depuis mod.io ne sont jamais écrasés.
    let modio = state.library.lock().expect("library").clone();
    state
        .nexus
        .install(
            InstallRequest {
                game_id,
                game_domain,
                mod_id,
                file_id,
                nxm,
            },
            &modio,
            emit,
        )
        .await
}

#[tauri::command]
pub fn nexus_cancel_install(state: State<'_, AppState>, mod_id: i64) {
    state.nexus.cancel(mod_id)
}

/// Désinstalle ; renvoie le dossier laissé en place s'il contient des fichiers ajoutés depuis.
#[tauri::command]
pub fn nexus_uninstall(state: State<'_, AppState>, game_id: i64, mod_id: i64) -> AppResult<Option<String>> {
    state.nexus.remove(game_id, mod_id)
}

#[tauri::command]
pub async fn nexus_updates(state: State<'_, AppState>, game_id: i64) -> AppResult<Vec<UpdateInfo>> {
    state.nexus.updates(game_id).await
}

#[tauri::command]
pub fn nexus_reveal(app: AppHandle, state: State<'_, AppState>, game_id: i64, mod_id: Option<i64>) -> AppResult<()> {
    let path = state.nexus.reveal_path(game_id, mod_id)?;
    app.opener()
        .reveal_item_in_dir(&path)
        .map_err(|e| AppError::Io(e.to_string()))
}
