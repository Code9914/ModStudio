//! Profils partagés : la liste des mods activés d'un jeu, à envoyer à un ami
//! (un code à coller ou un fichier) qui l'installe d'un coup.
//!
//! Le code ModStudio (`MS1-…`) contient toute la liste, compressée : il
//! marche hors ligne et ne passe par aucun serveur. ModStudio lit aussi les
//! profils de r2modman (et de Gale ou Thunderstore Mod Manager) : codes
//! publiés sur Thunderstore et fichiers `.r2z`, et sait en publier pour les
//! jeux de Thunderstore.

pub mod r2modman;
#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;
use flate2::Compression;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::commands::AppState;
use crate::error::{AppError, AppResult};
use crate::installer::{GameInstall, Library};

/// Début d'un code de profil ModStudio (version 1 du format).
pub const CODE_PREFIX: &str = "MS1-";
const FORMAT: &str = "modstudio-profile";
const VERSION: u32 = 1;
/// Longueur maximale d'un code collé, et taille de la liste décompressée.
const MAX_CODE: usize = 512 * 1024;
const MAX_JSON: u64 = 2 * 1024 * 1024;
/// Fichier de profil lu (ModStudio ou `.r2z`).
const MAX_FILE: u64 = 40 * 1024 * 1024;
const MAX_MODS: usize = 2000;

/// D'où viennent les mods du profil.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Modio,
    Nexus,
    Ts,
    Cl,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SharedGame {
    /// Identifiant du jeu sur mod.io ou Nexus Mods (0 pour les autres).
    #[serde(default)]
    pub id: i64,
    /// Domaine Nexus, jeu Thunderstore (vide : inconnu, profil r2modman) ou
    /// jeu des communautés (`raft`…).
    #[serde(default)]
    pub key: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SharedMod {
    /// mod.io et Nexus : numéro du mod ; Thunderstore : `Équipe-Nom` (ou
    /// `rml:<id>`) ; communautés : identifiant dans la liste.
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Fichier installé (mod.io : modfile ; Nexus : fichier).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<i64>,
    /// Installé comme mod nécessaire d'un autre.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub dep: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SharedProfile {
    pub name: String,
    pub source: Source,
    pub game: SharedGame,
    pub mods: Vec<SharedMod>,
}

/// Le profil tel qu'il est écrit dans un code ou un fichier.
#[derive(Serialize, Deserialize)]
struct Wire {
    format: String,
    v: u32,
    #[serde(flatten)]
    profile: SharedProfile,
}

/// Profil lu, prêt à être importé.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareImport {
    pub profile: SharedProfile,
    /// `modstudio` ou `r2modman`.
    pub origin: &'static str,
    /// Fichiers de réglages joints (profils r2modman), appliqués sur demande.
    pub configs: usize,
    /// Pour appliquer ces réglages : le profil reste en mémoire.
    pub token: Option<String>,
    /// Mods désactivés dans le profil r2modman : pas installés.
    pub skipped: Vec<String>,
}

/// Profil exporté : la liste et son code.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareExport {
    pub profile: SharedProfile,
    pub code: String,
    /// Mods laissés de côté (désactivés, ou sans source connue).
    pub left_out: usize,
}

/// Réglages des profils r2modman lus, en attendant d'être appliqués.
#[derive(Default)]
pub struct ShareState {
    pending: Mutex<Vec<(String, Vec<r2modman::ConfigFile>)>>,
    counter: Mutex<u64>,
}

impl ShareState {
    fn keep(&self, configs: Vec<r2modman::ConfigFile>) -> Option<String> {
        if configs.is_empty() {
            return None;
        }
        let mut counter = self.counter.lock().expect("counter");
        *counter += 1;
        let token = format!("r2-{}-{}", std::process::id(), *counter);
        let mut pending = self.pending.lock().expect("pending");
        pending.push((token.clone(), configs));
        // Seuls les derniers profils lus restent en mémoire.
        while pending.len() > 4 {
            pending.remove(0);
        }
        Some(token)
    }

