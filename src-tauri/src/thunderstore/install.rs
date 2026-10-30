//! Installation des mods Thunderstore : dépendances d'abord, BepInExPack dans
//! le dossier du jeu, chaque mod rangé dans `BepInEx/plugins/<Équipe-Nom>`
//! (comme les gestionnaires de mods de Thunderstore).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

use super::api::{check_community, check_part, check_version, parse_dependency, stable_id, Latest};
use super::ThunderstoreState;
use crate::archive;
use crate::error::{AppError, AppResult};
use crate::install_commands::{dependency_state, Orphan, UninstallResult};
use crate::installer::{self, DownloadRequest, GameInstall, InstallEvent, InstalledMod, Layout, Library, STAGING_DIR};
use crate::nexus::install::{owned_by_others, place_plan};
use crate::toggle;

/// Règles des jeux Thunderstore (dossier configuré = dossier du jeu).
pub const RULES: &str = "thunderstore";
/// Au plus autant de mods installés d'un coup (dépendances comprises).
const MAX_PACKAGES: usize = 150;

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Résultat d'une installation.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TsInstalled {
    pub game: GameInstall,
    /// Mods installés, dans l'ordre (dépendances d'abord).
    pub installed: Vec<String>,
    /// BepInExPack demandé mais BepInEx déjà présent : laissé tel quel.
    pub kept_loader: bool,
}

/// Chargeur de mods présent dans le dossier du jeu.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TsLoader {
    /// `BepInEx 5`, `BepInEx 6 (IL2CPP)`, `MelonLoader`, ou rien.
    pub name: Option<&'static str>,
    /// Le chargeur présent peut-il charger les mods de ce jeu ?
    pub suitable: bool,
    /// Jeu Unity `il2cpp` ou `mono` (inconnu sinon).
    pub engine: Option<&'static str>,
    /// Installé par ModStudio (depuis Thunderstore) : remplaçable sans risque.
    pub managed: bool,
    /// Paquet qui remplace un BepInEx qui ne convient pas.
    pub fix: Option<TsRef>,
    /// Option de lancement Steam pour Proton (Linux seulement).
    pub launch_option: Option<String>,
}

/// Un paquet Thunderstore (`Équipe-Nom`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TsRef {
    pub id: i64,
    pub namespace: String,
    pub name: String,
}

/// Nouvelle version d'un mod installé.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TsUpdate {
    pub mod_id: i64,
    pub version: String,
}

/// Le chargeur BepInEx lui-même (`BepInExPack`, `BepInExPack_Valheim`…).
pub fn is_loader_pack(name: &str) -> bool {
    name.to_ascii_lowercase().starts_with("bepinexpack")
}

/// MelonLoader lui-même (`LavaGang-MelonLoader`).
fn is_melon_pack(name: &str) -> bool {
    name.eq_ignore_ascii_case("MelonLoader")
}

/// Version de BepInEx installée dans le dossier du jeu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flavor {
    /// BepInEx 5 : jeux Unity Mono seulement.
    Mono5,
    /// BepInEx 6 pour Unity Mono.
    Mono6,
    /// BepInEx 6 pour Unity IL2CPP.
    Il2cpp6,
}

impl Flavor {
    pub fn il2cpp(self) -> bool {
        self == Flavor::Il2cpp6
    }

    pub fn label(self) -> &'static str {
        match self {
            Flavor::Mono5 => "BepInEx 5",
            Flavor::Mono6 => "BepInEx 6",
            Flavor::Il2cpp6 => "BepInEx 6 (IL2CPP)",
        }
    }
}

pub fn bepinex_flavor(folder: &Path) -> Option<Flavor> {
    let core = folder.join("BepInEx").join("core");
    if core.join("BepInEx.Unity.IL2CPP.dll").is_file() || core.join("BepInEx.IL2CPP.dll").is_file() {
        Some(Flavor::Il2cpp6)
    } else if core.join("BepInEx.Unity.Mono.dll").is_file() || core.join("BepInEx.Core.dll").is_file() {
        Some(Flavor::Mono6)
    } else if core.join("BepInEx.dll").is_file() {
        Some(Flavor::Mono5)
    } else {
        None
    }
}

/// BepInEx est-il déjà dans le dossier du jeu ?
pub fn bepinex_present(folder: &Path) -> bool {
    bepinex_flavor(folder).is_some()
}

/// Dossiers `<Jeu>_Data` du dossier du jeu.
fn data_dirs(folder: &Path) -> Vec<PathBuf> {
    fs::read_dir(folder)
        .map(|it| {
            it.flatten()
                .filter(|e| e.file_name().to_string_lossy().ends_with("_Data") && e.path().is_dir())
                .map(|e| e.path())
                .collect()
        })
        .unwrap_or_default()
}

/// Jeu Unity compilé en IL2CPP : seul BepInEx 6 IL2CPP peut y charger des mods.
pub fn game_is_il2cpp(folder: &Path) -> bool {
    folder.join("GameAssembly.dll").is_file()
        || folder.join("GameAssembly.so").is_file()
        || data_dirs(folder).iter().any(|d| d.join("il2cpp_data").is_dir())
}

/// Jeu Unity Mono (le code du jeu est dans `<Jeu>_Data/Managed`).
pub fn game_is_mono(folder: &Path) -> bool {
    !game_is_il2cpp(folder) && data_dirs(folder).iter().any(|d| d.join("Managed").join("Assembly-CSharp.dll").is_file())
}

/// Le BepInEx en place peut-il charger les mods du jeu ? `None` : pas de BepInEx.
pub fn bepinex_suits(folder: &Path) -> Option<bool> {
    bepinex_flavor(folder).map(|f| f.il2cpp() == game_is_il2cpp(folder))
}

/// Le bon BepInEx pour ce jeu, quand un mod en demande un qui ne peut pas
/// marcher (BepInEx 5 pour un jeu IL2CPP : erreur fréquente des auteurs).
fn right_loader(namespace: &str, name: &str, folder: &Path) -> (String, String) {
    match name.to_ascii_lowercase().as_str() {
        "bepinexpack" | "bepinexpack_x86" if game_is_il2cpp(folder) => ("BepInEx".into(), "BepInExPack_IL2CPP".into()),
        "bepinexpack_il2cpp" if game_is_mono(folder) => ("BepInEx".into(), "BepInExPack".into()),
        _ => (namespace.into(), name.into()),
    }
}

/// BepInEx à installer à la place de celui qui ne convient pas.
fn fix_for(folder: &Path) -> Option<TsRef> {
    let name = if game_is_il2cpp(folder) {
        "BepInExPack_IL2CPP"
    } else if game_is_mono(folder) {
        "BepInExPack"
    } else {
        return None;
    };
    Some(TsRef { id: stable_id(&format!("BepInEx-{name}")), namespace: "BepInEx".into(), name: name.into() })
}

