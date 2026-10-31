//! Installation des mods de ces listes : le chargeur du jeu s'il manque, les
//! mods demandés, puis le mod lui-même ; chaque fichier vérifié (quand la
//! liste publie son empreinte) et rangé là où le chargeur le lit.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use md5::{Digest, Md5};
use reqwest::{Client, StatusCode, Url};
use serde::Serialize;

use super::games::{self, safe_file_name, version_cmp, GameDef, Kind};
use super::{game_id, now, record_name, Artifact, Catalog, Hash, ListMod, ListsState, Place, ZipRoot};
use crate::archive;
use crate::error::{AppError, AppResult};
use crate::install_commands::{dependency_state, Orphan, UninstallResult};
use crate::installer::{self, GameInstall, InstallEvent, InstalledMod, Layout, Library, ModPart, STAGING_DIR};
use crate::nexus::install::{owned_by_others, place_plan};
use crate::nexus::tools::sha256_file;
use crate::thunderstore::install::ORIGINAL_SUFFIX;
use crate::toggle;

/// Règles des jeux de ces listes (dossier configuré = dossier du jeu).
pub const RULES: &str = "lists";
/// Au plus autant de mods installés d'un coup (dépendances comprises).
const MAX_MODS: usize = 80;

/// Résultat d'une installation.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListInstalled {
    pub game: GameInstall,
    /// Mods installés, dans l'ordre (chargeur et dépendances d'abord).
    pub installed: Vec<String>,
}

/// Nouvelle version d'un mod installé.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListUpdate {
    pub mod_id: i64,
    pub version: String,
}

/// Le chargeur de mods du jeu.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoaderState {
    pub name: &'static str,
    /// Dans le dossier du jeu.
    pub present: bool,
    /// Installé par ModStudio.
    pub managed: bool,
    pub version: Option<String>,
    /// ModStudio sait l'installer.
    pub installable: bool,
    /// Son identifiant dans la liste (pour l'installer tout de suite).
    pub id: Option<String>,
    pub latest: Option<String>,
    /// Ce qu'il faut savoir pour que les mods se chargent.
    pub note: Option<String>,
    /// Option de lancement Steam pour Proton (Linux).
    pub launch_option: Option<String>,
    pub game_version: Option<String>,
}

/// Un mod à installer.
#[derive(Debug, Clone)]
struct Step {
    id: String,
    version: String,
}

#[derive(Default)]
struct Plan {
    order: Vec<Step>,
    /// Mod → ceux qui en ont besoin.
    parents: HashMap<i64, Vec<i64>>,
}

/// La version installée répond-elle à la demande (`^1.2.0`, `>=1.0.0`, `1.0.1`, `*`) ?
fn satisfies(installed: Option<&str>, req: &str) -> bool {
    let Some(v) = installed else { return false };
    let req = req.trim();
    if req.is_empty() || req == "*" {
        return true;
    }
    match (semver::Version::parse(v.trim_start_matches('v')), semver::VersionReq::parse(req)) {
        (Ok(v), Ok(r)) => r.matches(&v),
        // Formats libres : une version au moins égale à celle demandée.
        _ => version_cmp(v, req.trim_start_matches(['^', '~', '>', '=', ' '])) != std::cmp::Ordering::Less,
    }
}

/// Fichiers sous `dir`, avec leur chemin relatif (séparateur `/`).
fn files_under(dir: &Path) -> Vec<(PathBuf, String)> {
    let mut out = Vec::new();
    let mut stack = vec![(dir.to_path_buf(), String::new())];
    while let Some((d, rel)) = stack.pop() {
        let Ok(entries) = fs::read_dir(&d) else { continue };
        let mut entries: Vec<_> = entries.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let name = e.file_name().to_string_lossy().into_owned();
            let r = if rel.is_empty() { name } else { format!("{rel}/{name}") };
            let p = e.path();
            if p.is_dir() {
                stack.push((p, r));
            } else if p.is_file() {
                out.push((p, r));
            }
        }
    }
    out.sort_by(|a, b| a.1.cmp(&b.1));
    out
}

/// Dossier (le moins profond) qui contient `file`.
fn dir_with(root: &Path, file: &str) -> Option<PathBuf> {
    files_under(root)
        .into_iter()
        .filter(|(_, r)| r.rsplit('/').next().is_some_and(|n| n.eq_ignore_ascii_case(file)))
        .min_by_key(|(_, r)| r.matches('/').count())
        .and_then(|(p, _)| p.parent().map(Path::to_path_buf))
}

fn join(dir: &str, rel: &str) -> String {
    if dir.is_empty() { rel.to_string() } else { format!("{}/{rel}", dir.trim_end_matches('/')) }
}

/// Nom de fichier donné par le serveur (`Content-Disposition`, sinon l'adresse finale).
fn server_name(response: &reqwest::Response) -> Option<String> {
    let from_header = response
        .headers()
        .get(reqwest::header::CONTENT_DISPOSITION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(';').find_map(|p| p.trim().strip_prefix("filename=")))
        .map(|n| n.trim_matches('"').to_string());
    let from_url = || response.url().path_segments()?.next_back().and_then(games::percent_decode);
    from_header.or_else(from_url).filter(|n| safe_file_name(n))
}

/// Télécharge vers `dest` ; renvoie le nom de fichier donné par le serveur.
async fn fetch(
    http: &Client,
    url: &str,
    dest: &Path,
    cancel: &AtomicBool,
    progress: &(dyn Fn(u64, u64) + Send + Sync),
) -> AppResult<Option<String>> {
    let result = fetch_inner(http, url, dest, cancel, progress).await;
    if result.is_err() {
        let _ = fs::remove_file(dest);
    }
    result
}

