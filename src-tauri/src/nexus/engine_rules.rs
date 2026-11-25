//! Rangement des mods selon le moteur du jeu (Unreal, Unity, Bethesda).
//!
//! Toutes les destinations sont relatives au dossier d'installation du jeu.

use std::fs;
use std::path::{Path, PathBuf};

use super::rules::RulePlan;
use crate::error::{AppError, AppResult};
use crate::games::engine::{self, BethesdaGame};
use crate::installer::{self, rel_string, safe_name};

/// Outil dont un mod a besoin pour se charger.
pub type NeededTool = Option<&'static str>;

fn split(rel: &Path) -> Vec<String> {
    rel_string(rel).split('/').map(str::to_string).collect()
}

fn lower(s: &str) -> String {
    s.to_ascii_lowercase()
}

fn ext(name: &str) -> String {
    name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default()
}

fn is_doc(name: &str) -> bool {
    matches!(
        ext(name).as_str(),
        "txt" | "md" | "url" | "pdf" | "png" | "jpg" | "jpeg" | "gif" | "webp" | "html" | "htm" | "rtf"
    )
}

/// Fichiers extraits, en composants de chemin.
fn files_of(extracted: &Path) -> AppResult<Vec<Vec<String>>> {
    let files: Vec<Vec<String>> = installer::walk_files(extracted)?.iter().map(|p| split(p)).collect();
    if files.is_empty() {
        return Err(AppError::BadArchive(crate::tr!("l'archive est vide", "the archive is empty")));
    }
    Ok(files)
}

/// Nombre de dossiers d'emballage communs à retirer (`MonMod-v1/…`), sans
/// jamais retirer un dossier reconnu (`keep`).
fn wrapper_depth(files: &[Vec<String>], keep: &dyn Fn(&str) -> bool) -> usize {
    let useful: Vec<&Vec<String>> = files.iter().filter(|f| !(f.len() == 1 && is_doc(&f[0]))).collect();
    let mut depth = 0;
    while depth < 4 {
        let Some(first) = useful.first().and_then(|f| f.get(depth)) else { break };
        let all_same = useful.iter().all(|f| f.len() > depth + 1 && f[depth].eq_ignore_ascii_case(first));
        if all_same && !keep(first) {
            depth += 1;
        } else {
            break;
        }
    }
    depth
}

// ---------------------------------------------------------------- Unreal