/// Mod installé qui est un chargeur (BepInExPack…).
fn is_loader_record(m: &InstalledMod) -> bool {
    m.file_name
        .as_deref()
        .and_then(|f| f.split_once('-'))
        .is_some_and(|(_, name)| is_loader_pack(name) || is_melon_pack(name))
}

fn melon_present(folder: &Path) -> bool {
    folder.join("MelonLoader").is_dir() && folder.join("version.dll").is_file()
}

/// Nom affiché d'un mod (`Lethal_Things` → `Lethal Things`).
pub fn display_name(name: &str) -> String {
    name.replace('_', " ")
}

/// Fichiers sous `dir`, avec leur chemin relatif (séparateur `/`).
fn files_under(dir: &Path) -> Vec<(PathBuf, String)> {
    let mut out = Vec::new();
    let mut stack = vec![(dir.to_path_buf(), String::new())];
    while let Some((d, prefix)) = stack.pop() {
        let Ok(entries) = fs::read_dir(&d) else { continue };
        let mut entries: Vec<_> = entries.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let name = e.file_name().to_string_lossy().into_owned();
            let rel = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
            let Ok(kind) = e.file_type() else { continue };
            if kind.is_dir() {
                stack.push((e.path(), rel));
            } else if kind.is_file() {
                out.push((e.path(), rel));
            }
        }
    }
    out.sort_by(|a, b| a.1.cmp(&b.1));
    out
}

/// Fichiers de présentation d'un paquet Thunderstore, à la racine de l'archive.
fn is_package_meta(rel: &str) -> bool {
    matches!(rel.to_ascii_lowercase().as_str(), "manifest.json" | "icon.png" | "readme.md" | "changelog.md")
}

/// Réglages : jamais remplacés, et laissés en place à la désinstallation
/// (ce sont ceux de l'utilisateur dès qu'il les modifie).
pub fn is_user_config(rel: &str) -> bool {
    let lower = rel.to_ascii_lowercase();
    lower.starts_with("bepinex/config/") || lower.starts_with("userdata/")
}

/// Où va chaque fichier de l'archive extraite (`root`) d'un paquet
/// `Équipe-Nom`, relativement au dossier du jeu.
pub fn ts_plan(root: &Path, full_name: &str, game_folder: &Path) -> Vec<(PathBuf, String)> {
    let name = full_name.split('-').nth(1).unwrap_or(full_name);
    let mut moves = Vec::new();

    // Le chargeur : le contenu de son dossier (`BepInExPack/`) va dans le dossier du jeu.
    if is_loader_pack(name) || is_melon_pack(name) {
        let inner = fs::read_dir(root).ok().and_then(|it| {
            it.flatten()
                .find(|e| e.path().is_dir() && e.file_name().to_string_lossy().to_ascii_lowercase().starts_with("bepinexpack"))
                .map(|e| e.path())
        });
        let src = inner.clone().unwrap_or_else(|| root.to_path_buf());
        for (abs, rel) in files_under(&src) {
            if inner.is_none() && is_package_meta(&rel) {
                continue;
            }
            if is_user_config(&rel) && game_folder.join(&rel).exists() {
                continue;
            }
            moves.push((abs, rel));
        }
        return moves;
    }

    // Jeu à MelonLoader : Mods, Plugins, UserLibs, UserData tels quels ; le reste dans Mods.
    if melon_present(game_folder) && !bepinex_present(game_folder) {
        for (abs, rel) in files_under(root) {
            let first = rel.split('/').next().unwrap_or("").to_ascii_lowercase();
            let dest = match first.as_str() {
                "userdata" if game_folder.join(&rel).exists() => continue,
                "mods" | "plugins" | "userlibs" | "userdata" => rel.clone(),
                _ if is_package_meta(&rel) => continue,
                _ => format!("Mods/{rel}"),
            };
            moves.push((abs, dest));
        }
        return moves;
    }

    let mut entries: Vec<_> = fs::read_dir(root).map(|it| it.flatten().collect()).unwrap_or_default();
    entries.sort_by_key(|e: &fs::DirEntry| e.file_name());
    for e in entries {
        route(&e.path(), full_name, game_folder, false, &mut moves);
    }
    moves
}

/// Range une entrée de l'archive selon les dossiers de BepInEx.
fn route(path: &Path, full: &str, game_folder: &Path, in_bepinex: bool, moves: &mut Vec<(PathBuf, String)>) {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    if !path.is_dir() {
        moves.push((path.to_path_buf(), format!("BepInEx/plugins/{full}/{name}")));
        return;
    }
    match name.to_ascii_lowercase().as_str() {
        "bepinex" if !in_bepinex => {
            let mut children: Vec<_> = fs::read_dir(path).map(|it| it.flatten().collect()).unwrap_or_default();
            children.sort_by_key(|e: &fs::DirEntry| e.file_name());
            for c in children {
                route(&c.path(), full, game_folder, true, moves);
            }
        }
        sub @ ("plugins" | "patchers" | "monomod" | "core") => {
            for (abs, rel) in files_under(path) {
                moves.push((abs, format!("BepInEx/{sub}/{full}/{rel}")));
            }
        }
        // Réglages par défaut : jamais à la place de ceux de l'utilisateur.
        "config" => {
            for (abs, rel) in files_under(path) {
                let dest = format!("BepInEx/config/{rel}");
                if !game_folder.join(&dest).exists() {
                    moves.push((abs, dest));
                }
            }
        }
        _ => {
            for (abs, rel) in files_under(path) {
                moves.push((abs, format!("BepInEx/plugins/{full}/{name}/{rel}")));
            }
        }
    }
}

/// Un mod à installer.
#[derive(Debug, Clone)]
struct Planned {
    namespace: String,
    name: String,
    version: String,
    icon: Option<String>,
}

impl Planned {
    fn full(&self) -> String {
        format!("{}-{}", self.namespace, self.name)
    }
    fn id(&self) -> i64 {
        stable_id(&self.full())
    }
}

/// Mods à installer, dépendances d'abord, et qui a besoin de qui (mod → parents).
#[derive(Default)]
struct Plan {
    order: Vec<Planned>,
    parents: HashMap<i64, Vec<i64>>,
    kept_loader: bool,
}

struct JobGuard<'a> {
    state: &'a ThunderstoreState,
    game_id: i64,
}

impl Drop for JobGuard<'_> {
    fn drop(&mut self) {
        self.state.jobs.lock().expect("jobs").remove(&self.game_id);
    }
}

impl ThunderstoreState {
    fn save_library(&self, lib: &Library) -> AppResult<()> {
        lib.save(&self.library_path)
    }

