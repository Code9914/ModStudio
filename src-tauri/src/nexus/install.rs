//! Installation des mods Nexus Mods : lien de téléchargement, archive,
//! placement (règles du jeu ou rangement choisi), manifeste.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

use super::browse::check_domain;
use super::engine_rules::{self, UnityLoader};
use super::local::{self, ToolOffer};
use super::models::{DownloadLink, NexusFile};
use super::rules::{self, RulePlan, GAME_DIR};
use super::NexusState;
use crate::archive;
use crate::error::{AppError, AppResult};
use crate::games::engine::{self, Engine, EngineInfo};
use crate::games::InstalledGame;
use crate::install_commands::{dependency_state, Orphan, UninstallResult};
use crate::installer::{
    self, Batch, DownloadRequest, GameInstall, InstallEvent, InstalledMod, Layout, Library, ModPart, PlaceInput,
    STAGING_DIR,
};
use crate::toggle::{self, ProfileApplied, ProfileEdit};

/// Ce que l'utilisateur a demandé d'installer.
#[derive(Debug, Clone)]
pub struct InstallRequest {
    pub game_id: i64,
    pub game_domain: String,
    pub mod_id: i64,
    pub file_id: i64,
    /// Clé et expiration d'un lien `nxm://` (comptes sans Premium).
    pub nxm: Option<(String, i64)>,
    /// Mod qui a besoin de celui-ci (installation d'un mod nécessaire).
    pub required_by: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NexusInstalled {
    pub record: InstalledMod,
    /// Remarque à afficher (plugins non activés…).
    pub notice: Option<String>,
    /// Outil de mods absent du jeu, sans lequel ce mod ne se chargera pas.
    pub needs_tool: Option<ToolOffer>,
}

/// Dossiers et règles d'un jeu, pour l'interface.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameSetup {
    pub config: Option<GameInstall>,
    pub rules: Option<String>,
    /// Dossier des mods habituel du jeu, s'il est connu.
    pub suggested_folder: Option<String>,
    pub suggested_folder_exists: bool,
    /// Dossier d'installation du jeu (enregistré, sinon détecté).
    pub game_dir: Option<String>,
    pub game_dir_detected: bool,
    /// MelonLoader est installé dans le dossier du jeu.
    pub melonloader: bool,
    /// Le jeu, trouvé dans Steam ou Epic.
    pub installed: Option<InstalledGame>,
    /// Moteur du jeu et outils déjà présents dans son dossier.
    pub engine: Option<EngineInfo>,
    /// Outil de mods conseillé pour ce jeu.
    pub tool: Option<ToolOffer>,
    /// Rangement automatique (`unreal`, `unity`, `bethesda`…) qu'aurait le jeu
    /// configuré sur son dossier d'installation trouvé (sinon celui du jeu configuré).
    pub auto_rules: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub mod_id: i64,
    /// Le fichier installé a été remplacé sur Nexus (ancienne version, archivé, retiré).
    pub superseded: bool,
    pub category: String,
}

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Retire le drapeau d'installation en cours, même en cas d'erreur.
struct JobGuard<'a> {
    state: &'a NexusState,
    mod_id: i64,
}

impl Drop for JobGuard<'_> {
    fn drop(&mut self) {
        self.state.jobs.lock().expect("jobs").remove(&self.mod_id);
    }
}

/// Fichiers déjà installés par d'autres mods (des deux bibliothèques), par dossier.
pub fn owned_by_others(libraries: &[&Library], game_id: i64, mod_id: i64) -> HashMap<PathBuf, BTreeMap<String, String>> {
    let mut out: HashMap<PathBuf, BTreeMap<String, String>> = HashMap::new();
    for (index, lib) in libraries.iter().enumerate() {
        for g in lib.games.values() {
            // Le mod lui-même n'est exclu que dans sa propre bibliothèque (la première).
            let except = if index == 0 && g.game_id == game_id { mod_id } else { -1 };
            for (folder, rel, owner) in g.owned_files(except) {
                out.entry(folder).or_default().insert(rel, owner);
            }
        }
    }
    out
}

impl NexusState {
    fn save_library(&self, lib: &Library) -> AppResult<()> {
        lib.save(&self.library_path)
    }

    pub fn overview(&self) -> Vec<GameInstall> {
        let mut games: Vec<GameInstall> = self.library.lock().expect("library").games.values().cloned().collect();
        games.sort_by(|a, b| a.game_name.to_lowercase().cmp(&b.game_name.to_lowercase()));
        games
    }

    pub fn setup(&self, game_id: i64) -> GameSetup {
        let config = self.library.lock().expect("library").games.get(&game_id).cloned();
        let rules = rules::rules_for(game_id).map(str::to_string);
        let suggested = rules::content_folder(game_id);
        let suggested_exists = suggested.as_deref().is_some_and(|p| Path::new(p).is_dir());
        let saved_dir = config.as_ref().and_then(|c| c.extra_folders.get(GAME_DIR).cloned());
        let (game_dir, detected) = match saved_dir {
            Some(d) => (Some(d), false),
            None if game_id == rules::BONELAB => (rules::detect_bonelab_dir(), true),
            None => (None, false),
        };
        GameSetup {
            melonloader: game_dir.as_deref().is_some_and(rules::has_melonloader),
            config,
            rules,
            suggested_folder: suggested,
            suggested_folder_exists: suggested_exists,
            game_dir_detected: detected && game_dir.is_some(),
            game_dir: game_dir.map(|d| d.to_string_lossy().into_owned()),
            installed: None,
            engine: None,
            tool: None,
            auto_rules: None,
        }
    }

