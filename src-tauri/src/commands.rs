//! Commandes appelées par l'interface (`invoke("…")`).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, RwLock};

use crate::installer::Library;

use tauri::State;

use crate::account::AuthState;
use crate::api::models::{Dependency, Game, Mod, Modfile, Page};
use crate::api::{ApiConfig, ModioClient};
use crate::auth::SecretStore;
use crate::error::{AppError, AppResult};
use crate::nexus::NexusState;
use crate::settings::{self, Settings, SettingsView};

pub struct AppState {
    pub client: ModioClient,
    pub settings: RwLock<Settings>,
    pub settings_path: PathBuf,
    /// Où le jeton de connexion est conservé (trousseau du système).
    pub secrets: Box<dyn SecretStore>,
    pub auth: RwLock<AuthState>,
    /// Mods installés à la main, par jeu (`installed.json`).
    pub library: Mutex<Library>,
    pub library_path: PathBuf,
    /// Installations en cours : drapeau d'annulation par mod.
    pub jobs: Mutex<HashMap<i64, Arc<AtomicBool>>>,
    /// Source Nexus Mods (compte, bibliothèque et installations à part).
    pub nexus: NexusState,
    /// Source Thunderstore (bibliothèque et installations à part).
    pub thunderstore: crate::thunderstore::ThunderstoreState,
    /// Listes de mods des communautés (Hollow Knight, Beat Saber…).
    pub lists: crate::lists::ListsState,
    /// Profils partagés lus (réglages r2modman en attente).
    pub share: crate::share::ShareState,
}

impl AppState {
    pub fn new(
        settings: Settings,
        settings_path: PathBuf,
        secrets: Box<dyn SecretStore>,
        nexus_secrets: Box<dyn SecretStore>,
    ) -> Self {
        let library_path = settings_path.with_file_name("installed.json");
        let nexus = NexusState::new(
            crate::nexus::client::API_BASE,
            nexus_secrets,
            settings_path.with_file_name("installed-nexus.json"),
        );
        let thunderstore = crate::thunderstore::ThunderstoreState::new(
            crate::thunderstore::api::API_BASE,
            settings_path.with_file_name("installed-thunderstore.json"),
        );
        let lists = crate::lists::ListsState::new(settings_path.with_file_name("installed-lists.json"));
        Self {
            nexus,
            thunderstore,
            lists,
            share: crate::share::ShareState::default(),
            client: ModioClient::new(),
            settings: RwLock::new(settings),
            library: Mutex::new(Library::load(&library_path)),
            library_path,
            settings_path,
            secrets,
            auth: RwLock::new(AuthState::default()),
            jobs: Mutex::new(HashMap::new()),
        }
    }

    pub(crate) fn snapshot(&self) -> Settings {
        self.settings.read().expect("réglages").clone()
    }

    pub(crate) fn api_config(&self) -> ApiConfig {
        let s = self.snapshot();
        ApiConfig {
            api_key: s.api_key,
            api_path: s.api_path,
        }
    }
}

// ---------------------------------------------------------------- Réglages

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> SettingsView {
    SettingsView::from(&state.snapshot())
}

/// Enregistre la clé et le chemin d'API après avoir vérifié qu'ils
/// fonctionnent. `api_key` vide ou absent = garder la clé actuelle.
#[tauri::command]
pub async fn save_settings(
    state: State<'_, AppState>,
    api_key: Option<String>,
    api_path: String,
) -> AppResult<SettingsView> {
    let current = state.snapshot();

    let api_key = match api_key.map(|k| k.trim().to_string()) {
        Some(k) if !k.is_empty() => {
            if k.chars().any(char::is_whitespace) {
                return Err(AppError::InvalidArgument(
                    "la clé API ne doit pas contenir d'espaces.".into(),
                ));
            }
            k
        }
        _ => current.api_key.clone(),
    };
    let api_path = settings::normalize_api_path(&api_path)?;

    let cfg = ApiConfig {
        api_key: api_key.clone(),
        api_path: api_path.clone(),
    };
    // Requête de test : la plus légère possible.
    let _: Page<Game> = state
        .client
        .get(&cfg, "/games", &[("_limit", "1".into())])
        .await?;

    let updated = Settings {
        api_key,
        api_path,
        ..current
    };
    settings::save(&state.settings_path, &updated)?;
    let view = SettingsView::from(&updated);
    *state.settings.write().expect("réglages") = updated;
    Ok(view)
}