async fn fetch_inner(
    http: &Client,
    url: &str,
    dest: &Path,
    cancel: &AtomicBool,
    progress: &(dyn Fn(u64, u64) + Send + Sync),
) -> AppResult<Option<String>> {
    let mut response = http.get(url).send().await.map_err(|e| AppError::Network(e.without_url().to_string()))?;
    let status = response.status();
    // Une page web à la place du fichier (avertissement, connexion…).
    let html = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|t| t.starts_with("text/html"));
    if html && status != StatusCode::NOT_FOUND {
        return Err(AppError::DownloadRefused(
            crate::i18n::pick("le site a renvoyé une page web au lieu du fichier", "the site sent back a web page instead of the file").into(),
        ));
    }
    if !status.is_success() {
        return Err(match status {
            StatusCode::NOT_FOUND => AppError::NotFound,
            other => AppError::Api { status: other.as_u16(), message: other.canonical_reason().unwrap_or(crate::i18n::pick("erreur", "error")).into(), error_ref: None },
        });
    }
    let name = server_name(&response);
    let total = response.content_length().unwrap_or(0);
    let limit = if total == 0 { 4 * 1024 * 1024 * 1024 } else { total.saturating_mul(2).saturating_add(10 * 1024 * 1024) };
    if let Some(dir) = dest.parent() {
        fs::create_dir_all(dir).map_err(installer::io_err)?;
    }
    let mut file = File::create(dest).map_err(installer::io_err)?;
    let mut received: u64 = 0;
    let mut last = Instant::now();
    progress(0, total);
    while let Some(chunk) = response.chunk().await.map_err(|e| AppError::Network(e.without_url().to_string()))? {
        if cancel.load(Ordering::Relaxed) {
            return Err(AppError::Cancelled);
        }
        file.write_all(&chunk).map_err(installer::io_err)?;
        received += chunk.len() as u64;
        if received > limit {
            return Err(AppError::DownloadRefused(crate::i18n::pick("taille de fichier inattendue", "unexpected file size").into()));
        }
        if last.elapsed() >= Duration::from_millis(120) {
            progress(received, total);
            last = Instant::now();
        }
    }
    file.flush().map_err(installer::io_err)?;
    progress(received, total.max(received));
    Ok(name)
}

fn md5_file(path: &Path) -> std::io::Result<String> {
    let bytes = fs::read(path)?;
    Ok(Md5::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect())
}

/// Le fichier correspond-il à l'empreinte publiée ?
fn verify(path: &Path, hash: &Hash) -> AppResult<()> {
    let ok = match hash {
        Hash::None => true,
        Hash::Sha256(h) => sha256_file(path).map_err(installer::io_err)? == *h,
    };
    if ok { Ok(()) } else { Err(AppError::Corrupted) }
}

/// Échec d'un téléchargement, expliqué.
fn download_error(e: AppError, name: &str, url: &str) -> AppError {
    let host = Url::parse(url).ok().and_then(|u| u.host_str().map(str::to_string)).unwrap_or_default();
    let detail = match e {
        AppError::Network(d) => d,
        AppError::NotFound => crate::i18n::pick("fichier introuvable", "file not found").into(),
        AppError::Api { status, .. } => crate::tr!("erreur {status}", "error {status}"),
        AppError::DownloadRefused(d) => d,
        other => return other,
    };
    AppError::ListDownload { name: name.to_string(), host, detail }
}

/// Fichiers du jeu remplacés par le chargeur : renommés en `.modstudio-orig`.
fn keep_originals(root: &Path, files: &[String]) -> Vec<String> {
    let mut kept = Vec::new();
    for rel in files {
        let (path, orig) = (root.join(rel), root.join(format!("{rel}{ORIGINAL_SUFFIX}")));
        if path.is_file() && !orig.exists() && fs::rename(&path, &orig).is_ok() {
            kept.push(rel.clone());
        }
    }
    kept
}

/// Après un échec : ce qui a pris la place d'un original (forcément posé par
/// ModStudio, l'original ayant été renommé) est retiré, puis l'original remis.
fn undo_originals(root: &Path, files: &[String]) {
    for rel in files {
        let (path, orig) = (root.join(rel), root.join(format!("{rel}{ORIGINAL_SUFFIX}")));
        if orig.is_file() {
            if path.is_file() {
                let _ = fs::remove_file(&path);
            }
            let _ = fs::rename(&orig, &path);
        }
    }
}

fn restore_originals(root: &Path, files: &[String]) {
    for rel in files {
        let (path, orig) = (root.join(rel), root.join(format!("{rel}{ORIGINAL_SUFFIX}")));
        if orig.is_file() && !path.exists() {
            let _ = fs::rename(&orig, &path);
        }
    }
}

fn def_of(game_id_value: i64) -> Option<&'static GameDef> {
    games::GAMES.iter().find(|g| game_id(g.id) == game_id_value)
}

struct JobGuard<'a> {
    state: &'a ListsState,
    game_id: i64,
}

impl Drop for JobGuard<'_> {
    fn drop(&mut self) {
        self.state.jobs.lock().expect("jobs").remove(&self.game_id);
    }
}