/// Mods Unreal : fichiers `.pak` dans `Content/Paks/~mods`, mods UE4SS
/// (`Scripts/main.lua`, `dlls/main.dll`) dans le dossier des mods UE4SS,
/// mods « Blueprint » dans `Content/Paks/LogicMods`.
pub fn unreal_plan(extracted: &Path, game_root: &Path, project: &str, mod_name: &str) -> AppResult<(RulePlan, NeededTool)> {
    let files = files_of(extracted)?;
    let mut plan = RulePlan::default();
    let mut needs = None;
    let project_l = lower(project);
    let ue4ss_mods = engine::ue4ss_mods_dir(game_root, project);
    let paks = format!("{project}/Content/Paks");

    let keep = |c: &str| {
        let c = lower(c);
        c == project_l || matches!(c.as_str(), "binaries" | "content" | "mods" | "ue4ss" | "logicmods" | "paks" | "~mods")
    };
    let strip = wrapper_depth(&files, &keep);
    let push = |plan: &mut RulePlan, f: &Vec<String>, dest: String| plan.push_root(extracted.join(f.join("/")), dest);

    // Dossiers de mods UE4SS : ceux qui contiennent `Scripts/main.lua` ou `dlls/main.dll`.
    let mut ue4ss_dirs: Vec<Vec<String>> = files
        .iter()
        .filter_map(|f| {
            let n = f.len();
            let hit = n >= 2
                && ((lower(&f[n - 2]) == "scripts" && lower(&f[n - 1]) == "main.lua")
                    || (lower(&f[n - 2]) == "dlls" && lower(&f[n - 1]) == "main.dll"));
            hit.then(|| f[..n - 2].to_vec())
        })
        .collect();
    ue4ss_dirs.sort();
    ue4ss_dirs.dedup();

    for f in &files {
        let inner = &f[strip.min(f.len() - 1)..];
        let name = &f[f.len() - 1];
        let first = lower(&inner[0]);

        // 1. Mod UE4SS.
        if let Some(dir) = ue4ss_dirs.iter().find(|d| f.len() > d.len() && f.starts_with(d)) {
            needs = Some("ue4ss");
            let mod_dir = dir.last().map(|d| safe_name(d)).unwrap_or_else(|| safe_name(mod_name));
            push(&mut plan, f, format!("{ue4ss_mods}/{mod_dir}/{}", f[dir.len()..].join("/")));
            continue;
        }
        // 2. Archive déjà rangée comme le dossier du jeu.
        if first == project_l && inner.len() > 1 {
            if inner.iter().any(|c| lower(c) == "logicmods") || inner.iter().any(|c| lower(c) == "ue4ss") {
                needs = Some("ue4ss");
            }
            push(&mut plan, f, format!("{project}/{}", inner[1..].join("/")));
            continue;
        }
        if matches!(first.as_str(), "binaries" | "content") && inner.len() > 1 {
            push(&mut plan, f, format!("{project}/{}", inner.join("/")));
            continue;
        }
        // 3. Fichiers de paquets Unreal.
        if matches!(ext(name).as_str(), "pak" | "ucas" | "utoc" | "sig") {
            let logic = inner.iter().any(|c| lower(c) == "logicmods");
            if logic {
                needs = Some("ue4ss");
                push(&mut plan, f, format!("{paks}/LogicMods/{name}"));
            } else {
                // Dossier propre au mod sous ~mods, pour ne pas mélanger les fichiers.
                let sub: Vec<&String> = inner[..inner.len() - 1]
                    .iter()
                    .filter(|c| !matches!(lower(c).as_str(), "~mods" | "paks" | "content"))
                    .collect();
                let rel = if sub.is_empty() {
                    name.clone()
                } else {
                    format!("{}/{name}", sub.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("/"))
                };
                push(&mut plan, f, format!("{paks}/~mods/{rel}"));
            }
            continue;
        }
        if is_doc(name) {
            continue;
        }
        return Err(AppError::UnknownLayout(crate::tr!(
            "ModStudio ne sait pas où ranger « {} » dans ce jeu Unreal : suis les instructions de la page du mod.",
            "ModStudio doesn't know where to put \"{}\" in this Unreal game: follow the instructions on the mod page.",
            inner.join("/")
        )));
    }
    if plan.is_empty() {
        return Err(AppError::UnknownLayout(crate::tr!(
            "Cette archive ne contient aucun fichier de mod Unreal (.pak) ni mod UE4SS.",
            "This archive contains no Unreal mod file (.pak) or UE4SS mod."
        )));
    }
    // UE4SS charge un mod si son dossier contient `enabled.txt`.
    for dir in &ue4ss_dirs {
        let mod_dir = dir.last().map(|d| safe_name(d)).unwrap_or_else(|| safe_name(mod_name));
        plan.add_generated(format!("{ue4ss_mods}/{mod_dir}/enabled.txt"));
    }
    Ok((plan, needs))
}

// ---------------------------------------------------------------- Unity

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnityLoader {
    BepInEx,
    MelonLoader,
}

impl UnityLoader {
    fn label(self) -> &'static str {
        match self {
            UnityLoader::BepInEx => "BepInEx",
            UnityLoader::MelonLoader => "MelonLoader",
        }
    }
}

/// Chargeur qu'attendent les DLL du mod, d'après les noms qu'elles citent :
/// un mod MelonLoader cite `MelonLoader`, un plugin BepInEx cite `BepInEx`
/// (noms d'assemblages et d'espaces de noms, écrits tels quels dans la DLL).
/// `None` si elles n'en citent aucun, ou les deux.
fn dll_loader(extracted: &Path, files: &[Vec<String>]) -> Option<UnityLoader> {
    let contains = |bytes: &[u8], needle: &[u8]| bytes.windows(needle.len()).any(|w| w == needle);
    let mut found = None;
    for f in files.iter().filter(|f| lower(&f[f.len() - 1]).ends_with(".dll")) {
        let path = extracted.join(f.join("/"));
        if fs::metadata(&path).map(|m| m.len() > 64 * 1024 * 1024).unwrap_or(true) {
            continue;
        }
        let Ok(bytes) = fs::read(&path) else { continue };
        let this = match (contains(&bytes, b"MelonLoader"), contains(&bytes, b"BepInEx")) {
            (true, false) => UnityLoader::MelonLoader,
            (false, true) => UnityLoader::BepInEx,
            _ => continue,
        };
        match found {
            Some(other) if other != this => return None,
            _ => found = Some(this),
        }
    }
    found
}

