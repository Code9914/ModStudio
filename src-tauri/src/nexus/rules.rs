//! Règles d'installation propres à certains jeux de Nexus Mods.
//!
//! Bonelab : les archives contiennent soit du contenu (« pallets » : avatars,
//! cartes, armes), à ranger dans `AppData\LocalLow\Stress Level Zero\BONELAB\Mods`,
//! soit des mods de code MelonLoader (`.dll`), à ranger dans le dossier
//! d'installation du jeu (`Mods`, `Plugins`, `UserLibs`…). Beaucoup d'archives
//! contiennent les versions PC et Quest côte à côte.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::error::{AppError, AppResult};
use crate::installer::{self, rel_string, safe_name};

/// Identifiant de Bonelab sur Nexus Mods.
pub const BONELAB: i64 = 4832;
/// Clé du dossier d'installation du jeu dans `GameInstall::extra_folders`.
pub const GAME_DIR: &str = "game";

/// Règles connues pour un jeu Nexus.
pub fn rules_for(game_id: i64) -> Option<&'static str> {
    match game_id {
        BONELAB => Some("bonelab"),
        _ => None,
    }
}

/// Dossier des mods suggéré pour un jeu Nexus (même dossier que pour mod.io).
pub fn content_folder(game_id: i64) -> Option<String> {
    let modio_id = match game_id {
        BONELAB => 3809,
        _ => return None,
    };
    let (template, _) = installer::known_folder(modio_id)?;
    installer::expand_env(template, |name| std::env::var(name).ok())
}

/// Ce qu'il faut déplacer, par dossier de destination (`""` : dossier des mods).
#[derive(Debug, Default)]
pub struct RulePlan {
    pub batches: BTreeMap<String, Vec<(PathBuf, String)>>,
    /// Fichiers vides à créer dans le dossier principal (`enabled.txt` d'UE4SS…).
    pub generated: Vec<String>,
}

impl RulePlan {
    pub fn needs_game_dir(&self) -> bool {
        self.batches.get(GAME_DIR).is_some_and(|m| !m.is_empty())
    }

    fn push(&mut self, key: &str, src: PathBuf, dest: String) {
        self.batches.entry(key.to_string()).or_default().push((src, dest));
    }

    /// Fichier pour le dossier principal du jeu.
    pub fn push_root(&mut self, src: PathBuf, dest: String) {
        self.push("", src, dest);
    }

    pub fn add_generated(&mut self, rel: String) {
        self.generated.push(rel);
    }

    pub fn is_empty(&self) -> bool {
        self.batches.values().all(Vec::is_empty)
    }

    #[cfg(test)]
    pub fn root_destinations(&self) -> Vec<String> {
        self.batches.get("").map(|m| m.iter().map(|(_, d)| d.clone()).collect()).unwrap_or_default()
    }
}

/// Dossiers MelonLoader qu'un mod de code peut contenir, avec leur nom exact.
const MELON_DIRS: &[&str] = &["Mods", "Plugins", "UserLibs", "UserData"];

fn is_pallet_file(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n == "pallet.json" || n.ends_with(".pallet.json")
}

/// Le chemin désigne-t-il la version Quest (Android) d'un mod ?
fn is_quest_path(components: &[String]) -> bool {
    components.iter().any(|c| {
        let c = c.to_ascii_lowercase();
        c.contains("quest") || c.contains("android") || c.contains("oculus")
    })
}

fn is_doc(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    [".txt", ".md", ".url", ".pdf", ".png", ".jpg", ".jpeg", ".gif", ".webp", ".html", ".htm", ".rtf"]
        .iter()
        .any(|ext| n.ends_with(ext))
}

fn parts(rel: &Path) -> Vec<String> {
    rel_string(rel).split('/').map(str::to_string).collect()
}