    fn get(&self, token: &str) -> Option<Vec<r2modman::ConfigFile>> {
        let pending = self.pending.lock().expect("pending");
        pending.iter().find(|(t, _)| t == token).map(|(_, c)| c.clone())
    }

    /// Réglages appliqués : plus besoin de les garder.
    fn forget(&self, token: &str) {
        self.pending.lock().expect("pending").retain(|(t, _)| t != token);
    }
}

// ---------------------------------------------------------------- Vérifications

fn clean_text(s: &str, max: usize) -> String {
    s.chars().filter(|c| !c.is_control()).take(max).collect::<String>().trim().to_string()
}

fn key_ok(key: &str) -> bool {
    key.len() <= 100 && key.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

fn id_ok(source: Source, id: &str) -> bool {
    match source {
        Source::Modio | Source::Nexus => id.parse::<i64>().is_ok_and(|n| n > 0),
        Source::Ts => {
            if let Some(rml) = id.strip_prefix("rml:") {
                return !rml.is_empty() && rml.len() <= 200 && rml.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
            }
            id.split_once('-').is_some_and(|(ns, name)| {
                crate::thunderstore::api::check_part(ns).is_ok() && crate::thunderstore::api::check_part(name).is_ok()
            })
        }
        Source::Cl => !id.trim().is_empty() && id.len() <= 200 && !id.chars().any(char::is_control),
    }
}

/// Remet un profil lu dans des limites sûres ; refuse ce qui ne peut pas être
/// installé.
pub fn validate(mut p: SharedProfile) -> AppResult<SharedProfile> {
    let bad = |m: &str| AppError::ShareInvalid(crate::tr!("Ce profil est abîmé : {m}.", "This profile is damaged: {m}."));
    p.game.name = clean_text(&p.game.name, 100);
    p.game.key = p.game.key.trim().to_string();
    if !key_ok(&p.game.key) {
        return Err(bad(crate::i18n::pick("jeu inconnu", "unknown game")));
    }
    match p.source {
        Source::Modio if p.game.id <= 0 => return Err(bad(crate::i18n::pick("jeu mod.io inconnu", "unknown mod.io game"))),
        Source::Nexus if p.game.id <= 0 || p.game.key.is_empty() => return Err(bad(crate::i18n::pick("jeu Nexus Mods inconnu", "unknown Nexus Mods game"))),
        Source::Cl if p.game.key.is_empty() => return Err(bad(crate::i18n::pick("jeu inconnu", "unknown game"))),
        _ => {}
    }
    if p.game.name.is_empty() {
        p.game.name = if p.game.key.is_empty() { crate::i18n::pick("Jeu", "Game").into() } else { p.game.key.clone() };
    }
    // Même limite que les profils d'un jeu.
    p.name = clean_text(&p.name, 40);
    if p.name.is_empty() {
        p.name = p.game.name.chars().take(60).collect();
    }
    if p.mods.len() > MAX_MODS {
        return Err(bad(crate::i18n::pick("trop de mods", "too many mods")));
    }
    let mut seen = HashSet::new();
    let mut mods = Vec::with_capacity(p.mods.len());
    for mut m in p.mods {
        m.id = m.id.trim().to_string();
        if !id_ok(p.source, &m.id) {
            return Err(bad(&crate::tr!("mod « {} » illisible", "unreadable mod \"{}\"", clean_text(&m.id, 60))));
        }
        if !seen.insert(m.id.to_lowercase()) {
            continue;
        }
        m.name = clean_text(&m.name, 200);
        if m.name.is_empty() {
            m.name = m.id.clone();
        }
        m.version = m.version.map(|v| clean_text(&v, 64)).filter(|v| !v.is_empty());
        m.file = m.file.filter(|f| *f > 0);
        mods.push(m);
    }
    if mods.is_empty() {
        return Err(AppError::ShareInvalid(crate::i18n::pick("Ce profil ne contient aucun mod.", "This profile doesn't contain any mods.").into()));
    }
    p.mods = mods;
    Ok(p)
}

// ---------------------------------------------------------------- Code et fichier

/// Le code à partager : la liste compressée, en caractères sûrs pour une
/// discussion (lettres, chiffres, `-` et `_`).
pub fn encode(profile: &SharedProfile) -> AppResult<String> {
    let wire = Wire { format: FORMAT.into(), v: VERSION, profile: profile.clone() };
    let json = serde_json::to_vec(&wire).map_err(|e| AppError::Io(e.to_string()))?;
    let mut z = DeflateEncoder::new(Vec::new(), Compression::best());
    z.write_all(&json).map_err(|e| AppError::Io(e.to_string()))?;
    let packed = z.finish().map_err(|e| AppError::Io(e.to_string()))?;
    Ok(format!("{CODE_PREFIX}{}", URL_SAFE_NO_PAD.encode(packed)))
}

fn from_json(bytes: &[u8]) -> AppResult<SharedProfile> {
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| AppError::ShareInvalid(crate::i18n::pick("Ce fichier n'est pas un profil ModStudio.", "This file isn't a ModStudio profile.").into()))?;
    if value.get("format").and_then(|f| f.as_str()) != Some(FORMAT) {
        return Err(AppError::ShareInvalid(crate::i18n::pick("Ce fichier n'est pas un profil ModStudio.", "This file isn't a ModStudio profile.").into()));
    }
    let v = value.get("v").and_then(|v| v.as_u64()).unwrap_or(0);
    if v > u64::from(VERSION) {
        return Err(AppError::ShareInvalid(
            crate::i18n::pick(
                "Ce profil vient d'une version plus récente de ModStudio : mets ModStudio à jour pour l'importer.",
                "This profile comes from a newer version of ModStudio: update ModStudio to import it.",
            )
            .into(),
        ));
    }
    let wire: Wire = serde_json::from_value(value).map_err(|e| AppError::ShareInvalid(crate::tr!("Ce profil est abîmé ({e}).", "This profile is damaged ({e}).")))?;
    validate(wire.profile)
}

