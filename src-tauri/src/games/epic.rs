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

/// Listes des jeux Epic installés par Heroic Games Launcher (Linux), version
/// classique et Flatpak.
pub fn heroic_files() -> Vec<PathBuf> {
    if cfg!(windows) {
        return Vec::new();
    }
    let Ok(home) = std::env::var("HOME") else { return Vec::new() };
    let home = PathBuf::from(home);
    [
        home.join(".config/heroic/legendaryConfig/legendary/installed.json"),
        home.join(".var/app/com.heroicgameslauncher.hgl/config/heroic/legendaryConfig/legendary/installed.json"),
    ]
    .into_iter()
    .filter(|p| p.is_file())
    .collect()
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct HeroicGame {
    app_name: String,
    title: String,
    install_path: String,
    is_dlc: bool,
}

/// Jeux Epic installés avec Heroic (`installed.json` de Legendary).
pub fn heroic_games(file: &Path) -> Vec<InstalledGame> {
    let Ok(raw) = fs::read(file) else { return Vec::new() };
    let Ok(map) = serde_json::from_slice::<std::collections::HashMap<String, HeroicGame>>(&raw) else { return Vec::new() };
    let mut out: Vec<InstalledGame> = map
        .into_values()
        .filter(|g| !g.is_dlc && !g.title.trim().is_empty() && Path::new(&g.install_path).is_dir())
        .map(|g| InstalledGame {
            launcher: Launcher::Epic,
            app_id: g.app_name,
            name: g.title,
            path: PathBuf::from(g.install_path),
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
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
    fn reads_heroic_installs() {
        let dir = tempdir("heroic");
        let game = dir.join("Games/Hogwarts");
        fs::create_dir_all(&game).unwrap();
        let file = dir.join("installed.json");
        let json = format!(
            r#"{{"fa4240e57a3c46b39f169041b7811293":{{"app_name":"fa4240e57a3c46b39f169041b7811293","title":"Hogwarts Legacy","install_path":"{}","is_dlc":false}},
               "dlc1":{{"app_name":"dlc1","title":"Un DLC","install_path":"{}","is_dlc":true}},
               "gone":{{"app_name":"gone","title":"Désinstallé","install_path":"/nulle/part","is_dlc":false}}}}"#,
            game.display(),
            game.display()
        );
        fs::write(&file, json).unwrap();
        let games = heroic_games(&file);
        assert_eq!(games.len(), 1);
        assert_eq!((games[0].name.as_str(), games[0].launcher), ("Hogwarts Legacy", Launcher::Epic));
        assert!(heroic_games(&dir.join("absent.json")).is_empty());
    }

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
