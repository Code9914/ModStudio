//! Chaque jeu : où est sa liste, comment la lire, où vont ses mods, et son
//! chargeur de mods.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer};

use super::{mod_id, web_url, Artifact, Catalog, Hash, ListDep, ListMod, ListVersion, Net, Place, ZipRoot};
use crate::error::{AppError, AppResult};

// Ces listes sont tenues à la main par leur communauté : un champ à `null`
// ou une entrée mal formée ne doit pas rendre toute la liste illisible.

/// Champ absent ou `null` : sa valeur par défaut.
fn or_default<'de, D: Deserializer<'de>, T: Deserialize<'de> + Default>(d: D) -> Result<T, D::Error> {
    Ok(Option::<T>::deserialize(d)?.unwrap_or_default())
}

/// Les éléments lisibles d'un tableau (absent ou `null` : vide).
fn lenient<'de, D: Deserializer<'de>, T: DeserializeOwned>(d: D) -> Result<Vec<T>, D::Error> {
    let items = Option::<Vec<serde_json::Value>>::deserialize(d)?.unwrap_or_default();
    Ok(items.into_iter().filter_map(|v| serde_json::from_value(v).ok()).collect())
}

/// Les entrées lisibles d'une liste JSON ; aucune sur une liste non vide :
/// le format a changé, la liste est illisible.
fn entries<T: DeserializeOwned>(items: Vec<serde_json::Value>) -> Option<Vec<T>> {
    let total = items.len();
    let out: Vec<T> = items.into_iter().filter_map(|v| serde_json::from_value(v).ok()).collect();
    (!out.is_empty() || total == 0).then_some(out)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    HollowKnight,
    OuterWilds,
    BeatSaber,
    ChilloutVr,
    Raft,
}

pub struct GameDef {
    pub kind: Kind,
    pub id: &'static str,
    pub name: &'static str,
    pub steam_app_id: u32,
    /// Nom de la liste (en français et en anglais) et son site.
    pub list_name_fr: &'static str,
    pub list_name_en: &'static str,
    pub list_url: &'static str,
    pub loader_name: &'static str,
    /// Exécutables qui montrent que le dossier est le bon.
    pub exe: &'static [&'static str],
    pub about_fr: &'static str,
    pub about_en: &'static str,
}

pub const GAMES: &[GameDef] = &[
    GameDef {
        kind: Kind::HollowKnight,
        id: "hollow-knight",
        name: "Hollow Knight",
        steam_app_id: 367520,
        list_name_fr: "ModLinks",
        list_name_en: "ModLinks",
        list_url: "https://github.com/hk-modding/modlinks",
        loader_name: "Modding API",
        exe: &["hollow_knight.exe", "hollow_knight.x86_64", "Hollow Knight.exe"],
        about_fr: "Les mods de la liste ModLinks (celle de Lumafly et Scarab), chargés par le Modding API.",
        about_en: "Mods from the ModLinks list (the one Lumafly and Scarab use), loaded by the Modding API.",
    },
    GameDef {
        kind: Kind::OuterWilds,
        id: "outer-wilds",
        name: "Outer Wilds",
        steam_app_id: 753640,
        list_name_fr: "base de mods d'OWML",
        list_name_en: "OWML mod database",
        list_url: "https://outerwildsmods.com/mods/",
        loader_name: "OWML",
        exe: &["OuterWilds.exe"],
        about_fr: "Les mods de la base d'OWML (celle d'Outer Wilds Mod Manager).",
        about_en: "Mods from the OWML database (the one Outer Wilds Mod Manager uses).",
    },
    GameDef {
        kind: Kind::BeatSaber,
        id: "beat-saber",
        name: "Beat Saber",
        steam_app_id: 620980,
        list_name_fr: "BeatMods",
        list_name_en: "BeatMods",
        list_url: "https://beatmods.com",
        loader_name: "BSIPA",
        exe: &["Beat Saber.exe"],
        about_fr: "Les mods vérifiés de BeatMods pour ta version du jeu, chargés par BSIPA.",
        about_en: "Verified BeatMods mods for your version of the game, loaded by BSIPA.",
    },
    GameDef {
        kind: Kind::ChilloutVr,
        id: "chilloutvr",
        name: "ChilloutVR",
        steam_app_id: 661130,
        list_name_fr: "ChilloutVR Modding Group",
        list_name_en: "ChilloutVR Modding Group",
        list_url: "https://discord.gg/2WR6rGVzht",
        loader_name: "MelonLoader",
        exe: &["ChilloutVR.exe"],
        about_fr: "Les mods vérifiés par le ChilloutVR Modding Group, chargés par MelonLoader.",
        about_en: "Mods verified by the ChilloutVR Modding Group, loaded by MelonLoader.",
    },
    GameDef {
        kind: Kind::Raft,
        id: "raft",
        name: "Raft",
        steam_app_id: 648800,
        list_name_fr: "RaftModding",
        list_name_en: "RaftModding",
        list_url: "https://www.raftmodding.com/mods",
        loader_name: "Raft Mod Loader",
        exe: &["Raft.exe"],
        about_fr: "Les mods de RaftModding, chargés par Raft Mod Loader (le jeu se lance avec son lanceur).",
        about_en: "Mods from RaftModding, loaded by Raft Mod Loader (the game starts from its launcher).",
    },
];

pub fn game(id: &str) -> Option<&'static GameDef> {
    GAMES.iter().find(|g| g.id == id)
}

/// Entrée d'un dossier, insensible à la casse.
pub(crate) fn child(dir: &Path, name: &str) -> Option<PathBuf> {
    fs::read_dir(dir).ok()?.flatten().find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(name)).map(|e| e.path())
}

/// Nom de fichier sûr sous Windows et Linux, sans chemin.
pub(crate) fn safe_file_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 200
        && name != "."
        && name != ".."
        && name.trim() == name
        && !name.ends_with('.')
        && !name.chars().any(|c| c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
}

/// `%20` → espace.
pub(crate) fn percent_decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            out.push(u8::from_str_radix(s.get(i + 1..i + 3)?, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Dernier segment d'une adresse, décodé.
fn url_file_name(url: &str) -> Option<String> {
    let u = reqwest::Url::parse(url).ok()?;
    let name = percent_decode(u.path_segments()?.next_back()?)?;
    safe_file_name(&name).then_some(name)
}

/// Empreinte SHA-256 en hexadécimal (64 caractères) ou en base64.
fn sha256(s: &str) -> Option<Hash> {
    let s = s.trim();
    if s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Some(Hash::Sha256(s.to_ascii_lowercase()));
    }
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD.decode(s).ok()?;
    (bytes.len() == 32).then(|| Hash::Sha256(bytes.iter().map(|b| format!("{b:02x}")).collect()))
}

/// Compare deux versions (`1.10.0` > `1.9.2`), sans exiger le format semver.
pub fn version_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let parts = |s: &str| -> Vec<u64> {
        s.trim_start_matches(['v', 'V'])
            .split(|c: char| !c.is_ascii_digit())
            .filter(|p| !p.is_empty())
            .map(|p| p.parse().unwrap_or(0))
            .collect()
    };
    let (pa, pb) = (parts(a), parts(b));
    for i in 0..pa.len().max(pb.len()) {
        let (x, y) = (pa.get(i).copied().unwrap_or(0), pb.get(i).copied().unwrap_or(0));
        if x != y {
            return x.cmp(&y);
        }
    }
    std::cmp::Ordering::Equal
}

