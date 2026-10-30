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
use super::fomod;
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

/// Résultat d'une installation : mod installé, ou options FOMOD à choisir.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
#[allow(clippy::large_enum_variant)] // valeur de retour éphémère, envoyée telle quelle à l'interface
pub enum InstallOutcome {
    Installed(NexusInstalled),
    Choices(FomodPrompt),
}

#[cfg(test)]
impl InstallOutcome {
    pub fn done(self) -> NexusInstalled {
        match self {
            InstallOutcome::Installed(i) => i,
            InstallOutcome::Choices(_) => panic!("options FOMOD à choisir"),
        }
    }
}

/// Installateur FOMOD à présenter, avec le jeton qui permet de le terminer.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FomodPrompt {
    pub token: String,
    pub installer: fomod::Installer,
}

/// Installation en cours, téléchargée et extraite dans `job_dir`.
struct Pending {
    req: InstallRequest,
    file: NexusFile,
    name: String,
    logo: Option<String>,
    job_dir: PathBuf,
    staging: PathBuf,
}

/// Installation en attente de choix : options FOMOD, ou archives à installer
/// parmi celles que contient le fichier téléchargé.
pub struct FomodSession {
    pending: Pending,
    installer: fomod::Installer,
    /// Dossier qui contient le dossier `fomod`.
    root: PathBuf,
    /// Archives proposées (une par option de `installer`), à extraire une fois choisies.
    nested: Option<Vec<PathBuf>>,
    /// Profondeur des archives proposées (archive dans l'archive…).
    depth: u8,
}

/// Étape de l'installation après extraction.
enum Stage {
    Placed {
        placed: Vec<installer::PlacedBatch>,
        root: Option<String>,
        detected_dir: Option<PathBuf>,
        plugins: Vec<String>,
    },
    Choices {
        installer: fomod::Installer,
        root: PathBuf,
    },
    /// Le fichier ne contient que des archives : laquelle installer ?
    Archives {
        paths: Vec<PathBuf>,
        depth: u8,
    },
}

/// Archives imbriquées ouvertes d'office : au plus deux niveaux.
const NESTED_DEPTH: u8 = 2;

/// Ce qu'il faut pour placer les fichiers d'un mod extrait.
#[derive(Clone)]
struct PlanCtx {
    config: GameInstall,
    domain: String,
    mod_id: i64,
    mod_name: String,
    old: Option<InstalledMod>,
    /// Ancienne version désactivée, supprimée une fois la nouvelle en place.
    set_aside: Option<InstalledMod>,
    others: HashMap<PathBuf, BTreeMap<String, String>>,
    engine: Option<EngineInfo>,
    loader: Option<UnityLoader>,
    game_state: fomod::GameState,
    archive: PathBuf,
    file_name: String,
    job_dir: PathBuf,
    limit: u64,
}

/// Choix entre les archives d'un fichier (présenté comme un installateur FOMOD).
fn archive_choice(mod_name: &str, paths: &[PathBuf]) -> fomod::Installer {
    let plugins = paths
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let file = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            fomod::Plugin {
                name: p.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| file.clone()),
                description: format!("Archive « {file} », contenue dans le fichier téléchargé."),
                image: None,
                flags: Vec::new(),
                type_default: "Optional".into(),
                type_patterns: Vec::new(),
                files: vec![fomod::FileItem { source: i.to_string(), destination: None, folder: false, priority: 0 }],
            }
        })
        .collect();
    fomod::Installer {
        module_name: mod_name.to_string(),
        image: None,
        steps: vec![fomod::Step {
            name: "Ce fichier contient plusieurs archives".into(),
            visible: None,
            groups: vec![fomod::Group { name: "Archives à installer".into(), kind: "SelectAtLeastOne".into(), plugins }],
        }],
        required: Vec::new(),
        conditional: Vec::new(),
    }
}

