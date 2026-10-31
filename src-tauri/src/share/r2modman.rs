//! Profils de r2modman (repris par Gale et Thunderstore Mod Manager) : une
//! archive `.r2z` avec `export.r2x` (nom du profil et mods, en YAML) et les
//! réglages des mods (`config/`, soit `BepInEx/config`). Partagée en code,
//! elle est publiée sur Thunderstore sous la forme `#r2modman` + base64.

use std::fs;
use std::io::{Cursor, Read, Write};
use std::path::Path;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use yaml_rust2::{Yaml, YamlLoader};
use zip::write::SimpleFileOptions;

use crate::error::{AppError, AppResult};

pub const PREFIX: &str = "#r2modman";
/// Seuls ces fichiers de réglages sont lus (et jamais de programmes).
const CONFIG_EXTENSIONS: [&str; 6] = [".cfg", ".txt", ".json", ".yml", ".yaml", ".ini"];
/// Archive lue ou publiée (r2modman refuse au-delà de 20 Mo).
pub const MAX_ZIP: usize = 20_000_000;
const MAX_CONFIG_FILE: u64 = 4 * 1024 * 1024;
const MAX_CONFIG_TOTAL: u64 = 64 * 1024 * 1024;
const MAX_CONFIG_FILES: usize = 5000;
const MAX_LIST: u64 = 4 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct R2Mod {
    /// `Équipe-Nom`.
    pub full_name: String,
    /// `1.2.3`.
    pub version: String,
    pub enabled: bool,
}

/// Fichier de réglages, chemin relatif à `BepInEx/config`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigFile {
    pub path: String,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct R2Profile {
    pub name: String,
    pub mods: Vec<R2Mod>,
    pub configs: Vec<ConfigFile>,
}

fn invalid(detail: &str) -> AppError {
    AppError::ShareInvalid(format!("Ce profil r2modman est illisible ({detail})."))
}

/// Un code r2modman : un identifiant comme `0192f0a1-…` (36 caractères).
pub fn is_code(text: &str) -> bool {
    let groups: Vec<&str> = text.split('-').collect();
    groups.len() == 5
        && groups.iter().zip([8, 4, 4, 4, 12]).all(|(g, n)| g.len() == n && g.chars().all(|c| c.is_ascii_hexdigit()))
}

/// `#r2modman` + base64 → l'archive du profil.
pub fn decode_payload(text: &str) -> AppResult<Vec<u8>> {
    let body = text.trim_start().strip_prefix(PREFIX).ok_or_else(|| invalid("en-tête manquant"))?;
    let b64: String = body.chars().filter(|c| !c.is_whitespace()).collect();
    if b64.len() > MAX_ZIP / 3 * 4 + 8 {
        return Err(AppError::ShareInvalid("Ce profil r2modman est trop gros.".into()));
    }
    STANDARD.decode(b64).map_err(|_| invalid("base64"))
}

fn version_of(y: &Yaml) -> Option<String> {
    let part = |k: &str| match &y[k] {
        Yaml::Integer(n) if *n >= 0 => Some(*n),
        Yaml::String(s) => s.trim().parse::<i64>().ok().filter(|n| *n >= 0),
        _ => None,
    };
    match y {
        Yaml::String(s) => {
            let s = s.trim();
            crate::thunderstore::api::check_version(s).ok().map(|_| s.to_string())
        }
        Yaml::Hash(_) => Some(format!("{}.{}.{}", part("major")?, part("minor")?, part("patch")?)),
        _ => None,
    }
}

/// Lit `export.r2x` : nom du profil et mods.
pub fn parse_list(text: &str) -> AppResult<(String, Vec<R2Mod>)> {
    let docs = YamlLoader::load_from_str(text).map_err(|_| invalid("liste des mods"))?;
    let doc = docs.first().ok_or_else(|| invalid("liste vide"))?;
    let name = doc["profileName"].as_str().unwrap_or("").to_string();
    let list = doc["mods"].as_vec().ok_or_else(|| invalid("liste des mods"))?;
    let mut mods = Vec::new();
    for m in list {
        let Some(full) = m["name"].as_str().map(str::trim) else { continue };
        let Some((ns, n)) = full.split_once('-') else { continue };
        if crate::thunderstore::api::check_part(ns).is_err() || crate::thunderstore::api::check_part(n).is_err() {
            continue;
        }
        let Some(version) = version_of(&m["version"]) else { continue };
        let enabled = m["enabled"].as_bool().unwrap_or(true);
        mods.push(R2Mod { full_name: full.to_string(), version, enabled });
    }
    if mods.is_empty() {
        return Err(AppError::ShareInvalid("Ce profil r2modman ne contient aucun mod.".into()));
    }
    Ok((name, mods))
}

fn config_extension_ok(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    CONFIG_EXTENSIONS.iter().any(|e| lower.ends_with(e))
}

/// Chemin sûr sous `config/` : pas de `..`, pas de chemin absolu.
fn config_path(entry: &str) -> Option<String> {
    let norm = entry.replace('\\', "/");
    let rest = norm.strip_prefix("config/")?;
    let parts: Vec<&str> = rest.split('/').collect();
    if rest.is_empty()
        || parts.iter().any(|p| p.is_empty() || *p == "." || *p == ".." || p.contains(':'))
        || !config_extension_ok(rest)
    {
        return None;
    }
    Some(parts.join("/"))
}