/// Le code ModStudio trouvé dans un texte collé (même au milieu d'un message).
pub fn find_code(text: &str) -> Option<&str> {
    let upper = text.to_ascii_uppercase();
    let start = upper.find(CODE_PREFIX)? + CODE_PREFIX.len();
    let rest = &text[start..];
    let end = rest.find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_')).unwrap_or(rest.len());
    Some(&rest[..end]).filter(|c| !c.is_empty())
}

pub fn decode(text: &str) -> AppResult<SharedProfile> {
    let invalid = || {
        AppError::ShareInvalid(
            crate::i18n::pick(
                "Ce code de profil est incomplet ou abîmé : copie-le en entier.",
                "This profile code is incomplete or damaged: copy all of it.",
            )
            .into(),
        )
    };
    if text.len() > MAX_CODE {
        return Err(AppError::ShareInvalid(crate::i18n::pick("Ce code est trop long pour être un profil.", "This code is too long to be a profile.").into()));
    }
    let body = find_code(text).ok_or_else(invalid)?;
    let packed = URL_SAFE_NO_PAD.decode(body).map_err(|_| invalid())?;
    let mut json = Vec::new();
    DeflateDecoder::new(packed.as_slice()).take(MAX_JSON + 1).read_to_end(&mut json).map_err(|_| invalid())?;
    if json.len() as u64 > MAX_JSON {
        return Err(AppError::ShareInvalid(crate::i18n::pick("Ce profil est trop gros.", "This profile is too big.").into()));
    }
    from_json(&json)
}

/// Fichier de profil ModStudio : la même liste, lisible.
pub fn to_file(profile: &SharedProfile) -> AppResult<Vec<u8>> {
    let wire = Wire { format: FORMAT.into(), v: VERSION, profile: profile.clone() };
    serde_json::to_vec_pretty(&wire).map_err(|e| AppError::Io(e.to_string()))
}

/// Ce qu'un texte collé désigne : un code ModStudio, un code r2modman, ou rien.
pub enum Pasted<'a> {
    ModStudio,
    R2modman(&'a str),
}

