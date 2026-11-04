//! Fichiers en commun entre les mods d'une collection Nexus.
//!
//! Hors collection, ModStudio n'écrase jamais le fichier d'un autre mod. Dans
//! une collection, plusieurs mods installent souvent le même fichier, et la
//! collection dit lequel doit gagner : celui qu'elle place après les autres
//! (`CollectionMark::rank`). À chaque chemin partagé, seul le fichier du mod
//! activé le mieux placé est dans le dossier du jeu ; ceux des autres sont
//! gardés de côté, chacun dans le rangement de son propre mod
//! (`InstalledMod::hidden`), donc jamais l'un par-dessus l'autre. Quand le mod
//! en place est retiré ou désactivé, le suivant dans l'ordre reprend sa place.
//!
//! Un mod ne revendique dans `files` que ses fichiers en place : le désactiver
//! ou le retirer ne touche jamais au fichier d'un autre.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::installer::{self, GameInstall, Hidden, InstalledMod, ModPart};
use crate::toggle::{self, Ctx, DISABLED_DIR};

/// `a` passe devant `b` : même collection, et `a` vient après.
pub fn wins(a: &InstalledMod, b: &InstalledMod) -> bool {
    match (&a.collection, &b.collection) {
        (Some(x), Some(y)) => x.slug == y.slug && x.rank > y.rank,
        _ => false,
    }
}

fn rank(m: &InstalledMod) -> u32 {
    m.collection.as_ref().map(|c| c.rank).unwrap_or(0)
}

/// Les fichiers cachés sont-ils gardés dans le dossier du jeu ? Comme les
/// mods désactivés : oui pour les jeux rangés selon leurs règles.
pub fn kept_in_game(game: &GameInstall) -> bool {
    game.rules.is_some()
}

/// Rangement des fichiers cachés de `owner`, pour un dossier du jeu.
pub fn store(game: &GameInstall, key: &str, owner: i64, in_game: bool, ctx: &Ctx) -> Option<PathBuf> {
    let folder = game.folder_for(key)?;
    Some(if in_game {
        folder.join(DISABLED_DIR).join(format!("{}-hidden-{owner}", ctx.source))
    } else {
        ctx.store_root
            .join(format!("{}-hidden", ctx.source))
            .join(game.game_id.to_string())
            .join(owner.to_string())
            .join(if key.is_empty() { "main" } else { key })
    })
}

fn hidden_path(game: &GameInstall, owner: i64, h: &Hidden, ctx: &Ctx) -> Option<PathBuf> {
    installer::resolve_rel(&store(game, &h.key, owner, h.in_game, ctx)?, &h.rel)
}

/// Le mod revendique `rel` dans le dossier `key` (et les dossiers créés pour lui).
fn claim(record: &mut InstalledMod, key: &str, rel: &str, created: Vec<String>) {
    let (files, dirs) = if key.is_empty() {
        (&mut record.files, &mut record.dirs)
    } else {
        if !record.parts.iter().any(|p| p.key == key) {
            record.parts.push(ModPart { key: key.to_string(), files: Vec::new(), dirs: Vec::new() });
        }
        let part = record.parts.iter_mut().find(|p| p.key == key).expect("ajouté plus haut");
        (&mut part.files, &mut part.dirs)
    };
    if !files.iter().any(|f| f == rel) {
        files.push(rel.to_string());
    }
    for d in created {
        if !dirs.contains(&d) {
            dirs.push(d);
        }
    }
}

/// Le mod ne revendique plus `rel` dans le dossier `key`.
fn unclaim(record: &mut InstalledMod, key: &str, rel: &str) {
    if key.is_empty() {
        record.files.retain(|f| f != rel);
    } else if let Some(p) = record.parts.iter_mut().find(|p| p.key == key) {
        p.files.retain(|f| f != rel);
    }
}