/// Lit une archive `.r2z`.
pub fn parse_r2z(bytes: &[u8]) -> AppResult<R2Profile> {
    if bytes.len() > MAX_ZIP * 2 {
        return Err(AppError::ShareInvalid("Ce profil r2modman est trop gros.".into()));
    }
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| invalid("archive"))?;
    let mut list = None;
    let mut configs = Vec::new();
    let mut total = 0u64;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|_| invalid("archive"))?;
        if entry.is_dir() {
            continue;
        }
        let Ok(name) = entry.name().map(|n| n.to_string()) else { continue };
        if name.eq_ignore_ascii_case("export.r2x") {
            let mut text = String::new();
            (&mut entry).take(MAX_LIST).read_to_string(&mut text).map_err(|_| invalid("liste des mods"))?;
            list = Some(text);
            continue;
        }
        let Some(path) = config_path(&name) else { continue };
        if entry.size() > MAX_CONFIG_FILE || configs.len() >= MAX_CONFIG_FILES {
            continue;
        }
        let mut data = Vec::new();
        (&mut entry).take(MAX_CONFIG_FILE + 1).read_to_end(&mut data).map_err(|_| invalid("réglages"))?;
        total += data.len() as u64;
        if data.len() as u64 > MAX_CONFIG_FILE || total > MAX_CONFIG_TOTAL {
            continue;
        }
        configs.push(ConfigFile { path, data });
    }
    let (name, mods) = parse_list(&list.ok_or_else(|| invalid("export.r2x manquant"))?)?;
    Ok(R2Profile { name, mods, configs })
}

/// Texte YAML sûr : une chaîne entre guillemets doubles (comme en JSON).
fn yaml_string(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}

/// `export.r2x` d'une liste de mods (`Équipe-Nom`, version).
pub fn list_yaml(name: &str, mods: &[(String, String)]) -> String {
    let mut out = format!("profileName: {}\nmods:\n", yaml_string(name));
    for (full, version) in mods {
        let parts: Vec<&str> = version.split('.').collect();
        let [major, minor, patch] = parts.as_slice() else { continue };
        let (Ok(major), Ok(minor), Ok(patch)) = (major.parse::<u64>(), minor.parse::<u64>(), patch.parse::<u64>()) else { continue };
        out.push_str(&format!(
            "  - name: {}\n    version:\n      major: {major}\n      minor: {minor}\n      patch: {patch}\n    enabled: true\n",
            yaml_string(full)
        ));
    }
    out
}

fn add_configs(zip: &mut zip::ZipWriter<Cursor<Vec<u8>>>, root: &Path, dir: &Path, total: &mut u64, count: &mut usize) -> AppResult<()> {
    let Ok(entries) = fs::read_dir(dir) else { return Ok(()) };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let path = e.path();
        let Ok(kind) = e.file_type() else { continue };
        if kind.is_symlink() {
            continue;
        }
        if kind.is_dir() {
            add_configs(zip, root, &path, total, count)?;
            continue;
        }
        let Ok(rel) = path.strip_prefix(root) else { continue };
        let rel = rel.to_string_lossy().replace('\\', "/");
        let Ok(meta) = e.metadata() else { continue };
        if !config_extension_ok(&rel) || meta.len() > MAX_CONFIG_FILE || *count >= MAX_CONFIG_FILES {
            continue;
        }
        if *total + meta.len() > (MAX_ZIP as u64) * 3 / 4 {
            return Err(AppError::ShareInvalid(
                "Les réglages des mods sont trop gros pour un code : partage le profil sans eux.".into(),
            ));
        }
        let Ok(data) = fs::read(&path) else { continue };
        zip.start_file(format!("config/{rel}"), SimpleFileOptions::default()).map_err(|e| AppError::Io(e.to_string()))?;
        zip.write_all(&data).map_err(|e| AppError::Io(e.to_string()))?;
        *total += data.len() as u64;
        *count += 1;
    }
    Ok(())
}

/// Archive `.r2z` : la liste des mods, et les réglages de `config_dir` s'il est donné.
pub fn build_r2z(name: &str, mods: &[(String, String)], config_dir: Option<&Path>) -> AppResult<Vec<u8>> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file("export.r2x", SimpleFileOptions::default()).map_err(|e| AppError::Io(e.to_string()))?;
    zip.write_all(list_yaml(name, mods).as_bytes()).map_err(|e| AppError::Io(e.to_string()))?;
    if let Some(dir) = config_dir.filter(|d| d.is_dir()) {
        let (mut total, mut count) = (0, 0);
        add_configs(&mut zip, dir, dir, &mut total, &mut count)?;
    }
    let bytes = zip.finish().map_err(|e| AppError::Io(e.to_string()))?.into_inner();
    if bytes.len() > MAX_ZIP {
        return Err(AppError::ShareInvalid("Ce profil est trop gros pour un code : partage-le sans les réglages des mods.".into()));
    }
    Ok(bytes)
}