pub fn classify(text: &str) -> AppResult<Pasted<'_>> {
    let t = text.trim().trim_matches(|c| matches!(c, '`' | '"' | '\''));
    if find_code(t).is_some() {
        return Ok(Pasted::ModStudio);
    }
    if r2modman::is_code(t) {
        return Ok(Pasted::R2modman(t));
    }
    Err(AppError::ShareInvalid(
        crate::i18n::pick(
            "Ce n'est pas un code de profil : colle un code ModStudio (« MS1-… ») ou un code r2modman.",
            "This isn't a profile code: paste a ModStudio code (\"MS1-…\") or an r2modman code.",
        )
        .into(),
    ))
}

// ---------------------------------------------------------------- Export

/// Mods activés d'un jeu, à partager. Les chargeurs et mods nécessaires en
/// font partie : leur version compte autant que celle des autres.
pub fn export(source: Source, game: &GameInstall) -> (SharedProfile, usize) {
    let mut left_out = 0;
    let mut key = String::new();
    let mut mods = Vec::new();
    match source {
        Source::Modio => {}
        Source::Nexus => key = game.domain.clone().unwrap_or_default(),
        Source::Ts => key = game.domain.clone().unwrap_or_default(),
        Source::Cl => {
            if let Some(def) = crate::lists::games::GAMES.iter().find(|d| crate::lists::game_id(d.id) == game.game_id) {
                key = def.id.to_string();
            }
        }
    }
    for m in game.mods.values() {
        if m.disabled {
            left_out += 1;
            continue;
        }
        let id = match source {
            Source::Modio | Source::Nexus => Some(m.mod_id.to_string()),
            Source::Ts => m.file_name.clone(),
            Source::Cl => m.file_name.as_deref().and_then(|f| f.split_once(':')).map(|(_, id)| id.to_string()),
        };
        let Some(id) = id.filter(|id| id_ok(source, id)) else {
            left_out += 1;
            continue;
        };
        let file = matches!(source, Source::Modio | Source::Nexus).then_some(m.modfile_id).filter(|f| *f > 0);
        mods.push(SharedMod { id, name: m.name.clone(), version: m.version.clone(), file, dep: m.auto });
    }
    // Les mods nécessaires d'abord : ils s'installent avant ceux qui en ont besoin.
    mods.sort_by(|a, b| b.dep.cmp(&a.dep).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    let name = game.active_profile.clone().unwrap_or_else(|| game.game_name.clone());
    let profile = SharedProfile {
        name,
        source,
        game: SharedGame { id: if matches!(source, Source::Modio | Source::Nexus) { game.game_id } else { 0 }, key, name: game.game_name.clone() },
        mods,
    };
    (profile, left_out)
}

fn library_game(lib: &Mutex<Library>, game_id: i64) -> AppResult<GameInstall> {
    lib.lock().expect("library").games.get(&game_id).cloned().ok_or(AppError::NotFound)
}

fn game_of(state: &AppState, source: Source, game_id: i64) -> AppResult<GameInstall> {
    match source {
        Source::Modio => library_game(&state.library, game_id),
        Source::Nexus => library_game(&state.nexus.library, game_id),
        Source::Ts => library_game(&state.thunderstore.library, game_id),
        Source::Cl => library_game(&state.lists.library, game_id),
    }
}

// ---------------------------------------------------------------- Réglages r2modman

fn no_bepinex_configs() -> &'static str {
    crate::i18n::pick(
        "Ce jeu ne charge pas ses mods avec BepInEx : les réglages du profil ne s'y appliquent pas.",
        "This game doesn't load its mods with BepInEx: the profile's settings don't apply to it.",
    )
}

/// Où vont les réglages d'un profil r2modman : `BepInEx/config` du jeu.
fn config_dir(game: &GameInstall) -> Option<PathBuf> {
    let dir = game.folder.join("BepInEx").join("config");
    game.folder.join("BepInEx").is_dir().then_some(dir)
}

fn safe_rel(rel: &str) -> Option<PathBuf> {
    let p = Path::new(rel);
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::Normal(s) => out.push(s),
            _ => return None,
        }
    }
    (!out.as_os_str().is_empty()).then_some(out)
}

