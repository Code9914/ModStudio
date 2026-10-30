//! Jeux installés avec l'Epic Games Launcher : un manifeste JSON (`.item`) par
//! jeu dans `%PROGRAMDATA%\Epic\EpicGamesLauncher\Data\Manifests`.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::{InstalledGame, Launcher};

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Manifest {
    display_name: String,
    install_location: String,
    app_name: String,
    main_game_app_name: String,
    #[serde(rename = "bIsIncompleteInstall")]
    incomplete: bool,
    app_categories: Vec<String>,
}

/// Dossier des manifestes Epic.
pub fn manifests_dir() -> Option<PathBuf> {
    let data = std::env::var("PROGRAMDATA").ok()?;
    Some(PathBuf::from(data).join("Epic").join("EpicGamesLauncher").join("Data").join("Manifests"))
}

/// Jeux Epic complètement installés (sans les DLC ni les extensions).
pub fn installed_games(dir: &Path) -> Vec<InstalledGame> {
    let Ok(entries) = fs::read_dir(dir) else { return Vec::new() };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.extension().is_some_and(|e| e.eq_ignore_ascii_case("item")) {
            continue;
        }
        let Ok(raw) = fs::read(&path) else { continue };
        let Ok(m) = serde_json::from_slice::<Manifest>(&raw) else { continue };
        let is_base_game = m.main_game_app_name.is_empty() || m.main_game_app_name == m.app_name;
        let is_game = m.app_categories.is_empty() || m.app_categories.iter().any(|c| c.eq_ignore_ascii_case("games"));
        let location = PathBuf::from(&m.install_location);
        if m.incomplete || !is_base_game || !is_game || m.display_name.trim().is_empty() || !location.is_dir() {
            continue;
        }
        out.push(InstalledGame {
            launcher: Launcher::Epic,
            app_id: m.app_name,
            name: m.display_name,
            path: location,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::installer::tests::tempdir;

    #[test]
    fn reads_epic_manifests() {
        let dir = tempdir("epic");
        let game = tempdir("epic-game");
        let item = |name: &str, app: &str, main: &str, incomplete: bool, cats: &str| {
            format!(
                r#"{{"DisplayName":"{name}","InstallLocation":{loc},"AppName":"{app}","MainGameAppName":"{main}","bIsIncompleteInstall":{incomplete},"AppCategories":[{cats}]}}"#,
                loc = serde_json::to_string(&game.to_string_lossy()).unwrap()
            )
        };
        fs::write(dir.join("a.item"), item("Satisfactory", "CrabEA", "CrabEA", false, r#""public","games""#)).unwrap();
        fs::write(dir.join("b.item"), item("Satisfactory DLC", "CrabDLC", "CrabEA", false, r#""addons""#)).unwrap();
        fs::write(dir.join("c.item"), item("Half Done", "Half", "Half", true, r#""games""#)).unwrap();
        fs::write(dir.join("d.txt"), "pas un manifeste").unwrap();
        let games = installed_games(&dir);
        assert_eq!(games.len(), 1);
        assert_eq!((games[0].name.as_str(), games[0].app_id.as_str()), ("Satisfactory", "CrabEA"));
        assert!(installed_games(&dir.join("absent")).is_empty());
    }
}
