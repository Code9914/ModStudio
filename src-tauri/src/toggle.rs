//! Activer ou désactiver un mod sans le désinstaller, et profils de mods.
//!
//! Désactiver un mod met ses fichiers de côté : dans un dossier caché à la
//! racine du dossier du jeu pour les jeux rangés selon leurs règles (Unreal,
//! Unity, Bethesda, BONELAB : ces dossiers ne sont jamais lus par le jeu), et
//! dans les données de ModStudio pour les autres (un jeu peut lire tout son
//! dossier de mods). Les réactiver vérifie d'abord qu'aucun autre mod n'a pris
//! leur place entre-temps.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::installer::{self, GameInstall, InstalledMod, Library};

/// Dossier caché qui garde les mods désactivés, dans le dossier du jeu.
pub const DISABLED_DIR: &str = ".modstudio-disabled";

/// Ensemble de mods activés d'un jeu, à retrouver d'un clic.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub name: String,
    /// Mods activés dans ce profil.
    #[serde(default)]
    pub mods: Vec<i64>,
}

/// Où mettre de côté les mods désactivés d'une bibliothèque.
pub struct Ctx<'a> {
    /// Dossier des mods désactivés dans les données de ModStudio.
    pub store_root: &'a Path,
    /// `modio` ou `nexus`.
    pub source: &'a str,
}

/// Fichiers d'un mod par dossier : (clé du dossier, fichiers, dossiers créés).
fn parts_of(record: &InstalledMod) -> Vec<(String, Vec<String>, Vec<String>)> {
    let mut out = vec![(String::new(), record.files.clone(), record.dirs.clone())];
    for p in &record.parts {
        out.push((p.key.clone(), p.files.clone(), p.dirs.clone()));
    }
    out
}

/// Les fichiers du mod désactivé sont-ils (ou seront-ils) gardés dans le
/// dossier du jeu ? L'endroit choisi à la désactivation est mémorisé dans le
/// mod : il ne change pas si les règles du jeu changent entre-temps.
fn kept_in_game(game: &GameInstall, record: &InstalledMod) -> bool {
    match record.disabled_in.as_deref() {
        Some("game") => true,
        Some("app") => false,
        _ => game.rules.is_some(),
    }
}

/// Dossier qui garde les fichiers d'un mod désactivé, pour un dossier du jeu.
pub fn store_for(game: &GameInstall, key: &str, mod_id: i64, in_game: bool, ctx: &Ctx) -> Option<PathBuf> {
    let folder = game.folder_for(key)?;
    Some(if in_game {
        folder.join(DISABLED_DIR).join(format!("{}-{mod_id}", ctx.source))
    } else {
        ctx.store_root
            .join(ctx.source)
            .join(game.game_id.to_string())
            .join(mod_id.to_string())
            .join(if key.is_empty() { "main" } else { key })
    })
}

/// Limite des dossiers vides à retirer au-dessus d'un dossier de rangement.
fn store_stop(folder: &Path, in_game: bool, ctx: &Ctx) -> PathBuf {
    if in_game { folder.join(DISABLED_DIR) } else { ctx.store_root.to_path_buf() }
}

/// Déplace des fichiers ; en cas d'échec, remet ceux déjà déplacés.
fn move_all(moves: &[(PathBuf, PathBuf, String)]) -> AppResult<()> {
    let mut done: Vec<(&PathBuf, &PathBuf)> = Vec::new();
    for (src, dst, rel) in moves {
        let result = (|| -> std::io::Result<()> {
            if let Some(parent) = dst.parent() {
                fs::create_dir_all(parent)?;
            }
            installer::move_file_io(src, dst)
        })();
        if let Err(e) = result {
            for (s, d) in done.into_iter().rev() {
                let _ = installer::move_file_io(d, s);
            }
            return Err(AppError::Io(crate::tr!("{rel} : {e} (le jeu est-il ouvert ?)", "{rel}: {e} (is the game running?)")));
        }
        done.push((src, dst));
    }
    Ok(())
}

