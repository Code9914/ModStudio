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

/// Dossier qui garde les fichiers d'un mod désactivé, pour un dossier du jeu.
pub fn store_for(game: &GameInstall, key: &str, mod_id: i64, ctx: &Ctx) -> Option<PathBuf> {
    let folder = game.folder_for(key)?;
    Some(if game.rules.is_some() {
        folder.join(DISABLED_DIR).join(format!("{}-{mod_id}", ctx.source))
    } else {
        ctx.store_root
            .join(ctx.source)
            .join(game.game_id.to_string())
            .join(mod_id.to_string())
            .join(if key.is_empty() { "main" } else { key })
    })
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
            return Err(AppError::Io(format!("{rel} : {e} (le jeu est-il ouvert ?)")));
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

/// Désactive un mod : ses fichiers sont mis de côté.
pub fn disable(game: &mut GameInstall, mod_id: i64, ctx: &Ctx) -> AppResult<()> {
    let record = game.mods.get(&mod_id).cloned().ok_or(AppError::NotFound)?;
    if record.disabled {
        return Ok(());
    }
    let mut moves = Vec::new();
    for (key, files, _) in parts_of(&record) {
        let (Some(folder), Some(store)) = (game.folder_for(&key), store_for(game, &key, mod_id, ctx)) else { continue };
        for rel in files {
            let (Some(src), Some(dst)) = (installer::resolve_rel(folder, &rel), installer::resolve_rel(&store, &rel)) else {
                continue;
            };
            // Fichier déjà supprimé à la main : rien à mettre de côté.
            if src.is_file() {
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
        m.disabled = true;
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
    let mut moves = Vec::new();
    let mut conflicts = Vec::new();
    let mut stores = Vec::new();
    for (key, files, _) in parts_of(&record) {
        let (Some(folder), Some(store)) = (game.folder_for(&key), store_for(game, &key, mod_id, ctx)) else { continue };
        let owned = others.get(folder);
        for rel in files {
            let (Some(dst), Some(src)) = (installer::resolve_rel(folder, &rel), installer::resolve_rel(&store, &rel)) else {
                continue;
            };
            if !src.is_file() {
                continue;
            }
            if let Some(owner) = owned.and_then(|o| o.get(&rel)) {
                conflicts.push(format!("{rel} (installé par « {owner} »)"));
            } else if dst.exists() {
                conflicts.push(rel.clone());
            } else {
                moves.push((src, dst, rel));
            }
        }
        stores.push((store, folder.to_path_buf()));
    }
    if !conflicts.is_empty() {
        let more = conflicts.len().saturating_sub(3);
        let mut text = conflicts.into_iter().take(3).collect::<Vec<_>>().join(", ");
        if more > 0 {
            text.push_str(&format!(" et {more} autres"));
        }
        return Err(AppError::Conflict(text));
    }
    move_all(&moves)?;
    for (store, folder) in stores {
        let stop = if game.rules.is_some() { folder.join(DISABLED_DIR) } else { ctx.store_root.to_path_buf() };
        remove_store(&store, &stop);
        if game.rules.is_some() {
            let _ = fs::remove_dir(folder.join(DISABLED_DIR));
        }
    }
    if let Some(m) = game.mods.get_mut(&mod_id) {
        m.disabled = false;
    }
    sync_active(game);
    Ok(())
}

/// Supprime les fichiers mis de côté d'un mod désactivé (désinstallation ou
/// nouvelle version). Ses chemins dans le dossier du jeu ne sont pas touchés :
/// un autre mod a pu les prendre.
pub fn discard(game: &GameInstall, record: &InstalledMod, ctx: &Ctx) {
    for (key, _, _) in parts_of(record) {
        let (Some(folder), Some(store)) = (game.folder_for(&key), store_for(game, &key, record.mod_id, ctx)) else { continue };
        let stop = if game.rules.is_some() { folder.join(DISABLED_DIR) } else { ctx.store_root.to_path_buf() };
        remove_store(&store, &stop);
        if game.rules.is_some() {
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
    Rename { name: String, new_name: String },
    Delete { name: String },
}

pub fn edit_profiles(game: &mut GameInstall, edit: &ProfileEdit) -> AppResult<()> {
    match edit {
        ProfileEdit::Create { name } => create_profile(game, name),
        ProfileEdit::Rename { name, new_name } => rename_profile(game, name, new_name),
        ProfileEdit::Delete { name } => delete_profile(game, name),
    }
}

// ---------------------------------------------------------------- Profils

fn check_name(name: &str) -> AppResult<String> {
    let n = name.trim();
    if n.is_empty() || n.chars().count() > 40 {
        return Err(AppError::InvalidArgument("nom de profil vide ou trop long (40 caractères au plus).".into()));
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

/// Nouveau profil avec les mods activés en ce moment ; il devient le profil
/// actif. Le premier profil créé garde aussi l'état actuel sous « Principal ».
pub fn create_profile(game: &mut GameInstall, name: &str) -> AppResult<()> {
    let name = check_name(name)?;
    if game.profiles.iter().any(|p| p.name.eq_ignore_ascii_case(&name)) {
        return Err(AppError::InvalidArgument(format!("le profil « {name} » existe déjà.")));
    }
    let enabled = enabled_mods(game);
    if game.profiles.is_empty() && !name.eq_ignore_ascii_case("Principal") {
        game.profiles.push(Profile { name: "Principal".into(), mods: enabled.clone() });
    }
    game.profiles.push(Profile { name: name.clone(), mods: enabled });
    game.active_profile = Some(name);
    Ok(())
}

pub fn rename_profile(game: &mut GameInstall, old: &str, new: &str) -> AppResult<()> {
    let new = check_name(new)?;
    if game.profiles.iter().any(|p| p.name != old && p.name.eq_ignore_ascii_case(&new)) {
        return Err(AppError::InvalidArgument(format!("le profil « {new} » existe déjà.")));
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
    if game.active_profile.as_deref() == Some(name) || game.profiles.is_empty() {
        game.active_profile = game.profiles.first().map(|p| p.name.clone());
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
        let record = game.mods[id].clone();
        match disable(game, *id, ctx) {
            Ok(()) => on_toggle(&record, false),
            Err(e) => issues.push(ProfileIssue { mod_id: *id, name: record.name, message: e.to_string() }),
        }
    }
    for id in ids.iter().filter(|id| wanted.contains(id)) {
        if !game.mods[id].disabled {
            continue;
        }
        let record = game.mods[id].clone();
        let others = others_for(game, *id);
        match enable(game, *id, ctx, &others) {
            Ok(()) => on_toggle(&record, true),
            Err(e) => issues.push(ProfileIssue { mod_id: *id, name: record.name, message: e.to_string() }),
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
}