#[tauri::command]
pub fn set_hide_mature(state: State<'_, AppState>, value: bool) -> AppResult<SettingsView> {
    let mut updated = state.snapshot();
    updated.hide_mature = value;
    settings::save(&state.settings_path, &updated)?;
    let view = SettingsView::from(&updated);
    *state.settings.write().expect("réglages") = updated;
    Ok(view)
}

// ---------------------------------------------------------------- Jeux

const GAME_SORTS: &[&str] = &["name", "-date_updated"];

#[tauri::command]
pub async fn list_games(
    state: State<'_, AppState>,
    query: Option<String>,
    sort: Option<String>,
    offset: u32,
    limit: u32,
) -> AppResult<Page<Game>> {
    let mut params = vec![
        ("_sort", pick(sort, GAME_SORTS, "name")?),
        ("_offset", offset.to_string()),
        ("_limit", clamp_limit(limit).to_string()),
    ];
    push_search(&mut params, query);
    state.client.get(&state.api_config(), "/games", &params).await
}

#[tauri::command]
pub async fn get_game(state: State<'_, AppState>, game_id: i64) -> AppResult<Game> {
    check_id(game_id)?;
    state
        .client
        .get(&state.api_config(), &format!("/games/{game_id}"), &[])
        .await
}

// ---------------------------------------------------------------- Mods

const MOD_SORTS: &[&str] = &[
    "-downloads_today",
    "-downloads_total",
    "-subscribers_total",
    "-ratings_weighted_aggregate",
    "-date_live",
    "-date_updated",
    "name",
];

#[tauri::command]
pub async fn list_mods(
    state: State<'_, AppState>,
    game_id: i64,
    query: Option<String>,
    tags: Option<Vec<String>>,
    sort: Option<String>,
    offset: u32,
    limit: u32,
) -> AppResult<Page<Mod>> {
    check_id(game_id)?;
    let mut params = vec![
        ("_sort", pick(sort, MOD_SORTS, "-downloads_today")?),
        ("_offset", offset.to_string()),
        ("_limit", clamp_limit(limit).to_string()),
    ];
    push_search(&mut params, query);

    let tags: Vec<String> = tags
        .unwrap_or_default()
        .into_iter()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect();
    if !tags.is_empty() {
        // Plusieurs tags : seuls les mods qui les ont tous sont renvoyés.
        params.push(("tags", tags.join(",")));
    }
    if state.snapshot().hide_mature {
        params.push(("maturity_option", "0".into()));
    }

    let page: Page<Mod> = state
        .client
        .get(&state.api_config(), &format!("/games/{game_id}/mods"), &params)
        .await?;
    Ok(Page {
        data: page.data.into_iter().map(Mod::into_summary).collect(),
        ..page
    })
}

#[tauri::command]
pub async fn get_mod(state: State<'_, AppState>, game_id: i64, mod_id: i64) -> AppResult<Mod> {
    check_id(game_id)?;
    check_id(mod_id)?;
    state
        .client
        .get(&state.api_config(), &format!("/games/{game_id}/mods/{mod_id}"), &[])
        .await
}

#[tauri::command]
pub async fn list_mod_files(
    state: State<'_, AppState>,
    game_id: i64,
    mod_id: i64,
) -> AppResult<Vec<Modfile>> {
    check_id(game_id)?;
    check_id(mod_id)?;
    let page: Page<Modfile> = state
        .client
        .get(
            &state.api_config(),
            &format!("/games/{game_id}/mods/{mod_id}/files"),
            &[("_sort", "-date_added".into()), ("_limit", "20".into())],
        )
        .await?;
    Ok(page.data)
}

#[tauri::command]
pub async fn list_dependencies(
    state: State<'_, AppState>,
    game_id: i64,
    mod_id: i64,
) -> AppResult<Vec<Dependency>> {
    check_id(game_id)?;
    check_id(mod_id)?;
    let page: Page<Dependency> = state
        .client
        .get(
            &state.api_config(),
            &format!("/games/{game_id}/mods/{mod_id}/dependencies"),
            &[("recursive", "true".into())],
        )
        .await?;
    Ok(page.data)
}