/// Retire les dossiers vides, du plus profond au moins profond.
fn prune(base: &Path, dirs: &[String]) {
    for d in installer::deepest_first(dirs) {
        if let Some(abs) = installer::resolve_rel(base, &d) {
            let _ = fs::remove_dir(abs); // seulement s'il est vide
        }
    }
}

/// Retire un dossier de rangement et ses parents vides jusqu'à `stop`.
fn remove_store(store: &Path, stop: &Path) {
    let _ = fs::remove_dir_all(store);
    let mut dir = store.parent();
    while let Some(d) = dir {
        if d == stop || !d.starts_with(stop) || fs::remove_dir(d).is_err() {
            break;
        }
        dir = d.parent();
    }
}

/// Le mod ne revendique plus que les fichiers `kept` (par clé de dossier) :
/// un fichier qui n'a pas pu être mis de côté ou remis ne lui appartient plus,
/// pour ne jamais supprimer celui d'un autre mod à sa place.
fn keep_only(record: &mut InstalledMod, kept: &HashMap<String, Vec<String>>) {
    let main: Vec<String> = kept.get("").cloned().unwrap_or_default();
    record.files.retain(|f| main.contains(f));
    for p in &mut record.parts {
        let list = kept.get(&p.key).cloned().unwrap_or_default();
        p.files.retain(|f| list.contains(f));
    }
}

/// Désactive un mod : ses fichiers sont mis de côté.
pub fn disable(game: &mut GameInstall, mod_id: i64, ctx: &Ctx) -> AppResult<()> {
    let record = game.mods.get(&mod_id).cloned().ok_or(AppError::NotFound)?;
    if record.disabled {
        return Ok(());
    }
    let in_game = game.rules.is_some();
    let mut moves = Vec::new();
    let mut kept: HashMap<String, Vec<String>> = HashMap::new();
    for (key, files, _) in parts_of(&record) {
        let (Some(folder), Some(store)) = (game.folder_for(&key), store_for(game, &key, mod_id, in_game, ctx)) else { continue };
        for rel in files {
            let (Some(src), Some(dst)) = (installer::resolve_rel(folder, &rel), installer::resolve_rel(&store, &rel)) else {
                continue;
            };
            // Fichier déjà supprimé à la main : rien à mettre de côté, le mod ne le revendique plus.
            if src.is_file() {
                kept.entry(key.clone()).or_default().push(rel.clone());
                moves.push((src, dst, rel));
            }
        }
    }
    move_all(&moves)?;
    for (key, _, dirs) in parts_of(&record) {
        if let Some(folder) = game.folder_for(&key) {
            prune(folder, &dirs);
        }
    }
    if let Some(m) = game.mods.get_mut(&mod_id) {
        keep_only(m, &kept);
        m.disabled = true;
        m.disabled_in = Some(if in_game { "game" } else { "app" }.to_string());
    }
    sync_active(game);
    Ok(())
}

/// Réactive un mod. `others` : fichiers des autres mods activés (des deux
/// bibliothèques), par dossier ; ils ne sont jamais écrasés.
pub fn enable(game: &mut GameInstall, mod_id: i64, ctx: &Ctx, others: &HashMap<PathBuf, BTreeMap<String, String>>) -> AppResult<()> {
    let record = game.mods.get(&mod_id).cloned().ok_or(AppError::NotFound)?;
    if !record.disabled {
        return Ok(());
    }
    let in_game = kept_in_game(game, &record);
    let mut moves = Vec::new();
    let mut conflicts = Vec::new();
    let mut stores = Vec::new();
    let mut kept: HashMap<String, Vec<String>> = HashMap::new();
    for (key, files, _) in parts_of(&record) {
        let (Some(folder), Some(store)) = (game.folder_for(&key), store_for(game, &key, mod_id, in_game, ctx)) else { continue };
        let owned = others.get(folder);
        for rel in files {
            let (Some(dst), Some(src)) = (installer::resolve_rel(folder, &rel), installer::resolve_rel(&store, &rel)) else {
                continue;
            };
            // Absent des fichiers mis de côté : il ne revient pas, et le mod ne le revendique plus.
            if !src.is_file() {
                continue;
            }
            if let Some(owner) = owned.and_then(|o| o.get(&rel)) {
                conflicts.push(crate::tr!("{rel} (installé par « {owner} »)", "{rel} (installed by \"{owner}\")"));
            } else if dst.exists() {
                conflicts.push(rel.clone());
            } else {
                kept.entry(key.clone()).or_default().push(rel.clone());
                moves.push((src, dst, rel));
            }
        }
        stores.push((store, folder.to_path_buf()));
    }
    if !conflicts.is_empty() {
        let more = conflicts.len().saturating_sub(3);
        let mut text = conflicts.into_iter().take(3).collect::<Vec<_>>().join(", ");
        if more > 0 {
            text.push_str(&crate::tr!(" et {more} autres", " and {more} others"));
        }
        return Err(AppError::Conflict(text));
    }
    move_all(&moves)?;
    for (store, folder) in stores {
        remove_store(&store, &store_stop(&folder, in_game, ctx));
        if in_game {
            let _ = fs::remove_dir(folder.join(DISABLED_DIR));
        }
    }
    if let Some(m) = game.mods.get_mut(&mod_id) {
        keep_only(m, &kept);
        m.disabled = false;
        m.disabled_in = None;
    }
    sync_active(game);
    Ok(())
}