/// Détail d'erreur : la liste téléchargée ne se lit pas.
fn unreadable() -> &'static str {
    crate::i18n::pick("liste illisible", "unreadable list")
}

fn unavailable(source: &str, detail: &str) -> AppError {
    AppError::ListUnavailable { list: source.to_string(), detail: detail.to_string() }
}

impl GameDef {
    /// Nom de la liste, dans la langue de l'interface.
    pub fn list_name(&self) -> &'static str {
        crate::i18n::pick(self.list_name_fr, self.list_name_en)
    }

    /// Présentation de la liste, dans la langue de l'interface.
    pub fn about(&self) -> &'static str {
        crate::i18n::pick(self.about_fr, self.about_en)
    }

    /// Le dossier contient-il le jeu ?
    pub fn validate(&self, folder: &Path) -> bool {
        self.exe.iter().any(|e| child(folder, e).is_some_and(|p| p.is_file()))
    }

    /// Ce qui change la liste à demander pour ce dossier (Beat Saber : la
    /// date du fichier où est écrite la version du jeu).
    pub fn cache_hint(&self, folder: &Path) -> String {
        match self.kind {
            Kind::BeatSaber => fs::metadata(folder.join("Beat Saber_Data").join("globalgamemanagers"))
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs().to_string())
                .unwrap_or_default(),
            _ => String::new(),
        }
    }

    /// Dossier des données du jeu (`hollow_knight_Data`).
    pub fn data_dir(&self, folder: &Path) -> Option<String> {
        match self.kind {
            Kind::HollowKnight => ["hollow_knight_Data", "Hollow Knight_Data"]
                .iter()
                .find_map(|d| child(folder, d).filter(|p| p.is_dir()))
                .map(|p| p.file_name().unwrap_or_default().to_string_lossy().into_owned()),
            _ => None,
        }
    }

    /// `{data}/Managed/Mods` → `hollow_knight_Data/Managed/Mods`.
    pub fn resolve(&self, folder: &Path, dir: &str) -> Option<String> {
        if dir.contains("{data}") {
            let data = self.data_dir(folder)?;
            Some(dir.replace("{data}", &data))
        } else {
            Some(dir.to_string())
        }
    }

    /// Variante des fichiers à télécharger (Hollow Knight : Windows ou Linux,
    /// Proton prenant ceux de Windows).
    pub fn variant(&self, folder: Option<&Path>) -> &'static str {
        match self.kind {
            Kind::HollowKnight if cfg!(target_os = "linux") && folder.is_some_and(|f| !f.join("UnityPlayer.dll").is_file()) => "linux",
            Kind::HollowKnight if cfg!(target_os = "macos") => "mac",
            _ => "windows",
        }
    }

    pub async fn fetch_catalog(&self, net: &Net, folder: Option<&Path>, refresh: bool) -> AppResult<Catalog> {
        match self.kind {
            Kind::HollowKnight => hollow_knight::catalog(self, net, self.variant(folder), refresh).await,
            Kind::OuterWilds => outer_wilds::catalog(self, net, refresh).await,
            Kind::BeatSaber => beat_saber::catalog(self, net, folder, refresh).await,
            Kind::ChilloutVr => chilloutvr::catalog(self, net, refresh).await,
            Kind::Raft => raft::catalog(self, net, refresh).await,
        }
    }

    /// Le chargeur est-il dans le dossier du jeu (installé par ModStudio ou à la main) ?
    pub fn loader_present(&self, config: &crate::installer::GameInstall) -> bool {
        let folder = config.folder.as_path();
        match self.kind {
            Kind::HollowKnight => hollow_knight::api_present(self, folder),
            Kind::OuterWilds => outer_wilds::owml_present(config),
            Kind::BeatSaber => beat_saber::bsipa_present(folder),
            Kind::ChilloutVr => folder.join("version.dll").is_file() && folder.join("MelonLoader").is_dir(),
            Kind::Raft => raft::launcher_present(folder),
        }
    }

    /// ModStudio sait-il installer le chargeur ?
    pub fn loader_installable(&self) -> bool {
        self.kind != Kind::Raft
    }

    /// Option de lancement Steam pour Proton (Linux), quand le chargeur passe par une DLL.
    pub fn launch_option(&self) -> Option<String> {
        if cfg!(windows) {
            return None;
        }
        let dll = match self.kind {
            Kind::ChilloutVr => "version",
            Kind::BeatSaber => "winhttp",
            Kind::OuterWilds => "winhttp",
            _ => return None,
        };
        Some(format!("WINEDLLOVERRIDES=\"{dll}=n,b\" %command%"))
    }

    /// Ce qu'il faut savoir pour que les mods se chargent.
    pub fn loader_note(&self, config: &crate::installer::GameInstall, game_version: Option<&str>, mods: usize) -> Option<String> {
        let folder = config.folder.as_path();
        match self.kind {
            Kind::Raft => Some(
                crate::i18n::pick(
                    "Lance Raft avec le lanceur de Raft Mod Loader (RMLLauncher), pas depuis Steam : c'est lui qui charge les mods.",
                    "Start Raft with the Raft Mod Loader launcher (RMLLauncher), not from Steam: it's the one that loads the mods.",
                )
                .into(),
            ),
            Kind::HollowKnight => hollow_knight::version_note(folder),
            Kind::BeatSaber => match game_version {
                None => Some(
                    crate::i18n::pick(
                        "Version du jeu introuvable dans son dossier : lance Beat Saber une fois, puis reviens ici.",
                        "Game version not found in its folder: start Beat Saber once, then come back here.",
                    )
                    .into(),
                ),
                Some(v) if mods == 0 => Some(crate::tr!(
                    "BeatMods n'a pas encore de mods vérifiés pour Beat Saber {v} : ils arrivent souvent quelques semaines après une mise à jour du jeu.",
                    "BeatMods doesn't have verified mods for Beat Saber {v} yet: they often arrive a few weeks after a game update."
                )),
                Some(_) => None,
            },
            Kind::OuterWilds if outer_wilds::owml_present(config) && !outer_wilds::game_patched(folder) => Some(
                crate::i18n::pick(
                    "Lance Outer Wilds une première fois avec le bouton « Lancer avec OWML » : OWML prépare le jeu, ensuite le lancement depuis Steam suffit. À refaire après chaque mise à jour du jeu.",
                    "Start Outer Wilds once with the \"Launch with OWML\" button: OWML prepares the game, after that starting it from Steam is enough. Do it again after each game update.",
                )
                .into(),
            ),
            _ => None,
        }
    }

    /// Fichiers d'une archive à ne pas placer (réglages de l'utilisateur).
    pub fn skip_file(&self, loader: bool, rel: &str) -> bool {
        self.kind == Kind::OuterWilds && !loader && matches!(rel.to_ascii_lowercase().as_str(), "config.json" | "save.json")
    }

    /// Copies en plus de l'archive telle quelle (BSIPA : ce que ferait `IPA.exe`).
    pub fn copies(&self, loader: bool, rel: &str) -> Option<String> {
        (self.kind == Kind::BeatSaber && loader).then(|| beat_saber::bsipa_copies(rel)).flatten()
    }

    /// Mods demandés que la liste ne donne pas (Outer Wilds : dans le `manifest.json` du mod).
    pub fn discovered_dependencies(&self, extracted: &Path) -> Vec<String> {
        match self.kind {
            Kind::OuterWilds => outer_wilds::manifest_dependencies(extracted),
            _ => Vec::new(),
        }
    }

    /// Ce qu'il faut lancer pour que les mods se chargent (OWML).
    pub fn launch(&self, extra: &std::collections::BTreeMap<String, PathBuf>) -> AppResult<()> {
        match self.kind {
            Kind::OuterWilds => outer_wilds::launch(extra),
            _ => Err(AppError::InvalidArgument(
                crate::i18n::pick("rien à lancer pour ce jeu : lance-le comme d'habitude.", "nothing to launch for this game: start it as usual.").into(),
            )),
        }
    }

    /// Dossiers en dehors du dossier du jeu (OWML d'Outer Wilds).
    pub fn extra_folders(&self, folder: &Path) -> Vec<(String, PathBuf)> {
        match self.kind {
            Kind::OuterWilds => outer_wilds::extra_folders(folder),
            _ => Vec::new(),
        }
    }

    /// Après l'installation du chargeur (réglages à écrire…).
    pub fn activate(&self, folder: &Path, extra: &std::collections::BTreeMap<String, PathBuf>) -> AppResult<()> {
        match self.kind {
            Kind::OuterWilds => outer_wilds::activate(folder, extra),
            Kind::BeatSaber => beat_saber::activate(folder),
            _ => Ok(()),
        }
    }
}