/// Dossiers du mod (dans le dossier `key`) au-dessus de `rel`.
fn parent_dirs(record: &InstalledMod, key: &str, rel: &str) -> Vec<String> {
    let dirs = if key.is_empty() { &record.dirs } else { record.parts.iter().find(|p| p.key == key).map(|p| &p.dirs).unwrap_or(&record.dirs) };
    dirs.iter().filter(|d| rel.starts_with(&format!("{d}/"))).cloned().collect()
}

/// Crée les dossiers qui manquent au-dessus de `rel` ; renvoie ceux créés.
fn ensure_parents(base: &Path, rel: &str) -> std::io::Result<Vec<String>> {
    let parts: Vec<&str> = rel.split('/').collect();
    let mut created = Vec::new();
    for depth in 1..parts.len() {
        let dir_rel = parts[..depth].join("/");
        let Some(abs) = installer::resolve_rel(base, &dir_rel) else { continue };
        if !abs.exists() {
            fs::create_dir(&abs)?;
            created.push(dir_rel);
        }
    }
    Ok(created)
}

fn move_into(src: &Path, dst: &Path) -> std::io::Result<()> {
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)?;
    }
    installer::move_file_io(src, dst)
}

/// Retire un dossier vide et ses parents vides, jusqu'à `stop`.
fn prune_up(dir: &Path, stop: &Path) {
    let mut cur = Some(dir.to_path_buf());
    while let Some(d) = cur {
        if d == stop || !d.starts_with(stop) || (d.exists() && fs::remove_dir(&d).is_err()) {
            break;
        }
        cur = d.parent().map(Path::to_path_buf);
    }
}

/// Retire les dossiers vides d'une arborescence (les fichiers restent).
fn remove_empty_tree(dir: &Path) {
    if let Ok(entries) = fs::read_dir(dir) {
        for e in entries.flatten() {
            if e.file_type().is_ok_and(|t| t.is_dir()) {
                remove_empty_tree(&e.path());
            }
        }
    }
    let _ = fs::remove_dir(dir);
}

/// Retire les dossiers vides du rangement des fichiers cachés de `owner`.
pub fn remove_stores(game: &GameInstall, owner: i64, ctx: &Ctx) {
    let mut keys: Vec<String> = vec![String::new()];
    keys.extend(game.extra_folders.keys().cloned());
    for key in keys {
        for in_game in [true, false] {
            let (Some(dir), Some(folder)) = (store(game, &key, owner, in_game, ctx), game.folder_for(&key)) else { continue };
            if !dir.exists() {
                continue;
            }
            remove_empty_tree(&dir);
            let stop = if in_game { folder.join(DISABLED_DIR) } else { ctx.store_root.to_path_buf() };
            prune_up(&dir, &stop);
            if in_game {
                let _ = fs::remove_dir(folder.join(DISABLED_DIR));
            }
        }
    }
}

/// Le chemin `rel` (dossier `key`) vient d'être libéré : le fichier caché du
/// mod activé le mieux placé reprend sa place.
pub fn promote(game: &mut GameInstall, key: &str, rel: &str, ctx: &Ctx) {
    let Some(next) = game
        .mods
        .values()
        .filter(|m| !m.disabled && m.hidden.iter().any(|h| h.key == key && h.rel == rel))
        .max_by_key(|m| (rank(m), m.mod_id))
        .map(|m| m.mod_id)
    else {
        return;
    };
    let Some(folder) = game.folder_for(key).map(Path::to_path_buf) else { return };
    let Some(dst) = installer::resolve_rel(&folder, rel) else { return };
    if dst.exists() {
        return;
    }
    let entry = game.mods[&next].hidden.iter().find(|h| h.key == key && h.rel == rel).cloned().expect("trouvé plus haut");
    let Some(src) = hidden_path(game, next, &entry, ctx) else { return };
    let Ok(created) = ensure_parents(&folder, rel) else { return };
    if src.is_file() && installer::move_file_io(&src, &dst).is_ok() {
        if let Some(m) = game.mods.get_mut(&next) {
            m.hidden.retain(|h| h != &entry);
            claim(m, key, rel, created);
        }
    } else if !src.is_file() {
        // Fichier caché disparu : le mod ne l'attend plus.
        if let Some(m) = game.mods.get_mut(&next) {
            m.hidden.retain(|h| h != &entry);
        }
        promote(game, key, rel, ctx);
    }
    remove_stores(game, next, ctx);
}

