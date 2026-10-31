//! Outils de mods installés par ModStudio dans le dossier d'un jeu :
//! UE4SS (Unreal), BepInEx et MelonLoader (Unity), depuis leurs versions
//! officielles sur GitHub (empreinte SHA-256 vérifiée quand GitHub la donne),
//! et BepInEx 6 IL2CPP depuis ses versions « Bleeding Edge » officielles
//! (`builds.bepinex.dev`), seules compatibles avec les jeux Unity récents.
//! Les script extenders des jeux Bethesda s'installent, eux, depuis leur page
//! Nexus Mods comme un mod (voir `engine_rules.rs`).

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Read;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::local::{detect_engine, tool_root};
use super::rules::GAME_DIR;
use super::NexusState;
use crate::archive;
use crate::error::{AppError, AppResult};
use crate::games::engine::{self, Arch, Engine};
use crate::installer::{self, Batch, DownloadRequest, InstallEvent, InstalledTool, STAGING_DIR};

pub const GITHUB_API: &str = "https://api.github.com";
pub const BEPINEX_BUILDS: &str = "https://builds.bepinex.dev/projects/bepinex_be";

/// D'où vient un outil.
pub enum Source {
    /// Version GitHub : `tag` précis, ou dernière version stable.
    GitHub { repo: &'static str, tag: Option<&'static str> },
    /// Versions « Bleeding Edge » de BepInEx 6 (builds.bepinex.dev).
    BepInExBleedingEdge,
}

pub struct ToolDef {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub description_en: &'static str,
    pub source: Source,
    /// Nom sous lequel `engine::present_tools` signale l'outil.
    pub present: &'static str,
}

/// Version d'un outil prête à télécharger.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolRelease {
    pub version: String,
    pub url: String,
    pub size: u64,
    pub sha256: Option<String>,
}

pub const TOOLS: &[ToolDef] = &[
    ToolDef {
        id: "ue4ss",
        name: "UE4SS",
        description: "chargeur de mods Lua et Blueprint pour les jeux Unreal Engine",
        description_en: "Lua and Blueprint mod loader for Unreal Engine games",
        // Les versions « expérimentales » gèrent Unreal Engine 5.4 et plus récent.
        source: Source::GitHub { repo: "UE4SS-RE/RE-UE4SS", tag: Some("experimental-latest") },
        present: "ue4ss",
    },
    ToolDef {
        id: "bepinex5",
        name: "BepInEx 5",
        description: "chargeur de plugins pour les jeux Unity (Mono)",
        description_en: "plugin loader for Unity games (Mono)",
        source: Source::GitHub { repo: "BepInEx/BepInEx", tag: None },
        present: "bepinex",
    },
    ToolDef {
        id: "bepinex6",
        name: "BepInEx 6 (IL2CPP)",
        description: "chargeur de plugins pour les jeux Unity compilés en IL2CPP",
        description_en: "plugin loader for Unity games compiled with IL2CPP",
        // La version 6.0.0-pre.2 de GitHub ne gère pas les versions récentes d'Unity.
        source: Source::BepInExBleedingEdge,
        present: "bepinex",
    },
    ToolDef {
        id: "melonloader",
        name: "MelonLoader",
        description: "chargeur de mods pour les jeux Unity",
        description_en: "mod loader for Unity games",
        source: Source::GitHub { repo: "LavaGang/MelonLoader", tag: None },
        present: "melonloader",
    },
];

impl ToolDef {
    /// Description dans la langue de l'interface.
    pub fn description(&self) -> &'static str {
        crate::i18n::pick(self.description, self.description_en)
    }
}

pub fn tool(id: &str) -> Option<&'static ToolDef> {
    TOOLS.iter().find(|t| t.id == id)
}

/// Fichier de la version à télécharger pour cette architecture.
pub fn asset_matches(id: &str, name: &str, arch: Arch) -> bool {
    let a = arch.as_str();
    match id {
        "ue4ss" => name.starts_with("UE4SS_v") && name.ends_with(".zip"),
        "bepinex5" => name.starts_with(&format!("BepInEx_win_{a}_")) && name.ends_with(".zip"),
        "melonloader" => name == format!("MelonLoader.{a}.zip"),
        _ => false,
    }
}