/// Plan d'installation d'une archive Bonelab déjà extraite dans `extracted`.
pub fn bonelab_plan(extracted: &Path, mod_name: &str) -> AppResult<RulePlan> {
    let files: Vec<Vec<String>> = installer::walk_files(extracted)?.iter().map(|p| parts(p)).collect();
    if files.is_empty() {
        return Err(AppError::BadArchive("l'archive est vide".into()));
    }
    let mut plan = RulePlan::default();

    // 1. Contenu : chaque dossier qui contient un fichier « pallet ».
    let mut pallet_dirs: Vec<Vec<String>> = files
        .iter()
        .filter(|f| is_pallet_file(f.last().map(String::as_str).unwrap_or("")))
        .map(|f| f[..f.len() - 1].to_vec())
        .collect();
    pallet_dirs.sort();
    pallet_dirs.dedup();
    // Un pallet rangé dans le dossier d'un autre fait partie de celui-ci.
    let outer: Vec<Vec<String>> = pallet_dirs
        .iter()
        .filter(|d| !pallet_dirs.iter().any(|o| o.len() < d.len() && d.starts_with(o)))
        .cloned()
        .collect();

    if !outer.is_empty() {
        // Seuls les dossiers parents comptent : le nom du pallet lui-même
        // peut contenir « Quest » sans être la version Quest.
        let pc: Vec<&Vec<String>> = outer
            .iter()
            .filter(|d| d.is_empty() || !is_quest_path(&d[..d.len() - 1]))
            .collect();
        if pc.is_empty() {
            return Err(AppError::WrongPlatform(
                "Ce fichier contient la version Quest de ce mod. Choisis la version PC dans la liste des fichiers.".into(),
            ));
        }
        let mut names: BTreeMap<String, ()> = BTreeMap::new();
        for dir in pc {
            let name = match dir.last() {
                Some(last) => safe_name(last),
                None => root_pallet_name(&files).unwrap_or_else(|| safe_name(mod_name)),
            };
            if names.insert(name.to_ascii_lowercase(), ()).is_some() {
                return Err(AppError::UnknownLayout(format!(
                    "L'archive contient plusieurs variantes du même contenu (« {name} ») : installe la bonne à la main en suivant la page du mod."
                )));
            }
            for f in files.iter().filter(|f| f.len() > dir.len() && f.starts_with(dir)) {
                let inner = f[dir.len()..].join("/");
                plan.push("", extracted.join(f.join("/")), format!("{name}/{inner}"));
            }
        }
    }

    // 2. Code : ce qui reste en dehors des pallets (et des versions Quest).
    let rest: Vec<&Vec<String>> = files
        .iter()
        .filter(|f| !outer.iter().any(|d| f.len() > d.len() && f.starts_with(d)))
        .filter(|f| !is_quest_path(&f[..f.len() - 1]))
        .collect();
    let code = code_moves(extracted, &rest)?;
    for (src, dest) in code {
        plan.push(GAME_DIR, src, dest);
    }

    if plan.batches.values().all(Vec::is_empty) {
        let save = files.iter().any(|f| f.last().is_some_and(|n| n.to_ascii_lowercase().ends_with(".save.json")));
        return Err(AppError::UnknownLayout(if save {
            "C'est une sauvegarde de partie (fichier .save.json) : ModStudio ne remplace pas tes sauvegardes. \
             Pour l'utiliser, garde une copie de la tienne puis suis les instructions de la page du mod."
                .into()
        } else {
            "ModStudio ne sait pas où installer ce fichier : il ne contient ni contenu Bonelab (pallet) ni mod MelonLoader (.dll). \
             Suis les instructions de la page du mod."
                .into()
        }));
    }
    Ok(plan)
}

/// Nom du dossier d'un pallet placé à la racine de l'archive : d'après le
/// fichier `Auteur.Mod.pallet.json` ou `catalog_Auteur.Mod.json`.
fn root_pallet_name(files: &[Vec<String>]) -> Option<String> {
    let top: Vec<&str> = files.iter().filter(|f| f.len() == 1).map(|f| f[0].as_str()).collect();
    let from = |name: &str, prefix: &str, suffix: &str| {
        let lower = name.to_ascii_lowercase();
        (lower.starts_with(prefix) && lower.ends_with(suffix) && name.len() > prefix.len() + suffix.len())
            .then(|| safe_name(&name[prefix.len()..name.len() - suffix.len()]))
    };
    top.iter()
        .find_map(|n| from(n, "", ".pallet.json"))
        .or_else(|| top.iter().find_map(|n| from(n, "catalog_", ".json")))
}