// ---------------------------------------------------------------- Hollow Knight

pub(crate) mod hollow_knight {
    use super::*;
    use roxmltree::{Document, Node};

    pub const API_ID: &str = "Modding API";
    /// Seule version du jeu pour laquelle le Modding API est publié.
    pub const API_GAME_VERSION: &str = "1.5.78.11833";

    fn text<'a>(n: Node<'a, 'a>, tag: &str) -> Option<String> {
        n.children().find(|c| c.has_tag_name(tag)).and_then(|c| c.text()).map(|t| t.trim().to_string()).filter(|t| !t.is_empty())
    }

    fn list<'a>(n: Node<'a, 'a>, tag: &str, item: &str) -> Vec<String> {
        n.children()
            .find(|c| c.has_tag_name(tag))
            .map(|c| c.children().filter(|x| x.has_tag_name(item)).filter_map(|x| x.text().map(|t| t.trim().to_string())).collect())
            .unwrap_or_default()
    }

    /// Lien (et empreinte) pour ce système : `<Link>` ou `<Links><Windows>…`.
    fn link<'a>(n: Node<'a, 'a>, variant: &str) -> Option<(String, Hash)> {
        let node = if let Some(l) = n.children().find(|c| c.has_tag_name("Link")) {
            l
        } else {
            let links = n.children().find(|c| c.has_tag_name("Links"))?;
            let tag = match variant {
                "linux" => "Linux",
                "mac" => "Mac",
                _ => "Windows",
            };
            links.children().find(|c| c.has_tag_name(tag))?
        };
        let url = node.text()?.trim().to_string();
        let hash = sha256(node.attribute("SHA256")?)?;
        (web_url(&url) || cfg!(test)).then_some((url, hash))
    }

    pub fn parse(game: &GameDef, modlinks: &str, apilinks: &str, variant: &str) -> Option<Vec<ListMod>> {
        let opts = roxmltree::ParsingOptions { allow_dtd: false, ..Default::default() };
        let doc = Document::parse_with_options(modlinks, opts).ok()?;
        let mut mods = Vec::new();
        for m in doc.root_element().children().filter(|c| c.has_tag_name("Manifest")) {
            let Some(name) = text(m, "Name").filter(|n| safe_file_name(n)) else { continue };
            let Some(version) = text(m, "Version") else { continue };
            let Some((url, hash)) = link(m, variant) else { continue };
            let lower = url.to_ascii_lowercase();
            let dir = format!("{{data}}/Managed/Mods/{name}");
            let place = if lower.ends_with(".dll") {
                Place::File { key: String::new(), dir: dir.clone(), name: url_file_name(&url) }
            } else {
                Place::Zip { key: String::new(), dir: dir.clone(), root: ZipRoot::AsIs }
            };
            let dependencies = list(m, "Dependencies", "Dependency")
                .into_iter()
                .map(|d| ListDep { name: d.clone(), id: d, version: String::new(), optional: false })
                .collect();
            let tags = list(m, "Tags", "Tag");
            let authors = list(m, "Authors", "Author");
            let repo = text(m, "Repository").filter(|u| web_url(u));
            let description = text(m, "Description").unwrap_or_default();
            let lower_desc = description.to_ascii_lowercase();
            mods.push(ListMod {
                mod_id: mod_id(game.id, &name),
                name: text(m, "DisplayName").unwrap_or_else(|| name.clone()),
                id: name,
                category: tags.first().cloned(),
                author: authors.join(", "),
                author_url: None,
                icon: avatar(repo.as_deref()),
                icon_fallback: None,
                page: repo,
                deprecated: lower_desc.contains("deprecated") || lower_desc.starts_with("[obsolete]"),
                broken: false,
                compatible: true,
                note: None,
                loader: false,
                downloads: None,
                updated: None,
                latest: version.clone(),
                tags,
                description,
                versions: vec![ListVersion {
                    version,
                    changelog: None,
                    release_url: None,
                    dependencies,
                    conflicts: Vec::new(),
                    game_versions: Vec::new(),
                    files: vec![format!("{dir}/")],
                    artifacts: vec![Artifact { url, hash, place, replaces: false, contents: Vec::new() }],
                }],
            });
        }
        // Le Modding API, publié à part.
        let api = Document::parse_with_options(apilinks, opts).ok()?;
        let manifest = api.root_element().children().find(|c| c.has_tag_name("Manifest")).unwrap_or(api.root_element());
        if let (Some(version), Some((url, hash))) = (text(manifest, "Version"), link(manifest, variant)) {
            mods.push(ListMod {
                id: API_ID.into(),
                mod_id: mod_id(game.id, API_ID),
                name: API_ID.into(),
                description: crate::i18n::pick(
                    "Le chargeur de mods de Hollow Knight : il remplace quelques fichiers du jeu (gardés de côté et remis à la désinstallation).",
                    "Hollow Knight's mod loader: it replaces a few game files (set aside and put back when you uninstall).",
                )
                .into(),
                category: Some("Library".into()),
                author: "hk-modding".into(),
                author_url: Some("https://github.com/hk-modding".into()),
                icon: Some("https://github.com/hk-modding.png".into()),
                icon_fallback: None,
                page: Some("https://github.com/hk-modding/api".into()),
                tags: Vec::new(),
                deprecated: false,
                broken: false,
                compatible: true,
                note: None,
                loader: true,
                downloads: None,
                updated: None,
                latest: version.clone(),
                versions: vec![ListVersion {
                    version,
                    changelog: None,
                    release_url: Some("https://github.com/hk-modding/api/releases".into()),
                    dependencies: Vec::new(),
                    conflicts: Vec::new(),
                    game_versions: vec![API_GAME_VERSION.into()],
                    files: vec!["{data}/Managed/".into()],
                    artifacts: vec![Artifact {
                        url,
                        hash,
                        place: Place::Zip { key: String::new(), dir: "{data}/Managed".into(), root: ZipRoot::AsIs },
                        replaces: true,
                        contents: Vec::new(),
                    }],
                }],
            });
        }
        Some(mods)
    }

    /// Portrait du compte GitHub du dépôt.
    pub fn avatar(repo: Option<&str>) -> Option<String> {
        let u = reqwest::Url::parse(repo?).ok()?;
        if u.host_str() != Some("github.com") {
            return None;
        }
        let user = u.path_segments()?.find(|s| !s.is_empty())?;
        user.chars().all(|c| c.is_ascii_alphanumeric() || c == '-').then(|| format!("https://github.com/{user}.png"))
    }

    pub async fn catalog(game: &GameDef, net: &Net, variant: &str, refresh: bool) -> AppResult<Catalog> {
        let modlinks = net.get(&net.urls.modlinks, refresh, "ModLinks").await?;
        let apilinks = net.get(&net.urls.apilinks, refresh, "ModLinks").await?;
        let mods = parse(game, &String::from_utf8_lossy(&modlinks), &String::from_utf8_lossy(&apilinks), variant)
            .ok_or_else(|| unavailable("ModLinks", unreadable()))?;
        Ok(Catalog::new(mods, None))
    }

    fn assembly(game: &GameDef, folder: &Path) -> Option<PathBuf> {
        let data = game.data_dir(folder)?;
        Some(folder.join(data).join("Managed").join("Assembly-CSharp.dll"))
    }

    /// Le Modding API est-il dans `Assembly-CSharp.dll` (type `Modding.ModHooks`) ?
    pub fn api_present(game: &GameDef, folder: &Path) -> bool {
        assembly(game, folder).and_then(|p| fs::read(p).ok()).is_some_and(|b| b.windows(8).any(|w| w == b"ModHooks"))
    }

    /// Version du jeu écrite dans `Assembly-CSharp.dll` (`Constants.GAME_VERSION`, en UTF-16).
    pub fn game_version(game: &GameDef, folder: &Path) -> Option<String> {
        let bytes = fs::read(assembly(game, folder)?).ok()?;
        // La version du Modding API d'abord (d'autres chaînes « 1.x.y.z » peuvent précéder).
        let wanted: Vec<u8> = API_GAME_VERSION.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        if bytes.windows(wanted.len()).any(|w| w == wanted.as_slice()) {
            return Some(API_GAME_VERSION.to_string());
        }
        // Chaînes « 1.x.y.z » en UTF-16 (à une adresse paire ou impaire) : la version du jeu.
        (0..2).find_map(|start| {
            let units = bytes.get(start..)?.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]]));
            let mut run = String::new();
            for u in units.chain(std::iter::once(0)) {
                match char::from_u32(u32::from(u)) {
                    Some(c) if c.is_ascii_digit() || c == '.' => run.push(c),
                    _ => {
                        let parts: Vec<&str> = run.split('.').collect();
                        if parts.len() == 4 && parts[0] == "1" && parts.iter().all(|p| !p.is_empty()) {
                            return Some(run);
                        }
                        run.clear();
                    }
                }
            }
            None
        })
    }

    pub fn version_note(folder: &Path) -> Option<String> {
        let game = super::game("hollow-knight")?;
        let v = game_version(game, folder)?;
        (v != API_GAME_VERSION).then(|| {
            crate::tr!(
                "Ton jeu est en version {v} : le Modding API n'existe que pour la version {API_GAME_VERSION}. Choisis-la dans Steam (propriétés du jeu, onglet Bêtas) avant d'installer des mods.",
                "Your game is version {v}: the Modding API only exists for version {API_GAME_VERSION}. Pick it in Steam (game properties, Betas tab) before installing mods."
            )
        })
    }
}