    /// Enregistre le dossier des mods d'un jeu (et, pour les jeux à règles, le
    /// dossier d'installation du jeu).
    #[allow(clippy::too_many_arguments)]
    pub fn configure(
        &self,
        game_id: i64,
        domain: &str,
        game_name: String,
        icon: Option<String>,
        folder: String,
        layout: Layout,
        create: bool,
        game_dir: Option<String>,
    ) -> AppResult<GameInstall> {
        if game_id <= 0 {
            return Err(AppError::InvalidArgument(format!("jeu {game_id}")));
        }
        check_domain(domain)?;
        let folder = PathBuf::from(folder.trim());
        if create && folder.is_absolute() && folder.parent().is_some() && !folder.exists() {
            fs::create_dir_all(&folder).map_err(installer::io_err)?;
        }
        installer::check_folder(&folder)?;
        let game_dir = match game_dir.map(|d| d.trim().to_string()).filter(|d| !d.is_empty()) {
            Some(d) => {
                let p = PathBuf::from(d);
                if !p.is_absolute() || !p.is_dir() {
                    return Err(AppError::InvalidArgument("le dossier du jeu n'existe pas.".into()));
                }
                Some(p)
            }
            None => None,
        };

        let mut lib = self.library.lock().expect("library");
        let entry = lib.games.entry(game_id).or_insert_with(|| GameInstall {
            game_id,
            game_name: game_name.clone(),
            folder: folder.clone(),
            layout,
            ..Default::default()
        });
        if !entry.mods.is_empty() {
            let moving_mods = entry.folder != folder;
            let moving_game = game_dir.is_some() && entry.extra_folders.get(GAME_DIR) != game_dir.as_ref();
            if moving_mods || (moving_game && entry.extra_folders.contains_key(GAME_DIR)) {
                let n = entry.mods.len();
                return Err(AppError::InvalidArgument(format!(
                    "désinstalle d'abord {} de ce jeu pour changer de dossier.",
                    if n > 1 { format!("les {n} mods") } else { "le mod".to_string() }
                )));
            }
        }
        entry.folder = folder;
        entry.layout = layout;
        entry.game_name = game_name;
        entry.domain = Some(domain.to_string());
        // Règles propres au jeu, sinon d'après son moteur (dossier d'installation choisi).
        let folder_rules = self.folder_rules(game_id, &entry.folder, domain);
        entry.rules = folder_rules.map(str::to_string);
        if icon.is_some() {
            entry.icon = icon;
        }
        if let Some(d) = game_dir {
            entry.extra_folders.insert(GAME_DIR.to_string(), d);
        }
        let result = entry.clone();
        self.save_library(&lib)?;
        Ok(result)
    }

    /// Lien de téléchargement d'un fichier (direct en Premium, sinon via `nxm`).
    async fn download_link(&self, req: &InstallRequest) -> AppResult<String> {
        let key = self.key()?;
        let path = format!(
            "/games/{}/mods/{}/files/{}/download_link.json",
            req.game_domain, req.mod_id, req.file_id
        );
        let query: Vec<(&str, String)> = match &req.nxm {
            Some((k, expires)) => vec![("key", k.clone()), ("expires", expires.to_string())],
            None => Vec::new(),
        };
        let links: Vec<DownloadLink> = match self.client.rest(&path, &key, &query).await {
            Ok(l) => l,
            Err(AppError::NexusApi { status: 403, .. }) | Err(AppError::NexusApi { status: 410, .. }) => {
                return Err(if req.nxm.is_some() { AppError::NexusLinkExpired } else { AppError::NexusPremiumRequired });
            }
            Err(AppError::NexusKeyInvalid) => {
                self.mark_rejected();
                return Err(AppError::NexusKeyInvalid);
            }
            Err(e) => return Err(e),
        };
        links
            .into_iter()
            .map(|l| l.uri)
            .find(|u| super::client::download_url_allowed(u, self.client.base()))
            .ok_or_else(|| AppError::DownloadRefused("aucun lien de téléchargement Nexus Mods valable".into()))
    }

