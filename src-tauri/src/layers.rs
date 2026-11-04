//! Fichiers en commun entre les mods d'une collection Nexus.
//!
//! Hors collection, ModStudio n'écrase jamais le fichier d'un autre mod. Dans
//! une collection, plusieurs mods installent souvent le même fichier, et la
//! collection dit lequel doit gagner : celui qu'elle place après les autres
//! (`CollectionMark::rank`). Le fichier recouvert n'est pas perdu : il est gardé
//! de côté par le mod qui le recouvre (`InstalledMod::shadowed`), et remis à sa
//! place quand ce mod est retiré ou désactivé. Le mod recouvert ne revendique
//! plus ce chemin entre-temps : le désactiver ou le retirer ne touche jamais au
//! fichier de l'autre.

use std::fs;
use std::path::{Path, PathBuf};

use crate::installer::{self, GameInstall, InstalledMod, ModPart, Shadow};
use crate::toggle::{self, Ctx, DISABLED_DIR};

/// `a` recouvre `b` : même collection, et `a` vient après.
pub fn wins(a: &InstalledMod, b: &InstalledMod) -> bool {
    match (&a.collection, &b.collection) {
        (Some(x), Some(y)) => x.slug == y.slug && x.rank > y.rank,
        _ => false,
    }
}

/// Les fichiers recouverts sont-ils gardés dans le dossier du jeu ? Comme les
/// mods désactivés : oui pour les jeux rangés selon leurs règles.
pub fn kept_in_game(game: &GameInstall) -> bool {
    game.rules.is_some()
}

/// Dossier qui garde les fichiers recouverts par `holder`, pour un dossier du jeu.
pub fn store(game: &GameInstall, key: &str, holder: i64, in_game: bool, ctx: &Ctx) -> Option<PathBuf> {
    let folder = game.folder_for(key)?;
    Some(if in_game {
        folder.join(DISABLED_DIR).join(format!("{}-shadow-{holder}", ctx.source))
    } else {
        ctx.store_root
            .join(format!("{}-shadow", ctx.source))
            .join(game.game_id.to_string())
            .join(holder.to_string())
            .join(if key.is_empty() { "main" } else { key })
    })
}