/// Ce qu'il faut installer : les mods demandés manquants (ou trop vieux), le
/// chargeur pour tous les mods, puis le mod lui-même.
fn plan(game: &GameDef, catalog: &Catalog, config: &GameInstall, target: &ListMod, version: &str) -> AppResult<Plan> {
    let loader = catalog.loader().filter(|_| game.loader_installable());
    let mut plan = Plan::default();
    let mut visited: HashSet<String> = HashSet::from([target.id.to_lowercase()]);
    let mut stack: Vec<(String, String, bool)> = vec![(target.id.clone(), version.to_string(), false)];
    while let Some((id, ver, expanded)) = stack.pop() {
        if expanded {
            plan.order.push(Step { id, version: ver });
            continue;
        }
        let m = catalog.get(&id).ok_or_else(|| AppError::InvalidArgument(crate::tr!("« {id} » n'est plus dans la liste.", "\"{id}\" is no longer in the list.")))?;
        let v = m.version(&ver).unwrap_or_else(|| m.newest());
        let mut deps: Vec<(String, String)> = v.dependencies.iter().filter(|d| !d.optional).map(|d| (d.id.clone(), d.version.clone())).collect();
        if let Some(l) = loader
            && !m.loader
        {
            deps.insert(0, (l.id.clone(), String::new()));
        }
        let parent = m.mod_id;
        let mut children = Vec::new();
        for (dep, req) in deps {
            // Hors de la liste : rien à installer.
            let Some(d) = catalog.get(&dep) else { continue };
            let parents = plan.parents.entry(d.mod_id).or_default();
            if !parents.contains(&parent) {
                parents.push(parent);
            }
            if !visited.insert(d.id.to_lowercase()) {
                continue;
            }
            match config.mods.get(&d.mod_id) {
                Some(rec) if satisfies(rec.version.as_deref(), &req) => continue,
                // Le chargeur déjà là sans ModStudio : gardé tel quel.
                None if d.loader && game.loader_present(config) => continue,
                _ => {}
            }
            if visited.len() > MAX_MODS {
                return Err(AppError::InvalidArgument(crate::i18n::pick("ce mod demande beaucoup trop d'autres mods.", "this mod requires far too many other mods.").into()));
            }
            children.push((d.id.clone(), d.latest.clone()));
        }
        stack.push((id, ver, true));
        for (cid, cver) in children.into_iter().rev() {
            stack.push((cid, cver, false));
        }
    }
    Ok(plan)
}

fn conflicting(installed: Option<&str>, req: &str) -> bool {
    req.trim().is_empty() || req.trim() == "*" || satisfies(installed, req)
}

/// Mods connus pour ne pas marcher ensemble : refusés avant tout téléchargement.
fn check_conflicts(game: &GameDef, catalog: &Catalog, config: &GameInstall, plan: &Plan) -> AppResult<()> {
    let prefix = format!("{}:", game.id);
    let installed: Vec<(String, Option<String>, String)> = config
        .mods
        .values()
        .filter_map(|m| m.file_name.as_deref().and_then(|f| f.strip_prefix(&prefix)).map(|id| (id.to_string(), m.version.clone(), m.name.clone())))
        .collect();
    for step in &plan.order {
        let Some(m) = catalog.get(&step.id) else { continue };
        let Some(v) = m.version(&step.version) else { continue };
        let refuse = |name: &str| {
            AppError::InvalidArgument(crate::tr!(
                "« {} » ne marche pas avec « {name} », déjà installé : désinstalle-le d'abord.",
                "\"{}\" doesn't work with \"{name}\", which is already installed: uninstall it first.",
                m.name
            ))
        };
        for c in &v.conflicts {
            if let Some((_, ver, name)) = installed.iter().find(|(id, ..)| id.eq_ignore_ascii_case(&c.id))
                && conflicting(ver.as_deref(), &c.version)
            {
                return Err(refuse(name));
            }
        }
        for (id, ver, name) in &installed {
            let Some(other) = catalog.get(id).and_then(|o| ver.as_deref().and_then(|v| o.version(v))) else { continue };
            if other.conflicts.iter().any(|c| c.id.eq_ignore_ascii_case(&m.id) && conflicting(Some(&step.version), &c.version)) {
                return Err(refuse(name));
            }
        }
    }
    Ok(())
}

impl ListsState {
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

    /// Choisit le dossier du jeu.
    pub fn configure(&self, game: &GameDef, folder: &str) -> AppResult<GameInstall> {
        let path = PathBuf::from(folder);
        installer::check_folder(&path)?;
        if !path.is_dir() {
            return Err(AppError::InvalidArgument(crate::tr!("le dossier n'existe pas : {folder}", "the folder doesn't exist: {folder}")));
        }
        if !game.validate(&path) {
            return Err(AppError::InvalidArgument(crate::tr!(
                "ce dossier ne contient pas {} ({} introuvable). Choisis le dossier où se trouve son exécutable.",
                "this folder doesn't contain {} ({} not found). Choose the folder that has its executable.",
                game.name,
                game.exe[0]
            )));
        }
        let id = game_id(game.id);
        if self.jobs.lock().expect("jobs").contains_key(&id) {
            return Err(AppError::InstallInProgress);
        }
        let mut lib = self.library.lock().expect("library");
        if let Some(existing) = lib.games.get(&id)
            && !existing.mods.is_empty()
            && existing.folder != path
        {
            return Err(AppError::InvalidArgument(
                crate::i18n::pick("désinstalle d'abord les mods de ce jeu pour changer de dossier.", "uninstall this game's mods first to change folders.").into(),
            ));
        }
        let mods = lib.games.get(&id).map(|g| g.mods.clone()).unwrap_or_default();
        let config = GameInstall {
            game_id: id,
            game_name: game.name.to_string(),
            icon: Some(format!("https://cdn.cloudflare.steamstatic.com/steam/apps/{}/capsule_231x87.jpg", game.steam_app_id)),
            extra_folders: game.extra_folders(&path).into_iter().collect(),
            folder: path,
            layout: Layout::Flat,
            mods,
            rules: Some(RULES.into()),
            domain: Some(game.id.into()),
            ..Default::default()
        };
        lib.games.insert(id, config.clone());
        self.save_library(&lib)?;
        Ok(config)
    }

