//! Jeux installés sur ce PC (Steam, Epic), reconnus parmi les jeux de Nexus
//! Mods, et l'outil de mods dont chacun a besoin.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::{json, Value};

use super::install::GameSetup;
use super::rules::{self, GAME_DIR};
use super::tools::{self, Hints};
use super::NexusState;
use crate::error::AppResult;
use crate::games::catalog::Catalog;
use crate::games::engine::{self, Engine, EngineInfo};
use crate::games::{self, DetectedGame};
use crate::installer::GameInstall;

/// Outil de mods conseillé pour un jeu.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ToolOffer {
    /// `ue4ss`, `bepinex5`, `bepinex6`, `melonloader`, ou le préfixe d'un
    /// script extender (`skse64`).
    pub id: String,
    pub name: String,
    pub description: String,
    /// Déjà présent dans le dossier du jeu.
    pub present: bool,
    /// Installé par ModStudio (et donc retirable depuis ModStudio).
    pub managed: bool,
    /// Script extender : sa page sur Nexus Mods (il s'installe comme un mod).
    pub nexus_mod: Option<i64>,
    /// Version du jeu (le script extender doit lui correspondre).
    pub game_version: Option<String>,
    /// Nombre de mods parmi les plus populaires du jeu qui en ont besoin.
    pub popular: usize,
    pub sampled: usize,
    /// Version installée par ModStudio.
    pub installed_version: Option<String>,
    /// Version différente disponible (ou demandée par les mods du jeu).
    pub update: Option<String>,
    /// Version précise demandée par les mods du jeu (`6.0.0-be.791`).
    pub required_version: Option<String>,
    /// Sous Linux (Proton) : options de lancement pour que le chargeur soit chargé.
    pub launch_option: Option<String>,
}

impl ToolOffer {
    /// Outil installable depuis GitHub (`tools::TOOLS`).
    pub fn github(id: &str, config: Option<&GameInstall>, info: Option<&EngineInfo>, hints: &Hints) -> Option<ToolOffer> {
        let def = tools::tool(id)?;
        let record = config.and_then(|c| c.tools.get(def.id));
        Some(ToolOffer {
            id: def.id.to_string(),
            name: def.name.to_string(),
            description: def.description.to_string(),
            present: info.is_some_and(|i| i.tools.iter().any(|t| t == def.present)),
            managed: record.is_some(),
            nexus_mod: None,
            game_version: None,
            popular: hints.count(def.present),
            sampled: hints.mods,
            installed_version: record.and_then(|r| r.version.clone()),
            update: None,
            required_version: (def.id == "bepinex6")
                .then(|| hints.wanted_be_build().map(|b| format!("6.0.0-be.{b}")))
                .flatten(),
            launch_option: crate::games::proton::launch_option(def.id),
        })
    }

    /// Script extender d'un jeu Bethesda.
    pub fn extender(domain: &str, info: Option<&EngineInfo>) -> Option<ToolOffer> {
        let g = engine::bethesda(domain)?;
        Some(ToolOffer {
            id: g.extender.to_string(),
            name: g.extender_name.to_string(),
            description: "script extender, nécessaire aux mods qui ajoutent du code au jeu".to_string(),
            present: info.is_some_and(|i| i.tools.iter().any(|t| t == g.extender)),
            managed: false,
            nexus_mod: g.extender_mod,
            game_version: info.and_then(|i| i.exe_version.clone()),
            popular: 0,
            sampled: 0,
            installed_version: None,
            update: None,
            required_version: None,
            launch_option: None,
        })
    }
}

/// Chargeur de mods trouvé dans le dossier d'un jeu, installé sans ModStudio.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FoundLoader {
    /// `ue4ss`, `bepinex`, `melonloader`, ou le préfixe d'un script extender (`skse64`).
    pub id: String,
    pub name: String,
}

