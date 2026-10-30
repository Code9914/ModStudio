//! Source Thunderstore : jeux (« communautés »), mods et installation avec
//! leurs dépendances. Pas de compte ni de clé : l'API est publique.

pub mod api;
pub mod ecosystem;
pub mod install;
pub mod rml;

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;
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
use ecosystem::{loader_label, Ecosystem};
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
    /// Ce que chaque jeu demande pour charger ses mods (et sa copie sur le disque).
    pub ecosystem: tokio::sync::Mutex<Option<Arc<Ecosystem>>>,
    pub ecosystem_path: PathBuf,
    /// Mods de ResoniteModLoader (liste vérifiée de la communauté).
    pub rml: rml::RmlSource,
}

impl ThunderstoreState {
    pub fn new(base: &str, library_path: PathBuf) -> Self {
        Self {
            client: TsClient::new(base),
            library: Mutex::new(Library::load(&library_path)),
            jobs: Mutex::new(HashMap::new()),
            communities: Mutex::new(None),
            ecosystem: tokio::sync::Mutex::new(None),
            ecosystem_path: library_path.with_file_name("thunderstore-ecosystem.json"),
            rml: rml::RmlSource::new(rml::MANIFEST_URL, library_path.with_file_name("rml-manifest.json")),
            library_path,
        }
    }

    /// Règles de tous les jeux : copie du jour sur le disque, sinon téléchargées,
    /// sinon la dernière copie (ou celle livrée avec ModStudio) hors ligne.
    pub async fn ecosystem(&self) -> Arc<Ecosystem> {
        let mut slot = self.ecosystem.lock().await;
        if let Some(e) = slot.as_ref() {
            return e.clone();
        }
        let from_disk = || fs::read(&self.ecosystem_path).ok().and_then(|b| Ecosystem::parse(&b));
        let fresh = fs::metadata(&self.ecosystem_path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age < Duration::from_secs(24 * 3600));
        let cached = if fresh { from_disk() } else { None };
        let e = match cached {
            Some(e) => e,
            None => match self.client.ecosystem_raw().await.ok().and_then(|b| Ecosystem::parse(&b)) {
                Some(e) => {
                    let tmp = self.ecosystem_path.with_extension("json.tmp");
                    if fs::write(&tmp, e.to_compact_json()).is_ok() {
                        let _ = fs::rename(&tmp, &self.ecosystem_path);
                    }
                    e
                }
                None => from_disk().unwrap_or_else(Ecosystem::bundled),
            },
        };
        let e = Arc::new(e);
        *slot = Some(e.clone());
        e
    }

    pub async fn communities(&self, refresh: bool) -> AppResult<Vec<Community>> {
        if !refresh && let Some(list) = self.communities.lock().expect("communities").clone() {
            return Ok(list);
        }
        let mut list = self.client.communities().await?;
        list.sort_by_key(|c| std::cmp::Reverse(c.total_download_count));
        let eco = self.ecosystem().await;
        for c in &mut list {
            let game = eco.game(&c.identifier);
            c.loader = game.as_ref().map(|g| loader_label(&g.loader_id).to_string());
            c.supported = game.is_some_and(|g| g.kind.supported());
        }
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
    let limit = limit.unwrap_or(40).clamp(1, 100);
    let offset = offset.unwrap_or(0);
    let (count, data) = state
        .thunderstore
        .client
        .packages_range(&community, query.as_deref().unwrap_or(""), ordering.as_deref().unwrap_or("most-downloaded"), offset, limit)
        .await?;
    let count = count.clamp(0, u32::MAX as i64) as u32;
    Ok(Page {
        result_count: data.len() as u32,
        result_offset: offset,
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
pub async fn ts_loader(state: State<'_, AppState>, game_id: i64) -> AppResult<TsLoader> {
    state.thunderstore.loader(game_id).await
}

/// Mods de la liste vérifiée de ResoniteModLoader (Resonite).
#[tauri::command]
pub async fn rml_mods(state: State<'_, AppState>, refresh: Option<bool>) -> AppResult<Vec<rml::RmlMod>> {
    Ok(state.thunderstore.rml.catalog(refresh.unwrap_or(false)).await?.mods.clone())
}

#[tauri::command]
pub fn rml_status(state: State<'_, AppState>) -> Option<rml::RmlStatus> {
    state.thunderstore.rml_status()
}

#[tauri::command]
pub async fn rml_install(
    state: State<'_, AppState>,
    id: String,
    version: Option<String>,
    on_event: Channel<InstallEvent>,
) -> AppResult<TsInstalled> {
    let emit: Arc<dyn Fn(InstallEvent) + Send + Sync> = Arc::new(move |e| {
        let _ = on_event.send(e);
    });
    let others = others(&state);
    state.thunderstore.rml_install(&id, version, &others, emit).await
}

#[tauri::command]
pub fn ts_reveal(app: AppHandle, state: State<'_, AppState>, game_id: i64, mod_id: Option<i64>) -> AppResult<()> {
    let path = state.thunderstore.reveal_path(game_id, mod_id)?;
    app.opener()
        .reveal_item_in_dir(&path)
        .map_err(|e| AppError::Io(e.to_string()))
}
