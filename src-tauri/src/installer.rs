//! Installation manuelle des mods : téléchargement, vérification MD5,
//! extraction, placement dans le dossier du jeu, manifeste et désinstallation.
//!
//! Les fonctions de ce module ne dépendent pas de Tauri (testables seules) ;
//! les commandes sont dans `install_commands.rs`.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use md5::{Digest, Md5};
use reqwest::{StatusCode, Url};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

/// Dossier de travail temporaire, créé dans le dossier des mods du jeu (même
/// disque : les fichiers y sont ensuite déplacés sans copie).
pub const STAGING_DIR: &str = ".modstudio-tmp";

// ---------------------------------------------------------------- Manifeste

/// Façon de ranger les fichiers d'un mod dans le dossier du jeu.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Layout {
    /// Chaque mod extrait dans son propre sous-dossier (par défaut).
    Subfolder,
    /// Contenu de l'archive extrait directement dans le dossier.
    Flat,
    /// Archive copiée telle quelle, sans extraction.
    Archive,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledMod {
    pub mod_id: i64,
    pub name: String,
    #[serde(default)]
    pub logo: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    pub modfile_id: i64,
    #[serde(default)]
    pub size: i64,
    pub installed_at: i64,
    pub layout: Layout,
    /// Sous-dossier du mod (mode `Subfolder`), relatif au dossier du jeu.
    #[serde(default)]
    pub root: Option<String>,
    /// Fichiers installés, relatifs au dossier du jeu, séparés par `/`.
    #[serde(default)]
    pub files: Vec<String>,
    /// Dossiers créés par l'installation, relatifs au dossier du jeu.
    #[serde(default)]
    pub dirs: Vec<String>,
    /// Installé automatiquement comme mod nécessaire d'un autre mod.
    #[serde(default)]
    pub auto: bool,
    /// Mods installés qui ont entraîné l'installation de celui-ci.
    #[serde(default)]
    pub required_by: Vec<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameInstall {
    pub game_id: i64,
    pub game_name: String,
    #[serde(default)]
    pub icon: Option<String>,
    pub folder: PathBuf,
    pub layout: Layout,
    #[serde(default)]
    pub mods: BTreeMap<i64, InstalledMod>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Library {
    #[serde(default)]
    pub games: BTreeMap<i64, GameInstall>,
}

impl Library {
    pub fn load(path: &Path) -> Library {
        fs::read_to_string(path)
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default()
    }

    /// Écriture atomique : fichier temporaire puis renommage.
    pub fn save(&self, path: &Path) -> AppResult<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(io_err)?;
        }
        let tmp = path.with_extension("json.tmp");
        let raw = serde_json::to_vec_pretty(self).map_err(|e| AppError::Io(e.to_string()))?;
        fs::write(&tmp, raw).map_err(io_err)?;
        fs::rename(&tmp, path).map_err(io_err)
    }
}

pub fn io_err(e: io::Error) -> AppError {
    AppError::Io(e.to_string())
}

// ---------------------------------------------------------------- Chemins

/// Chemin relatif → texte portable (`a/b/c.pak`).
pub fn rel_string(p: &Path) -> String {
    p.components()
        .filter_map(|c| match c {
            Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// Texte du manifeste → chemin sous `base`. Refuse tout ce qui sortirait de `base`.
pub fn resolve_rel(base: &Path, rel: &str) -> Option<PathBuf> {
    let mut out = base.to_path_buf();
    let mut any = false;
    for part in rel.split('/') {
        if part.is_empty() || part == "." || part == ".." || part.contains(['\\', ':']) {
            return None;
        }
        out.push(part);
        any = true;
    }
    any.then_some(out)
}

/// Nom de fichier ou de dossier sûr sous Windows et Linux.
pub fn safe_name(input: &str) -> String {
    let mut s: String = input
        .chars()
        .map(|c| if c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | ' ' | '(' | ')') { c } else { '-' })
        .collect();
    s = s.trim_matches(|c| c == '.' || c == ' ').to_string();
    if s.chars().count() > 80 {
        s = s.chars().take(80).collect::<String>().trim_end().to_string();
    }
    let stem = s.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.len() == 4
            && stem.as_bytes()[3].is_ascii_digit());
    if reserved {
        s = format!("_{s}");
    }
    if s.is_empty() { "mod".to_string() } else { s }
}

/// Le lien de téléchargement doit venir de mod.io (ou du même hôte que l'API).
pub fn download_url_allowed(url: &str, api_path: &str) -> bool {
    let Ok(u) = Url::parse(url) else { return false };
    let Some(host) = u.host_str().map(str::to_ascii_lowercase) else { return false };
    let api_host = Url::parse(api_path).ok().and_then(|a| a.host_str().map(str::to_ascii_lowercase));
    if api_host.as_deref() == Some(host.as_str()) {
        return true;
    }
    u.scheme() == "https"
        && (host == "mod.io"
            || host.ends_with(".mod.io")
            || host.ends_with(".modapi.io")
            || host.ends_with(".modcdn.io"))
}

// ---------------------------------------------------------------- Progression

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "phase", rename_all = "camelCase")]
pub enum InstallEvent {
    Preparing,
    Downloading { received: u64, total: u64 },
    Verifying,
    Extracting { done: u64, total: u64 },
    Finishing,
}