/// Chargeurs de mods présents dans le dossier d'un jeu configuré, hors ceux
/// déjà listés : outils installés par ModStudio, script extender installé
/// comme un mod depuis sa page Nexus. Lecture rapide (pas d'exécutable lu).
pub fn found_loaders(config: &GameInstall) -> Vec<FoundLoader> {
    let Some(domain) = config.domain.as_deref() else {
        return Vec::new();
    };
    let Some((_, root)) = tool_root(config).filter(|(_, r)| r.is_dir()) else {
        return Vec::new();
    };
    let managed = |present: &str| config.tools.keys().any(|id| tools::tool(id).is_some_and(|d| d.present == present));
    let project = engine::unreal_project(&root);
    engine::present_tools(&root, project.as_deref(), domain)
        .into_iter()
        .filter(|t| !managed(t))
        .filter_map(|t| {
            if let Some(g) = engine::bethesda(domain).filter(|g| g.extender == t) {
                let as_mod = g.extender_mod.is_some_and(|m| config.mods.contains_key(&m));
                return (!as_mod).then(|| FoundLoader { id: t, name: g.extender_name.to_string() });
            }
            let name = match t.as_str() {
                "ue4ss" => "UE4SS",
                "bepinex" => "BepInEx",
                "melonloader" => "MelonLoader",
                _ => return None,
            };
            Some(FoundLoader { id: t, name: name.to_string() })
        })
        .collect()
}

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Dossier où vont les outils de mods, avec sa clé : le dossier configuré,
/// ou pour BONELAB son dossier d'installation (les mods vont ailleurs).
pub fn tool_root(config: &GameInstall) -> Option<(String, PathBuf)> {
    if config.rules.as_deref() == Some("bonelab") {
        return config
            .extra_folders
            .get(GAME_DIR)
            .cloned()
            .or_else(rules::detect_bonelab_dir)
            .map(|d| (GAME_DIR.to_string(), d));
    }
    Some((String::new(), config.folder.clone()))
}

/// Moteur du jeu installé dans `root`, s'il est reconnu.
pub fn detect_engine(root: &Path, domain: &str) -> Option<EngineInfo> {
    if !root.is_dir() {
        return None;
    }
    let info = engine::detect(root, domain);
    match info.engine {
        Engine::Unknown => None,
        // Le moteur Bethesda vient du jeu choisi : le dossier doit contenir son exécutable.
        Engine::Bethesda => engine::bethesda(domain).filter(|g| root.join(g.exe).is_file()).map(|_| info),
        _ => Some(info),
    }
}

/// Règles d'installation d'après le moteur du jeu installé dans `folder`.
/// Les jeux Unity ne sont rangés pour BepInEx / MelonLoader que si l'un des
/// deux y est installé : beaucoup ont leur propre système de mods (RimWorld…).
pub fn engine_rules(folder: &Path, domain: &str) -> Option<&'static str> {
    let info = detect_engine(folder, domain)?;
    match info.engine {
        Engine::Unreal => Some("unreal"),
        Engine::UnityMono | Engine::UnityIl2cpp => {
            let loader = info.tools.iter().any(|t| t == "bepinex" || t == "melonloader");
            loader.then_some("unity")
        }
        Engine::Bethesda => Some("bethesda"),
        Engine::Unknown => None,
    }
}

/// Outil conseillé : celui déjà présent s'il y en a un, sinon d'après le moteur.
pub fn chosen_tool(game_id: i64, info: &EngineInfo, hints: &Hints) -> Option<&'static str> {
    let il2cpp = info.engine == Engine::UnityIl2cpp;
    let has = |t: &str| info.tools.iter().any(|x| x == t);
    match info.engine {
        Engine::UnityMono | Engine::UnityIl2cpp => {
            if game_id == rules::BONELAB || (has("melonloader") && !has("bepinex")) {
                Some("melonloader")
            } else if has("bepinex") {
                Some(if il2cpp { "bepinex6" } else { "bepinex5" })
            } else {
                tools::recommend(info.engine, hints)
            }
        }
        other => tools::recommend(other, hints),
    }
}

