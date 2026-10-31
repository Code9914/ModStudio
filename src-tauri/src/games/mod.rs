//! Jeux installés sur ce PC (Steam, Epic ; sous Linux, Steam et Heroic) et leur moteur.

pub mod catalog;
pub mod engine;
pub mod epic;
pub mod launch;
pub mod proton;
pub mod steam;

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Launcher {
    Steam,
    Epic,
}

/// Un jeu trouvé dans un launcher.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InstalledGame {
    pub launcher: Launcher,
    /// Identifiant dans le launcher (numéro Steam, nom d'appli Epic).
    pub app_id: String,
    pub name: String,
    pub path: PathBuf,
}

/// Jeu installé, avec sa page Nexus quand elle existe.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectedGame {
    #[serde(flatten)]
    pub game: InstalledGame,
    pub nexus: Option<catalog::CatalogGame>,
}

/// Tous les jeux Steam et Epic installés sur ce PC.
pub fn installed_games() -> Vec<InstalledGame> {
    let mut games = steam::installed_games(&steam::steam_roots());
    if let Some(dir) = epic::manifests_dir() {
        games.extend(epic::installed_games(&dir));
    }
    // Sous Linux, les jeux Epic passent par Heroic Games Launcher.
    for file in epic::heroic_files() {
        games.extend(epic::heroic_games(&file));
    }
    games.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    games
}

/// Associe chaque jeu installé à sa page Nexus.
pub fn with_nexus(games: Vec<InstalledGame>, catalog: Option<&catalog::Catalog>) -> Vec<DetectedGame> {
    let matcher = catalog.map(catalog::Matcher::new);
    games
        .into_iter()
        .map(|game| DetectedGame {
            nexus: matcher.as_ref().and_then(|m| m.find(&game)).cloned(),
            game,
        })
        .collect()
}