    /// Le chargeur de mods du jeu : présent ou non, et ce qu'il faut savoir.
    pub async fn loader_state(&self, game: &GameDef) -> AppResult<LoaderState> {
        let config = self.config(game).ok_or(AppError::NoGameFolder)?;
        let catalog = self.catalog(game, false).await.ok();
        let loader = catalog.as_ref().and_then(|c| c.loader().cloned());
        let record = loader.as_ref().and_then(|l| config.mods.get(&l.mod_id));
        let game_version = match game.kind {
            Kind::HollowKnight => games::hollow_knight::game_version(game, &config.folder),
            _ => catalog.as_ref().and_then(|c| c.game_version.clone()),
        };
        let count = catalog.as_ref().map_or(0, |c| c.mods.len());
        Ok(LoaderState {
            name: game.loader_name,
            present: game.loader_present(&config),
            managed: record.is_some(),
            version: record.and_then(|r| r.version.clone()),
            installable: game.loader_installable() && loader.is_some(),
            id: loader.as_ref().map(|l| l.id.clone()),
            latest: loader.as_ref().map(|l| l.latest.clone()),
            note: game.loader_note(&config, game_version.as_deref(), count),
            launch_option: game.launch_option(),
            game_version,
        })
    }

    /// Installe un mod de la liste, ce dont il a besoin, et le chargeur s'il manque.
    pub async fn install(
        &self,
        game: &GameDef,
        id: &str,
        version: Option<String>,
        other_libraries: &[Library],
        emit: Arc<dyn Fn(InstallEvent) + Send + Sync>,
    ) -> AppResult<ListInstalled> {
        let gid = game_id(game.id);
        let config = self.config(game).ok_or(AppError::NoGameFolder)?;
        if !config.folder.is_dir() {
            return Err(AppError::Io(crate::tr!("le dossier du jeu n'existe plus : {}", "the game folder no longer exists: {}", config.folder.display())));
        }
        let cancel = Arc::new(AtomicBool::new(false));
        {
            let mut jobs = self.jobs.lock().expect("jobs");
            if jobs.contains_key(&gid) {
                return Err(AppError::InstallInProgress);
            }
            jobs.insert(gid, cancel.clone());
        }
        let _guard = JobGuard { state: self, game_id: gid };
        emit(InstallEvent::Preparing);
        let catalog = self.catalog(game, false).await?;
        let target = catalog.get(id).ok_or_else(|| AppError::InvalidArgument(crate::tr!("ce mod n'est plus dans la liste {}.", "this mod is no longer in the {} list.", game.list_name())))?;
        let version = match version {
            Some(v) => target
                .version(&v)
                .map(|x| x.version.clone())
                .ok_or_else(|| {
                    AppError::InvalidArgument(crate::tr!(
                        "la version {v} de « {} » n'est plus dans la liste.",
                        "version {v} of \"{}\" is no longer in the list.",
                        target.name
                    ))
                })?,
            None => target.latest.clone(),
        };
        if target.broken {
            return Err(AppError::InvalidArgument(crate::tr!(
                "« {} » est signalé comme cassé : il ne marche plus avec le jeu.",
                "\"{}\" is reported as broken: it no longer works with the game.",
                target.name
            )));
        }
        if !target.compatible {
            return Err(AppError::WrongPlatform(
                target.note.clone().unwrap_or_else(|| {
                    crate::tr!("« {} » ne se charge pas dans le jeu sur ce PC.", "\"{}\" doesn't load in the game on this PC.", target.name)
                }),
            ));
        }
        // Chargeur absent de la liste (site injoignable) et pas dans le jeu : rien n'est installé sans lui.
        if game.loader_installable() && catalog.loader().is_none() && !game.loader_present(&config) {
            return Err(AppError::ListUnavailable {
                list: game.loader_name.to_string(),
                detail: crate::i18n::pick("version introuvable, réessaie plus tard", "version not found, try again later").into(),
            });
        }
        let plan = plan(game, &catalog, &config, target, &version)?;
        check_conflicts(game, &catalog, &config, &plan)?;
        // Hollow Knight : le Modding API n'existe que pour une version du jeu.
        if plan.order.iter().any(|s| catalog.get(&s.id).is_some_and(|m| m.loader))
            && game.kind == Kind::HollowKnight
            && let Some(note) = games::hollow_knight::version_note(&config.folder)
        {
            return Err(AppError::InvalidArgument(note));
        }

        let staging = config.folder.join(STAGING_DIR);
        let job_dir = staging.join(format!("list-{}", target.mod_id));
        let _ = fs::remove_dir_all(&job_dir);
        let cleanup = || {
            let _ = fs::remove_dir_all(&job_dir);
            let _ = fs::remove_dir(&staging); // seulement s'il est vide
        };
        let mut plan = plan;
        let mut queue: Vec<Step> = plan.order.clone();
        let mut seen: HashSet<String> = queue.iter().map(|s| s.id.to_lowercase()).collect();
        let mut names = Vec::new();
        let mut i = 0;
        while i < queue.len() {
            if cancel.load(Ordering::Relaxed) {
                cleanup();
                self.settle(gid);
                return Err(AppError::Cancelled);
            }
            let step = queue[i].clone();
            let m = catalog.get(&step.id).expect("dans la liste");
            emit(InstallEvent::Step { name: m.name.clone(), index: i as u32 + 1, total: queue.len() as u32 });
            let is_target = m.mod_id == target.mod_id;
            let found = match self.install_one(game, m, &step.version, is_target, &plan, &job_dir, other_libraries, &cancel, &emit).await {
                Ok(found) => found,
                Err(e) => {
                    cleanup();
                    self.settle(gid);
                    return Err(e);
                }
            };
            names.push(m.name.clone());
            // Mods demandés dans le mod lui-même (Outer Wilds) : installés ensuite.
            let installed: HashSet<i64> = self.config(game).map(|c| c.mods.keys().copied().collect()).unwrap_or_default();
            for dep in found {
                let Some(d) = catalog.get(&dep) else { continue };
                let parents = plan.parents.entry(d.mod_id).or_default();
                if !parents.contains(&m.mod_id) {
                    parents.push(m.mod_id);
                }
                if installed.contains(&d.mod_id) || !seen.insert(d.id.to_lowercase()) || queue.len() >= MAX_MODS {
                    continue;
                }
                queue.push(Step { id: d.id.clone(), version: d.latest.clone() });
            }
            i += 1;
        }
        plan.order = queue;
        cleanup();

        // Mods déjà installés dont le nouveau mod a besoin : il compte parmi leurs parents.
        let mut lib = self.library.lock().expect("library");
        let config = lib.games.get_mut(&gid).ok_or(AppError::NotFound)?;
        for (child, parents) in &plan.parents {
            if let Some(m) = config.mods.get_mut(child).filter(|m| m.auto) {
                for p in parents {
                    if !m.required_by.contains(p) && *p != m.mod_id {
                        m.required_by.push(*p);
                    }
                }
            }
        }
        toggle::sync_active(config);
        let out = config.clone();
        self.save_library(&lib)?;
        Ok(ListInstalled { game: out, installed: names })
    }

