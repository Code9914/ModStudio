//! Commandes d'installation manuelle (voir `installer.rs` pour la mécanique).

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

use crate::api::Auth;
use crate::api::models::{Mod, Modfile, Page};
use crate::commands::AppState;
use crate::error::{AppError, AppResult};
use crate::installer::{
    self, DownloadRequest, GameInstall, InstallEvent, InstalledMod, Layout, PlaceInput, STAGING_DIR,
};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LatestFile {
    pub mod_id: i64,
    pub modfile_id: i64,
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UninstallResult {
    /// Dossier laissé en place parce qu'il contient des fichiers ajoutés depuis.
    pub left_folder: Option<String>,
}

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Retire le drapeau d'installation en cours, même en cas d'erreur.
struct JobGuard<'a> {
    state: &'a AppState,
    mod_id: i64,
}

impl Drop for JobGuard<'_> {
    fn drop(&mut self) {
        self.state.jobs.lock().expect("jobs").remove(&self.mod_id);
    }
}

impl AppState {
    fn save_library(&self, lib: &installer::Library) -> AppResult<()> {
        lib.save(&self.library_path)
    }
}

// ---------------------------------------------------------------- Dossiers

pub fn overview(state: &AppState) -> Vec<GameInstall> {
    let mut games: Vec<GameInstall> = state.library.lock().expect("library").games.values().cloned().collect();
    games.sort_by(|a, b| a.game_name.to_lowercase().cmp(&b.game_name.to_lowercase()));
    games
}

pub fn set_folder(
    state: &AppState,
    game_id: i64,
    game_name: String,
    icon: Option<String>,
    folder: String,
    layout: Layout,
) -> AppResult<GameInstall> {
    if game_id <= 0 {
        return Err(AppError::InvalidArgument(format!("jeu {game_id}")));
    }
    let folder = PathBuf::from(folder.trim());
    installer::check_folder(&folder)?;

    let mut lib = state.library.lock().expect("library");
    let entry = lib.games.entry(game_id).or_insert_with(|| GameInstall {
        game_id,
        game_name: game_name.clone(),
        icon: icon.clone(),
        folder: folder.clone(),
        layout,
        mods: BTreeMap::new(),
    });
    if entry.folder != folder && !entry.mods.is_empty() {
        let n = entry.mods.len();
        return Err(AppError::InvalidArgument(format!(
            "désinstalle d'abord {} de ce jeu pour changer de dossier.",
            if n > 1 { format!("les {n} mods") } else { "le mod".to_string() }
        )));
    }
    entry.folder = folder;
    entry.layout = layout;
    entry.game_name = game_name;
    if icon.is_some() {
        entry.icon = icon;
    }
    let result = entry.clone();
    state.save_library(&lib)?;
    Ok(result)
}

// ---------------------------------------------------------------- Installation

