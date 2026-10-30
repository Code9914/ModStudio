//! Installation des mods Thunderstore : dépendances d'abord, le chargeur du
//! jeu (BepInEx, MelonLoader…) s'il manque, puis chaque fichier rangé selon les
//! règles publiées pour le jeu (comme r2modman, directement dans le dossier du
//! jeu).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

use super::api::{check_community, check_part, check_version, parse_dependency, stable_id, Latest};
use super::ecosystem::{loader_label, Ecosystem, GameRules, LoaderKind, LoaderPackage, Rule, Tracking};
use super::ThunderstoreState;
use crate::archive;
use crate::error::{AppError, AppResult};
use crate::install_commands::{dependency_state, Orphan, UninstallResult};
use crate::installer::{self, DownloadRequest, GameInstall, InstallEvent, InstalledMod, Layout, Library, ModPart, STAGING_DIR};
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
    /// Chargeur que le jeu demande (`bepinex`, `melonloader`, `shimloader`…) ;
    /// `None` : Thunderstore ne publie pas de règles pour ce jeu.
    pub required: Option<String>,
    /// Son nom affiché (`BepInEx 6 (IL2CPP)`, `MelonLoader`…).
    pub required_label: Option<String>,
    /// ModStudio sait installer les mods de ce jeu.
    pub supported: bool,
    /// Chargeur présent : `BepInEx 5`, `BepInEx 6 (IL2CPP)`, `MelonLoader`…
    pub name: Option<&'static str>,
    /// Le chargeur présent peut-il charger les mods de ce jeu ?
    pub suitable: bool,
    /// Jeu Unity `il2cpp` ou `mono` (inconnu sinon).
    pub engine: Option<&'static str>,
    /// Installé par ModStudio (depuis Thunderstore) : remplaçable sans risque.
    pub managed: bool,
    /// Paquet à installer : chargeur absent, ou qui ne convient pas.
    pub needed: Option<TsRef>,
    /// Option de lancement Steam pour Proton (Linux seulement).
    pub launch_option: Option<String>,
    /// Option de lancement que le chargeur demande sur tous les systèmes (Godot).
    pub launch_args: Option<String>,
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

fn melon_present(folder: &Path) -> bool {
    folder.join("MelonLoader").is_dir() && folder.join("version.dll").is_file()
}

fn northstar_present(folder: &Path) -> bool {
    folder.join("R2Northstar").is_dir()
}

/// Dossier de l'exécutable du jeu, relatif au dossier du jeu, avec `/` final
/// (`Ship/` pour Hades II, `Release/` pour Scrap Mechanic, `""` sinon).
pub fn exe_rel(folder: &Path, game: &GameRules) -> String {
    let wanted: Vec<String> = game.exe_names.iter().map(|n| n.to_ascii_lowercase()).collect();
    if wanted.is_empty() {
        return String::new();
    }
    let mut level = vec![(folder.to_path_buf(), String::new())];
    for _ in 0..3 {
        let mut next = Vec::new();
        for (dir, rel) in level {
            let Ok(entries) = fs::read_dir(&dir) else { continue };
            let mut entries: Vec<_> = entries.flatten().collect();
            entries.sort_by_key(|e| e.file_name());
            for e in &entries {
                let name = e.file_name().to_string_lossy().into_owned();
                if e.path().is_file() && wanted.contains(&name.to_ascii_lowercase()) {
                    return rel;
                }
            }
            for e in entries.into_iter().filter(|e| e.path().is_dir()) {
                let name = e.file_name().to_string_lossy().into_owned();
                next.push((e.path(), format!("{rel}{name}/")));
            }
        }
        level = next;
    }
    String::new()
}

/// `Binaries/Win64` d'un jeu Unreal (là où va UE4SS), relatif au dossier du jeu.
pub fn unreal_bin_rel(folder: &Path, game: &GameRules) -> String {
    let data = game.data_folder.trim_matches('/');
    if !data.is_empty() && folder.join(data).join("Binaries").join("Win64").is_dir() {
        return format!("{data}/Binaries/Win64");
    }
    // Données inconnues : le premier dossier qui a un `Binaries/Win64` (hors moteur).
    let found = fs::read_dir(folder).ok().and_then(|it| {
        let mut dirs: Vec<_> = it.flatten().filter(|e| e.path().join("Binaries").join("Win64").is_dir()).collect();
        dirs.sort_by_key(|e| e.file_name());
        dirs.into_iter()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .find(|n| !n.eq_ignore_ascii_case("Engine"))
    });
    match found {
        Some(name) => format!("{name}/Binaries/Win64"),
        None if !data.is_empty() => format!("{data}/Binaries/Win64"),
        None => "Binaries/Win64".into(),
    }
}

/// Dossier des mods UE4SS (`ue4ss/Mods` pour UE4SS 3 installé à part).
fn ue4ss_mods_rel(folder: &Path, bin: &str) -> String {
    if folder.join(bin).join("ue4ss").join("UE4SS.dll").is_file() {
        format!("{bin}/ue4ss/Mods")
    } else {
        format!("{bin}/Mods")
    }
}

fn ue4ss_present(folder: &Path, bin: &str) -> bool {
    let dir = folder.join(bin);
    dir.join("dwmapi.dll").is_file() && (dir.join("UE4SS.dll").is_file() || dir.join("ue4ss").join("UE4SS.dll").is_file())
}

/// Le chargeur que le jeu demande est-il en place (et le bon) ?
fn loader_ok(game: &GameRules, folder: &Path) -> bool {
    let exe = || folder.join(exe_rel(folder, game));
    match game.kind {
        LoaderKind::BepInEx => bepinex_suits(folder) == Some(true),
        LoaderKind::BepisLoader => folder.join("BepisLoader.dll").is_file() && folder.join("hostfxr.dll").is_file(),
        LoaderKind::MelonLoader | LoaderKind::RecursiveMelonLoader | LoaderKind::None => melon_present(folder),
        LoaderKind::Northstar => northstar_present(folder),
        LoaderKind::Shimloader => ue4ss_present(folder, &unreal_bin_rel(folder, game)),
        LoaderKind::Lovely => ["winmm.dll", "version.dll"].iter().any(|d| exe().join(d).is_file()),
        LoaderKind::GdWeave => folder.join("GDWeave").join("core").join("GDWeave.dll").is_file(),
        LoaderKind::ReturnOfModding => ["version.dll", "d3d12.dll"].iter().any(|d| exe().join(d).is_file()),
        LoaderKind::GodotMl => folder.join("addons").join("mod_loader").is_dir(),
        LoaderKind::Umm => folder.join("UMM").join("Core").join("UnityModManager.dll").is_file(),
        LoaderKind::Rivet => exe().join("version.dll").is_file() && exe().join("Rivet").join("Loader.dll").is_file(),
        LoaderKind::Other => false,
    }
}

/// DLL que Proton doit laisser au chargeur (options de lancement Steam).
fn proxy_dll(game: &GameRules, folder: &Path) -> Option<&'static str> {
    Some(match game.kind {
        LoaderKind::BepInEx | LoaderKind::Umm => "winhttp",
        LoaderKind::MelonLoader | LoaderKind::RecursiveMelonLoader | LoaderKind::Rivet => "version",
        LoaderKind::None if melon_present(folder) => "version",
        LoaderKind::Shimloader => "dwmapi",
        LoaderKind::Lovely | LoaderKind::GdWeave => "winmm",
        LoaderKind::ReturnOfModding if folder.join(exe_rel(folder, game)).join("d3d12.dll").is_file() => "d3d12",
        LoaderKind::ReturnOfModding => "version",
        LoaderKind::BepisLoader => "hostfxr",
        _ => return None,
    })
}

/// Options de lancement Steam pour Proton (`None` sous Windows).
fn proton_option(dll: &str) -> Option<String> {
    (!cfg!(windows)).then(|| format!("WINEDLLOVERRIDES=\"{dll}=n,b\" %command%"))
}

/// Paquet du chargeur par défaut quand la communauté n'en met aucun en avant.
fn default_loader(game: &GameRules, community: &str) -> Option<&'static str> {
    Some(match game.kind {
        LoaderKind::MelonLoader | LoaderKind::RecursiveMelonLoader => "LavaGang-MelonLoader",
        LoaderKind::Northstar => "northstar-Northstar",
        LoaderKind::BepisLoader => "ResoniteModding-BepisLoader",
        LoaderKind::Shimloader => "Thunderstore-unreal_shimloader",
        LoaderKind::Lovely => "Thunderstore-lovely",
        LoaderKind::GdWeave => "NotNet-GDWeave",
        LoaderKind::ReturnOfModding if community == "hades-ii" => "Hell2Modding-Hell2Modding",
        LoaderKind::ReturnOfModding => "ReturnOfModding-ReturnOfModding",
        LoaderKind::GodotMl => "GodotModding-GodotModLoader",
        LoaderKind::Umm if community == "a-dance-of-fire-and-ice" => "ADOFAI_Modding-UMM_ADOFAI",
        LoaderKind::Umm => "UMM-UMM",
        LoaderKind::Rivet => "ReDoIngMods-Rivet",
        LoaderKind::BepInEx | LoaderKind::None | LoaderKind::Other => return None,
    })
}

/// `Équipe-Nom` → (équipe, nom).
fn split_full(full: &str) -> (String, String) {
    match full.split_once('-') {
        Some((ns, name)) => (ns.to_string(), name.to_string()),
        None => (String::new(), full.to_string()),
    }
}

/// Le paquet est-il un chargeur ? D'après Thunderstore, sinon d'après son nom.
fn loader_of(eco: &Ecosystem, full: &str) -> Option<LoaderPackage> {
    if let Some(l) = eco.loader_package(full) {
        return Some(l.clone());
    }
    let (_, name) = split_full(full);
    let loader = if is_loader_pack(&name) {
        "bepinex"
    } else if is_melon_pack(&name) {
        "melonloader"
    } else {
        return None;
    };
    Some(LoaderPackage { package_id: full.to_string(), loader: loader.into(), root_folder: String::new() })
}

