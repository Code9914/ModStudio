//! Listes de mods tenues par la communauté de certains jeux, en dehors des
//! grands sites : ModLinks (Hollow Knight), la base d'OWML (Outer Wilds),
//! BeatMods (Beat Saber), le ChilloutVR Modding Group (ChilloutVR) et
//! RaftModding (Raft). Chaque jeu a son chargeur de mods ; les mods sont
//! rangés là où il les lit, dans le dossier du jeu.

pub mod games;
pub mod install;

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::{redirect, Client, Url};
use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

use crate::thunderstore::api::stable_id as stable_id_of;
use crate::commands::AppState;
use crate::error::{AppError, AppResult};
use crate::install_commands::UninstallResult;
use crate::installer::{GameInstall, InstallEvent, Library};
use games::GameDef;

const USER_AGENT: &str = concat!("ModStudio/", env!("CARGO_PKG_VERSION"), " (+https://github.com/Code9914/ModStudio)");
/// Les listes sont relues au plus toutes les six heures (ou à la demande).
const CACHE_AGE: Duration = Duration::from_secs(6 * 3600);
/// Taille maximale d'une liste téléchargée.
const LIST_LIMIT: usize = 48 * 1024 * 1024;

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Identifiant d'un jeu de ces listes dans la bibliothèque.
pub fn game_id(game: &str) -> i64 {
    stable_id_of(&format!("list:{game}"))
}

/// Nom enregistré d'un mod (`<jeu>:<identifiant>`).
pub fn record_name(game: &str, id: &str) -> String {
    format!("{game}:{id}")
}

/// Identifiant d'un mod dans la bibliothèque.
pub fn mod_id(game: &str, id: &str) -> i64 {
    stable_id_of(&record_name(game, id))
}

// ---------------------------------------------------------------- Mods

/// Un mod d'une liste, prêt pour l'interface.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListMod {
    pub id: String,
    /// Identifiant dans la bibliothèque (voir [`mod_id`]).
    pub mod_id: i64,
    pub name: String,
    pub description: String,
    pub category: Option<String>,
    pub author: String,
    pub author_url: Option<String>,
    pub icon: Option<String>,
    /// Second essai si `icon` ne s'affiche pas.
    pub icon_fallback: Option<String>,
    /// Page du mod (dépôt, page de la liste…).
    pub page: Option<String>,
    pub tags: Vec<String>,
    /// Plus maintenu, ou retiré de la liste.
    pub deprecated: bool,
    /// Signalé comme cassé.
    pub broken: bool,
    /// Peut se charger dans le jeu sur ce PC.
    pub compatible: bool,
    /// Pourquoi il est signalé (ancienne version du jeu…).
    pub note: Option<String>,
    /// C'est le chargeur de mods du jeu.
    pub loader: bool,
    pub downloads: Option<u64>,
    /// Date de la dernière version (ISO 8601).
    pub updated: Option<String>,
    pub latest: String,
    /// Versions, la plus récente d'abord.
    pub versions: Vec<ListVersion>,
}

impl ListMod {
    pub fn version(&self, v: &str) -> Option<&ListVersion> {
        self.versions.iter().find(|x| x.version == v)
    }