/// Outils demandés par les mods les plus populaires d'un jeu (d'après les
/// mods nécessaires indiqués sur leur page Nexus).
#[derive(Debug, Default, Clone)]
pub struct Hints {
    /// Mods examinés.
    pub mods: usize,
    pub ue4ss: usize,
    pub bepinex: usize,
    pub melonloader: usize,
    /// Versions « Bleeding Edge » de BepInEx demandées (numéro → nombre de mods).
    pub be_builds: BTreeMap<u32, usize>,
}

/// Numéro de version « Bleeding Edge » de BepInEx cité dans un texte
/// (`6.0.0-be.791`, `…/bepinex_be/791`).
pub fn be_build_in(text: &str) -> Option<u32> {
    let t = text.to_ascii_lowercase();
    for marker in ["-be.", " be.", "bepinex_be/", "be.", "be "] {
        let mut from = 0;
        while let Some(i) = t[from..].find(marker) {
            let start = from + i + marker.len();
            let digits: String = t[start..].chars().take_while(|c| c.is_ascii_digit()).collect();
            if (3..=5).contains(&digits.len()) {
                return digits.parse().ok();
            }
            from = start;
        }
    }
    None
}

impl Hints {
    /// Ajoute un mod d'après ses mods nécessaires (nom, lien).
    pub fn add_mod<'a>(&mut self, requirements: impl IntoIterator<Item = &'a str>) {
        let names: Vec<String> = requirements.into_iter().map(str::to_ascii_lowercase).collect();
        let any = |keys: &[&str]| names.iter().any(|n| keys.iter().any(|k| n.contains(k)));
        self.mods += 1;
        self.ue4ss += any(&["ue4ss"]) as usize;
        self.bepinex += any(&["bepinex"]) as usize;
        self.melonloader += any(&["melonloader", "melon loader"]) as usize;
        let build = names.iter().filter(|n| n.contains("bepinex")).find_map(|n| be_build_in(n));
        if let Some(b) = build {
            *self.be_builds.entry(b).or_default() += 1;
        }
    }

    /// Version « Bleeding Edge » de BepInEx la plus demandée par les mods du jeu.
    pub fn wanted_be_build(&self) -> Option<u32> {
        self.be_builds.iter().max_by_key(|(build, n)| (**n, **build)).map(|(b, _)| *b)
    }

    /// Nombre de mods qui demandent cet outil (`ue4ss`, `bepinex`, `melonloader`).
    pub fn count(&self, present: &str) -> usize {
        match present {
            "ue4ss" => self.ue4ss,
            "bepinex" => self.bepinex,
            "melonloader" => self.melonloader,
            _ => 0,
        }
    }
}

/// Outil conseillé pour un moteur (les script extenders sont à part).
pub fn recommend(engine: Engine, hints: &Hints) -> Option<&'static str> {
    match engine {
        Engine::Unreal => Some("ue4ss"),
        Engine::UnityMono => Some(if hints.melonloader > hints.bepinex { "melonloader" } else { "bepinex5" }),
        Engine::UnityIl2cpp => Some(if hints.bepinex > hints.melonloader { "bepinex6" } else { "melonloader" }),
        Engine::Bethesda | Engine::Unknown => None,
    }
}

#[derive(Debug, Deserialize)]
struct Release {
    tag_name: String,
    #[serde(default)]
    assets: Vec<Asset>,
}

#[derive(Debug, Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    digest: Option<String>,
}

/// Les téléchargements doivent venir de GitHub ou du site officiel des versions de BepInEx.
pub fn tool_url_allowed(url: &str, test_bases: &[&str]) -> bool {
    let Ok(u) = reqwest::Url::parse(url) else { return false };
    let Some(host) = u.host_str().map(str::to_ascii_lowercase) else { return false };
    // Serveur de test local.
    let local = test_bases
        .iter()
        .filter_map(|b| reqwest::Url::parse(b).ok().and_then(|a| a.host_str().map(str::to_ascii_lowercase)))
        .any(|h| h == host && h == "127.0.0.1");
    if local {
        return true;
    }
    u.scheme() == "https" && (host == "github.com" || host.ends_with(".githubusercontent.com") || host == "builds.bepinex.dev")
}

