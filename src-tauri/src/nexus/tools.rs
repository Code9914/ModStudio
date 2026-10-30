//! Outils de mods installés par ModStudio dans le dossier d'un jeu :
//! UE4SS (Unreal), BepInEx et MelonLoader (Unity), depuis leurs versions
//! officielles sur GitHub (empreinte SHA-256 vérifiée quand GitHub la donne).
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

pub struct ToolDef {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub repo: &'static str,
    /// Version précise ; `None` : dernière version stable.
    pub tag: Option<&'static str>,
    /// Nom sous lequel `engine::present_tools` signale l'outil.
    pub present: &'static str,
}

pub const TOOLS: &[ToolDef] = &[
    ToolDef {
        id: "ue4ss",
        name: "UE4SS",
        description: "chargeur de mods Lua et Blueprint pour les jeux Unreal Engine",
        repo: "UE4SS-RE/RE-UE4SS",
        // Les versions « expérimentales » gèrent Unreal Engine 5.4 et plus récent.
        tag: Some("experimental-latest"),
        present: "ue4ss",
    },
    ToolDef {
        id: "bepinex5",
        name: "BepInEx 5",
        description: "chargeur de plugins pour les jeux Unity (Mono)",
        repo: "BepInEx/BepInEx",
        tag: None,
        present: "bepinex",
    },
    ToolDef {
        id: "bepinex6",
        name: "BepInEx 6 (IL2CPP)",
        description: "chargeur de plugins pour les jeux Unity compilés en IL2CPP",
        repo: "BepInEx/BepInEx",
        tag: Some("v6.0.0-pre.2"),
        present: "bepinex",
    },
    ToolDef {
        id: "melonloader",
        name: "MelonLoader",
        description: "chargeur de mods pour les jeux Unity",
        repo: "LavaGang/MelonLoader",
        tag: None,
        present: "melonloader",
    },
];

pub fn tool(id: &str) -> Option<&'static ToolDef> {
    TOOLS.iter().find(|t| t.id == id)
}

