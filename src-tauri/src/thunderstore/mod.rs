//! Source Thunderstore : jeux (« communautés »), mods et installation avec
//! leurs dépendances. Pas de compte ni de clé : l'API est publique.

pub mod api;
pub mod install;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use tauri::ipc::Channel;
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

use crate::api::models::Page;
use crate::commands::AppState;
use crate::error::{AppError, AppResult};
use crate::install_commands::UninstallResult;
use crate::installer::{self, GameInstall, InstallEvent, Library};
use api::{Community, Package, PackageView, TsClient};
use install::{TsInstalled, TsLoader, TsUpdate};

pub struct ThunderstoreState {
    pub client: TsClient,
    /// Mods installés depuis Thunderstore, par jeu (`installed-thunderstore.json`).
    pub library: Mutex<Library>,
    pub library_path: PathBuf,
    /// Installations en cours : drapeau d'annulation par jeu.
    pub jobs: Mutex<HashMap<i64, Arc<AtomicBool>>>,
    /// Liste des jeux, gardée le temps de la session.
    pub communities: Mutex<Option<Vec<Community>>>,
}

impl ThunderstoreState {
    pub fn new(base: &str, library_path: PathBuf) -> Self {
        Self {
            client: TsClient::new(base),
            library: Mutex::new(Library::load(&library_path)),
            library_path,
            jobs: Mutex::new(HashMap::new()),
            communities: Mutex::new(None),
        }
    }

    pub async fn communities(&self, refresh: bool) -> AppResult<Vec<Community>> {
        if !refresh && let Some(list) = self.communities.lock().expect("communities").clone() {
            return Ok(list);
        }
        let mut list = self.client.communities().await?;
        list.sort_by_key(|c| std::cmp::Reverse(c.total_download_count));
        *self.communities.lock().expect("communities") = Some(list.clone());
        Ok(list)
    }

    /// Fichier ou dossier à montrer dans l'explorateur.
    pub fn reveal_path(&self, game_id: i64, mod_id: Option<i64>) -> AppResult<PathBuf> {
        let lib = self.library.lock().expect("library");
        let game = lib.games.get(&game_id).ok_or(AppError::NotFound)?;
        let Some(mod_id) = mod_id else { return Ok(game.folder.clone()) };
        let record = game.mods.get(&mod_id).ok_or(AppError::NotFound)?;
        // Le dossier du mod (`BepInEx/plugins/Équipe-Nom`) plutôt qu'un de ses fichiers.
        let full = record.file_name.clone().unwrap_or_default();
        let dir = record
            .dirs
            .iter()
            .filter(|d| !full.is_empty() && d.rsplit('/').next() == Some(full.as_str()))
            .min_by_key(|d| d.len())
            .or(record.files.first());
        Ok(dir.and_then(|rel| installer::resolve_rel(&game.folder, rel)).unwrap_or_else(|| game.folder.clone()))
    }
}

/// Les bibliothèques des autres sources, réunies : leurs fichiers ne sont
/// jamais écrasés. Les clés sont renumérotées (les identifiants des jeux de
/// chaque source peuvent se recouper).
pub(crate) fn merged(parts: &[&Mutex<Library>]) -> Library {
    let mut out = Library::default();
    let mut key = 0;
    for part in parts {
        for g in part.lock().expect("library").games.values() {
            key += 1;
            out.games.insert(key, g.clone());
        }
    }
    out
}

fn others(state: &AppState) -> Vec<Library> {
    vec![
        state.library.lock().expect("library").clone(),
        state.nexus.library.lock().expect("library").clone(),
    ]
}

#[tauri::command]
pub async fn ts_communities(state: State<'_, AppState>, refresh: Option<bool>) -> AppResult<Vec<Community>> {
    state.thunderstore.communities(refresh.unwrap_or(false)).await
}

/// Une page de mods d'un jeu, au format des listes de l'interface.
#[tauri::command]
pub async fn ts_packages(
    state: State<'_, AppState>,
    community: String,
    query: Option<String>,
    ordering: Option<String>,
    offset: Option<u32>,
    limit: Option<u32>,
) -> AppResult<Page<Package>> {
    let limit = limit.unwrap_or(24).clamp(1, 100);
    let offset = offset.unwrap_or(0);
    let page = offset / limit + 1;
    let (count, data) = state
        .thunderstore
        .client
        .packages(&community, query.as_deref().unwrap_or(""), ordering.as_deref().unwrap_or("most-downloaded"), page, limit)
        .await?;
    let count = count.clamp(0, u32::MAX as i64) as u32;
    Ok(Page {
        result_count: data.len() as u32,
        result_offset: (page - 1) * limit,
        result_limit: limit,
        result_total: count,
        data,
    })
}

#[tauri::command]
pub async fn ts_package(state: State<'_, AppState>, community: String, namespace: String, name: String) -> AppResult<PackageView> {
    state.thunderstore.client.package(&community, &namespace, &name).await
}

#[tauri::command]
pub fn ts_overview(state: State<'_, AppState>) -> Vec<GameInstall> {
    state.thunderstore.overview()
}

#[tauri::command]
pub fn ts_configure(
    state: State<'_, AppState>,
    community: String,
    name: String,
    icon: Option<String>,
    folder: String,
) -> AppResult<GameInstall> {
    state.thunderstore.configure(&community, name, icon, &folder)
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn ts_install(
    state: State<'_, AppState>,
    community: String,
    namespace: String,
    name: String,
    version: Option<String>,
    on_event: Channel<InstallEvent>,
) -> AppResult<TsInstalled> {
    let emit: Arc<dyn Fn(InstallEvent) + Send + Sync> = Arc::new(move |e| {
        let _ = on_event.send(e);
    });
    let others = others(&state);
    state.thunderstore.install(&community, &namespace, &name, version, &others, emit).await
}

#[tauri::command]
pub fn ts_cancel(state: State<'_, AppState>, community: String) {
    state.thunderstore.cancel(&community)
}

#[tauri::command]
pub fn ts_uninstall(state: State<'_, AppState>, game_id: i64, mod_id: i64) -> AppResult<UninstallResult> {
    state.thunderstore.remove(game_id, mod_id)
}

#[tauri::command]
pub fn ts_set_enabled(state: State<'_, AppState>, game_id: i64, mod_id: i64, enabled: bool) -> AppResult<GameInstall> {
    let others = others(&state);
    state.thunderstore.set_enabled(game_id, mod_id, enabled, &others)
}

#[tauri::command]
pub async fn ts_updates(state: State<'_, AppState>, game_id: i64) -> AppResult<Vec<TsUpdate>> {
    state.thunderstore.updates(game_id).await
}

#[tauri::command]
pub fn ts_loader(state: State<'_, AppState>, game_id: i64) -> AppResult<TsLoader> {
    state.thunderstore.loader(game_id)
}

#[tauri::command]
pub fn ts_reveal(app: AppHandle, state: State<'_, AppState>, game_id: i64, mod_id: Option<i64>) -> AppResult<()> {
    let path = state.thunderstore.reveal_path(game_id, mod_id)?;
    app.opener()
        .reveal_item_in_dir(&path)
        .map_err(|e| AppError::Io(e.to_string()))
}
