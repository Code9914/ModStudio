//! Jeux Windows lancés avec Proton (Steam sous Linux). Chaque jeu a son
//! « préfixe » (`steamapps/compatdata/<numéro>/pfx`), un faux disque C: où il
//! range ses réglages : `plugins.txt` des jeux Bethesda, mods de BONELAB…
//! Sous Windows, ces fonctions renvoient simplement les dossiers du système.

use std::fs;
use std::path::{Path, PathBuf};

use super::steam;
use crate::installer;

/// Numéro Steam de BONELAB.
pub const BONELAB_APP: &str = "1592190";

/// Numéro Steam d'un jeu d'après son dossier (`…/steamapps/common/<dossier>`),
/// lu dans les manifestes `appmanifest_<numéro>.acf`.
pub fn app_id_for_dir(game_dir: &Path) -> Option<String> {
    let common = game_dir.parent()?;
    if !common.file_name()?.to_string_lossy().eq_ignore_ascii_case("common") {
        return None;
    }
    let steamapps = common.parent()?;
    let wanted = game_dir.file_name()?.to_string_lossy().into_owned();
    fs::read_dir(steamapps).ok()?.flatten().find_map(|entry| {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !(name.starts_with("appmanifest_") && name.ends_with(".acf")) {
            return None;
        }
        let values = steam::top_level_values(&fs::read_to_string(entry.path()).ok()?);
        values
            .get("installdir")
            .filter(|d| d.eq_ignore_ascii_case(&wanted))
            .and_then(|_| values.get("appid").cloned())
    })
}

/// Préfixe d'un jeu parmi ces bibliothèques Steam.
pub fn prefix_in(app_id: &str, libraries: &[PathBuf]) -> Option<PathBuf> {
    libraries
        .iter()
        .map(|lib| lib.join("steamapps").join("compatdata").join(app_id).join("pfx"))
        .find(|pfx| pfx.join("drive_c").is_dir())
}

/// Préfixe Proton d'un jeu Steam (jamais sous Windows).
pub fn game_prefix(app_id: &str) -> Option<PathBuf> {
    if cfg!(windows) {
        return None;
    }
    prefix_in(app_id, &steam::libraries(&steam::steam_roots()))
}

/// Préfixe du jeu installé dans `game_dir` (jamais sous Windows).
pub fn prefix_for_dir(game_dir: &Path) -> Option<PathBuf> {
    if cfg!(windows) {
        return None;
    }
    game_prefix(&app_id_for_dir(game_dir)?)
}

/// Tous les préfixes Proton (jamais sous Windows).
pub fn all_prefixes() -> Vec<PathBuf> {
    if cfg!(windows) {
        return Vec::new();
    }
    let mut out = Vec::new();
    for lib in steam::libraries(&steam::steam_roots()) {
        let Ok(entries) = fs::read_dir(lib.join("steamapps").join("compatdata")) else { continue };
        out.extend(entries.flatten().map(|e| e.path().join("pfx")).filter(|p| p.join("drive_c").is_dir()));
    }
    out
}

/// Variable d'environnement Windows telle que la voit un jeu : celle du
/// système sous Windows, celle de son préfixe ailleurs.
pub fn windows_env(name: &str, prefix: Option<&Path>) -> Option<String> {
    if cfg!(windows) {
        return std::env::var(name).ok();
    }
    let pfx = prefix?;
    let users = pfx.join("drive_c").join("users");
    let user = users.join("steamuser");
    let path = match name.to_ascii_uppercase().as_str() {
        "USERPROFILE" => user,
        "LOCALAPPDATA" => user.join("AppData").join("Local"),
        "APPDATA" => user.join("AppData").join("Roaming"),
        "PUBLIC" => users.join("Public"),
        _ => return None,
    };
    Some(path.to_string_lossy().into_owned())
}

/// Chemin Windows à variables (`%USERPROFILE%\…`) pour le jeu de ce préfixe.
pub fn expand(template: &str, prefix: Option<&Path>) -> Option<String> {
    let path = installer::expand_env(template, |name| windows_env(name, prefix))?;
    Some(if cfg!(windows) { path } else { path.replace('\\', "/") })
}

/// Options de lancement Steam pour qu'un chargeur de mods (DLL de
/// remplacement) soit chargé sous Proton. `None` sous Windows.
pub fn launch_option(tool: &str) -> Option<String> {
    if cfg!(windows) {
        return None;
    }
    let dll = match tool {
        "bepinex" | "bepinex5" | "bepinex6" => "winhttp",
        "melonloader" => "version",
        "ue4ss" => "dwmapi",
        _ => return None,
    };
    Some(format!("WINEDLLOVERRIDES=\"{dll}=n,b\" %command%"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::installer::tests::tempdir;

    #[test]
    fn finds_the_app_and_its_prefix() {
        let lib = tempdir("proton-lib");
        let steamapps = lib.join("steamapps");
        fs::create_dir_all(steamapps.join("common/Skyrim Special Edition")).unwrap();
        fs::write(
            steamapps.join("appmanifest_489830.acf"),
            "\"AppState\"\n{\n\t\"appid\"\t\t\"489830\"\n\t\"installdir\"\t\t\"Skyrim Special Edition\"\n}\n",
        )
        .unwrap();
        fs::write(steamapps.join("appmanifest_1.acf"), "\"AppState\" { \"appid\" \"1\" \"installdir\" \"Autre\" }").unwrap();
        let game = steamapps.join("common/Skyrim Special Edition");
        assert_eq!(app_id_for_dir(&game).as_deref(), Some("489830"));
        assert_eq!(app_id_for_dir(&lib), None, "pas un dossier de jeu Steam");

        assert!(prefix_in("489830", &[lib.clone()]).is_none(), "jeu jamais lancé : pas de préfixe");
        let pfx = steamapps.join("compatdata/489830/pfx");
        fs::create_dir_all(pfx.join("drive_c/users/steamuser")).unwrap();
        assert_eq!(prefix_in("489830", &[lib.clone()]), Some(pfx));
    }

    #[cfg(not(windows))]
    #[test]
    fn maps_windows_folders_into_the_prefix() {
        let pfx = Path::new("/jeux/compatdata/1592190/pfx");
        assert_eq!(windows_env("LOCALAPPDATA", Some(pfx)).as_deref(), Some("/jeux/compatdata/1592190/pfx/drive_c/users/steamuser/AppData/Local"));
        assert_eq!(windows_env("PUBLIC", Some(pfx)).as_deref(), Some("/jeux/compatdata/1592190/pfx/drive_c/users/Public"));
        assert_eq!(windows_env("WINDIR", Some(pfx)), None);
        assert_eq!(windows_env("USERPROFILE", None), None);
        assert_eq!(
            expand(r"%USERPROFILE%\AppData\LocalLow\Stress Level Zero\BONELAB\Mods", Some(pfx)).as_deref(),
            Some("/jeux/compatdata/1592190/pfx/drive_c/users/steamuser/AppData/LocalLow/Stress Level Zero/BONELAB/Mods")
        );
        assert_eq!(launch_option("bepinex6").as_deref(), Some("WINEDLLOVERRIDES=\"winhttp=n,b\" %command%"));
        assert_eq!(launch_option("melonloader").as_deref(), Some("WINEDLLOVERRIDES=\"version=n,b\" %command%"));
        assert_eq!(launch_option("skse64"), None);
    }
}