/// Versions BepInEx 6 IL2CPP de la page des versions « Bleeding Edge » :
/// (numéro, nom du fichier), la plus récente d'abord.
pub fn parse_be_builds(html: &str, arch: Arch) -> Vec<(u32, String)> {
    let prefix = format!("BepInEx-Unity.IL2CPP-win-{}-6.0.0-be.", arch.as_str());
    let mut out: Vec<(u32, String)> = Vec::new();
    let mut from = 0;
    while let Some(i) = html[from..].find(&prefix) {
        let start = from + i;
        let rest = &html[start + prefix.len()..];
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        let end = rest.find(".zip").map(|e| start + prefix.len() + e + 4);
        from = start + prefix.len();
        let (Ok(build), Some(end)) = (digits.parse::<u32>(), end) else { continue };
        let name = &html[start..end];
        // Nom de fichier simple uniquement (`+` du hash encodé en `%2B`).
        if name.len() < 160 && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '%' | '_' | '+')) && !out.iter().any(|(b, _)| *b == build) {
            out.push((build, name.to_string()));
        }
    }
    out.sort_by(|a, b| b.0.cmp(&a.0));
    out
}

pub(crate) fn sha256_file(path: &std::path::Path) -> std::io::Result<String> {
    let mut f = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

impl NexusState {
    /// Installe un outil de mods (UE4SS, BepInEx, MelonLoader) dans le dossier du jeu.
    pub async fn install_tool(
        &self,
        game_id: i64,
        tool_id: &str,
        emit: Arc<dyn Fn(InstallEvent) + Send + Sync>,
    ) -> AppResult<InstalledTool> {
        let def = tool(tool_id).ok_or_else(|| AppError::InvalidArgument(crate::tr!("outil « {tool_id} »", "tool \"{tool_id}\"")))?;
        let config = self
            .library
            .lock()
            .expect("library")
            .games
            .get(&game_id)
            .cloned()
            .ok_or(AppError::NoGameFolder)?;
        let (key, root) = tool_root(&config).ok_or_else(|| AppError::NeedGameDir(config.game_name.clone()))?;
        let domain = config.domain.clone().unwrap_or_default();
        let info = detect_engine(&root, &domain).ok_or_else(|| {
            AppError::ToolMismatch(crate::tr!(
                "ModStudio ne reconnaît pas le jeu dans {} : choisis le dossier où le jeu est installé.",
                "ModStudio doesn't recognize the game in {}: choose the folder where the game is installed.",
                root.display()
            ))
        })?;
        let target = match (def.id, info.engine) {
            ("ue4ss", Engine::Unreal) => engine::unreal_win64(info.project.as_deref().unwrap_or_default()),
            ("bepinex5" | "melonloader", Engine::UnityMono | Engine::UnityIl2cpp) | ("bepinex6", Engine::UnityIl2cpp) => String::new(),
            _ => {
                return Err(AppError::ToolMismatch(crate::tr!(
                    "{} ne convient pas au moteur de ce jeu.",
                    "{} isn't suited to this game's engine.",
                    def.name
                )));
            }
        };
        // Déjà installé par ModStudio : mise à jour (les anciens fichiers sont remplacés).
        let old = config.tools.get(def.id).cloned();
        if old.is_none() && info.tools.iter().any(|t| t == def.present) {
            return Err(AppError::ToolPresent(def.name.to_string()));
        }
        // Un seul chargeur Unity à la fois.
        if let Some(other) = ["bepinex", "melonloader"].into_iter().find(|t| *t != def.present && info.tools.iter().any(|x| x == t)) {
            return Err(AppError::ToolMismatch(crate::tr!(
                "Ce jeu utilise déjà {} : installer aussi {} empêcherait ses mods de se charger.",
                "This game already uses {}: also installing {} would stop its mods from loading.",
                if other == "bepinex" { "BepInEx" } else { "MelonLoader" },
                def.name
            )));
        }

        let job_key = -game_id;
        let cancel = Arc::new(AtomicBool::new(false));
        {
            let mut jobs = self.jobs.lock().expect("jobs");
            if jobs.contains_key(&job_key) {
                return Err(AppError::InstallInProgress);
            }
            jobs.insert(job_key, cancel.clone());
        }
        struct Guard<'a>(&'a NexusState, i64);
        impl Drop for Guard<'_> {
            fn drop(&mut self) {
                self.0.jobs.lock().expect("jobs").remove(&self.1);
            }
        }
        let _guard = Guard(self, job_key);
        emit(InstallEvent::Preparing);

        // Version à installer (celle demandée par les mods du jeu pour BepInEx 6).
        let wanted = if def.id == "bepinex6" { self.hints(&domain).await.wanted_be_build() } else { None };
        let release = self.tool_release(def, info.arch, wanted, true).await?;

        let staging = root.join(STAGING_DIR);
        let job_dir = staging.join(format!("tool-{}", def.id));
        let _ = fs::remove_dir_all(&job_dir);
        fs::create_dir_all(&job_dir).map_err(installer::io_err)?;
        let archive_path = job_dir.join("download.part");
        let cleanup = || {
            let _ = fs::remove_dir_all(&job_dir);
            let _ = fs::remove_dir(&staging);
        };
        let progress_emit = emit.clone();
        let progress = move |received: u64, total: u64| progress_emit(InstallEvent::Downloading { received, total });
        if let Err(e) = installer::download(
            self.client.downloads(),
            DownloadRequest {
                url: &release.url,
                token: None,
                dest: &archive_path,
                expected_size: release.size,
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
        if let Some(expected) = release.sha256.as_deref() {
            let actual = sha256_file(&archive_path).map_err(installer::io_err)?;
            if !actual.eq_ignore_ascii_case(expected) {
                cleanup();
                return Err(AppError::Corrupted);
            }
        }

        if !root.is_dir() {
            return Err(AppError::Io(crate::tr!(
                "le dossier du jeu n'existe plus : {}",
                "the game folder no longer exists: {}",
                root.display()
            )));
        }
        // Fichiers des mods et des autres outils : jamais écrasés.
        let mut without_this = config.clone();
        without_this.tools.remove(def.id);
        let others: BTreeMap<String, String> = without_this
            .owned_files(-1)
            .into_iter()
            .filter(|(folder, _, _)| *folder == root)
            .map(|(_, rel, owner)| (rel, owner))
            .collect();
        // Mise à jour de BepInEx 6 : les fichiers générés au premier lancement
        // (assemblies d'interop) correspondent à l'ancienne version.
        if old.is_some() && def.id == "bepinex6" {
            for generated in ["BepInEx/interop", "BepInEx/unity-libs", "BepInEx/cache"] {
                if let Some(dir) = installer::resolve_rel(&root, generated) {
                    let _ = fs::remove_dir_all(dir);
                }
            }
        }
        let limit = installer::extraction_limit(release.size.max(1024 * 1024), 0);
        let (old_files, old_dirs) = old.as_ref().map(|o| (o.files.clone(), o.dirs.clone())).unwrap_or_default();
        let (root_b, archive_b, job_b, emit_b, cancel_b, target_b) =
            (root.clone(), archive_path.clone(), job_dir.clone(), emit.clone(), cancel.clone(), target.clone());
        let placed = tokio::task::spawn_blocking(move || -> AppResult<installer::PlacedBatch> {
            let extracted = job_b.join("extracted");
            let progress = |done: u64, total: u64| emit_b(InstallEvent::Extracting { done, total });
            archive::extract(&archive_b, &extracted, limit, &cancel_b, &progress)?;
            emit_b(InstallEvent::Finishing);
            let moves = installer::walk_files(&extracted)?
                .into_iter()
                .map(|rel| {
                    let rel_s = installer::rel_string(&rel);
                    let dest = if target_b.is_empty() { rel_s } else { format!("{target_b}/{rel_s}") };
                    (extracted.join(&rel), dest)
                })
                .collect();
            let batch = Batch {
                key: String::new(),
                folder: root_b,
                moves,
                old_files,
                old_dirs,
                others,
            };
            Ok(installer::place_batches(vec![batch])?.pop().expect("un lot"))
        })
        .await
        .map_err(|e| AppError::Io(e.to_string()))
        .and_then(|r| r);
        cleanup();
        let placed = placed?;

        let record = InstalledTool {
            id: def.id.to_string(),
            name: def.name.to_string(),
            key: key.clone(),
            version: Some(release.version.clone()).filter(|t| !t.is_empty()),
            installed_at: now(),
            files: placed.files,
            dirs: placed.dirs,
        };
        let mut lib = self.library.lock().expect("library");
        if let Some(game) = lib.games.get_mut(&game_id) {
            if key == GAME_DIR {
                // BONELAB : dossier du jeu trouvé automatiquement, gardé.
                game.extra_folders.entry(GAME_DIR.to_string()).or_insert(root.clone());
            } else if game.rules.is_none() && matches!(info.engine, Engine::UnityMono | Engine::UnityIl2cpp) {
                // Chargeur Unity installé : les mods sont désormais rangés pour lui.
                game.rules = Some("unity".to_string());
            }
            game.tools.insert(def.id.to_string(), record.clone());
        }
        lib.save(&self.library_path)?;
        Ok(record)
    }

    /// Version d'un outil à télécharger (gardée en mémoire une heure).
    /// `wanted` : version « Bleeding Edge » précise de BepInEx 6, si elle existe.
    pub async fn tool_release(&self, def: &ToolDef, arch: Arch, wanted: Option<u32>, fresh: bool) -> AppResult<ToolRelease> {
        let key = format!("{}:{}:{wanted:?}", def.id, arch.as_str());
        if !fresh {
            if let Some((at, r)) = self.releases.lock().expect("releases").get(&key) {
                if now() - at < 3600 {
                    return Ok(r.clone());
                }
            }
        }
        let release = match &def.source {
            Source::GitHub { repo, tag } => {
                let path = match tag {
                    Some(tag) => format!("/repos/{repo}/releases/tags/{tag}"),
                    None => format!("/repos/{repo}/releases/latest"),
                };
                let release: Release = self
                    .client
                    .fetch_json(&format!("{}{path}", self.github_api))
                    .await
                    .map_err(|e| AppError::ToolUnavailable(crate::tr!("{} sur GitHub ({e})", "{} from GitHub ({e})", def.name)))?;
                let asset = release
                    .assets
                    .iter()
                    .find(|a| asset_matches(def.id, &a.name, arch))
                    .ok_or_else(|| AppError::ToolUnavailable(crate::tr!(
                        "{} pour Windows {} (aucun fichier publié)",
                        "{} for Windows {} (no published file)",
                        def.name,
                        arch.as_str()
                    )))?;
                ToolRelease {
                    // La version « experimental-latest » d'UE4SS garde toujours le même nom : on prend celui du fichier.
                    version: if def.id == "ue4ss" { asset.name.trim_end_matches(".zip").to_string() } else { release.tag_name.clone() },
                    url: asset.browser_download_url.clone(),
                    size: asset.size,
                    sha256: asset.digest.as_deref().and_then(|d| d.strip_prefix("sha256:")).map(str::to_string),
                }
            }
            Source::BepInExBleedingEdge => {
                let html = self
                    .client
                    .fetch_bytes(&self.bepinex_builds)
                    .await
                    .map_err(|e| AppError::ToolUnavailable(crate::tr!("{} sur builds.bepinex.dev ({e})", "{} from builds.bepinex.dev ({e})", def.name)))?;
                let builds = parse_be_builds(&String::from_utf8_lossy(&html), arch);
                let (build, file) = wanted
                    .and_then(|w| builds.iter().find(|(b, _)| *b == w))
                    .or_else(|| builds.first())
                    .cloned()
                    .ok_or_else(|| AppError::ToolUnavailable(crate::tr!(
                        "{} pour Windows {} (aucune version trouvée)",
                        "{} for Windows {} (no version found)",
                        def.name,
                        arch.as_str()
                    )))?;
                ToolRelease {
                    version: format!("6.0.0-be.{build}"),
                    url: format!("{}/{build}/{file}", self.bepinex_builds.trim_end_matches('/')),
                    size: 0,
                    sha256: None,
                }
            }
        };
        if !tool_url_allowed(&release.url, &[&self.github_api, &self.bepinex_builds]) {
            return Err(AppError::DownloadRefused(crate::tr!("lien de téléchargement inattendu", "unexpected download link")));
        }
        self.releases.lock().expect("releases").insert(key, (now(), release.clone()));
        Ok(release)
    }

    /// Retire un outil installé par ModStudio. Renvoie le dossier laissé en
    /// place s'il contient des fichiers créés depuis (réglages, journaux…).
    pub fn remove_tool(&self, game_id: i64, tool_id: &str) -> AppResult<Option<String>> {
        let mut lib = self.library.lock().expect("library");
        let game = lib.games.get_mut(&game_id).ok_or(AppError::NotFound)?;
        let record = game.tools.get(tool_id).cloned().ok_or(AppError::NotFound)?;
        let folder = game.folder_for(&record.key).ok_or(AppError::NotFound)?.to_path_buf();
        let removal = installer::uninstall_files(&folder, &record.files, &record.dirs);
        if !removal.remaining.is_empty() {
            let n = removal.remaining.len();
            if let Some(t) = game.tools.get_mut(tool_id) {
                t.files = removal.remaining;
            }
            lib.save(&self.library_path)?;
            return Err(AppError::Io(crate::tr!(
                "{n} fichier(s) n'ont pas pu être supprimés. Ferme le jeu puis réessaie.",
                "{n} file(s) couldn't be deleted. Close the game and try again."
            )));
        }
        game.tools.remove(tool_id);
        lib.save(&self.library_path)?;
        Ok(removal.left_folder)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::MemoryStore;
    use crate::installer::tests::{make_zip, tempdir};
    use crate::installer::Layout;
    use crate::testutil::{fake_server_with, json_response};

    #[test]
    fn picks_the_right_files() {
        assert!(asset_matches("ue4ss", "UE4SS_v3.0.1-1185-gdff435dd.zip", Arch::X64));
        assert!(!asset_matches("ue4ss", "zDEV-UE4SS_v3.0.1-1185-gdff435dd.zip", Arch::X64));
        assert!(asset_matches("bepinex5", "BepInEx_win_x64_5.4.23.6.zip", Arch::X64));
        assert!(!asset_matches("bepinex5", "BepInEx_win_x86_5.4.23.6.zip", Arch::X64));
        assert!(!asset_matches("bepinex5", "BepInEx_linux_x64_5.4.23.6.zip", Arch::X64));
        assert!(asset_matches("melonloader", "MelonLoader.x64.zip", Arch::X64));
        assert!(!asset_matches("melonloader", "MelonLoader.Installer.exe", Arch::X64));
    }

    #[test]
    fn recommends_a_tool_per_engine() {
        let none = Hints::default();
        assert_eq!(recommend(Engine::Unreal, &none), Some("ue4ss"));
        assert_eq!(recommend(Engine::UnityMono, &none), Some("bepinex5"));
        assert_eq!(recommend(Engine::UnityIl2cpp, &none), Some("melonloader"));
        let mut melon = Hints::default();
        melon.add_mod(["MelonLoader", "MelonLoader 0.6"]);
        melon.add_mod(["Melon Loader"]);
        melon.add_mod(["BepInEx"]);
        assert_eq!((melon.mods, melon.melonloader, melon.bepinex), (3, 2, 1));
        assert_eq!(recommend(Engine::UnityMono, &melon), Some("melonloader"));
        let mut bep = Hints::default();
        bep.add_mod(["BepInEx 6 IL2CPP", "BepInExPack"]);
        assert_eq!(bep.count("bepinex"), 1);
        assert_eq!(recommend(Engine::UnityIl2cpp, &bep), Some("bepinex6"));
        assert_eq!(recommend(Engine::Bethesda, &none), None);
        assert!(tool_url_allowed("https://github.com/BepInEx/BepInEx/releases/download/v5/x.zip", &[GITHUB_API]));
        assert!(tool_url_allowed("https://release-assets.githubusercontent.com/x", &[GITHUB_API]));
        assert!(tool_url_allowed("https://builds.bepinex.dev/projects/bepinex_be/791/x.zip", &[GITHUB_API]));
        assert!(!tool_url_allowed("https://github.com.evil.example/x.zip", &[GITHUB_API]));
        assert!(!tool_url_allowed("http://github.com/x.zip", &[GITHUB_API]));
        assert!(!tool_url_allowed("http://127.0.0.1:9/x.zip", &[GITHUB_API]));
    }

    #[test]
    fn reads_bepinex_bleeding_edge_builds() {
        let html = r#"<a href="/projects/bepinex_be/791/BepInEx-Unity.IL2CPP-win-x64-6.0.0-be.791%2Bb782da4.zip">x</a>
            <a href="/projects/bepinex_be/791/BepInEx-Unity.IL2CPP-win-x86-6.0.0-be.791%2Bb782da4.zip">x</a>
            <a href="/projects/bepinex_be/790/BepInEx-Unity.IL2CPP-win-x64-6.0.0-be.790%2B4e98359.zip">x</a>
            <a href="/projects/bepinex_be/791/BepInEx-Unity.Mono-win-x64-6.0.0-be.791%2Bb782da4.zip">x</a>"#;
        let builds = parse_be_builds(html, Arch::X64);
        assert_eq!(
            builds,
            vec![
                (791, "BepInEx-Unity.IL2CPP-win-x64-6.0.0-be.791%2Bb782da4.zip".to_string()),
                (790, "BepInEx-Unity.IL2CPP-win-x64-6.0.0-be.790%2B4e98359.zip".to_string()),
            ]
        );
        assert_eq!(be_build_in("BepInEx 6.0.0-be.791, Unity.IL2CPP win-x64"), Some(791));
        assert_eq!(be_build_in("https://builds.bepinex.dev/projects/bepinex_be/791"), Some(791));
        assert_eq!(be_build_in("BepInEx 6 IL2CPP"), None);
        let mut hints = Hints::default();
        hints.add_mod(["MoonSweepers BepInEx Patcher", "BepInEx 6.0.0-be.791 (Unity.IL2CPP win-x64)"]);
        hints.add_mod(["BepInEx 6 IL2CPP + Unity 6000.6 fix https://builds.bepinex.dev/projects/bepinex_be/791"]);
        hints.add_mod(["BepInEx 6.0.0-be.735"]);
        assert_eq!(hints.wanted_be_build(), Some(791));
    }

    fn sha(data: &[u8]) -> String {
        let mut h = Sha256::new();
        h.update(data);
        h.finalize().iter().map(|b| format!("{b:02x}")).collect()
    }

    fn file_response(body: &[u8]) -> String {
        let mut file = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/zip\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        file.extend(body.iter().map(|&b| b as char));
        file
    }

    #[tokio::test]
    async fn installs_updates_and_removes_ue4ss() {
        let v1 = make_zip(&[
            ("dwmapi.dll", b"proxy"),
            ("ue4ss/UE4SS.dll", b"core"),
            ("ue4ss/Mods/mods.txt", b"BPModLoaderMod : 1"),
        ]);
        let v2 = make_zip(&[("dwmapi.dll", b"proxy 2"), ("ue4ss/UE4SS.dll", b"core 2"), ("ue4ss/UE4SS-settings.ini", b"[x]")]);
        let server = fake_server_with(|base| {
            let root = base.trim_end_matches("/v1").to_string();
            let release = |name: &str, zip: &[u8], file: &str| {
                format!(
                    r#"{{"tag_name":"experimental-latest","assets":[
                    {{"name":"zDEV-{name}.zip","browser_download_url":"{root}/dev.zip","size":1}},
                    {{"name":"{name}.zip","browser_download_url":"{root}/{file}","size":{},"digest":"sha256:{}"}}]}}"#,
                    zip.len(),
                    sha(zip)
                )
            };
            vec![
                json_response(200, &release("UE4SS_v3.0.1-1185", &v1, "v1.zip")),
                file_response(&v1),
                json_response(200, &release("UE4SS_v3.0.2-1200", &v2, "v2.zip")),
                file_response(&v2),
            ]
        });
        let mut state = NexusState::new("http://127.0.0.1:9", Box::new(MemoryStore::default()), tempdir("tool-lib").join("n.json"));
        state.github_api = server.base.trim_end_matches("/v1").to_string();

        // Jeu Unreal configuré sur son dossier d'installation.
        let game = tempdir("tool-game");
        let win64 = game.join("Pilgrim/Binaries/Win64");
        fs::create_dir_all(&win64).unwrap();
        fs::create_dir_all(game.join("Pilgrim/Content/Paks")).unwrap();
        fs::write(win64.join("Pilgrim-Win64-Shipping.exe"), b"").unwrap();
        let g = state
            .configure(6534, "pilgrim", "PILGRIM".into(), None, game.to_string_lossy().into_owned(), Layout::Flat, false, None)
            .unwrap();
        assert_eq!(g.rules.as_deref(), Some("unreal"));

        let record = state.install_tool(6534, "ue4ss", Arc::new(|_| {})).await.unwrap();
        assert_eq!(record.version.as_deref(), Some("UE4SS_v3.0.1-1185"));
        assert_eq!(fs::read(win64.join("dwmapi.dll")).unwrap(), b"proxy");
        assert!(win64.join("ue4ss/UE4SS.dll").is_file());
        let req = server.request(0);
        assert!(req.line.starts_with("GET /repos/UE4SS-RE/RE-UE4SS/releases/tags/experimental-latest"), "{}", req.line);
        assert!(req.header("user-agent").is_some());
        assert_eq!(engine::detect(&game, "pilgrim").tools, vec!["ue4ss"]);

        // Mise à jour : anciens fichiers remplacés, fichiers créés par UE4SS gardés.
        fs::write(win64.join("ue4ss/UE4SS.log"), b"journal").unwrap();
        let record = state.install_tool(6534, "ue4ss", Arc::new(|_| {})).await.unwrap();
        assert_eq!(record.version.as_deref(), Some("UE4SS_v3.0.2-1200"));
        assert_eq!(fs::read(win64.join("ue4ss/UE4SS.dll")).unwrap(), b"core 2");
        assert!(!win64.join("ue4ss/Mods/mods.txt").exists());
        assert!(win64.join("ue4ss/UE4SS.log").exists());

        // Mauvais outil pour ce moteur.
        assert!(matches!(state.install_tool(6534, "bepinex5", Arc::new(|_| {})).await, Err(AppError::ToolMismatch(_))));

        // Retrait : le dossier qui contient le journal reste.
        assert_eq!(state.remove_tool(6534, "ue4ss").unwrap().as_deref(), Some("Pilgrim/Binaries/Win64/ue4ss"));
        assert!(!win64.join("dwmapi.dll").exists());
        assert!(!win64.join("ue4ss/UE4SS.dll").exists());
        assert!(win64.join("Pilgrim-Win64-Shipping.exe").exists(), "les fichiers du jeu restent");
    }

    #[tokio::test]
    async fn bepinex_6_comes_from_the_bleeding_edge_builds() {
        let zip = make_zip(&[("winhttp.dll", b"doorstop"), ("BepInEx/core/BepInEx.Core.dll", b"core"), ("dotnet/coreclr.dll", b"clr")]);
        let server = fake_server_with(|base| {
            let html = r#"<a href="/projects/bepinex_be/792/BepInEx-Unity.IL2CPP-win-x64-6.0.0-be.792%2Bc0ffee1.zip">792</a>
                <a href="/projects/bepinex_be/791/BepInEx-Unity.IL2CPP-win-x64-6.0.0-be.791%2Bb782da4.zip">791</a>"#;
            let _ = base;
            vec![
                // Mods nécessaires des mods populaires : BepInEx be.791.
                json_response(
                    200,
                    r#"{"data":{"mods":{"nodes":[{"modRequirements":{"nexusRequirements":{"nodes":[{"modName":"BepInEx 6.0.0-be.791","url":"https://builds.bepinex.dev/projects/bepinex_be"}]}}}]}}}"#,
                ),
                format!("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{html}", html.len()),
                file_response(&zip),
            ]
        });
        let root = server.base.trim_end_matches("/v1").to_string();
        let mut state = NexusState::new(&root, Box::new(MemoryStore::default()), tempdir("be-lib").join("n.json"));
        state.bepinex_builds = format!("{root}/projects/bepinex_be");

        let game = tempdir("be-game");
        fs::write(game.join("UnityPlayer.dll"), b"").unwrap();
        fs::write(game.join("GameAssembly.dll"), b"").unwrap();
        fs::create_dir_all(game.join("MoonSweepers_Data")).unwrap();
        state
            .configure(10774, "moonsweepers", "Moon Sweepers".into(), None, game.to_string_lossy().into_owned(), Layout::Flat, false, None)
            .unwrap();
        let record = state.install_tool(10774, "bepinex6", Arc::new(|_| {})).await.unwrap();
        assert_eq!(record.version.as_deref(), Some("6.0.0-be.791"), "la version demandée par les mods du jeu");
        assert!(server.request(2).line.starts_with("GET /projects/bepinex_be/791/BepInEx-Unity.IL2CPP-win-x64-6.0.0-be.791%2Bb782da4.zip"));
        assert!(game.join("BepInEx/core/BepInEx.Core.dll").is_file());
        assert_eq!(state.library.lock().unwrap().games[&10774].rules.as_deref(), Some("unity"));
    }
}