    /// Dossier des mods désactivés hors des dossiers des jeux.
    fn disabled_root(&self) -> PathBuf {
        self.library_path.with_file_name("disabled")
    }

    pub fn overview(&self) -> Vec<GameInstall> {
        let mut games: Vec<GameInstall> = self.library.lock().expect("library").games.values().cloned().collect();
        games.sort_by_key(|g| g.game_name.to_lowercase());
        games
    }

    /// Chargeur de mods présent dans le dossier du jeu.
    pub fn loader(&self, game_id: i64) -> AppResult<TsLoader> {
        let (folder, managed) = {
            let lib = self.library.lock().expect("library");
            let game = lib.games.get(&game_id).ok_or(AppError::NotFound)?;
            (game.folder.clone(), game.mods.values().any(is_loader_record))
        };
        let engine = if game_is_il2cpp(&folder) {
            Some("il2cpp")
        } else if game_is_mono(&folder) {
            Some("mono")
        } else {
            None
        };
        let (name, tool, suitable) = match bepinex_flavor(&folder) {
            Some(f) => (Some(f.label()), "bepinex", bepinex_suits(&folder) == Some(true)),
            None if melon_present(&folder) => (Some("MelonLoader"), "melonloader", true),
            None => (None, "", false),
        };
        let fix = (name.is_some() && tool == "bepinex" && !suitable).then(|| fix_for(&folder)).flatten();
        Ok(TsLoader { name, suitable, engine, managed, fix, launch_option: crate::games::proton::launch_option(tool) })
    }

    /// Retire le BepInEx installé par ModStudio qui ne convient pas au jeu ;
    /// renvoie son statut (nécessaire ou non, et à quels mods) pour le nouveau.
    fn drop_wrong_loader(&self, game_id: i64) -> AppResult<(bool, Vec<i64>)> {
        let root = self.disabled_root();
        let mut lib = self.library.lock().expect("library");
        let game = lib.games.get_mut(&game_id).ok_or(AppError::NotFound)?;
        let wrong: Vec<InstalledMod> = game.mods.values().filter(|m| is_loader_record(m)).cloned().collect();
        if wrong.is_empty() {
            let flavor = bepinex_flavor(&game.folder).map(Flavor::label).unwrap_or("BepInEx");
            let kind = if game_is_il2cpp(&game.folder) { "IL2CPP" } else { "Mono" };
            return Err(AppError::InvalidArgument(format!(
                "{flavor}, installé dans ce jeu sans ModStudio, ne peut pas charger ses mods (le jeu est en {kind}). \
                 Retire-le (dossier BepInEx\\core, winhttp.dll, doorstop_config.ini), puis réessaie."
            )));
        }
        let (mut auto, mut required_by) = (true, Vec::new());
        for rec in &wrong {
            let removal = if rec.disabled {
                toggle::discard(game, rec, &toggle::Ctx { store_root: &root, source: "thunderstore" });
                installer::FullRemoval { remaining: Vec::new(), remaining_parts: Vec::new(), left_folder: None }
            } else {
                installer::uninstall_all(game, rec)
            };
            if !removal.complete() {
                let first = removal.first_remaining().cloned().unwrap_or_default();
                if let Some(r) = game.mods.get_mut(&rec.mod_id) {
                    r.files = removal.remaining;
                }
                self.save_library(&lib)?;
                return Err(AppError::Io(format!("{first} n'a pas pu être supprimé. Ferme le jeu puis réessaie.")));
            }
            auto &= rec.auto;
            for p in &rec.required_by {
                if !required_by.contains(p) {
                    required_by.push(*p);
                }
            }
            game.mods.remove(&rec.mod_id);
            toggle::forget_mod(game, rec.mod_id);
        }
        // Ses réglages par défaut, jamais utilisés (BepInEx n'a pas tourné) : place à ceux du nouveau.
        if !game.folder.join("BepInEx").join("LogOutput.log").exists() {
            let _ = fs::remove_file(game.folder.join("BepInEx").join("config").join("BepInEx.cfg"));
        }
        self.save_library(&lib)?;
        Ok((auto, required_by))
    }

    /// Dossier d'installation d'un jeu (là où se trouve son exécutable).
    pub fn configure(&self, community: &str, name: String, icon: Option<String>, folder: &str) -> AppResult<GameInstall> {
        check_community(community)?;
        let path = PathBuf::from(folder.trim());
        if !path.is_dir() {
            return Err(AppError::Io(format!("le dossier n'existe pas : {}", path.display())));
        }
        let game_id = stable_id(community);
        let mut lib = self.library.lock().expect("library");
        if let Some(existing) = lib.games.get(&game_id)
            && !existing.mods.is_empty()
            && existing.folder != path
        {
            return Err(AppError::InvalidArgument("désinstalle d'abord les mods de ce jeu pour changer de dossier.".into()));
        }
        let entry = lib.games.entry(game_id).or_insert_with(|| GameInstall {
            game_id,
            layout: Layout::Flat,
            ..Default::default()
        });
        entry.game_name = name;
        entry.icon = icon.or(entry.icon.take());
        entry.folder = path;
        entry.rules = Some(RULES.to_string());
        entry.domain = Some(community.to_string());
        let out = entry.clone();
        self.save_library(&lib)?;
        Ok(out)
    }

    /// Infos d'une version précise (dépendances), ou de la dernière.
    async fn info(&self, namespace: &str, name: &str, version: Option<&str>) -> AppResult<Latest> {
        match version {
            Some(v) => {
                check_version(v)?;
                self.client.version(namespace, name, v).await
            }
            None => self.client.latest(namespace, name).await,
        }
    }

    /// Ce qu'il faut installer pour `target` : ses dépendances manquantes
    /// (dernière version), puis lui-même.
    async fn plan(&self, target: Planned, deps: Vec<String>, config: &GameInstall, bepinex_ok: bool, melon_ok: bool) -> AppResult<Plan> {
        let installed = &config.mods;
        let mut plan = Plan::default();
        let mut visited: HashSet<i64> = HashSet::new();
        visited.insert(target.id());
        // Pile : (paquet, ses dépendances, déjà développé ?).
        let mut stack: Vec<(Planned, Vec<String>, bool)> = vec![(target, deps, false)];
        while let Some((pkg, deps, expanded)) = stack.pop() {
            if expanded {
                plan.order.push(pkg);
                continue;
            }
            let parent = pkg.id();
            let mut children = Vec::new();
            for dep in &deps {
                let Some((ns, name, _)) = parse_dependency(dep) else { continue };
                let (ns, name) = right_loader(&ns, &name, &config.folder);
                let id = stable_id(&format!("{ns}-{name}"));
                plan.parents.entry(id).or_default().push(parent);
                if installed.contains_key(&id) || visited.contains(&id) {
                    continue;
                }
                if (is_loader_pack(&name) && bepinex_ok) || (is_melon_pack(&name) && melon_ok) {
                    plan.kept_loader = true;
                    visited.insert(id);
                    continue;
                }
                visited.insert(id);
                if visited.len() > MAX_PACKAGES {
                    return Err(AppError::InvalidArgument("ce mod demande beaucoup trop d'autres mods.".into()));
                }
                let latest = self.client.latest(&ns, &name).await?;
                children.push((
                    Planned {
                        namespace: ns,
                        name,
                        version: latest.version_number.clone(),
                        icon: latest.icon.clone(),
                    },
                    latest.dependencies.clone(),
                ));
            }
            stack.push((pkg, Vec::new(), true));
            for (child, child_deps) in children.into_iter().rev() {
                stack.push((child, child_deps, false));
            }
        }
        Ok(plan)
    }