// ---------------------------------------------------------------- Téléchargement

pub struct DownloadRequest<'a> {
    pub url: &'a str,
    pub token: Option<&'a str>,
    pub dest: &'a Path,
    pub expected_size: u64,
    pub expected_md5: Option<&'a str>,
}

/// Télécharge vers `dest` en vérifiant la taille et le MD5. En cas d'échec ou
/// d'annulation, le fichier partiel est supprimé.
pub async fn download(
    http: &reqwest::Client,
    req: DownloadRequest<'_>,
    cancel: &AtomicBool,
    progress: &(dyn Fn(u64, u64) + Send + Sync),
) -> AppResult<()> {
    let result = download_inner(http, &req, cancel, progress).await;
    if result.is_err() {
        let _ = fs::remove_file(req.dest);
    }
    result
}

async fn download_inner(
    http: &reqwest::Client,
    req: &DownloadRequest<'_>,
    cancel: &AtomicBool,
    progress: &(dyn Fn(u64, u64) + Send + Sync),
) -> AppResult<()> {
    let mut request = http.get(req.url);
    if let Some(token) = req.token {
        // reqwest retire cet en-tête si mod.io redirige vers un autre domaine.
        request = request.bearer_auth(token);
    }
    let mut response = request
        .send()
        .await
        .map_err(|e| AppError::Network(e.without_url().to_string()))?;

    let status = response.status();
    if !status.is_success() {
        let message = response
            .json::<crate::api::models::ApiErrorBody>()
            .await
            .ok()
            .and_then(|b| b.error.message)
            .unwrap_or_else(|| status.canonical_reason().unwrap_or("erreur").to_string());
        return Err(match status {
            StatusCode::UNAUTHORIZED if req.token.is_some() => AppError::SessionExpired,
            StatusCode::UNAUTHORIZED => AppError::LoginRequiredForDownload,
            StatusCode::FORBIDDEN => AppError::DownloadRefused(message),
            StatusCode::NOT_FOUND => AppError::NotFound,
            other => AppError::Api {
                status: other.as_u16(),
                message,
                error_ref: None,
            },
        });
    }

    let total = response.content_length().unwrap_or(req.expected_size).max(req.expected_size);
    // Garde-fou contre une réponse sans fin.
    let limit = total.saturating_mul(2).saturating_add(10 * 1024 * 1024);

    if let Some(dir) = req.dest.parent() {
        fs::create_dir_all(dir).map_err(io_err)?;
    }
    let mut file = File::create(req.dest).map_err(io_err)?;
    let mut hasher = Md5::new();
    let mut received: u64 = 0;
    let mut last = Instant::now();
    progress(0, total);

    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| AppError::Network(e.without_url().to_string()))?
    {
        if cancel.load(Ordering::Relaxed) {
            return Err(AppError::Cancelled);
        }
        file.write_all(&chunk).map_err(io_err)?;
        hasher.update(&chunk);
        received += chunk.len() as u64;
        if received > limit {
            return Err(AppError::DownloadRefused("taille de fichier inattendue".into()));
        }
        if last.elapsed() >= Duration::from_millis(120) {
            progress(received, total);
            last = Instant::now();
        }
    }
    file.flush().map_err(io_err)?;
    drop(file);
    progress(received, total.max(received));

    if cancel.load(Ordering::Relaxed) {
        return Err(AppError::Cancelled);
    }
    if let Some(expected) = req.expected_md5.map(str::trim).filter(|s| !s.is_empty()) {
        let actual: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
        if !actual.eq_ignore_ascii_case(expected) {
            return Err(AppError::Corrupted);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------- Extraction

/// Extrait une archive zip dans `dest` (qui doit être vide ou absent).
/// Refuse les chemins qui sortiraient du dossier et les archives démesurées.
pub fn extract_zip(
    archive: &Path,
    dest: &Path,
    max_total: u64,
    cancel: &AtomicBool,
    progress: &dyn Fn(u64, u64),
) -> AppResult<()> {
    let file = File::open(archive).map_err(io_err)?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| AppError::BadArchive(e.to_string()))?;
    fs::create_dir_all(dest).map_err(io_err)?;
    let count = zip.len() as u64;
    let mut written: u64 = 0;

    for i in 0..zip.len() {
        if cancel.load(Ordering::Relaxed) {
            return Err(AppError::Cancelled);
        }
        let mut entry = zip.by_index(i).map_err(|e| AppError::BadArchive(e.to_string()))?;
        let name = entry.name().map(|n| n.into_owned()).unwrap_or_else(|_| format!("entrée {i}"));
        let Some(rel) = entry.enclosed_name() else {
            return Err(AppError::BadArchive(format!("chemin dangereux dans l'archive : {name}")));
        };
        if entry.is_symlink() {
            // Les liens symboliques pourraient pointer hors du dossier : ignorés.
            continue;
        }
        let out = dest.join(&rel);
        if entry.is_dir() {
            fs::create_dir_all(&out).map_err(io_err)?;
            continue;
        }
        if let Some(parent) = out.parent() {
            fs::create_dir_all(parent).map_err(io_err)?;
        }
        let mut target = File::create(&out).map_err(|e| AppError::Io(format!("{name} : {e}")))?;
        let remaining = max_total.saturating_sub(written);
        let copied = io::copy(&mut (&mut entry).take(remaining + 1), &mut target)
            .map_err(|e| AppError::BadArchive(format!("{name} : {e}")))?;
        written += copied;
        if written > max_total {
            return Err(AppError::BadArchive("contenu bien plus gros qu'annoncé".into()));
        }
        progress(i as u64 + 1, count);
    }
    progress(count, count);
    Ok(())
}

/// Taille maximale acceptée une fois l'archive décompressée.
pub fn extraction_limit(compressed: u64, announced_uncompressed: u64) -> u64 {
    const CAP: u64 = 64 * 1024 * 1024 * 1024;
    let base = if announced_uncompressed > 0 {
        announced_uncompressed.saturating_add(announced_uncompressed / 10)
    } else {
        compressed.saturating_mul(100)
    };
    base.max(16 * 1024 * 1024).min(CAP)
}

/// Fichiers (pas les dossiers) sous `root`, en chemins relatifs.
fn walk_files(root: &Path) -> AppResult<Vec<PathBuf>> {
    let mut out = Vec::new();
    let mut stack = vec![PathBuf::new()];
    while let Some(rel) = stack.pop() {
        for entry in fs::read_dir(root.join(&rel)).map_err(io_err)? {
            let entry = entry.map_err(io_err)?;
            let kind = entry.file_type().map_err(io_err)?;
            let child = rel.join(entry.file_name());
            if kind.is_dir() {
                stack.push(child);
            } else if kind.is_file() {
                out.push(child);
            }
        }
    }
    out.sort();
    Ok(out)
}

// ---------------------------------------------------------------- Placement

pub struct PlaceInput<'a> {
    pub folder: &'a Path,
    pub layout: Layout,
    pub mod_id: i64,
    /// Identifiant texte du mod (`clear-skies`), pour nommer son sous-dossier.
    pub name_id: &'a str,
    /// Archive téléchargée.
    pub archive: &'a Path,
    /// Nom d'origine du fichier (mode `Archive`).
    pub filename: &'a str,
    /// Dossier où l'archive a été extraite (modes `Subfolder` et `Flat`).
    pub extracted: Option<&'a Path>,
    /// Version installée auparavant (mise à jour).
    pub old: Option<&'a InstalledMod>,
    /// Fichiers des autres mods du même jeu : jamais écrasés.
    pub others: &'a BTreeMap<String, String>,
}