/// Paquet qui complète le chargeur du jeu au lieu de le remplacer : le
/// BepInEx du moteur de rendu de Resonite, à côté de BepisLoader.
fn is_companion(full: &str) -> bool {
    full.eq_ignore_ascii_case("ResoniteModding-BepInExRenderer")
}

/// Ce paquet de chargeur est-il déjà en place dans le dossier du jeu ?
fn loader_pack_ok(full: &str, game: &GameRules, folder: &Path) -> bool {
    if is_companion(full) {
        return folder.join("Renderer").join("BepInEx").join("core").join("BepInEx.Preloader.dll").is_file();
    }
    loader_ok(game, folder)
}

/// Le chargeur principal du jeu (pas un complément comme BepInExRenderer).
fn is_main_loader(eco: &Ecosystem, full: &str) -> bool {
    !is_companion(full) && loader_of(eco, full).is_some()
}

/// Mod installé qui est le chargeur du jeu (BepInExPack…).
fn is_loader_record(m: &InstalledMod, eco: &Ecosystem) -> bool {
    m.file_name.as_deref().is_some_and(|f| is_main_loader(eco, f))
}

/// Le paquet BepInEx qui convient au jeu : celui que la communauté met en
/// avant s'il correspond au jeu (IL2CPP ou Mono), sinon le paquet générique.
fn preferred_bepinex(game: &GameRules, eco: &Ecosystem, folder: &Path) -> String {
    let il2cpp = game_is_il2cpp(folder);
    let known = il2cpp || game_is_mono(folder);
    game.autolist
        .iter()
        .filter_map(|id| eco.loader_package(id))
        .find(|l| l.loader == "bepinex" && (!known || l.package_id.to_ascii_lowercase().contains("il2cpp") == il2cpp))
        .map(|l| l.package_id.clone())
        .unwrap_or_else(|| if il2cpp { "BepInEx-BepInExPack_IL2CPP".into() } else { "BepInEx-BepInExPack".into() })
}

/// Chargeur à installer pour que le jeu charge ses mods (`None` : déjà là).
fn needed_loader(game: &GameRules, eco: &Ecosystem, folder: &Path, community: &str) -> Option<String> {
    if loader_ok(game, folder) {
        return None;
    }
    match game.kind {
        LoaderKind::BepInEx => Some(preferred_bepinex(game, eco, folder)),
        // Le chargeur n'est installé que si un mod le demande.
        LoaderKind::None | LoaderKind::Other => None,
        _ => {
            let wanted = game.loader_id.trim_start_matches("recursive-");
            game.autolist
                .iter()
                .filter_map(|id| eco.loader_package(id))
                .find(|l| l.loader.trim_start_matches("recursive-") == wanted)
                .map(|l| l.package_id.clone())
                .or_else(|| default_loader(game, community).map(String::from))
        }
    }
}

/// Le bon BepInEx pour ce jeu, quand un mod en demande un qui ne peut pas
/// marcher (BepInEx 5 pour un jeu IL2CPP : erreur fréquente des auteurs).
fn right_loader(namespace: &str, name: &str, folder: &Path, game: &GameRules, eco: &Ecosystem) -> (String, String) {
    if game.kind != LoaderKind::BepInEx {
        return (namespace.into(), name.into());
    }
    match name.to_ascii_lowercase().as_str() {
        "bepinexpack" | "bepinexpack_x86" if game_is_il2cpp(folder) => split_full(&preferred_bepinex(game, eco, folder)),
        "bepinexpack_il2cpp" if game_is_mono(folder) => ("BepInEx".into(), "BepInExPack".into()),
        _ => (namespace.into(), name.into()),
    }
}

/// Nom affiché du chargeur qu'un jeu demande.
fn required_label(game: &GameRules, folder: &Path) -> String {
    match game.kind {
        LoaderKind::BepInEx if game_is_il2cpp(folder) => "BepInEx 6 (IL2CPP)".into(),
        LoaderKind::Shimloader => "UE4SS".into(),
        _ => loader_label(&game.loader_id).into(),
    }
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

/// Fichiers d'un paquet à placer.
#[derive(Debug, Default)]
pub struct Placement {
    /// Par dossier (`""` : dossier du jeu ; `mods` : dossier des mods de
    /// Lovely) : source absolue → destination relative à ce dossier.
    pub batches: BTreeMap<String, Vec<(PathBuf, String)>>,
    /// (dossier, destination) non suivies (réglages) : laissées en place à la désinstallation.
    pub untracked: HashSet<(String, String)>,
    /// Fichiers du jeu remplacés par le chargeur : gardés de côté, remis à sa désinstallation.
    pub replaced: Vec<String>,
}

/// Fichiers placés, et ceux qui ne sont pas suivis (réglages).
type Placed = (Vec<installer::PlacedBatch>, HashSet<(String, String)>);

/// Suffixe des fichiers du jeu gardés de côté.
pub const ORIGINAL_SUFFIX: &str = ".modstudio-orig";

/// Ce qu'il faut savoir du jeu pour placer les fichiers d'un paquet.
pub struct Target<'a> {
    pub folder: &'a Path,
    pub game: &'a GameRules,
    /// Dossiers en dehors du dossier du jeu (`mods` de Lovely).
    pub extra: &'a BTreeMap<String, PathBuf>,
}

impl Target<'_> {
    fn base(&self, key: &str) -> PathBuf {
        if key.is_empty() { self.folder.to_path_buf() } else { self.extra.get(key).cloned().unwrap_or_default() }
    }
}

impl Placement {
    fn push(&mut self, src: PathBuf, dest: String, untracked: bool, target: &Target) {
        self.push_in("", src, dest, untracked, target);
    }

    fn push_in(&mut self, key: &str, src: PathBuf, dest: String, untracked: bool, target: &Target) {
        // Réglages déjà là : ceux de l'utilisateur, jamais remplacés.
        if untracked && target.base(key).join(&dest).exists() {
            return;
        }
        // Deux fichiers pour la même destination : le dernier l'emporte (comme r2modman).
        let lower = dest.to_ascii_lowercase();
        let list = self.batches.entry(key.to_string()).or_default();
        list.retain(|(_, d)| d.to_ascii_lowercase() != lower);
        if untracked {
            self.untracked.insert((key.to_string(), dest.clone()));
        }
        list.push((src, dest));
    }

    /// Destinations, pour les tests (`clé:chemin` hors du dossier du jeu).
    #[cfg(test)]
    pub fn dests(&self) -> Vec<String> {
        let mut v: Vec<String> = self
            .batches
            .iter()
            .flat_map(|(k, list)| list.iter().map(move |(_, d)| if k.is_empty() { d.clone() } else { format!("{k}:{d}") }))
            .collect();
        v.sort();
        v
    }
}

/// Dossier de l'archive d'un chargeur dont le contenu va dans le dossier du jeu.
fn loader_root(root: &Path, loader: &LoaderPackage) -> PathBuf {
    let wanted = loader.root_folder.trim();
    let entries: Vec<fs::DirEntry> = fs::read_dir(root).map(|it| it.flatten().filter(|e| e.path().is_dir()).collect()).unwrap_or_default();
    if !wanted.is_empty()
        && let Some(e) = entries.iter().find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(wanted))
    {
        return e.path();
    }
    // Données inconnues : le dossier `BepInExPack…` habituel, sinon la racine.
    if loader.loader == "bepinex"
        && let Some(e) = entries.iter().find(|e| e.file_name().to_string_lossy().to_ascii_lowercase().starts_with("bepinexpack"))
    {
        return e.path();
    }
    root.to_path_buf()
}

/// Entrée de l'archive, insensible à la casse (`UE4SS`, `ue4ss`…).
fn child(dir: &Path, name: &str) -> Option<PathBuf> {
    fs::read_dir(dir).ok()?.flatten().find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(name)).map(|e| e.path())
}

/// Copie `src` (fichier ou dossier) vers `dest` (relatif), dans le dossier `key`.
fn push_tree(out: &mut Placement, key: &str, src: &Path, dest: &str, untracked: &dyn Fn(&str) -> bool, target: &Target) {
    if src.is_file() {
        out.push_in(key, src.to_path_buf(), dest.to_string(), untracked(dest), target);
    } else {
        for (abs, rel) in files_under(src) {
            let d = if dest.is_empty() { rel } else { format!("{dest}/{rel}") };
            let u = untracked(&d);
            out.push_in(key, abs, d, u, target);
        }
    }
}