// ---------------------------------------------------------------- Fenêtre mod.io

/// Étiquette de la fenêtre qui affiche le site officiel de mod.io. Elle n'a
/// aucune capacité Tauri (aucun accès aux commandes de l'appli).
const MODIO_WINDOW: &str = "modio";

/// N'accepte que des pages du site officiel de mod.io, en HTTPS.
fn modio_page(url: &str) -> Option<tauri::Url> {
    let parsed = tauri::Url::parse(url.trim()).ok()?;
    let host = parsed.host_str()?.to_ascii_lowercase();
    (parsed.scheme() == "https" && (host == "mod.io" || host == "www.mod.io")).then_some(parsed)
}

/// Ouvre (ou réutilise) une fenêtre de ModStudio sur une page de mod.io.
/// Sert pour les jeux fermés aux applications tierces : c'est le site officiel
/// qui fait tout, avec la connexion de l'utilisateur à mod.io.
/// `async` obligatoire : voir `nexus_open_download`.
#[tauri::command]
pub async fn open_modio_window(app: tauri::AppHandle, url: String, title: Option<String>) -> AppResult<()> {
    use tauri::Manager;

    let page = modio_page(&url)
        .ok_or_else(|| AppError::InvalidArgument("seules les pages de mod.io peuvent s'ouvrir ici.".into()))?;
    let title = format!("{} sur mod.io", title.unwrap_or_else(|| "Mods".into()));

    if let Some(window) = app.get_webview_window(MODIO_WINDOW) {
        window.navigate(page).map_err(|e| AppError::Io(e.to_string()))?;
        let _ = window.set_title(&title);
        let _ = window.unminimize();
        let _ = window.set_focus();
        return Ok(());
    }
    tauri::WebviewWindowBuilder::new(&app, MODIO_WINDOW, tauri::WebviewUrl::External(page))
        .title(title)
        .inner_size(1200.0, 820.0)
        .min_inner_size(800.0, 600.0)
        .build()
        .map_err(|e| AppError::Io(e.to_string()))?;
    Ok(())
}

// ---------------------------------------------------------------- Utilitaires

fn clamp_limit(limit: u32) -> u32 {
    limit.clamp(1, 100)
}

fn check_id(id: i64) -> AppResult<()> {
    if id > 0 {
        Ok(())
    } else {
        Err(AppError::InvalidArgument(format!("identifiant {id}")))
    }
}

fn pick(value: Option<String>, allowed: &[&str], default: &str) -> AppResult<String> {
    match value {
        None => Ok(default.to_string()),
        Some(v) if allowed.contains(&v.as_str()) => Ok(v),
        Some(v) => Err(AppError::InvalidArgument(format!("tri « {v} »"))),
    }
}

fn push_search(params: &mut Vec<(&'static str, String)>, query: Option<String>) {
    if let Some(q) = query.map(|q| q.trim().to_string()).filter(|q| !q.is_empty()) {
        params.push(("_q", q));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sort_whitelist() {
        assert_eq!(pick(None, MOD_SORTS, "-downloads_today").unwrap(), "-downloads_today");
        assert_eq!(pick(Some("name".into()), MOD_SORTS, "x").unwrap(), "name");
        assert!(pick(Some("id;drop".into()), MOD_SORTS, "x").is_err());
    }

    #[test]
    fn limits_and_ids() {
        assert_eq!(clamp_limit(0), 1);
        assert_eq!(clamp_limit(500), 100);
        assert!(check_id(0).is_err());
        assert!(check_id(42).is_ok());
    }

    #[test]
    fn modio_window_only_opens_modio_pages() {
        assert!(modio_page("https://mod.io/g/incredibox").is_some());
        assert!(modio_page(" https://www.mod.io/g/incredibox/m/abc ").is_some());
        assert!(modio_page("http://mod.io/g/incredibox").is_none());
        assert!(modio_page("https://mod.io.evil.example/g/x").is_none());
        assert!(modio_page("https://evil.example/?https://mod.io").is_none());
        assert!(modio_page("file:///C:/Windows").is_none());
        assert!(modio_page("pas une url").is_none());
    }
}