pub struct Placed {
    pub root: Option<String>,
    pub files: Vec<String>,
    pub dirs: Vec<String>,
}

/// Déplace les fichiers dans le dossier du jeu. Vérifie tous les conflits
/// avant de toucher au moindre fichier.
pub fn place(input: PlaceInput<'_>) -> AppResult<Placed> {
    let old_files: BTreeSet<&str> = input
        .old
        .map(|o| o.files.iter().map(String::as_str).collect())
        .unwrap_or_default();

    // 1. Liste des déplacements (source absolue → destination relative).
    let mut moves: Vec<(PathBuf, String)> = Vec::new();
    let mut root: Option<String> = None;
    match input.layout {
        Layout::Archive => {
            moves.push((input.archive.to_path_buf(), safe_name(input.filename)));
        }
        Layout::Subfolder | Layout::Flat => {
            let extracted = input
                .extracted
                .ok_or_else(|| AppError::BadArchive("archive non extraite".into()))?;
            let mut base = extracted.to_path_buf();
            if input.layout == Layout::Subfolder {
                // Archive qui contient un seul dossier : on ne l'imbrique pas une
                // deuxième fois (`MonMod/MonMod/...`).
                let entries: Vec<_> = fs::read_dir(extracted).map_err(io_err)?.flatten().collect();
                if entries.len() == 1 && entries[0].file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    base = entries[0].path();
                }
                let chosen = match input.old.and_then(|o| o.root.clone()) {
                    Some(r) => r,
                    None => {
                        let preferred = safe_name(input.name_id);
                        if input.folder.join(&preferred).exists() {
                            format!("{preferred}-{}", input.mod_id)
                        } else {
                            preferred
                        }
                    }
                };
                root = Some(chosen);
            }
            for rel in walk_files(&base)? {
                let rel = rel_string(&rel);
                let dest = match &root {
                    Some(r) => format!("{r}/{rel}"),
                    None => rel.clone(),
                };
                moves.push((base.join(rel.split('/').collect::<PathBuf>()), dest));
            }
        }
    }
    if moves.is_empty() {
        return Err(AppError::BadArchive("l'archive est vide".into()));
    }

    // 2. Conflits : rien n'est écrasé, sauf les fichiers de la version précédente.
    let mut conflicts = Vec::new();
    for (_, rel) in &moves {
        let abs = resolve_rel(input.folder, rel).ok_or_else(|| AppError::BadArchive(format!("chemin invalide : {rel}")))?;
        if let Some(owner) = input.others.get(rel) {
            conflicts.push(format!("{rel} (déjà installé par « {owner} »)"));
        } else if abs.exists() && !old_files.contains(rel.as_str()) {
            conflicts.push(rel.clone());
        }
    }
    if !conflicts.is_empty() {
        let more = conflicts.len().saturating_sub(3);
        let mut text = conflicts.into_iter().take(3).collect::<Vec<_>>().join(", ");
        if more > 0 {
            text.push_str(&format!(" et {more} autres"));
        }
        return Err(AppError::Conflict(text));
    }

    // 3. Retire l'ancienne version.
    if let Some(old) = input.old {
        for rel in &old.files {
            if let Some(abs) = resolve_rel(input.folder, rel) {
                match fs::remove_file(&abs) {
                    Ok(()) => {}
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                    Err(e) => return Err(AppError::Io(format!("{rel} : {e} (le jeu est-il ouvert ?)"))),
                }
            }
        }
    }

    // 4. Déplace les nouveaux fichiers, en notant les dossiers créés.
    let mut created: BTreeSet<String> = BTreeSet::new();
    let mut files = Vec::with_capacity(moves.len());
    for (src, rel) in moves {
        let abs = resolve_rel(input.folder, &rel).expect("vérifié plus haut");
        let parts: Vec<&str> = rel.split('/').collect();
        for depth in 1..parts.len() {
            let dir_rel = parts[..depth].join("/");
            let dir_abs = resolve_rel(input.folder, &dir_rel).expect("vérifié plus haut");
            if !dir_abs.exists() {
                fs::create_dir(&dir_abs).map_err(io_err)?;
                created.insert(dir_rel);
            }
        }
        move_file(&src, &abs)?;
        files.push(rel);
    }

    // 5. Dossiers de l'ancienne version devenus vides.
    let mut dirs: BTreeSet<String> = created;
    if let Some(old) = input.old {
        for d in deepest_first(&old.dirs) {
            if let Some(abs) = resolve_rel(input.folder, &d) {
                let _ = fs::remove_dir(&abs); // seulement s'il est vide
                if abs.exists() {
                    dirs.insert(d);
                }
            }
        }
    }

    Ok(Placed {
        root,
        files,
        dirs: dirs.into_iter().collect(),
    })
}