/// Chemins (dossier, chemin) en place d'un mod.
pub fn in_place(record: &InstalledMod) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = record.files.iter().map(|f| (String::new(), f.clone())).collect();
    for p in &record.parts {
        out.extend(p.files.iter().map(|f| (p.key.clone(), f.clone())));
    }
    out
}

/// Chemins libérés par un mod retiré ou désactivé : les mods cachés dessous reprennent leur place.
pub fn promote_all(game: &mut GameInstall, paths: &[(String, String)], ctx: &Ctx) {
    for (key, rel) in paths {
        promote(game, key, rel, ctx);
    }
}

/// `mod_id` est désinstallé : ses fichiers cachés sont supprimés.
pub fn drop_hidden(game: &mut GameInstall, mod_id: i64, ctx: &Ctx) {
    let entries = game.mods.get_mut(&mod_id).map(|m| std::mem::take(&mut m.hidden)).unwrap_or_default();
    for h in &entries {
        if let Some(p) = hidden_path(game, mod_id, h, ctx) {
            let _ = fs::remove_file(p);
        }
    }
    remove_stores(game, mod_id, ctx);
}

/// `mod_id` est désactivé : ses fichiers cachés rejoignent ses fichiers mis de
/// côté (`in_game` : rangement de la désactivation), et il les revendique.
pub fn stash_hidden(game: &mut GameInstall, mod_id: i64, in_game: bool, ctx: &Ctx) {
    let entries = game.mods.get_mut(&mod_id).map(|m| std::mem::take(&mut m.hidden)).unwrap_or_default();
    for h in &entries {
        let src = hidden_path(game, mod_id, h, ctx);
        let dst = toggle::store_for(game, &h.key, mod_id, in_game, ctx).and_then(|d| installer::resolve_rel(&d, &h.rel));
        if let (Some(src), Some(dst)) = (src, dst)
            && src.is_file()
            && move_into(&src, &dst).is_ok()
            && let Some(m) = game.mods.get_mut(&mod_id)
        {
            claim(m, &h.key, &h.rel, Vec::new());
        }
    }
    remove_stores(game, mod_id, ctx);
}

/// Enregistre ce qu'une mise en place de `mod_id` a changé : fichiers d'autres
/// mods passés dessous ((dossier, chemin, propriétaire), rangés chez leur
/// propriétaire), et fichiers de `mod_id` restés dessous ((dossier, chemin),
/// rangés chez lui).
pub fn record_placement(game: &mut GameInstall, mod_id: i64, covered: &[(String, String, i64)], hidden: &[(String, String)], in_game: bool) {
    for (key, rel, owner) in covered {
        let mut inherited = Vec::new();
        if let Some(o) = game.mods.get_mut(owner) {
            unclaim(o, key, rel);
            let h = Hidden { key: key.clone(), rel: rel.clone(), in_game };
            if !o.hidden.contains(&h) {
                o.hidden.push(h);
            }
            inherited = parent_dirs(o, key, rel);
        }
        // Le mod en place garde les dossiers au-dessus du fichier : ils seront
        // retirés avec lui s'ils sont vides.
        if let Some(m) = game.mods.get_mut(&mod_id) {
            claim(m, key, rel, inherited);
        }
    }
    if let Some(m) = game.mods.get_mut(&mod_id) {
        for (key, rel) in hidden {
            unclaim(m, key, rel);
            let h = Hidden { key: key.clone(), rel: rel.clone(), in_game };
            if !m.hidden.contains(&h) {
                m.hidden.push(h);
            }
        }
    }
}

