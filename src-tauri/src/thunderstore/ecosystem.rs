//! Ce que chaque jeu de Thunderstore demande pour charger ses mods : son
//! chargeur (BepInEx, MelonLoader…), ses règles d'installation (où va chaque
//! fichier d'un mod) et les paquets qui sont des chargeurs. Ce sont les
//! données publiées par Thunderstore pour les gestionnaires de mods (r2modman).

use std::collections::HashMap;

use serde::Deserialize;
use serde_json::Value;

/// Version embarquée (hors ligne) ; la version en ligne la remplace.
const BUNDLED: &str = include_str!("ecosystem.json");

/// Chargeur de mods d'un jeu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoaderKind {
    BepInEx,
    /// BepInEx pour .NET de Resonite (`hookfxr`).
    BepisLoader,
    MelonLoader,
    /// MelonLoader 0.7+ : chaque mod dans son dossier, `Mods/<Équipe-Nom>`.
    RecursiveMelonLoader,
    Northstar,
    /// UE4SS des jeux Unreal (Thunderstore le livre avec « shimloader »).
    Shimloader,
    /// Balatro : mods dans `%AppData%/Balatro/Mods`.
    Lovely,
    /// WEBFISHING.
    GdWeave,
    /// Hades II, Risk of Rain Returns.
    ReturnOfModding,
    /// Jeux Godot : mods zippés dans `mods`.
    GodotMl,
    /// Unity Mod Manager.
    Umm,
    /// Scrap Mechanic.
    Rivet,
    /// Mods copiés tels quels (chargeur fourni par le jeu ou par un mod).
    None,
    /// Chargeurs que ModStudio ne sait pas installer (MIO : chargeur absent de Thunderstore).
    Other,
}

impl LoaderKind {
    fn parse(s: &str) -> LoaderKind {
        match s {
            "bepinex" => LoaderKind::BepInEx,
            "bepisloader" => LoaderKind::BepisLoader,
            "melonloader" => LoaderKind::MelonLoader,
            "recursive-melonloader" => LoaderKind::RecursiveMelonLoader,
            "northstar" => LoaderKind::Northstar,
            "shimloader" => LoaderKind::Shimloader,
            "lovely" => LoaderKind::Lovely,
            "gdweave" => LoaderKind::GdWeave,
            "return-of-modding" => LoaderKind::ReturnOfModding,
            "godotml" => LoaderKind::GodotMl,
            "umm" => LoaderKind::Umm,
            "rivet" => LoaderKind::Rivet,
            "none" => LoaderKind::None,
            _ => LoaderKind::Other,
        }
    }

    pub fn supported(self) -> bool {
        self != LoaderKind::Other
    }
}

/// Nom affiché d'un chargeur (`packageLoader` de Thunderstore).
pub fn loader_label(id: &str) -> &'static str {
    match id {
        "bepinex" => "BepInEx",
        "melonloader" | "recursive-melonloader" => "MelonLoader",
        "northstar" => "Northstar",
        "shimloader" => "Shimloader (UE4SS)",
        "lovely" => "Lovely",
        "gdweave" => "GDWeave",
        "return-of-modding" => "Return of Modding",
        "godotml" => "Godot Mod Loader",
        "umm" => "Unity Mod Manager",
        "bepisloader" => "BepisLoader",
        "rivet" => "Rivet",
        "mio" => "Mio",
        "none" => "aucun chargeur",
        _ => "un chargeur inconnu",
    }
}

/// Suivi des fichiers d'une règle (comme r2modman).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tracking {
    /// Directement dans le dossier de la règle (réglages).
    None,
    /// Chemins gardés tels quels sous le dossier de la règle.
    State,
    /// Dans `<règle>/<Équipe-Nom>/`, à plat.
    Subdir,
    /// Dans `<règle>/<Équipe-Nom>/`, chemins gardés.
    SubdirNoFlatten,
    /// Tout le paquet zippé : `<règle>/<Équipe-Nom>.ts.zip`.
    PackageZip,
}

impl Tracking {
    fn parse(s: &str) -> Tracking {
        match s {
            "none" => Tracking::None,
            "state" => Tracking::State,
            "subdir-no-flatten" => Tracking::SubdirNoFlatten,
            "package-zip" => Tracking::PackageZip,
            _ => Tracking::Subdir,
        }
    }
}

/// Une règle d'installation, chemin complet (sous-règles aplaties).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    /// Relatif au dossier du jeu, séparé par `/` (`BepInEx/plugins`).
    pub route: String,
    pub tracking: Tracking,
    /// Extensions qui y vont d'office (`.dll`, `.mm.dll`).
    pub extensions: Vec<String>,
    /// Destination des fichiers qu'aucune autre règle ne prend.
    pub default: bool,
}

