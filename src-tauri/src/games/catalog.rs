//! Liste des jeux de Nexus Mods (publiée par Nexus sur
//! `data.nexusmods.com`), gardée en cache, et correspondance avec les jeux
//! installés d'après leur nom.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::{InstalledGame, Launcher};

pub const CATALOG_URL: &str = "https://data.nexusmods.com/file/nexus-data/games.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CatalogGame {
    pub id: i64,
    pub name: String,
    pub domain_name: String,
    #[serde(default)]
    pub mods: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Catalog {
    /// Date du téléchargement (secondes Unix).
    #[serde(default)]
    pub fetched_at: i64,
    #[serde(default)]
    pub games: Vec<CatalogGame>,
}

impl Catalog {
    pub fn load(path: &Path) -> Option<Catalog> {
        let raw = fs::read(path).ok()?;
        serde_json::from_slice::<Catalog>(&raw).ok().filter(|c| !c.games.is_empty())
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_vec(self).unwrap_or_default())?;
        fs::rename(tmp, path)
    }

    /// Lecture du fichier publié par Nexus (tableau d'objets).
    pub fn from_nexus_json(raw: &[u8], now: i64) -> Option<Catalog> {
        #[derive(Deserialize)]
        struct Raw {
            id: i64,
            #[serde(default)]
            name: String,
            #[serde(default)]
            domain_name: String,
            #[serde(default)]
            mods: i64,
        }
        let list: Vec<serde_json::Value> = serde_json::from_slice(raw).ok()?;
        let games: Vec<CatalogGame> = list
            .into_iter()
            .filter_map(|v| serde_json::from_value::<Raw>(v).ok())
            .filter(|g| g.id > 0 && !g.domain_name.is_empty() && !g.name.is_empty())
            .map(|g| CatalogGame { id: g.id, name: g.name, domain_name: g.domain_name, mods: g.mods })
            .collect();
        (!games.is_empty()).then_some(Catalog { fetched_at: now, games })
    }

    pub fn is_stale(&self, now: i64) -> bool {
        now - self.fetched_at > 7 * 24 * 3600
    }
}

/// Nom simplifié pour comparer : minuscules, sans symboles ni ponctuation.
pub fn normalize(name: &str) -> String {
    let lowered = name.to_lowercase().replace('&', " and ");
    let mut out = String::new();
    let mut space = true;
    for c in lowered.chars() {
        if matches!(c, '™' | '®' | '©' | '\'' | '’') {
            continue;
        }
        if c.is_alphanumeric() {
            out.push(c);
            space = false;
        } else if !space {
            out.push(' ');
            space = true;
        }
    }
    let out = out.trim().to_string();
    out.strip_prefix("the ").map(str::to_string).unwrap_or(out)
}

/// Mentions d'édition retirées en dernier recours (« Game of the Year Edition »…).
const EDITIONS: &[&str] = &[
    "game of the year edition",
    "goty edition",
    "goty",
    "definitive edition",
    "enhanced edition",
    "complete edition",
    "deluxe edition",
    "gold edition",
    "anniversary edition",
    "directors cut",
    "remastered",
];

fn without_edition(normalized: &str) -> String {
    let mut s = normalized.to_string();
    for e in EDITIONS {
        if let Some(stripped) = s.strip_suffix(e) {
            s = stripped.trim().to_string();
        }
    }
    s
}

/// Jeux dont le nom sur Steam diffère trop de leur nom sur Nexus.
fn steam_alias(app_id: &str) -> Option<&'static str> {
    Some(match app_id {
        "489830" => "skyrimspecialedition",
        "72850" => "skyrim",
        "377160" => "fallout4",
        "22380" => "newvegas",
        "22300" | "22370" => "fallout3",
        "22330" => "oblivion",
        "22320" => "morrowind",
        "1716740" => "starfield",
        "2623190" => "oblivionremastered",
        "1091500" => "cyberpunk2077",
        "292030" => "witcher3",
        "1086940" => "baldursgate3",
        "261550" => "mountandblade2bannerlord",
        _ => return None,
    })
}

/// Index du catalogue pour retrouver vite un jeu par nom ou identifiant.
pub struct Matcher<'a> {
    by_name: HashMap<String, Vec<&'a CatalogGame>>,
    by_domain: HashMap<&'a str, &'a CatalogGame>,
}