// ---------------------------------------------------------------- ChilloutVR

pub(crate) mod chilloutvr {
    use super::*;

    pub const LOADER_ID: &str = "MelonLoader";

    #[derive(Deserialize)]
    struct Entry {
        #[serde(rename = "_id")]
        id: i64,
        #[serde(default, deserialize_with = "lenient")]
        aliases: Vec<String>,
        category: Option<String>,
        #[serde(default, deserialize_with = "lenient")]
        versions: Vec<Version>,
        name: Option<String>,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Version {
        #[serde(default, deserialize_with = "or_default")]
        approval_status: i64,
        name: Option<String>,
        mod_version: Option<String>,
        game_version: Option<String>,
        loader_version: Option<String>,
        mod_type: Option<String>,
        author: Option<String>,
        description: Option<String>,
        #[serde(default, deserialize_with = "lenient")]
        search_tags: Vec<String>,
        #[serde(default, deserialize_with = "lenient")]
        requirements: Vec<String>,
        download_link: Option<String>,
        source_link: Option<String>,
        changelog: Option<String>,
        hash: Option<String>,
        update_date: Option<String>,
    }

    #[derive(Deserialize)]
    struct Release {
        tag_name: String,
        #[serde(default, deserialize_with = "lenient")]
        assets: Vec<Asset>,
    }

    #[derive(Deserialize)]
    struct Asset {
        name: String,
        browser_download_url: String,
        digest: Option<String>,
    }

    /// `[BTKUILib](…/v1/mods/download/113)` → `113` ; un nom seul → ce nom.
    fn requirement(text: &str) -> Option<(String, bool)> {
        let t = text.trim();
        let lower = t.to_ascii_lowercase();
        if t.is_empty() || matches!(lower.trim_end_matches('!'), "none" | "n/a" | "-") {
            return None;
        }
        let optional = lower.contains("optional") || lower.contains("supports");
        for marker in ["/v1/mods/download/", "/v1/mods/files/"] {
            if let Some(i) = lower.find(marker) {
                let id: String = lower[i + marker.len()..].chars().take_while(char::is_ascii_digit).collect();
                if !id.is_empty() {
                    return Some((format!("#{id}"), optional));
                }
            }
        }
        // `[Nom](lien)` : le nom ; sinon le texte.
        let name = t.strip_prefix('[').and_then(|r| r.split(']').next()).unwrap_or(t);
        let name = name.rsplit(" - ").next().unwrap_or(name).trim();
        Some((name.to_string(), optional))
    }