/// Supprime les fichiers mis de côté d'un mod désactivé (désinstallation, ou
/// nouvelle version une fois installée). Ses chemins dans le dossier du jeu ne
/// sont pas touchés : un autre mod a pu les prendre.
pub fn discard(game: &GameInstall, record: &InstalledMod, ctx: &Ctx) {
    let in_game = kept_in_game(game, record);
    for (key, _, _) in parts_of(record) {
        let (Some(folder), Some(store)) = (game.folder_for(&key), store_for(game, &key, record.mod_id, in_game, ctx)) else { continue };
        remove_store(&store, &store_stop(folder, in_game, ctx));
        if in_game {
            let _ = fs::remove_dir(folder.join(DISABLED_DIR));
        }
    }
}

/// Fichiers en place des autres mods activés : ceux du jeu (sauf `mod_id`)
/// et ceux des bibliothèques `rest` (l'autre source, les autres jeux).
pub fn others_map(game: &GameInstall, mod_id: i64, rest: &[&Library]) -> HashMap<PathBuf, BTreeMap<String, String>> {
    let mut map: HashMap<PathBuf, BTreeMap<String, String>> = HashMap::new();
    for (folder, rel, owner) in game.owned_files(mod_id) {
        map.entry(folder).or_default().insert(rel, owner);
    }
    for lib in rest {
        for other in lib.games.values() {
            for (folder, rel, owner) in other.owned_files(-1) {
                map.entry(folder).or_default().insert(rel, owner);
            }
        }
    }
    map
}

/// Résultat d'un changement de profil.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileApplied {
    pub game: GameInstall,
    pub issues: Vec<ProfileIssue>,
}

/// Opération sur les profils demandée par l'interface.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "action", rename_all = "camelCase")]
pub enum ProfileEdit {
    Create { name: String },
    /// Profil importé : ces mods-là (les autres seront désactivés en y passant).
    CreateWith { name: String, mods: Vec<i64> },
    Rename { name: String, new_name: String },
    Delete { name: String },
}

pub fn edit_profiles(game: &mut GameInstall, edit: &ProfileEdit) -> AppResult<()> {
    match edit {
        ProfileEdit::Create { name } => create_profile(game, name),
        ProfileEdit::CreateWith { name, mods } => create_profile_with(game, name, mods),
        ProfileEdit::Rename { name, new_name } => rename_profile(game, name, new_name),
        ProfileEdit::Delete { name } => delete_profile(game, name),
    }
}

// ---------------------------------------------------------------- Profils

fn check_name(name: &str) -> AppResult<String> {
    let n = name.trim();
    if n.is_empty() || n.chars().count() > 40 {
        return Err(AppError::InvalidArgument(crate::tr!(
            "nom de profil vide ou trop long (40 caractères au plus).",
            "profile name empty or too long (40 characters max)."
        )));
    }
    Ok(n.to_string())
}