/// Écrit les réglages dans `BepInEx/config` ; ceux qu'ils remplacent sont
/// copiés avant dans `backup` (dossier de ModStudio).
pub fn write_configs(game: &GameInstall, files: &[r2modman::ConfigFile], backup: &Path) -> AppResult<ConfigsApplied> {
    let dir = config_dir(game).ok_or_else(|| {
        AppError::ShareInvalid(no_bepinex_configs().into())
    })?;
    fs::create_dir_all(&dir).map_err(|e| AppError::Io(e.to_string()))?;
    let is_link = |p: &Path| fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_symlink());
    let mut written = 0;
    let mut saved = 0;
    'files: for f in files {
        let Some(rel) = safe_rel(&f.path) else { continue };
        // Dossiers créés un à un : un lien symbolique en chemin pourrait mener
        // hors de `BepInEx/config`, il arrête tout pour ce fichier.
        let mut at = dir.clone();
        let parts: Vec<_> = rel.components().collect();
        for c in &parts[..parts.len() - 1] {
            at.push(c);
            if is_link(&at) || at.is_file() {
                continue 'files;
            }
            if !at.exists() {
                fs::create_dir(&at).map_err(|e| AppError::Io(e.to_string()))?;
            }
        }
        let dest = dir.join(&rel);
        if dest.is_dir() || is_link(&dest) {
            continue;
        }
        if dest.is_file() {
            if fs::read(&dest).is_ok_and(|old| old == f.data) {
                continue;
            }
            let copy = backup.join(&rel);
            if let Some(parent) = copy.parent() {
                fs::create_dir_all(parent).map_err(|e| AppError::Io(e.to_string()))?;
            }
            fs::copy(&dest, &copy).map_err(|e| AppError::Io(e.to_string()))?;
            saved += 1;
        }
        // Fichier temporaire tout neuf (jamais un lien laissé là) puis renommé.
        let tmp = dest.with_extension(format!("modstudio-{}.tmp", std::process::id()));
        if is_link(&tmp) || tmp.is_dir() {
            continue;
        }
        let _ = fs::remove_file(&tmp);
        let mut out = fs::OpenOptions::new().write(true).create_new(true).open(&tmp).map_err(|e| AppError::Io(e.to_string()))?;
        out.write_all(&f.data).map_err(|e| AppError::Io(e.to_string()))?;
        drop(out);
        fs::rename(&tmp, &dest).map_err(|e| AppError::Io(e.to_string()))?;
        written += 1;
    }
    Ok(ConfigsApplied { written, backup: (saved > 0).then(|| backup.to_string_lossy().into_owned()) })
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigsApplied {
    pub written: usize,
    /// Dossier qui garde les réglages remplacés.
    pub backup: Option<String>,
}

// ---------------------------------------------------------------- Lecture

fn from_r2(state: &AppState, r2: r2modman::R2Profile) -> AppResult<ShareImport> {
    let skipped: Vec<String> = r2.mods.iter().filter(|m| !m.enabled).map(|m| m.full_name.clone()).collect();
    let mods: Vec<SharedMod> = r2
        .mods
        .into_iter()
        .filter(|m| m.enabled)
        .map(|m| SharedMod {
            name: m.full_name.split_once('-').map(|(_, n)| n.replace('_', " ")).unwrap_or_else(|| m.full_name.clone()),
            id: m.full_name,
            version: Some(m.version),
            file: None,
            dep: false,
        })
        .collect();
    let profile = validate(SharedProfile {
        name: r2.name,
        source: Source::Ts,
        // Gale écrit le jeu du profil ; r2modman non (il est alors deviné).
        game: SharedGame { id: 0, key: r2.community.unwrap_or_default(), name: String::new() },
        mods,
    })?;
    let configs = r2.configs.len();
    let token = state.share.keep(r2.configs);
    Ok(ShareImport { profile, origin: "r2modman", configs, token, skipped })
}