    pub async fn install(
        &self,
        req: InstallRequest,
        other_library: &Library,
        emit: Arc<dyn Fn(InstallEvent) + Send + Sync>,
    ) -> AppResult<NexusInstalled> {
        check_domain(&req.game_domain)?;
        if req.game_id <= 0 || req.mod_id <= 0 || req.file_id <= 0 {
            return Err(AppError::InvalidArgument("identifiants invalides".into()));
        }
        let mod_id = req.mod_id;
        let config = {
            let lib = self.library.lock().expect("library");
            lib.games.get(&req.game_id).cloned().ok_or(AppError::NoGameFolder)?
        };
        if !config.folder.is_dir() {
            return Err(AppError::Io(format!("le dossier des mods n'existe plus : {}", config.folder.display())));
        }

        let cancel = Arc::new(AtomicBool::new(false));
        {
            let mut jobs = self.jobs.lock().expect("jobs");
            if jobs.contains_key(&mod_id) {
                return Err(AppError::InstallInProgress);
            }
            jobs.insert(mod_id, cancel.clone());
        }
        let _guard = JobGuard { state: self, mod_id };
        emit(InstallEvent::Preparing);

        // Le fichier demandé et la fiche du mod.
        let files = self.list_files(req.game_id, mod_id).await?;
        let file: NexusFile = files
            .into_iter()
            .find(|f| f.file_id == req.file_id)
            .ok_or(AppError::NexusNotFound)?;
        let detail = self.get_mod(req.game_id, mod_id).await?;
        let url = self.download_link(&req).await?;

        // Téléchargement dans le dossier de travail.
        let staging = config.folder.join(STAGING_DIR);
        let job_dir = staging.join(format!("nexus-{mod_id}-{}", file.file_id));
        let _ = fs::remove_dir_all(&job_dir);
        fs::create_dir_all(&job_dir).map_err(installer::io_err)?;
        let archive_path = job_dir.join("download.part");
        let cleanup = || {
            let _ = fs::remove_dir_all(&job_dir);
            let _ = fs::remove_dir(&staging); // seulement s'il est vide
        };

        let progress_emit = emit.clone();
        let progress = move |received: u64, total: u64| progress_emit(InstallEvent::Downloading { received, total });
        if let Err(e) = installer::download(
            self.client.downloads(),
            DownloadRequest {
                url: &url,
                token: None,
                dest: &archive_path,
                expected_size: file.bytes().max(0) as u64,
                expected_md5: None,
            },
            &cancel,
            &progress,
        )
        .await
        {
            cleanup();
            return Err(e);
        }
        emit(InstallEvent::Verifying);

        // Fichiers des autres mods, à ne jamais écraser.
        let others = {
            let lib = self.library.lock().expect("library");
            owned_by_others(&[&lib, other_library], req.game_id, mod_id)
        };
        let mut old = config.mods.get(&mod_id).cloned();
        if let Some(o) = old.as_mut().filter(|o| o.disabled) {
            // Nouvelle version d'un mod désactivé : l'ancienne, mise de côté, disparaît.
            let root = self.disabled_root();
            toggle::discard(&config, o, &toggle::Ctx { store_root: &root, source: "nexus" });
            o.files.clear();
            o.dirs.clear();
            o.parts.clear();
        }
        let mod_name = detail.summary.name.clone();
        let domain = req.game_domain.clone();
        // Jeux rangés selon leur moteur : le dossier configuré est celui du jeu.
        let engine_info = match config.rules.as_deref() {
            Some("unreal" | "unity" | "bethesda") => Some(
                local::detect_engine(&config.folder, &domain).ok_or_else(|| {
                    AppError::UnknownLayout(format!(
                        "ModStudio ne reconnaît plus le jeu dans {}. Vérifie le dossier du jeu.",
                        config.folder.display()
                    ))
                })?,
            ),
            _ => None,
        };
        let loader = match &engine_info {
            Some(info) if config.rules.as_deref() == Some("unity") => {
                let hints = if info.tools.is_empty() { self.hints(&domain).await } else { Default::default() };
                Some(match local::chosen_tool(req.game_id, info, &hints) {
                    Some("melonloader") => UnityLoader::MelonLoader,
                    _ => UnityLoader::BepInEx,
                })
            }
            _ => None,
        };
        let file_name = file.uri.clone().unwrap_or_else(|| format!("{}.zip", file.name));
        let limit = installer::extraction_limit(file.bytes().max(0) as u64, 0);
        let blocking_emit = emit.clone();
        let blocking_cancel = cancel.clone();
        let (config_b, archive_b, job_b, old_b) = (config.clone(), archive_path.clone(), job_dir.clone(), old.clone());
        let (engine_b, domain_b) = (engine_info.clone(), domain.clone());

        type Outcome = (Vec<installer::PlacedBatch>, Option<String>, Option<PathBuf>, Vec<String>);
        let placed = tokio::task::spawn_blocking(move || -> AppResult<Outcome> {
            let extracted = job_b.join("extracted");
            let needs_extract = config_b.rules.is_some() || config_b.layout != Layout::Archive;
            if needs_extract {
                let progress = |done: u64, total: u64| blocking_emit(InstallEvent::Extracting { done, total });
                archive::extract(&archive_b, &extracted, limit, &blocking_cancel, &progress)?;
            }
            if blocking_cancel.load(Ordering::Relaxed) {
                return Err(AppError::Cancelled);
            }
            blocking_emit(InstallEvent::Finishing);

            let mut root = None;
            let mut detected_dir = None;
            let mut plugins = Vec::new();
            let mut new_batches: BTreeMap<String, Vec<(PathBuf, String)>> = BTreeMap::new();
            let mut ruled: Option<RulePlan> = None;
            match (config_b.rules.as_deref(), &engine_b) {
                (Some("bonelab"), _) => {
                    let plan = rules::bonelab_plan(&extracted, &mod_name)?;
                    if plan.needs_game_dir() && !config_b.extra_folders.contains_key(GAME_DIR) {
                        // Dossier du jeu pas encore indiqué : on le cherche.
                        detected_dir = Some(rules::detect_bonelab_dir().ok_or_else(|| AppError::NeedGameDir(config_b.game_name.clone()))?);
                    }
                    ruled = Some(plan);
                }
                (Some("unreal"), Some(info)) => {
                    let project = info.project.clone().unwrap_or_default();
                    ruled = Some(engine_rules::unreal_plan(&extracted, &config_b.folder, &project, &mod_name)?.0);
                }
                (Some("unity"), Some(info)) => {
                    let loader = loader.unwrap_or(UnityLoader::BepInEx);
                    ruled = Some(engine_rules::unity_plan(&extracted, loader, &mod_name, &info.tools)?.0);
                }
                (Some("bethesda"), Some(_)) => {
                    let game = engine::bethesda(&domain_b).ok_or_else(|| AppError::UnknownLayout("jeu Bethesda inconnu".into()))?;
                    let (plan, p) = engine_rules::bethesda_plan(&extracted, game)?;
                    plugins = p;
                    ruled = Some(plan);
                }
                _ => {
                    let empty = BTreeMap::new();
                    let (r, moves) = installer::layout_moves(&PlaceInput {
                        folder: &config_b.folder,
                        layout: config_b.layout,
                        mod_id,
                        name_id: &installer::safe_name(&mod_name),
                        archive: &archive_b,
                        filename: &file_name,
                        extracted: needs_extract.then_some(extracted.as_path()),
                        old: old_b.as_ref(),
                        others: &empty,
                    })?;
                    root = r;
                    new_batches.insert(String::new(), moves);
                }
            }
            if let Some(mut plan) = ruled {
                // Fichiers vides à créer (`enabled.txt` d'UE4SS), sauf s'ils sont dans l'archive.
                let generated = std::mem::take(&mut plan.generated);
                let present: BTreeSet<String> = plan
                    .batches
                    .get("")
                    .map(|m| m.iter().map(|(_, d)| d.to_ascii_lowercase()).collect())
                    .unwrap_or_default();
                let gen_dir = job_b.join("generated");
                for (i, rel) in generated.into_iter().enumerate() {
                    if present.contains(&rel.to_ascii_lowercase()) {
                        continue;
                    }
                    fs::create_dir_all(&gen_dir).map_err(installer::io_err)?;
                    let src = gen_dir.join(i.to_string());
                    fs::write(&src, b"").map_err(installer::io_err)?;
                    plan.push_root(src, rel);
                }
                new_batches = plan.batches;
            }

            // Un lot par dossier touché (nouvelle version ou ancienne).
            let mut keys: BTreeSet<String> = new_batches.keys().cloned().collect();
            if let Some(o) = &old_b {
                keys.insert(String::new());
                keys.extend(o.parts.iter().map(|p| p.key.clone()));
            }
            let mut batches = Vec::new();
            for key in keys {
                let folder = match config_b.folder_for(&key) {
                    Some(f) => f.to_path_buf(),
                    None if key == GAME_DIR => detected_dir.clone().ok_or_else(|| AppError::NeedGameDir(config_b.game_name.clone()))?,
                    None => continue,
                };
                let (old_files, old_dirs) = match (&old_b, key.as_str()) {
                    (Some(o), "") => (o.files.clone(), o.dirs.clone()),
                    (Some(o), k) => o
                        .parts
                        .iter()
                        .find(|p| p.key == k)
                        .map(|p| (p.files.clone(), p.dirs.clone()))
                        .unwrap_or_default(),
                    (None, _) => Default::default(),
                };
                batches.push(Batch {
                    others: others.get(&folder).cloned().unwrap_or_default(),
                    moves: new_batches.remove(&key).unwrap_or_default(),
                    key,
                    folder,
                    old_files,
                    old_dirs,
                });
            }
            Ok((installer::place_batches(batches)?, root, detected_dir, plugins))
        })
        .await
        .map_err(|e| AppError::Io(e.to_string()))
        .and_then(|r| r);
        cleanup();
        let (placed, root, detected_dir, plugins) = placed?;

        let (auto, required_by) = dependency_state(req.required_by, old.as_ref());
        let mut record = InstalledMod {
            mod_id,
            auto,
            required_by,
            name: detail.summary.name.clone(),
            logo: detail.summary.thumbnail_url.clone().or(detail.summary.picture_url.clone()),
            version: file.version.clone().filter(|v| !v.trim().is_empty()),
            modfile_id: file.file_id,
            size: file.bytes(),
            installed_at: now(),
            layout: config.layout,
            root,
            file_name: Some(file.name.clone()),
            file_uid: file.uid.clone(),
            ..Default::default()
        };
        for batch in placed {
            if batch.key.is_empty() {
                record.files = batch.files;
                record.dirs = batch.dirs;
            } else if !batch.files.is_empty() {
                record.parts.push(ModPart {
                    key: batch.key,
                    files: batch.files,
                    dirs: batch.dirs,
                });
            }
        }

        let game_dir = detected_dir.clone().or_else(|| config.extra_folders.get(GAME_DIR).cloned());
        let mut notice = None;

        // Plugins des jeux Bethesda : activés à la fin de l'ordre de chargement.
        let old_plugins = old.as_ref().map(|o| o.plugins.clone()).unwrap_or_default();
        let kept: Vec<String> = old_plugins
            .iter()
            .filter(|p| plugins.iter().any(|n| n.eq_ignore_ascii_case(p)))
            .cloned()
            .collect();
        let dropped: Vec<String> = old_plugins.iter().filter(|p| !kept.contains(p)).cloned().collect();
        if let Some(path) = engine::bethesda(&domain).and_then(|g| self.plugins_path(g).map(|p| (p, g.star_format))) {
            if !dropped.is_empty() {
                let _ = engine_rules::deactivate_plugins(&path.0, &dropped);
            }
            if !plugins.is_empty() {
                match engine_rules::activate_plugins(&path.0, &plugins, path.1) {
                    Ok(added) => record.plugins = kept.into_iter().chain(added).collect(),
                    Err(e) => {
                        record.plugins = kept;
                        notice = Some(format!(
                            "Les plugins de ce mod n'ont pas pu être activés ({e}). Active-les depuis le menu Mods du jeu."
                        ));
                    }
                }
            }
        }

        // Outil de mods dont ce mod a besoin, s'il manque.
        let needs_tool = self.missing_tool(&req, &config, engine_info.as_ref(), &record, game_dir.as_deref()).await;

        let mut lib = self.library.lock().expect("library");
        if let Some(game) = lib.games.get_mut(&req.game_id) {
            if let Some(dir) = detected_dir {
                game.extra_folders.entry(GAME_DIR.to_string()).or_insert(dir);
            }
            game.mods.insert(mod_id, record.clone());
            toggle::sync_active(game);
        }
        self.save_library(&lib)?;
        Ok(NexusInstalled { record, notice, needs_tool })
    }