/// Le chargeur : où va chacun de ses fichiers, selon le jeu.
fn plan_loader(root: &Path, loader: &LoaderPackage, target: &Target, out: &mut Placement) {
    let (folder, game) = (target.folder, target.game);
    let none = |_: &str| false;
    let base_file = |rel: &str| !rel.contains('/') && matches!(rel.to_ascii_lowercase().as_str(), "manifest.json" | "readme.md" | "icon.png");
    match game.kind {
        // MelonLoader 0.7+ : seulement le chargeur, pas ses exemples.
        LoaderKind::RecursiveMelonLoader | LoaderKind::None if loader.loader.contains("melonloader") => {
            for item in ["MelonLoader", "version.dll", "winhttp.dll"] {
                if let Some(path) = child(root, item) {
                    push_tree(out, "", &path, item, &|d: &str| is_user_config(d), target);
                }
            }
        }
        // UE4SS (livré dans le paquet du shimloader) dans `Binaries/Win64`.
        LoaderKind::Shimloader => {
            let bin = unreal_bin_rel(folder, game);
            if let Some(ue4ss) = child(root, "UE4SS") {
                push_tree(out, "", &ue4ss, &bin, &none, target);
            }
        }
        // Lovely : sa DLL à côté du jeu, sa configuration dans le dossier des mods.
        LoaderKind::Lovely => {
            let exe = exe_rel(folder, game);
            for dll in ["winmm.dll", "version.dll"] {
                if let Some(path) = child(root, dll) {
                    out.push(path, format!("{exe}{dll}"), false, target);
                }
            }
            if let Some(dir) = child(root, "lovely") {
                push_tree(out, "mods", &dir, "lovely", &|_: &str| true, target);
            }
        }
        LoaderKind::GdWeave => {
            let src = loader_root(root, loader);
            for item in ["winmm.dll", "GDWeave"] {
                if let Some(path) = child(&src, item) {
                    push_tree(out, "", &path, item, &none, target);
                }
            }
        }
        // Return of Modding : sa DLL à côté de l'exécutable (`Ship/` pour Hades II).
        LoaderKind::ReturnOfModding => {
            let exe = exe_rel(folder, game);
            for (abs, rel) in files_under(&loader_root(root, loader)) {
                if !base_file(&rel) {
                    out.push(abs, format!("{exe}{rel}"), false, target);
                }
            }
        }
        LoaderKind::GodotMl => {
            for item in ["mod_loader", "JSON_Schema_Validator"] {
                if let Some(path) = child(root, "addons").and_then(|a| child(&a, item)) {
                    push_tree(out, "", &path, &format!("addons/{item}"), &none, target);
                }
            }
        }
        LoaderKind::Umm => {
            for item in ["UMM/Core", "doorstop_config.ini", "winhttp.dll", ".doorstop_version", "doorstop_libs", "run_umm.sh"] {
                let path = item.split('/').try_fold(root.to_path_buf(), |dir, part| child(&dir, part));
                if let Some(path) = path {
                    push_tree(out, "", &path, item, &none, target);
                }
            }
        }
        // Rivet : à côté de l'exécutable (`Release/`), réglages gardés.
        LoaderKind::Rivet => {
            let exe = exe_rel(folder, game);
            let pack = child(root, "RivetPack").unwrap_or_else(|| root.to_path_buf());
            for item in ["version.dll", "Rivet.ini", "Rivet"] {
                if let Some(path) = child(&pack, item) {
                    let ini = item == "Rivet.ini";
                    push_tree(out, "", &path, &format!("{exe}{item}"), &move |_: &str| ini, target);
                }
            }
        }
        // BepInEx, BepisLoader, MelonLoader, Northstar : le contenu de son dossier dans le dossier du jeu.
        _ => {
            for (abs, rel) in files_under(&loader_root(root, loader)) {
                if base_file(&rel) {
                    continue;
                }
                // Resonite sous Linux : son lanceur est remplacé par celui de BepisLoader.
                if game.kind == LoaderKind::BepisLoader && rel.eq_ignore_ascii_case("LinuxBootstrap.sh") && folder.join(&rel).is_file() {
                    out.replaced.push(rel.clone());
                }
                let user = is_user_config(&rel);
                out.push(abs, rel, user, target);
            }
        }
    }
}

/// Où va chaque fichier de l'archive extraite (`root`) du paquet `full`,
/// selon le chargeur du jeu et ses règles (comme r2modman, mais dans le
/// dossier du jeu au lieu d'un profil).
pub fn ts_plan(root: &Path, full: &str, target: &Target, loader: Option<&LoaderPackage>) -> AppResult<Placement> {
    let (folder, game) = (target.folder, target.game);
    let mut out = Placement::default();
    if let Some(loader) = loader {
        plan_loader(root, loader, target, &mut out);
        return Ok(out);
    }

    match game.kind {
        // `UserData` dans `UserData/<Équipe-Nom>`, tout le reste dans `Mods/<Équipe-Nom>`.
        LoaderKind::RecursiveMelonLoader | LoaderKind::None => {
            let mut entries: Vec<fs::DirEntry> = fs::read_dir(root).map(|it| it.flatten().collect()).unwrap_or_default();
            entries.sort_by_key(|e| e.file_name());
            for e in entries {
                let name = e.file_name().to_string_lossy().into_owned();
                let path = e.path();
                if path.is_dir() && name.eq_ignore_ascii_case("UserData") {
                    push_tree(&mut out, "", &path, &format!("UserData/{full}"), &|_: &str| true, target);
                } else {
                    push_tree(&mut out, "", &path, &format!("Mods/{full}/{name}"), &|_: &str| false, target);
                }
            }
        }
        // Lovely : tout le paquet dans son dossier, dans le dossier des mods.
        LoaderKind::Lovely => push_tree(&mut out, "mods", root, full, &|_: &str| false, target),
        // GDWeave : le dossier qui a son propre `manifest.json`, dans `GDWeave/mods/<Équipe-Nom>`.
        LoaderKind::GdWeave => {
            let dir = find_dir_with(root, "manifest.json", true)
                .ok_or_else(|| AppError::BadArchive("aucun dossier de mod GDWeave (avec son manifest.json) dans l'archive".into()))?;
            push_tree(&mut out, "", &dir, &format!("GDWeave/mods/{full}"), &|_: &str| false, target);
        }
        // Rivet : tout le paquet dans `Rivet/Mods/<Équipe-Nom>`, à côté de l'exécutable.
        LoaderKind::Rivet => {
            let exe = exe_rel(folder, game);
            push_tree(&mut out, "", root, &format!("{exe}Rivet/Mods/{full}"), &|_: &str| false, target);
        }
        // Return of Modding : `plugins`, `plugins_data`, `config`, chemins gardés.
        LoaderKind::ReturnOfModding => {
            let base = format!("{}ReturnOfModding", exe_rel(folder, game));
            let rule = |name: &str, default| Rule {
                route: format!("{base}/{name}"),
                tracking: Tracking::SubdirNoFlatten,
                extensions: Vec::new(),
                default,
            };
            let rules = [rule("plugins", true), rule("plugins_data", false), rule("config", false)];
            plan_rules(root, full, target, &rules, &[], &mut out)?;
            // Les réglages restent à l'utilisateur.
            let config = format!("{base}/config/").to_ascii_lowercase();
            for (_, d) in out.batches.get("").into_iter().flatten() {
                if d.to_ascii_lowercase().starts_with(&config) {
                    out.untracked.insert((String::new(), d.clone()));
                }
            }
        }
        LoaderKind::Shimloader => plan_shimloader(root, full, target, &mut out)?,
        LoaderKind::Other => {
            return Err(AppError::ThunderstoreUnsupported(loader_label(&game.loader_id).to_string()));
        }
        // BepInEx, BepisLoader, MelonLoader, Northstar, Unity Mod Manager, Godot : règles du jeu.
        _ => plan_rules(root, full, target, &game.rules, &game.exclusions, &mut out)?,
    }
    Ok(out)
}

/// Premier dossier (en profondeur) qui contient `file` ; `skip_top` : pas la racine.
fn find_dir_with(dir: &Path, file: &str, skip_top: bool) -> Option<PathBuf> {
    if !skip_top && dir.join(file).is_file() {
        return Some(dir.to_path_buf());
    }
    let mut subdirs: Vec<_> = fs::read_dir(dir).ok()?.flatten().filter(|e| e.path().is_dir()).collect();
    subdirs.sort_by_key(|e| e.file_name());
    subdirs.into_iter().find_map(|e| find_dir_with(&e.path(), file, false))
}

/// Jeux Unreal : les dossiers que le shimloader simule, à leur vraie place
/// (`Binaries/Win64/Mods`, `Content/Paks/LogicMods`, `Config`).
fn plan_shimloader(root: &Path, full: &str, target: &Target, out: &mut Placement) -> AppResult<()> {
    let (folder, game) = (target.folder, target.game);
    let bin = unreal_bin_rel(folder, game);
    let data = bin.trim_end_matches("/Binaries/Win64").to_string();
    // Paquet rangé comme le dossier du jeu (`Pal/Binaries/…`) : tel quel.
    if !data.is_empty()
        && let Some(dir) = child(root, &data)
        && dir.is_dir()
    {
        push_tree(out, "", &dir, &data, &|_: &str| false, target);
        return Ok(());
    }
    let mut virt = Placement::default();
    plan_rules(root, full, target, &game.rules, &game.exclusions, &mut virt)?;
    let mods = ue4ss_mods_rel(folder, &bin);
    let mut enabled: BTreeMap<String, bool> = BTreeMap::new();
    for (src, dest) in virt.batches.remove("").unwrap_or_default() {
        let untracked = virt.untracked.contains(&(String::new(), dest.clone()));
        let lower = dest.to_ascii_lowercase();
        let rest = |prefix: &str| dest[prefix.len()..].to_string();
        let real = if lower.starts_with("shimloader/mod/") {
            let r = rest("shimloader/mod/");
            if let Some((modname, file)) = r.split_once('/') {
                let seen = enabled.entry(modname.to_string()).or_default();
                *seen |= file.eq_ignore_ascii_case("enabled.txt");
            }
            format!("{mods}/{r}")
        } else if lower.starts_with("shimloader/pak/") {
            format!("{data}/Content/Paks/LogicMods/{}", rest("shimloader/pak/"))
        } else if lower.starts_with("shimloader/cfg/") {
            format!("{data}/Config/{}", rest("shimloader/cfg/"))
        } else if lower.starts_with("shimloader/overlay/") {
            // Chaque paquet superposé à `Binaries/Win64`.
            let r = rest("shimloader/overlay/");
            format!("{bin}/{}", r.split_once('/').map(|(_, f)| f).unwrap_or(&r))
        } else {
            dest.clone()
        };
        out.push(src, real, untracked, target);
    }
    // UE4SS ne charge un mod que s'il a un `enabled.txt` (le shimloader les active tous).
    for (modname, has) in enabled {
        if !has {
            let marker = root.with_extension(format!("{modname}.enabled.txt"));
            fs::write(&marker, b"").map_err(installer::io_err)?;
            out.push(marker, format!("{mods}/{modname}/enabled.txt"), false, target);
        }
    }
    Ok(())
}

/// Règle qui prend un fichier : extension la plus longue qui correspond,
/// sinon la destination par défaut.
fn rule_for_file<'a>(name: &str, rules: &'a [Rule]) -> Option<&'a Rule> {
    let lower = name.to_ascii_lowercase();
    rules
        .iter()
        .filter_map(|r| r.extensions.iter().filter(|e| lower.ends_with(e.as_str())).map(|e| e.len()).max().map(|n| (n, r)))
        .fold(None::<(usize, &Rule)>, |best, (n, r)| match best {
            Some((b, _)) if b > n => best,
            _ => Some((n, r)),
        })
        .map(|(_, r)| r)
        .or_else(|| rules.iter().find(|r| r.default))
}