impl NexusState {
    /// Chargeurs de mods installés sans ModStudio, par jeu configuré.
    pub fn found_loaders(&self) -> BTreeMap<i64, Vec<FoundLoader>> {
        let games: Vec<GameInstall> = self.library.lock().expect("library").games.values().cloned().collect();
        games
            .iter()
            .map(|g| (g.game_id, found_loaders(g)))
            .filter(|(_, found)| !found.is_empty())
            .collect()
    }

    /// Règles d'installation d'un jeu configuré sur `folder` : règles propres
    /// au jeu, sinon d'après son moteur. Un jeu Unity sans chargeur est rangé
    /// pour BepInEx / MelonLoader si ses mods populaires en ont besoin.
    pub fn folder_rules(&self, game_id: i64, folder: &Path, domain: &str) -> Option<&'static str> {
        if let Some(r) = rules::rules_for(game_id) {
            return Some(r);
        }
        engine_rules(folder, domain).or_else(|| {
            let info = detect_engine(folder, domain)?;
            let unity = matches!(info.engine, Engine::UnityMono | Engine::UnityIl2cpp);
            let used = self
                .hints
                .lock()
                .expect("hints")
                .get(domain)
                .is_some_and(|h| h.bepinex + h.melonloader > 0);
            (unity && used).then_some("unity")
        })
    }

    /// Liste des jeux de Nexus Mods : cache sur le disque, téléchargé de
    /// nouveau chaque semaine. L'ancienne liste sert si Nexus ne répond pas.
    pub async fn catalog(&self) -> Option<Catalog> {
        let now = now();
        let cached = self
            .catalog
            .lock()
            .expect("catalog")
            .clone()
            .or_else(|| Catalog::load(&self.catalog_path));
        if let Some(c) = cached.as_ref().filter(|c| !c.is_stale(now)) {
            *self.catalog.lock().expect("catalog") = Some(c.clone());
            return Some(c.clone());
        }
        let fresh = match self.client.fetch_bytes(&self.catalog_url).await {
            Ok(raw) => Catalog::from_nexus_json(&raw, now),
            Err(_) => None,
        };
        let chosen = fresh.inspect(|c| {
            let _ = c.save(&self.catalog_path);
        });
        let result = chosen.or(cached);
        *self.catalog.lock().expect("catalog") = result.clone();
        result
    }

    /// Jeux Steam et Epic installés, avec leur page Nexus. Le résultat est
    /// gardé jusqu'au prochain `refresh`.
    pub async fn detect_games(&self, refresh: bool) -> Vec<DetectedGame> {
        let _one_at_a_time = self.detect_lock.lock().await;
        if !refresh {
            if let Some(found) = self.detected.lock().expect("detected").clone() {
                return found;
            }
        }
        let catalog = self.catalog().await;
        let scanner = self.scanner.clone();
        let installed = tokio::task::spawn_blocking(move || scanner()).await.unwrap_or_default();
        let found = games::with_nexus(installed, catalog.as_ref());
        *self.detected.lock().expect("detected") = Some(found.clone());
        found
    }

    /// Outils demandés par les mods les plus populaires d'un jeu (gardé en mémoire).
    pub async fn hints(&self, domain: &str) -> Hints {
        if let Some(h) = self.hints.lock().expect("hints").get(domain) {
            return h.clone();
        }
        let gql = "query Requirements($filter: ModsFilter, $count: Int, $sort: [ModsSort!]) \
                   { mods(count: $count, filter: $filter, sort: $sort) \
                   { nodes { modRequirements { nexusRequirements { nodes { modName url } } } } } }";
        let variables = json!({
            "count": 30,
            "filter": { "gameDomainName": [{ "value": domain, "op": "EQUALS" }] },
            "sort": [{ "endorsements": { "direction": "DESC" } }],
        });
        let Ok(data) = self.client.graphql::<Value>(gql, variables).await else {
            return Hints::default();
        };
        let mut hints = Hints::default();
        for node in data["mods"]["nodes"].as_array().into_iter().flatten() {
            // Nom et lien de chaque mod nécessaire (le lien donne parfois la version voulue).
            let texts: Vec<String> = node["modRequirements"]["nexusRequirements"]["nodes"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|r| format!("{} {}", r["modName"].as_str().unwrap_or(""), r["url"].as_str().unwrap_or("")))
                .collect();
            hints.add_mod(texts.iter().map(String::as_str));
        }
        self.hints.lock().expect("hints").insert(domain.to_string(), hints.clone());
        hints
    }

    /// Réglages d'un jeu, avec le jeu installé trouvé dans Steam ou Epic, son
    /// moteur et l'outil de mods conseillé.
    pub async fn game_setup(&self, game_id: i64, domain: &str) -> AppResult<GameSetup> {
        super::browse::check_domain(domain)?;
        let mut setup = self.setup(game_id);
        setup.installed = self
            .detect_games(false)
            .await
            .into_iter()
            .find(|d| d.nexus.as_ref().is_some_and(|n| n.id == game_id))
            .map(|d| d.game);

        let root = match &setup.config {
            Some(c) => tool_root(c).map(|(_, p)| p),
            None if game_id == rules::BONELAB => setup.game_dir.as_ref().map(PathBuf::from),
            None => setup.installed.as_ref().map(|g| g.path.clone()),
        };
        let domain = setup.config.as_ref().and_then(|c| c.domain.clone()).unwrap_or_else(|| domain.to_string());
        let info = match root {
            Some(r) => tokio::task::spawn_blocking({
                let domain = domain.clone();
                move || detect_engine(&r, &domain)
            })
            .await
            .unwrap_or(None),
            None => None,
        };
        if let Some(info) = &info {
            setup.tool = match info.engine {
                Engine::Bethesda => ToolOffer::extender(&domain, Some(info)),
                _ => {
                    let hints = self.hints(&domain).await;
                    // Jeux Unity : seulement si ses mods utilisent BepInEx ou MelonLoader.
                    let unity = matches!(info.engine, Engine::UnityMono | Engine::UnityIl2cpp);
                    let loader_used = game_id == rules::BONELAB
                        || hints.bepinex + hints.melonloader > 0
                        || info.tools.iter().any(|t| t == "bepinex" || t == "melonloader");
                    if unity && !loader_used {
                        None
                    } else {
                        let mut offer = chosen_tool(game_id, info, &hints)
                            .and_then(|id| ToolOffer::github(id, setup.config.as_ref(), Some(info), &hints));
                        // Installé par ModStudio : nouvelle version (ou version demandée par les mods) ?
                        if let Some(o) = offer.as_mut().filter(|o| o.managed) {
                            if let Some(def) = tools::tool(&o.id) {
                                let wanted = if def.id == "bepinex6" { hints.wanted_be_build() } else { None };
                                if let Ok(r) = self.tool_release(def, info.arch, wanted, false).await {
                                    if o.installed_version.as_deref() != Some(r.version.as_str()) {
                                        o.update = Some(r.version);
                                    }
                                }
                            }
                        }
                        offer
                    }
                }
            };
        }
        setup.engine = info;
        // Rangement qu'aurait le jeu configuré sur son dossier d'installation
        // (sinon celui du jeu configuré).
        setup.auto_rules = match (&setup.installed, &setup.config) {
            (Some(g), _) => self.folder_rules(game_id, &g.path, &domain).map(str::to_string),
            (None, Some(c)) => c.rules.clone(),
            (None, None) => None,
        };
        Ok(setup)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::MemoryStore;
    use crate::games::{InstalledGame, Launcher};
    use crate::installer::tests::tempdir;
    use crate::installer::Layout;
    use crate::testutil::{fake_server, json_response};
    use std::fs;
    use std::sync::Arc;

    #[tokio::test]
    async fn finds_installed_games_and_what_they_need() {
        let catalog = r#"[{"id":6534,"name":"PILGRIM","domain_name":"pilgrim","mods":40},{"id":1704,"name":"Skyrim Special Edition","domain_name":"skyrimspecialedition","mods":100}]"#;
        let requirements = r#"{"data":{"mods":{"nodes":[
            {"modRequirements":{"nexusRequirements":{"nodes":[{"modName":"UE4SS"}]}}},
            {"modRequirements":{"nexusRequirements":{"nodes":[{"modName":"RE-UE4SS"},{"modName":"UE4SS experimental"}]}}},
            {"modRequirements":{"nexusRequirements":{"nodes":[]}}}]}}}"#;
        let server = fake_server(vec![json_response(200, catalog), json_response(200, requirements)]);
        let dir = tempdir("local");
        let mut s = NexusState::new(server.base.trim_end_matches("/v1"), Box::new(MemoryStore::default()), dir.join("n.json"));
        s.catalog_url = format!("{}/games.json", server.base.trim_end_matches("/v1"));

        let game = dir.join("PILGRIM");
        fs::create_dir_all(game.join("Pilgrim/Binaries/Win64")).unwrap();
        fs::create_dir_all(game.join("Pilgrim/Content/Paks")).unwrap();
        let path = game.clone();
        s.scanner = Arc::new(move || {
            vec![
                InstalledGame { launcher: Launcher::Steam, app_id: "1".into(), name: "PILGRIM".into(), path: path.clone() },
                InstalledGame { launcher: Launcher::Epic, app_id: "x".into(), name: "Un jeu sans mods".into(), path: path.clone() },
            ]
        });

        let found = s.detect_games(false).await;
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].nexus.as_ref().map(|n| n.domain_name.as_str()), Some("pilgrim"));
        assert!(found[1].nexus.is_none());
        assert!(s.catalog_path.is_file(), "liste des jeux gardée sur le disque");

        // Jeu pas encore configuré : son dossier Steam sert à reconnaître le moteur.
        let setup = s.game_setup(6534, "pilgrim").await.unwrap();
        assert_eq!(setup.installed.as_ref().map(|g| g.path.clone()), Some(game.clone()));
        let info = setup.engine.unwrap();
        assert_eq!((info.engine, info.project.as_deref()), (Engine::Unreal, Some("Pilgrim")));
        let tool = setup.tool.unwrap();
        assert_eq!((tool.id.as_str(), tool.present, tool.popular, tool.sampled), ("ue4ss", false, 2, 3));
        assert_eq!(setup.auto_rules.as_deref(), Some("unreal"));

        // Résultats gardés en mémoire : aucune nouvelle requête.
        s.game_setup(6534, "pilgrim").await.unwrap();
        assert_eq!(server.count(), 2);

        // Configuré sur ce dossier : règles Unreal.
        let g = s
            .configure(6534, "pilgrim", "PILGRIM".into(), None, game.to_string_lossy().into_owned(), Layout::Flat, false, None)
            .unwrap();
        assert_eq!(g.rules.as_deref(), Some("unreal"));
    }

    #[test]
    fn bethesda_folders_need_the_game_exe() {
        let dir = tempdir("beth");
        assert!(engine_rules(&dir, "skyrimspecialedition").is_none());
        fs::write(dir.join("SkyrimSE.exe"), b"MZ").unwrap();
        assert_eq!(engine_rules(&dir, "skyrimspecialedition"), Some("bethesda"));
        let info = detect_engine(&dir, "skyrimspecialedition").unwrap();
        let offer = ToolOffer::extender("skyrimspecialedition", Some(&info)).unwrap();
        assert_eq!((offer.name.as_str(), offer.nexus_mod, offer.present), ("SKSE64", Some(30379), false));
    }

    #[test]
    fn unity_games_use_loader_rules_only_with_a_loader() {
        let dir = tempdir("unity-rules");
        fs::write(dir.join("UnityPlayer.dll"), b"").unwrap();
        fs::create_dir_all(dir.join("Game_Data/Managed")).unwrap();
        fs::write(dir.join("Game_Data/Managed/Assembly-CSharp.dll"), b"").unwrap();
        assert_eq!(engine_rules(&dir, "rimworld"), None, "mods propres au jeu : rangement au choix");
        fs::create_dir_all(dir.join("BepInEx/core")).unwrap();
        fs::write(dir.join("BepInEx/core/BepInEx.dll"), b"").unwrap();
        assert_eq!(engine_rules(&dir, "valheim"), Some("unity"));
    }

    #[test]
    fn loaders_installed_without_modstudio_are_found() {
        use crate::installer::{InstalledMod, InstalledTool};
        let dir = tempdir("found-loaders");
        let names = |g: &GameInstall| found_loaders(g).into_iter().map(|l| l.name).collect::<Vec<_>>();

        // Jeu Unity : MelonLoader installé à la main, BepInEx installé par ModStudio.
        let unity = dir.join("unity");
        fs::create_dir_all(unity.join("MelonLoader")).unwrap();
        fs::write(unity.join("version.dll"), b"").unwrap();
        fs::create_dir_all(unity.join("BepInEx/core")).unwrap();
        fs::write(unity.join("BepInEx/core/BepInEx.Core.dll"), b"").unwrap();
        let mut g = GameInstall { game_id: 1, folder: unity, domain: Some("moonsweepers".into()), ..Default::default() };
        assert_eq!(names(&g), ["BepInEx", "MelonLoader"]);
        g.tools.insert("bepinex6".into(), InstalledTool { id: "bepinex6".into(), ..Default::default() });
        assert_eq!(names(&g), ["MelonLoader"], "l'outil de ModStudio est déjà listé");
        g.domain = None;
        assert!(found_loaders(&g).is_empty(), "jeu sans page Nexus");

        // Script extender : listé, sauf s'il a été installé comme un mod depuis sa page.
        let skyrim = dir.join("skyrim");
        fs::create_dir_all(&skyrim).unwrap();
        fs::write(skyrim.join("skse64_loader.exe"), b"MZ").unwrap();
        let mut g = GameInstall { game_id: 1704, folder: skyrim, domain: Some("skyrimspecialedition".into()), ..Default::default() };
        assert_eq!(found_loaders(&g), [FoundLoader { id: "skse64".into(), name: "SKSE64".into() }]);
        g.mods.insert(30379, InstalledMod { mod_id: 30379, ..Default::default() });
        assert!(found_loaders(&g).is_empty());

        // Dossier absent : rien.
        let gone = GameInstall { folder: dir.join("absent"), domain: Some("pilgrim".into()), ..Default::default() };
        assert!(found_loaders(&gone).is_empty());
    }

    #[test]
    fn unity_games_keep_the_loader_they_have() {
        let info = |engine, tools: &[&str]| EngineInfo {
            engine,
            project: None,
            arch: engine::Arch::X64,
            exe_version: None,
            tools: tools.iter().map(|t| t.to_string()).collect(),
        };
        let none = Hints::default();
        assert_eq!(chosen_tool(1, &info(Engine::UnityMono, &[]), &none), Some("bepinex5"));
        assert_eq!(chosen_tool(1, &info(Engine::UnityMono, &["melonloader"]), &none), Some("melonloader"));
        assert_eq!(chosen_tool(1, &info(Engine::UnityIl2cpp, &["bepinex"]), &none), Some("bepinex6"));
        assert_eq!(chosen_tool(rules::BONELAB, &info(Engine::UnityIl2cpp, &[]), &none), Some("melonloader"));
        assert_eq!(chosen_tool(1, &info(Engine::Unreal, &[]), &none), Some("ue4ss"));
    }
}