    /// Télécharge, vérifie, place et enregistre un mod.
    #[allow(clippy::too_many_arguments)]
    async fn install_one(
        &self,
        game: &GameDef,
        m: &ListMod,
        version: &str,
        is_target: bool,
        plan: &Plan,
        job_dir: &Path,
        other_libraries: &[Library],
        cancel: &Arc<AtomicBool>,
        emit: &Arc<dyn Fn(InstallEvent) + Send + Sync>,
    ) -> AppResult<Vec<String>> {
        let gid = game_id(game.id);
        let v = m.version(version).unwrap_or_else(|| m.newest());
        let mut found: Vec<String> = Vec::new();
        let (config, others, old) = {
            let lib = self.library.lock().expect("library");
            let config = lib.games.get(&gid).cloned().ok_or(AppError::NotFound)?;
            let mut libs: Vec<&Library> = vec![&*lib];
            libs.extend(other_libraries.iter());
            let old = config.mods.get(&m.mod_id).cloned();
            (config, owned_by_others(&libs, gid, m.mod_id), old)
        };
        if old.as_ref().is_some_and(|o| o.disabled) {
            return Err(AppError::InvalidArgument(crate::tr!("réactive « {} » avant de le mettre à jour.", "enable \"{}\" again before updating it.", m.name)));
        }
        let old_files: HashSet<(String, String)> = old
            .iter()
            .flat_map(|o| {
                o.files.iter().map(|f| (String::new(), f.clone())).chain(o.parts.iter().flat_map(|p| p.files.iter().map(|f| (p.key.clone(), f.clone()))))
            })
            .collect();
        let dir = job_dir.join(m.mod_id.to_string());
        let mut batches: BTreeMap<String, Vec<(PathBuf, String)>> = BTreeMap::new();
        let mut adopted: Vec<(String, String)> = Vec::new();
        let mut replaced: Vec<(String, String)> = Vec::new();
        let mut size: i64 = 0;
        for (i, a) in v.artifacts.iter().enumerate() {
            let Artifact { url, hash, place, replaces, contents } = a;
            let (key, raw_dir) = match place {
                Place::File { key, dir, .. } | Place::Zip { key, dir, .. } => (key.clone(), dir.clone()),
            };
            let root = config.folder_for(&key).map(Path::to_path_buf).ok_or_else(|| AppError::NeedGameDir(game.name.to_string()))?;
            let dest_dir = game
                .resolve(&config.folder, &raw_dir)
                .ok_or_else(|| {
                    AppError::InvalidArgument(crate::tr!(
                        "dossier des données de {} introuvable dans le dossier du jeu.",
                        "{}'s data folder not found in the game folder.",
                        game.name
                    ))
                })?;
            let owners = others.get(&root);
            let unowned = |rel: &str| !old_files.contains(&(key.clone(), rel.to_string())) && owners.is_none_or(|o| !o.contains_key(rel));
            // Fichier identique déjà là (installé à la main) : repris tel quel.
            if let (Place::File { name: Some(n), .. }, Hash::Sha256(h)) = (place, hash) {
                let rel = join(&dest_dir, n);
                if unowned(&rel) && root.join(&rel).is_file() && sha256_file(&root.join(&rel)).is_ok_and(|x| x == *h) {
                    adopted.push((key.clone(), rel));
                    continue;
                }
            }
            let tmp = dir.join(i.to_string());
            let progress_emit = emit.clone();
            let progress = move |received: u64, total: u64| progress_emit(InstallEvent::Downloading { received, total });
            let given = fetch(self.net.downloads(), url, &tmp, cancel, &progress).await.map_err(|e| download_error(e, &m.name, url))?;
            verify(&tmp, hash)?;
            let len = fs::metadata(&tmp).map(|x| x.len()).unwrap_or(0);
            size += len as i64;
            let mut placed: Vec<(PathBuf, String)> = Vec::new();
            match place {
                Place::File { name, .. } => {
                    let n = name.clone().or(given).ok_or_else(|| AppError::BadArchive(crate::i18n::pick("nom de fichier inconnu", "unknown file name").into()))?;
                    // ChilloutVR : MelonLoader ne lit que les `.dll`.
                    let n = if game.kind == Kind::ChilloutVr && !n.to_ascii_lowercase().ends_with(".dll") {
                        format!("{}.dll", installer::safe_name(&m.name))
                    } else {
                        n
                    };
                    placed.push((tmp, join(&dest_dir, &n)));
                }
                Place::Zip { root: zip_root, .. } => {
                    let out = dir.join(format!("{i}.x"));
                    let (tmp_b, out_b, cancel_b, emit_b) = (tmp.clone(), out.clone(), cancel.clone(), emit.clone());
                    tokio::task::spawn_blocking(move || {
                        let progress = |done: u64, total: u64| emit_b(InstallEvent::Extracting { done, total });
                        archive::extract(&tmp_b, &out_b, installer::extraction_limit(len, 0), &cancel_b, &progress)
                    })
                    .await
                    .map_err(|e| AppError::Io(e.to_string()))??;
                    // Chaque fichier doit avoir l'empreinte publiée (BeatMods), et aucun autre n'est accepté.
                    if !contents.is_empty() {
                        let listed: HashMap<&str, &str> = contents.iter().map(|(p, h)| (p.as_str(), h.as_str())).collect();
                        let extracted = files_under(&out);
                        if extracted.len() != listed.len() {
                            return Err(AppError::Corrupted);
                        }
                        for (abs, rel) in &extracted {
                            let expected = listed.get(rel.as_str()).ok_or(AppError::Corrupted)?;
                            if !md5_file(abs).is_ok_and(|h| h.eq_ignore_ascii_case(expected)) {
                                return Err(AppError::Corrupted);
                            }
                        }
                    }
                    let base = match zip_root {
                        ZipRoot::AsIs => out.clone(),
                        ZipRoot::DirOf(file) => dir_with(&out, file).unwrap_or(out.clone()),
                    };
                    found.extend(game.discovered_dependencies(&base));
                    let copies_dir = dir.join(format!("{i}.copies"));
                    for (abs, rel) in files_under(&base) {
                        let rel = game.zip_rel(&rel);
                        if rel.is_empty() || game.skip_file(m.loader, &rel) {
                            continue;
                        }
                        // BSIPA : aussi là où `IPA.exe` les copierait.
                        if let Some(copy) = game.copies(m.loader, &rel) {
                            let dup = copies_dir.join(&copy);
                            if let Some(parent) = dup.parent() {
                                fs::create_dir_all(parent).map_err(installer::io_err)?;
                            }
                            fs::copy(&abs, &dup).map_err(installer::io_err)?;
                            placed.push((dup, join(&dest_dir, &copy)));
                        }
                        placed.push((abs, join(&dest_dir, &rel)));
                    }
                }
            }
            if *replaces {
                replaced.extend(placed.iter().filter(|(_, r)| unowned(r)).map(|(_, r)| (key.clone(), r.clone())));
            }
            batches.entry(key).or_default().extend(placed);
        }
        emit(InstallEvent::Finishing);

        // Fichiers du jeu remplacés (chargeur) : gardés de côté.
        let replaced: Vec<String> = replaced.into_iter().filter(|(k, _)| k.is_empty()).map(|(_, r)| r).collect();
        let kept = keep_originals(&config.folder, &replaced);
        let placed = if batches.values().all(Vec::is_empty) {
            Vec::new()
        } else {
            let (config_b, old_b) = (config.clone(), old.clone());
            let result = tokio::task::spawn_blocking(move || place_plan(&config_b, old_b.as_ref(), &others, batches, None))
                .await
                .map_err(|e| AppError::Io(e.to_string()))?;
            match result {
                Ok(p) => p,
                Err(e) => {
                    undo_originals(&config.folder, &kept);
                    return Err(e);
                }
            }
        };

        let parent = (!is_target).then(|| plan.parents.get(&m.mod_id).and_then(|p| p.first().copied())).flatten();
        let (auto, mut required_by) = match (&old, is_target) {
            (Some(o), true) => (o.auto, o.required_by.clone()),
            _ => dependency_state(parent, old.as_ref()),
        };
        if auto {
            for p in plan.parents.get(&m.mod_id).into_iter().flatten() {
                if !required_by.contains(p) {
                    required_by.push(*p);
                }
            }
        }
        let mut record = InstalledMod {
            mod_id: m.mod_id,
            auto,
            required_by,
            name: m.name.clone(),
            logo: m.icon.clone().or_else(|| m.icon_fallback.clone()),
            version: Some(v.version.clone()),
            modfile_id: 0,
            size,
            installed_at: now(),
            layout: Layout::Flat,
            file_name: Some(record_name(game.id, &m.id)),
            ..Default::default()
        };
        for batch in placed {
            if batch.key.is_empty() {
                record.files = batch.files;
                record.dirs = batch.dirs;
            } else if !batch.files.is_empty() || !batch.dirs.is_empty() {
                record.parts.push(ModPart { key: batch.key, files: batch.files, dirs: batch.dirs });
            }
        }
        // Originaux gardés : ceux de cette version et ceux des précédentes (remis au retrait).
        record.replaced = old.as_ref().map(|o| o.replaced.clone()).unwrap_or_default();
        for rel in kept {
            if !record.replaced.contains(&rel) {
                record.replaced.push(rel);
            }
        }
        for (key, rel) in adopted {
            if key.is_empty() {
                record.files.push(rel);
            } else if let Some(p) = record.parts.iter_mut().find(|p| p.key == key) {
                p.files.push(rel);
            } else {
                record.parts.push(ModPart { key, files: vec![rel], dirs: Vec::new() });
            }
        }
        {
            let mut lib = self.library.lock().expect("library");
            let config = lib.games.get_mut(&gid).ok_or(AppError::NotFound)?;
            config.mods.insert(m.mod_id, record);
            self.save_library(&lib)?;
        }
        // Après l'enregistrement : un échec ici n'empêche pas de réessayer (mise à jour).
        if m.loader {
            game.activate(&config.folder, &config.extra_folders)?;
        }
        Ok(found)
    }