    pub fn newest(&self) -> &ListVersion {
        &self.versions[0]
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListVersion {
    pub version: String,
    pub changelog: Option<String>,
    pub release_url: Option<String>,
    pub dependencies: Vec<ListDep>,
    pub conflicts: Vec<ListDep>,
    /// Versions du jeu visées, quand la liste les donne.
    pub game_versions: Vec<String>,
    /// Où vont ses fichiers (pour l'affichage).
    pub files: Vec<String>,
    #[serde(skip)]
    pub(crate) artifacts: Vec<Artifact>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListDep {
    pub id: String,
    pub name: String,
    /// Version demandée (`^1.2.0`, `*`…), vide si la liste n'en dit rien.
    pub version: String,
    pub optional: bool,
}

/// Empreinte publiée pour un fichier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Hash {
    /// La liste n'en publie pas : le fichier vient en HTTPS de son site.
    None,
    Sha256(String),
}

/// Où va un fichier téléchargé.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Place {
    /// Le fichier tel quel dans `dir` (relatif au dossier `key`), sous le nom
    /// `name` ou, sans nom, celui que donne le serveur.
    File { key: String, dir: String, name: Option<String> },
    /// Archive zip extraite dans `dir`.
    Zip { key: String, dir: String, root: ZipRoot },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ZipRoot {
    /// Le contenu tel quel.
    AsIs,
    /// Le dossier qui contient ce fichier (`manifest.json`) devient la racine.
    DirOf(&'static str),
}

#[derive(Debug, Clone)]
pub(crate) struct Artifact {
    pub url: String,
    pub hash: Hash,
    pub place: Place,
    /// Fichiers du jeu remplacés (chargeur) : gardés de côté et remis à la désinstallation.
    pub replaces: bool,
    /// Empreinte MD5 de chaque fichier de l'archive (chemin dans l'archive, empreinte).
    pub contents: Vec<(String, String)>,
}

/// La liste d'un jeu.
pub struct Catalog {
    pub mods: Vec<ListMod>,
    index: HashMap<String, usize>,
    /// Version du jeu lue dans son dossier (Beat Saber).
    pub game_version: Option<String>,
}

impl Catalog {
    pub fn new(mut mods: Vec<ListMod>, game_version: Option<String>) -> Self {
        // Noms des mods demandés ou en conflit.
        let names: HashMap<String, String> = mods.iter().map(|m| (m.id.to_lowercase(), m.name.clone())).collect();
        for m in &mut mods {
            for v in &mut m.versions {
                for d in v.dependencies.iter_mut().chain(v.conflicts.iter_mut()) {
                    if let Some(n) = names.get(&d.id.to_lowercase()) {
                        d.name = n.clone();
                    }
                }
            }
        }
        mods.retain(|m| !m.versions.is_empty());
        mods.sort_by_key(|m| m.name.to_lowercase());
        let index = mods.iter().enumerate().map(|(i, m)| (m.id.to_lowercase(), i)).collect();
        Self { mods, index, game_version }
    }

    pub fn get(&self, id: &str) -> Option<&ListMod> {
        self.index.get(&id.to_lowercase()).map(|&i| &self.mods[i])
    }

    /// Le chargeur de mods du jeu, s'il est dans la liste.
    pub fn loader(&self) -> Option<&ListMod> {
        self.mods.iter().find(|m| m.loader)
    }
}

// ---------------------------------------------------------------- Réseau

/// Adresses des listes (remplacées par un faux serveur dans les tests).
#[derive(Debug, Clone)]
pub struct Urls {
    pub modlinks: String,
    pub apilinks: String,
    pub owdb: String,
    pub beatmods: String,
    pub cvrmg: String,
    pub raft: String,
    pub github_api: String,
}

impl Default for Urls {
    fn default() -> Self {
        Self {
            modlinks: "https://raw.githubusercontent.com/hk-modding/modlinks/main/ModLinks.xml".into(),
            apilinks: "https://raw.githubusercontent.com/hk-modding/modlinks/main/ApiLinks.xml".into(),
            owdb: "https://ow-mods.github.io/ow-mod-db/database.json".into(),
            beatmods: "https://beatmods.com".into(),
            cvrmg: "https://api.cvrmg.com/v1/mods".into(),
            raft: "https://www.raftmodding.com".into(),
            github_api: "https://api.github.com".into(),
        }
    }
}

/// Lien affichable (pages web seulement).
pub(crate) fn web_url(u: &str) -> bool {
    Url::parse(u).is_ok_and(|u| u.scheme() == "https")
}

/// Téléchargement autorisé : en HTTPS (et le faux serveur des tests).
pub(crate) fn download_allowed(url: &Url) -> bool {
    url.scheme() == "https" || (cfg!(test) && url.host_str() == Some("127.0.0.1"))
}

pub struct Net {
    pub urls: Urls,
    http: Client,
    downloads: Client,
    cache_dir: PathBuf,
}

impl Net {
    pub fn new(urls: Urls, cache_dir: PathBuf) -> Self {
        let http = Client::builder()
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(40))
            .connect_timeout(Duration::from_secs(10))
            .build()
            .expect("client HTTP");
        let policy = redirect::Policy::custom(|attempt| {
            if attempt.previous().len() > 5 {
                attempt.error(crate::i18n::pick("trop de redirections", "too many redirects"))
            } else if download_allowed(attempt.url()) {
                attempt.follow()
            } else {
                attempt.stop()
            }
        });
        let downloads = Client::builder()
            .user_agent(USER_AGENT)
            .redirect(policy)
            .connect_timeout(Duration::from_secs(15))
            .read_timeout(Duration::from_secs(60))
            .build()
            .expect("client HTTP");
        Self { urls, http, downloads, cache_dir }
    }

    pub fn downloads(&self) -> &Client {
        &self.downloads
    }

    fn cache_path(&self, url: &str) -> PathBuf {
        self.cache_dir.join(format!("{:x}.cache", stable_id_of(url)))
    }

    /// Contenu d'une adresse : la copie récente du disque, sinon téléchargé
    /// (et la dernière copie si le site ne répond pas).
    pub async fn get(&self, url: &str, refresh: bool, source: &str) -> AppResult<Vec<u8>> {
        let path = self.cache_path(url);
        let fresh = fs::metadata(&path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age < CACHE_AGE);
        if fresh && !refresh && let Ok(bytes) = fs::read(&path) {
            return Ok(bytes);
        }
        match self.fetch(url, source).await {
            Ok(bytes) => {
                let _ = fs::create_dir_all(&self.cache_dir);
                let tmp = path.with_extension("tmp");
                if fs::write(&tmp, &bytes).is_ok() {
                    let _ = fs::rename(&tmp, &path);
                }
                Ok(bytes)
            }
            Err(e) => fs::read(&path).map_err(|_| e),
        }
    }

    async fn fetch(&self, url: &str, source: &str) -> AppResult<Vec<u8>> {
        let unavailable = |detail: String| AppError::ListUnavailable { list: source.to_string(), detail };
        let mut response = self
            .http
            .get(url)
            .header("Accept", "application/json, application/xml;q=0.9, */*;q=0.8")
            .send()
            .await
            .map_err(|e| unavailable(e.without_url().to_string()))?;
        if !response.status().is_success() {
            return Err(unavailable(crate::tr!("erreur {}", "error {}", response.status().as_u16())));
        }
        let mut out = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|e| unavailable(e.without_url().to_string()))? {
            out.extend_from_slice(&chunk);
            if out.len() > LIST_LIMIT {
                return Err(unavailable(crate::i18n::pick("réponse démesurée", "response too large").into()));
            }
        }
        Ok(out)
    }
}

// ---------------------------------------------------------------- État

pub struct ListsState {
    /// Mods installés depuis ces listes, par jeu (`installed-lists.json`).
    pub library: Mutex<Library>,
    pub library_path: PathBuf,
    /// Installations en cours : drapeau d'annulation par jeu.
    pub jobs: Mutex<HashMap<i64, Arc<AtomicBool>>>,
    pub net: Net,
    catalogs: tokio::sync::Mutex<HashMap<String, (i64, Arc<Catalog>)>>,
}

impl ListsState {
    pub fn new(library_path: PathBuf) -> Self {
        Self::with_urls(library_path, Urls::default())
    }

