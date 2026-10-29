//! Détection des jeux qui installent leurs mods eux-mêmes grâce au kit de
//! développement (SDK) de mod.io.
//!
//! Ces jeux rangent les mods de leurs abonnés dans un dossier commun,
//! `C:\Users\Public\mod.io\<identifiant du jeu>` par défaut. L'emplacement peut
//! être changé dans `%LOCALAPPDATA%\mod.io\globalsettings.json`
//! (`RootLocalStoragePath`). Si ce dossier existe pour un jeu, c'est que le jeu
//! a déjà utilisé mod.io sur ce PC : s'abonner suffit, il téléchargera les mods.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ManagedByGame {
    /// Dossier où le jeu range les mods téléchargés via mod.io.
    pub folder: String,
    /// Nombre de mods que le jeu y a déjà téléchargés.
    pub mods: usize,
}

/// Dossier racine du SDK mod.io sur ce PC.
///
/// `local_appdata` : `%LOCALAPPDATA%` ; `public` : `%PUBLIC%`.
pub fn sdk_root(local_appdata: Option<&Path>, public: Option<&Path>) -> Option<PathBuf> {
    if let Some(custom) = local_appdata.and_then(read_custom_root) {
        return Some(custom);
    }
    public.map(|p| p.join("mod.io"))
}

/// Lit `RootLocalStoragePath` dans `globalsettings.json`, où qu'il soit rangé.
fn read_custom_root(local_appdata: &Path) -> Option<PathBuf> {
    let raw = fs::read_to_string(local_appdata.join("mod.io").join("globalsettings.json")).ok()?;
    let json: Value = serde_json::from_str(raw.trim_start_matches('\u{feff}')).ok()?;
    let path = find_key(&json, "RootLocalStoragePath")?;
    let trimmed = path.trim();
    (!trimmed.is_empty()).then(|| PathBuf::from(trimmed))
}

fn find_key<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    match value {
        Value::Object(map) => map.iter().find_map(|(k, v)| {
            if k.eq_ignore_ascii_case(key) {
                v.as_str()
            } else {
                find_key(v, key)
            }
        }),
        Value::Array(items) => items.iter().find_map(|v| find_key(v, key)),
        _ => None,
    }
}

/// Le jeu `game_id` a-t-il déjà son dossier mod.io sur ce PC ?
pub fn managed_by_game(root: &Path, game_id: i64) -> Option<ManagedByGame> {
    if game_id <= 0 {
        return None;
    }
    let folder = root.join(game_id.to_string());
    if !folder.is_dir() {
        return None;
    }
    let mods = fs::read_dir(folder.join("mods"))
        .map(|entries| entries.flatten().filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false)).count())
        .unwrap_or(0);
    Some(ManagedByGame {
        folder: folder.to_string_lossy().into_owned(),
        mods,
    })
}

/// Détection avec les variables d'environnement de Windows.
pub fn detect(game_id: i64) -> Option<ManagedByGame> {
    let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    let public = std::env::var_os("PUBLIC").map(PathBuf::from);
    let root = sdk_root(local.as_deref(), public.as_deref())?;
    managed_by_game(&root, game_id)
}

#[tauri::command]
pub fn detect_modio_install(game_id: i64) -> Option<ManagedByGame> {
    detect(game_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::installer::tests::tempdir;

    #[test]
    fn default_root_is_public_modio() {
        let local = tempdir("local-empty");
        let public = Path::new(r"C:\Users\Public");
        assert_eq!(sdk_root(Some(&local), Some(public)).unwrap(), public.join("mod.io"));
        assert!(sdk_root(Some(&local), None).is_none());
    }

    #[test]
    fn custom_root_from_global_settings() {
        let local = tempdir("local-custom");
        fs::create_dir_all(local.join("mod.io")).unwrap();
        // Avec BOM et clé imbriquée : on la trouve quand même.
        fs::write(
            local.join("mod.io/globalsettings.json"),
            "\u{feff}{\"Settings\":{\"RootLocalStoragePath\":\"D:/ModsModio/\"}}",
        )
        .unwrap();
        assert_eq!(sdk_root(Some(&local), Some(Path::new("/p"))).unwrap(), PathBuf::from("D:/ModsModio/"));

        // Valeur vide : retour au dossier par défaut.
        fs::write(local.join("mod.io/globalsettings.json"), r#"{"RootLocalStoragePath":"  "}"#).unwrap();
        assert_eq!(sdk_root(Some(&local), Some(Path::new("/p"))).unwrap(), Path::new("/p").join("mod.io"));
    }

    #[test]
    fn detects_games_that_already_used_modio() {
        let root = tempdir("sdk-root");
        assert!(managed_by_game(&root, 5289).is_none());

        fs::create_dir_all(root.join("5289/mods/111")).unwrap();
        fs::create_dir_all(root.join("5289/mods/222")).unwrap();
        fs::write(root.join("5289/mods/notes.txt"), b"").unwrap();
        let found = managed_by_game(&root, 5289).unwrap();
        assert_eq!(found.mods, 2);
        assert!(found.folder.ends_with("5289"));

        // Dossier du jeu sans encore de mods.
        fs::create_dir_all(root.join("3809")).unwrap();
        assert_eq!(managed_by_game(&root, 3809).unwrap().mods, 0);
        assert!(managed_by_game(&root, 0).is_none());
    }
}