    /// Dossier des mods désactivés hors des dossiers des jeux.
    pub fn disabled_root(&self) -> PathBuf {
        self.library_path.with_file_name("disabled")
    }

    /// Plugins Bethesda d'un mod qu'on active ou désactive.
    fn toggle_plugins(&self, domain: &str, record: &InstalledMod, enabled: bool) {
        if record.plugins.is_empty() {
            return;
        }
        let Some(g) = engine::bethesda(domain) else { return };
        let Some(path) = self.plugins_path(g) else { return };
        if enabled {
            let _ = engine_rules::activate_plugins(&path, &record.plugins, g.star_format);
        } else {
            let _ = engine_rules::deactivate_plugins(&path, &record.plugins);
        }
    }

    /// Active ou désactive un mod installé. `other_library` : mods installés
    /// depuis mod.io, jamais écrasés.
    pub fn set_enabled(&self, game_id: i64, mod_id: i64, enabled: bool, other_library: &Library) -> AppResult<GameInstall> {
        if self.jobs.lock().expect("jobs").contains_key(&mod_id) {
            return Err(AppError::InstallInProgress);
        }
        let root = self.disabled_root();
        let ctx = toggle::Ctx { store_root: &root, source: "nexus" };
        let mut lib = self.library.lock().expect("library");
        let mut game = lib.games.remove(&game_id).ok_or(AppError::NotFound)?;
        let record = game.mods.get(&mod_id).cloned().ok_or(AppError::NotFound);
        let result = record.and_then(|record| {
            if enabled {
                let others = toggle::others_map(&game, mod_id, &[&lib, other_library]);
                toggle::enable(&mut game, mod_id, &ctx, &others)?;
            } else {
                toggle::disable(&mut game, mod_id, &ctx)?;
            }
            self.toggle_plugins(game.domain.as_deref().unwrap_or_default(), &record, enabled);
            Ok(())
        });
        let out = game.clone();
        lib.games.insert(game_id, game);
        self.save_library(&lib)?;
        result.map(|()| out)
    }

    /// Passe à un profil de mods.
    pub fn apply_profile(&self, game_id: i64, name: &str, other_library: &Library) -> AppResult<ProfileApplied> {
        if !self.jobs.lock().expect("jobs").is_empty() {
            return Err(AppError::InstallInProgress);
        }
        let root = self.disabled_root();
        let ctx = toggle::Ctx { store_root: &root, source: "nexus" };
        let mut lib = self.library.lock().expect("library");
        let mut game = lib.games.remove(&game_id).ok_or(AppError::NotFound)?;
        let domain = game.domain.clone().unwrap_or_default();
        let result = {
            let rest: &Library = &lib;
            let others_for = |g: &GameInstall, id: i64| toggle::others_map(g, id, &[rest, other_library]);
            let mut on_toggle = |m: &InstalledMod, on: bool| self.toggle_plugins(&domain, m, on);
            toggle::apply_profile(&mut game, name, &ctx, &others_for, &mut on_toggle)
        };
        let out = game.clone();
        lib.games.insert(game_id, game);
        self.save_library(&lib)?;
        result.map(|issues| ProfileApplied { game: out, issues })
    }