/// Fichiers d'un chargeur de mods (BepInEx, MelonLoader) ou d'un injecteur
/// « autonome », chemin relatif à la racine de l'archive (en minuscules).
fn is_loader_file(inner: &[String]) -> bool {
    let first = lower(&inner[0]);
    if inner.len() == 1 {
        return matches!(
            first.as_str(),
            "winhttp.dll" | "version.dll" | "doorstop_config.ini" | ".doorstop_version" | "dobby.dll"
        );
    }
    first == "melonloader"
        || first == "dotnet"
        || (first == "bepinex" && matches!(lower(&inner[1]).as_str(), "core" | "unity-libs" | "interop" | "cache"))
}

/// Mods Unity : plugins BepInEx (`BepInEx/plugins`) ou mods MelonLoader (`Mods`).
/// `present` : outils déjà installés dans le jeu (`bepinex`, `melonloader`).
pub fn unity_plan(extracted: &Path, loader: UnityLoader, mod_name: &str, present: &[String]) -> AppResult<(RulePlan, NeededTool)> {
    let files = files_of(extracted)?;
    let mut plan = RulePlan::default();
    let keep = |c: &str| {
        matches!(
            lower(c).as_str(),
            "bepinex" | "plugins" | "patchers" | "config" | "mods" | "userlibs" | "userdata"
        )
    };
    let strip = wrapper_depth(&files, &keep);
    let inners: Vec<(&Vec<String>, &[String])> = files.iter().map(|f| (f, &f[strip.min(f.len() - 1)..])).collect();
    let has = |dir: &str| inners.iter().any(|(_, i)| i.len() > 1 && lower(&i[0]) == dir);
    let game_has = |t: &str| present.iter().any(|p| p == t);

    // Version « autonome » : l'archive apporte son propre chargeur ou injecteur.
    if inners.iter().any(|(_, i)| is_loader_file(i)) {
        let full_bepinex = inners.iter().any(|(_, i)| i.len() > 2 && lower(&i[0]) == "bepinex" && lower(&i[1]) == "core");
        if full_bepinex && game_has("bepinex") {
            // BepInEx fourni avec le mod alors qu'il est déjà installé : on ne garde que le mod.
        } else if game_has("bepinex") || game_has("melonloader") {
            let current = if game_has("bepinex") { "BepInEx" } else { "MelonLoader" };
            return Err(AppError::UnknownLayout(crate::tr!(
                "Ce fichier est la version « autonome » du mod : il apporte son propre chargeur (winhttp.dll, \
                 version.dll…), qui entrerait en conflit avec {current}, déjà installé dans ce jeu. Choisis la version \
                 {current} du mod dans la liste des fichiers.",
                "This file is the \"standalone\" version of the mod: it brings its own loader (winhttp.dll, \
                 version.dll…), which would conflict with {current}, already installed in this game. Pick the \
                 {current} version of the mod in the file list."
            )));
        } else {
            // Aucun chargeur dans le jeu : le mod s'installe tel quel dans le dossier du jeu.
            for (f, inner) in &inners {
                if inner.len() == 1 && is_doc(&inner[0]) {
                    continue;
                }
                plan.push_root(extracted.join(f.join("/")), inner.join("/"));
            }
            return Ok((plan, None));
        }
    }

    // Quel chargeur ce mod vise-t-il ? D'après l'archive, sinon d'après ses DLL.
    let target = if has("bepinex") || has("plugins") || has("patchers") {
        UnityLoader::BepInEx
    } else if has("mods") || has("userlibs") {
        UnityLoader::MelonLoader
    } else {
        dll_loader(extracted, &files).unwrap_or(loader)
    };
    // Fait pour l'autre chargeur que celui du jeu : il ne se chargerait pas.
    let current = if game_has("bepinex") {
        Some(UnityLoader::BepInEx)
    } else if game_has("melonloader") {
        Some(UnityLoader::MelonLoader)
    } else {
        None
    };
    if let Some(current) = current
        && current != target
    {
        let (made_for, current) = (target.label(), current.label());
        return Err(AppError::UnknownLayout(crate::tr!(
            "Ce fichier est fait pour {made_for}, alors que ce jeu utilise {current} : le mod ne se chargerait pas. \
             Choisis sa version {current} dans la liste des fichiers, s'il y en a une.",
            "This file is made for {made_for}, but this game uses {current}: the mod wouldn't load. \
             Pick its {current} version in the file list, if there is one."
        )));
    }
    let needs: NeededTool = Some(match target {
        UnityLoader::BepInEx => "bepinex",
        UnityLoader::MelonLoader => "melonloader",
    });
    // Archive rangée comme le dossier du jeu (`BepInEx/…` à la racine) : les
    // autres dossiers vont aussi à la racine du jeu.
    let root_relative = match target {
        UnityLoader::BepInEx => has("bepinex"),
        UnityLoader::MelonLoader => has("mods") || has("userlibs") || has("userdata") || has("plugins"),
    };
    // Paquet Thunderstore : ses métadonnées restent avec le plugin.
    let thunderstore = inners.iter().any(|(_, i)| i.len() == 1 && lower(&i[0]) == "manifest.json");

    let mod_dir = safe_name(mod_name);
    for (f, inner) in &inners {
        // Fichiers du chargeur fourni avec le mod, et sa configuration (créée par le chargeur installé).
        let loader_config = inner.len() == 3
            && lower(&inner[0]) == "bepinex"
            && lower(&inner[1]) == "config"
            && lower(&inner[2]) == "bepinex.cfg";
        if is_loader_file(inner) || loader_config {
            continue;
        }
        let name = &f[f.len() - 1];
        let first = lower(&inner[0]);
        let dest = match target {
            UnityLoader::BepInEx => {
                if first == "bepinex" && inner.len() > 1 {
                    format!("BepInEx/{}", inner[1..].join("/"))
                } else if matches!(first.as_str(), "plugins" | "patchers" | "config") && inner.len() > 1 {
                    format!("BepInEx/{first}/{}", inner[1..].join("/"))
                } else if inner.len() == 1
                    && thunderstore
                    && matches!(lower(name).as_str(), "manifest.json" | "icon.png" | "readme.md" | "changelog.md")
                {
                    format!("BepInEx/plugins/{mod_dir}/{name}")
                } else if inner.len() == 1 && is_doc(name) {
                    continue;
                } else if root_relative {
                    inner.join("/")
                } else {
                    format!("BepInEx/plugins/{mod_dir}/{}", inner.join("/"))
                }
            }
            UnityLoader::MelonLoader => {
                if matches!(first.as_str(), "mods" | "plugins" | "userlibs" | "userdata") && inner.len() > 1 {
                    let root = match first.as_str() {
                        "mods" => "Mods",
                        "plugins" => "Plugins",
                        "userlibs" => "UserLibs",
                        _ => "UserData",
                    };
                    format!("{root}/{}", inner[1..].join("/"))
                } else if inner.len() == 1 && is_doc(name) {
                    continue;
                } else if root_relative && inner.len() > 1 {
                    inner.join("/")
                } else {
                    format!("Mods/{}", inner.join("/"))
                }
            }
        };
        plan.push_root(extracted.join(f.join("/")), dest);
    }
    if plan.is_empty() {
        return Err(AppError::UnknownLayout(crate::tr!("Cette archive ne contient aucun fichier de mod.", "This archive contains no mod files.")));
    }
    Ok((plan, needs))
}