pub async fn install(
    state: &AppState,
    game_id: i64,
    mod_id: i64,
    emit: Arc<dyn Fn(InstallEvent) + Send + Sync>,
) -> AppResult<InstalledMod> {
    if game_id <= 0 || mod_id <= 0 {
        return Err(AppError::InvalidArgument(format!("identifiants {game_id}/{mod_id}")));
    }
    let (folder, layout, old, others) = {
        let lib = state.library.lock().expect("library");
        let game = lib.games.get(&game_id).ok_or(AppError::NoGameFolder)?;
        let others: BTreeMap<String, String> = game
            .mods
            .values()
            .filter(|m| m.mod_id != mod_id)
            .flat_map(|m| m.files.iter().map(move |f| (f.clone(), m.name.clone())))
            .collect();
        (game.folder.clone(), game.layout, game.mods.get(&mod_id).cloned(), others)
    };
    if !folder.is_dir() {
        return Err(AppError::Io(format!(
            "le dossier des mods n'existe plus : {}",
            folder.display()
        )));
    }

    // Une seule installation à la fois par mod.
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut jobs = state.jobs.lock().expect("jobs");
        if jobs.contains_key(&mod_id) {
            return Err(AppError::InstallInProgress);
        }
        jobs.insert(mod_id, cancel.clone());
    }
    let _guard = JobGuard { state, mod_id };

    emit(InstallEvent::Preparing);
    let cfg = state.api_config();
    let token = state.token().ok();

    // Le mod et son fichier actuel, avec un lien de téléchargement tout frais.
    let m: Mod = state
        .client
        .get(&cfg, &format!("/games/{game_id}/mods/{mod_id}"), &[])
        .await?;
    let current = m.modfile.as_ref().filter(|f| f.id > 0).ok_or(AppError::NoModfile)?;
    let auth = token.as_deref().map(Auth::Token).unwrap_or(Auth::ApiKey);
    let file: Modfile = state.check_session(
        state
            .client
            .get_as(&cfg, auth, &format!("/games/{game_id}/mods/{mod_id}/files/{}", current.id), &[])
            .await,
    )?;
    if file.virus_positive == 1 {
        return Err(AppError::VirusFlagged);
    }
    let url = file
        .download
        .as_ref()
        .and_then(|d| d.binary_url.clone())
        .ok_or_else(|| AppError::DownloadRefused("aucun lien de téléchargement fourni".into()))?;
    if !installer::download_url_allowed(&url, &cfg.api_path) {
        return Err(AppError::DownloadRefused("lien de téléchargement inattendu".into()));
    }

    // Téléchargement dans le dossier de travail.
    let staging = folder.join(STAGING_DIR);
    let job_dir = staging.join(format!("{mod_id}-{}", file.id));
    let _ = fs::remove_dir_all(&job_dir);
    fs::create_dir_all(&job_dir).map_err(installer::io_err)?;
    let archive = job_dir.join("download.part");
    let cleanup = || {
        let _ = fs::remove_dir_all(&job_dir);
        let _ = fs::remove_dir(&staging); // seulement s'il est vide
    };

    let progress_emit = emit.clone();
    let progress = move |received: u64, total: u64| progress_emit(InstallEvent::Downloading { received, total });
    let downloaded = installer::download(
        state.client.downloads(),
        DownloadRequest {
            url: &url,
            token: token.as_deref(),
            dest: &archive,
            expected_size: file.filesize.max(0) as u64,
            expected_md5: file.filehash.as_ref().and_then(|h| h.md5.as_deref()),
        },
        &cancel,
        &progress,
    )
    .await;
    if let Err(e) = state.check_session(downloaded) {
        cleanup();
        return Err(e);
    }
    emit(InstallEvent::Verifying);

    // Extraction et placement : travail disque, hors de la boucle asynchrone.
    let name_id = m.name_id.clone().unwrap_or_else(|| format!("mod-{mod_id}"));
    let filename = file.filename.clone().unwrap_or_else(|| format!("{name_id}.zip"));
    let limit = installer::extraction_limit(file.filesize.max(0) as u64, file.filesize_uncompressed.max(0) as u64);
    let blocking_emit = emit.clone();
    let blocking_cancel = cancel.clone();
    let (folder_b, archive_b, job_b, old_b) = (folder.clone(), archive.clone(), job_dir.clone(), old.clone());
    let placed = tokio::task::spawn_blocking(move || {
        let extracted = job_b.join("extracted");
        if layout != Layout::Archive {
            let progress = |done: u64, total: u64| blocking_emit(InstallEvent::Extracting { done, total });
            installer::extract_zip(&archive_b, &extracted, limit, &blocking_cancel, &progress)?;
        }
        if blocking_cancel.load(Ordering::Relaxed) {
            return Err(AppError::Cancelled);
        }
        blocking_emit(InstallEvent::Finishing);
        installer::place(PlaceInput {
            folder: &folder_b,
            layout,
            mod_id,
            name_id: &name_id,
            archive: &archive_b,
            filename: &filename,
            extracted: (layout != Layout::Archive).then_some(extracted.as_path()),
            old: old_b.as_ref(),
            others: &others,
        })
    })
    .await
    .map_err(|e| AppError::Io(e.to_string()))
    .and_then(|r| r);
    cleanup();
    let placed = placed?;

    let record = InstalledMod {
        mod_id,
        name: m.name.clone().unwrap_or_else(|| format!("Mod {mod_id}")),
        logo: m.logo.as_ref().and_then(|l| l.thumb_320x180.clone().or(l.original.clone())),
        version: file.version.clone().filter(|v| !v.trim().is_empty()),
        modfile_id: file.id,
        size: file.filesize,
        installed_at: now(),
        layout,
        root: placed.root,
        files: placed.files,
        dirs: placed.dirs,
    };
    let mut lib = state.library.lock().expect("library");
    if let Some(game) = lib.games.get_mut(&game_id) {
        game.mods.insert(mod_id, record.clone());
    }
    state.save_library(&lib)?;
    Ok(record)
}