fn enabled_mods(game: &GameInstall) -> Vec<i64> {
    game.mods.values().filter(|m| !m.disabled).map(|m| m.mod_id).collect()
}

/// Le profil actif suit l'état des mods (activés, installés, désinstallés).
pub fn sync_active(game: &mut GameInstall) {
    let enabled = enabled_mods(game);
    if let Some(name) = game.active_profile.clone() {
        if let Some(p) = game.profiles.iter_mut().find(|p| p.name == name) {
            p.mods = enabled;
        }
    }
}

/// Un mod désinstallé disparaît de tous les profils.
pub fn forget_mod(game: &mut GameInstall, mod_id: i64) {
    for p in &mut game.profiles {
        p.mods.retain(|id| *id != mod_id);
    }
}

/// Nom du premier profil, qui garde l'état d'avant (« Main » en anglais).
fn main_name() -> &'static str {
    crate::i18n::pick("Principal", "Main")
}

/// Ce nom est celui du premier profil (dans l'une ou l'autre langue).
fn is_main(name: &str) -> bool {
    name.eq_ignore_ascii_case("Principal") || name.eq_ignore_ascii_case("Main")
}

/// Nouveau profil avec les mods activés en ce moment ; il devient le profil
/// actif. Le premier profil créé garde aussi l'état actuel sous « Principal ».
pub fn create_profile(game: &mut GameInstall, name: &str) -> AppResult<()> {
    let name = check_name(name)?;
    if game.profiles.iter().any(|p| p.name.eq_ignore_ascii_case(&name)) {
        return Err(AppError::InvalidArgument(crate::tr!("le profil « {name} » existe déjà.", "the profile \"{name}\" already exists.")));
    }
    let enabled = enabled_mods(game);
    if game.profiles.is_empty() && !is_main(&name) {
        game.profiles.push(Profile { name: main_name().into(), mods: enabled.clone() });
    }
    game.profiles.push(Profile { name: name.clone(), mods: enabled });
    game.active_profile = Some(name);
    Ok(())
}

/// Profil importé, avec ces mods (ceux qui sont installés), sans y passer :
/// un profil du même nom est mis à jour. L'état actuel est d'abord gardé dans
/// le profil actif, ou, sans profil actif, dans « Principal » (« Avant
/// l'import » si « Principal » existe déjà) qui devient le profil actif.
pub fn create_profile_with(game: &mut GameInstall, name: &str, mods: &[i64]) -> AppResult<()> {
    let name = check_name(name)?;
    if is_main(&name) {
        return Err(AppError::InvalidArgument(crate::tr!(
            "« {name} » garde tes mods d'avant l'import : choisis un autre nom.",
            "\"{name}\" keeps your mods from before the import: choose another name."
        )));
    }
    let wanted: Vec<i64> = mods.iter().copied().filter(|id| game.mods.contains_key(id)).collect();
    if game.active_profile.is_some() {
        sync_active(game);
    } else {
        let backup = if game.profiles.iter().any(|p| is_main(&p.name)) {
            crate::i18n::pick("Avant l'import", "Before import")
        } else {
            main_name()
        };
        let enabled = enabled_mods(game);
        match game.profiles.iter_mut().find(|p| p.name == backup) {
            Some(p) => p.mods = enabled,
            None => game.profiles.push(Profile { name: backup.into(), mods: enabled }),
        }
        game.active_profile = Some(backup.into());
    }
    match game.profiles.iter_mut().find(|p| p.name.eq_ignore_ascii_case(&name)) {
        Some(p) => p.mods = wanted,
        None => game.profiles.push(Profile { name, mods: wanted }),
    }
    Ok(())
}

pub fn rename_profile(game: &mut GameInstall, old: &str, new: &str) -> AppResult<()> {
    let new = check_name(new)?;
    if game.profiles.iter().any(|p| p.name != old && p.name.eq_ignore_ascii_case(&new)) {
        return Err(AppError::InvalidArgument(crate::tr!("le profil « {new} » existe déjà.", "the profile \"{new}\" already exists.")));
    }
    let p = game.profiles.iter_mut().find(|p| p.name == old).ok_or(AppError::NotFound)?;
    p.name = new.clone();
    if game.active_profile.as_deref() == Some(old) {
        game.active_profile = Some(new);
    }
    Ok(())
}