    pub fn parse(game: &GameDef, json: &[u8], melon: Option<&[u8]>) -> Option<Vec<ListMod>> {
        let entries: Vec<Entry> = entries(serde_json::from_slice(json).ok()?)?;
        // Noms et alias → identifiant, pour les mods demandés par leur nom.
        let mut by_name: HashMap<String, String> = HashMap::new();
        for e in &entries {
            let id = e.id.to_string();
            let names = e.aliases.iter().chain(e.name.iter()).chain(e.versions.first().and_then(|v| v.name.as_ref()));
            for n in names {
                by_name.entry(n.to_lowercase()).or_insert_with(|| id.clone());
            }
        }
        let mut mods = Vec::new();
        for e in entries {
            let Some(v) = e.versions.into_iter().next() else { continue };
            let (Some(link), Some(version)) = (v.download_link.filter(|u| web_url(u) || cfg!(test)), v.mod_version) else { continue };
            let Some(hash) = v.hash.as_deref().and_then(sha256) else { continue };
            let id = e.id.to_string();
            let name = v.name.or(e.name).unwrap_or_else(|| format!("Mod {id}"));
            let plugin = v.mod_type.as_deref().is_some_and(|t| t.eq_ignore_ascii_case("plugin"));
            let dir = if plugin { "Plugins" } else { "Mods" };
            let mut dependencies: Vec<ListDep> = Vec::new();
            for r in &v.requirements {
                let Some((dep, optional)) = requirement(r) else { continue };
                let dep_id = match dep.strip_prefix('#') {
                    Some(n) => n.to_string(),
                    None => match by_name.get(&dep.to_lowercase()) {
                        Some(i) => i.clone(),
                        None => continue,
                    },
                };
                if dep_id != id && !dependencies.iter().any(|d| d.id == dep_id) {
                    dependencies.push(ListDep { name: dep_id.clone(), id: dep_id, version: String::new(), optional });
                }
            }
            let loader = v.loader_version.map(|l| l.trim_start_matches(['v', 'V']).to_string());
            mods.push(ListMod {
                mod_id: mod_id(game.id, &id),
                id,
                name,
                description: v.description.unwrap_or_default().trim().to_string(),
                category: e.category,
                author: v.author.unwrap_or_default(),
                author_url: None,
                icon: hollow_knight::avatar(v.source_link.as_deref()),
                icon_fallback: None,
                page: v.source_link.filter(|u| web_url(u)),
                tags: v.search_tags,
                deprecated: v.approval_status == 3,
                broken: v.approval_status == 2,
                compatible: true,
                note: match v.approval_status {
                    2 => Some(crate::i18n::pick("Signalé comme cassé par le ChilloutVR Modding Group.", "Reported as broken by the ChilloutVR Modding Group.").into()),
                    3 => Some(crate::i18n::pick("Retiré : il n'est plus maintenu.", "Removed: it's no longer maintained.").into()),
                    _ => None,
                },
                loader: false,
                downloads: None,
                updated: v.update_date,
                latest: version.clone(),
                versions: vec![ListVersion {
                    version,
                    changelog: v.changelog.filter(|c| !c.trim().is_empty()),
                    release_url: None,
                    dependencies,
                    conflicts: Vec::new(),
                    game_versions: v.game_version.into_iter().chain(loader.map(|l| format!("MelonLoader {l}"))).collect(),
                    files: vec![format!("{dir}/")],
                    artifacts: vec![Artifact { url: link, hash, place: Place::File { key: String::new(), dir: dir.into(), name: None }, replaces: false, contents: Vec::new() }],
                }],
            });
        }
        // MelonLoader, depuis ses versions GitHub.
        if let Some(r) = melon.and_then(|b| serde_json::from_slice::<Release>(b).ok())
            && let Some(a) = r.assets.iter().find(|a| a.name == "MelonLoader.x64.zip" && (web_url(&a.browser_download_url) || cfg!(test)))
            && let Some(hash) = a.digest.as_deref().and_then(|d| d.strip_prefix("sha256:")).and_then(sha256)
        {
            mods.push(ListMod {
                id: LOADER_ID.into(),
                mod_id: mod_id(game.id, LOADER_ID),
                name: "MelonLoader".into(),
                description: crate::i18n::pick("Le chargeur de mods de ChilloutVR.", "ChilloutVR's mod loader.").into(),
                category: Some("Core Mods & Libraries".into()),
                author: "LavaGang".into(),
                author_url: Some("https://github.com/LavaGang".into()),
                icon: Some("https://github.com/LavaGang.png".into()),
                icon_fallback: None,
                page: Some("https://github.com/LavaGang/MelonLoader".into()),
                tags: Vec::new(),
                deprecated: false,
                broken: false,
                compatible: true,
                note: None,
                loader: true,
                downloads: None,
                updated: None,
                latest: r.tag_name.trim_start_matches('v').to_string(),
                versions: vec![ListVersion {
                    version: r.tag_name.trim_start_matches('v').to_string(),
                    changelog: None,
                    release_url: Some(format!("https://github.com/LavaGang/MelonLoader/releases/tag/{}", r.tag_name)),
                    dependencies: Vec::new(),
                    conflicts: Vec::new(),
                    game_versions: Vec::new(),
                    files: vec!["version.dll".into(), "MelonLoader/".into()],
                    artifacts: vec![Artifact {
                        url: a.browser_download_url.clone(),
                        hash,
                        place: Place::Zip { key: String::new(), dir: String::new(), root: ZipRoot::AsIs },
                        replaces: false,
                        contents: Vec::new(),
                    }],
                }],
            });
        }
        Some(mods)
    }