pub fn cancel(state: &AppState, mod_id: i64) {
    if let Some(flag) = state.jobs.lock().expect("jobs").get(&mod_id) {
        flag.store(true, Ordering::Relaxed);
    }
}

pub fn remove(state: &AppState, game_id: i64, mod_id: i64) -> AppResult<UninstallResult> {
    if state.jobs.lock().expect("jobs").contains_key(&mod_id) {
        return Err(AppError::InstallInProgress);
    }
    let mut lib = state.library.lock().expect("library");
    let game = lib.games.get_mut(&game_id).ok_or(AppError::NotFound)?;
    let record = game.mods.get(&mod_id).cloned().ok_or(AppError::NotFound)?;
    let removal = installer::uninstall(&game.folder, &record);
    if !removal.remaining.is_empty() {
        // On garde la trace des fichiers restants pour pouvoir réessayer.
        let n = removal.remaining.len();
        let first = removal.remaining[0].clone();
        if let Some(r) = game.mods.get_mut(&mod_id) {
            r.files = removal.remaining;
        }
        state.save_library(&lib)?;
        return Err(AppError::Io(format!(
            "{n} fichier(s) n'ont pas pu être supprimés, dont {first}. Ferme le jeu puis réessaie."
        )));
    }
    game.mods.remove(&mod_id);
    state.save_library(&lib)?;
    Ok(UninstallResult {
        left_folder: removal.left_folder,
    })
}

/// Version actuelle sur mod.io des mods installés d'un jeu.
pub async fn latest(state: &AppState, game_id: i64) -> AppResult<Vec<LatestFile>> {
    let ids: Vec<i64> = state
        .library
        .lock()
        .expect("library")
        .games
        .get(&game_id)
        .map(|g| g.mods.keys().copied().collect())
        .unwrap_or_default();
    let cfg = state.api_config();
    let mut out = Vec::new();
    for chunk in ids.chunks(100) {
        let list = chunk.iter().map(i64::to_string).collect::<Vec<_>>().join(",");
        let page: Page<Mod> = state
            .client
            .get(
                &cfg,
                &format!("/games/{game_id}/mods"),
                &[("id-in", list), ("_limit", "100".into())],
            )
            .await?;
        out.extend(page.data.into_iter().filter_map(|m| {
            let f = m.modfile.filter(|f| f.id > 0)?;
            Some(LatestFile {
                mod_id: m.id,
                modfile_id: f.id,
                version: f.version,
            })
        }));
    }
    Ok(out)
}

fn reveal_path(state: &AppState, game_id: i64, mod_id: Option<i64>) -> AppResult<PathBuf> {
    let lib = state.library.lock().expect("library");
    let game = lib.games.get(&game_id).ok_or(AppError::NotFound)?;
    let Some(mod_id) = mod_id else { return Ok(game.folder.clone()) };
    let record = game.mods.get(&mod_id).ok_or(AppError::NotFound)?;
    let rel = record.root.as_ref().or(record.files.first()).ok_or(AppError::NotFound)?;
    Ok(installer::resolve_rel(&game.folder, rel).unwrap_or_else(|| game.folder.clone()))
}