/// Fichiers d'un mod de code, relatifs au dossier du jeu.
fn code_moves(extracted: &Path, files: &[&Vec<String>]) -> AppResult<Vec<(PathBuf, String)>> {
    let useful: Vec<&Vec<String>> = files.iter().copied().filter(|f| !(f.len() == 1 && is_doc(&f[0]))).collect();
    if !useful.iter().any(|f| f.last().is_some_and(|n| n.to_ascii_lowercase().ends_with(".dll"))) {
        return Ok(Vec::new());
    }
    if useful.iter().any(|f| {
        f.iter().any(|c| c.eq_ignore_ascii_case("MelonLoader"))
            || f.last().is_some_and(|n| n.eq_ignore_ascii_case("version.dll"))
    }) {
        return Err(AppError::UnknownLayout(
            "Ce fichier contient MelonLoader lui-même : installe-le avec l'installeur officiel de MelonLoader.".into(),
        ));
    }

    // Retire les dossiers d'emballage (`MonMod-v1/Mods/x.dll` → `Mods/x.dll`).
    let mut strip = 0;
    loop {
        let firsts: Vec<&str> = useful.iter().filter_map(|f| f.get(strip).map(String::as_str)).collect();
        let single = useful.iter().all(|f| f.len() > strip + 1)
            && firsts.windows(2).all(|w| w[0] == w[1])
            && !firsts.is_empty()
            && !MELON_DIRS.iter().any(|d| d.eq_ignore_ascii_case(firsts[0]));
        if single && strip < 4 {
            strip += 1;
        } else {
            break;
        }
    }

    let melon_root = |c: &str| MELON_DIRS.iter().find(|d| d.eq_ignore_ascii_case(c)).copied();
    let structured = useful.iter().any(|f| f.len() > strip + 1 && melon_root(&f[strip]).is_some());
    let mut out = Vec::new();
    for f in &useful {
        let inner = &f[strip..];
        let dest = if structured {
            match melon_root(&inner[0]) {
                Some(root) if inner.len() > 1 => format!("{root}/{}", inner[1..].join("/")),
                // Fichier à côté des dossiers MelonLoader (lisez-moi…) : ignoré.
                _ if inner.len() == 1 && is_doc(&inner[0]) => continue,
                _ => {
                    return Err(AppError::UnknownLayout(format!(
                        "ModStudio ne sait pas où ranger « {} » : suis les instructions de la page du mod.",
                        inner.join("/")
                    )));
                }
            }
        } else {
            if inner.len() == 1 && is_doc(&inner[0]) {
                continue;
            }
            // `.dll` à la racine : tout va dans `Mods` (avec leurs dossiers de données).
            format!("Mods/{}", inner.join("/"))
        };
        out.push((extracted.join(f.join("/")), dest));
    }
    Ok(out)
}

// ---------------------------------------------------------------- Dossier du jeu

/// Cherche le dossier d'installation de Bonelab (Steam, puis application Meta).
pub fn detect_bonelab_dir() -> Option<PathBuf> {
    let roots = crate::games::steam::steam_roots();
    let mut candidates: Vec<PathBuf> = crate::games::steam::libraries(&roots)
        .into_iter()
        .map(|lib| lib.join("steamapps").join("common").join("BONELAB"))
        .collect();
    if let Ok(p) = std::env::var("ProgramFiles") {
        candidates.push(
            PathBuf::from(p)
                .join("Oculus")
                .join("Software")
                .join("Software")
                .join("stress-level-zero-inc-bonelab"),
        );
    }
    candidates.into_iter().find(|c| installer::looks_like_game_root(c))
}