    pub fn with_urls(library_path: PathBuf, urls: Urls) -> Self {
        let cache_dir = library_path.with_file_name("lists-cache");
        Self {
            library: Mutex::new(Library::load(&library_path)),
            jobs: Mutex::new(HashMap::new()),
            net: Net::new(urls, cache_dir),
            catalogs: tokio::sync::Mutex::new(HashMap::new()),
            library_path,
        }
    }

    pub fn config(&self, game: &GameDef) -> Option<GameInstall> {
        self.library.lock().expect("library").games.get(&game_id(game.id)).cloned()
    }

    /// La liste d'un jeu (gardée six heures en mémoire).
    pub async fn catalog(&self, game: &GameDef, refresh: bool) -> AppResult<Arc<Catalog>> {
        let folder = self.config(game).map(|c| c.folder);
        // Beat Saber : la liste dépend de la version du jeu installée (relue après une mise à jour).
        // Les textes (catégories, remarques) suivent la langue de l'interface.
        let key = format!(
            "{}|{}|{}|{}",
            game.id,
            game.variant(folder.as_deref()),
            folder.as_deref().map(|f| game.cache_hint(f)).unwrap_or_default(),
            crate::i18n::english()
        );
        let mut slot = self.catalogs.lock().await;
        if !refresh && let Some((at, c)) = slot.get(&key) && now() - at < CACHE_AGE.as_secs() as i64 {
            return Ok(c.clone());
        }
        let catalog = Arc::new(game.fetch_catalog(&self.net, folder.as_deref(), refresh).await?);
        slot.insert(key, (now(), catalog.clone()));
        Ok(catalog)
    }
}

// ---------------------------------------------------------------- Commandes

/// Un jeu de ces listes, pour l'interface.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListGameView {
    pub id: &'static str,
    pub game_id: i64,
    pub name: &'static str,
    pub list_name: &'static str,
    pub list_url: &'static str,
    pub loader: &'static str,
    pub steam_app_id: u32,
    pub cover: String,
    pub hero: String,
    pub about: &'static str,
}