fn move_file(src: &Path, dest: &Path) -> AppResult<()> {
    match fs::rename(src, dest) {
        Ok(()) => Ok(()),
        Err(_) => {
            // Autre disque : copie puis suppression.
            fs::copy(src, dest).map_err(io_err)?;
            fs::remove_file(src).map_err(io_err)
        }
    }
}

fn deepest_first(dirs: &[String]) -> Vec<String> {
    let mut v = dirs.to_vec();
    v.sort_by_key(|d| std::cmp::Reverse(d.matches('/').count()));
    v
}

// ---------------------------------------------------------------- Désinstallation

pub struct Removal {
    /// Fichiers qui n'ont pas pu être supprimés (ouverts par le jeu…).
    pub remaining: Vec<String>,
    /// Un dossier du mod contient des fichiers ajoutés depuis : laissé en place.
    pub left_folder: Option<String>,
}

pub fn uninstall(folder: &Path, record: &InstalledMod) -> Removal {
    let mut remaining = Vec::new();
    for rel in &record.files {
        let Some(abs) = resolve_rel(folder, rel) else { continue };
        match fs::remove_file(&abs) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(_) => remaining.push(rel.clone()),
        }
    }
    let mut left_folder = None;
    for d in deepest_first(&record.dirs) {
        if let Some(abs) = resolve_rel(folder, &d) {
            let _ = fs::remove_dir(&abs);
            if abs.exists() && left_folder.is_none() && remaining.is_empty() {
                left_folder = Some(d);
            }
        }
    }
    Removal { remaining, left_folder }
}