    pub async fn catalog(game: &GameDef, net: &Net, refresh: bool) -> AppResult<Catalog> {
        let json = net.get(&net.urls.cvrmg, refresh, "ChilloutVR Modding Group").await?;
        let melon = net.get(&format!("{}/repos/LavaGang/MelonLoader/releases/latest", net.urls.github_api), refresh, "GitHub").await.ok();
        let mods = parse(game, &json, melon.as_deref()).ok_or_else(|| unavailable("ChilloutVR Modding Group", unreadable()))?;
        Ok(Catalog::new(mods, None))
    }
}

// ---------------------------------------------------------------- Raft

pub(crate) mod raft {
    use super::*;

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Entry {
        id: String,
        title: String,
        #[serde(default, deserialize_with = "or_default")]
        description: String,
        category: Option<String>,
        #[serde(default, deserialize_with = "or_default")]
        author: String,
        banner_image_url: Option<String>,
        icon_image_url: Option<String>,
        #[serde(rename = "mod-versions", default, deserialize_with = "lenient")]
        versions: Vec<Version>,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Version {
        id: i64,
        version: String,
        changelog: Option<String>,
        download_url: String,
        #[serde(default, deserialize_with = "or_default")]
        download_count: u64,
        /// `null` pour les vieux mods (d'avant les `.rmod`).
        max_raft_version_id: Option<i64>,
        #[serde(default, deserialize_with = "or_default")]
        definite_max_raft_version: bool,
        created_at: Option<String>,
    }

    fn category(c: &str) -> String {
        match c {
            "utility" => crate::i18n::pick("Utilitaires", "Utilities"),
            "content" => crate::i18n::pick("Contenu", "Content"),
            "fun" => crate::i18n::pick("Pour rire", "Fun"),
            "optimization" => crate::i18n::pick("Optimisation", "Optimization"),
            other => other,
        }
        .to_string()
    }

    pub fn parse(game: &GameDef, base: &str, json: &[u8]) -> Option<Vec<ListMod>> {
        let entries: Vec<Entry> = entries(serde_json::from_slice(json).ok()?)?;
        // La version de Raft la plus récente visée par un mod : celle du jeu.
        let current = entries.iter().flat_map(|e| e.versions.iter().filter_map(|v| v.max_raft_version_id)).max().unwrap_or(0);
        let mut mods = Vec::new();
        for mut e in entries {
            if e.id.is_empty() || e.id.len() > 100 || !e.id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')) {
                continue;
            }
            e.versions.sort_by_key(|v| std::cmp::Reverse(v.id));
            let downloads = e.versions.iter().map(|v| v.download_count).sum();
            let old = e.versions.first().is_some_and(|v| v.definite_max_raft_version && v.max_raft_version_id.is_some_and(|m| m < current));
            let versions: Vec<ListVersion> = e
                .versions
                .iter()
                .filter_map(|v| {
                    let path = v.download_url.trim();
                    if !path.starts_with('/') || !path.to_ascii_lowercase().ends_with(".rmod") {
                        return None;
                    }
                    let name = percent_decode(path.rsplit('/').next()?).filter(|n| safe_file_name(n))?;
                    let encoded: String = path.split('/').map(|s| s.replace(' ', "%20")).collect::<Vec<_>>().join("/");
                    // RaftModding demande de passer son avertissement (fichier seulement analysé par VirusTotal).
                    let url = format!("{}{encoded}?ignoreVirusScan=true", base.trim_end_matches('/'));
                    Some(ListVersion {
                        version: v.version.clone(),
                        changelog: v.changelog.clone().filter(|c| !c.trim().is_empty()),
                        release_url: None,
                        dependencies: Vec::new(),
                        conflicts: Vec::new(),
                        game_versions: Vec::new(),
                        files: vec![format!("mods/{name}")],
                        artifacts: vec![Artifact {
                            url,
                            hash: Hash::None,
                            place: Place::File { key: String::new(), dir: "mods".into(), name: Some(name) },
                            replaces: false,
                            contents: Vec::new(),
                        }],
                    })
                })
                .collect();
            let Some(latest) = versions.first().map(|v| v.version.clone()) else { continue };
            mods.push(ListMod {
                mod_id: mod_id(game.id, &e.id),
                page: Some(format!("https://www.raftmodding.com/mods/{}", e.id)),
                name: e.title.trim().to_string(),
                description: e.description.trim().to_string(),
                category: e.category.as_deref().map(category),
                author: e.author,
                author_url: None,
                icon: e.icon_image_url.filter(|u| web_url(u)),
                icon_fallback: e.banner_image_url.filter(|u| web_url(u)),
                tags: Vec::new(),
                deprecated: old,
                broken: false,
                compatible: true,
                note: old.then(|| crate::i18n::pick("Fait pour une ancienne version de Raft.", "Made for an older version of Raft.").to_string()),
                loader: false,
                downloads: Some(downloads),
                updated: e.versions.first().and_then(|v| v.created_at.clone()),
                latest,
                id: e.id,
                versions,
            });
        }
        Some(mods)
    }

    pub async fn catalog(game: &GameDef, net: &Net, refresh: bool) -> AppResult<Catalog> {
        let base = net.urls.raft.clone();
        let json = net.get(&format!("{base}/api/v1/mods"), refresh, "RaftModding").await?;
        let mods = parse(game, &base, &json).ok_or_else(|| unavailable("RaftModding", unreadable()))?;
        Ok(Catalog::new(mods, None))
    }

    /// Le lanceur de Raft Mod Loader installe ses fichiers dans `%AppData%\RaftModLoader`.
    pub fn launcher_present(folder: &Path) -> bool {
        let prefix = crate::games::proton::prefix_for_dir(folder);
        crate::games::proton::expand("%APPDATA%\\RaftModLoader", prefix.as_deref()).is_some_and(|p| Path::new(&p).is_dir())
    }
}

// ---------------------------------------------------------------- Beat Saber

pub(crate) mod beat_saber {
    use super::*;

    #[derive(Deserialize)]
    struct Mods {
        #[serde(default, deserialize_with = "or_default")]
        mods: Vec<serde_json::Value>,
    }