// ---------------------------------------------------------------- Bethesda

/// Dossiers et types de fichiers qui vont dans `Data`.
const DATA_DIRS: &[&str] = &[
    "meshes", "textures", "scripts", "interface", "sound", "music", "seq", "strings", "video", "materials",
    "lodsettings", "shadersfx", "grass", "skse", "f4se", "sfse", "nvse", "obse", "fose", "menus", "fonts",
    "distantlod", "lsdata", "trees", "facegen", "geometries", "platform", "particles", "calientetools",
];

fn is_plugin(name: &str) -> bool {
    matches!(ext(name).as_str(), "esp" | "esm" | "esl")
}

/// Mods Bethesda : contenu dans `Data` (plugins activés ensuite dans
/// `plugins.txt`), script extender à la racine du jeu.
pub fn bethesda_plan(extracted: &Path, game: &BethesdaGame) -> AppResult<(RulePlan, Vec<String>)> {
    let files = files_of(extracted)?;
    let mut plan = RulePlan::default();
    let mut plugins = Vec::new();
    let extender = lower(game.extender);

    // Installateur FOMOD trop enfoui dans l'archive pour être lu (ceux qu'on trouve passent par `fomod.rs`).
    let installer = files.iter().any(|f| f.last().is_some_and(|n| lower(n) == "moduleconfig.xml"));
    if installer {
        return Err(AppError::UnknownLayout(crate::tr!(
            "Ce mod a un installateur avec des options (FOMOD) que ModStudio n'a pas trouvé à l'endroit habituel : \
             installe-le à la main en suivant la page du mod.",
            "This mod has an installer with options (FOMOD) that ModStudio didn't find in the usual place: \
             install it manually by following the mod page."
        )));
    }

    let keep = |c: &str| {
        let c = lower(c);
        c == "data" || DATA_DIRS.contains(&c.as_str())
    };
    let strip = wrapper_depth(&files, &keep);

    // Script extender : son lanceur `<prefixe>_loader.exe` est dans l'archive.
    let is_extender = files.iter().any(|f| f.last().is_some_and(|n| lower(n) == format!("{extender}_loader.exe")));

    for f in &files {
        let inner = &f[strip.min(f.len() - 1)..];
        let name = &f[f.len() - 1];
        let first = lower(&inner[0]);
        if first == "src" || first == "fomod" {
            continue;
        }
        let dest = if first == "data" && inner.len() > 1 {
            format!("Data/{}", inner[1..].join("/"))
        } else if is_extender && inner.len() == 1 && matches!(ext(name).as_str(), "exe" | "dll" | "bin") {
            name.clone()
        } else if inner.len() == 1 && is_doc(name) {
            continue;
        } else if (inner.len() == 1 && (is_plugin(name) || matches!(ext(name).as_str(), "bsa" | "ba2" | "ini")))
            || DATA_DIRS.contains(&first.as_str())
        {
            format!("Data/{}", inner.join("/"))
        } else if is_extender {
            continue;
        } else {
            return Err(AppError::UnknownLayout(crate::tr!(
                "ModStudio ne sait pas où ranger « {} » : suis les instructions de la page du mod.",
                "ModStudio doesn't know where to put \"{}\": follow the instructions on the mod page.",
                inner.join("/")
            )));
        };
        let parts: Vec<&str> = dest.split('/').collect();
        if parts.len() == 2 && parts[0] == "Data" && is_plugin(parts[1]) {
            plugins.push(parts[1].to_string());
        }
        plan.push_root(extracted.join(f.join("/")), dest);
    }
    if plan.is_empty() {
        return Err(AppError::UnknownLayout(crate::tr!("Cette archive ne contient aucun fichier de mod.", "This archive contains no mod files.")));
    }
    Ok((plan, plugins))
}