/// MelonLoader est-il installé dans ce dossier de jeu ?
pub fn has_melonloader(game_dir: &Path) -> bool {
    game_dir.join("MelonLoader").is_dir()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::installer::tests::tempdir;
    use std::fs;

    /// Crée les fichiers donnés dans un dossier temporaire.
    fn tree(tag: &str, files: &[&str]) -> PathBuf {
        let dir = tempdir(tag);
        for f in files {
            let p = dir.join(f);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, b"x").unwrap();
        }
        dir
    }

    fn dests(plan: &RulePlan, key: &str) -> Vec<String> {
        let mut v: Vec<String> = plan.batches.get(key).map(|m| m.iter().map(|(_, d)| d.clone()).collect()).unwrap_or_default();
        v.sort();
        v
    }

    #[test]
    fn pallets_go_to_the_mods_folder() {
        // Forme la plus courante : un dossier « Auteur.Mod » avec son pallet.
        let dir = tree(
            "bl-pallet",
            &["BaBaCorp.711/BaBaCorp.711.pallet.json", "BaBaCorp.711/catalog_BaBaCorp.711.json", "BaBaCorp.711/a/711.bundle"],
        );
        let plan = bonelab_plan(&dir, "7-11 SANDBOX").unwrap();
        assert_eq!(
            dests(&plan, ""),
            vec!["BaBaCorp.711/BaBaCorp.711.pallet.json", "BaBaCorp.711/a/711.bundle", "BaBaCorp.711/catalog_BaBaCorp.711.json"]
        );
        assert!(!plan.needs_game_dir());
    }

    #[test]
    fn quest_versions_are_left_out() {
        let dir = tree(
            "bl-quest",
            &[
                "PC/Yernemm.Rusks/pallet.json",
                "PC/Yernemm.Rusks/x.bundle",
                "Quest 2/Yernemm.Rusks/pallet.json",
                "Quest 2/Yernemm.Rusks/x.bundle",
                "Lisez-moi.txt",
            ],
        );
        let plan = bonelab_plan(&dir, "Rusk").unwrap();
        assert_eq!(dests(&plan, ""), vec!["Yernemm.Rusks/pallet.json", "Yernemm.Rusks/x.bundle"]);

        let quest_only = tree("bl-android", &["Android/Kyle.TestMap/pallet.json", "Android/Kyle.TestMap/m.bundle"]);
        assert!(matches!(bonelab_plan(&quest_only, "UFC").unwrap_err(), AppError::WrongPlatform(_)));
    }

    #[test]
    fn pallet_at_the_archive_root_gets_its_barcode_as_folder() {
        let dir = tree("bl-root", &["pallet.json", "catalog_tailflapper0319.HEUEU.json", "heueu_avatars/a.bundle"]);
        let plan = bonelab_plan(&dir, "Detective Ma").unwrap();
        assert_eq!(
            dests(&plan, ""),
            vec![
                "tailflapper0319.HEUEU/catalog_tailflapper0319.HEUEU.json",
                "tailflapper0319.HEUEU/heueu_avatars/a.bundle",
                "tailflapper0319.HEUEU/pallet.json"
            ]
        );
    }

    #[test]
    fn code_mods_go_to_the_game_folder() {
        // Archive déjà rangée comme le dossier du jeu.
        let dir = tree("bl-code", &["Mods/Bonelab_OWO.dll", "Mods/OWO/Explosion.owo", "UserLibs/OWO.dll", "README.txt"]);
        let plan = bonelab_plan(&dir, "OWO").unwrap();
        assert!(plan.needs_game_dir());
        assert_eq!(dests(&plan, GAME_DIR), vec!["Mods/Bonelab_OWO.dll", "Mods/OWO/Explosion.owo", "UserLibs/OWO.dll"]);

        // `.dll` à la racine, avec un dossier de données : tout dans Mods.
        let flat = tree("bl-dll", &["Bonelab_bhaptics.dll", "bHaptics/Gun.tact", "readme.md"]);
        let plan = bonelab_plan(&flat, "bhaptics").unwrap();
        assert_eq!(dests(&plan, GAME_DIR), vec!["Mods/Bonelab_bhaptics.dll", "Mods/bHaptics/Gun.tact"]);

        // Dossier d'emballage retiré.
        let wrapped = tree("bl-wrap", &["LabFusion-1.0/Mods/LabFusion.dll", "LabFusion-1.0/Plugins/Updater.dll"]);
        let plan = bonelab_plan(&wrapped, "Fusion").unwrap();
        assert_eq!(dests(&plan, GAME_DIR), vec!["Mods/LabFusion.dll", "Plugins/Updater.dll"]);
    }

    #[test]
    fn unknown_content_is_refused_with_an_explanation() {
        let save = tree("bl-save", &["slot_0.save.json"]);
        let AppError::UnknownLayout(text) = bonelab_plan(&save, "Save").unwrap_err() else { panic!() };
        assert!(text.contains("sauvegarde"));

        let source = tree("bl-src", &["source/model.fbx", "textures/a.png"]);
        assert!(matches!(bonelab_plan(&source, "Model").unwrap_err(), AppError::UnknownLayout(_)));

        let loader = tree("bl-ml", &["version.dll", "MelonLoader/net6/MelonLoader.dll"]);
        let AppError::UnknownLayout(text) = bonelab_plan(&loader, "ML").unwrap_err() else { panic!() };
        assert!(text.contains("MelonLoader"));
    }

    #[test]
    fn knows_which_games_have_rules() {
        assert_eq!(rules_for(BONELAB), Some("bonelab"));
        assert_eq!(rules_for(1704), None);
    }
}