impl<'a> Matcher<'a> {
    pub fn new(catalog: &'a Catalog) -> Self {
        let mut by_name: HashMap<String, Vec<&CatalogGame>> = HashMap::new();
        let mut by_domain = HashMap::new();
        for g in &catalog.games {
            by_name.entry(normalize(&g.name)).or_default().push(g);
            by_domain.insert(g.domain_name.as_str(), g);
        }
        Self { by_name, by_domain }
    }

    /// Le jeu Nexus qui correspond à un jeu installé, s'il y en a un sans ambiguïté.
    pub fn find(&self, game: &InstalledGame) -> Option<&'a CatalogGame> {
        if game.launcher == Launcher::Steam {
            if let Some(g) = steam_alias(&game.app_id).and_then(|d| self.by_domain.get(d)) {
                return Some(g);
            }
        }
        let name = normalize(&game.name);
        if let Some(found) = self.unique(&name) {
            return Some(found);
        }
        let short = without_edition(&name);
        if short != name {
            return self.unique(&short);
        }
        None
    }

    fn unique(&self, name: &str) -> Option<&'a CatalogGame> {
        match self.by_name.get(name).map(Vec::as_slice) {
            Some([one]) => Some(*one),
            // Homonymes : le jeu qui a le plus de mods.
            Some(many) if !many.is_empty() => many.iter().max_by_key(|g| g.mods).copied(),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn g(id: i64, name: &str, domain: &str) -> CatalogGame {
        CatalogGame { id, name: name.into(), domain_name: domain.into(), mods: 1 }
    }

    fn installed(launcher: Launcher, app_id: &str, name: &str) -> InstalledGame {
        InstalledGame { launcher, app_id: app_id.into(), name: name.into(), path: PathBuf::from("/x") }
    }

    #[test]
    fn normalizes_names() {
        assert_eq!(normalize("PILGRIM"), "pilgrim");
        assert_eq!(normalize("Baldur's Gate 3"), "baldurs gate 3");
        assert_eq!(normalize("Mount & Blade II: Bannerlord"), "mount and blade ii bannerlord");
        assert_eq!(normalize("The Witcher® 3: Wild Hunt"), "witcher 3 wild hunt");
        assert_eq!(normalize("  BONELAB™ "), "bonelab");
    }

    #[test]
    fn matches_installed_games_to_nexus() {
        let catalog = Catalog {
            fetched_at: 0,
            games: vec![
                g(6534, "PILGRIM", "pilgrim"),
                g(4832, "BONELAB", "bonelab"),
                g(1704, "Skyrim Special Edition", "skyrimspecialedition"),
                g(110, "Skyrim", "skyrim"),
                g(952, "Witcher 3", "witcher3"),
                g(2000, "Saints Row The Third", "saintsrowthethird"),
            ],
        };
        let m = Matcher::new(&catalog);
        let found = |l, id: &str, name: &str| m.find(&installed(l, id, name)).map(|g| g.domain_name.as_str());
        assert_eq!(found(Launcher::Steam, "1", "PILGRIM"), Some("pilgrim"));
        assert_eq!(found(Launcher::Steam, "1592190", "BONELAB"), Some("bonelab"));
        assert_eq!(found(Launcher::Steam, "489830", "The Elder Scrolls V: Skyrim Special Edition"), Some("skyrimspecialedition"));
        assert_eq!(found(Launcher::Steam, "72850", "The Elder Scrolls V: Skyrim"), Some("skyrim"));
        assert_eq!(found(Launcher::Epic, "x", "Saints Row: The Third Remastered"), Some("saintsrowthethird"));
        assert_eq!(found(Launcher::Steam, "228980", "Steamworks Common Redistributables"), None);
    }

    #[test]
    fn reads_the_nexus_list_and_caches_it() {
        let raw = br#"[{"id":6534,"name":"PILGRIM","name_lower":"pilgrim","domain_name":"pilgrim","mods":9,"genre":"Indie"},{"id":0,"name":"","domain_name":""}]"#;
        let c = Catalog::from_nexus_json(raw, 100).unwrap();
        assert_eq!(c.games.len(), 1);
        assert!(!c.is_stale(100) && c.is_stale(100 + 8 * 24 * 3600));
        let dir = crate::installer::tests::tempdir("catalog");
        c.save(&dir.join("nexus-games.json")).unwrap();
        assert_eq!(Catalog::load(&dir.join("nexus-games.json")).unwrap().games, c.games);
        assert!(Catalog::from_nexus_json(b"[]", 0).is_none());
    }
}