/// Fichier de la version à télécharger pour cette architecture.
pub fn asset_matches(id: &str, name: &str, arch: Arch) -> bool {
    let a = arch.as_str();
    match id {
        "ue4ss" => name.starts_with("UE4SS_v") && name.ends_with(".zip"),
        "bepinex5" => name.starts_with(&format!("BepInEx_win_{a}_")) && name.ends_with(".zip"),
        "bepinex6" => name.starts_with(&format!("BepInEx-Unity.IL2CPP-win-{a}-")) && name.ends_with(".zip"),
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
}

impl Hints {
    /// Ajoute un mod d'après les noms de ses mods nécessaires.
    pub fn add_mod<'a>(&mut self, requirements: impl IntoIterator<Item = &'a str>) {
        let names: Vec<String> = requirements.into_iter().map(str::to_ascii_lowercase).collect();
        let any = |keys: &[&str]| names.iter().any(|n| keys.iter().any(|k| n.contains(k)));
        self.mods += 1;
        self.ue4ss += any(&["ue4ss"]) as usize;
        self.bepinex += any(&["bepinex"]) as usize;
        self.melonloader += any(&["melonloader", "melon loader"]) as usize;
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

/// Les téléchargements doivent venir de GitHub.
pub fn github_url_allowed(url: &str, api_base: &str) -> bool {
    let Ok(u) = reqwest::Url::parse(url) else { return false };
    let Some(host) = u.host_str().map(str::to_ascii_lowercase) else { return false };
    let api_host = reqwest::Url::parse(api_base).ok().and_then(|a| a.host_str().map(str::to_ascii_lowercase));
    if api_host.as_deref() == Some(host.as_str()) && host == "127.0.0.1" {
        return true;
    }
    u.scheme() == "https" && (host == "github.com" || host.ends_with(".githubusercontent.com"))
}

fn sha256_file(path: &std::path::Path) -> std::io::Result<String> {
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
        let def = tool(tool_id).ok_or_else(|| AppError::InvalidArgument(format!("outil « {tool_id} »")))?;
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
            AppError::ToolMismatch(format!(
                "ModStudio ne reconnaît pas le jeu dans {} : choisis le dossier où le jeu est installé.",
                root.display()
            ))
        })?;
        let target = match (def.id, info.engine) {
            ("ue4ss", Engine::Unreal) => engine::unreal_win64(info.project.as_deref().unwrap_or_default()),
            ("bepinex5" | "melonloader", Engine::UnityMono | Engine::UnityIl2cpp) | ("bepinex6", Engine::UnityIl2cpp) => String::new(),
            _ => {
                return Err(AppError::ToolMismatch(format!("{} ne convient pas au moteur de ce jeu.", def.name)));
            }
        };
        if info.tools.iter().any(|t| t == def.present) {
            return Err(AppError::ToolPresent(def.name.to_string()));
        }
        // Un seul chargeur Unity à la fois.
        if let Some(other) = ["bepinex", "melonloader"].into_iter().find(|t| *t != def.present && info.tools.iter().any(|x| x == t)) {
            return Err(AppError::ToolMismatch(format!(
                "Ce jeu utilise déjà {} : installer aussi {} empêcherait ses mods de se charger.",
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

        // Version publiée sur GitHub.
        let path = match def.tag {
            Some(tag) => format!("/repos/{}/releases/tags/{tag}", def.repo),
            None => format!("/repos/{}/releases/latest", def.repo),
        };
        let release: Release = self
            .client
            .fetch_json(&format!("{}{path}", self.github_api))
            .await
            .map_err(|e| AppError::ToolUnavailable(format!("{} sur GitHub ({e})", def.name)))?;
        let asset = release
            .assets
            .iter()
            .find(|a| asset_matches(def.id, &a.name, info.arch))
            .ok_or_else(|| AppError::ToolUnavailable(format!("{} pour Windows {} (aucun fichier publié)", def.name, info.arch.as_str())))?;
        if !github_url_allowed(&asset.browser_download_url, &self.github_api) {
            return Err(AppError::DownloadRefused("lien de téléchargement inattendu".into()));
        }

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
                url: &asset.browser_download_url,
                token: None,
                dest: &archive_path,
                expected_size: asset.size,
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
        if let Some(expected) = asset.digest.as_deref().and_then(|d| d.strip_prefix("sha256:")) {
            let actual = sha256_file(&archive_path).map_err(installer::io_err)?;
            if !actual.eq_ignore_ascii_case(expected) {
                cleanup();
                return Err(AppError::Corrupted);
            }
        }

        if !root.is_dir() {
            return Err(AppError::Io(format!("le dossier du jeu n'existe plus : {}", root.display())));
        }
        let others: BTreeMap<String, String> = config
            .owned_files(-1)
            .into_iter()
            .filter(|(folder, _, _)| *folder == root)
            .map(|(_, rel, owner)| (rel, owner))
            .collect();
        let limit = installer::extraction_limit(asset.size, 0);
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
                old_files: Vec::new(),
                old_dirs: Vec::new(),
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
            version: Some(release.tag_name.clone()).filter(|t| !t.is_empty()),
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
            return Err(AppError::Io(format!("{n} fichier(s) n'ont pas pu être supprimés. Ferme le jeu puis réessaie.")));
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
        assert!(asset_matches("bepinex6", "BepInEx-Unity.IL2CPP-win-x86-6.0.0-pre.2.zip", Arch::X86));
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
        assert!(github_url_allowed("https://github.com/BepInEx/BepInEx/releases/download/v5/x.zip", GITHUB_API));
        assert!(github_url_allowed("https://release-assets.githubusercontent.com/x", GITHUB_API));
        assert!(!github_url_allowed("https://github.com.evil.example/x.zip", GITHUB_API));
        assert!(!github_url_allowed("http://github.com/x.zip", GITHUB_API));
    }

    #[tokio::test]
    async fn installs_and_removes_ue4ss() {
        let zip = make_zip(&[
            ("dwmapi.dll", b"proxy"),
            ("ue4ss/UE4SS.dll", b"core"),
            ("ue4ss/Mods/mods.txt", b"BPModLoaderMod : 1"),
        ]);
        let digest = {
            let mut h = Sha256::new();
            h.update(&zip);
            h.finalize().iter().map(|b| format!("{b:02x}")).collect::<String>()
        };
        let server = fake_server_with(|base| {
            let root = base.trim_end_matches("/v1").to_string();
            let release = format!(
                r#"{{"tag_name":"experimental-latest","assets":[
                    {{"name":"zDEV-UE4SS_v3.zip","browser_download_url":"{root}/dev.zip","size":1}},
                    {{"name":"UE4SS_v3.0.1-1185.zip","browser_download_url":"{root}/ue4ss.zip","size":{},"digest":"sha256:{digest}"}}]}}"#,
                zip.len()
            );
            let mut file = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/zip\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                zip.len()
            );
            file.extend(zip.iter().map(|&b| b as char));
            vec![json_response(200, &release), file]
        });
        let mut state = NexusState::new("http://127.0.0.1:9", Box::new(MemoryStore::default()), tempdir("tool-lib").join("n.json"));
        state.github_api = server.base.trim_end_matches("/v1").to_string();

        // Jeu Unreal configuré sur son dossier d'installation.
        let game = tempdir("tool-game");
        fs::create_dir_all(game.join("Pilgrim/Binaries/Win64")).unwrap();
        fs::create_dir_all(game.join("Pilgrim/Content/Paks")).unwrap();
        fs::write(game.join("Pilgrim/Binaries/Win64/Pilgrim-Win64-Shipping.exe"), b"").unwrap();
        let g = state
            .configure(6534, "pilgrim", "PILGRIM".into(), None, game.to_string_lossy().into_owned(), Layout::Flat, false, None)
            .unwrap();
        assert_eq!(g.rules.as_deref(), Some("unreal"));

        let record = state.install_tool(6534, "ue4ss", Arc::new(|_| {})).await.unwrap();
        assert_eq!(record.version.as_deref(), Some("experimental-latest"));
        assert_eq!(fs::read(game.join("Pilgrim/Binaries/Win64/dwmapi.dll")).unwrap(), b"proxy");
        assert!(game.join("Pilgrim/Binaries/Win64/ue4ss/UE4SS.dll").is_file());
        let req = server.request(0);
        assert!(req.line.starts_with("GET /repos/UE4SS-RE/RE-UE4SS/releases/tags/experimental-latest"), "{}", req.line);
        assert!(req.header("user-agent").is_some());
        assert_eq!(engine::detect(&game, "pilgrim").tools, vec!["ue4ss"]);

        // Déjà présent : pas de deuxième installation.
        assert!(matches!(state.install_tool(6534, "ue4ss", Arc::new(|_| {})).await, Err(AppError::ToolPresent(_))));
        // Mauvais outil pour ce moteur.
        assert!(matches!(state.install_tool(6534, "bepinex5", Arc::new(|_| {})).await, Err(AppError::ToolMismatch(_))));

        assert!(state.remove_tool(6534, "ue4ss").unwrap().is_none());
        assert!(!game.join("Pilgrim/Binaries/Win64/dwmapi.dll").exists());
        assert!(!game.join("Pilgrim/Binaries/Win64/ue4ss").exists());
        assert!(game.join("Pilgrim/Binaries/Win64/Pilgrim-Win64-Shipping.exe").exists(), "les fichiers du jeu restent");
    }
}