fn view(g: &GameDef) -> ListGameView {
    ListGameView {
        id: g.id,
        game_id: game_id(g.id),
        name: g.name,
        list_name: g.list_name(),
        list_url: g.list_url,
        loader: g.loader_name,
        steam_app_id: g.steam_app_id,
        cover: format!("https://cdn.cloudflare.steamstatic.com/steam/apps/{}/library_600x900.jpg", g.steam_app_id),
        hero: format!("https://cdn.cloudflare.steamstatic.com/steam/apps/{}/library_hero.jpg", g.steam_app_id),
        about: g.about(),
    }
}

fn def(game: &str) -> AppResult<&'static GameDef> {
    games::game(game).ok_or_else(|| AppError::InvalidArgument(crate::tr!("jeu inconnu : {game}", "unknown game: {game}")))
}

fn others(state: &AppState) -> Vec<Library> {
    vec![
        state.library.lock().expect("library").clone(),
        state.nexus.library.lock().expect("library").clone(),
        state.thunderstore.library.lock().expect("library").clone(),
    ]
}

#[tauri::command]
pub fn cl_games() -> Vec<ListGameView> {
    games::GAMES.iter().map(view).collect()
}

#[tauri::command]
pub fn cl_overview(state: State<'_, AppState>) -> Vec<GameInstall> {
    state.lists.overview()
}

#[tauri::command]
pub async fn cl_mods(state: State<'_, AppState>, game: String, refresh: Option<bool>) -> AppResult<Vec<ListMod>> {
    let g = def(&game)?;
    Ok(state.lists.catalog(g, refresh.unwrap_or(false)).await?.mods.clone())
}

#[tauri::command]
pub fn cl_configure(state: State<'_, AppState>, game: String, folder: String) -> AppResult<GameInstall> {
    state.lists.configure(def(&game)?, &folder)
}

#[tauri::command]
pub async fn cl_loader(state: State<'_, AppState>, game: String) -> AppResult<install::LoaderState> {
    state.lists.loader_state(def(&game)?).await
}

#[tauri::command]
pub async fn cl_install(
    state: State<'_, AppState>,
    game: String,
    id: String,
    version: Option<String>,
    on_event: Channel<InstallEvent>,
) -> AppResult<install::ListInstalled> {
    let emit: Arc<dyn Fn(InstallEvent) + Send + Sync> = Arc::new(move |e| {
        let _ = on_event.send(e);
    });
    let others = others(&state);
    state.lists.install(def(&game)?, &id, version, &others, emit).await
}

#[tauri::command]
pub fn cl_cancel(state: State<'_, AppState>, game: String) {
    if let Ok(g) = def(&game) {
        state.lists.cancel(g);
    }
}

#[tauri::command]
pub fn cl_uninstall(state: State<'_, AppState>, game_id: i64, mod_id: i64) -> AppResult<UninstallResult> {
    state.lists.remove(game_id, mod_id)
}

#[tauri::command]
pub fn cl_set_enabled(state: State<'_, AppState>, game_id: i64, mod_id: i64, enabled: bool) -> AppResult<GameInstall> {
    let others = others(&state);
    state.lists.set_enabled(game_id, mod_id, enabled, &others)
}

/// Passe à un profil de mods.
#[tauri::command]
pub fn cl_apply_profile(state: State<'_, AppState>, game_id: i64, name: String) -> AppResult<crate::toggle::ProfileApplied> {
    let others = others(&state);
    state.lists.apply_profile(game_id, &name, &others)
}

/// Crée, renomme ou supprime un profil.
#[tauri::command]
pub fn cl_edit_profiles(state: State<'_, AppState>, game_id: i64, edit: crate::toggle::ProfileEdit) -> AppResult<GameInstall> {
    state.lists.edit_profiles(game_id, &edit)
}

#[tauri::command]
pub async fn cl_updates(state: State<'_, AppState>, game_id: i64) -> AppResult<Vec<install::ListUpdate>> {
    state.lists.updates(game_id).await
}

/// Lance l'outil du chargeur (OWML d'Outer Wilds prépare le jeu, puis le lance).
#[tauri::command]
pub fn cl_launch(state: State<'_, AppState>, game: String) -> AppResult<()> {
    state.lists.launch(def(&game)?)
}

#[tauri::command]
pub fn cl_reveal(app: AppHandle, state: State<'_, AppState>, game_id: i64, mod_id: Option<i64>) -> AppResult<()> {
    let path = state.lists.reveal_path(game_id, mod_id)?;
    app.opener()
        .reveal_item_in_dir(&path)
        .map_err(|e| AppError::Io(e.to_string()))
}

#[cfg(test)]
mod tests;