    /// Crée, renomme ou supprime un profil.
    pub fn edit_profiles(&self, game_id: i64, edit: &ProfileEdit) -> AppResult<GameInstall> {
        let mut lib = self.library.lock().expect("library");
        let game = lib.games.get_mut(&game_id).ok_or(AppError::NotFound)?;
        toggle::edit_profiles(game, edit)?;
        let out = game.clone();
        self.save_library(&lib)?;
        Ok(out)
    }

    /// `plugins.txt` d'un jeu Bethesda.
    pub fn plugins_path(&self, game: &engine::BethesdaGame) -> Option<PathBuf> {
        self.local_appdata.as_ref().map(|base| engine_rules::plugins_file(base, game))
    }

    /// L'outil qui manque au jeu pour charger ce mod, d'après les fichiers installés.
    async fn missing_tool(
        &self,
        req: &InstallRequest,
        config: &GameInstall,
        info: Option<&EngineInfo>,
        record: &InstalledMod,
        game_dir: Option<&Path>,
    ) -> Option<ToolOffer> {
        let lower: Vec<String> = record
            .files
            .iter()
            .chain(record.parts.iter().flat_map(|p| p.files.iter()))
            .map(|f| f.to_ascii_lowercase())
            .collect();
        let has = |needle: &str| lower.iter().any(|f| f.contains(needle));
        match config.rules.as_deref() {
            Some("bonelab") => {
                let code = record.parts.iter().any(|p| p.key == GAME_DIR && !p.files.is_empty());
                let melon = game_dir.is_some_and(rules::has_melonloader);
                (code && !melon).then(|| ToolOffer::github("melonloader", Some(config), None, &Default::default()))?
            }
            Some("unreal") => {
                let info = info?;
                let now = engine::present_tools(&config.folder, info.project.as_deref(), &req.game_domain);
                let needed = has("/ue4ss/mods/") || has("/win64/mods/") || has("/paks/logicmods/");
                (needed && !now.iter().any(|t| t == "ue4ss")).then(|| ToolOffer::github("ue4ss", Some(config), Some(info), &Default::default()))?
            }
            Some("unity") => {
                let info = info?;
                let now = engine::present_tools(&config.folder, None, &req.game_domain);
                let id = if has("bepinex/") {
                    (!now.iter().any(|t| t == "bepinex")).then_some(if info.engine == Engine::UnityIl2cpp { "bepinex6" } else { "bepinex5" })
                } else {
                    (!now.iter().any(|t| t == "melonloader")).then_some("melonloader")
                }?;
                // Le jeu a déjà l'autre chargeur : rien à proposer.
                if !now.is_empty() {
                    return None;
                }
                let hints = self.hints(&req.game_domain).await;
                ToolOffer::github(id, Some(config), Some(info), &hints)
            }
            Some("bethesda") => {
                let g = engine::bethesda(&req.game_domain)?;
                let now = engine::present_tools(&config.folder, None, &req.game_domain);
                // Plugins du script extender : `Data/SKSE/Plugins` pour `skse64`.
                let needed = has(&format!("data/{}/plugins/", g.extender.trim_end_matches(|c: char| c.is_ascii_digit())));
                (needed && !now.iter().any(|t| t == g.extender)).then(|| ToolOffer::extender(&req.game_domain, info))?
            }
            _ => None,
        }
    }

    pub fn cancel(&self, mod_id: i64) {
        if let Some(flag) = self.jobs.lock().expect("jobs").get(&mod_id) {
            flag.store(true, Ordering::Relaxed);
        }
    }

    /// Désinstalle ; renvoie le dossier laissé en place et les mods nécessaires devenus inutiles.
    pub fn remove(&self, game_id: i64, mod_id: i64) -> AppResult<UninstallResult> {
        if self.jobs.lock().expect("jobs").contains_key(&mod_id) {
            return Err(AppError::InstallInProgress);
        }
        let mut lib = self.library.lock().expect("library");
        let game = lib.games.get_mut(&game_id).ok_or(AppError::NotFound)?;
        let record = game.mods.get(&mod_id).cloned().ok_or(AppError::NotFound)?;
        let removal = if record.disabled {
            // Désactivé : seuls ses fichiers mis de côté sont supprimés.
            let root = self.disabled_root();
            toggle::discard(game, &record, &toggle::Ctx { store_root: &root, source: "nexus" });
            installer::FullRemoval { remaining: Vec::new(), remaining_parts: Vec::new(), left_folder: None }
        } else {
            installer::uninstall_all(game, &record)
        };
        if !record.plugins.is_empty() && !record.disabled {
            let domain = game.domain.clone().unwrap_or_default();
            if let Some(path) = engine::bethesda(&domain).and_then(|g| self.plugins_path(g)) {
                let _ = engine_rules::deactivate_plugins(&path, &record.plugins);
            }
        }
        if !removal.complete() {
            let n = removal.remaining_count();
            let first = removal.first_remaining().cloned().unwrap_or_default();
            if let Some(r) = game.mods.get_mut(&mod_id) {
                r.files = removal.remaining;
                r.parts = removal.remaining_parts;
            }
            self.save_library(&lib)?;
            return Err(AppError::Io(format!(
                "{n} fichier(s) n'ont pas pu être supprimés, dont {first}. Ferme le jeu puis réessaie."
            )));
        }
        game.mods.remove(&mod_id);
        toggle::forget_mod(game, mod_id);

        // Mods nécessaires installés avec lui et dont plus aucun mod n'a besoin.
        let mut orphans = Vec::new();
        for other in game.mods.values_mut() {
            if other.required_by.contains(&mod_id) {
                other.required_by.retain(|id| *id != mod_id);
                if other.auto && other.required_by.is_empty() {
                    orphans.push(Orphan {
                        mod_id: other.mod_id,
                        name: other.name.clone(),
                    });
                }
            }
        }
        self.save_library(&lib)?;
        Ok(UninstallResult {
            left_folder: removal.left_folder,
            orphans,
        })
    }