/// Après l'extraction : archive imbriquée, installateur FOMOD, puis règles du
/// jeu ou rangement choisi. `extracted` est absent si l'archive se copie telle quelle.
fn plan_extracted(
    ctx: &PlanCtx,
    extracted: Option<&Path>,
    depth: u8,
    cancel: &AtomicBool,
    emit: &(dyn Fn(InstallEvent) + Send + Sync),
) -> AppResult<Stage> {
    // Un fichier qui ne contient qu'une archive : on l'ouvre ; plusieurs : à choisir.
    if let Some(dir) = extracted.filter(|_| depth < NESTED_DEPTH) {
        let nested = archive::nested_archives(dir);
        match nested.as_slice() {
            [] => {}
            [only] => {
                let inner = ctx.job_dir.join(format!("nested-{depth}"));
                let progress = |done: u64, total: u64| emit(InstallEvent::Extracting { done, total });
                archive::extract(only, &inner, ctx.limit, cancel, &progress)?;
                emit(InstallEvent::Finishing);
                return plan_extracted(ctx, Some(&inner), depth + 1, cancel, emit);
            }
            _ => return Ok(Stage::Archives { paths: nested, depth }),
        }
    }
    let config = &ctx.config;
    let mod_id = ctx.mod_id;
    let unpacked = || extracted.ok_or_else(|| AppError::Io("archive non extraite".into()));

    // Installateur FOMOD : options à choisir (sauf BONELAB, qui a ses règles).
    let fomod_allowed = config.rules.as_deref() != Some("bonelab");
    if let Some((froot, fconfig)) = extracted.filter(|_| fomod_allowed).and_then(fomod::find) {
        let inst = fomod::parse(&froot, &fconfig, &ctx.game_state)?;
        if inst.has_choices() {
            return Ok(Stage::Choices { installer: inst, root: froot });
        }
        // Pas de choix à faire : fichiers obligatoires et conditionnels.
        let moves = fomod::expand(&froot, &fomod::resolve(&inst, &[])?, &ctx.job_dir.join("fomod-copies"))?;
        let (batches, plugins, root) = fomod_batches(config, &ctx.domain, moves, &ctx.mod_name, mod_id, ctx.old.as_ref());
        let placed = place_plan(config, ctx.old.as_ref(), &ctx.others, batches, None)?;
        return Ok(Stage::Placed { placed, root, detected_dir: None, plugins });
    }

    let mut root = None;
    let mut detected_dir = None;
    let mut plugins = Vec::new();
    let mut new_batches: BTreeMap<String, Vec<(PathBuf, String)>> = BTreeMap::new();
    let mut ruled: Option<RulePlan> = None;
    match (config.rules.as_deref(), &ctx.engine) {
        (Some("bonelab"), _) => {
            let plan = rules::bonelab_plan(unpacked()?, &ctx.mod_name)?;
            if plan.needs_game_dir() && !config.extra_folders.contains_key(GAME_DIR) {
                // Dossier du jeu pas encore indiqué : on le cherche.
                detected_dir = Some(rules::detect_bonelab_dir().ok_or_else(|| AppError::NeedGameDir(config.game_name.clone()))?);
            }
            ruled = Some(plan);
        }
        (Some("unreal"), Some(info)) => {
            let project = info.project.clone().unwrap_or_default();
            ruled = Some(engine_rules::unreal_plan(unpacked()?, &config.folder, &project, &ctx.mod_name)?.0);
        }
        (Some("unity"), Some(info)) => {
            let loader = ctx.loader.unwrap_or(UnityLoader::BepInEx);
            ruled = Some(engine_rules::unity_plan(unpacked()?, loader, &ctx.mod_name, &info.tools)?.0);
        }
        (Some("bethesda"), Some(_)) => {
            let game = engine::bethesda(&ctx.domain).ok_or_else(|| AppError::UnknownLayout("jeu Bethesda inconnu".into()))?;
            let (plan, p) = engine_rules::bethesda_plan(unpacked()?, game)?;
            plugins = p;
            ruled = Some(plan);
        }
        _ => {
            let empty = BTreeMap::new();
            let (r, moves) = installer::layout_moves(&PlaceInput {
                folder: &config.folder,
                layout: config.layout,
                mod_id,
                name_id: &installer::safe_name(&ctx.mod_name),
                archive: &ctx.archive,
                filename: &ctx.file_name,
                extracted,
                old: ctx.old.as_ref(),
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
        let gen_dir = ctx.job_dir.join("generated");
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
    let placed = place_plan(config, ctx.old.as_ref(), &ctx.others, new_batches, detected_dir.clone())?;
    Ok(Stage::Placed { placed, root, detected_dir, plugins })
}

/// Jetons des installations en attente de choix.
static SESSIONS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// Mod placé, à enregistrer.
struct Finished {
    req: InstallRequest,
    config: GameInstall,
    file: NexusFile,
    name: String,
    logo: Option<String>,
    old: Option<InstalledMod>,
    /// Ancienne version désactivée, supprimée maintenant que la nouvelle est en place.
    set_aside: Option<InstalledMod>,
    engine_info: Option<EngineInfo>,
    placed: Vec<installer::PlacedBatch>,
    root: Option<String>,
    detected_dir: Option<PathBuf>,
    plugins: Vec<String>,
}

/// Place les fichiers : un lot par dossier touché (nouvelle version ou ancienne).
fn place_plan(
    config: &GameInstall,
    old: Option<&InstalledMod>,
    others: &HashMap<PathBuf, BTreeMap<String, String>>,
    mut new_batches: BTreeMap<String, Vec<(PathBuf, String)>>,
    detected_dir: Option<PathBuf>,
) -> AppResult<Vec<installer::PlacedBatch>> {
    let mut keys: BTreeSet<String> = new_batches.keys().cloned().collect();
    if let Some(o) = old {
        keys.insert(String::new());
        keys.extend(o.parts.iter().map(|p| p.key.clone()));
    }
    let mut batches = Vec::new();
    for key in keys {
        let folder = match config.folder_for(&key) {
            Some(f) => f.to_path_buf(),
            None if key == GAME_DIR => detected_dir.clone().ok_or_else(|| AppError::NeedGameDir(config.game_name.clone()))?,
            None => continue,
        };
        let (old_files, old_dirs) = match (old, key.as_str()) {
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
    installer::place_batches(batches)
}

/// Fichiers choisis dans un installateur FOMOD → lots à placer. Les
/// destinations FOMOD partent du dossier `Data` des jeux Bethesda, du dossier
/// du jeu pour Unreal / Unity, et du dossier des mods sinon (dans le
/// sous-dossier du mod pour ce rangement).
fn fomod_batches(
    config: &GameInstall,
    domain: &str,
    moves: Vec<(PathBuf, String)>,
    mod_name: &str,
    mod_id: i64,
    old: Option<&InstalledMod>,
) -> (BTreeMap<String, Vec<(PathBuf, String)>>, Vec<String>, Option<String>) {
    let bethesda = config.rules.as_deref() == Some("bethesda") && engine::bethesda(domain).is_some();
    let mut sub = None;
    let prefix = if bethesda {
        "Data/".to_string()
    } else if config.rules.is_none() && config.layout == Layout::Subfolder {
        let chosen = old.and_then(|o| o.root.clone()).unwrap_or_else(|| {
            let preferred = installer::safe_name(mod_name);
            if config.folder.join(&preferred).exists() { format!("{preferred}-{mod_id}") } else { preferred }
        });
        sub = Some(chosen.clone());
        format!("{chosen}/")
    } else {
        String::new()
    };
    let mut plugins = Vec::new();
    let moved: Vec<(PathBuf, String)> = moves
        .into_iter()
        .map(|(src, dest)| {
            if bethesda && !dest.contains('/') {
                let lower = dest.to_ascii_lowercase();
                if lower.ends_with(".esp") || lower.ends_with(".esm") || lower.ends_with(".esl") {
                    plugins.push(dest.clone());
                }
            }
            (src, format!("{prefix}{dest}"))
        })
        .collect();
    (BTreeMap::from([(String::new(), moved)]), plugins, sub)
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
    ) -> AppResult<InstallOutcome> {
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

        let mod_name = detail.summary.name.clone();
        let logo = detail.summary.thumbnail_url.clone().or(detail.summary.picture_url.clone());
        let file_name = file.uri.clone().unwrap_or_else(|| format!("{}.zip", file.name));
        let limit = installer::extraction_limit(file.bytes().max(0) as u64, 0);
        let ctx = match self.plan_ctx(&req, &config, &mod_name, &archive_path, &file_name, &job_dir, limit, other_library).await {
            Ok(c) => c,
            Err(e) => {
                cleanup();
                return Err(e);
            }
        };
        let needs_extract = config.rules.is_some() || config.layout != Layout::Archive;
        let (blocking_emit, blocking_cancel, ctx_b) = (emit.clone(), cancel.clone(), ctx.clone());
        let stage = tokio::task::spawn_blocking(move || -> AppResult<Stage> {
            let extracted = ctx_b.job_dir.join("extracted");
            if needs_extract {
                let progress = |done: u64, total: u64| blocking_emit(InstallEvent::Extracting { done, total });
                archive::extract(&ctx_b.archive, &extracted, limit, &blocking_cancel, &progress)?;
            }
            if blocking_cancel.load(Ordering::Relaxed) {
                return Err(AppError::Cancelled);
            }
            blocking_emit(InstallEvent::Finishing);
            plan_extracted(&ctx_b, needs_extract.then_some(extracted.as_path()), 0, &blocking_cancel, &*blocking_emit)
        })
        .await
        .map_err(|e| AppError::Io(e.to_string()))
        .and_then(|r| r);
        let pending = Pending { req, file, name: mod_name, logo, job_dir, staging };
        self.conclude(stage, pending, ctx).await
    }

    /// Ce qu'il faut pour placer les fichiers : fichiers des autres mods (à ne
    /// jamais écraser), version déjà installée, moteur et outil du jeu.
    #[allow(clippy::too_many_arguments)]
    async fn plan_ctx(
        &self,
        req: &InstallRequest,
        config: &GameInstall,
        mod_name: &str,
        archive: &Path,
        file_name: &str,
        job_dir: &Path,
        limit: u64,
        other_library: &Library,
    ) -> AppResult<PlanCtx> {
        let others = {
            let lib = self.library.lock().expect("library");
            owned_by_others(&[&lib, other_library], req.game_id, req.mod_id)
        };
        let (old, set_aside) = Self::previous_version(config, req.mod_id);
        let domain = req.game_domain.clone();
        let engine = self.engine_for(config, &domain)?;
        let loader = match &engine {
            Some(info) if config.rules.as_deref() == Some("unity") => {
                let hints = if info.tools.is_empty() { self.hints(&domain).await } else { Default::default() };
                Some(match local::chosen_tool(req.game_id, info, &hints) {
                    Some("melonloader") => UnityLoader::MelonLoader,
                    _ => UnityLoader::BepInEx,
                })
            }
            _ => None,
        };
        let game_state = self.fomod_game_state(config, &domain, engine.as_ref());
        Ok(PlanCtx {
            config: config.clone(),
            domain,
            mod_id: req.mod_id,
            mod_name: mod_name.to_string(),
            old,
            set_aside,
            others,
            engine,
            loader,
            game_state,
            archive: archive.to_path_buf(),
            file_name: file_name.to_string(),
            job_dir: job_dir.to_path_buf(),
            limit,
        })
    }

    /// Suite d'une étape : choix à faire (la session garde les fichiers
    /// extraits), ou mod placé à enregistrer.
    async fn conclude(&self, stage: AppResult<Stage>, pending: Pending, ctx: PlanCtx) -> AppResult<InstallOutcome> {
        let cleanup = |p: &Pending| {
            let _ = fs::remove_dir_all(&p.job_dir);
            let _ = fs::remove_dir(&p.staging); // seulement s'il est vide
        };
        match stage {
            Err(e) => {
                cleanup(&pending);
                Err(e)
            }
            Ok(Stage::Choices { installer, root }) => Ok(self.open_session(pending, installer, root, None, 0)),
            Ok(Stage::Archives { paths, depth }) => {
                let installer = archive_choice(&pending.name, &paths);
                let root = pending.job_dir.clone();
                Ok(self.open_session(pending, installer, root, Some(paths), depth))
            }
            Ok(Stage::Placed { placed, root, detected_dir, plugins }) => {
                cleanup(&pending);
                let PlanCtx { config, old, set_aside, engine, .. } = ctx;
                let Pending { req, file, name, logo, .. } = pending;
                let done = self
                    .record_install(Finished {
                        req,
                        config,
                        file,
                        name,
                        logo,
                        old,
                        set_aside,
                        engine_info: engine,
                        placed,
                        root,
                        detected_dir,
                        plugins,
                    })
                    .await?;
                Ok(InstallOutcome::Installed(done))
            }
        }
    }

    /// Garde les fichiers extraits en attendant les choix (voir `fomod_finish`).
    fn open_session(
        &self,
        pending: Pending,
        installer: fomod::Installer,
        root: PathBuf,
        nested: Option<Vec<PathBuf>>,
        depth: u8,
    ) -> InstallOutcome {
        let n = SESSIONS.fetch_add(1, Ordering::Relaxed);
        let token = format!("{}-{}-{n}", pending.req.mod_id, pending.file.file_id);
        let prompt = FomodPrompt { token: token.clone(), installer: installer.clone() };
        self.fomods.lock().expect("fomods").insert(token, FomodSession { pending, installer, root, nested, depth });
        InstallOutcome::Choices(prompt)
    }

    /// Termine une installation en attente de choix : options FOMOD, ou
    /// archives choisies (qui peuvent à leur tour proposer des options).
    pub async fn fomod_finish(
        &self,
        token: &str,
        selections: Vec<Vec<Vec<usize>>>,
        other_library: &Library,
        emit: Arc<dyn Fn(InstallEvent) + Send + Sync>,
    ) -> AppResult<InstallOutcome> {
        let session = self.fomods.lock().expect("fomods").remove(token).ok_or(AppError::NotFound)?;
        let req = session.pending.req.clone();
        let mod_id = req.mod_id;
        let cleanup = |s: &FomodSession| {
            let _ = fs::remove_dir_all(&s.pending.job_dir);
            let _ = fs::remove_dir(&s.pending.staging);
        };
        let config = match self.library.lock().expect("library").games.get(&req.game_id).cloned() {
            Some(c) => c,
            None => {
                cleanup(&session);
                return Err(AppError::NoGameFolder);
            }
        };
        let cancel = Arc::new(AtomicBool::new(false));
        {
            let mut jobs = self.jobs.lock().expect("jobs");
            if jobs.contains_key(&mod_id) {
                drop(jobs);
                // Garde la session : l'utilisateur pourra réessayer.
                self.fomods.lock().expect("fomods").insert(token.to_string(), session);
                return Err(AppError::InstallInProgress);
            }
            jobs.insert(mod_id, cancel.clone());
        }
        let _guard = JobGuard { state: self, mod_id };
        emit(InstallEvent::Finishing);

        // Choix refusés (règle d'un groupe) : la session reste pour corriger.
        let items = match fomod::resolve(&session.installer, &selections) {
            Ok(items) => items,
            Err(e @ AppError::InvalidArgument(_)) => {
                self.fomods.lock().expect("fomods").insert(token.to_string(), session);
                return Err(e);
            }
            Err(e) => {
                cleanup(&session);
                return Err(e);
            }
        };
        let p = &session.pending;
        let file_name = p.file.uri.clone().unwrap_or_else(|| format!("{}.zip", p.file.name));
        let limit = installer::extraction_limit(p.file.bytes().max(0) as u64, 0);
        let archive_path = p.job_dir.join("download.part");
        let ctx = match self.plan_ctx(&req, &config, &p.name, &archive_path, &file_name, &p.job_dir, limit, other_library).await {
            Ok(c) => c,
            Err(e) => {
                cleanup(&session);
                return Err(e);
            }
        };
        let (ctx_b, emit_b) = (ctx.clone(), emit.clone());
        let (root, nested, depth) = (session.root.clone(), session.nested.clone(), session.depth);
        let stage = tokio::task::spawn_blocking(move || -> AppResult<Stage> {
            match nested {
                // Archives choisies : extraites ensemble, puis installées comme un fichier téléchargé.
                Some(paths) => {
                    let dest = ctx_b.job_dir.join(format!("chosen-{depth}"));
                    let progress = |done: u64, total: u64| emit_b(InstallEvent::Extracting { done, total });
                    for i in items.iter().filter_map(|f| f.source.parse::<usize>().ok()) {
                        if let Some(path) = paths.get(i) {
                            archive::extract(path, &dest, limit, &cancel, &progress)?;
                        }
                    }
                    emit_b(InstallEvent::Finishing);
                    plan_extracted(&ctx_b, Some(&dest), depth + 1, &cancel, &*emit_b)
                }
                None => {
                    let moves = fomod::expand(&root, &items, &ctx_b.job_dir.join("fomod-copies"))?;
                    let config = &ctx_b.config;
                    let (batches, plugins, sub) =
                        fomod_batches(config, &ctx_b.domain, moves, &ctx_b.mod_name, mod_id, ctx_b.old.as_ref());
                    let placed = place_plan(config, ctx_b.old.as_ref(), &ctx_b.others, batches, None)?;
                    Ok(Stage::Placed { placed, root: sub, detected_dir: None, plugins })
                }
            }
        })
        .await
        .map_err(|e| AppError::Io(e.to_string()))
        .and_then(|r| r);
        self.conclude(stage, session.pending, ctx).await
    }

    /// Abandonne une installation en attente de choix.
    pub fn fomod_cancel(&self, token: &str) {
        if let Some(s) = self.fomods.lock().expect("fomods").remove(token) {
            let _ = fs::remove_dir_all(&s.pending.job_dir);
            let _ = fs::remove_dir(&s.pending.staging);
        }
    }

    /// Version déjà installée du mod, telle qu'elle est dans le dossier du jeu.
    /// Désactivée, elle n'a aucun fichier en place : la nouvelle version
    /// s'installe à côté, et l'ancienne (mise de côté) est renvoyée à part pour
    /// n'être supprimée qu'une fois la nouvelle installée.
    fn previous_version(config: &GameInstall, mod_id: i64) -> (Option<InstalledMod>, Option<InstalledMod>) {
        let mut old = config.mods.get(&mod_id).cloned();
        let mut set_aside = None;
        if let Some(o) = old.as_mut().filter(|o| o.disabled) {
            set_aside = Some(o.clone());
            o.files.clear();
            o.dirs.clear();
            o.parts.clear();
        }
        (old, set_aside)
    }

    /// Moteur du jeu, pour les jeux rangés selon leur moteur (le dossier
    /// configuré est alors celui du jeu).
    fn engine_for(&self, config: &GameInstall, domain: &str) -> AppResult<Option<EngineInfo>> {
        Ok(match config.rules.as_deref() {
            Some("unreal" | "unity" | "bethesda") => Some(local::detect_engine(&config.folder, domain).ok_or_else(|| {
                AppError::UnknownLayout(format!(
                    "ModStudio ne reconnaît plus le jeu dans {}. Vérifie le dossier du jeu.",
                    config.folder.display()
                ))
            })?),
            _ => None,
        })
    }

    /// Ce qu'un installateur FOMOD peut vérifier sur le jeu (fichiers, plugins actifs, version).
    fn fomod_game_state(&self, config: &GameInstall, domain: &str, info: Option<&EngineInfo>) -> fomod::GameState {
        let beth = engine::bethesda(domain).filter(|_| config.rules.as_deref() == Some("bethesda"));
        let active_plugins = beth
            .and_then(|g| self.plugins_path(g, &config.folder).map(|p| (p, g.star_format)))
            .and_then(|(p, star)| fs::read_to_string(p).ok().map(|t| (t, star)))
            .map(|(text, star)| {
                text.lines()
                    .map(str::trim)
                    .filter(|l| !l.is_empty() && !l.starts_with('#'))
                    .filter(|l| !star || l.starts_with('*'))
                    .map(|l| l.trim_start_matches('*').trim().to_ascii_lowercase())
                    .collect()
            })
            .unwrap_or_default();
        fomod::GameState {
            data_dir: Some(if beth.is_some() { config.folder.join("Data") } else { config.folder.clone() }),
            active_plugins,
            game_version: info.and_then(|i| i.exe_version.clone()),
            extender_present: beth.is_some_and(|g| info.is_some_and(|i| i.tools.iter().any(|t| t == g.extender))),
        }
    }

    /// Enregistre un mod placé : plugins Bethesda, outil manquant, manifeste.
    async fn record_install(&self, f: Finished) -> AppResult<NexusInstalled> {
        let Finished { req, config, file, name, logo, old, set_aside, engine_info, placed, root, detected_dir, plugins } = f;
        if let Some(previous) = &set_aside {
            let store_root = self.disabled_root();
            toggle::discard(&config, previous, &toggle::Ctx { store_root: &store_root, source: "nexus" });
        }
        let mod_id = req.mod_id;
        let domain = req.game_domain.clone();
        let (auto, required_by) = dependency_state(req.required_by, old.as_ref());
        let mut record = InstalledMod {
            mod_id,
            auto,
            required_by,
            name,
            logo,
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
        if let Some(path) = engine::bethesda(&domain).and_then(|g| self.plugins_path(g, &config.folder).map(|p| (p, g.star_format))) {
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
    fn toggle_plugins(&self, domain: &str, game_dir: &Path, record: &InstalledMod, enabled: bool) {
        if record.plugins.is_empty() {
            return;
        }
        let Some(g) = engine::bethesda(domain) else { return };
        let Some(path) = self.plugins_path(g, game_dir) else { return };
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
            self.toggle_plugins(game.domain.as_deref().unwrap_or_default(), &game.folder, &record, enabled);
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
            let folder = game.folder.clone();
            let mut on_toggle = |m: &InstalledMod, on: bool| self.toggle_plugins(&domain, &folder, m, on);
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

    /// `plugins.txt` d'un jeu Bethesda installé dans `game_dir` : sous
    /// `%LOCALAPPDATA%`, ou sous Linux dans le préfixe Proton du jeu.
    pub fn plugins_path(&self, game: &engine::BethesdaGame, game_dir: &Path) -> Option<PathBuf> {
        let base = self.local_appdata.clone().or_else(|| {
            let prefix = crate::games::proton::prefix_for_dir(game_dir)?;
            crate::games::proton::windows_env("LOCALAPPDATA", Some(&prefix)).map(PathBuf::from)
        })?;
        Some(engine_rules::plugins_file(&base, game))
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
            if let Some(path) = engine::bethesda(&domain).and_then(|g| self.plugins_path(g, &game.folder)) {
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
        let out = s.install(request(None), &Library::default(), emit).await.unwrap().done();
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
        let out = s.install(request(None), &Library::default(), quiet()).await.unwrap().done();
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

        let out = s.install(request_for(6534, "pilgrim"), &Library::default(), quiet()).await.unwrap().done();
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

        let out = s.install(request_for(1704, "skyrimspecialedition"), &Library::default(), quiet()).await.unwrap().done();
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
    async fn fomod_installers_ask_then_install_the_chosen_options() {
        let config = br#"<config><moduleName>Lanternes</moduleName>
            <requiredInstallFiles><folder source="Core" destination=""/></requiredInstallFiles>
            <installSteps order="Explicit"><installStep name="Style"><optionalFileGroups>
              <group name="Couleur" type="SelectExactlyOne"><plugins order="Explicit">
                <plugin name="Chaude"><description>Orange</description><files><file source="Warm\Lanterns.esp" destination="Lanterns.esp"/></files><typeDescriptor><type name="Recommended"/></typeDescriptor></plugin>
                <plugin name="Froide"><description>Bleu</description><files><file source="Cold/Lanterns.esp" destination="Lanterns.esp"/></files><typeDescriptor><type name="Optional"/></typeDescriptor></plugin>
              </plugins></group>
            </optionalFileGroups></installStep></installSteps></config>"#;
        let zip = make_zip(&[
            ("Lanternes/fomod/ModuleConfig.xml", config.as_slice()),
            ("Lanternes/Core/textures/lantern.dds", b"dds"),
            ("Lanternes/Warm/Lanterns.esp", b"chaude"),
            ("Lanternes/Cold/Lanterns.esp", b"froide"),
        ]);
        let (_server, mut s) = nexus_with(zip, 200);
        let appdata = tempdir("fomod-appdata");
        s.local_appdata = Some(appdata.clone());
        s.set_key("cle").await.unwrap();
        let game = tempdir("fomod-game");
        fs::write(game.join("SkyrimSE.exe"), b"MZ").unwrap();
        fs::create_dir_all(game.join("Data")).unwrap();
        s.configure(1704, "skyrimspecialedition", "Skyrim SE".into(), None, game.to_string_lossy().into_owned(), Layout::Subfolder, false, None)
            .unwrap();

        let outcome = s.install(request_for(1704, "skyrimspecialedition"), &Library::default(), quiet()).await.unwrap();
        let InstallOutcome::Choices(prompt) = outcome else { panic!("des choix étaient attendus") };
        assert_eq!(prompt.installer.steps[0].groups[0].plugins[1].name, "Froide");
        assert!(!game.join("Data/Lanterns.esp").exists(), "rien n'est installé avant les choix");

        // Choix incomplet : refusé, la session reste.
        let err = s.fomod_finish(&prompt.token, vec![vec![vec![]]], &Library::default(), quiet()).await.unwrap_err();
        assert!(matches!(err, AppError::InvalidArgument(_)), "{err}");

        let done = s.fomod_finish(&prompt.token, vec![vec![vec![1]]], &Library::default(), quiet()).await.unwrap().done();
        assert_eq!(fs::read(game.join("Data/Lanterns.esp")).unwrap(), b"froide");
        assert!(game.join("Data/textures/lantern.dds").is_file());
        assert_eq!(done.record.plugins, vec!["Lanterns.esp"]);
        assert!(fs::read_to_string(appdata.join("Skyrim Special Edition/plugins.txt")).unwrap().contains("*Lanterns.esp"));
        assert!(!game.join(STAGING_DIR).exists(), "fichiers extraits supprimés");
        assert!(s.fomods.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn an_archive_inside_the_archive_is_opened() {
        let inner = make_zip(&[("BaBaCorp.711/BaBaCorp.711.pallet.json", b"{}")]);
        let zip = make_zip(&[("711 Sandbox/711 Sandbox PC.zip", inner.as_slice()), ("711 Sandbox/Lisez-moi.txt", b"PC")]);
        let (_server, s) = nexus_with(zip, 200);
        s.set_key("cle").await.unwrap();
        let mods = tempdir("nested-one");
        s.configure(rules::BONELAB, "bonelab", "BONELAB".into(), None, mods.to_string_lossy().into_owned(), Layout::Flat, false, None)
            .unwrap();
        s.install(request(None), &Library::default(), quiet()).await.unwrap().done();
        assert!(mods.join("BaBaCorp.711/BaBaCorp.711.pallet.json").is_file());
        assert!(!mods.join(STAGING_DIR).exists());
    }

    #[tokio::test]
    async fn several_archives_inside_the_archive_are_chosen() {
        let pc = make_zip(&[("BaBaCorp.711/BaBaCorp.711.pallet.json", b"pc")]);
        let quest = make_zip(&[("BaBaCorp.711Quest/BaBaCorp.711Quest.pallet.json", b"quest")]);
        let zip = make_zip(&[("711 PC.zip", pc.as_slice()), ("711 QUEST.zip", quest.as_slice()), ("readme.txt", b"choisis")]);
        let (_server, s) = nexus_with(zip, 200);
        s.set_key("cle").await.unwrap();
        let mods = tempdir("nested-many");
        s.configure(rules::BONELAB, "bonelab", "BONELAB".into(), None, mods.to_string_lossy().into_owned(), Layout::Flat, false, None)
            .unwrap();

        let InstallOutcome::Choices(prompt) = s.install(request(None), &Library::default(), quiet()).await.unwrap() else {
            panic!("un choix d'archive était attendu")
        };
        let names: Vec<_> = prompt.installer.steps[0].groups[0].plugins.iter().map(|p| p.name.clone()).collect();
        assert_eq!(names, ["711 PC", "711 QUEST"]);
        assert!(!mods.join("BaBaCorp.711").exists(), "rien n'est installé avant le choix");

        // Aucune archive cochée : refusé, la session reste.
        let err = s.fomod_finish(&prompt.token, vec![vec![vec![]]], &Library::default(), quiet()).await.unwrap_err();
        assert!(matches!(err, AppError::InvalidArgument(_)), "{err}");

        s.fomod_finish(&prompt.token, vec![vec![vec![0]]], &Library::default(), quiet()).await.unwrap().done();
        assert_eq!(fs::read(mods.join("BaBaCorp.711/BaBaCorp.711.pallet.json")).unwrap(), b"pc");
        assert!(!mods.join("BaBaCorp.711Quest").exists());
        assert!(!mods.join(STAGING_DIR).exists(), "fichiers extraits supprimés");
        assert!(s.fomods.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_failed_update_keeps_the_disabled_version() {
        let zip = make_zip(&[("BaBaCorp.711/BaBaCorp.711.pallet.json", b"{}")]);
        let (_server, s) = nexus_with(zip, 200);
        s.set_key("cle").await.unwrap();
        let mods = tempdir("bl-upd");
        s.configure(rules::BONELAB, "bonelab", "BONELAB".into(), None, mods.to_string_lossy().into_owned(), Layout::Flat, false, None)
            .unwrap();
        // Le mod 148 est installé puis désactivé : ses fichiers sont de côté.
        let kept = mods.join(".modstudio-disabled/nexus-148/BaBaCorp.711/BaBaCorp.711.pallet.json");
        fs::create_dir_all(kept.parent().unwrap()).unwrap();
        fs::write(&kept, b"ancienne version").unwrap();
        s.library.lock().unwrap().games.get_mut(&rules::BONELAB).unwrap().mods.insert(
            148,
            InstalledMod {
                mod_id: 148,
                name: "7-11".into(),
                files: vec!["BaBaCorp.711/BaBaCorp.711.pallet.json".into()],
                disabled: true,
                disabled_in: Some("game".into()),
                ..Default::default()
            },
        );
        // Un mod mod.io occupe maintenant le même chemin.
        let mut modio = Library::default();
        let mut g = GameInstall { game_id: 3809, game_name: "BONELAB".into(), folder: mods.clone(), layout: Layout::Flat, ..Default::default() };
        g.mods.insert(5, InstalledMod { mod_id: 5, name: "7/11 Extra".into(), files: vec!["BaBaCorp.711/BaBaCorp.711.pallet.json".into()], ..Default::default() });
        modio.games.insert(3809, g);
        fs::create_dir_all(mods.join("BaBaCorp.711")).unwrap();
        fs::write(mods.join("BaBaCorp.711/BaBaCorp.711.pallet.json"), b"mod.io").unwrap();

        let err = s.install(request(None), &modio, quiet()).await.unwrap_err();
        assert!(matches!(err, AppError::Conflict(_)), "{err}");
        assert_eq!(fs::read(&kept).unwrap(), b"ancienne version", "l'ancienne version mise de côté est intacte");
        assert!(s.library.lock().unwrap().games[&rules::BONELAB].mods[&148].disabled);
        assert_eq!(fs::read(mods.join("BaBaCorp.711/BaBaCorp.711.pallet.json")).unwrap(), b"mod.io");
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
        let out = s.install(req, &Library::default(), quiet()).await.unwrap().done();
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