/// Devine le jeu Thunderstore d'un profil r2modman : celui où ses mods sont
/// publiés (les chargeurs, publiés pour beaucoup de jeux, ne comptent pas).
async fn guess_community(state: &AppState, import: &mut ShareImport) {
    if !import.profile.game.key.is_empty() {
        return;
    }
    let candidates: Vec<(String, String)> = import
        .profile
        .mods
        .iter()
        .filter_map(|m| m.id.split_once('-').map(|(ns, n)| (ns.to_string(), n.to_string())))
        .filter(|(ns, n)| !n.to_ascii_lowercase().starts_with("bepinexpack") && !ns.eq_ignore_ascii_case("BepInEx"))
        .take(3)
        .collect();
    let mut common: Option<Vec<String>> = None;
    for (ns, n) in candidates {
        let Ok(list) = state.thunderstore.client.communities_of(&ns, &n).await else { continue };
        common = Some(match common {
            None => list,
            Some(prev) => prev.into_iter().filter(|c| list.contains(c)).collect(),
        });
        if common.as_ref().is_some_and(|c| c.len() <= 1) {
            break;
        }
    }
    let Some(found) = common.filter(|c| !c.is_empty()) else { return };
    // Plusieurs jeux possibles : celui déjà configuré dans ModStudio.
    let configured: HashMap<String, String> = state
        .thunderstore
        .library
        .lock()
        .expect("library")
        .games
        .values()
        .filter_map(|g| g.domain.clone().map(|d| (d, g.game_name.clone())))
        .collect();
    let pick = found.iter().find(|c| configured.contains_key(*c)).or(found.first()).cloned();
    if let Some(c) = pick {
        import.profile.game.name = configured.get(&c).cloned().unwrap_or_default();
        import.profile.game.key = c;
    }
}

/// Lecture de l'archive hors du fil des commandes (décompression bornée, mais longue).
async fn parse_r2z(bytes: Vec<u8>) -> AppResult<r2modman::R2Profile> {
    tokio::task::spawn_blocking(move || r2modman::parse_r2z(&bytes))
        .await
        .map_err(|e| AppError::Io(e.to_string()))?
}

async fn read_r2modman_code(state: &AppState, code: &str) -> AppResult<ShareImport> {
    let text = state.thunderstore.client.legacy_profile(code).await?;
    let zip = r2modman::decode_payload(&text)?;
    let mut import = from_r2(state, parse_r2z(zip).await?)?;
    guess_community(state, &mut import).await;
    Ok(import)
}

// ---------------------------------------------------------------- Commandes

/// Profil d'un jeu installé, avec son code.
#[tauri::command]
pub fn share_export(state: State<'_, AppState>, source: Source, game_id: i64) -> AppResult<ShareExport> {
    let game = game_of(&state, source, game_id)?;
    let (profile, left_out) = export(source, &game);
    if profile.mods.is_empty() {
        return Err(AppError::ShareInvalid(crate::i18n::pick("Aucun mod activé à partager pour ce jeu.", "No enabled mods to share for this game.").into()));
    }
    if matches!(source, Source::Nexus | Source::Ts | Source::Cl) && profile.game.key.is_empty() {
        return Err(AppError::ShareInvalid(
            crate::i18n::pick("ModStudio ne retrouve pas ce jeu : impossible de le partager.", "ModStudio can't find this game, so it can't be shared.").into(),
        ));
    }
    let code = encode(&profile)?;
    Ok(ShareExport { profile, code, left_out })
}

/// Lit un code collé : code ModStudio, ou code r2modman (téléchargé depuis Thunderstore).
#[tauri::command]
pub async fn share_decode(state: State<'_, AppState>, text: String) -> AppResult<ShareImport> {
    match classify(&text)? {
        Pasted::ModStudio => Ok(ShareImport { profile: decode(&text)?, origin: "modstudio", configs: 0, token: None, skipped: Vec::new() }),
        Pasted::R2modman(code) => read_r2modman_code(&state, code).await,
    }
}

