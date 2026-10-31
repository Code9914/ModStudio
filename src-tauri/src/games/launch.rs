//! Lancer un jeu installé depuis ModStudio : par son launcher (Steam, Epic ;
//! Heroic sous Linux), ou, sous Windows, par le lanceur de son script extender
//! (SKSE…) pour que les mods qui en dépendent se chargent.

use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;

use super::{engine, installed_games, InstalledGame, Launcher};
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Launched {
    /// `steam`, `epic`, `heroic` ou `extender`.
    pub via: &'static str,
    /// Script extender utilisé (`SKSE64`…).
    pub extender: Option<&'static str>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Target {
    Url(String),
    Exe(PathBuf),
}

/// Lien qui demande au launcher de démarrer le jeu.
pub fn launcher_url(game: &InstalledGame, windows: bool) -> AppResult<(String, &'static str)> {
    let id = game.app_id.as_str();
    let bad = || AppError::LaunchFailed(crate::tr!("identifiant « {id} » inattendu", "unexpected ID \"{id}\""));
    match game.launcher {
        Launcher::Steam => {
            if id.is_empty() || id.len() > 12 || !id.bytes().all(|b| b.is_ascii_digit()) {
                return Err(bad());
            }
            Ok((format!("steam://rungameid/{id}"), "steam"))
        }
        Launcher::Epic => {
            let ok = !id.is_empty() && id.len() <= 128 && id.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'));
            if !ok {
                return Err(bad());
            }
            // Sous Linux, les jeux Epic sont ceux de Heroic Games Launcher.
            if windows {
                Ok((format!("com.epicgames.launcher://apps/{id}?action=launch&silent=true"), "epic"))
            } else {
                Ok((format!("heroic://launch?appName={id}&runner=legendary"), "heroic"))
            }
        }
    }
}

/// Lanceur du script extender d'un jeu Bethesda, s'il est installé.
pub fn extender_exe(game_dir: &Path) -> Option<(PathBuf, &'static str)> {
    engine::BETHESDA.iter().find_map(|g| {
        let exe = game_dir.join(format!("{}_loader.exe", g.extender));
        (game_dir.join(g.exe).is_file() && exe.is_file()).then_some((exe, g.extender_name))
    })
}

/// Comment lancer ce jeu ici.
pub fn plan(game: &InstalledGame, windows: bool) -> AppResult<(Target, Launched)> {
    if windows && let Some((exe, name)) = extender_exe(&game.path) {
        return Ok((Target::Exe(exe), Launched { via: "extender", extender: Some(name) }));
    }
    let (url, via) = launcher_url(game, windows)?;
    Ok((Target::Url(url), Launched { via, extender: None }))
}

fn same_path(a: &Path, b: &str) -> bool {
    let norm = |s: &str| s.replace('\\', "/").trim_end_matches('/').to_lowercase();
    norm(&a.to_string_lossy()) == norm(b)
}

/// Lance un jeu trouvé sur ce PC (et seulement un jeu trouvé : rien d'autre ne s'ouvre).
#[tauri::command]
pub async fn launch_game(app: AppHandle, launcher: Launcher, app_id: String, path: String) -> AppResult<Launched> {
    let games = tokio::task::spawn_blocking(installed_games).await.unwrap_or_default();
    let game = games
        .into_iter()
        .find(|g| g.launcher == launcher && g.app_id == app_id && same_path(&g.path, &path))
        .ok_or_else(|| AppError::LaunchFailed(crate::tr!("le jeu n'est plus trouvé sur ce PC", "the game can no longer be found on this PC")))?;
    let (target, launched) = plan(&game, cfg!(windows))?;
    match target {
        Target::Url(url) => app.opener().open_url(url, None::<&str>).map_err(|e| AppError::LaunchFailed(e.to_string()))?,
        Target::Exe(exe) => {
            std::process::Command::new(&exe)
                .current_dir(&game.path)
                .spawn()
                .map_err(|e| AppError::LaunchFailed(e.to_string()))?;
        }
    }
    Ok(launched)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::installer::tests::tempdir;

    fn game(launcher: Launcher, id: &str, path: PathBuf) -> InstalledGame {
        InstalledGame { launcher, app_id: id.into(), name: "Jeu".into(), path }
    }

    #[test]
    fn builds_launcher_links_from_checked_ids() {
        let dir = tempdir("launch-url");
        let steam = game(Launcher::Steam, "1966720", dir.clone());
        assert_eq!(launcher_url(&steam, true).unwrap().0, "steam://rungameid/1966720");
        let epic = game(Launcher::Epic, "Fortnite", dir.clone());
        assert_eq!(launcher_url(&epic, true).unwrap().0, "com.epicgames.launcher://apps/Fortnite?action=launch&silent=true");
        assert_eq!(launcher_url(&epic, false).unwrap(), ("heroic://launch?appName=Fortnite&runner=legendary".into(), "heroic"));
        for bad in ["", "12a", "1;calc", "123456789012345"] {
            assert!(launcher_url(&game(Launcher::Steam, bad, dir.clone()), true).is_err(), "{bad}");
        }
        assert!(launcher_url(&game(Launcher::Epic, "a&b=c", dir.clone()), false).is_err());
        assert!(launcher_url(&game(Launcher::Epic, "x/../y", dir), true).is_err());
    }

    #[test]
    fn uses_the_script_extender_on_windows_when_present() {
        let dir = tempdir("launch-skse");
        let skyrim = game(Launcher::Steam, "489830", dir.clone());
        // Sans SKSE : Steam.
        std::fs::write(dir.join("SkyrimSE.exe"), b"").unwrap();
        assert_eq!(plan(&skyrim, true).unwrap().1.via, "steam");
        std::fs::write(dir.join("skse64_loader.exe"), b"").unwrap();
        let (target, launched) = plan(&skyrim, true).unwrap();
        assert_eq!(target, Target::Exe(dir.join("skse64_loader.exe")));
        assert_eq!(launched, Launched { via: "extender", extender: Some("SKSE64") });
        // Sous Linux (Proton), toujours par Steam.
        assert_eq!(plan(&skyrim, false).unwrap().1.via, "steam");
    }

    #[test]
    fn compares_paths_loosely() {
        assert!(same_path(Path::new("C:\\Games\\Lethal Company"), "c:/games/lethal company/"));
        assert!(!same_path(Path::new("/games/a"), "/games/ab"));
    }
}