    #[derive(Deserialize)]
    struct Item {
        #[serde(rename = "mod")]
        info: Info,
        latest: Option<Version>,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Info {
        id: i64,
        name: String,
        #[serde(default, deserialize_with = "or_default")]
        summary: String,
        #[serde(default, deserialize_with = "or_default")]
        description: String,
        category: Option<String>,
        #[serde(default, deserialize_with = "lenient")]
        authors: Vec<Author>,
        git_url: Option<String>,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Author {
        display_name: Option<String>,
        username: Option<String>,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Version {
        id: i64,
        mod_version: String,
        zip_hash: String,
        // Pas d'élément ignoré pour les empreintes : une entrée illisible
        // laisse tout le mod de côté.
        #[serde(default, deserialize_with = "or_default")]
        content_hashes: Vec<ContentHash>,
        #[serde(default, deserialize_with = "or_default")]
        dependencies: Vec<i64>,
        #[serde(default, deserialize_with = "or_default")]
        download_count: u64,
        updated_at: Option<String>,
    }

    #[derive(Deserialize)]
    struct ContentHash {
        path: String,
        hash: String,
    }

    fn category(c: &str) -> String {
        match c {
            "core" => crate::i18n::pick("Essentiels du jeu", "Core"),
            "essential" => crate::i18n::pick("Recommandés", "Recommended"),
            "library" => crate::i18n::pick("Bibliothèques", "Libraries"),
            "cosmetic" => crate::i18n::pick("Apparence", "Cosmetic"),
            "practice" => crate::i18n::pick("Entraînement", "Practice"),
            "gameplay" => crate::i18n::pick("Jeu", "Gameplay"),
            "streamtools" => crate::i18n::pick("Streaming", "Streaming"),
            "ui" => crate::i18n::pick("Interface", "UI"),
            "lighting" => crate::i18n::pick("Lumières", "Lighting"),
            "tweaks" => crate::i18n::pick("Réglages", "Tweaks"),
            "multiplayer" => crate::i18n::pick("Multijoueur", "Multiplayer"),
            "text" => crate::i18n::pick("Texte", "Text"),
            "editor" => crate::i18n::pick("Éditeur", "Editor"),
            "other" => crate::i18n::pick("Divers", "Other"),
            other => other,
        }
        .to_string()
    }

    /// Version du jeu écrite dans `Beat Saber_Data/globalgamemanagers` : la
    /// plus récente des versions connues de BeatMods qui y figure.
    pub fn detect_version(folder: &Path, known: &[String]) -> Option<String> {
        let bytes = fs::read(child(folder, "Beat Saber_Data")?.join("globalgamemanagers")).ok()?;
        known.iter().find(|v| {
            let needle = v.as_bytes();
            bytes.windows(needle.len()).enumerate().any(|(i, w)| {
                w == needle && !bytes.get(i + needle.len()).is_some_and(u8::is_ascii_digit) && !(i > 0 && (bytes[i - 1].is_ascii_digit() || bytes[i - 1] == b'.'))
            })
        }).cloned()
    }

    pub fn parse(game: &GameDef, base: &str, json: &[u8]) -> Option<Vec<ListMod>> {
        let items: Vec<Item> = entries(serde_json::from_slice::<Mods>(json).ok()?.mods)?;
        // Les mods demandés sont désignés par l'identifiant de leur version.
        let by_version: HashMap<i64, String> = items.iter().filter_map(|i| i.latest.as_ref().map(|v| (v.id, i.info.id.to_string()))).collect();
        let mut mods = Vec::new();
        for item in items {
            let Some(v) = item.latest else { continue };
            let hash_ok = |h: &str| h.len() == 32 && h.bytes().all(|b| b.is_ascii_hexdigit());
            if !hash_ok(&v.zip_hash) || v.content_hashes.iter().any(|c| !hash_ok(&c.hash) || c.path.contains("..")) {
                continue;
            }
            let id = item.info.id.to_string();
            let loader = item.info.name == "BSIPA";
            let dependencies = v
                .dependencies
                .iter()
                .filter_map(|d| by_version.get(d))
                .filter(|d| **d != id)
                .map(|d| ListDep { id: d.clone(), name: d.clone(), version: String::new(), optional: false })
                .collect();
            let authors: Vec<String> = item.info.authors.iter().filter_map(|a| a.display_name.clone().or(a.username.clone())).collect();
            let git = item.info.git_url.filter(|u| web_url(u));
            let description = if item.info.description.trim().is_empty() { item.info.summary } else { item.info.description };
            mods.push(ListMod {
                mod_id: mod_id(game.id, &id),
                id,
                name: item.info.name.trim().to_string(),
                description: description.trim().to_string(),
                category: item.info.category.as_deref().map(category),
                author: authors.join(", "),
                author_url: None,
                icon: hollow_knight::avatar(git.as_deref()),
                icon_fallback: None,
                page: git,
                tags: Vec::new(),
                deprecated: false,
                broken: false,
                compatible: true,
                note: None,
                loader,
                downloads: Some(v.download_count),
                updated: v.updated_at,
                latest: v.mod_version.clone(),
                versions: vec![ListVersion {
                    version: v.mod_version,
                    changelog: None,
                    release_url: None,
                    dependencies,
                    conflicts: Vec::new(),
                    game_versions: Vec::new(),
                    files: v.content_hashes.iter().map(|c| c.path.clone()).collect(),
                    artifacts: vec![Artifact {
                        url: format!("{}/cdn/mod/{}.zip", base.trim_end_matches('/'), v.zip_hash.to_ascii_lowercase()),
                        hash: Hash::None,
                        place: Place::Zip { key: String::new(), dir: String::new(), root: ZipRoot::AsIs },
                        replaces: false,
                        contents: v.content_hashes.into_iter().map(|c| (c.path, c.hash.to_ascii_lowercase())).collect(),
                    }],
                }],
            });
        }
        Some(mods)
    }

    pub async fn catalog(game: &GameDef, net: &Net, folder: Option<&Path>, refresh: bool) -> AppResult<Catalog> {
        let base = net.urls.beatmods.clone();
        let versions = net.get(&format!("{base}/api/beatmods/versions"), refresh, "BeatMods").await?;
        let mut known: Vec<String> = serde_json::from_slice(&versions).map_err(|_| unavailable("BeatMods", crate::i18n::pick("liste des versions illisible", "unreadable version list")))?;
        known.retain(|v| !v.trim().is_empty());
        let Some(version) = folder.and_then(|f| detect_version(f, &known)) else { return Ok(Catalog::new(Vec::new(), None)) };
        let url = format!("{base}/api/mods?status=verified&gameVersion={version}&gameName=BeatSaber&platform=steampc");
        let json = net.get(&url, refresh, "BeatMods").await?;
        let mods = parse(game, &base, &json).ok_or_else(|| unavailable("BeatMods", unreadable()))?;
        Ok(Catalog::new(mods, Some(version)))
    }

    /// BSIPA est installé : son `winhttp.dll` et son injecteur sont en place.
    pub fn bsipa_present(folder: &Path) -> bool {
        folder.join("winhttp.dll").is_file() && folder.join("Beat Saber_Data").join("Managed").join("IPA.Injector.dll").is_file()
    }

    /// Ce que fait `IPA.exe` à l'installation : `IPA/Data` dans `Beat Saber_Data`,
    /// `IPA/Libs` dans `Libs` et `IPA/winhttp.dll` à la racine (en plus de l'archive telle quelle).
    pub fn bsipa_copies(rel: &str) -> Option<String> {
        let rest = rel.strip_prefix("IPA/")?;
        if let Some(r) = rest.strip_prefix("Data/") {
            return Some(format!("Beat Saber_Data/{r}"));
        }
        if let Some(r) = rest.strip_prefix("Libs/") {
            return Some(format!("Libs/{r}"));
        }
        (!rest.contains('/') && !rest.is_empty()).then(|| rest.to_string())
    }

    pub fn activate(folder: &Path) -> AppResult<()> {
        fs::create_dir_all(folder.join("Plugins")).map_err(crate::installer::io_err)
    }
}

// ---------------------------------------------------------------- Outer Wilds

pub(crate) mod outer_wilds {
    use super::*;

    pub const OWML_ID: &str = "Alek.OWML";
    pub const OWML_KEY: &str = "owml";

    #[derive(Deserialize)]
    struct Db {
        #[serde(default, deserialize_with = "or_default")]
        releases: Vec<serde_json::Value>,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Release {
        name: String,
        unique_name: String,
        #[serde(default, deserialize_with = "or_default")]
        description: String,
        #[serde(default, deserialize_with = "or_default")]
        author: String,
        author_display: Option<String>,
        repo: Option<String>,
        version: String,
        download_url: String,
        #[serde(default, deserialize_with = "or_default")]
        download_count: u64,
        #[serde(default, deserialize_with = "lenient")]
        tags: Vec<String>,
        thumbnail: Option<Thumbnail>,
        latest_release_date: Option<String>,
        latest_release_description: Option<String>,
    }

    #[derive(Deserialize)]
    struct Thumbnail {
        main: Option<String>,
    }

    pub fn parse(game: &GameDef, json: &[u8]) -> Option<Vec<ListMod>> {
        let releases: Vec<Release> = entries(serde_json::from_slice::<Db>(json).ok()?.releases)?;
        let mut mods = Vec::new();
        for r in releases {
            let id = r.unique_name.trim().to_string();
            if !safe_file_name(&id) || !web_url(&r.download_url) && !cfg!(test) {
                continue;
            }
            let loader = id == OWML_ID;
            let version = r.version.trim().trim_start_matches('v').to_string();
            let place = if loader {
                Place::Zip { key: OWML_KEY.into(), dir: String::new(), root: ZipRoot::AsIs }
            } else {
                Place::Zip { key: OWML_KEY.into(), dir: format!("Mods/{id}"), root: ZipRoot::DirOf("manifest.json") }
            };
            let thumb = r
                .thumbnail
                .and_then(|t| t.main)
                .filter(|f| safe_file_name(f))
                .map(|f| format!("https://ow-mods.github.io/ow-mod-db/thumbnails/{f}"));
            let repo = r.repo.filter(|u| web_url(u));
            mods.push(ListMod {
                mod_id: mod_id(game.id, &id),
                name: r.name.trim().to_string(),
                description: r.description.trim().to_string(),
                category: r.tags.first().cloned(),
                author: r.author_display.unwrap_or(r.author),
                author_url: None,
                icon: thumb,
                icon_fallback: hollow_knight::avatar(repo.as_deref()),
                page: repo,
                deprecated: false,
                broken: false,
                compatible: true,
                note: None,
                loader,
                downloads: Some(r.download_count),
                updated: r.latest_release_date,
                latest: version.clone(),
                tags: r.tags,
                versions: vec![ListVersion {
                    version,
                    changelog: r.latest_release_description.filter(|c| !c.trim().is_empty()),
                    release_url: None,
                    dependencies: Vec::new(),
                    conflicts: Vec::new(),
                    game_versions: Vec::new(),
                    files: vec![if loader { "OWML/".into() } else { format!("OWML/Mods/{id}/") }],
                    artifacts: vec![Artifact { url: r.download_url, hash: Hash::None, place, replaces: false, contents: Vec::new() }],
                }],
                id,
            });
        }
        Some(mods)
    }

    pub async fn catalog(game: &GameDef, net: &Net, refresh: bool) -> AppResult<Catalog> {
        let json = net.get(&net.urls.owdb, refresh, game.list_name()).await?;
        let mods = parse(game, &json).ok_or_else(|| unavailable(game.list_name(), unreadable()))?;
        Ok(Catalog::new(mods, None))
    }

    /// Dossier d'OWML, celui d'Outer Wilds Mod Manager (partagé avec lui).
    pub fn owml_dir() -> Option<PathBuf> {
        if cfg!(test) {
            return None;
        }
        if cfg!(windows) {
            std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("OuterWildsModManager").join("OWML"))
        } else {
            let data = std::env::var_os("XDG_DATA_HOME")
                .map(PathBuf::from)
                .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local").join("share")))?;
            Some(data.join("OuterWildsModManager").join("OWML"))
        }
    }

    pub fn extra_folders(_folder: &Path) -> Vec<(String, PathBuf)> {
        let Some(dir) = owml_dir() else { return Vec::new() };
        let _ = fs::create_dir_all(&dir);
        vec![(OWML_KEY.into(), dir)]
    }

    pub fn owml_present(config: &crate::installer::GameInstall) -> bool {
        config.extra_folders.get(OWML_KEY).is_some_and(|d| d.join("OWML.Launcher.exe").is_file() && d.join("OWML.Manifest.json").is_file())
    }

    /// OWML a-t-il déjà préparé le jeu (ses fichiers copiés dans `Managed`) ?
    pub fn game_patched(folder: &Path) -> bool {
        child(folder, "OuterWilds_Data").is_some_and(|d| d.join("Managed").join("OWML.ModLoader.dll").is_file())
    }

    /// `OWML.Config.json` (d'après `OWML.DefaultConfig.json`) avec le dossier du jeu.
    pub fn activate(folder: &Path, extra: &std::collections::BTreeMap<String, PathBuf>) -> AppResult<()> {
        let Some(dir) = extra.get(OWML_KEY) else { return Ok(()) };
        let config = dir.join("OWML.Config.json");
        let mut value: serde_json::Value = fs::read(&config)
            .ok()
            .or_else(|| fs::read(dir.join("OWML.DefaultConfig.json")).ok())
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_else(|| serde_json::json!({}));
        if let Some(obj) = value.as_object_mut() {
            obj.insert("gamePath".into(), serde_json::Value::String(folder.to_string_lossy().into_owned()));
        }
        let text = serde_json::to_string_pretty(&value).map_err(|e| AppError::Io(e.to_string()))?;
        fs::write(&config, text).map_err(crate::installer::io_err)
    }

    /// Mods demandés par le `manifest.json` du mod (par leur `uniqueName`).
    pub fn manifest_dependencies(mod_dir: &Path) -> Vec<String> {
        let Some(v) = fs::read(mod_dir.join("manifest.json")).ok().and_then(|b| serde_json::from_slice::<serde_json::Value>(strip_bom(&b)).ok()) else {
            return Vec::new();
        };
        v.get("dependencies")
            .and_then(|d| d.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
            .unwrap_or_default()
    }

    fn strip_bom(b: &[u8]) -> &[u8] {
        b.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(b)
    }

    /// Lance OWML : il prépare le jeu (à refaire après une mise à jour), puis le lance.
    pub fn launch(extra: &std::collections::BTreeMap<String, PathBuf>) -> AppResult<()> {
        let dir = extra.get(OWML_KEY).filter(|d| d.join("OWML.Launcher.exe").is_file()).ok_or_else(|| {
            AppError::InvalidArgument(
                crate::i18n::pick(
                    "OWML n'est pas installé : installe d'abord un mod (ou OWML) depuis cette page.",
                    "OWML isn't installed: first install a mod (or OWML) from this page.",
                )
                .into(),
            )
        })?;
        let mut cmd = if cfg!(windows) {
            std::process::Command::new(dir.join("OWML.Launcher.exe"))
        } else {
            let mut c = std::process::Command::new(std::env::var("MONO_BINARY").unwrap_or_else(|_| "mono".into()));
            c.arg("OWML.Launcher.exe");
            c
        };
        cmd.current_dir(dir).spawn().map(|_| ()).map_err(|e| {
            if cfg!(windows) {
                AppError::Io(crate::tr!("impossible de lancer OWML : {e}", "couldn't start OWML: {e}"))
            } else {
                AppError::Io(crate::tr!(
                    "impossible de lancer OWML avec Mono ({e}). Installe Mono (paquet « mono »), puis réessaie.",
                    "couldn't start OWML with Mono ({e}). Install Mono (the \"mono\" package), then try again."
                ))
            }
        })
    }
}