/// Règle qui prend un dossier entier : son nom est celui du dernier dossier
/// de la règle (`plugins` pour `BepInEx/plugins`) ; à égalité, celle dont le
/// chemin correspond le mieux.
fn rule_for_dir<'a>(rel: &str, rules: &'a [Rule]) -> Option<&'a Rule> {
    let name = rel.rsplit('/').next().unwrap_or(rel);
    let parts: Vec<&str> = rel.split('/').rev().collect();
    rules
        .iter()
        .filter(|r| r.route.rsplit('/').next().is_some_and(|last| last.eq_ignore_ascii_case(name)))
        .map(|r| {
            let score = r.route.split('/').rev().zip(parts.iter()).filter(|(a, b)| a.eq_ignore_ascii_case(b)).count();
            (score, r)
        })
        .fold(None::<(usize, &Rule)>, |best, (n, r)| match best {
            Some((b, _)) if b >= n => best,
            _ => Some((n, r)),
        })
        .map(|(_, r)| r)
}

/// Ce que chaque règle reçoit : fichiers et dossiers entiers de l'archive.
fn collect<'a>(dir: &Path, rel: &str, rules: &'a [Rule], out: &mut Vec<(&'a Rule, PathBuf, String)>) {
    let mut entries: Vec<fs::DirEntry> = fs::read_dir(dir).map(|it| it.flatten().collect()).unwrap_or_default();
    entries.sort_by_key(|e| e.file_name());
    let join = |name: &str| if rel.is_empty() { name.to_string() } else { format!("{rel}/{name}") };
    // Les fichiers d'abord, puis les dossiers (même ordre que r2modman).
    for e in entries.iter().filter(|e| e.path().is_file()) {
        let name = e.file_name().to_string_lossy().into_owned();
        if let Some(rule) = rule_for_file(&name, rules) {
            out.push((rule, e.path(), join(&name)));
        }
    }
    for e in entries.iter().filter(|e| e.path().is_dir()) {
        let name = e.file_name().to_string_lossy().into_owned();
        let sub = join(&name);
        match rule_for_dir(&sub, rules) {
            Some(rule) => out.push((rule, e.path(), sub)),
            None => collect(&e.path(), &sub, rules, out),
        }
    }
}