    /// Après un échec : les mods nécessaires installés pour un mod qui ne l'a
    /// pas été ne dépendent plus de lui (et redeviennent des mods ordinaires).
    fn settle(&self, gid: i64) {
        let mut lib = self.library.lock().expect("library");
        let Some(config) = lib.games.get_mut(&gid) else { return };
        let ids: HashSet<i64> = config.mods.keys().copied().collect();
        for m in config.mods.values_mut() {
            m.required_by.retain(|p| ids.contains(p));
            if m.auto && m.required_by.is_empty() {
                m.auto = false;
            }
        }
        let _ = self.save_library(&lib);
    }

    /// Lance l'outil du chargeur (OWML).
    pub fn launch(&self, game: &GameDef) -> AppResult<()> {
        let config = self.config(game).ok_or(AppError::NoGameFolder)?;
        game.launch(&config.extra_folders)
    }

    /// Annule l'installation en cours pour ce jeu.
    pub fn cancel(&self, game: &GameDef) {
        if let Some(flag) = self.jobs.lock().expect("jobs").get(&game_id(game.id)) {
            flag.store(true, Ordering::Relaxed);
        }
    }

    /// Désinstalle ; renvoie le dossier laissé en place et les mods nécessaires devenus inutiles.
    pub fn remove(&self, game_id: i64, mod_id: i64) -> AppResult<UninstallResult> {
        if self.jobs.lock().expect("jobs").contains_key(&game_id) {
            return Err(AppError::InstallInProgress);
        }
        let mut lib = self.library.lock().expect("library");
        let game = lib.games.get_mut(&game_id).ok_or(AppError::NotFound)?;
        let record = game.mods.get(&mod_id).cloned().ok_or(AppError::NotFound)?;
        let removal = if record.disabled {
            let root = self.disabled_root();
            toggle::discard(game, &record, &toggle::Ctx { store_root: &root, source: "lists" });
            installer::FullRemoval { remaining: Vec::new(), remaining_parts: Vec::new(), left_folder: None }
        } else {
            installer::uninstall_all(game, &record)
        };
        // Fichiers du jeu que le chargeur avait remplacés : remis dès que leur remplaçant est parti.
        restore_originals(&game.folder, &record.replaced);
        if !removal.complete() {
            let n = removal.remaining_count();
            let first = removal.first_remaining().cloned().unwrap_or_default();
            if let Some(r) = game.mods.get_mut(&mod_id) {
                r.files = removal.remaining;
            }
            self.save_library(&lib)?;
            return Err(AppError::Io(if n == 1 {
                crate::tr!(
                    "{n} fichier(s) n'ont pas pu être supprimés, dont {first}. Ferme le jeu puis réessaie.",
                    "{n} file couldn't be deleted: {first}. Close the game, then try again."
                )
            } else {
                crate::tr!(
                    "{n} fichier(s) n'ont pas pu être supprimés, dont {first}. Ferme le jeu puis réessaie.",
                    "{n} files couldn't be deleted, including {first}. Close the game, then try again."
                )
            }));
        }
        restore_originals(&game.folder, &record.files);
        for part in &record.parts {
            if let Some(root) = game.extra_folders.get(&part.key) {
                restore_originals(root, &part.files);
            }
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
        let ctx = toggle::Ctx { store_root: &root, source: "lists" };
        let mut lib = self.library.lock().expect("library");
        let mut game = lib.games.remove(&game_id).ok_or(AppError::NotFound)?;
        let replaced = game.mods.get(&mod_id).map(|m| m.replaced.clone()).unwrap_or_default();
        let result = if !game.mods.contains_key(&mod_id) {
            Err(AppError::NotFound)
        } else if enabled {
            let mut libs: Vec<&Library> = other_libraries.iter().collect();
            libs.insert(0, &*lib);
            let others = toggle::others_map(&game, mod_id, &libs);
            // Le chargeur revient : les fichiers du jeu repartent de côté.
            let kept = keep_originals(&game.folder, &replaced);
            let r = toggle::enable(&mut game, mod_id, &ctx, &others);
            if r.is_err() {
                restore_originals(&game.folder, &kept);
            }
            r
        } else {
            let r = toggle::disable(&mut game, mod_id, &ctx);
            // Chargeur désactivé : le jeu retrouve ses fichiers (sinon il ne démarre plus).
            if r.is_ok() {
                restore_originals(&game.folder, &replaced);
            }
            r
        };
        let out = game.clone();
        lib.games.insert(game_id, game);
        self.save_library(&lib)?;
        result.map(|_| out)
    }

    /// Passe à un profil de mods (un chargeur désactivé rend au jeu ses
    /// fichiers, comme avec l'interrupteur du mod).
    pub fn apply_profile(&self, game_id: i64, name: &str, other_libraries: &[Library]) -> AppResult<toggle::ProfileApplied> {
        if self.jobs.lock().expect("jobs").contains_key(&game_id) {
            return Err(AppError::InstallInProgress);
        }
        let root = self.disabled_root();
        let ctx = toggle::Ctx { store_root: &root, source: "lists" };
        let mut lib = self.library.lock().expect("library");
        let mut game = lib.games.remove(&game_id).ok_or(AppError::NotFound)?;
        let result = {
            let mut libs: Vec<&Library> = other_libraries.iter().collect();
            libs.insert(0, &*lib);
            toggle::apply_profile_by(&mut game, name, &mut |g, id, on| {
                let replaced = g.mods.get(&id).map(|m| m.replaced.clone()).unwrap_or_default();
                if on {
                    let others = toggle::others_map(g, id, &libs);
                    let kept = keep_originals(&g.folder, &replaced);
                    let r = toggle::enable(g, id, &ctx, &others);
                    if r.is_err() {
                        restore_originals(&g.folder, &kept);
                    }
                    r
                } else {
                    let r = toggle::disable(g, id, &ctx);
                    if r.is_ok() {
                        restore_originals(&g.folder, &replaced);
                    }
                    r
                }
            })
        };
        let out = game.clone();
        lib.games.insert(game_id, game);
        self.save_library(&lib)?;
        result.map(|issues| toggle::ProfileApplied { game: out, issues })
    }

    /// Crée, renomme ou supprime un profil.
    pub fn edit_profiles(&self, game_id: i64, edit: &toggle::ProfileEdit) -> AppResult<GameInstall> {
        let mut lib = self.library.lock().expect("library");
        let game = lib.games.get_mut(&game_id).ok_or(AppError::NotFound)?;
        toggle::edit_profiles(game, edit)?;
        let out = game.clone();
        self.save_library(&lib)?;
        Ok(out)
    }

    /// Mods installés dont une version plus récente est dans la liste.
    pub async fn updates(&self, game_id: i64) -> AppResult<Vec<ListUpdate>> {
        let Some(game) = def_of(game_id) else { return Ok(Vec::new()) };
        let prefix = format!("{}:", game.id);
        let installed: Vec<(i64, String, Option<String>)> = self
            .config(game)
            .map(|g| {
                g.mods
                    .values()
                    .filter_map(|m| m.file_name.as_deref().and_then(|f| f.strip_prefix(&prefix)).map(|id| (m.mod_id, id.to_string(), m.version.clone())))
                    .collect()
            })
            .unwrap_or_default();
        if installed.is_empty() {
            return Ok(Vec::new());
        }
        let catalog = self.catalog(game, false).await?;
        Ok(installed
            .into_iter()
            .filter_map(|(mod_id, id, v)| {
                let m = catalog.get(&id)?;
                let newer = v.as_deref().is_none_or(|v| version_cmp(&m.latest, v) == std::cmp::Ordering::Greater);
                newer.then(|| ListUpdate { mod_id, version: m.latest.clone() })
            })
            .collect())
    }

    /// Fichier ou dossier à montrer dans l'explorateur.
    pub fn reveal_path(&self, game_id: i64, mod_id: Option<i64>) -> AppResult<PathBuf> {
        let lib = self.library.lock().expect("library");
        let game = lib.games.get(&game_id).ok_or(AppError::NotFound)?;
        let Some(mod_id) = mod_id else { return Ok(game.folder.clone()) };
        let record = game.mods.get(&mod_id).ok_or(AppError::NotFound)?;
        if let Some(rel) = record.dirs.iter().min_by_key(|d| d.len()).or(record.files.first()) {
            return Ok(installer::resolve_rel(&game.folder, rel).unwrap_or_else(|| game.folder.clone()));
        }
        if let Some(part) = record.parts.first()
            && let (Some(root), Some(rel)) = (game.extra_folders.get(&part.key), part.dirs.first().or(part.files.first()))
        {
            return Ok(installer::resolve_rel(root, rel).unwrap_or_else(|| root.clone()));
        }
        Ok(game.folder.clone())
    }
}

impl GameDef {
    /// Chemin d'un fichier d'archive dans le dossier du mod (Hollow Knight :
    /// sans le préfixe `hollow_knight_Data/Managed/Mods/` qu'ont certaines archives).
    pub fn zip_rel(&self, rel: &str) -> String {
        if self.kind == Kind::HollowKnight {
            let lower = rel.to_ascii_lowercase();
            for prefix in ["hollow_knight_data/managed/mods/", "hollow_knight_data/managed/"] {
                if lower.starts_with(prefix) {
                    return rel[prefix.len()..].to_string();
                }
            }
        }
        rel.to_string()
    }
}