// ---------------------------------------------------------------- Commandes Tauri

#[tauri::command]
pub fn install_overview(state: State<'_, AppState>) -> Vec<GameInstall> {
    overview(&state)
}

#[tauri::command]
pub fn set_game_folder(
    state: State<'_, AppState>,
    game_id: i64,
    game_name: String,
    icon: Option<String>,
    folder: String,
    layout: Layout,
) -> AppResult<GameInstall> {
    set_folder(&state, game_id, game_name, icon, folder, layout)
}

#[tauri::command]
pub async fn install_mod(
    state: State<'_, AppState>,
    game_id: i64,
    mod_id: i64,
    on_event: Channel<InstallEvent>,
) -> AppResult<InstalledMod> {
    let emit: Arc<dyn Fn(InstallEvent) + Send + Sync> = Arc::new(move |e| {
        let _ = on_event.send(e);
    });
    install(&state, game_id, mod_id, emit).await
}

#[tauri::command]
pub fn cancel_install(state: State<'_, AppState>, mod_id: i64) {
    cancel(&state, mod_id)
}

#[tauri::command]
pub fn uninstall_mod(state: State<'_, AppState>, game_id: i64, mod_id: i64) -> AppResult<UninstallResult> {
    remove(&state, game_id, mod_id)
}

#[tauri::command]
pub async fn latest_installed_files(state: State<'_, AppState>, game_id: i64) -> AppResult<Vec<LatestFile>> {
    latest(&state, game_id).await
}

#[tauri::command]
pub fn reveal_install(
    app: AppHandle,
    state: State<'_, AppState>,
    game_id: i64,
    mod_id: Option<i64>,
) -> AppResult<()> {
    let path = reveal_path(&state, game_id, mod_id)?;
    app.opener()
        .reveal_item_in_dir(&path)
        .map_err(|e| AppError::Io(e.to_string()))
}