/// Vérifie qu'un dossier choisi par l'utilisateur peut recevoir des mods.
pub fn check_folder(path: &Path) -> AppResult<()> {
    if !path.is_absolute() {
        return Err(AppError::InvalidArgument("le dossier doit être un chemin complet.".into()));
    }
    if path.parent().is_none() {
        return Err(AppError::InvalidArgument(
            "choisis un dossier, pas la racine d'un disque.".into(),
        ));
    }
    if !path.is_dir() {
        return Err(AppError::InvalidArgument("ce dossier n'existe pas.".into()));
    }
    let probe = path.join(".modstudio-write-test");
    fs::write(&probe, b"ok").map_err(|e| AppError::Io(format!("impossible d'écrire dans ce dossier ({e})")))?;
    let _ = fs::remove_file(&probe);
    Ok(())
}

// ---------------------------------------------------------------- Aide au choix du dossier

/// Le dossier ressemble au dossier d'installation d'un jeu (exécutable,
/// moteur…), ce qui n'est presque jamais le dossier où il charge ses mods.
pub fn looks_like_game_root(path: &Path) -> bool {
    let Ok(entries) = fs::read_dir(path) else { return false };
    let mut exe = false;
    let mut engine = false;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if !is_dir && name.ends_with(".exe") && !name.starts_with("unins") {
            exe = true;
        }
        let engine_file = matches!(
            name.as_str(),
            "unityplayer.dll" | "gameassembly.dll" | "steam_api64.dll" | "steam_api.dll" | "eossdk-win64-shipping.dll"
        );
        let engine_dir = is_dir && (name.ends_with("_data") || name == "engine" || name == "binaries");
        if engine_file || engine_dir {
            engine = true;
        }
    }
    exe && engine
}

/// Dossier de mods habituel des jeux connus, avec le rangement adapté.
/// Les chemins contiennent des variables d'environnement Windows (`%USERPROFILE%`).
pub fn known_folder(game_id: i64) -> Option<(&'static str, Layout)> {
    match game_id {
        // Bonelab : les « pallets » vont directement dans Mods.
        3809 => Some((r"%USERPROFILE%\AppData\LocalLow\Stress Level Zero\BONELAB\Mods", Layout::Flat)),
        _ => None,
    }
}

/// Remplace `%NOM%` par la variable d'environnement correspondante.
/// Renvoie `None` si une variable est absente.
pub fn expand_env(template: &str, lookup: impl Fn(&str) -> Option<String>) -> Option<String> {
    let mut out = String::new();
    let mut rest = template;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let end = after.find('%')?;
        out.push_str(&lookup(&after[..end])?);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Some(out)
}