    /// Installe un mod et ses dépendances dans le dossier du jeu.
    pub async fn install(
        &self,
        community: &str,
        namespace: &str,
        name: &str,
        version: Option<String>,
        other_libraries: &[Library],
        emit: Arc<dyn Fn(InstallEvent) + Send + Sync>,
    ) -> AppResult<TsInstalled> {
        check_community(community)?;
        check_part(namespace)?;
        check_part(name)?;
        let game_id = stable_id(community);
        let config = self.library.lock().expect("library").games.get(&game_id).cloned().ok_or(AppError::NoGameFolder)?;
        if !config.folder.is_dir() {
            return Err(AppError::Io(format!("le dossier du jeu n'existe plus : {}", config.folder.display())));
        }
        let cancel = Arc::new(AtomicBool::new(false));
        {
            let mut jobs = self.jobs.lock().expect("jobs");
            if jobs.contains_key(&game_id) {
                return Err(AppError::InstallInProgress);
            }
            jobs.insert(game_id, cancel.clone());
        }
        let _guard = JobGuard { state: self, game_id };
        emit(InstallEvent::Preparing);

        let info = self.info(namespace, name, version.as_deref()).await?;
        let target = Planned {
            namespace: namespace.to_string(),
            name: name.to_string(),
            version: info.version_number.clone(),
            icon: info.icon.clone(),
        };
        let target_id = target.id();
        let suits = bepinex_suits(&config.folder);
        let (bepinex_ok, melon_ok) = (suits == Some(true), melon_present(&config.folder));
        // Le chargeur lui-même, déjà là sans ModStudio : rien à faire.
        let already = (is_loader_pack(name) && bepinex_ok) || (is_melon_pack(name) && melon_ok);
        if already && !config.mods.contains_key(&target_id) {
            return Err(AppError::ToolPresent(if is_melon_pack(name) { "MelonLoader".into() } else { "BepInEx".into() }));
        }
        let plan = self.plan(target, info.dependencies.clone(), &config, bepinex_ok, melon_ok).await?;
        // Un BepInEx qui ne convient pas au jeu est en place : remplacé par le bon.
        let inherited = if suits == Some(false) && plan.order.iter().any(|p| is_loader_pack(&p.name)) {
            Some(self.drop_wrong_loader(game_id)?)
        } else {
            None
        };

        let staging = config.folder.join(STAGING_DIR);
        let job_dir = staging.join(format!("thunderstore-{target_id}"));
        let _ = fs::remove_dir_all(&job_dir);
        let cleanup = || {
            let _ = fs::remove_dir_all(&job_dir);
            let _ = fs::remove_dir(&staging); // seulement s'il est vide
        };
        let total = plan.order.len();
        let mut done_names = Vec::new();
        for (i, pkg) in plan.order.iter().enumerate() {
            if cancel.load(Ordering::Relaxed) {
                cleanup();
                return Err(AppError::Cancelled);
            }
            emit(InstallEvent::Step { name: display_name(&pkg.name), index: i as u32 + 1, total: total as u32 });
            let inherit = inherited.as_ref().filter(|_| is_loader_pack(&pkg.name));
            if let Err(e) = self.install_one(&config, pkg, pkg.id() == target_id, &plan, inherit, &job_dir, other_libraries, &cancel, &emit).await {
                cleanup();
                return Err(e);
            }
            done_names.push(display_name(&pkg.name));
        }
        cleanup();

        // Mods déjà installés dont le nouveau mod a besoin : il compte parmi leurs parents.
        let mut lib = self.library.lock().expect("library");
        let game = lib.games.get_mut(&game_id).ok_or(AppError::NotFound)?;
        for (child, parents) in &plan.parents {
            if let Some(m) = game.mods.get_mut(child).filter(|m| m.auto) {
                for p in parents {
                    if !m.required_by.contains(p) && *p != m.mod_id {
                        m.required_by.push(*p);
                    }
                }
            }
        }
        toggle::sync_active(game);
        let out = game.clone();
        self.save_library(&lib)?;
        Ok(TsInstalled { game: out, installed: done_names, kept_loader: plan.kept_loader })
    }