fn backup(game: &GameInstall, holder: i64, s: &Shadow, ctx: &Ctx) -> Option<PathBuf> {
    installer::resolve_rel(&store(game, &s.key, holder, s.in_game, ctx)?, &s.rel)
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
pub fn unclaim(record: &mut InstalledMod, key: &str, rel: &str) {
    if key.is_empty() {
        record.files.retain(|f| f != rel);
    } else if let Some(p) = record.parts.iter_mut().find(|p| p.key == key) {
        p.files.retain(|f| f != rel);
    }
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

/// Retire un dossier de rangement vide et ses parents vides, jusqu'à `stop`.
fn prune_store(dir: &Path, stop: &Path) {
    let mut cur = Some(dir.to_path_buf());
    while let Some(d) = cur {
        if d == stop || !d.starts_with(stop) || (d.exists() && fs::remove_dir(&d).is_err()) {
            break;
        }
        cur = d.parent().map(Path::to_path_buf);
    }
}

/// Retire les dossiers vides des fichiers gardés par `holder`.
pub fn remove_stores(game: &GameInstall, holder: i64, ctx: &Ctx) {
    let mut keys: Vec<String> = vec![String::new()];
    keys.extend(game.extra_folders.keys().cloned());
    for key in keys {
        for in_game in [true, false] {
            let (Some(dir), Some(folder)) = (store(game, &key, holder, in_game, ctx), game.folder_for(&key)) else { continue };
            if !dir.exists() {
                continue;
            }
            remove_empty_tree(&dir);
            let stop = if in_game { folder.join(DISABLED_DIR) } else { ctx.store_root.to_path_buf() };
            prune_store(&dir, &stop);
            if in_game {
                let _ = fs::remove_dir(folder.join(DISABLED_DIR));
            }
        }
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

/// Remet à leur place les fichiers recouverts par `holder` (dont les propres
/// fichiers à ces chemins sont déjà partis) : chez leur propriétaire s'il est
/// activé, avec ses fichiers mis de côté s'il est désactivé, supprimés s'il a
/// été désinstallé entre-temps.
pub fn release(game: &mut GameInstall, holder: i64, entries: Vec<Shadow>, ctx: &Ctx) {
    for s in entries.into_iter().rev() {
        let Some(src) = backup(game, holder, &s, ctx) else { continue };
        if !src.is_file() {
            continue;
        }
        let owner = game.mods.get(&s.owner).cloned();
        match owner {
            Some(o) if !o.disabled => {
                let Some(folder) = game.folder_for(&s.key).map(Path::to_path_buf) else { continue };
                let Some(dst) = installer::resolve_rel(&folder, &s.rel) else { continue };
                if dst.exists() {
                    // Un autre fichier a pris la place : celui-ci ne revient pas.
                    let _ = fs::remove_file(&src);
                    continue;
                }
                let Ok(created) = ensure_parents(&folder, &s.rel) else { continue };
                if installer::move_file_io(&src, &dst).is_ok()
                    && let Some(m) = game.mods.get_mut(&s.owner)
                {
                    claim(m, &s.key, &s.rel, created);
                }
            }
            Some(o) => {
                // Désactivé : rangé avec ses autres fichiers, il reviendra avec lui.
                let in_game = o.disabled_in.as_deref().map(|w| w == "game").unwrap_or_else(|| game.rules.is_some());
                let Some(dst) = toggle::store_for(game, &s.key, s.owner, in_game, ctx).and_then(|d| installer::resolve_rel(&d, &s.rel)) else {
                    continue;
                };
                if move_into(&src, &dst).is_ok()
                    && let Some(m) = game.mods.get_mut(&s.owner)
                {
                    claim(m, &s.key, &s.rel, Vec::new());
                }
            }
            None => {
                let _ = fs::remove_file(&src);
            }
        }
    }
}

/// Remet tous les fichiers recouverts par `holder` (retiré ou désactivé).
pub fn release_all(game: &mut GameInstall, holder: i64, ctx: &Ctx) {
    let entries = game.mods.get_mut(&holder).map(|m| std::mem::take(&mut m.shadowed)).unwrap_or_default();
    if entries.is_empty() {
        return;
    }
    release(game, holder, entries, ctx);
    remove_stores(game, holder, ctx);
}

/// Fichiers de `mod_id` cachés sous ceux d'autres mods : (mod qui les recouvre, entrée).
fn buried_of(game: &GameInstall, mod_id: i64) -> Vec<(i64, Shadow)> {
    game.mods
        .values()
        .filter(|m| m.mod_id != mod_id)
        .flat_map(|m| m.shadowed.iter().filter(|s| s.owner == mod_id).map(move |s| (m.mod_id, s.clone())))
        .collect()
}

fn drop_entry(game: &mut GameInstall, holder: i64, s: &Shadow) {
    if let Some(h) = game.mods.get_mut(&holder) {
        h.shadowed.retain(|x| x != s);
    }
}

/// `mod_id` est désinstallé : ses fichiers cachés sous d'autres mods sont supprimés.
pub fn forget_buried(game: &mut GameInstall, mod_id: i64, ctx: &Ctx) {
    for (holder, s) in buried_of(game, mod_id) {
        if let Some(p) = backup(game, holder, &s, ctx) {
            let _ = fs::remove_file(p);
        }
        drop_entry(game, holder, &s);
        remove_stores(game, holder, ctx);
    }
}

/// `mod_id` est désactivé : ses fichiers cachés sous d'autres mods rejoignent
/// ses fichiers mis de côté (`in_game` : rangés dans le dossier du jeu).
pub fn reclaim_buried(game: &mut GameInstall, mod_id: i64, in_game: bool, ctx: &Ctx) {
    for (holder, s) in buried_of(game, mod_id) {
        let src = backup(game, holder, &s, ctx);
        let dst = toggle::store_for(game, &s.key, mod_id, in_game, ctx).and_then(|d| installer::resolve_rel(&d, &s.rel));
        if let (Some(src), Some(dst)) = (src, dst)
            && src.is_file()
            && move_into(&src, &dst).is_ok()
            && let Some(m) = game.mods.get_mut(&mod_id)
        {
            claim(m, &s.key, &s.rel, Vec::new());
        }
        drop_entry(game, holder, &s);
        remove_stores(game, holder, ctx);
    }
}

/// Après une nouvelle version de `mod_id` : ses anciens fichiers cachés sous
/// d'autres mods qui ne le sont plus (`keep` : (dossier, chemin, mod qui recouvre)
/// de la nouvelle version) sont supprimés.
pub fn settle_buried(game: &mut GameInstall, mod_id: i64, keep: &[(String, String, i64)], ctx: &Ctx) {
    for (holder, s) in buried_of(game, mod_id) {
        if keep.iter().any(|(k, r, h)| *k == s.key && *r == s.rel && *h == holder) {
            continue;
        }
        if let Some(p) = backup(game, holder, &s, ctx) {
            let _ = fs::remove_file(p);
        }
        drop_entry(game, holder, &s);
    }
}

/// Enregistre ce qu'une installation de `mod_id` a recouvert ou caché :
/// (dossier, chemin, propriétaire) recouverts, (dossier, chemin, mod qui recouvre) cachés.
pub fn record_placement(
    game: &mut GameInstall,
    mod_id: i64,
    covered: &[(String, String, i64)],
    buried: &[(String, String, i64)],
    in_game: bool,
) {
    for (key, rel, owner) in covered {
        if let Some(o) = game.mods.get_mut(owner) {
            unclaim(o, key, rel);
        }
        if let Some(m) = game.mods.get_mut(&mod_id) {
            let s = Shadow { key: key.clone(), rel: rel.clone(), owner: *owner, in_game };
            if !m.shadowed.contains(&s) {
                m.shadowed.push(s);
            }
        }
    }
    for (key, rel, holder) in buried {
        if let Some(h) = game.mods.get_mut(holder) {
            let s = Shadow { key: key.clone(), rel: rel.clone(), owner: mod_id, in_game };
            if !h.shadowed.contains(&s) {
                h.shadowed.push(s);
            }
        }
    }
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
    fn released_files_go_back_to_their_owner() {
        let dir = tempdir("layers-release");
        let folder = dir.join("game");
        let root = dir.join("store");
        fs::create_dir_all(&folder).unwrap();
        let ctx = Ctx { store_root: &root, source: "nexus" };
        let owner = InstalledMod { mod_id: 1, files: vec!["Data/a.txt".into()], collection: mark(1), ..Default::default() };
        let holder = InstalledMod {
            mod_id: 2,
            collection: mark(2),
            shadowed: vec![Shadow { key: String::new(), rel: "Data/b.txt".into(), owner: 1, in_game: false }],
            ..Default::default()
        };
        let mut game = GameInstall {
            game_id: 7,
            folder: folder.clone(),
            layout: Layout::Flat,
            mods: BTreeMap::from([(1, owner), (2, holder)]),
            ..Default::default()
        };
        let kept = store(&game, "", 2, false, &ctx).unwrap().join("Data/b.txt");
        fs::create_dir_all(kept.parent().unwrap()).unwrap();
        fs::write(&kept, b"original").unwrap();

        release_all(&mut game, 2, &ctx);
        assert_eq!(fs::read(folder.join("Data/b.txt")).unwrap(), b"original");
        assert!(game.mods[&1].files.contains(&"Data/b.txt".to_string()));
        assert!(game.mods[&1].dirs.contains(&"Data".to_string()));
        assert!(game.mods[&2].shadowed.is_empty());
        assert!(!root.join("nexus-shadow").exists(), "dossiers de rangement vides retirés");
    }
}