/// Après une nouvelle version de `mod_id` : ses anciens fichiers cachés qui ne
/// le sont plus sont supprimés (`still` : chemins cachés de la nouvelle version).
pub fn drop_stale_hidden(game: &mut GameInstall, mod_id: i64, old: &[Hidden], still: &[(String, String)], ctx: &Ctx) {
    let keep: BTreeSet<(&str, &str)> = still.iter().map(|(k, r)| (k.as_str(), r.as_str())).collect();
    for h in old {
        if keep.contains(&(h.key.as_str(), h.rel.as_str())) {
            continue;
        }
        if let Some(p) = hidden_path(game, mod_id, h, ctx) {
            let _ = fs::remove_file(p);
        }
    }
    remove_stores(game, mod_id, ctx);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::installer::tests::tempdir;
    use crate::installer::{CollectionMark, Layout};
    use std::collections::BTreeMap;

    fn mark(rank: u32) -> Option<CollectionMark> {
        Some(CollectionMark { slug: "abcd12".into(), rank })
    }

    #[test]
    fn later_mods_of_the_same_collection_win() {
        let a = InstalledMod { mod_id: 1, collection: mark(1), ..Default::default() };
        let b = InstalledMod { mod_id: 2, collection: mark(2), ..Default::default() };
        let other = InstalledMod { mod_id: 3, collection: Some(CollectionMark { slug: "zzz".into(), rank: 9 }), ..Default::default() };
        let plain = InstalledMod { mod_id: 4, ..Default::default() };
        assert!(wins(&b, &a) && !wins(&a, &b));
        assert!(!wins(&other, &a) && !wins(&a, &other));
        assert!(!wins(&plain, &a) && !wins(&a, &plain));
    }

    #[test]
    fn the_best_placed_hidden_file_comes_back() {
        let dir = tempdir("layers-promote");
        let folder = dir.join("game");
        let root = dir.join("store");
        fs::create_dir_all(&folder).unwrap();
        let ctx = Ctx { store_root: &root, source: "nexus" };
        let hidden = |r: u32| InstalledMod {
            mod_id: r as i64,
            collection: mark(r),
            hidden: vec![Hidden { key: String::new(), rel: "Data/b.txt".into(), in_game: false }],
            dirs: vec!["Data".into()],
            ..Default::default()
        };
        let mut game = GameInstall {
            game_id: 7,
            folder: folder.clone(),
            layout: Layout::Flat,
            mods: BTreeMap::from([(1, hidden(1)), (2, hidden(2))]),
            ..Default::default()
        };
        for r in [1, 2] {
            let kept = store(&game, "", r, false, &ctx).unwrap().join("Data/b.txt");
            fs::create_dir_all(kept.parent().unwrap()).unwrap();
            fs::write(&kept, format!("mod {r}")).unwrap();
        }
        promote(&mut game, "", "Data/b.txt", &ctx);
        assert_eq!(fs::read_to_string(folder.join("Data/b.txt")).unwrap(), "mod 2");
        assert!(game.mods[&2].files.contains(&"Data/b.txt".to_string()) && game.mods[&2].hidden.is_empty());
        assert_eq!(game.mods[&1].hidden.len(), 1, "le moins bien placé reste caché");
        // Le chemin libéré de nouveau : au tour du suivant.
        fs::remove_file(folder.join("Data/b.txt")).unwrap();
        game.mods.get_mut(&2).unwrap().files.clear();
        promote(&mut game, "", "Data/b.txt", &ctx);
        assert_eq!(fs::read_to_string(folder.join("Data/b.txt")).unwrap(), "mod 1");
        assert!(!root.join("nexus-hidden").exists(), "rangements vides retirés");
    }
}