/// Lit un fichier de profil choisi par l'utilisateur : `.modstudio` ou `.r2z`.
#[tauri::command]
pub async fn share_open_file(state: State<'_, AppState>, path: PathBuf) -> AppResult<ShareImport> {
    let meta = fs::metadata(&path).map_err(|e| AppError::Io(e.to_string()))?;
    if !meta.is_file() || meta.len() > MAX_FILE {
        return Err(AppError::ShareInvalid(
            crate::i18n::pick("Ce fichier n'est pas un profil (ou il est trop gros).", "This file isn't a profile (or it's too big).").into(),
        ));
    }
    let bytes = fs::read(&path).map_err(|e| AppError::Io(e.to_string()))?;
    if bytes.starts_with(b"PK") {
        let mut import = from_r2(&state, parse_r2z(bytes).await?)?;
        guess_community(&state, &mut import).await;
        return Ok(import);
    }
    let text = String::from_utf8_lossy(bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes)).into_owned();
    let trimmed = text.trim_start();
    if trimmed.starts_with(r2modman::PREFIX) {
        let mut import = from_r2(&state, parse_r2z(r2modman::decode_payload(trimmed)?).await?)?;
        guess_community(&state, &mut import).await;
        return Ok(import);
    }
    let profile = if trimmed.starts_with('{') { from_json(trimmed.as_bytes())? } else { decode(trimmed)? };
    Ok(ShareImport { profile, origin: "modstudio", configs: 0, token: None, skipped: Vec::new() })
}

/// Enregistre un profil dans le fichier choisi par l'utilisateur.
#[tauri::command]
pub fn share_save_file(path: PathBuf, profile: SharedProfile) -> AppResult<()> {
    let profile = validate(profile)?;
    let bytes = to_file(&profile)?;
    fs::write(&path, bytes).map_err(|e| AppError::Io(e.to_string()))
}

/// Publie sur Thunderstore un profil r2modman des mods activés d'un jeu
/// Thunderstore (et de ses réglages) : renvoie le code à partager.
#[tauri::command]
pub async fn share_r2modman_create(state: State<'_, AppState>, game_id: i64, name: String, include_configs: bool) -> AppResult<String> {
    let game = game_of(&state, Source::Ts, game_id)?;
    let mods: Vec<(String, String)> = game
        .mods
        .values()
        .filter(|m| !m.disabled)
        .filter_map(|m| Some((m.file_name.clone().filter(|f| !f.starts_with("rml:"))?, m.version.clone()?)))
        .collect();
    if mods.is_empty() {
        return Err(AppError::ShareInvalid(
            crate::i18n::pick("Aucun mod de Thunderstore activé à partager pour ce jeu.", "No enabled Thunderstore mods to share for this game.").into(),
        ));
    }
    let name = clean_text(&name, 60);
    let name = if name.is_empty() { game.game_name.clone() } else { name };
    let configs = if include_configs { config_dir(&game) } else { None };
    let zip = r2modman::build_r2z(&name, &mods, configs.as_deref())?;
    let payload = format!("{}\n{}", r2modman::PREFIX, base64::engine::general_purpose::STANDARD.encode(zip));
    state.thunderstore.client.create_legacy_profile(payload).await
}

/// Applique les réglages d'un profil r2modman lu plus tôt au jeu Thunderstore choisi.
#[tauri::command]
pub fn share_apply_configs(state: State<'_, AppState>, token: String, game_id: i64) -> AppResult<ConfigsApplied> {
    let game = game_of(&state, Source::Ts, game_id)?;
    if config_dir(&game).is_none() {
        return Err(AppError::ShareInvalid(no_bepinex_configs().into()));
    }
    let files = state
        .share
        .get(&token)
        .ok_or_else(|| {
            AppError::ShareInvalid(
                crate::i18n::pick(
                    "Les réglages de ce profil ne sont plus en mémoire : importe-le à nouveau.",
                    "This profile's settings are no longer in memory: import it again.",
                )
                .into(),
            )
        })?;
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let backup = state.library_path.with_file_name("profile-backups").join(format!("{game_id}-{stamp}"));
    let applied = write_configs(&game, &files, &backup)?;
    state.share.forget(&token);
    Ok(applied)
}