/// Ce qu'un jeu demande.
#[derive(Debug, Clone)]
pub struct GameRules {
    /// `packageLoader` de Thunderstore (`bepinex`, `shimloader`…).
    pub loader_id: String,
    pub kind: LoaderKind,
    pub rules: Vec<Rule>,
    /// Fichiers jamais installés (suivi `state`).
    pub exclusions: Vec<String>,
    /// Paquets mis en avant par la communauté (le bon chargeur, souvent).
    pub autolist: Vec<String>,
    /// Exécutables du jeu (`Hades2.exe`…) : le chargeur va à côté.
    pub exe_names: Vec<String>,
    /// Dossier de données (`Pal` pour Palworld, `Balatro_Data`…).
    pub data_folder: String,
}

/// Paquet qui est un chargeur de mods.
#[derive(Debug, Clone, Deserialize)]
pub struct LoaderPackage {
    #[serde(rename = "packageId")]
    pub package_id: String,
    pub loader: String,
    /// Dossier de l'archive dont le contenu va dans le dossier du jeu.
    #[serde(default, rename = "rootFolder")]
    pub root_folder: String,
}

#[derive(Deserialize)]
struct RawRule {
    route: String,
    #[serde(default, rename = "trackingMethod")]
    tracking: String,
    #[serde(default, rename = "defaultFileExtensions")]
    extensions: Vec<String>,
    #[serde(default, rename = "isDefaultLocation")]
    default: bool,
    #[serde(default, rename = "subRoutes")]
    sub: Vec<RawRule>,
}

fn flatten(rules: &[RawRule], prefix: &str, out: &mut Vec<Rule>) {
    for r in rules {
        let route = if prefix.is_empty() { r.route.clone() } else { format!("{prefix}/{}", r.route) };
        out.push(Rule {
            route: route.clone(),
            tracking: Tracking::parse(&r.tracking),
            extensions: r.extensions.iter().map(|e| e.to_ascii_lowercase()).collect(),
            default: r.default,
        });
        flatten(&r.sub, &route, out);
    }
}

/// Données de tous les jeux.
#[derive(Debug, Default)]
pub struct Ecosystem {
    games: HashMap<String, Value>,
    communities: HashMap<String, Value>,
    pub loaders: Vec<LoaderPackage>,
}

impl Ecosystem {
    /// Lit le schéma complet de Thunderstore (ou sa version réduite).
    pub fn parse(raw: &[u8]) -> Option<Ecosystem> {
        #[derive(Deserialize)]
        struct Raw {
            #[serde(default)]
            games: HashMap<String, Value>,
            #[serde(default)]
            communities: HashMap<String, Value>,
            #[serde(default, rename = "modloaderPackages")]
            loaders: Vec<LoaderPackage>,
        }
        let r: Raw = serde_json::from_slice(raw).ok()?;
        (!r.games.is_empty() && !r.loaders.is_empty()).then_some(Ecosystem {
            games: r.games,
            communities: r.communities,
            loaders: r.loaders,
        })
    }

    pub fn bundled() -> Ecosystem {
        Ecosystem::parse(BUNDLED.as_bytes()).expect("ecosystem.json embarqué")
    }