fn plan_rules(root: &Path, full: &str, target: &Target, rules: &[Rule], exclusions: &[String], out: &mut Placement) -> AppResult<()> {
    let mut intents = Vec::new();
    collect(root, "", rules, &mut intents);
    let excluded = |rel: &str| {
        let lower = rel.to_ascii_lowercase();
        let base = lower.rsplit('/').next().unwrap_or(&lower).to_string();
        exclusions.iter().any(|x| *x == lower || *x == base)
    };
    let mut zipped = HashSet::new();
    for (rule, src, rel) in intents {
        let route = rule.route.trim_end_matches('/');
        let base = src.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let is_file = src.is_file();
        match rule.tracking {
            Tracking::PackageZip => {
                // Tout le paquet, zippé tel quel, une seule fois.
                if zipped.insert(route.to_string()) {
                    let zip = root.with_extension("ts.zip");
                    zip_dir(root, &zip)?;
                    out.push(zip, format!("{route}/{full}.ts.zip"), false, target);
                }
            }
            Tracking::Subdir if is_file => out.push(src, format!("{route}/{full}/{base}"), false, target),
            Tracking::SubdirNoFlatten if is_file => out.push(src, format!("{route}/{full}/{rel}"), false, target),
            Tracking::Subdir | Tracking::SubdirNoFlatten => {
                for (abs, sub) in files_under(&src) {
                    out.push(abs, format!("{route}/{full}/{sub}"), false, target);
                }
            }
            Tracking::None | Tracking::State => {
                let untracked = rule.tracking == Tracking::None;
                if is_file {
                    // Les fichiers de présentation de chaque paquet se marcheraient dessus.
                    if (!rel.contains('/') && is_package_meta(&rel)) || excluded(&base) {
                        continue;
                    }
                    out.push(src, format!("{route}/{base}"), untracked, target);
                } else {
                    for (abs, sub) in files_under(&src) {
                        if !excluded(&sub) {
                            out.push(abs, format!("{route}/{sub}"), untracked, target);
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

/// Zippe un dossier (règle « package-zip » : mods Godot).
fn zip_dir(dir: &Path, dest: &Path) -> AppResult<()> {
    use std::io::Write;
    let file = fs::File::create(dest).map_err(installer::io_err)?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (abs, rel) in files_under(dir) {
        zip.start_file(rel, options).map_err(|e| AppError::Io(e.to_string()))?;
        let data = fs::read(&abs).map_err(installer::io_err)?;
        zip.write_all(&data).map_err(installer::io_err)?;
    }
    zip.finish().map_err(|e| AppError::Io(e.to_string()))?;
    Ok(())
}

/// Met de côté les fichiers du jeu que le chargeur remplace (`x` → `x.modstudio-orig`).
fn keep_originals(folder: &Path, files: &[String]) -> Vec<String> {
    let mut kept = Vec::new();
    for rel in files {
        let (path, orig) = (folder.join(rel), folder.join(format!("{rel}{ORIGINAL_SUFFIX}")));
        if path.is_file() && !orig.exists() && fs::rename(&path, &orig).is_ok() {
            kept.push(rel.clone());
        }
    }
    kept
}

/// Remet les fichiers du jeu mis de côté (quand le fichier du chargeur n'est plus là).
fn restore_originals(folder: &Path, files: &[String]) {
    for rel in files {
        let (path, orig) = (folder.join(rel), folder.join(format!("{rel}{ORIGINAL_SUFFIX}")));
        if orig.is_file() && !path.exists() {
            let _ = fs::rename(&orig, &path);
        }
    }
}

/// Remplace (ou ajoute) `clé=valeur` dans une section d'un fichier INI.
fn set_ini(text: &str, section: &str, key: &str, value: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let (mut current, mut done, mut seen) = (String::new(), false, false);
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with('[') && t.ends_with(']') {
            if current.eq_ignore_ascii_case(section) && !done {
                out.push(format!("{key}={value}"));
                done = true;
            }
            current = t[1..t.len() - 1].trim().to_string();
            seen |= current.eq_ignore_ascii_case(section);
        } else if current.eq_ignore_ascii_case(section)
            && !t.starts_with(';')
            && !t.starts_with('#')
            && t.split_once('=').is_some_and(|(k, _)| k.trim().eq_ignore_ascii_case(key))
        {
            if !done {
                out.push(format!("{key}={value}"));
                done = true;
            }
            continue;
        }
        out.push(line.to_string());
    }
    if !done {
        if !seen {
            if out.last().is_some_and(|l| !l.trim().is_empty()) {
                out.push(String::new());
            }
            out.push(format!("[{section}]"));
        }
        out.push(format!("{key}={value}"));
    }
    out.join("\n") + "\n"
}

/// Active le chargeur qu'on vient d'installer, quand il est livré éteint.
fn activate(target: &Target) -> AppResult<()> {
    let edit = |path: PathBuf, changes: &[(&str, &str, &str)]| -> AppResult<()> {
        let mut text = fs::read_to_string(&path).unwrap_or_default();
        for (section, key, value) in changes {
            text = set_ini(&text, section, key, value);
        }
        fs::write(&path, text).map_err(installer::io_err)
    };
    match target.game.kind {
        // Resonite : hookfxr lance BepisLoader au lieu du jeu.
        LoaderKind::BepisLoader if target.folder.join("hookfxr.ini").is_file() => {
            edit(target.folder.join("hookfxr.ini"), &[("hookfxr", "enable", "true")])
        }
        // Scrap Mechanic : Rivet et ses mods dans le dossier `Rivet`.
        LoaderKind::Rivet => {
            let ini = target.folder.join(exe_rel(target.folder, target.game)).join("Rivet.ini");
            edit(ini, &[("Doorstop", "enable", "true"), ("Doorstop", "target", "Rivet/Loader.dll"), ("Loader", "directory", "Rivet/Mods")])
        }
        _ => Ok(()),
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
    pub async fn loader(&self, game_id: i64) -> AppResult<TsLoader> {
        let eco = self.ecosystem().await;
        let (folder, managed, community) = {
            let lib = self.library.lock().expect("library");
            let game = lib.games.get(&game_id).ok_or(AppError::NotFound)?;
            (game.folder.clone(), game.mods.values().any(|m| is_loader_record(m, &eco)), game.domain.clone().unwrap_or_default())
        };
        let rules = eco.game(&community);
        let engine = if game_is_il2cpp(&folder) {
            Some("il2cpp")
        } else if game_is_mono(&folder) {
            Some("mono")
        } else {
            None
        };
        let suitable = match &rules {
            Some(g) => loader_ok(g, &folder),
            None => bepinex_suits(&folder) == Some(true) || melon_present(&folder),
        };
        // Chargeur présent : le bon (son nom), sinon un BepInEx ou un MelonLoader qui ne convient pas.
        let name: Option<&'static str> = match (&rules, bepinex_flavor(&folder)) {
            (Some(g), _) if suitable && g.kind != LoaderKind::BepInEx => Some(loader_label(&g.loader_id)).map(|l| match g.kind {
                LoaderKind::Shimloader => "UE4SS",
                _ => l,
            }),
            (_, Some(f)) => Some(f.label()),
            _ if melon_present(&folder) => Some("MelonLoader"),
            _ if northstar_present(&folder) => Some("Northstar"),
            _ => None,
        };
        let launch_option = match (&rules, name) {
            (Some(g), Some(_)) => proxy_dll(g, &folder).and_then(proton_option),
            (None, Some(_)) => bepinex_flavor(&folder).map(|_| "winhttp").or(melon_present(&folder).then_some("version")).and_then(proton_option),
            _ => None,
        };
        let launch_args = rules
            .as_ref()
            .filter(|g| g.kind == LoaderKind::GodotMl)
            .map(|_| "--script res://addons/mod_loader/mod_loader_setup.gd".to_string());
        let needed = rules
            .as_ref()
            .filter(|g| g.kind.supported())
            .and_then(|g| needed_loader(g, &eco, &folder, &community))
            .map(|full| {
                let (namespace, name) = split_full(&full);
                TsRef { id: stable_id(&full), namespace, name }
            });
        Ok(TsLoader {
            required: rules.as_ref().map(|g| g.loader_id.clone()),
            required_label: rules.as_ref().map(|g| required_label(g, &folder)),
            supported: rules.as_ref().is_some_and(|g| g.kind.supported()),
            name,
            suitable,
            engine,
            managed,
            needed,
            launch_option,
            launch_args,
        })
    }

    /// Retire le BepInEx installé par ModStudio qui ne convient pas au jeu ;
    /// renvoie son statut (nécessaire ou non, et à quels mods) pour le nouveau.
    fn drop_wrong_loader(&self, game_id: i64, eco: &Ecosystem) -> AppResult<(bool, Vec<i64>)> {
        let root = self.disabled_root();
        let mut lib = self.library.lock().expect("library");
        let game = lib.games.get_mut(&game_id).ok_or(AppError::NotFound)?;
        let is_bepinex = |m: &InstalledMod| m.file_name.as_deref().and_then(|f| loader_of(eco, f)).is_some_and(|l| l.loader == "bepinex");
        let wrong: Vec<InstalledMod> = game.mods.values().filter(|m| is_bepinex(m)).cloned().collect();
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
    async fn plan(&self, target: Planned, deps: Vec<String>, config: &GameInstall, game: &GameRules, eco: &Ecosystem) -> AppResult<Plan> {
        let installed = &config.mods;
        let ready = loader_ok(game, &config.folder);
        let community = config.domain.clone().unwrap_or_default();
        let target_id = target.id();
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
                let (ns, name) = right_loader(&ns, &name, &config.folder, game, eco);
                let id = stable_id(&format!("{ns}-{name}"));
                plan.parents.entry(id).or_default().push(parent);
                if installed.contains_key(&id) || visited.contains(&id) {
                    continue;
                }
                let full = format!("{ns}-{name}");
                if ready && is_main_loader(eco, &full) {
                    plan.kept_loader = true;
                    visited.insert(id);
                    continue;
                }
                // BepInExRenderer déjà installé à la main : gardé.
                if is_companion(&full) && loader_pack_ok(&full, game, &config.folder) {
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
        // Aucun mod n'a demandé le chargeur du jeu, absent : installé d'abord.
        if !plan.order.iter().any(|p| is_main_loader(eco, &p.full()))
            && let Some(full) = needed_loader(game, eco, &config.folder, &community)
        {
            let (ns, name) = split_full(&full);
            let id = stable_id(&full);
            if !installed.contains_key(&id) {
                let latest = self.client.latest(&ns, &name).await?;
                plan.parents.entry(id).or_default().push(target_id);
                plan.order.insert(0, Planned { namespace: ns, name, version: latest.version_number, icon: latest.icon });
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
        let mut config = self.library.lock().expect("library").games.get(&game_id).cloned().ok_or(AppError::NoGameFolder)?;
        if !config.folder.is_dir() {
            return Err(AppError::Io(format!("le dossier du jeu n'existe plus : {}", config.folder.display())));
        }
        let eco = self.ecosystem().await;
        let game = eco.game(community).ok_or_else(|| AppError::ThunderstoreUnsupported(loader_label("").into()))?;
        if !game.kind.supported() {
            return Err(AppError::ThunderstoreUnsupported(loader_label(&game.loader_id).into()));
        }
        // Lovely : les mods vont dans le dossier des mods du jeu (`%AppData%/Balatro/Mods`).
        if game.kind == LoaderKind::Lovely && !config.extra_folders.contains_key("mods") {
            let dir = self.lovely_mods_dir(&config.folder, &game)?;
            if let Some(g) = self.library.lock().expect("library").games.get_mut(&game_id) {
                g.extra_folders.insert("mods".into(), dir.clone());
            }
            self.save_library(&self.library.lock().expect("library"))?;
            config.extra_folders.insert("mods".into(), dir);
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
        // Le chargeur lui-même, déjà là sans ModStudio : rien à faire.
        let target_full = target.full();
        if loader_of(&eco, &target_full).is_some() && loader_pack_ok(&target_full, &game, &config.folder) && !config.mods.contains_key(&target_id) {
            let label = if is_companion(&target_full) { display_name(name) } else { required_label(&game, &config.folder) };
            return Err(AppError::ToolPresent(label));
        }
        let plan = self.plan(target, info.dependencies.clone(), &config, &game, &eco).await?;
        // Un BepInEx qui ne convient pas au jeu est en place : remplacé par le bon.
        let is_bepinex = |p: &Planned| loader_of(&eco, &p.full()).is_some_and(|l| l.loader == "bepinex");
        let inherited = if game.kind == LoaderKind::BepInEx && bepinex_suits(&config.folder) == Some(false) && plan.order.iter().any(is_bepinex) {
            Some(self.drop_wrong_loader(game_id, &eco)?)
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
            let inherit = inherited.as_ref().filter(|_| is_bepinex(pkg));
            let loader = loader_of(&eco, &pkg.full());
            let place = (&game, loader);
            if let Err(e) = self.install_one(&config, pkg, pkg.id() == target_id, &plan, inherit, place, &job_dir, other_libraries, &cancel, &emit).await {
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
        (game, loader): (&GameRules, Option<LoaderPackage>),
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
        let (game_b, loader_b) = (game.clone(), loader.clone());
        let extracted = job_dir.join(&full);
        let (placed, untracked) = tokio::task::spawn_blocking(move || -> AppResult<Placed> {
            let size = fs::metadata(&zip_b).map(|m| m.len()).unwrap_or(0);
            let progress = |done: u64, total: u64| emit_b(InstallEvent::Extracting { done, total });
            archive::extract(&zip_b, &extracted, installer::extraction_limit(size, 0), &cancel_b, &progress)?;
            emit_b(InstallEvent::Finishing);
            let target = Target { folder: &config_b.folder, game: &game_b, extra: &config_b.extra_folders };
            let placement = ts_plan(&extracted, &full_b, &target, loader_b.as_ref())?;
            // Fichiers du jeu remplacés par le chargeur : gardés de côté.
            let kept = keep_originals(&config_b.folder, &placement.replaced);
            match place_plan(&config_b, old_b.as_ref(), &others, placement.batches, None) {
                Ok(placed) => {
                    if loader_b.is_some() {
                        activate(&target)?;
                    }
                    Ok((placed, placement.untracked))
                }
                Err(e) => {
                    restore_originals(&config_b.folder, &kept);
                    Err(e)
                }
            }
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
        let loader = loader.is_some();
        let full = record.file_name.clone().unwrap_or_default();
        // Dossiers partagés (`BepInEx/plugins`…) : jamais retirés avec un mod.
        let own = |d: &String| {
            loader || *d == full || d.starts_with(&format!("{full}/")) || d.contains(&format!("/{full}/")) || d.ends_with(&format!("/{full}"))
        };
        for batch in placed {
            // Les réglages restent à l'utilisateur.
            let key = batch.key.clone();
            let files: Vec<String> = batch.files.into_iter().filter(|f| !untracked.contains(&(key.clone(), f.clone()))).collect();
            let dirs: Vec<String> = batch.dirs.into_iter().filter(own).collect();
            if key.is_empty() {
                record.files = files;
                record.dirs = dirs;
            } else if !files.is_empty() || !dirs.is_empty() {
                record.parts.push(ModPart { key, files, dirs });
            }
        }
        let mut lib = self.library.lock().expect("library");
        let game = lib.games.get_mut(&config.game_id).ok_or(AppError::NotFound)?;
        game.mods.insert(mod_id, record);
        self.save_library(&lib)
    }

    /// Dossier des mods de Lovely : `%AppData%/<jeu>/Mods` (dans le préfixe Proton sous Linux).
    fn lovely_mods_dir(&self, folder: &Path, game: &GameRules) -> AppResult<PathBuf> {
        let name = game
            .exe_names
            .first()
            .and_then(|e| Path::new(e).file_stem().map(|s| s.to_string_lossy().into_owned()))
            .unwrap_or_else(|| "Balatro".into());
        let prefix = crate::games::proton::prefix_for_dir(folder);
        let dir = crate::games::proton::expand(&format!("%APPDATA%\\{name}\\Mods"), prefix.as_deref())
            .map(PathBuf::from)
            .ok_or_else(|| AppError::InvalidArgument("dossier des mods introuvable : lance le jeu une fois (avec Proton sous Linux), puis réessaie.".into()))?;
        fs::create_dir_all(&dir).map_err(installer::io_err)?;
        Ok(dir)
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
        // Fichiers du jeu que le chargeur avait remplacés : remis.
        restore_originals(&game.folder, &record.files);
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


    fn plan_in(root: &Path, full: &str, game: &Path, community: &str, extra: &BTreeMap<String, PathBuf>) -> Placement {
        let eco = Ecosystem::bundled();
        let rules = eco.game(community).unwrap();
        let loader = loader_of(&eco, full);
        ts_plan(root, full, &Target { folder: game, game: &rules, extra }, loader.as_ref()).unwrap()
    }

    fn plan(root: &Path, full: &str, game: &Path, community: &str) -> Vec<String> {
        plan_in(root, full, game, community, &BTreeMap::new()).dests()
    }

    /// Archive extraite dans un dossier neuf.
    fn package(tag: &str, entries: &[(&str, &[u8])]) -> PathBuf {
        let dir = tempdir(tag).join("pkg");
        unzip_to(&make_zip(entries), &dir);
        dir
    }

    #[test]
    fn plans_follow_the_bepinex_rules() {
        let game = tempdir("ts-game");
        fs::create_dir_all(game.join("BepInEx/config")).unwrap();
        fs::write(game.join("BepInEx/config/existing.cfg"), b"user").unwrap();

        // Mod simple : DLL et fichiers du paquet à la racine.
        let a = tempdir("ts-a");
        unzip_to(&make_zip(&[("MoreCompany.dll", b"dll"), ("manifest.json", b"{}"), ("icon.png", b"png")]), &a);
        assert_eq!(
            plan(&a, "notnotnotswipez-MoreCompany", &game, "lethal-company"),
            [
                "BepInEx/plugins/notnotnotswipez-MoreCompany/MoreCompany.dll",
                "BepInEx/plugins/notnotnotswipez-MoreCompany/icon.png",
                "BepInEx/plugins/notnotnotswipez-MoreCompany/manifest.json",
            ]
        );

        // Dossiers des règles (plugins, patchers, config jamais par-dessus les réglages
        // existants, `.mm.dll` dans monomod) ; les autres dossiers sont aplatis, comme r2modman.
        let b = tempdir("ts-b");
        unzip_to(
            &make_zip(&[
                ("BepInEx/plugins/Lib/Lib.dll", b"dll"),
                ("patchers/Patch.dll", b"p"),
                ("config/existing.cfg", b"defaut"),
                ("config/new.cfg", b"defaut"),
                ("Assets/bundle", b"x"),
                ("Hooks.mm.dll", b"mm"),
            ]),
            &b,
        );
        assert_eq!(
            plan(&b, "Team-Lib", &game, "lethal-company"),
            [
                "BepInEx/config/new.cfg",
                "BepInEx/monomod/Team-Lib/Hooks.mm.dll",
                "BepInEx/patchers/Team-Lib/Patch.dll",
                "BepInEx/plugins/Team-Lib/Lib/Lib.dll",
                "BepInEx/plugins/Team-Lib/bundle",
            ]
        );

        // Règles propres à un jeu : cartes de Timberborn.
        let m = tempdir("ts-maps");
        unzip_to(&make_zip(&[("Maps/Island.timber", b"map")]), &m);
        assert_eq!(plan(&m, "Team-Island", &game, "timberborn"), ["BepInEx/Maps/Team-Island/Island.timber"]);

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
        assert_eq!(
            plan(&c, "BepInEx-BepInExPack", &game, "lethal-company"),
            ["BepInEx/core/BepInEx.dll", "doorstop_config.ini", "winhttp.dll"]
        );
        // Paquet de Valheim : son dossier porte un autre nom.
        let v = tempdir("ts-v");
        unzip_to(&make_zip(&[("BepInExPack_Valheim/winhttp.dll", b"proxy"), ("BepInExPack_Valheim/unstripped_corlib/x.dll", b"x")]), &v);
        assert_eq!(plan(&v, "denikson-BepInExPack_Valheim", &game, "valheim"), ["unstripped_corlib/x.dll", "winhttp.dll"]);
    }

    #[test]
    fn plans_follow_the_melonloader_rules() {
        let game = tempdir("ts-melon-game");
        // BONELAB : DLL dans Mods, `.lib.dll` dans UserLibs, cartes dans UserData/CustomMaps ;
        // les fichiers de présentation ne sont pas copiés (ils se marcheraient dessus).
        let a = tempdir("ts-bonelab");
        unzip_to(
            &make_zip(&[
                ("Fusion.dll", b"dll"),
                ("Helper.lib.dll", b"lib"),
                ("UserData/CustomMaps/Town.bcm", b"map"),
                ("manifest.json", b"{}"),
                ("icon.png", b"png"),
            ]),
            &a,
        );
        assert_eq!(
            plan(&a, "Lakatrazz-Fusion", &game, "bonelab"),
            ["Mods/Fusion.dll", "UserData/CustomMaps/Town.bcm", "UserLibs/Helper.lib.dll"]
        );

        // MelonLoader 0.7+ (Schedule I) : chaque mod dans son dossier.
        let b = tempdir("ts-schedule");
        unzip_to(&make_zip(&[("Mods/Cool.dll", b"dll"), ("UserData/cool.cfg", b"cfg"), ("manifest.json", b"{}")]), &b);
        assert_eq!(
            plan(&b, "Team-Cool", &game, "schedule-i"),
            ["Mods/Team-Cool/Mods/Cool.dll", "Mods/Team-Cool/manifest.json", "UserData/Team-Cool/cool.cfg"]
        );
        // Son chargeur : MelonLoader et version.dll seulement.
        let c = tempdir("ts-ml");
        unzip_to(
            &make_zip(&[("MelonLoader/net6/MelonLoader.dll", b"ml"), ("version.dll", b"proxy"), ("NOTICE.txt", b"n"), ("manifest.json", b"{}")]),
            &c,
        );
        assert_eq!(plan(&c, "LavaGang-MelonLoader", &game, "schedule-i"), ["MelonLoader/net6/MelonLoader.dll", "version.dll"]);
    }

    #[test]
    fn package_zip_rule_zips_the_whole_package() {
        let game = tempdir("ts-zip-game");
        let rules = GameRules {
            loader_id: "bepinex".into(),
            kind: LoaderKind::BepInEx,
            rules: vec![Rule { route: "mods".into(), tracking: Tracking::PackageZip, extensions: vec![], default: true }],
            exclusions: vec![],
            autolist: vec![],
            exe_names: vec![],
            data_folder: String::new(),
        };
        let root = tempdir("ts-zip").join("Team-Godot");
        unzip_to(&make_zip(&[("mod.pck", b"pck"), ("manifest.json", b"{}")]), &root);
        let extra = BTreeMap::new();
        let placement = ts_plan(&root, "Team-Godot", &Target { folder: &game, game: &rules, extra: &extra }, None).unwrap();
        let moves = &placement.batches[""];
        assert_eq!(moves.len(), 1);
        assert_eq!(moves[0].1, "mods/Team-Godot.ts.zip");
        let mut z = zip::ZipArchive::new(fs::File::open(&moves[0].0).unwrap()).unwrap();
        assert!(z.by_name("mod.pck").is_ok());
    }

    #[test]
    fn unreal_games_get_ue4ss_and_mods_where_the_shimloader_points() {
        let game = tempdir("ts-pal");
        fs::create_dir_all(game.join("Pal/Binaries/Win64")).unwrap();
        fs::write(game.join("Palworld.exe"), b"exe").unwrap();
        let bin = "Pal/Binaries/Win64";
        // Le paquet du shimloader : UE4SS dans Binaries/Win64 (pas la DLL du shim).
        let loader = package(
            "ts-shim",
            &[
                ("dwmapi.dll", b"shim"),
                ("UE4SS/dwmapi.dll", b"proxy"),
                ("UE4SS/UE4SS.dll", b"ue4ss"),
                ("UE4SS/UE4SS-settings.ini", b"ini"),
                ("UE4SS/Mods/mods.txt", b"list"),
                ("manifest.json", b"{}"),
            ],
        );
        assert_eq!(
            plan(&loader, "Thunderstore-unreal_shimloader", &game, "palworld"),
            [format!("{bin}/Mods/mods.txt"), format!("{bin}/UE4SS-settings.ini"), format!("{bin}/UE4SS.dll"), format!("{bin}/dwmapi.dll")]
        );
        // Mod Lua, mod DLL sans enabled.txt (ajouté), pak, réglages.
        let m = package(
            "ts-shim-mod",
            &[
                ("mod/enabled.txt", b""),
                ("mod/scripts/main.lua", b"lua"),
                ("pak/Big.pak", b"pak"),
                ("cfg/Mod-config.ini", b"cfg"),
            ],
        );
        let p = plan_in(&m, "Team-Mod", &game, "palworld", &BTreeMap::new());
        assert_eq!(
            p.dests(),
            [
                "Pal/Binaries/Win64/Mods/Team-Mod/enabled.txt",
                "Pal/Binaries/Win64/Mods/Team-Mod/scripts/main.lua",
                "Pal/Config/Mod-config.ini",
                "Pal/Content/Paks/LogicMods/Team-Mod/Big.pak",
            ]
        );
        assert!(p.untracked.contains(&(String::new(), "Pal/Config/Mod-config.ini".to_string())));
        let d = package("ts-shim-dll", &[("mod/dlls/main.dll", b"dll")]);
        assert_eq!(plan(&d, "Team-Dll", &game, "palworld"), [format!("{bin}/Mods/Team-Dll/dlls/main.dll"), format!("{bin}/Mods/Team-Dll/enabled.txt")]);
        // Paquet rangé comme le dossier du jeu : tel quel.
        let manual = package("ts-shim-manual", &[("Pal/Binaries/Win64/Mods/BasesPlus/Scripts/main.lua", b"lua"), ("manifest.json", b"{}")]);
        assert_eq!(plan(&manual, "PalModders-BasesPlus", &game, "palworld"), ["Pal/Binaries/Win64/Mods/BasesPlus/Scripts/main.lua"]);
        // UE4SS 3 déjà installé à part (dossier ue4ss) : ses mods y vont.
        fs::create_dir_all(game.join(bin).join("ue4ss")).unwrap();
        fs::write(game.join(bin).join("ue4ss/UE4SS.dll"), b"x").unwrap();
        assert_eq!(plan(&d, "Team-Dll", &game, "palworld")[0], format!("{bin}/ue4ss/Mods/Team-Dll/dlls/main.dll"));
    }

    #[test]
    fn other_loaders_put_each_file_where_the_game_reads_it() {
        // Balatro (Lovely) : la DLL à côté du jeu, les mods dans %AppData%/Balatro/Mods.
        let balatro = tempdir("ts-balatro");
        fs::write(balatro.join("Balatro.exe"), b"exe").unwrap();
        let extra = BTreeMap::from([("mods".to_string(), tempdir("ts-balatro-mods"))]);
        let l = package("ts-lovely", &[("winmm.dll", b"dll"), ("lovely/config.toml", b"cfg"), ("manifest.json", b"{}")]);
        assert_eq!(plan_in(&l, "Thunderstore-lovely", &balatro, "balatro", &extra).dests(), ["mods:lovely/config.toml", "winmm.dll"]);
        let m = package("ts-smods", &[("src/core.lua", b"lua"), ("manifest.json", b"{}")]);
        assert_eq!(
            plan_in(&m, "Steamodded-Steamodded", &balatro, "balatro", &extra).dests(),
            ["mods:Steamodded-Steamodded/manifest.json", "mods:Steamodded-Steamodded/src/core.lua"]
        );

        // WEBFISHING (GDWeave) : le dossier du mod (son manifest.json) dans GDWeave/mods.
        let wf = tempdir("ts-webfishing");
        let g = package("ts-gdweave", &[("winmm.dll", b"dll"), ("GDWeave/core/GDWeave.dll", b"core"), ("manifest.json", b"{}")]);
        assert_eq!(plan(&g, "NotNet-GDWeave", &wf, "webfishing"), ["GDWeave/core/GDWeave.dll", "winmm.dll"]);
        let t = package("ts-tackle", &[("GDWeave/mods/TackleBox/manifest.json", b"{}"), ("GDWeave/mods/TackleBox/TackleBox.dll", b"dll"), ("manifest.json", b"{}")]);
        assert_eq!(
            plan(&t, "PuppyGirl-TackleBox", &wf, "webfishing"),
            ["GDWeave/mods/PuppyGirl-TackleBox/TackleBox.dll", "GDWeave/mods/PuppyGirl-TackleBox/manifest.json"]
        );

        // Hades II (Return of Modding) : à côté de Hades2.exe, dans Ship.
        let hades = tempdir("ts-hades");
        fs::create_dir_all(hades.join("Ship")).unwrap();
        fs::write(hades.join("Ship/Hades2.exe"), b"exe").unwrap();
        let h = package("ts-h2m", &[("ReturnOfModdingPack/d3d12.dll", b"dll"), ("manifest.json", b"{}")]);
        assert_eq!(plan(&h, "Hell2Modding-Hell2Modding", &hades, "hades-ii"), ["Ship/d3d12.dll"]);
        let u = package("ts-modutil", &[("main.lua", b"lua"), ("class/item.lua", b"lua"), ("config/ModUtil.cfg", b"cfg")]);
        let p = plan_in(&u, "SGG_Modding-ModUtil", &hades, "hades-ii", &BTreeMap::new());
        assert_eq!(
            p.dests(),
            [
                "Ship/ReturnOfModding/config/SGG_Modding-ModUtil/ModUtil.cfg",
                "Ship/ReturnOfModding/plugins/SGG_Modding-ModUtil/class/item.lua",
                "Ship/ReturnOfModding/plugins/SGG_Modding-ModUtil/main.lua",
            ]
        );
        assert!(p.untracked.iter().any(|(_, d)| d.ends_with("ModUtil.cfg")));

        // Scrap Mechanic (Rivet) : dans Release, à côté de ScrapMechanic.exe.
        let sm = tempdir("ts-scrap");
        fs::create_dir_all(sm.join("Release")).unwrap();
        fs::write(sm.join("Release/ScrapMechanic.exe"), b"exe").unwrap();
        let r = package("ts-rivet", &[("RivetPack/version.dll", b"dll"), ("RivetPack/Rivet.ini", b""), ("RivetPack/Rivet/Loader.dll", b"loader")]);
        assert_eq!(plan(&r, "ReDoIngMods-Rivet", &sm, "scrap-mechanic"), ["Release/Rivet.ini", "Release/Rivet/Loader.dll", "Release/version.dll"]);
        let rm = package("ts-rivet-mod", &[("lua_hot_reload.dll", b"dll")]);
        assert_eq!(plan(&rm, "BenMcAvoy-Lua_Hot_Reload", &sm, "scrap-mechanic"), ["Release/Rivet/Mods/BenMcAvoy-Lua_Hot_Reload/lua_hot_reload.dll"]);

        // Broforce (Unity Mod Manager) : UMM et son doorstop, puis UMM/Mods.
        let bro = tempdir("ts-bro");
        let umm = package(
            "ts-umm",
            &[("UMM/Core/UnityModManager.dll", b"umm"), ("doorstop_config.ini", b"ini"), ("winhttp.dll", b"dll"), ("CHANGELOG.md", b"c"), ("manifest.json", b"{}")],
        );
        assert_eq!(plan(&umm, "UMM-UMM", &bro, "broforce"), ["UMM/Core/UnityModManager.dll", "doorstop_config.ini", "winhttp.dll"]);
        let rl = package("ts-rocketlib", &[("UMM/Mods/RocketLib/RocketLib.dll", b"dll"), ("UMM/Mods/RocketLib/Info.json", b"{}")]);
        assert_eq!(
            plan(&rl, "RocketLib-RocketLib", &bro, "broforce"),
            ["UMM/Mods/RocketLib-RocketLib/RocketLib/Info.json", "UMM/Mods/RocketLib-RocketLib/RocketLib/RocketLib.dll"]
        );

        // Brotato (Godot Mod Loader) : addons, puis chaque mod zippé dans mods.
        let brotato = tempdir("ts-brotato");
        let gml = package("ts-gml", &[("addons/mod_loader/mod_loader.gd", b"gd"), ("addons/JSON_Schema_Validator/v.gd", b"gd"), ("manifest.json", b"{}")]);
        assert_eq!(
            plan(&gml, "GodotModding-GodotModLoader", &brotato, "brotato"),
            ["addons/JSON_Schema_Validator/v.gd", "addons/mod_loader/mod_loader.gd"]
        );
        let bm = package("ts-bml", &[("mods-unpacked/otDan-BetterModList/mod_main.gd", b"gd")]);
        assert_eq!(plan(&bm, "otDan-BetterModList", &brotato, "brotato"), ["mods/otDan-BetterModList.ts.zip"]);

        // Ancient Dungeon VR (pas de chargeur déclaré) : chaque mod dans Mods.
        let advr = tempdir("ts-advr");
        let a = package("ts-advr-mod", &[("Mods/ItemRarityColors.dll", b"dll")]);
        assert_eq!(plan(&a, "GenericMods-ItemRarityColors", &advr, "ancient-dungeon-vr"), ["Mods/GenericMods-ItemRarityColors/Mods/ItemRarityColors.dll"]);

        // Resonite (BepisLoader) : son lanceur Linux remplace celui du jeu, gardé de côté.
        let reso = tempdir("ts-reso");
        fs::write(reso.join("LinuxBootstrap.sh"), b"original").unwrap();
        let b = package(
            "ts-bepis",
            &[("BepInExPack/hookfxr.ini", b"[hookfxr]\nenable=false\n"), ("BepInExPack/LinuxBootstrap.sh", b"bepis"), ("BepInExPack/BepisLoader.dll", b"dll")],
        );
        let p = plan_in(&b, "ResoniteModding-BepisLoader", &reso, "resonite", &BTreeMap::new());
        assert_eq!(p.dests(), ["BepisLoader.dll", "LinuxBootstrap.sh", "hookfxr.ini"]);
        assert_eq!(p.replaced, ["LinuxBootstrap.sh"]);
    }

    #[test]
    fn ini_settings_are_set_without_losing_the_rest() {
        let text = "[hookfxr]\n# commentaire\nenable=false\ntarget_assembly=BepisLoader.dll\n";
        assert_eq!(
            set_ini(text, "hookfxr", "enable", "true"),
            "[hookfxr]\n# commentaire\nenable=true\ntarget_assembly=BepisLoader.dll\n"
        );
        let rivet = set_ini(&set_ini("", "Doorstop", "enable", "true"), "Loader", "directory", "Rivet/Mods");
        assert_eq!(rivet, "[Doorstop]\nenable=true\n\n[Loader]\ndirectory=Rivet/Mods\n");
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
        let s = ThunderstoreState::new(base.trim_end_matches("/v1"), tempdir(tag).join("installed-thunderstore.json"));
        // Les règles des jeux livrées avec ModStudio (le faux serveur ne les sert pas).
        *s.ecosystem.try_lock().unwrap() = Some(Arc::new(Ecosystem::bundled()));
        s
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
        assert!(bepinex_flavor(&game).is_some());
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
        let l = s.loader(out.game.game_id).await.unwrap();
        assert!(l.suitable && l.needed.is_none());
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
                // Réparation.
                latest("6.0.788", &[]),
                zip_response(&il2cpp_pack()),
            ]
        });
        let s = state_for(&server.base, "ts-wrong-state");
        let g = s.configure("roadside-research", "Roadside Research".into(), None, game.to_str().unwrap()).unwrap();
        s.install("roadside-research", "BepInEx", "BepInExPack", None, &[], quiet()).await.unwrap();
        {
            // Comme avant la correction : HighJump comptait sur BepInEx 5.
            let mut lib = s.library.lock().unwrap();
            let gm = lib.games.get_mut(&g.game_id).unwrap();
            let m = gm.mods.get_mut(&stable_id("BepInEx-BepInExPack")).unwrap();
            m.auto = true;
            m.required_by = vec![stable_id("Deja-HighJump")];
            let dir = game.join("BepInEx/plugins/Deja-HighJump");
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("HighJump.dll"), b"jump").unwrap();
            gm.mods.insert(
                stable_id("Deja-HighJump"),
                InstalledMod {
                    mod_id: stable_id("Deja-HighJump"),
                    name: "HighJump".into(),
                    files: vec!["BepInEx/plugins/Deja-HighJump/HighJump.dll".into()],
                    file_name: Some("Deja-HighJump".into()),
                    ..Default::default()
                },
            );
        }
        let l = s.loader(g.game_id).await.unwrap();
        assert_eq!(l.name, Some("BepInEx 5"));
        assert_eq!(l.required_label.as_deref(), Some("BepInEx 6 (IL2CPP)"));
        assert!(!l.suitable && l.managed && l.supported);
        let fix = l.needed.unwrap();
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
        assert!(s.loader(g.game_id).await.unwrap().suitable);
    }

    #[tokio::test]
    async fn installing_any_mod_brings_the_loader_the_game_needs() {
        // Lethal Company sans BepInEx, mod qui ne déclare rien : BepInEx d'abord.
        let game = tempdir("ts-auto");
        let server = fake_server_with(|_| {
            vec![
                latest("1.0.0", &[]),
                latest("5.4.2305", &[]),
                zip_response(&mono_pack()),
                zip_response(&make_zip(&[("Cool.dll", b"cool")])),
            ]
        });
        let s = state_for(&server.base, "ts-auto-state");
        s.configure("lethal-company", "Lethal Company".into(), None, game.to_str().unwrap()).unwrap();
        let out = s.install("lethal-company", "Team", "Cool", None, &[], quiet()).await.unwrap();
        assert_eq!(out.installed, ["BepInExPack", "Cool"]);
        assert_eq!(server.request(1).line, "GET /api/experimental/package/BepInEx/BepInExPack/ HTTP/1.1");
        let pack = &out.game.mods[&stable_id("BepInEx-BepInExPack")];
        assert!(pack.auto);
        assert_eq!(pack.required_by, [stable_id("Team-Cool")]);
        assert!(game.join("BepInEx/plugins/Team-Cool/Cool.dll").is_file());
    }

    #[tokio::test]
    async fn games_with_unknown_loaders_are_refused_before_any_download() {
        let game = tempdir("ts-mio");
        let server = fake_server_with(|_| vec![]);
        let s = state_for(&server.base, "ts-mio-state");
        let g = s.configure("mio-memories-in-orbit", "MIO".into(), None, game.to_str().unwrap()).unwrap();
        let err = s.install("mio-memories-in-orbit", "Team", "Mod", None, &[], quiet()).await.unwrap_err();
        assert!(matches!(err, AppError::ThunderstoreUnsupported(ref l) if l.contains("Mio")), "{err:?}");
        assert_eq!(server.count(), 0);
        let l = s.loader(g.game_id).await.unwrap();
        assert!(!l.supported && l.needed.is_none());
        assert_eq!(l.required.as_deref(), Some("mio"));
    }

    #[tokio::test]
    async fn resonite_loader_is_turned_on_and_its_launcher_put_back_on_removal() {
        let game = tempdir("ts-resonite");
        fs::write(game.join("Resonite.exe"), b"exe").unwrap();
        fs::write(game.join("LinuxBootstrap.sh"), b"original").unwrap();
        let pack = make_zip(&[
            ("BepInExPack/hookfxr.ini", b"[hookfxr]\nenable=false\ntarget_assembly=BepisLoader.dll\n"),
            ("BepInExPack/hostfxr.dll", b"hook"),
            ("BepInExPack/BepisLoader.dll", b"loader"),
            ("BepInExPack/LinuxBootstrap.sh", b"bepis"),
            ("BepInExPack/BepInEx/core/BepInEx.NET.CoreCLR.dll", b"core"),
        ]);
        let server = fake_server_with(|_| vec![latest("1.7.0", &[]), zip_response(&pack)]);
        let s = state_for(&server.base, "ts-resonite-state");
        let g = s.configure("resonite", "Resonite".into(), None, game.to_str().unwrap()).unwrap();
        let l = s.loader(g.game_id).await.unwrap();
        assert!(l.supported && !l.suitable);
        assert_eq!(l.needed.as_ref().map(|n| n.name.as_str()), Some("BepisLoader"));

        let out = s.install("resonite", "ResoniteModding", "BepisLoader", None, &[], quiet()).await.unwrap();
        assert!(fs::read_to_string(game.join("hookfxr.ini")).unwrap().contains("enable=true"));
        assert_eq!(fs::read(game.join("LinuxBootstrap.sh")).unwrap(), b"bepis");
        assert_eq!(fs::read(game.join("LinuxBootstrap.sh.modstudio-orig")).unwrap(), b"original");
        assert!(s.loader(g.game_id).await.unwrap().suitable);

        s.remove(out.game.game_id, stable_id("ResoniteModding-BepisLoader")).unwrap();
        assert_eq!(fs::read(game.join("LinuxBootstrap.sh")).unwrap(), b"original");
        assert!(!game.join("LinuxBootstrap.sh.modstudio-orig").exists());
        assert!(!game.join("hostfxr.dll").exists());
    }

    #[tokio::test]
    async fn resonite_renderer_mods_get_bepinex_renderer_even_with_bepisloader_there() {
        let game = tempdir("ts-resonite-renderer");
        fs::write(game.join("Resonite.exe"), b"exe").unwrap();
        fs::write(game.join("BepisLoader.dll"), b"loader").unwrap();
        fs::write(game.join("hostfxr.dll"), b"hook").unwrap();
        fs::create_dir_all(game.join("Renderer")).unwrap();
        let renderer = make_zip(&[("BepInExPack/Renderer/BepInEx/core/BepInEx.Preloader.dll", b"bep5"), ("manifest.json", b"{}")]);
        let lib = make_zip(&[
            ("plugins/InterprocessLib.BepisLoader/InterprocessLib.FrooxEngine.dll", b"engine"),
            ("Renderer/BepInEx/plugins/InterprocessLib.BepInEx/InterprocessLib.Unity.dll", b"unity"),
            ("manifest.json", b"{}"),
        ]);
        let server = fake_server_with(move |_| {
            vec![
                latest("3.0.1", &["ResoniteModding-BepInExRenderer-5.4.233001", "ResoniteModding-BepisLoader-1.6.0"]),
                latest("5.4.233001", &[]),
                zip_response(&renderer),
                zip_response(&lib),
                latest("5.4.233001", &[]),
            ]
        });
        let s = state_for(&server.base, "ts-resonite-renderer-state");
        s.configure("resonite", "Resonite".into(), None, game.to_str().unwrap()).unwrap();
        let out = s.install("resonite", "Nytra", "InterprocessLib", None, &[], quiet()).await.unwrap();
        assert!(out.kept_loader, "BepisLoader, installé à la main, est gardé");
        assert!(game.join("Renderer/BepInEx/core/BepInEx.Preloader.dll").is_file());
        assert!(game.join("BepInEx/plugins/Nytra-InterprocessLib/InterprocessLib.BepisLoader/InterprocessLib.FrooxEngine.dll").is_file());
        assert!(game.join("Renderer/BepInEx/plugins/Nytra-InterprocessLib/InterprocessLib.BepInEx/InterprocessLib.Unity.dll").is_file());
        let config = &out.game;
        assert!(config.mods.get(&stable_id("ResoniteModding-BepInExRenderer")).is_some_and(|m| m.auto));
        // Le chargeur du jeu reste celui installé à la main : BepInExRenderer ne compte pas comme tel.
        assert!(!s.loader(config.game_id).await.unwrap().managed);

        // Déjà là : installé directement, il est refusé comme les autres chargeurs.
        s.remove(config.game_id, stable_id("Nytra-InterprocessLib")).unwrap();
        fs::create_dir_all(game.join("Renderer/BepInEx/core")).unwrap();
        fs::write(game.join("Renderer/BepInEx/core/BepInEx.Preloader.dll"), b"bep5").unwrap();
        let mut lib = s.library.lock().unwrap().clone();
        lib.games.get_mut(&config.game_id).unwrap().mods.remove(&stable_id("ResoniteModding-BepInExRenderer"));
        *s.library.lock().unwrap() = lib;
        let err = s.install("resonite", "ResoniteModding", "BepInExRenderer", None, &[], quiet()).await.unwrap_err();
        assert!(matches!(err, AppError::ToolPresent(ref l) if l == "BepInExRenderer"), "{err:?}");
    }

    #[tokio::test]
    async fn balatro_mods_go_to_its_mods_folder_with_lovely_first() {
        let game = tempdir("ts-balatro-game");
        fs::write(game.join("Balatro.exe"), b"exe").unwrap();
        let mods = tempdir("ts-balatro-appdata");
        let server = fake_server_with(|_| {
            vec![
                latest("1.0.0", &[]),
                latest("0.10.0", &[]),
                zip_response(&make_zip(&[("winmm.dll", b"lovely"), ("lovely/config.toml", b"cfg")])),
                zip_response(&make_zip(&[("core.lua", b"lua"), ("manifest.json", b"{}")])),
            ]
        });
        let s = state_for(&server.base, "ts-balatro-state");
        let g = s.configure("balatro", "Balatro".into(), None, game.to_str().unwrap()).unwrap();
        s.library.lock().unwrap().games.get_mut(&g.game_id).unwrap().extra_folders.insert("mods".into(), mods.clone());
        let out = s.install("balatro", "Steamodded", "Steamodded", None, &[], quiet()).await.unwrap();
        assert_eq!(out.installed, ["lovely", "Steamodded"]);
        assert!(game.join("winmm.dll").is_file());
        assert!(mods.join("lovely/config.toml").is_file());
        assert!(mods.join("Steamodded-Steamodded/core.lua").is_file());
        let m = &out.game.mods[&stable_id("Steamodded-Steamodded")];
        assert!(m.files.is_empty());
        assert_eq!(m.parts[0].key, "mods");
        assert!(m.parts[0].files.contains(&"Steamodded-Steamodded/core.lua".to_string()));
        s.remove(g.game_id, m.mod_id).unwrap();
        assert!(!mods.join("Steamodded-Steamodded").exists());
        assert!(mods.join("lovely/config.toml").is_file(), "réglages de Lovely gardés");
    }

    #[tokio::test]
    async fn never_removes_a_wrong_bepinex_installed_by_hand() {
        let game = il2cpp_game("ts-hand");
        fs::create_dir_all(game.join("BepInEx/core")).unwrap();
        fs::write(game.join("BepInEx/core/BepInEx.dll"), b"bep5").unwrap();
        let server = fake_server_with(|_| vec![latest("1.0.0", &["BepInEx-BepInExPack-5.4.2100"]), latest("6.0.788", &[])]);
        let s = state_for(&server.base, "ts-hand-state");
        let g = s.configure("roadside-research", "Roadside Research".into(), None, game.to_str().unwrap()).unwrap();
        let l = s.loader(g.game_id).await.unwrap();
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