// ---------------------------------------------------------------- plugins.txt

/// `plugins.txt` du jeu, sous `%LOCALAPPDATA%` (`local_appdata`).
pub fn plugins_file(local_appdata: &Path, game: &BethesdaGame) -> PathBuf {
    local_appdata.join(game.appdata).join("plugins.txt")
}

fn plugin_name(line: &str) -> Option<String> {
    let t = line.trim().trim_start_matches('*').trim();
    (!t.is_empty() && !t.starts_with('#')).then(|| t.to_ascii_lowercase())
}

/// Ajoute les plugins à la fin de l'ordre de chargement (s'ils n'y sont pas).
/// Les autres lignes ne sont jamais modifiées.
pub fn activate_plugins(path: &Path, plugins: &[String], star_format: bool) -> std::io::Result<Vec<String>> {
    let existing = std::fs::read_to_string(path).unwrap_or_default();
    let present: Vec<String> = existing.lines().filter_map(plugin_name).collect();
    let mut added = Vec::new();
    let mut out = existing.clone();
    for p in plugins {
        if present.contains(&p.to_ascii_lowercase()) || added.iter().any(|a: &String| a.eq_ignore_ascii_case(p)) {
            continue;
        }
        if !out.is_empty() && !out.ends_with('\n') {
            out.push_str("\r\n");
        }
        out.push_str(&if star_format { format!("*{p}\r\n") } else { format!("{p}\r\n") });
        added.push(p.clone());
    }
    if !added.is_empty() {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        // Copie de sécurité du fichier d'origine, une seule fois.
        let backup = path.with_extension("txt.modstudio-backup");
        if path.exists() && !backup.exists() {
            let _ = std::fs::copy(path, &backup);
        }
        let tmp = path.with_extension("txt.tmp");
        std::fs::write(&tmp, out)?;
        std::fs::rename(tmp, path)?;
    }
    Ok(added)
}