/// Supprime un profil (les mods ne bougent pas). S'il n'en reste qu'un, les
/// profils disparaissent.
pub fn delete_profile(game: &mut GameInstall, name: &str) -> AppResult<()> {
    let before = game.profiles.len();
    game.profiles.retain(|p| p.name != name);
    if game.profiles.len() == before {
        return Err(AppError::NotFound);
    }
    if game.profiles.len() == 1 {
        game.profiles.clear();
    }
    // Profil actif supprimé : aucun profil ne suit plus les changements (en
    // activer un autre sans l'appliquer écraserait sa liste de mods).
    if game.active_profile.as_deref() == Some(name) || game.profiles.is_empty() {
        game.active_profile = None;
    }
    Ok(())
}

/// Mod qui n'a pas pu changer d'état en passant à un profil.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileIssue {
    pub mod_id: i64,
    pub name: String,
    pub message: String,
}

/// Passe à un profil : désactive les mods qui n'en font pas partie, puis
/// active les siens. `others_for` : fichiers des autres mods activés (les deux
/// bibliothèques), recalculés avant chaque activation.
pub fn apply_profile(
    game: &mut GameInstall,
    name: &str,
    ctx: &Ctx,
    others_for: &dyn Fn(&GameInstall, i64) -> HashMap<PathBuf, BTreeMap<String, String>>,
    on_toggle: &mut dyn FnMut(&InstalledMod, bool),
) -> AppResult<Vec<ProfileIssue>> {
    apply_profile_by(game, name, &mut |g, id, on| {
        let record = g.mods[&id].clone();
        let r = if on {
            let others = others_for(g, id);
            enable(g, id, ctx, &others)
        } else {
            disable(g, id, ctx)
        };
        if r.is_ok() {
            on_toggle(&record, on);
        }
        r
    })
}