    /// Version réduite à ce qu'utilise ModStudio (cache sur le disque).
    pub fn to_compact_json(&self) -> String {
        let games: serde_json::Map<String, Value> = self
            .games
            .iter()
            .map(|(k, g)| {
                let entries: Vec<Value> = g
                    .get("r2modman")
                    .and_then(Value::as_array)
                    .map(|a| {
                        a.iter()
                            .map(|r| {
                                serde_json::json!({
                                    "gameInstanceType": r.get("gameInstanceType"),
                                    "packageLoader": r.get("packageLoader"),
                                    "installRules": r.get("installRules"),
                                    "relativeFileExclusions": r.get("relativeFileExclusions"),
                                    "exeNames": r.get("exeNames"),
                                    "dataFolderName": r.get("dataFolderName"),
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let mut out = serde_json::json!({ "r2modman": entries });
                if let Some(a) = g.pointer("/thunderstore/autolistPackageIds") {
                    out["thunderstore"] = serde_json::json!({ "autolistPackageIds": a });
                }
                (k.clone(), out)
            })
            .collect();
        let communities: serde_json::Map<String, Value> = self
            .communities
            .iter()
            .filter_map(|(k, c)| c.get("autolistPackageIds").map(|a| (k.clone(), serde_json::json!({ "autolistPackageIds": a }))))
            .collect();
        let loaders: Vec<Value> = self
            .loaders
            .iter()
            .map(|l| serde_json::json!({ "packageId": l.package_id, "loader": l.loader, "rootFolder": l.root_folder }))
            .collect();
        serde_json::json!({ "games": games, "communities": communities, "modloaderPackages": loaders }).to_string()
    }

    /// Règles d'un jeu (`None` : Thunderstore n'en publie pas).
    pub fn game(&self, community: &str) -> Option<GameRules> {
        let g = self.games.get(community)?;
        let entries = g.get("r2modman")?.as_array()?;
        // Le jeu plutôt que son serveur dédié.
        let entry = entries
            .iter()
            .find(|e| e.get("gameInstanceType").and_then(Value::as_str) != Some("server"))
            .or(entries.first())?;
        let loader_id = entry.get("packageLoader").and_then(Value::as_str).unwrap_or("none").to_string();
        let raw: Vec<RawRule> = entry
            .get("installRules")
            .cloned()
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();
        let mut rules = Vec::new();
        flatten(&raw, "", &mut rules);
        let exclusions = entry
            .get("relativeFileExclusions")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).map(str::to_ascii_lowercase).collect())
            .unwrap_or_default();
        let autolist_of = |v: Option<&Value>| -> Vec<String> {
            v.and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(String::from).collect()).unwrap_or_default()
        };
        let mut autolist = autolist_of(self.communities.get(community).and_then(|c| c.get("autolistPackageIds")));
        if autolist.is_empty() {
            autolist = autolist_of(g.pointer("/thunderstore/autolistPackageIds"));
        }
        let exe_names = entry
            .get("exeNames")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).map(String::from).collect())
            .unwrap_or_default();
        let data_folder = entry.get("dataFolderName").and_then(Value::as_str).unwrap_or_default().to_string();
        Some(GameRules { kind: LoaderKind::parse(&loader_id), loader_id, rules, exclusions, autolist, exe_names, data_folder })
    }

    /// Le paquet `Équipe-Nom` est-il un chargeur ?
    pub fn loader_package(&self, full_name: &str) -> Option<&LoaderPackage> {
        self.loaders.iter().find(|l| l.package_id.eq_ignore_ascii_case(full_name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_data_knows_the_games() {
        let e = Ecosystem::bundled();
        let lc = e.game("lethal-company").unwrap();
        assert_eq!(lc.kind, LoaderKind::BepInEx);
        assert_eq!(lc.rules[0].route, "BepInEx/plugins");
        assert!(lc.rules[0].default);
        assert_eq!(lc.rules.iter().find(|r| r.route == "BepInEx/config").unwrap().tracking, Tracking::None);

        let rr = e.game("roadside-research").unwrap();
        assert_eq!(rr.autolist, ["BepInEx-BepInExPack_IL2CPP"]);

        assert_eq!(e.game("schedule-i").unwrap().kind, LoaderKind::RecursiveMelonLoader);
        let bonelab = e.game("bonelab").unwrap();
        assert_eq!(bonelab.kind, LoaderKind::MelonLoader);
        // Sous-règles aplaties.
        assert!(bonelab.rules.iter().any(|r| r.route == "UserData/CustomMaps" && r.extensions.contains(&".bcm".to_string())));
        let pal = e.game("palworld").unwrap();
        assert_eq!(pal.kind, LoaderKind::Shimloader);
        assert_eq!(pal.data_folder, "Pal");
        assert_eq!(e.game("hades-ii").unwrap().exe_names, ["Hades2.exe"]);
        assert_eq!(e.game("mio-memories-in-orbit").unwrap().kind, LoaderKind::Other);
        // Le jeu, pas le serveur dédié.
        assert_eq!(e.game("valheim").unwrap().kind, LoaderKind::BepInEx);
        assert!(e.game("phasmophobia").is_none());

        let pack = e.loader_package("bepinex-bepinexpack_il2cpp").unwrap();
        assert_eq!(pack.root_folder, "BepInExPack");
        assert_eq!(e.loader_package("denikson-BepInExPack_Valheim").unwrap().root_folder, "BepInExPack_Valheim");
        assert!(e.loader_package("Deja-HighJump").is_none());
    }

    #[test]
    fn compact_copy_reads_back_the_same() {
        let e = Ecosystem::bundled();
        let again = Ecosystem::parse(e.to_compact_json().as_bytes()).unwrap();
        for id in ["lethal-company", "bonelab", "subnautica", "roadside-research", "palworld"] {
            let (a, b) = (e.game(id).unwrap(), again.game(id).unwrap());
            assert_eq!((a.kind, a.rules, a.autolist, a.exe_names, a.data_folder), (b.kind, b.rules, b.autolist, b.exe_names, b.data_folder), "{id}");
        }
        assert_eq!(e.loaders.len(), again.loaders.len());
    }
}