    /// Mods installés dont le fichier a été remplacé sur Nexus Mods.
    pub async fn updates(&self, game_id: i64) -> AppResult<Vec<UpdateInfo>> {
        let installed: Vec<(i64, String)> = self
            .library
            .lock()
            .expect("library")
            .games
            .get(&game_id)
            .map(|g| g.mods.values().filter_map(|m| Some((m.mod_id, m.file_uid.clone()?))).collect())
            .unwrap_or_default();
        let uids: Vec<String> = installed.iter().map(|(_, u)| u.clone()).collect();
        let files = self.files_by_uid(&uids).await?;
        Ok(installed
            .into_iter()
            .filter_map(|(mod_id, uid)| {
                let f = files.iter().find(|f| f.uid.as_deref() == Some(uid.as_str()))?;
                Some(UpdateInfo {
                    mod_id,
                    superseded: matches!(f.category.as_str(), "OLD_VERSION" | "ARCHIVED" | "REMOVED"),
                    category: f.category.clone(),
                })
            })
            .collect())
    }

    pub fn reveal_path(&self, game_id: i64, mod_id: Option<i64>) -> AppResult<PathBuf> {
        let lib = self.library.lock().expect("library");
        let game = lib.games.get(&game_id).ok_or(AppError::NotFound)?;
        let Some(mod_id) = mod_id else { return Ok(game.folder.clone()) };
        let record = game.mods.get(&mod_id).ok_or(AppError::NotFound)?;
        if let Some(rel) = record.root.as_ref().or(record.files.first()) {
            return Ok(installer::resolve_rel(&game.folder, rel).unwrap_or_else(|| game.folder.clone()));
        }
        // Mod de code seulement : son premier fichier dans le dossier du jeu.
        for part in &record.parts {
            if let (Some(dir), Some(rel)) = (game.extra_folders.get(&part.key), part.files.first()) {
                return Ok(installer::resolve_rel(dir, rel).unwrap_or_else(|| dir.clone()));
            }
        }
        Ok(game.folder.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::MemoryStore;
    use crate::installer::tests::{make_zip, tempdir};
    use crate::testutil::{fake_server_with, json_response};
    use std::sync::Mutex;

    fn zip_response(body: &[u8]) -> String {
        let mut s = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/zip\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        s.extend(body.iter().map(|&b| b as char));
        s
    }

    fn validate_ok() -> String {
        json_response(200, r#"{"user_id":7,"name":"Code","is_premium":true,"is_supporter":false}"#)
    }

    /// Faux Nexus : fichiers du mod, fiche, lien de téléchargement, archive.
    fn nexus_with(zip: Vec<u8>, link_status: u16) -> (crate::testutil::FakeServer, NexusState) {
        let server = fake_server_with(|base| {
            let root = base.trim_end_matches("/v1").to_string();
            let mut v = vec![
                validate_ok(),
                json_response(
                    200,
                    &format!(
                        r#"{{"data":{{"modFiles":[{{"fileId":316,"uid":"u316","name":"711 Sandbox PC","version":"1.0.0","category":"MAIN","date":5,"sizeInBytes":"{}"}}]}}}}"#,
                        zip.len()
                    ),
                ),
                json_response(
                    200,
                    r#"{"data":{"legacyMods":{"nodes":[{"modId":148,"name":"7-11 SANDBOX","thumbnailUrl":"https://staticdelivery.nexusmods.com/t.png"}]}}}"#,
                ),
            ];
            if link_status == 200 {
                v.push(json_response(200, &format!(r#"[{{"name":"Nexus CDN","short_name":"Nexus CDN","URI":"{root}/dl/x.zip"}}]"#)));
                v.push(zip_response(&zip));
            } else {
                v.push(json_response(link_status, r#"{"message":"premium only"}"#));
            }
            v
        });
        let state = NexusState::new(
            server.base.trim_end_matches("/v1"),
            Box::new(MemoryStore::default()),
            tempdir("nx-lib").join("installed-nexus.json"),
        );
        (server, state)
    }

    fn quiet() -> Arc<dyn Fn(InstallEvent) + Send + Sync> {
        Arc::new(|_| {})
    }

    fn request(nxm: Option<(String, i64)>) -> InstallRequest {
        InstallRequest {
            game_id: rules::BONELAB,
            game_domain: "bonelab".into(),
            mod_id: 148,
            file_id: 316,
            nxm,
            required_by: None,
        }
    }

    #[tokio::test]
    async fn installs_a_bonelab_pallet_then_removes_it() {
        let zip = make_zip(&[
            ("BaBaCorp.711/", b""),
            ("BaBaCorp.711/BaBaCorp.711.pallet.json", b"{}"),
            ("BaBaCorp.711/levels/711.bundle", b"carte"),
        ]);
        let (server, s) = nexus_with(zip, 200);
        s.set_key("cle").await.unwrap();
        let mods = tempdir("bl-mods");
        s.configure(rules::BONELAB, "bonelab", "BONELAB".into(), None, mods.to_string_lossy().into_owned(), Layout::Flat, false, None)
            .unwrap();

        let events = Arc::new(Mutex::new(Vec::<String>::new()));
        let ev = events.clone();
        let emit: Arc<dyn Fn(InstallEvent) + Send + Sync> = Arc::new(move |e| {
            let tag = serde_json::to_value(&e).unwrap()["phase"].as_str().unwrap().to_string();
            let mut v = ev.lock().unwrap();
            if v.last() != Some(&tag) {
                v.push(tag);
            }
        });
        let out = s.install(request(None), &Library::default(), emit).await.unwrap();
        assert_eq!(out.record.modfile_id, 316);
        assert_eq!(out.record.file_uid.as_deref(), Some("u316"));
        assert!(out.notice.is_none() && out.needs_tool.is_none());
        assert_eq!(fs::read(mods.join("BaBaCorp.711/levels/711.bundle")).unwrap(), b"carte");
        assert!(!mods.join(STAGING_DIR).exists());
        assert_eq!(*events.lock().unwrap(), vec!["preparing", "downloading", "verifying", "extracting", "finishing"]);

        // Le lien est demandé avec la clé, sans paramètres nxm (Premium).
        let link = server.request(3);
        assert!(link.line.starts_with("GET /v1/games/bonelab/mods/148/files/316/download_link.json"), "{}", link.line);
        assert_eq!(link.header("apikey").as_deref(), Some("cle"));
        assert_eq!(server.request(4).header("apikey"), None, "la clé ne part jamais vers le CDN");

        // Manifeste relu depuis le disque.
        let reloaded = Library::load(&s.library_path);
        assert!(reloaded.games[&rules::BONELAB].mods.contains_key(&148));

        assert!(s.remove(rules::BONELAB, 148).unwrap().left_folder.is_none());
        assert!(!mods.join("BaBaCorp.711").exists());
    }

    #[tokio::test]
    async fn code_mods_need_the_game_folder() {
        let zip = make_zip(&[("Mods/LabFusion.dll", b"dll")]);
        let (_server, s) = nexus_with(zip, 200);
        s.set_key("cle").await.unwrap();
        let mods = tempdir("bl-mods2");
        let game = tempdir("bl-game");
        fs::create_dir_all(game.join("Mods")).unwrap();
        s.configure(
            rules::BONELAB,
            "bonelab",
            "BONELAB".into(),
            None,
            mods.to_string_lossy().into_owned(),
            Layout::Flat,
            false,
            Some(game.to_string_lossy().into_owned()),
        )
        .unwrap();
        let out = s.install(request(None), &Library::default(), quiet()).await.unwrap();
        assert_eq!(out.record.parts, vec![ModPart { key: "game".into(), files: vec!["Mods/LabFusion.dll".into()], dirs: vec![] }]);
        assert_eq!(out.needs_tool.map(|t| t.id).as_deref(), Some("melonloader"));
        assert_eq!(fs::read(game.join("Mods/LabFusion.dll")).unwrap(), b"dll");
        assert_eq!(s.reveal_path(rules::BONELAB, Some(148)).unwrap(), game.join("Mods/LabFusion.dll"));

        s.remove(rules::BONELAB, 148).unwrap();
        assert!(!game.join("Mods/LabFusion.dll").exists());
        assert!(game.join("Mods").exists(), "le dossier Mods du jeu existait déjà : il reste");
    }

    #[tokio::test]
    async fn free_accounts_are_sent_to_the_website() {
        let zip = make_zip(&[("a/pallet.json", b"{}")]);
        let (_server, s) = nexus_with(zip.clone(), 403);
        s.set_key("cle").await.unwrap();
        let mods = tempdir("bl-mods3");
        s.configure(rules::BONELAB, "bonelab", "BONELAB".into(), None, mods.to_string_lossy().into_owned(), Layout::Flat, false, None)
            .unwrap();
        let err = s.install(request(None), &Library::default(), quiet()).await.unwrap_err();
        assert!(matches!(err, AppError::NexusPremiumRequired), "{err}");

        // Avec un lien nxm refusé : il a expiré.
        let (server, s) = nexus_with(zip, 403);
        s.set_key("cle").await.unwrap();
        s.configure(rules::BONELAB, "bonelab", "BONELAB".into(), None, mods.to_string_lossy().into_owned(), Layout::Flat, false, None)
            .unwrap();
        let err = s.install(request(Some(("k123".into(), 99))), &Library::default(), quiet()).await.unwrap_err();
        assert!(matches!(err, AppError::NexusLinkExpired), "{err}");
        assert!(server.request(3).line.contains("key=k123&expires=99"));
    }

    #[tokio::test]
    async fn never_overwrites_files_of_modio_mods() {
        let zip = make_zip(&[("BaBaCorp.711/BaBaCorp.711.pallet.json", b"{}")]);
        let (_server, s) = nexus_with(zip, 200);
        s.set_key("cle").await.unwrap();
        let mods = tempdir("bl-shared");
        s.configure(rules::BONELAB, "bonelab", "BONELAB".into(), None, mods.to_string_lossy().into_owned(), Layout::Flat, false, None)
            .unwrap();
        // Le même dossier sert aux mods installés depuis mod.io.
        let mut modio = Library::default();
        let mut g = GameInstall {
            game_id: 3809,
            game_name: "BONELAB".into(),
            folder: mods.clone(),
            layout: Layout::Flat,
            ..Default::default()
        };
        g.mods.insert(
            5,
            InstalledMod {
                mod_id: 5,
                name: "7/11 Extra Content".into(),
                files: vec!["BaBaCorp.711/BaBaCorp.711.pallet.json".into()],
                ..Default::default()
            },
        );
        modio.games.insert(3809, g);
        fs::create_dir_all(mods.join("BaBaCorp.711")).unwrap();
        fs::write(mods.join("BaBaCorp.711/BaBaCorp.711.pallet.json"), b"mod.io").unwrap();

        let err = s.install(request(None), &modio, quiet()).await.unwrap_err();
        let AppError::Conflict(text) = err else { panic!("conflit attendu : {err}") };
        assert!(text.contains("7/11 Extra Content"), "{text}");
        assert_eq!(fs::read(mods.join("BaBaCorp.711/BaBaCorp.711.pallet.json")).unwrap(), b"mod.io");
    }

    fn request_for(game_id: i64, domain: &str) -> InstallRequest {
        InstallRequest {
            game_id,
            game_domain: domain.into(),
            mod_id: 148,
            file_id: 316,
            nxm: None,
            required_by: None,
        }
    }

    #[tokio::test]
    async fn unreal_mods_go_where_ue4ss_and_the_game_load_them() {
        let zip = make_zip(&[
            ("PushToCharge/Scripts/main.lua", b"print('hi')"),
            ("PushToCharge.pak", b"pak"),
            ("readme.txt", b"lis-moi"),
        ]);
        let (_server, s) = nexus_with(zip, 200);
        s.set_key("cle").await.unwrap();
        let game = tempdir("ue-game");
        fs::create_dir_all(game.join("Pilgrim/Binaries/Win64")).unwrap();
        fs::create_dir_all(game.join("Pilgrim/Content/Paks")).unwrap();
        let g = s
            .configure(6534, "pilgrim", "PILGRIM".into(), None, game.to_string_lossy().into_owned(), Layout::Subfolder, false, None)
            .unwrap();
        assert_eq!(g.rules.as_deref(), Some("unreal"));

        let out = s.install(request_for(6534, "pilgrim"), &Library::default(), quiet()).await.unwrap();
        let mods = game.join("Pilgrim/Binaries/Win64/ue4ss/Mods/PushToCharge");
        assert_eq!(fs::read(mods.join("Scripts/main.lua")).unwrap(), b"print('hi')");
        assert!(mods.join("enabled.txt").is_file(), "UE4SS ne charge que les mods activés");
        assert_eq!(fs::read(game.join("Pilgrim/Content/Paks/~mods/PushToCharge.pak")).unwrap(), b"pak");
        assert!(!game.join("readme.txt").exists());
        assert_eq!(out.needs_tool.map(|t| t.id).as_deref(), Some("ue4ss"));

        s.remove(6534, 148).unwrap();
        assert!(!mods.exists());
        assert!(!game.join("Pilgrim/Content/Paks/~mods").exists());
        assert!(game.join("Pilgrim/Content/Paks").exists());
    }

    #[tokio::test]
    async fn bethesda_mods_go_to_data_and_get_activated() {
        let zip = make_zip(&[
            ("Better Lanterns/BetterLanterns.esp", b"esp"),
            ("Better Lanterns/textures/lantern.dds", b"dds"),
            ("Better Lanterns/SKSE/Plugins/lanterns.dll", b"dll"),
        ]);
        let (_server, mut s) = nexus_with(zip, 200);
        let appdata = tempdir("beth-appdata");
        s.local_appdata = Some(appdata.clone());
        let plugins = appdata.join("Skyrim Special Edition/plugins.txt");
        fs::create_dir_all(plugins.parent().unwrap()).unwrap();
        fs::write(&plugins, "# Ordre de chargement\r\n*Unofficial Skyrim Special Edition Patch.esp\r\n").unwrap();
        s.set_key("cle").await.unwrap();
        let game = tempdir("beth-game");
        fs::write(game.join("SkyrimSE.exe"), b"MZ").unwrap();
        fs::create_dir_all(game.join("Data")).unwrap();
        fs::write(game.join("Data/Skyrim.esm"), b"jeu").unwrap();
        let g = s
            .configure(1704, "skyrimspecialedition", "Skyrim SE".into(), None, game.to_string_lossy().into_owned(), Layout::Subfolder, false, None)
            .unwrap();
        assert_eq!(g.rules.as_deref(), Some("bethesda"));

        let out = s.install(request_for(1704, "skyrimspecialedition"), &Library::default(), quiet()).await.unwrap();
        assert_eq!(fs::read(game.join("Data/BetterLanterns.esp")).unwrap(), b"esp");
        assert!(game.join("Data/textures/lantern.dds").is_file());
        assert!(game.join("Data/SKSE/Plugins/lanterns.dll").is_file());
        assert_eq!(out.record.plugins, vec!["BetterLanterns.esp"]);
        let text = fs::read_to_string(&plugins).unwrap();
        assert!(text.ends_with("*BetterLanterns.esp\r\n"), "{text:?}");
        assert!(text.contains("*Unofficial Skyrim Special Edition Patch.esp"));
        assert_eq!(out.needs_tool.map(|t| (t.id, t.nexus_mod)), Some(("skse64".to_string(), Some(30379))));

        // Désactivé : fichiers mis de côté dans le dossier du jeu, plugin retiré.
        let g = s.set_enabled(1704, 148, false, &Library::default()).unwrap();
        assert!(g.mods[&148].disabled);
        assert!(!game.join("Data/BetterLanterns.esp").exists());
        assert!(game.join(".modstudio-disabled/nexus-148/Data/BetterLanterns.esp").is_file());
        assert!(!fs::read_to_string(&plugins).unwrap().contains("BetterLanterns"));
        // Réactivé : tout revient, le plugin aussi.
        s.set_enabled(1704, 148, true, &Library::default()).unwrap();
        assert!(game.join("Data/BetterLanterns.esp").is_file());
        assert!(fs::read_to_string(&plugins).unwrap().contains("*BetterLanterns.esp"));
        assert!(!game.join(".modstudio-disabled").exists());

        s.remove(1704, 148).unwrap();
        assert!(!game.join("Data/BetterLanterns.esp").exists());
        assert!(game.join("Data/Skyrim.esm").exists(), "les fichiers du jeu restent");
        let text = fs::read_to_string(&plugins).unwrap();
        assert!(!text.contains("BetterLanterns") && text.contains("Unofficial"), "{text:?}");
    }

    #[tokio::test]
    async fn required_mods_are_marked_and_offered_for_removal() {
        let zip = make_zip(&[("BoneLib/BoneLib.pallet.json", b"{}")]);
        let (_server, s) = nexus_with(zip, 200);
        s.set_key("cle").await.unwrap();
        let mods = tempdir("bl-req");
        s.configure(rules::BONELAB, "bonelab", "BONELAB".into(), None, mods.to_string_lossy().into_owned(), Layout::Flat, false, None)
            .unwrap();
        // Le mod 148 est installé comme mod nécessaire du mod 200.
        let mut req = request(None);
        req.required_by = Some(200);
        let out = s.install(req, &Library::default(), quiet()).await.unwrap();
        assert!(out.record.auto);
        assert_eq!(out.record.required_by, vec![200]);

        // Le mod 200 (sans fichiers ici) est désinstallé : le 148 ne sert plus.
        s.library.lock().unwrap().games.get_mut(&rules::BONELAB).unwrap().mods.insert(
            200,
            InstalledMod { mod_id: 200, name: "LabFusion".into(), ..Default::default() },
        );
        let r = s.remove(rules::BONELAB, 200).unwrap();
        assert_eq!(r.orphans.iter().map(|o| o.mod_id).collect::<Vec<_>>(), vec![148]);
        assert!(s.library.lock().unwrap().games[&rules::BONELAB].mods[&148].required_by.is_empty());
    }

    #[test]
    fn other_games_use_the_chosen_layout() {
        let s = NexusState::new("http://127.0.0.1:9", Box::new(MemoryStore::default()), tempdir("nx-gen").join("n.json"));
        let folder = tempdir("gen-mods");
        let g = s
            .configure(1704, "skyrimspecialedition", "Skyrim SE".into(), None, folder.to_string_lossy().into_owned(), Layout::Subfolder, false, None)
            .unwrap();
        assert!(g.rules.is_none());
        assert_eq!(g.domain.as_deref(), Some("skyrimspecialedition"));
        let setup = s.setup(1704);
        assert!(setup.config.is_some() && setup.rules.is_none() && setup.suggested_folder.is_none());
        assert!(s.configure(1704, "Bad Domain", "x".into(), None, folder.to_string_lossy().into_owned(), Layout::Flat, false, None).is_err());
    }
}