    /// Télécharge, extrait, place et enregistre un paquet.
    #[allow(clippy::too_many_arguments)]
    async fn install_one(
        &self,
        config: &GameInstall,
        pkg: &Planned,
        is_target: bool,
        plan: &Plan,
        inherited: Option<&(bool, Vec<i64>)>,
        job_dir: &Path,
        other_libraries: &[Library],
        cancel: &Arc<AtomicBool>,
        emit: &Arc<dyn Fn(InstallEvent) + Send + Sync>,
    ) -> AppResult<()> {
        let full = pkg.full();
        let mod_id = pkg.id();
        let url = self.client.download_url(&pkg.namespace, &pkg.name, &pkg.version)?;
        let zip = job_dir.join(format!("{full}.zip"));
        let progress_emit = emit.clone();
        let progress = move |received: u64, total: u64| progress_emit(InstallEvent::Downloading { received, total });
        installer::download(
            self.client.downloads(),
            DownloadRequest { url: &url, token: None, dest: &zip, expected_size: 0, expected_md5: None },
            cancel,
            &progress,
        )
        .await?;

        let (others, old) = {
            let lib = self.library.lock().expect("library");
            let mut libs: Vec<&Library> = vec![&*lib];
            libs.extend(other_libraries.iter());
            let old = lib.games.get(&config.game_id).and_then(|g| g.mods.get(&mod_id).cloned());
            (owned_by_others(&libs, config.game_id, mod_id), old)
        };
        if old.as_ref().is_some_and(|o| o.disabled) {
            return Err(AppError::InvalidArgument(format!("réactive « {} » avant de le mettre à jour.", display_name(&pkg.name))));
        }
        let (config_b, full_b, cancel_b, emit_b, old_b, zip_b) = (config.clone(), full.clone(), cancel.clone(), emit.clone(), old.clone(), zip.clone());
        let extracted = job_dir.join(&full);
        let placed = tokio::task::spawn_blocking(move || -> AppResult<Vec<installer::PlacedBatch>> {
            let size = fs::metadata(&zip_b).map(|m| m.len()).unwrap_or(0);
            let progress = |done: u64, total: u64| emit_b(InstallEvent::Extracting { done, total });
            archive::extract(&zip_b, &extracted, installer::extraction_limit(size, 0), &cancel_b, &progress)?;
            emit_b(InstallEvent::Finishing);
            let moves = ts_plan(&extracted, &full_b, &config_b.folder);
            let mut batches = BTreeMap::new();
            batches.insert(String::new(), moves);
            place_plan(&config_b, old_b.as_ref(), &others, batches, None)
        })
        .await
        .map_err(|e| AppError::Io(e.to_string()))??;

        let size = fs::metadata(&zip).map(|m| m.len() as i64).unwrap_or(0);
        let parent = (!is_target).then(|| plan.parents.get(&mod_id).and_then(|p| p.first().copied())).flatten();
        // Mise à jour d'un mod déjà là : il garde son statut (nécessaire ou non).
        let (mut auto, mut required_by) = match (&old, is_target) {
            (Some(o), true) => (o.auto, o.required_by.clone()),
            _ => dependency_state(parent, old.as_ref()),
        };
        // Remplace un BepInEx qui ne convenait pas : il en reprend le statut.
        if let Some((was_auto, was_needed_by)) = inherited {
            auto = *was_auto;
            if auto {
                for p in was_needed_by {
                    if !required_by.contains(p) {
                        required_by.push(*p);
                    }
                }
            } else {
                required_by.clear();
            }
        }
        if auto {
            for p in plan.parents.get(&mod_id).into_iter().flatten() {
                if !required_by.contains(p) {
                    required_by.push(*p);
                }
            }
        }
        let mut record = InstalledMod {
            mod_id,
            auto,
            required_by,
            name: display_name(&pkg.name),
            logo: pkg.icon.clone(),
            version: Some(pkg.version.clone()),
            modfile_id: 0,
            size,
            installed_at: now(),
            layout: Layout::Flat,
            file_name: Some(full),
            ..Default::default()
        };
        let loader = is_loader_pack(&pkg.name) || is_melon_pack(&pkg.name);
        for batch in placed.into_iter().filter(|b| b.key.is_empty()) {
            // Les réglages restent à l'utilisateur.
            record.files = batch.files.into_iter().filter(|f| !is_user_config(f)).collect();
            // Dossiers partagés (`BepInEx/plugins`…) : jamais retirés avec un mod.
            let own = format!("/{}", record.file_name.as_deref().unwrap_or_default());
            record.dirs = batch
                .dirs
                .into_iter()
                .filter(|d| loader || d.contains(&format!("{own}/")) || d.ends_with(&own))
                .collect();
        }
        let mut lib = self.library.lock().expect("library");
        let game = lib.games.get_mut(&config.game_id).ok_or(AppError::NotFound)?;
        game.mods.insert(mod_id, record);
        self.save_library(&lib)
    }

    /// Annule l'installation en cours pour ce jeu.
    pub fn cancel(&self, community: &str) {
        if let Some(flag) = self.jobs.lock().expect("jobs").get(&stable_id(community)) {
            flag.store(true, Ordering::Relaxed);
        }
    }

    /// Désinstalle ; renvoie le dossier laissé en place et les dépendances devenues inutiles.
    pub fn remove(&self, game_id: i64, mod_id: i64) -> AppResult<UninstallResult> {
        if self.jobs.lock().expect("jobs").contains_key(&game_id) {
            return Err(AppError::InstallInProgress);
        }
        let mut lib = self.library.lock().expect("library");
        let game = lib.games.get_mut(&game_id).ok_or(AppError::NotFound)?;
        let record = game.mods.get(&mod_id).cloned().ok_or(AppError::NotFound)?;
        let removal = if record.disabled {
            let root = self.disabled_root();
            toggle::discard(game, &record, &toggle::Ctx { store_root: &root, source: "thunderstore" });
            installer::FullRemoval { remaining: Vec::new(), remaining_parts: Vec::new(), left_folder: None }
        } else {
            installer::uninstall_all(game, &record)
        };
        if !removal.complete() {
            let n = removal.remaining_count();
            let first = removal.first_remaining().cloned().unwrap_or_default();
            if let Some(r) = game.mods.get_mut(&mod_id) {
                r.files = removal.remaining;
            }
            self.save_library(&lib)?;
            return Err(AppError::Io(format!(
                "{n} fichier(s) n'ont pas pu être supprimés, dont {first}. Ferme le jeu puis réessaie."
            )));
        }
        game.mods.remove(&mod_id);
        toggle::forget_mod(game, mod_id);
        let mut orphans = Vec::new();
        for other in game.mods.values_mut() {
            if other.required_by.contains(&mod_id) {
                other.required_by.retain(|id| *id != mod_id);
                if other.auto && other.required_by.is_empty() {
                    orphans.push(Orphan { mod_id: other.mod_id, name: other.name.clone() });
                }
            }
        }
        self.save_library(&lib)?;
        Ok(UninstallResult { left_folder: removal.left_folder, orphans })
    }

    /// Active ou désactive un mod (ses fichiers sont mis de côté).
    pub fn set_enabled(&self, game_id: i64, mod_id: i64, enabled: bool, other_libraries: &[Library]) -> AppResult<GameInstall> {
        if self.jobs.lock().expect("jobs").contains_key(&game_id) {
            return Err(AppError::InstallInProgress);
        }
        let root = self.disabled_root();
        let ctx = toggle::Ctx { store_root: &root, source: "thunderstore" };
        let mut lib = self.library.lock().expect("library");
        let mut game = lib.games.remove(&game_id).ok_or(AppError::NotFound)?;
        let result = if !game.mods.contains_key(&mod_id) {
            Err(AppError::NotFound)
        } else if enabled {
            let mut libs: Vec<&Library> = other_libraries.iter().collect();
            libs.insert(0, &*lib);
            let others = toggle::others_map(&game, mod_id, &libs);
            toggle::enable(&mut game, mod_id, &ctx, &others)
        } else {
            toggle::disable(&mut game, mod_id, &ctx)
        };
        let out = game.clone();
        lib.games.insert(game_id, game);
        self.save_library(&lib)?;
        result.map(|_| out)
    }