/// Retire les plugins de `plugins.txt` (et seulement eux).
pub fn deactivate_plugins(path: &Path, plugins: &[String]) -> std::io::Result<()> {
    let Ok(existing) = std::fs::read_to_string(path) else { return Ok(()) };
    let targets: Vec<String> = plugins.iter().map(|p| p.to_ascii_lowercase()).collect();
    let kept: Vec<&str> = existing
        .lines()
        .filter(|l| !plugin_name(l).is_some_and(|n| targets.contains(&n)))
        .collect();
    let mut out = kept.join("\r\n");
    if !out.is_empty() {
        out.push_str("\r\n");
    }
    if out != existing {
        let tmp = path.with_extension("txt.tmp");
        std::fs::write(&tmp, out)?;
        std::fs::rename(tmp, path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::installer::tests::tempdir;
    use std::fs;

    fn tree(tag: &str, files: &[&str]) -> PathBuf {
        let dir = tempdir(tag);
        for f in files {
            let p = dir.join(f);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, b"x").unwrap();
        }
        dir
    }

    fn dests(plan: &RulePlan) -> Vec<String> {
        let mut v = plan.root_destinations();
        v.sort();
        v
    }

    #[test]
    fn unreal_paks_and_ue4ss_mods() {
        let game = tempdir("ue-game");
        fs::create_dir_all(game.join("Pilgrim/Binaries/Win64")).unwrap();
        // Simple .pak (pas besoin d'UE4SS).
        let pak = tree("ue-pak", &["PushToCharge_P/PushToCharge_P.pak", "readme.txt"]);
        let (plan, needs) = unreal_plan(&pak, &game, "Pilgrim", "Push to Charge").unwrap();
        assert_eq!(dests(&plan), vec!["Pilgrim/Content/Paks/~mods/PushToCharge_P.pak"]);
        assert_eq!(needs, None);

        // Mod Lua UE4SS (avec dossier d'emballage) : rangé et activé.
        let lua = tree("ue-lua", &["InfiniteStamina-1.0/InfiniteStamina/Scripts/main.lua", "InfiniteStamina-1.0/InfiniteStamina/config.ini"]);
        let (plan, needs) = unreal_plan(&lua, &game, "Pilgrim", "Infinite Stamina").unwrap();
        assert_eq!(needs, Some("ue4ss"));
        assert_eq!(
            dests(&plan),
            vec![
                "Pilgrim/Binaries/Win64/ue4ss/Mods/InfiniteStamina/Scripts/main.lua",
                "Pilgrim/Binaries/Win64/ue4ss/Mods/InfiniteStamina/config.ini",
            ]
        );
        assert_eq!(plan.generated, vec!["Pilgrim/Binaries/Win64/ue4ss/Mods/InfiniteStamina/enabled.txt"]);

        // Mod Blueprint (LogicMods) et archive rangée comme le jeu.
        let logic = tree("ue-logic", &["LogicMods/MyMod.pak", "Pilgrim/Content/Paks/~mods/Other.pak"]);
        let (plan, needs) = unreal_plan(&logic, &game, "Pilgrim", "x").unwrap();
        assert_eq!(needs, Some("ue4ss"));
        assert_eq!(dests(&plan), vec!["Pilgrim/Content/Paks/LogicMods/MyMod.pak", "Pilgrim/Content/Paks/~mods/Other.pak"]);

        let odd = tree("ue-odd", &["Saved/SaveGames/slot.sav"]);
        assert!(matches!(unreal_plan(&odd, &game, "Pilgrim", "x").unwrap_err(), AppError::UnknownLayout(_)));
    }

    #[test]
    fn unity_mods_made_for_the_other_loader_are_refused() {
        let none: Vec<String> = Vec::new();
        let bepinex = vec!["bepinex".to_string()];
        let melon = vec!["melonloader".to_string()];
        fn made_for(err: &AppError, loader: &str) -> bool {
            matches!(err, AppError::UnknownLayout(t) if t.contains(loader))
        }

        // Version MelonLoader d'un mod (dossier Mods) pour un jeu à BepInEx (AutoPickup Melon, Megabonk).
        let mods = tree("un-melon-mods", &["Mods/AutoPickup.dll", "README.txt"]);
        let err = unity_plan(&mods, UnityLoader::BepInEx, "AutoPickup", &bepinex).unwrap_err();
        assert!(made_for(&err, "MelonLoader"), "{err}");

        // DLL seule : ce qu'elle cite dit pour quel chargeur elle est faite.
        let dll = tempdir("un-melon-dll");
        fs::write(dll.join("InstantCollect.dll"), b"MZ\0MelonLoader\0MelonMod\0Il2Cpp\0").unwrap();
        let err = unity_plan(&dll, UnityLoader::BepInEx, "InstantCollect", &bepinex).unwrap_err();
        assert!(made_for(&err, "BepInEx"), "{err}");
        // La même dans un jeu à MelonLoader, ou sans chargeur : rangée dans Mods.
        for present in [&melon, &none] {
            let (plan, needs) = unity_plan(&dll, UnityLoader::BepInEx, "InstantCollect", present).unwrap();
            assert_eq!(dests(&plan), vec!["Mods/InstantCollect.dll"]);
            assert_eq!(needs, Some("melonloader"));
        }

        // Plugin BepInEx dans un jeu à MelonLoader : refusé aussi.
        let plugin = tempdir("un-bep-dll");
        fs::write(plugin.join("StatHUD.dll"), b"MZ\0BepInEx.Unity.IL2CPP\0BasePlugin\0").unwrap();
        assert!(unity_plan(&plugin, UnityLoader::MelonLoader, "StatHUD", &melon).is_err());
        let (plan, _) = unity_plan(&plugin, UnityLoader::MelonLoader, "StatHUD", &bepinex).unwrap();
        assert_eq!(dests(&plan), vec!["BepInEx/plugins/StatHUD/StatHUD.dll"]);

        // DLL qui ne cite aucun des deux (ou les deux) : le chargeur du jeu.
        let both = tempdir("un-both-dll");
        fs::write(both.join("Both.dll"), b"MelonLoader BepInEx").unwrap();
        let (plan, _) = unity_plan(&both, UnityLoader::BepInEx, "Both", &bepinex).unwrap();
        assert_eq!(dests(&plan), vec!["BepInEx/plugins/Both/Both.dll"]);
    }

    #[test]
    fn unity_plugins_go_to_the_right_loader() {
        let dll = tree("un-dll", &["MyMod.dll", "README.txt"]);
        let none: Vec<String> = Vec::new();
        let bepinex = vec!["bepinex".to_string()];
        let (plan, needs) = unity_plan(&dll, UnityLoader::BepInEx, "My Mod", &none).unwrap();
        assert_eq!(dests(&plan), vec!["BepInEx/plugins/My Mod/MyMod.dll"]);
        assert_eq!(needs, Some("bepinex"));
        let (plan, needs) = unity_plan(&dll, UnityLoader::MelonLoader, "My Mod", &none).unwrap();
        assert_eq!(dests(&plan), vec!["Mods/MyMod.dll"]);
        assert_eq!(needs, Some("melonloader"));

        // L'archive dit elle-même quel chargeur elle vise.
        let bep = tree("un-bep", &["Pack/BepInEx/plugins/A/A.dll", "Pack/BepInEx/config/a.cfg"]);
        let (plan, _) = unity_plan(&bep, UnityLoader::MelonLoader, "A", &none).unwrap();
        assert_eq!(dests(&plan), vec!["BepInEx/config/a.cfg", "BepInEx/plugins/A/A.dll"]);
        let thunder = tree("un-ts", &["manifest.json", "icon.png", "README.md", "plugins/B.dll"]);
        let (plan, _) = unity_plan(&thunder, UnityLoader::BepInEx, "B", &none).unwrap();
        assert_eq!(
            dests(&plan),
            vec!["BepInEx/plugins/B.dll", "BepInEx/plugins/B/README.md", "BepInEx/plugins/B/icon.png", "BepInEx/plugins/B/manifest.json"]
        );

        // Plugin livré avec son dossier à la racine du jeu (Moon Sweepers Trainer, version BepInEx).
        let rooted = tree(
            "un-rooted",
            &[
                "BepInEx/plugins/MoonSweepersTrainer/MoonSweepersTrainer.BepInEx.dll",
                "MoonSweepersTrainer/MoonSweepersTrainer.dll",
                "MoonSweepersTrainer/MoonSweepersTrainer.ini",
                "CHANGELOG.txt",
                "README.txt",
            ],
        );
        let (plan, needs) = unity_plan(&rooted, UnityLoader::BepInEx, "Trainer", &bepinex).unwrap();
        assert_eq!(needs, Some("bepinex"));
        assert_eq!(
            dests(&plan),
            vec![
                "BepInEx/plugins/MoonSweepersTrainer/MoonSweepersTrainer.BepInEx.dll",
                "MoonSweepersTrainer/MoonSweepersTrainer.dll",
                "MoonSweepersTrainer/MoonSweepersTrainer.ini",
            ]
        );

        // Version « autonome » (injecteur winhttp.dll) : refusée si BepInEx est là…
        let standalone = tree("un-standalone", &["winhttp.dll", "MoonSweepersTrainer/MoonSweepersTrainer.dll", "README.txt"]);
        let err = unity_plan(&standalone, UnityLoader::BepInEx, "Trainer", &bepinex).unwrap_err();
        assert!(matches!(&err, AppError::UnknownLayout(m) if m.contains("version BepInEx")), "{err}");
        // … installée telle quelle sinon.
        let (plan, needs) = unity_plan(&standalone, UnityLoader::BepInEx, "Trainer", &none).unwrap();
        assert_eq!(needs, None);
        assert_eq!(dests(&plan), vec!["MoonSweepersTrainer/MoonSweepersTrainer.dll", "winhttp.dll"]);

        // BepInEx fourni avec le plugin alors qu'il est déjà installé : seul le plugin est gardé.
        let pack = tree("un-pack", &["winhttp.dll", "doorstop_config.ini", "BepInEx/core/BepInEx.Core.dll", "BepInEx/plugins/C/C.dll"]);
        let (plan, _) = unity_plan(&pack, UnityLoader::BepInEx, "C", &bepinex).unwrap();
        assert_eq!(dests(&plan), vec!["BepInEx/plugins/C/C.dll"]);
    }

    #[test]
    fn bethesda_data_and_script_extenders() {
        let sse = engine::bethesda("skyrimspecialedition").unwrap();
        let m = tree("bg-mod", &["Cool Mod/CoolMod.esp", "Cool Mod/meshes/a.nif", "Cool Mod/textures/b.dds", "Cool Mod/readme.txt"]);
        let (plan, plugins) = bethesda_plan(&m, sse).unwrap();
        assert_eq!(dests(&plan), vec!["Data/CoolMod.esp", "Data/meshes/a.nif", "Data/textures/b.dds"]);
        assert_eq!(plugins, vec!["CoolMod.esp"]);

        let data = tree("bg-data", &["Data/Other.esm", "Data/Scripts/x.pex"]);
        let (plan, plugins) = bethesda_plan(&data, sse).unwrap();
        assert_eq!(dests(&plan), vec!["Data/Other.esm", "Data/Scripts/x.pex"]);
        assert_eq!(plugins, vec!["Other.esm"]);

        let skse = tree(
            "bg-skse",
            &["skse64_2_02_06/skse64_loader.exe", "skse64_2_02_06/skse64_1_6_1170.dll", "skse64_2_02_06/Data/Scripts/Game.pex", "skse64_2_02_06/src/x.cpp", "skse64_2_02_06/skse64_readme.txt"],
        );
        let (plan, plugins) = bethesda_plan(&skse, sse).unwrap();
        assert_eq!(dests(&plan), vec!["Data/Scripts/Game.pex", "skse64_1_6_1170.dll", "skse64_loader.exe"]);
        assert!(plugins.is_empty());

        let fomod = tree("bg-fomod", &["fomod/ModuleConfig.xml", "00 Core/a.esp", "01 Option/b.esp"]);
        assert!(matches!(bethesda_plan(&fomod, sse).unwrap_err(), AppError::UnknownLayout(_)));
    }

    #[test]
    fn plugins_txt_is_only_appended_to() {
        let dir = tempdir("plugins");
        let path = dir.join("Skyrim Special Edition/plugins.txt");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "# This file is used by the game\r\n*Unofficial Skyrim Special Edition Patch.esp\r\nDisabled.esp\r\n").unwrap();
        let added = activate_plugins(&path, &["CoolMod.esp".into(), "disabled.esp".into()], true).unwrap();
        assert_eq!(added, vec!["CoolMod.esp"], "un plugin déjà listé (même désactivé) n'est pas touché");
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.ends_with("Disabled.esp\r\n*CoolMod.esp\r\n"), "{text:?}");
        assert!(path.with_extension("txt.modstudio-backup").exists());

        deactivate_plugins(&path, &["CoolMod.esp".into()]).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(!text.contains("CoolMod") && text.contains("*Unofficial Skyrim Special Edition Patch.esp"));

        // Ancien format (sans astérisque) et fichier absent.
        let nv = dir.join("FalloutNV/plugins.txt");
        activate_plugins(&nv, &["NewVegasMod.esp".into()], false).unwrap();
        assert_eq!(fs::read_to_string(&nv).unwrap(), "NewVegasMod.esp\r\n");
    }
}