// ---------------------------------------------------------------- Tests

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::testutil::fake_server;
    use std::io::Cursor;
    use std::sync::Mutex;

    pub fn tempdir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "modstudio-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Construit une archive zip en mémoire.
    pub fn make_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default();
        for (name, data) in entries {
            if name.ends_with('/') {
                w.add_directory(*name, opts).unwrap();
            } else {
                w.start_file(*name, opts).unwrap();
                w.write_all(data).unwrap();
            }
        }
        w.finish().unwrap().into_inner()
    }

    pub fn md5_hex(data: &[u8]) -> String {
        Md5::digest(data).iter().map(|b| format!("{b:02x}")).collect()
    }

    fn never() -> AtomicBool {
        AtomicBool::new(false)
    }

    fn list(dir: &Path) -> Vec<String> {
        walk_files(dir).unwrap().iter().map(|p| rel_string(p)).collect()
    }

    fn record(placed: &Placed, layout: Layout, modfile_id: i64) -> InstalledMod {
        InstalledMod {
            mod_id: 1,
            name: "Clear Skies".into(),
            logo: None,
            version: None,
            modfile_id,
            size: 0,
            installed_at: 0,
            layout,
            root: placed.root.clone(),
            files: placed.files.clone(),
            dirs: placed.dirs.clone(),
            auto: false,
            required_by: Vec::new(),
        }
    }

    fn extract(bytes: &[u8], tag: &str) -> (PathBuf, PathBuf) {
        let work = tempdir(tag);
        let archive = work.join("mod.zip");
        fs::write(&archive, bytes).unwrap();
        let out = work.join("x");
        extract_zip(&archive, &out, 1 << 30, &never(), &|_, _| {}).unwrap();
        (archive, out)
    }

    #[test]
    fn names_and_paths_are_safe() {
        assert_eq!(safe_name("clear-skies"), "clear-skies");
        assert_eq!(safe_name("a/b\\c:d*e?"), "a-b-c-d-e-");
        assert_eq!(safe_name("CON"), "_CON");
        assert_eq!(safe_name("com1.txt"), "_com1.txt");
        assert_eq!(safe_name("..."), "mod");
        assert!(resolve_rel(Path::new("/g"), "../etc/passwd").is_none());
        assert!(resolve_rel(Path::new("/g"), "a/../../b").is_none());
        assert!(resolve_rel(Path::new("/g"), "C:/Windows").is_none());
        assert_eq!(resolve_rel(Path::new("/g"), "a/b.pak").unwrap(), Path::new("/g/a/b.pak"));
    }

    #[test]
    fn download_urls_must_come_from_modio() {
        let api = "https://u-1.modapi.io/v1";
        assert!(download_url_allowed("https://g-3.modapi.io/v1/games/3/mods/1/files/2/download/abc", api));
        assert!(download_url_allowed("https://cdn.modcdn.io/x.zip", api));
        assert!(!download_url_allowed("http://g-3.modapi.io/x", api));
        assert!(!download_url_allowed("https://evil.example/x", api));
        assert!(!download_url_allowed("https://modapi.io.evil.example/x", api));
        assert!(download_url_allowed("http://127.0.0.1:9/f", "http://127.0.0.1:9/v1"));
    }

    #[test]
    fn zip_slip_is_refused() {
        let work = tempdir("slip");
        let archive = work.join("evil.zip");
        fs::write(&archive, make_zip(&[("../evil.txt", b"x")])).unwrap();
        let err = extract_zip(&archive, &work.join("x"), 1 << 20, &never(), &|_, _| {}).unwrap_err();
        assert!(matches!(err, AppError::BadArchive(_)), "{err}");
        assert!(!work.join("evil.txt").exists());
    }

    #[test]
    fn zip_bombs_are_stopped() {
        let work = tempdir("bomb");
        let archive = work.join("bomb.zip");
        fs::write(&archive, make_zip(&[("big.bin", &vec![0u8; 100_000])])).unwrap();
        let err = extract_zip(&archive, &work.join("x"), 1000, &never(), &|_, _| {}).unwrap_err();
        assert!(matches!(err, AppError::BadArchive(_)));
        assert!(extraction_limit(1000, 0) >= 16 * 1024 * 1024);
    }

    #[test]
    fn subfolder_layout_strips_single_root_and_updates_cleanly() {
        let game = tempdir("game-sub");
        let others = BTreeMap::new();

        let v1 = make_zip(&[("ClearSkies/", b""), ("ClearSkies/sky.pak", b"v1"), ("ClearSkies/old/legacy.txt", b"v1")]);
        let (archive, out) = extract(&v1, "x1");
        let placed = place(PlaceInput {
            folder: &game,
            layout: Layout::Subfolder,
            mod_id: 1,
            name_id: "clear-skies",
            archive: &archive,
            filename: "clear-skies.zip",
            extracted: Some(&out),
            old: None,
            others: &others,
        })
        .unwrap();
        assert_eq!(placed.root.as_deref(), Some("clear-skies"));
        assert_eq!(list(&game), vec!["clear-skies/old/legacy.txt", "clear-skies/sky.pak"]);
        let v1rec = record(&placed, Layout::Subfolder, 10);

        // Un fichier ajouté par l'utilisateur dans le dossier du mod.
        fs::write(game.join("clear-skies/config.ini"), b"perso").unwrap();

        let v2 = make_zip(&[("sky.pak", b"v2"), ("new/extra.pak", b"v2")]);
        let (archive2, out2) = extract(&v2, "x2");
        let placed2 = place(PlaceInput {
            folder: &game,
            layout: Layout::Subfolder,
            mod_id: 1,
            name_id: "clear-skies",
            archive: &archive2,
            filename: "clear-skies.zip",
            extracted: Some(&out2),
            old: Some(&v1rec),
            others: &others,
        })
        .unwrap();
        assert_eq!(
            list(&game),
            vec!["clear-skies/config.ini", "clear-skies/new/extra.pak", "clear-skies/sky.pak"]
        );
        assert_eq!(fs::read(game.join("clear-skies/sky.pak")).unwrap(), b"v2");
        assert!(!game.join("clear-skies/old").exists(), "dossier vide de l'ancienne version retiré");

        // Désinstallation : le fichier de l'utilisateur reste, avec son dossier.
        let removal = uninstall(&game, &record(&placed2, Layout::Subfolder, 11));
        assert!(removal.remaining.is_empty());
        assert_eq!(removal.left_folder.as_deref(), Some("clear-skies"));
        assert_eq!(list(&game), vec!["clear-skies/config.ini"]);
    }

    #[test]
    fn existing_files_are_never_overwritten() {
        let game = tempdir("game-flat");
        fs::write(game.join("shared.pak"), b"a moi").unwrap();
        let mut others = BTreeMap::new();
        others.insert("other.pak".to_string(), "Autre mod".to_string());

        let zip = make_zip(&[("shared.pak", b"mod"), ("other.pak", b"mod"), ("mine.pak", b"mod")]);
        let (archive, out) = extract(&zip, "x3");
        let err = place(PlaceInput {
            folder: &game,
            layout: Layout::Flat,
            mod_id: 2,
            name_id: "m",
            archive: &archive,
            filename: "m.zip",
            extracted: Some(&out),
            old: None,
            others: &others,
        })
        .err()
        .unwrap();
        let AppError::Conflict(text) = err else { panic!("conflit attendu") };
        assert!(text.contains("shared.pak") && text.contains("Autre mod"), "{text}");
        assert_eq!(fs::read(game.join("shared.pak")).unwrap(), b"a moi");
        assert!(!game.join("mine.pak").exists(), "rien n'est déplacé en cas de conflit");
    }

    #[test]
    fn archive_layout_keeps_the_file() {
        let game = tempdir("game-arch");
        let work = tempdir("arch");
        let archive = work.join("dl.part");
        fs::write(&archive, b"PK...").unwrap();
        let placed = place(PlaceInput {
            folder: &game,
            layout: Layout::Archive,
            mod_id: 3,
            name_id: "m",
            archive: &archive,
            filename: "Mon Mod v1.zip",
            extracted: None,
            old: None,
            others: &BTreeMap::new(),
        })
        .unwrap();
        assert_eq!(placed.files, vec!["Mon Mod v1.zip"]);
        assert!(game.join("Mon Mod v1.zip").exists());
    }

    #[test]
    fn manifest_roundtrip() {
        let dir = tempdir("manifest");
        let path = dir.join("installed.json");
        let mut lib = Library::default();
        lib.games.insert(
            7,
            GameInstall {
                game_id: 7,
                game_name: "Jeu".into(),
                icon: None,
                folder: dir.clone(),
                layout: Layout::Flat,
                mods: BTreeMap::new(),
            },
        );
        lib.save(&path).unwrap();
        let back = Library::load(&path);
        assert_eq!(back.games[&7].layout, Layout::Flat);
        assert!(Library::load(&dir.join("absent.json")).games.is_empty());

        // Un manifeste écrit par la v0.3 (sans `auto` ni `requiredBy`) reste lisible.
        let old = r#"{"games":{"3":{"gameId":3,"gameName":"J","folder":"/x","layout":"flat","mods":{"5":
            {"modId":5,"name":"M","modfileId":1,"installedAt":0,"layout":"flat","files":["a"],"dirs":[]}}}}}"#;
        fs::write(&path, old).unwrap();
        let back = Library::load(&path);
        let m = &back.games[&3].mods[&5];
        assert!(!m.auto && m.required_by.is_empty());
    }

    #[test]
    fn spots_game_install_folders() {
        let root = tempdir("gameroot");
        fs::write(root.join("BONELAB_Steam_Windows64.exe"), b"").unwrap();
        fs::write(root.join("UnityPlayer.dll"), b"").unwrap();
        fs::create_dir(root.join("BONELAB_Steam_Windows64_Data")).unwrap();
        assert!(looks_like_game_root(&root));

        let mods = tempdir("modsdir");
        fs::create_dir(mods.join("BaBaCorp.711ExtraContent")).unwrap();
        fs::write(mods.join("readme.txt"), b"").unwrap();
        assert!(!looks_like_game_root(&mods));
    }

    #[test]
    fn expands_known_folders() {
        let env = |name: &str| (name == "USERPROFILE").then(|| r"C:\Users\Code".to_string());
        let (template, layout) = known_folder(3809).unwrap();
        assert_eq!(
            expand_env(template, env).unwrap(),
            r"C:\Users\Code\AppData\LocalLow\Stress Level Zero\BONELAB\Mods"
        );
        assert_eq!(layout, Layout::Flat);
        assert!(expand_env("%ABSENTE%\\x", env).is_none());
        assert!(known_folder(1).is_none());
    }

    fn file_response(body: &[u8]) -> String {
        // Le corps binaire est passé en latin-1 pour rester une chaîne.
        let mut s = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/zip\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        s.extend(body.iter().map(|&b| b as char));
        s
    }

    #[tokio::test]
    async fn download_checks_md5_and_reports_progress() {
        let body = b"contenu du mod".to_vec();
        let server = fake_server(vec![file_response(&body), file_response(&body)]);
        let dir = tempdir("dl");
        let http = reqwest::Client::new();
        let seen = Mutex::new(Vec::new());
        let progress = |r: u64, t: u64| seen.lock().unwrap().push((r, t));

        let dest = dir.join("ok.zip");
        download(
            &http,
            DownloadRequest {
                url: &format!("{}/f", server.base),
                token: Some("tok"),
                dest: &dest,
                expected_size: body.len() as u64,
                expected_md5: Some(&md5_hex(&body).to_uppercase()),
            },
            &never(),
            &progress,
        )
        .await
        .unwrap();
        assert_eq!(fs::read(&dest).unwrap(), body);
        assert_eq!(seen.lock().unwrap().last(), Some(&(body.len() as u64, body.len() as u64)));
        assert_eq!(server.request(0).header("authorization").as_deref(), Some("Bearer tok"));

        let bad = dir.join("bad.zip");
        let err = download(
            &http,
            DownloadRequest {
                url: &format!("{}/f", server.base),
                token: None,
                dest: &bad,
                expected_size: 0,
                expected_md5: Some("00000000000000000000000000000000"),
            },
            &never(),
            &|_, _| {},
        )
        .await
        .unwrap_err();
        assert!(matches!(err, AppError::Corrupted));
        assert!(!bad.exists(), "le fichier abîmé est supprimé");
    }

    #[tokio::test]
    async fn cancelled_download_leaves_nothing() {
        let body = vec![7u8; 4096];
        let server = fake_server(vec![file_response(&body)]);
        let dir = tempdir("cancel");
        let dest = dir.join("c.zip");
        let err = download(
            &reqwest::Client::new(),
            DownloadRequest {
                url: &format!("{}/f", server.base),
                token: None,
                dest: &dest,
                expected_size: 0,
                expected_md5: None,
            },
            &AtomicBool::new(true),
            &|_, _| {},
        )
        .await
        .unwrap_err();
        assert!(matches!(err, AppError::Cancelled));
        assert!(!dest.exists());
    }
}