    /// Mods installés dont une nouvelle version est publiée.
    pub async fn updates(&self, game_id: i64) -> AppResult<Vec<TsUpdate>> {
        let installed: Vec<(i64, String, Option<String>)> = self
            .library
            .lock()
            .expect("library")
            .games
            .get(&game_id)
            .map(|g| g.mods.values().filter_map(|m| m.file_name.clone().map(|f| (m.mod_id, f, m.version.clone()))).collect())
            .unwrap_or_default();
        let mut out = Vec::new();
        for (mod_id, full, version) in installed {
            let Some((ns, name)) = full.split_once('-') else { continue };
            if let Ok(latest) = self.client.latest(ns, name).await
                && version.as_deref() != Some(latest.version_number.as_str())
            {
                out.push(TsUpdate { mod_id, version: latest.version_number });
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::installer::tests::{make_zip, tempdir};
    use crate::testutil::{fake_server_with, json_response};

    fn unzip_to(zip: &[u8], dir: &Path) {
        let path = dir.with_extension("zip");
        fs::write(&path, zip).unwrap();
        archive::extract(&path, dir, 1 << 24, &AtomicBool::new(false), &|_, _| {}).unwrap();
    }

    fn dests(moves: &[(PathBuf, String)]) -> Vec<String> {
        let mut v: Vec<String> = moves.iter().map(|(_, d)| d.clone()).collect();
        v.sort();
        v
    }

    #[test]
    fn plans_follow_the_bepinex_folders() {
        let game = tempdir("ts-game");
        fs::create_dir_all(game.join("BepInEx/config")).unwrap();
        fs::write(game.join("BepInEx/config/existing.cfg"), b"user").unwrap();

        // Mod simple : DLL et fichiers du paquet à la racine.
        let a = tempdir("ts-a");
        unzip_to(&make_zip(&[("MoreCompany.dll", b"dll"), ("manifest.json", b"{}"), ("icon.png", b"png")]), &a);
        assert_eq!(
            dests(&ts_plan(&a, "notnotnotswipez-MoreCompany", &game)),
            [
                "BepInEx/plugins/notnotnotswipez-MoreCompany/MoreCompany.dll",
                "BepInEx/plugins/notnotnotswipez-MoreCompany/icon.png",
                "BepInEx/plugins/notnotnotswipez-MoreCompany/manifest.json",
            ]
        );

        // Dossiers BepInEx : plugins, patchers, config (jamais par-dessus les réglages existants).
        let b = tempdir("ts-b");
        unzip_to(
            &make_zip(&[
                ("BepInEx/plugins/Lib/Lib.dll", b"dll"),
                ("patchers/Patch.dll", b"p"),
                ("config/existing.cfg", b"defaut"),
                ("config/new.cfg", b"defaut"),
                ("Assets/bundle", b"x"),
            ]),
            &b,
        );
        assert_eq!(
            dests(&ts_plan(&b, "Team-Lib", &game)),
            [
                "BepInEx/config/new.cfg",
                "BepInEx/patchers/Team-Lib/Patch.dll",
                "BepInEx/plugins/Team-Lib/Assets/bundle",
                "BepInEx/plugins/Team-Lib/Lib/Lib.dll",
            ]
        );

        // BepInExPack : le contenu de son dossier va dans le dossier du jeu.
        let c = tempdir("ts-c");
        unzip_to(
            &make_zip(&[
                ("BepInExPack/BepInEx/core/BepInEx.dll", b"core"),
                ("BepInExPack/winhttp.dll", b"proxy"),
                ("BepInExPack/doorstop_config.ini", b"ini"),
                ("manifest.json", b"{}"),
            ]),
            &c,
        );
        assert_eq!(dests(&ts_plan(&c, "BepInEx-BepInExPack", &game)), ["BepInEx/core/BepInEx.dll", "doorstop_config.ini", "winhttp.dll"]);
    }

    fn zip_response(body: &[u8]) -> String {
        let mut s = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/zip\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        s.extend(body.iter().map(|&b| b as char));
        s
    }

    fn latest(version: &str, deps: &[&str]) -> String {
        let deps: Vec<String> = deps.iter().map(|d| format!("\"{d}\"")).collect();
        json_response(200, &format!(r#"{{"latest":{{"version_number":"{version}","dependencies":[{}],"icon":null}}}}"#, deps.join(",")))
    }

    fn quiet() -> Arc<dyn Fn(InstallEvent) + Send + Sync> {
        Arc::new(|_| {})
    }

    fn state_for(base: &str, tag: &str) -> ThunderstoreState {
        ThunderstoreState::new(base.trim_end_matches("/v1"), tempdir(tag).join("installed-thunderstore.json"))
    }

    /// Jeu avec BepInEx déjà installé (sans ModStudio).
    fn game_with_bepinex(tag: &str) -> PathBuf {
        let game = tempdir(tag);
        fs::create_dir_all(game.join("BepInEx/core")).unwrap();
        fs::write(game.join("BepInEx/core/BepInEx.dll"), b"core").unwrap();
        game
    }

    #[tokio::test]
    async fn installs_dependencies_first_and_skips_the_loader() {
        let game = game_with_bepinex("ts-install");
        let lib_zip = make_zip(&[("LethalLib.dll", b"lib"), ("config/LethalLib.cfg", b"defaut"), ("manifest.json", b"{}")]);
        let mod_zip = make_zip(&[("plugins/Cool/Cool.dll", b"cool"), ("icon.png", b"png")]);
        let server = fake_server_with(|_| {
            vec![
                latest("2.0.0", &["Evaisa-LethalLib-1.0.0", "BepInEx-BepInExPack-5.4.2100"]),
                latest("1.1.0", &["BepInEx-BepInExPack-5.4.2100"]),
                zip_response(&lib_zip),
                zip_response(&mod_zip),
            ]
        });
        let s = state_for(&server.base, "ts-install-state");
        s.configure("lethal-company", "Lethal Company".into(), None, game.to_str().unwrap()).unwrap();

        let out = s.install("lethal-company", "Team", "Cool", None, &[], quiet()).await.unwrap();
        assert_eq!(out.installed, ["LethalLib", "Cool"]);
        assert!(out.kept_loader);
        assert_eq!(server.count(), 4);
        assert_eq!(server.request(2).line, "GET /package/download/Evaisa/LethalLib/1.1.0/ HTTP/1.1");

        assert_eq!(fs::read(game.join("BepInEx/plugins/Evaisa-LethalLib/LethalLib.dll")).unwrap(), b"lib");
        assert_eq!(fs::read(game.join("BepInEx/config/LethalLib.cfg")).unwrap(), b"defaut");
        assert_eq!(fs::read(game.join("BepInEx/plugins/Team-Cool/Cool/Cool.dll")).unwrap(), b"cool");
        assert!(!game.join(STAGING_DIR).exists(), "dossier de travail retiré");

        let g = &out.game;
        let lib = &g.mods[&stable_id("Evaisa-LethalLib")];
        assert!(lib.auto);
        assert_eq!(lib.required_by, [stable_id("Team-Cool")]);
        assert!(!lib.files.iter().any(|f| f.contains("config")), "les réglages ne sont pas suivis");
        assert!(lib.dirs.iter().all(|d| d.contains("Evaisa-LethalLib")), "dossiers partagés jamais suivis : {:?}", lib.dirs);
        let cool = &g.mods[&stable_id("Team-Cool")];
        assert!(!cool.auto);
        assert_eq!(cool.version.as_deref(), Some("2.0.0"));
        assert_eq!(cool.file_name.as_deref(), Some("Team-Cool"));

        // Désinstallation : le mod nécessaire devient orphelin, ses réglages restent.
        let removed = s.remove(g.game_id, cool.mod_id).unwrap();
        assert_eq!(removed.orphans.len(), 1);
        assert_eq!(removed.left_folder, None);
        assert!(!game.join("BepInEx/plugins/Team-Cool").exists());
        s.remove(g.game_id, lib.mod_id).unwrap();
        assert!(!game.join("BepInEx/plugins/Evaisa-LethalLib").exists());
        assert!(game.join("BepInEx/config/LethalLib.cfg").exists());
        assert!(game.join("BepInEx/core/BepInEx.dll").exists());
    }

    #[tokio::test]
    async fn update_keeps_user_settings_and_dependencies_already_there() {
        let game = game_with_bepinex("ts-update");
        let v1 = make_zip(&[("Cool.dll", b"v1"), ("old.txt", b"old"), ("config/Cool.cfg", b"defaut")]);
        let v2 = make_zip(&[("Cool.dll", b"v2"), ("config/Cool.cfg", b"defaut v2")]);
        let server = fake_server_with(|_| {
            vec![
                latest("1.0.0", &[]),
                zip_response(&v1),
                latest("2.0.0", &[]),
                zip_response(&v2),
                latest("3.0.0", &[]),
            ]
        });
        let s = state_for(&server.base, "ts-update-state");
        s.configure("lethal-company", "Lethal Company".into(), None, game.to_str().unwrap()).unwrap();
        s.install("lethal-company", "Team", "Cool", None, &[], quiet()).await.unwrap();
        fs::write(game.join("BepInEx/config/Cool.cfg"), b"mes reglages").unwrap();

        let out = s.install("lethal-company", "Team", "Cool", None, &[], quiet()).await.unwrap();
        let dir = game.join("BepInEx/plugins/Team-Cool");
        assert_eq!(fs::read(dir.join("Cool.dll")).unwrap(), b"v2");
        assert!(!dir.join("old.txt").exists(), "fichiers de l'ancienne version retirés");
        assert_eq!(fs::read(game.join("BepInEx/config/Cool.cfg")).unwrap(), b"mes reglages");
        assert_eq!(out.game.mods[&stable_id("Team-Cool")].version.as_deref(), Some("2.0.0"));

        let updates = s.updates(out.game.game_id).await.unwrap();
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].version, "3.0.0");
    }

    #[tokio::test]
    async fn never_overwrites_other_files_and_cleans_up() {
        let game = game_with_bepinex("ts-conflict");
        fs::create_dir_all(game.join("BepInEx/plugins/Team-Cool")).unwrap();
        fs::write(game.join("BepInEx/plugins/Team-Cool/Cool.dll"), b"a la main").unwrap();
        let server = fake_server_with(|_| vec![latest("1.0.0", &[]), zip_response(&make_zip(&[("Cool.dll", b"new")]))]);
        let s = state_for(&server.base, "ts-conflict-state");
        s.configure("lethal-company", "Lethal Company".into(), None, game.to_str().unwrap()).unwrap();
        let err = s.install("lethal-company", "Team", "Cool", None, &[], quiet()).await.unwrap_err();
        assert!(matches!(err, AppError::Conflict(_)), "{err:?}");
        assert_eq!(fs::read(game.join("BepInEx/plugins/Team-Cool/Cool.dll")).unwrap(), b"a la main");
        assert!(!game.join(STAGING_DIR).exists());
        assert!(s.overview()[0].mods.is_empty());
    }

    #[tokio::test]
    async fn installs_bepinex_when_missing_and_refuses_it_when_present() {
        let game = tempdir("ts-loader");
        let pack = make_zip(&[
            ("BepInExPack/BepInEx/core/BepInEx.dll", b"core"),
            ("BepInExPack/winhttp.dll", b"proxy"),
            ("manifest.json", b"{}"),
        ]);
        let server = fake_server_with(|_| {
            vec![
                latest("1.0.0", &["BepInEx-BepInExPack-5.4.2100"]),
                latest("5.4.2100", &[]),
                zip_response(&pack),
                zip_response(&make_zip(&[("Cool.dll", b"cool")])),
            ]
        });
        let s = state_for(&server.base, "ts-loader-state");
        s.configure("lethal-company", "Lethal Company".into(), None, game.to_str().unwrap()).unwrap();
        let out = s.install("lethal-company", "Team", "Cool", None, &[], quiet()).await.unwrap();
        assert_eq!(out.installed, ["BepInExPack", "Cool"]);
        assert!(game.join("winhttp.dll").is_file());
        assert!(bepinex_present(&game));
        assert!(game.join("BepInEx/plugins/Team-Cool/Cool.dll").is_file());

        // BepInEx installé à la main dans un autre jeu : pas d'installation par-dessus.
        let other = game_with_bepinex("ts-loader-2");
        let server = fake_server_with(|_| vec![latest("5.4.2100", &[])]);
        let s = state_for(&server.base, "ts-loader-state-2");
        s.configure("valheim", "Valheim".into(), None, other.to_str().unwrap()).unwrap();
        let err = s.install("valheim", "denikson", "BepInExPack_Valheim", None, &[], quiet()).await.unwrap_err();
        assert!(matches!(err, AppError::ToolPresent(_)), "{err:?}");
    }

    /// Jeu Unity IL2CPP (comme Roadside Research), sans chargeur.
    fn il2cpp_game(tag: &str) -> PathBuf {
        let game = tempdir(tag);
        fs::write(game.join("GameAssembly.dll"), b"il2cpp").unwrap();
        fs::create_dir_all(game.join("Game_Data/il2cpp_data")).unwrap();
        game
    }

    fn mono_pack() -> Vec<u8> {
        make_zip(&[
            ("BepInExPack/BepInEx/core/BepInEx.dll", b"bep5"),
            ("BepInExPack/BepInEx/config/BepInEx.cfg", b"cfg5"),
            ("BepInExPack/winhttp.dll", b"proxy5"),
            ("BepInExPack/doorstop_config.ini", b"ini5"),
        ])
    }

    fn il2cpp_pack() -> Vec<u8> {
        make_zip(&[
            ("BepInExPack/BepInEx/core/BepInEx.Unity.IL2CPP.dll", b"bep6"),
            ("BepInExPack/BepInEx/config/BepInEx.cfg", b"cfg6"),
            ("BepInExPack/dotnet/coreclr.dll", b"clr"),
            ("BepInExPack/winhttp.dll", b"proxy6"),
            ("BepInExPack/doorstop_config.ini", b"ini6"),
        ])
    }

    #[tokio::test]
    async fn il2cpp_games_get_bepinex_6_even_when_the_mod_asks_for_bepinex_5() {
        let game = il2cpp_game("ts-il2cpp");
        let server = fake_server_with(|_| {
            vec![
                latest("1.0.0", &["BepInEx-BepInExPack-5.4.2100"]),
                latest("6.0.788", &[]),
                zip_response(&il2cpp_pack()),
                zip_response(&make_zip(&[("HighJump.dll", b"jump")])),
            ]
        });
        let s = state_for(&server.base, "ts-il2cpp-state");
        s.configure("roadside-research", "Roadside Research".into(), None, game.to_str().unwrap()).unwrap();
        let out = s.install("roadside-research", "Deja", "HighJump", None, &[], quiet()).await.unwrap();
        assert_eq!(out.installed, ["BepInExPack IL2CPP", "HighJump"]);
        assert_eq!(server.request(1).line, "GET /api/experimental/package/BepInEx/BepInExPack_IL2CPP/ HTTP/1.1");
        assert_eq!(bepinex_flavor(&game), Some(Flavor::Il2cpp6));
        assert!(game.join("dotnet/coreclr.dll").is_file());
        assert!(game.join("BepInEx/plugins/Deja-HighJump/HighJump.dll").is_file());
        let pack = &out.game.mods[&stable_id("BepInEx-BepInExPack_IL2CPP")];
        assert_eq!(pack.required_by, [stable_id("Deja-HighJump")]);
        let l = s.loader(out.game.game_id).unwrap();
        assert!(l.suitable && l.fix.is_none());
        assert_eq!(l.engine, Some("il2cpp"));
    }

    #[tokio::test]
    async fn replaces_a_wrong_bepinex_installed_by_modstudio() {
        let game = il2cpp_game("ts-wrong");
        let server = fake_server_with(|_| {
            vec![
                // Ancienne version de ModStudio : BepInEx 5 installé tel quel.
                latest("5.4.2305", &[]),
                zip_response(&mono_pack()),
                latest("1.0.0", &[]),
                zip_response(&make_zip(&[("HighJump.dll", b"jump")])),
                // Réparation.
                latest("6.0.788", &[]),
                zip_response(&il2cpp_pack()),
            ]
        });
        let s = state_for(&server.base, "ts-wrong-state");
        let g = s.configure("roadside-research", "Roadside Research".into(), None, game.to_str().unwrap()).unwrap();
        s.install("roadside-research", "BepInEx", "BepInExPack", None, &[], quiet()).await.unwrap();
        s.install("roadside-research", "Deja", "HighJump", None, &[], quiet()).await.unwrap();
        {
            // Comme avant la correction : le mod comptait sur BepInEx 5.
            let mut lib = s.library.lock().unwrap();
            let m = lib.games.get_mut(&g.game_id).unwrap().mods.get_mut(&stable_id("BepInEx-BepInExPack")).unwrap();
            m.auto = true;
            m.required_by = vec![stable_id("Deja-HighJump")];
        }
        let l = s.loader(g.game_id).unwrap();
        assert_eq!(l.name, Some("BepInEx 5"));
        assert!(!l.suitable && l.managed);
        let fix = l.fix.unwrap();
        assert_eq!((fix.namespace.as_str(), fix.name.as_str()), ("BepInEx", "BepInExPack_IL2CPP"));

        let out = s.install("roadside-research", &fix.namespace, &fix.name, None, &[], quiet()).await.unwrap();
        assert_eq!(bepinex_flavor(&game), Some(Flavor::Il2cpp6));
        assert!(!game.join("BepInEx/core/BepInEx.dll").exists(), "BepInEx 5 retiré");
        assert_eq!(fs::read(game.join("winhttp.dll")).unwrap(), b"proxy6");
        assert_eq!(fs::read(game.join("BepInEx/config/BepInEx.cfg")).unwrap(), b"cfg6", "réglages de BepInEx 6");
        assert!(game.join("BepInEx/plugins/Deja-HighJump/HighJump.dll").is_file(), "les mods restent");
        assert!(!out.game.mods.contains_key(&stable_id("BepInEx-BepInExPack")));
        let pack = &out.game.mods[&stable_id("BepInEx-BepInExPack_IL2CPP")];
        assert!(pack.auto, "toujours un mod nécessaire");
        assert_eq!(pack.required_by, [stable_id("Deja-HighJump")]);
        assert!(s.loader(g.game_id).unwrap().suitable);
    }

    #[tokio::test]
    async fn never_removes_a_wrong_bepinex_installed_by_hand() {
        let game = il2cpp_game("ts-hand");
        fs::create_dir_all(game.join("BepInEx/core")).unwrap();
        fs::write(game.join("BepInEx/core/BepInEx.dll"), b"bep5").unwrap();
        let server = fake_server_with(|_| vec![latest("1.0.0", &["BepInEx-BepInExPack-5.4.2100"]), latest("6.0.788", &[])]);
        let s = state_for(&server.base, "ts-hand-state");
        let g = s.configure("roadside-research", "Roadside Research".into(), None, game.to_str().unwrap()).unwrap();
        let l = s.loader(g.game_id).unwrap();
        assert!(!l.suitable && !l.managed);
        let err = s.install("roadside-research", "Deja", "HighJump", None, &[], quiet()).await.unwrap_err();
        assert!(matches!(err, AppError::InvalidArgument(ref m) if m.contains("BepInEx 5")), "{err:?}");
        assert!(game.join("BepInEx/core/BepInEx.dll").is_file());
        assert!(!game.join("BepInEx/plugins").exists());
    }

    #[test]
    fn rejects_bad_names_before_any_request() {
        let s = ThunderstoreState::new("http://127.0.0.1:9", tempdir("ts-names").join("lib.json"));
        assert!(s.configure("../x", "X".into(), None, "/").is_err());
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let err = rt.block_on(s.install("lethal-company", "../Team", "Cool", None, &[], quiet())).unwrap_err();
        assert!(matches!(err, AppError::InvalidArgument(_)));
    }

    #[test]
    fn loader_and_names() {
        assert!(is_loader_pack("BepInExPack_Valheim"));
        assert!(!is_loader_pack("MoreCompany"));
        assert_eq!(display_name("Lethal_Things"), "Lethal Things");
        assert!(is_user_config("BepInEx/config/a.cfg"));
        assert!(!is_user_config("BepInEx/plugins/a.dll"));
    }
}