// ---------------------------------------------------------------- Tests

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::MemoryStore;
    use crate::installer::tests::{make_zip, md5_hex, tempdir};
    use crate::settings::Settings;
    use crate::testutil::json_response;
    use std::sync::Mutex;

    fn state(base: &str, config_dir: &std::path::Path) -> AppState {
        let settings = Settings {
            api_key: "secretkey1234".into(),
            api_path: base.to_string(),
            hide_mature: false,
        };
        AppState::new(settings, config_dir.join("settings.json"), Box::new(MemoryStore::default()))
    }

    fn zip_response(body: &[u8]) -> String {
        let mut s = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/zip\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        s.extend(body.iter().map(|&b| b as char));
        s
    }

    /// Installe le mod 5 du jeu 3 depuis un faux mod.io qui répond : fiche du
    /// mod, fichier (avec un lien vers lui-même), puis l'archive.
    async fn run_install(zip: Vec<u8>, virus: i64, layout: Layout) -> (AppResult<InstalledMod>, std::path::PathBuf, Vec<String>) {
        let md5 = md5_hex(&zip);
        let mod_json = r#"{"id":5,"game_id":3,"name":"Clear Skies","name_id":"clear-skies","modfile":{"id":42,"version":"2.3"}}"#;
        let responses = |base: &str| {
            let file_json = format!(
                r#"{{"id":42,"mod_id":5,"filesize":{},"filename":"clear-skies.zip","version":"2.3","virus_positive":{virus},"filehash":{{"md5":"{md5}"}},"download":{{"binary_url":"{base}/dl","date_expires":9999999999}}}}"#,
                zip.len()
            );
            vec![json_response(200, mod_json), json_response(200, &file_json), zip_response(&zip)]
        };
        let server = crate::testutil::fake_server_with(responses);

        let config = tempdir("cfg");
        let game_folder = tempdir("mods");
        let s = state(&server.base, &config);
        set_folder(&s, 3, "Tidebreak".into(), None, game_folder.to_string_lossy().into_owned(), layout).unwrap();

        let events = Arc::new(Mutex::new(Vec::<String>::new()));
        let ev = events.clone();
        let emit: Arc<dyn Fn(InstallEvent) + Send + Sync> = Arc::new(move |e| {
            let tag = serde_json::to_value(&e).unwrap()["phase"].as_str().unwrap().to_string();
            let mut v = ev.lock().unwrap();
            if v.last() != Some(&tag) {
                v.push(tag);
            }
        });
        let result = install(&s, 3, 5, emit).await;
        let phases = events.lock().unwrap().clone();
        // Le manifeste est relu depuis le disque pour vérifier la sauvegarde.
        let reloaded = installer::Library::load(&config.join("installed.json"));
        if result.is_ok() {
            assert!(reloaded.games[&3].mods.contains_key(&5));
        }
        (result, game_folder, phases)
    }

    #[tokio::test]
    async fn full_install_then_uninstall() {
        let zip = make_zip(&[("ClearSkies/", b""), ("ClearSkies/sky.pak", b"ciel")]);
        let (result, folder, phases) = run_install(zip, 0, Layout::Subfolder).await;
        let rec = result.unwrap();
        assert_eq!(rec.modfile_id, 42);
        assert_eq!(rec.version.as_deref(), Some("2.3"));
        assert_eq!(rec.files, vec!["clear-skies/sky.pak"]);
        assert_eq!(fs::read(folder.join("clear-skies/sky.pak")).unwrap(), b"ciel");
        assert!(!folder.join(STAGING_DIR).exists(), "dossier de travail nettoyé");
        assert_eq!(phases, vec!["preparing", "downloading", "verifying", "extracting", "finishing"]);
    }

    #[tokio::test]
    async fn flagged_files_are_not_downloaded() {
        let zip = make_zip(&[("a.pak", b"x")]);
        let (result, folder, _) = run_install(zip, 1, Layout::Flat).await;
        assert!(matches!(result, Err(AppError::VirusFlagged)));
        assert!(fs::read_dir(&folder).unwrap().next().is_none());
    }

    #[tokio::test]
    async fn install_requires_a_folder_and_uninstall_cleans_up() {
        let config = tempdir("cfg2");
        let s = state("http://127.0.0.1:9/v1", &config);
        let emit: Arc<dyn Fn(InstallEvent) + Send + Sync> = Arc::new(|_| {});
        assert!(matches!(install(&s, 3, 5, emit).await, Err(AppError::NoGameFolder)));

        // Installation simulée, puis désinstallation.
        let folder = tempdir("mods2");
        set_folder(&s, 3, "Jeu".into(), None, folder.to_string_lossy().into_owned(), Layout::Flat).unwrap();
        fs::create_dir_all(folder.join("m")).unwrap();
        fs::write(folder.join("m/a.pak"), b"x").unwrap();
        s.library.lock().unwrap().games.get_mut(&3).unwrap().mods.insert(
            5,
            InstalledMod {
                mod_id: 5,
                name: "M".into(),
                logo: None,
                version: None,
                modfile_id: 1,
                size: 1,
                installed_at: 0,
                layout: Layout::Flat,
                root: None,
                files: vec!["m/a.pak".into()],
                dirs: vec!["m".into()],
            },
        );
        let r = remove(&s, 3, 5).unwrap();
        assert!(r.left_folder.is_none());
        assert!(!folder.join("m").exists());
        assert!(overview(&s)[0].mods.is_empty());

        // Changer de dossier est possible tant qu'aucun mod n'est installé.
        let other = tempdir("mods3");
        assert!(set_folder(&s, 3, "Jeu".into(), None, other.to_string_lossy().into_owned(), Layout::Subfolder).is_ok());
        assert!(set_folder(&s, 3, "Jeu".into(), None, "relatif/chemin".into(), Layout::Flat).is_err());
    }
}