/// Passe à un profil en activant ou désactivant chaque mod avec `change`
/// (les sources qui ont plus à faire qu'un simple `enable` / `disable`).
pub fn apply_profile_by(
    game: &mut GameInstall,
    name: &str,
    change: &mut dyn FnMut(&mut GameInstall, i64, bool) -> AppResult<()>,
) -> AppResult<Vec<ProfileIssue>> {
    let wanted: Vec<i64> = game.profiles.iter().find(|p| p.name == name).ok_or(AppError::NotFound)?.mods.clone();
    // Le profil quitté garde l'état actuel ; aucun profil ne suit les changements qui viennent.
    sync_active(game);
    game.active_profile = None;
    let mut issues = Vec::new();
    let ids: Vec<i64> = game.mods.keys().copied().collect();
    for id in ids.iter().filter(|id| !wanted.contains(id)) {
        if game.mods[id].disabled {
            continue;
        }
        let name = game.mods[id].name.clone();
        if let Err(e) = change(game, *id, false) {
            issues.push(ProfileIssue { mod_id: *id, name, message: e.to_string() });
        }
    }
    for id in ids.iter().filter(|id| wanted.contains(id)) {
        if !game.mods[id].disabled {
            continue;
        }
        let name = game.mods[id].name.clone();
        if let Err(e) = change(game, *id, true) {
            issues.push(ProfileIssue { mod_id: *id, name, message: e.to_string() });
        }
    }
    game.active_profile = Some(name.to_string());
    // Le profil garde ce qu'il demandait, même si un mod n'a pas pu changer.
    if let Some(p) = game.profiles.iter_mut().find(|p| p.name == name) {
        p.mods = wanted.into_iter().filter(|id| game.mods.contains_key(id)).collect();
    }
    Ok(issues)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::installer::tests::tempdir;
    use crate::installer::{Layout, ModPart};

    fn game(folder: &Path, rules: Option<&str>) -> GameInstall {
        GameInstall {
            game_id: 7,
            game_name: "Jeu".into(),
            folder: folder.to_path_buf(),
            layout: Layout::Subfolder,
            rules: rules.map(str::to_string),
            ..Default::default()
        }
    }

    fn put(folder: &Path, rel: &str, body: &[u8]) {
        let p = folder.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, body).unwrap();
    }

    fn record(id: i64, name: &str, files: &[&str], dirs: &[&str]) -> InstalledMod {
        InstalledMod {
            mod_id: id,
            name: name.into(),
            files: files.iter().map(|s| s.to_string()).collect(),
            dirs: dirs.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn disables_and_enables_without_losing_files() {
        let folder = tempdir("tg-mods");
        let store = tempdir("tg-store");
        let ctx = Ctx { store_root: &store, source: "modio" };
        let mut g = game(&folder, None);
        put(&folder, "Sky/sky.pak", b"ciel");
        put(&folder, "Sky/user.cfg", b"reglage perso");
        g.mods.insert(1, record(1, "Clear Skies", &["Sky/sky.pak"], &["Sky"]));

        disable(&mut g, 1, &ctx).unwrap();
        assert!(g.mods[&1].disabled);
        assert!(!folder.join("Sky/sky.pak").exists());
        assert!(folder.join("Sky/user.cfg").exists(), "les fichiers ajoutés par l'utilisateur restent");
        assert_eq!(fs::read(store.join("modio/7/1/main/Sky/sky.pak")).unwrap(), b"ciel");

        enable(&mut g, 1, &ctx, &HashMap::new()).unwrap();
        assert!(!g.mods[&1].disabled);
        assert_eq!(fs::read(folder.join("Sky/sky.pak")).unwrap(), b"ciel");
        assert!(!store.join("modio").exists(), "plus rien de côté");
    }

    #[test]
    fn games_with_rules_keep_disabled_mods_in_a_hidden_folder() {
        let game_dir = tempdir("tg-game");
        let content = tempdir("tg-content");
        let store = tempdir("tg-store2");
        let ctx = Ctx { store_root: &store, source: "nexus" };
        let mut g = game(&content, Some("bonelab"));
        g.extra_folders.insert("game".into(), game_dir.clone());
        put(&content, "Pallet/pallet.json", b"{}");
        put(&game_dir, "Mods/Code.dll", b"dll");
        let mut r = record(148, "Fusion", &["Pallet/pallet.json"], &["Pallet"]);
        r.parts = vec![ModPart { key: "game".into(), files: vec!["Mods/Code.dll".into()], dirs: vec![] }];
        g.mods.insert(148, r);

        disable(&mut g, 148, &ctx).unwrap();
        assert!(content.join(".modstudio-disabled/nexus-148/Pallet/pallet.json").is_file());
        assert!(game_dir.join(".modstudio-disabled/nexus-148/Mods/Code.dll").is_file());
        assert!(!content.join("Pallet").exists());
        assert!(game_dir.join("Mods").exists(), "le dossier Mods du jeu reste");

        enable(&mut g, 148, &ctx, &HashMap::new()).unwrap();
        assert!(content.join("Pallet/pallet.json").is_file());
        assert!(game_dir.join("Mods/Code.dll").is_file());
        assert!(!content.join(".modstudio-disabled").exists());
        assert!(!game_dir.join(".modstudio-disabled").exists());
    }

    #[test]
    fn enabling_never_overwrites_another_mod() {
        let folder = tempdir("tg-conf");
        let store = tempdir("tg-store3");
        let ctx = Ctx { store_root: &store, source: "modio" };
        let mut g = game(&folder, None);
        put(&folder, "a.pak", b"A");
        g.mods.insert(1, record(1, "Mod A", &["a.pak"], &[]));
        disable(&mut g, 1, &ctx).unwrap();
        // Un autre mod a pris la place pendant ce temps.
        put(&folder, "a.pak", b"B");
        let mut others = HashMap::new();
        others.insert(folder.clone(), BTreeMap::from([("a.pak".to_string(), "Mod B".to_string())]));
        let err = enable(&mut g, 1, &ctx, &others).unwrap_err();
        assert!(matches!(&err, AppError::Conflict(t) if t.contains("Mod B")), "{err}");
        assert!(g.mods[&1].disabled);
        assert_eq!(fs::read(folder.join("a.pak")).unwrap(), b"B");

        // Désinstallé pendant qu'il est désactivé : seuls ses fichiers de côté partent.
        let rec = g.mods[&1].clone();
        discard(&g, &rec, &ctx);
        assert!(!store.join("modio").exists());
        assert_eq!(fs::read(folder.join("a.pak")).unwrap(), b"B");
    }

    #[test]
    fn files_that_could_not_be_restored_are_no_longer_claimed() {
        let folder = tempdir("tg-lost");
        let store = tempdir("tg-store5");
        let ctx = Ctx { store_root: &store, source: "modio" };
        let mut g = game(&folder, None);
        put(&folder, "x.pak", b"A");
        put(&folder, "y.pak", b"A");
        g.mods.insert(1, record(1, "Mod A", &["x.pak", "y.pak"], &[]));
        disable(&mut g, 1, &ctx).unwrap();
        assert_eq!(g.mods[&1].disabled_in.as_deref(), Some("app"));
        // Fichier mis de côté perdu, et un autre mod a pris sa place.
        fs::remove_file(store.join("modio/7/1/main/x.pak")).unwrap();
        put(&folder, "x.pak", b"B");
        enable(&mut g, 1, &ctx, &HashMap::new()).unwrap();
        assert_eq!(g.mods[&1].files, vec!["y.pak"], "x.pak n'appartient plus au mod A");
        // Désinstaller A ne touche donc pas au fichier de B.
        installer::uninstall(&folder, &g.mods[&1]);
        assert_eq!(fs::read(folder.join("x.pak")).unwrap(), b"B");
        assert!(!folder.join("y.pak").exists());
    }

    #[test]
    fn the_store_location_survives_a_change_of_rules() {
        let folder = tempdir("tg-rules");
        let store = tempdir("tg-store6");
        let ctx = Ctx { store_root: &store, source: "nexus" };
        let mut g = game(&folder, None);
        put(&folder, "a.dll", b"A");
        g.mods.insert(1, record(1, "Mod A", &["a.dll"], &[]));
        disable(&mut g, 1, &ctx).unwrap();
        // Le jeu passe ensuite aux règles Unity (chargeur installé).
        g.rules = Some("unity".into());
        enable(&mut g, 1, &ctx, &HashMap::new()).unwrap();
        assert_eq!(fs::read(folder.join("a.dll")).unwrap(), b"A");
        assert_eq!(g.mods[&1].files, vec!["a.dll"]);
    }

    #[test]
    fn deleting_the_active_profile_leaves_the_others_untouched() {
        let folder = tempdir("tg-del");
        let store = tempdir("tg-store7");
        let ctx = Ctx { store_root: &store, source: "modio" };
        let mut g = game(&folder, None);
        for (id, f) in [(1, "a.pak"), (2, "b.pak"), (3, "c.pak"), (4, "d.pak")] {
            put(&folder, f, b"x");
            g.mods.insert(id, record(id, &format!("Mod {id}"), &[f], &[]));
        }
        create_profile(&mut g, "Coop").unwrap();
        create_profile(&mut g, "Solo").unwrap();
        disable(&mut g, 2, &ctx).unwrap();
        delete_profile(&mut g, "Solo").unwrap();
        assert_eq!(g.active_profile, None);
        disable(&mut g, 3, &ctx).unwrap();
        let principal = g.profiles.iter().find(|p| p.name == "Principal").unwrap();
        assert_eq!(principal.mods.len(), 4, "« Principal » garde ses quatre mods");
    }

    #[test]
    fn profiles_switch_sets_of_mods() {
        let folder = tempdir("tg-prof");
        let store = tempdir("tg-store4");
        let ctx = Ctx { store_root: &store, source: "modio" };
        let mut g = game(&folder, None);
        for (id, f) in [(1, "a.pak"), (2, "b.pak"), (3, "c.pak")] {
            put(&folder, f, b"x");
            g.mods.insert(id, record(id, &format!("Mod {id}"), &[f], &[]));
        }
        create_profile(&mut g, "Coop").unwrap();
        assert_eq!(g.profiles.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), vec!["Principal", "Coop"]);
        assert_eq!(g.active_profile.as_deref(), Some("Coop"));

        // Dans « Coop », le mod 2 est désactivé ; le profil suit.
        disable(&mut g, 2, &ctx).unwrap();
        assert_eq!(g.profiles[1].mods.iter().copied().collect::<std::collections::BTreeSet<_>>(), [1, 3].into());

        let others = |g: &GameInstall, id: i64| {
            let mut map: HashMap<PathBuf, BTreeMap<String, String>> = HashMap::new();
            for (folder, rel, owner) in g.owned_files(id) {
                map.entry(folder).or_default().insert(rel, owner);
            }
            map
        };
        let mut toggled = Vec::new();
        let issues = apply_profile(&mut g, "Principal", &ctx, &others, &mut |m, on| toggled.push((m.mod_id, on))).unwrap();
        assert!(issues.is_empty());
        assert_eq!(toggled, vec![(2, true)]);
        assert!(folder.join("b.pak").exists());
        assert_eq!(g.active_profile.as_deref(), Some("Principal"));

        apply_profile(&mut g, "Coop", &ctx, &others, &mut |_, _| {}).unwrap();
        assert!(!folder.join("b.pak").exists());

        rename_profile(&mut g, "Coop", "Avec amis").unwrap();
        assert_eq!(g.active_profile.as_deref(), Some("Avec amis"));
        assert!(create_profile(&mut g, "principal").is_err());
        delete_profile(&mut g, "Avec amis").unwrap();
        assert!(g.profiles.is_empty() && g.active_profile.is_none(), "un seul profil restant : plus de profils");
        forget_mod(&mut g, 1);
    }

    #[test]
    fn an_imported_profile_keeps_only_its_mods_and_the_current_state_stays_in_principal() {
        let folder = tempdir("tg-import");
        let store = tempdir("tg-store5");
        let ctx = Ctx { store_root: &store, source: "nexus" };
        let mut g = game(&folder, None);
        for (id, f) in [(1, "a.pak"), (2, "b.pak"), (3, "c.pak")] {
            put(&folder, f, b"x");
            g.mods.insert(id, record(id, &format!("Mod {id}"), &[f], &[]));
        }
        create_profile_with(&mut g, "Coop de Léo", &[1, 3, 99]).unwrap();
        assert_eq!(g.profiles.iter().map(|p| (p.name.as_str(), p.mods.clone())).collect::<Vec<_>>(), vec![
            ("Principal", vec![1, 2, 3]),
            ("Coop de Léo", vec![1, 3]),
        ]);
        let others = |_: &GameInstall, _: i64| HashMap::new();
        apply_profile(&mut g, "Coop de Léo", &ctx, &others, &mut |_, _| {}).unwrap();
        assert!(!folder.join("b.pak").exists());
        assert_eq!(g.active_profile.as_deref(), Some("Coop de Léo"));

        // Importé une deuxième fois (mis à jour) : la liste est remplacée.
        create_profile_with(&mut g, "coop de léo", &[1]).unwrap();
        assert_eq!(g.profiles.len(), 2);
        assert_eq!(g.profiles[1].mods, vec![1]);
        // « Principal » ne peut pas être écrasé par un import.
        assert!(create_profile_with(&mut g, "principal", &[1]).is_err());
    }

    #[test]
    fn without_an_active_profile_the_current_state_is_kept_before_importing() {
        let folder = tempdir("tg-import2");
        let mut g = game(&folder, None);
        for id in [1, 2] {
            g.mods.insert(id, record(id, &format!("Mod {id}"), &[], &[]));
        }
        g.profiles = vec![Profile { name: "Principal".into(), mods: vec![1] }, Profile { name: "Solo".into(), mods: vec![2] }];
        g.active_profile = None;
        create_profile_with(&mut g, "Coop", &[2]).unwrap();
        let names: Vec<&str> = g.profiles.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["Principal", "Solo", "Avant l'import", "Coop"]);
        assert_eq!(g.profiles[0].mods, vec![1], "le « Principal » existant n'est pas touché");
        assert_eq!(g.profiles[2].mods, vec![1, 2]);
        assert_eq!(g.active_profile.as_deref(), Some("Avant l'import"));
    }
}
