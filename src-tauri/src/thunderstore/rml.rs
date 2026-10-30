//! Mods de ResoniteModLoader (RML) pour Resonite : la liste vérifiée par la
//! communauté (`resonite-mod-manifest`, celle de Resolute). Ils sont rangés
//! dans le dossier du jeu et enregistrés avec les mods Thunderstore de
//! Resonite. RML lui-même est chargé par BepisLoader grâce au plugin
//! « Resonite Mod Loader Loader » (Thunderstore) : pas d'option de lancement
//! à ajouter dans Steam.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::{redirect, Client, Url};
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};

use super::api::stable_id;
use super::install::{TsInstalled, TsUpdate};
use super::ThunderstoreState;
use crate::error::{AppError, AppResult};
use crate::install_commands::dependency_state;
use crate::installer::{self, DownloadRequest, GameInstall, InstallEvent, InstalledMod, Layout, Library, STAGING_DIR};
use crate::nexus::install::{owned_by_others, place_plan};
use crate::nexus::tools::sha256_file;
use crate::toggle;

/// La liste vérifiée des mods de RML.
pub const MANIFEST_URL: &str = "https://raw.githubusercontent.com/resonite-modding-group/resonite-mod-manifest/main/manifest.json";
/// Le jeu Thunderstore qui reçoit ces mods.
pub const COMMUNITY: &str = "resonite";
/// ResoniteModLoader lui-même, dans la liste.
pub const RML_ID: &str = "com.resonitemodloader.ResoniteModLoader";
/// Le plugin BepInEx (Thunderstore) qui charge RML au démarrage du jeu.
pub const LOADER_LOADER: (&str, &str) = ("DevL0rd", "ResoniteModLoaderLoader");
/// Ce qu'il faudrait dans Steam sans BepisLoader.
pub const LAUNCH_OPTION: &str = "-LoadAssembly Libraries/ResoniteModLoader.dll";
const PREFIX: &str = "rml:";
const USER_AGENT: &str = concat!("ModStudio/", env!("CARGO_PKG_VERSION"), " (+https://github.com/Code9914/ModStudio)");
/// Au plus autant de mods installés d'un coup (dépendances comprises).
const MAX_MODS: usize = 60;
/// La liste est relue au plus toutes les six heures (ou à la demande).
const CACHE_AGE: Duration = Duration::from_secs(6 * 3600);
/// Taille maximale de la liste téléchargée.
const MANIFEST_LIMIT: usize = 32 * 1024 * 1024;

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Nom enregistré d'un mod RML (`rml:<id>`), distinct des paquets Thunderstore.
pub fn record_name(id: &str) -> String {
    format!("{PREFIX}{id}")
}

/// Identifiant d'un mod RML dans la bibliothèque.
pub fn mod_id(id: &str) -> i64 {
    stable_id(&record_name(id))
}

/// `rml:<id>` → `<id>`.
pub fn rml_id_of(file_name: &str) -> Option<&str> {
    file_name.strip_prefix(PREFIX)
}

// ---------------------------------------------------------------- Liste

#[derive(Deserialize)]
struct RawManifest {
    #[serde(default)]
    objects: BTreeMap<String, RawGroup>,
}

#[derive(Deserialize)]
struct RawGroup {
    #[serde(default)]
    author: BTreeMap<String, RawAuthor>,
    #[serde(default)]
    entries: BTreeMap<String, serde_json::Value>,
}

#[derive(Deserialize, Default)]
struct RawAuthor {
    icon: Option<String>,
    url: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawEntry {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    category: String,
    source_location: Option<String>,
    website: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    flags: Vec<String>,
    #[serde(default)]
    platforms: Vec<String>,
    #[serde(default)]
    versions: BTreeMap<String, RawVersion>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawVersion {
    #[serde(default)]
    artifacts: Vec<RawArtifact>,
    release_url: Option<String>,
    changelog: Option<String>,
    #[serde(default)]
    dependencies: BTreeMap<String, RawReq>,
    #[serde(default)]
    conflicts: BTreeMap<String, RawReq>,
}

#[derive(Deserialize)]
struct RawReq {
    #[serde(default)]
    version: String,
    #[serde(default)]
    optional: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawArtifact {
    url: String,
    sha256: String,
    filename: Option<String>,
    install_location: Option<String>,
}

/// Un mod de la liste, prêt pour l'interface.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RmlMod {
    pub id: String,
    /// Identifiant dans la bibliothèque (voir [`mod_id`]).
    pub mod_id: i64,
    pub name: String,
    pub description: String,
    pub category: String,
    pub author: String,
    pub author_url: Option<String>,
    pub author_icon: Option<String>,
    pub source: Option<String>,
    pub website: Option<String>,
    pub tags: Vec<String>,
    /// Plus maintenu (souvent : intégré au jeu depuis).
    pub deprecated: bool,
    /// Signalé comme cassé : ne marche plus avec Resonite.
    pub broken: bool,
    /// Systèmes visés quand le mod ne marche pas partout (`windows`, `headless`…).
    pub platforms: Vec<String>,
    /// Peut se charger dans le jeu sur ce PC (pas seulement sur un serveur).
    pub compatible: bool,
    pub latest: String,
    /// Versions, la plus récente d'abord.
    pub versions: Vec<RmlVersion>,
}

impl RmlMod {
    pub fn version(&self, v: &str) -> Option<&RmlVersion> {
        self.versions.iter().find(|x| x.version == v)
    }

    fn newest(&self) -> &RmlVersion {
        &self.versions[0]
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RmlVersion {
    pub version: String,
    pub changelog: Option<String>,
    pub release_url: Option<String>,
    pub dependencies: Vec<RmlDep>,
    pub conflicts: Vec<RmlDep>,
    /// Où vont ses fichiers, relatifs au dossier du jeu.
    pub files: Vec<String>,
    #[serde(skip)]
    artifacts: Vec<Artifact>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RmlDep {
    pub id: String,
    pub name: String,
    /// Version demandée (`^2.4.3`, `*`…).
    pub version: String,
    pub optional: bool,
}

#[derive(Debug, Clone)]
struct Artifact {
    url: String,
    sha256: String,
    dest: String,
}

pub struct Catalog {
    pub mods: Vec<RmlMod>,
    index: HashMap<String, usize>,
}

impl Catalog {
    /// Lit la liste ; les mods ou versions mal décrits sont laissés de côté.
    pub fn parse(bytes: &[u8]) -> Option<Catalog> {
        let raw: RawManifest = serde_json::from_slice(bytes).ok()?;
        let mut mods = Vec::new();
        for group in raw.objects.into_values() {
            let (author, info) = group.author.into_iter().next().unwrap_or_default();
            for (id, value) in group.entries {
                let Ok(e) = serde_json::from_value::<RawEntry>(value) else { continue };
                if let Some(m) = entry(id, e, &author, &info) {
                    mods.push(m);
                }
            }
        }
        if mods.is_empty() {
            return None;
        }
        // Noms des mods demandés ou en conflit.
        let names: HashMap<String, String> = mods.iter().map(|m| (m.id.to_lowercase(), m.name.clone())).collect();
        for m in &mut mods {
            for v in &mut m.versions {
                for d in v.dependencies.iter_mut().chain(v.conflicts.iter_mut()) {
                    if let Some(n) = names.get(&d.id.to_lowercase()) {
                        d.name = n.clone();
                    }
                }
            }
        }
        mods.sort_by_key(|m| m.name.to_lowercase());
        let index = mods.iter().enumerate().map(|(i, m)| (m.id.to_lowercase(), i)).collect();
        Some(Catalog { mods, index })
    }

    pub fn get(&self, id: &str) -> Option<&RmlMod> {
        self.index.get(&id.to_lowercase()).map(|&i| &self.mods[i])
    }
}

fn entry(id: String, e: RawEntry, author: &str, info: &RawAuthor) -> Option<RmlMod> {
    if id.is_empty() || id.len() > 200 || id.chars().any(char::is_control) || e.name.trim().is_empty() {
        return None;
    }
    let plugin = e.flags.iter().any(|f| f == "plugin") || e.category == "Plugins";
    let mut versions: Vec<(Version, RmlVersion)> = e
        .versions
        .into_iter()
        .filter_map(|(v, raw)| {
            let parsed = Version::parse(&v).ok()?;
            let artifacts = raw.artifacts.iter().map(|a| artifact(a, plugin)).collect::<Option<Vec<_>>>()?;
            if artifacts.is_empty() {
                return None;
            }
            let reqs = |m: BTreeMap<String, RawReq>| -> Vec<RmlDep> {
                m.into_iter()
                    .map(|(id, r)| RmlDep { name: id.clone(), id, version: r.version.trim().to_string(), optional: r.optional })
                    .collect()
            };
            Some((
                parsed,
                RmlVersion {
                    version: v,
                    changelog: raw.changelog.filter(|c| !c.trim().is_empty()),
                    release_url: raw.release_url.filter(|u| web_url(u)),
                    dependencies: reqs(raw.dependencies),
                    conflicts: reqs(raw.conflicts),
                    files: artifacts.iter().map(|a| a.dest.clone()).collect(),
                    artifacts,
                },
            ))
        })
        .collect();
    if versions.is_empty() {
        return None;
    }
    versions.sort_by(|a, b| b.0.cmp(&a.0));
    let versions: Vec<RmlVersion> = versions.into_iter().map(|(_, v)| v).collect();
    Some(RmlMod {
        mod_id: mod_id(&id),
        id,
        name: e.name.trim().to_string(),
        description: e.description.trim().to_string(),
        category: if e.category.is_empty() { "Misc".into() } else { e.category },
        author: author.to_string(),
        author_url: info.url.clone().filter(|u| web_url(u)),
        author_icon: info.icon.clone().filter(|u| web_url(u)),
        source: e.source_location.filter(|u| web_url(u)),
        website: e.website.filter(|u| web_url(u)),
        tags: e.tags,
        deprecated: e.flags.iter().any(|f| f == "deprecated"),
        broken: e.flags.iter().any(|f| f == "broken"),
        compatible: compatible(&e.platforms),
        platforms: e.platforms,
        latest: versions[0].version.clone(),
        versions,
    })
}

/// Lien affichable (pages web seulement).
fn web_url(u: &str) -> bool {
    Url::parse(u).is_ok_and(|u| u.scheme() == "https")
}

/// Téléchargement autorisé : en https (et le faux serveur des tests).
fn download_allowed(url: &Url) -> bool {
    url.scheme() == "https" || (cfg!(test) && url.host_str() == Some("127.0.0.1"))
}

/// Le mod peut-il se charger dans le jeu sur ce système ?
fn compatible(platforms: &[String]) -> bool {
    platforms.is_empty()
        || platforms.iter().any(|p| match p.as_str() {
            "windows" => cfg!(windows),
            "linux" | "linux-native" | "linux-wine" => cfg!(target_os = "linux"),
            _ => false,
        })
}

/// Nom de fichier sûr sous Windows et Linux, sans chemin.
fn safe_file_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 200
        && name != "."
        && name != ".."
        && name.trim() == name
        && !name.ends_with('.')
        && !name.chars().any(|c| c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
}

/// `%20` → espace dans le nom tiré de l'adresse.
fn percent_decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = s.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

fn artifact(a: &RawArtifact, plugin: bool) -> Option<Artifact> {
    let url = Url::parse(a.url.trim()).ok()?;
    if !download_allowed(&url) {
        return None;
    }
    let sha256 = a.sha256.trim().to_ascii_lowercase();
    if sha256.len() != 64 || !sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let name = match &a.filename {
        Some(f) => f.trim().to_string(),
        None => percent_decode(url.path_segments()?.next_back()?)?,
    };
    if !safe_file_name(&name) {
        return None;
    }
    // `/` : dossier du jeu. Par défaut `rml_mods`, et `Libraries` pour RML lui-même.
    let location = a.install_location.as_deref().unwrap_or(if plugin { "/Libraries" } else { "/rml_mods" });
    let parts: Vec<&str> = location.trim().split('/').filter(|p| !p.is_empty()).collect();
    if !parts.iter().all(|p| safe_file_name(p)) {
        return None;
    }
    let dest = parts.into_iter().chain(std::iter::once(name.as_str())).collect::<Vec<_>>().join("/");
    Some(Artifact { url: url.to_string(), sha256, dest })
}

/// La version installée répond-elle à la demande (`^2.4.3`, `1.0.1`, `*`) ?
fn satisfies(installed: Option<&str>, req: &str) -> bool {
    let Some(v) = installed.and_then(|v| Version::parse(v).ok()) else { return false };
    let req = req.trim();
    if req.is_empty() || req == "*" {
        return true;
    }
    // Une demande illisible ne bloque rien.
    VersionReq::parse(req).map(|r| r.matches(&v)).unwrap_or(true)
}

fn newer(candidate: &str, installed: Option<&str>) -> bool {
    match (Version::parse(candidate), installed.map(Version::parse)) {
        (Ok(c), Some(Ok(i))) => c > i,
        (Ok(_), _) => true,
        _ => false,
    }
}

// ---------------------------------------------------------------- Source

pub struct RmlSource {
    url: String,
    cache_path: PathBuf,
    http: Client,
    downloads: Client,
    catalog: tokio::sync::Mutex<Option<Arc<Catalog>>>,
}

impl RmlSource {
    pub fn new(url: impl Into<String>, cache_path: PathBuf) -> Self {
        let http = Client::builder()
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .build()
            .expect("client HTTP");
        // Les fichiers viennent des forges des auteurs (GitHub, Codeberg…) : en https seulement.
        let policy = redirect::Policy::custom(|attempt| {
            if attempt.previous().len() > 5 {
                attempt.error("trop de redirections")
            } else if download_allowed(attempt.url()) {
                attempt.follow()
            } else {
                attempt.stop()
            }
        });
        let downloads = Client::builder()
            .user_agent(USER_AGENT)
            .redirect(policy)
            .connect_timeout(Duration::from_secs(15))
            .read_timeout(Duration::from_secs(60))
            .build()
            .expect("client HTTP");
        Self { url: url.into(), cache_path, http, downloads, catalog: tokio::sync::Mutex::new(None) }
    }

    /// La liste : en mémoire, sinon la copie récente du disque, sinon
    /// téléchargée (et la dernière copie hors ligne).
    pub async fn catalog(&self, refresh: bool) -> AppResult<Arc<Catalog>> {
        let mut slot = self.catalog.lock().await;
        if !refresh && let Some(c) = slot.as_ref() {
            return Ok(c.clone());
        }
        let from_disk = || fs::read(&self.cache_path).ok().and_then(|b| Catalog::parse(&b));
        let fresh = fs::metadata(&self.cache_path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age < CACHE_AGE);
        let cached = if fresh && !refresh { from_disk() } else { None };
        let catalog = match cached {
            Some(c) => c,
            None => match self.fetch().await {
                Ok(bytes) => match Catalog::parse(&bytes) {
                    Some(c) => {
                        let tmp = self.cache_path.with_extension("json.tmp");
                        if fs::write(&tmp, &bytes).is_ok() {
                            let _ = fs::rename(&tmp, &self.cache_path);
                        }
                        c
                    }
                    None => from_disk().ok_or_else(|| AppError::RmlUnavailable("liste illisible".into()))?,
                },
                Err(e) => from_disk().ok_or(e)?,
            },
        };
        let catalog = Arc::new(catalog);
        *slot = Some(catalog.clone());
        Ok(catalog)
    }

    async fn fetch(&self) -> AppResult<Vec<u8>> {
        let mut response = self
            .http
            .get(&self.url)
            .send()
            .await
            .map_err(|e| AppError::RmlUnavailable(e.without_url().to_string()))?;
        if !response.status().is_success() {
            return Err(AppError::RmlUnavailable(format!("erreur {}", response.status().as_u16())));
        }
        let mut out = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|e| AppError::RmlUnavailable(e.without_url().to_string()))? {
            out.extend_from_slice(&chunk);
            if out.len() > MANIFEST_LIMIT {
                return Err(AppError::RmlUnavailable("liste démesurée".into()));
            }
        }
        Ok(out)
    }

    pub fn downloads(&self) -> &Client {
        &self.downloads
    }
}

// ---------------------------------------------------------------- Installation

/// RML dans le dossier de Resonite.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RmlStatus {
    /// `Libraries/ResoniteModLoader.dll` est là.
    pub present: bool,
    /// Installé par ModStudio (sinon à la main).
    pub managed: bool,
    pub version: Option<String>,
    /// Chargé par BepisLoader (Resonite Mod Loader Loader est là).
    pub via_bepis: bool,
    /// Option de lancement Steam quand RML n'est pas chargé par BepisLoader.
    pub launch_option: &'static str,
}

/// Un mod à installer.
#[derive(Debug, Clone)]
struct Step {
    id: String,
    version: String,
}

#[derive(Default)]
struct Plan {
    order: Vec<Step>,
    /// Mod → ceux qui en ont besoin.
    parents: HashMap<i64, Vec<i64>>,
}

fn loader_loader_id() -> i64 {
    stable_id(&format!("{}-{}", LOADER_LOADER.0, LOADER_LOADER.1))
}

/// Resonite Mod Loader Loader est-il là (installé par ModStudio ou à la main) ?
fn loader_loader_present(config: &GameInstall) -> bool {
    config.mods.contains_key(&loader_loader_id())
        || config.folder.join("BepInEx").join("plugins").join(format!("{}-{}", LOADER_LOADER.0, LOADER_LOADER.1)).is_dir()
}

/// Tous les fichiers de la dernière version sont déjà là (installés à la main).
fn present_by_hand(config: &GameInstall, m: &RmlMod) -> bool {
    m.newest().files.iter().all(|f| installer::resolve_rel(&config.folder, f).is_some_and(|p| p.is_file()))
}

/// Mods RML installés : identifiant → (version, nom).
fn installed_rml(config: &GameInstall) -> Vec<(String, Option<String>, String)> {
    config
        .mods
        .values()
        .filter_map(|m| m.file_name.as_deref().and_then(rml_id_of).map(|id| (id.to_string(), m.version.clone(), m.name.clone())))
        .collect()
}

/// Ce qu'il faut installer : les mods demandés manquants (ou trop vieux),
/// RML pour tous les mods, puis le mod lui-même.
fn plan(catalog: &Catalog, config: &GameInstall, target: &RmlMod, version: &str) -> AppResult<Plan> {
    // Ce dont RML a besoin (Harmony) ne passe pas par RML.
    let rml_deps: HashSet<String> = catalog
        .get(RML_ID)
        .map(|r| r.newest().dependencies.iter().map(|d| d.id.to_lowercase()).collect())
        .unwrap_or_default();
    let mut plan = Plan::default();
    let mut visited: HashSet<String> = HashSet::from([target.id.to_lowercase()]);
    let mut stack: Vec<(String, String, bool)> = vec![(target.id.clone(), version.to_string(), false)];
    while let Some((id, ver, expanded)) = stack.pop() {
        if expanded {
            plan.order.push(Step { id, version: ver });
            continue;
        }
        let m = catalog.get(&id).ok_or_else(|| AppError::InvalidArgument(format!("« {id} » n'est plus dans la liste.")))?;
        let v = m.version(&ver).unwrap_or_else(|| m.newest());
        let mut deps: Vec<(String, String)> = v.dependencies.iter().filter(|d| !d.optional).map(|d| (d.id.clone(), d.version.clone())).collect();
        let lower = m.id.to_lowercase();
        if lower != RML_ID.to_lowercase() && !rml_deps.contains(&lower) {
            deps.push((RML_ID.into(), "*".into()));
        }
        let parent = m.mod_id;
        let mut children = Vec::new();
        for (dep, req) in deps {
            // Hors de la liste : rien à installer.
            let Some(d) = catalog.get(&dep) else { continue };
            let parents = plan.parents.entry(d.mod_id).or_default();
            if !parents.contains(&parent) {
                parents.push(parent);
            }
            if !visited.insert(d.id.to_lowercase()) {
                continue;
            }
            match config.mods.get(&d.mod_id) {
                Some(rec) if satisfies(rec.version.as_deref(), &req) => continue,
                None if present_by_hand(config, d) => continue,
                _ => {}
            }
            if visited.len() > MAX_MODS {
                return Err(AppError::InvalidArgument("ce mod demande beaucoup trop d'autres mods.".into()));
            }
            children.push((d.id.clone(), d.latest.clone()));
        }
        stack.push((id, ver, true));
        for (cid, cver) in children.into_iter().rev() {
            stack.push((cid, cver, false));
        }
    }
    Ok(plan)
}

fn conflicting(installed: Option<&str>, req: &str) -> bool {
    req.trim() == "*" || satisfies(installed, req)
}

/// Mods connus pour ne pas marcher ensemble : refusés avant tout téléchargement.
fn check_conflicts(catalog: &Catalog, config: &GameInstall, plan: &Plan) -> AppResult<()> {
    let installed = installed_rml(config);
    for step in &plan.order {
        let Some(m) = catalog.get(&step.id) else { continue };
        let Some(v) = m.version(&step.version) else { continue };
        for c in &v.conflicts {
            if let Some((_, ver, name)) = installed.iter().find(|(id, ..)| id.eq_ignore_ascii_case(&c.id))
                && conflicting(ver.as_deref(), &c.version)
            {
                return Err(AppError::InvalidArgument(format!(
                    "« {} » ne marche pas avec « {name} », déjà installé : désinstalle-le d'abord.",
                    m.name
                )));
            }
        }
        for (id, ver, name) in &installed {
            let Some(other) = catalog.get(id).and_then(|o| ver.as_deref().and_then(|v| o.version(v))) else { continue };
            if other.conflicts.iter().any(|c| c.id.eq_ignore_ascii_case(&m.id) && conflicting(Some(&step.version), &c.version)) {
                return Err(AppError::InvalidArgument(format!(
                    "« {} » ne marche pas avec « {name} », déjà installé : désinstalle-le d'abord.",
                    m.name
                )));
            }
        }
    }
    Ok(())
}

fn platforms_label(platforms: &[String]) -> String {
    let names: Vec<&str> = platforms
        .iter()
        .map(|p| match p.as_str() {
            "headless" => "les serveurs (headless)",
            "windows" => "Windows",
            "linux" | "linux-native" => "Linux",
            "linux-wine" => "Linux (Proton)",
            "android" => "Android",
            other => other,
        })
        .collect();
    names.join(", ")
}

/// Échec d'un téléchargement depuis la forge d'un auteur, expliqué.
fn download_error(e: AppError, name: &str, url: &str) -> AppError {
    let host = Url::parse(url).ok().and_then(|u| u.host_str().map(str::to_string)).unwrap_or_default();
    let detail = match e {
        AppError::Network(d) => d,
        AppError::NotFound => "fichier introuvable".into(),
        AppError::Api { status, .. } => format!("erreur {status}"),
        AppError::DownloadRefused(d) => d,
        other => return other,
    };
    AppError::RmlDownload { name: name.to_string(), host, detail }
}

struct JobGuard<'a> {
    state: &'a ThunderstoreState,
    game_id: i64,
}

impl Drop for JobGuard<'_> {
    fn drop(&mut self) {
        self.state.jobs.lock().expect("jobs").remove(&self.game_id);
    }
}

impl ThunderstoreState {
    /// RML dans le dossier de Resonite (`None` : dossier pas encore choisi).
    pub fn rml_status(&self) -> Option<RmlStatus> {
        let lib = self.library.lock().expect("library");
        let config = lib.games.get(&stable_id(COMMUNITY))?;
        let record = config.mods.get(&mod_id(RML_ID));
        Some(RmlStatus {
            present: config.folder.join("Libraries").join("ResoniteModLoader.dll").is_file(),
            managed: record.is_some(),
            version: record.and_then(|r| r.version.clone()),
            via_bepis: loader_loader_present(config),
            launch_option: LAUNCH_OPTION,
        })
    }

    /// Installe un mod de la liste, ce dont il a besoin, et RML s'il manque.
    pub async fn rml_install(
        &self,
        id: &str,
        version: Option<String>,
        other_libraries: &[Library],
        emit: Arc<dyn Fn(InstallEvent) + Send + Sync>,
    ) -> AppResult<TsInstalled> {
        let game_id = stable_id(COMMUNITY);
        let config = self.library.lock().expect("library").games.get(&game_id).cloned().ok_or(AppError::NoGameFolder)?;
        if !config.folder.is_dir() {
            return Err(AppError::Io(format!("le dossier du jeu n'existe plus : {}", config.folder.display())));
        }
        if self.jobs.lock().expect("jobs").contains_key(&game_id) {
            return Err(AppError::InstallInProgress);
        }
        emit(InstallEvent::Preparing);
        let catalog = self.rml.catalog(false).await?;
        let target = catalog
            .get(id)
            .ok_or_else(|| AppError::InvalidArgument("ce mod n'est plus dans la liste de ResoniteModLoader.".into()))?;
        let version = match version {
            Some(v) => target
                .version(&v)
                .map(|x| x.version.clone())
                .ok_or_else(|| AppError::InvalidArgument(format!("la version {v} de « {} » n'est plus dans la liste.", target.name)))?,
            None => target.latest.clone(),
        };
        if target.broken {
            return Err(AppError::InvalidArgument(format!(
                "« {} » est signalé comme cassé dans la liste de ResoniteModLoader : il ne marche plus avec Resonite.",
                target.name
            )));
        }
        if !target.compatible {
            return Err(AppError::WrongPlatform(format!(
                "« {} » est fait pour {} : il ne se charge pas dans le jeu sur ce PC.",
                target.name,
                platforms_label(&target.platforms)
            )));
        }
        let plan = plan(&catalog, &config, target, &version)?;
        check_conflicts(&catalog, &config, &plan)?;

        // RML installé par ModStudio : chargé par BepisLoader (sans option de lancement).
        let rml_ours = plan.order.iter().any(|s| s.id.eq_ignore_ascii_case(RML_ID)) || config.mods.contains_key(&mod_id(RML_ID));
        if rml_ours && !loader_loader_present(&config) {
            let (ns, name) = LOADER_LOADER;
            let fresh = !config.mods.contains_key(&loader_loader_id());
            self.install(COMMUNITY, ns, name, None, other_libraries, emit.clone()).await?;
            // Retiré avec RML : proposé quand plus rien n'en a besoin.
            if fresh {
                let mut lib = self.library.lock().expect("library");
                if let Some(m) = lib.games.get_mut(&game_id).and_then(|g| g.mods.get_mut(&loader_loader_id())) {
                    m.auto = true;
                    m.required_by = vec![mod_id(RML_ID)];
                }
                lib.save(&self.library_path)?;
            }
        }

        let cancel = Arc::new(AtomicBool::new(false));
        {
            let mut jobs = self.jobs.lock().expect("jobs");
            if jobs.contains_key(&game_id) {
                return Err(AppError::InstallInProgress);
            }
            jobs.insert(game_id, cancel.clone());
        }
        let _guard = JobGuard { state: self, game_id };
        let staging = config.folder.join(STAGING_DIR);
        let job_dir = staging.join(format!("rml-{}", target.mod_id));
        let _ = fs::remove_dir_all(&job_dir);
        let cleanup = || {
            let _ = fs::remove_dir_all(&job_dir);
            let _ = fs::remove_dir(&staging); // seulement s'il est vide
        };
        let total = plan.order.len();
        let mut names = Vec::new();
        for (i, step) in plan.order.iter().enumerate() {
            if cancel.load(Ordering::Relaxed) {
                cleanup();
                return Err(AppError::Cancelled);
            }
            let m = catalog.get(&step.id).expect("dans la liste");
            emit(InstallEvent::Step { name: m.name.clone(), index: i as u32 + 1, total: total as u32 });
            let is_target = m.mod_id == target.mod_id;
            if let Err(e) = self.rml_install_one(game_id, m, &step.version, is_target, &plan, &job_dir, other_libraries, &cancel, &emit).await {
                cleanup();
                return Err(e);
            }
            names.push(m.name.clone());
        }
        cleanup();

        // Mods déjà installés dont le nouveau mod a besoin : il compte parmi leurs parents.
        let mut lib = self.library.lock().expect("library");
        let game = lib.games.get_mut(&game_id).ok_or(AppError::NotFound)?;
        for (child, parents) in &plan.parents {
            if let Some(m) = game.mods.get_mut(child).filter(|m| m.auto) {
                for p in parents {
                    if !m.required_by.contains(p) && *p != m.mod_id {
                        m.required_by.push(*p);
                    }
                }
            }
        }
        toggle::sync_active(game);
        let out = game.clone();
        lib.save(&self.library_path)?;
        Ok(TsInstalled { game: out, installed: names, kept_loader: false })
    }

    /// Télécharge, vérifie, place et enregistre un mod.
    #[allow(clippy::too_many_arguments)]
    async fn rml_install_one(
        &self,
        game_id: i64,
        m: &RmlMod,
        version: &str,
        is_target: bool,
        plan: &Plan,
        job_dir: &Path,
        other_libraries: &[Library],
        cancel: &Arc<AtomicBool>,
        emit: &Arc<dyn Fn(InstallEvent) + Send + Sync>,
    ) -> AppResult<()> {
        let v = m.version(version).unwrap_or_else(|| m.newest());
        let (config, others, old) = {
            let lib = self.library.lock().expect("library");
            let config = lib.games.get(&game_id).cloned().ok_or(AppError::NotFound)?;
            let mut libs: Vec<&Library> = vec![&*lib];
            libs.extend(other_libraries.iter());
            let old = config.mods.get(&m.mod_id).cloned();
            (config, owned_by_others(&libs, game_id, m.mod_id), old)
        };
        if old.as_ref().is_some_and(|o| o.disabled) {
            return Err(AppError::InvalidArgument(format!("réactive « {} » avant de le mettre à jour.", m.name)));
        }
        let owners = others.get(&config.folder);
        let old_files: HashSet<&str> = old.iter().flat_map(|o| o.files.iter().map(String::as_str)).collect();
        let dir = job_dir.join(m.mod_id.to_string());
        let mut moves = Vec::new();
        let mut adopted = Vec::new();
        let mut size: i64 = 0;
        for (i, a) in v.artifacts.iter().enumerate() {
            let dest = installer::resolve_rel(&config.folder, &a.dest).ok_or_else(|| AppError::BadArchive(format!("chemin invalide : {}", a.dest)))?;
            // Déjà là, identique (installé à la main) : gardé tel quel.
            let unowned = !old_files.contains(a.dest.as_str()) && owners.is_none_or(|o| !o.contains_key(&a.dest));
            if unowned && dest.is_file() && sha256_file(&dest).is_ok_and(|h| h == a.sha256) {
                adopted.push(a.dest.clone());
                continue;
            }
            let tmp = dir.join(i.to_string());
            let progress_emit = emit.clone();
            let progress = move |received: u64, total: u64| progress_emit(InstallEvent::Downloading { received, total });
            installer::download(
                self.rml.downloads(),
                DownloadRequest { url: &a.url, token: None, dest: &tmp, expected_size: 0, expected_md5: None },
                cancel,
                &progress,
            )
            .await
            .map_err(|e| download_error(e, &m.name, &a.url))?;
            if sha256_file(&tmp).map_err(installer::io_err)? != a.sha256 {
                return Err(AppError::Corrupted);
            }
            size += fs::metadata(&tmp).map(|x| x.len() as i64).unwrap_or(0);
            moves.push((tmp, a.dest.clone()));
        }
        emit(InstallEvent::Finishing);
        let placed = if moves.is_empty() {
            Vec::new()
        } else {
            let (config_b, old_b) = (config.clone(), old.clone());
            tokio::task::spawn_blocking(move || place_plan(&config_b, old_b.as_ref(), &others, BTreeMap::from([(String::new(), moves)]), None))
                .await
                .map_err(|e| AppError::Io(e.to_string()))??
        };

        let parent = (!is_target).then(|| plan.parents.get(&m.mod_id).and_then(|p| p.first().copied())).flatten();
        let (auto, mut required_by) = match (&old, is_target) {
            (Some(o), true) => (o.auto, o.required_by.clone()),
            _ => dependency_state(parent, old.as_ref()),
        };
        if auto {
            for p in plan.parents.get(&m.mod_id).into_iter().flatten() {
                if !required_by.contains(p) {
                    required_by.push(*p);
                }
            }
        }
        let mut files: Vec<String> = placed.iter().filter(|b| b.key.is_empty()).flat_map(|b| b.files.clone()).collect();
        files.extend(adopted);
        // Dossiers partagés (`rml_mods`, `Libraries`…) : jamais retirés avec un mod.
        let dirs: Vec<String> = placed
            .iter()
            .flat_map(|b| b.dirs.clone())
            .filter(|d| d.contains('/') && (d.starts_with("rml_mods/") || d.starts_with("rml_libs/")))
            .collect();
        let record = InstalledMod {
            mod_id: m.mod_id,
            auto,
            required_by,
            name: m.name.clone(),
            logo: m.author_icon.clone(),
            version: Some(v.version.clone()),
            modfile_id: 0,
            size,
            installed_at: now(),
            layout: Layout::Flat,
            file_name: Some(record_name(&m.id)),
            files,
            dirs,
            ..Default::default()
        };
        let mut lib = self.library.lock().expect("library");
        let game = lib.games.get_mut(&game_id).ok_or(AppError::NotFound)?;
        game.mods.insert(m.mod_id, record);
        lib.save(&self.library_path)
    }

    /// Mods RML installés dont une version plus récente est dans la liste.
    pub(super) async fn rml_updates(&self, installed: &[(i64, String, Option<String>)]) -> Vec<TsUpdate> {
        let rml: Vec<_> = installed.iter().filter_map(|(mid, full, v)| rml_id_of(full).map(|id| (*mid, id, v))).collect();
        if rml.is_empty() {
            return Vec::new();
        }
        let Ok(catalog) = self.rml.catalog(false).await else { return Vec::new() };
        rml.into_iter()
            .filter_map(|(mod_id, id, v)| {
                let m = catalog.get(id)?;
                newer(&m.latest, v.as_deref()).then(|| TsUpdate { mod_id, version: m.latest.clone() })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::installer::tests::tempdir;
    use crate::testutil::{fake_server_with, json_response};
    use serde_json::json;
    use sha2::{Digest, Sha256};

    fn sha(bytes: &[u8]) -> String {
        Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
    }

    fn file_response(body: &[u8]) -> String {
        let mut s = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        s.extend(body.iter().map(|&b| b as char));
        s
    }

    const RML: &[u8] = b"rml dll";
    const HARMONY: &[u8] = b"harmony";
    const IK_OLD: &[u8] = b"ik 2.8.1";
    const IK: &[u8] = b"ik 2.8.3";

    /// Liste de test : RML (+ Harmony), IkCulling, un mod cassé, un mod de serveur, un conflit.
    fn manifest(base: &str, ik_sha: &str) -> serde_json::Value {
        json!({
            "schemaVersion": "1.0.0",
            "objects": {
                "com.resonitemodloader": {
                    "author": { "Resonite Modding Group": { "url": "https://github.com/resonite-modding-group" } },
                    "entries": {
                        "com.resonitemodloader.ResoniteModLoader": {
                            "name": "ResoniteModLoader", "description": "A mod loader for Resonite", "category": "Plugins", "flags": ["plugin"],
                            "versions": { "5.0.2": {
                                "artifacts": [{ "url": format!("{base}/rml/ResoniteModLoader.dll"), "sha256": sha(RML) }],
                                "dependencies": { "net.pardeike.harmony": { "version": "^2.4.3" } }
                            } }
                        }
                    }
                },
                "net.pardeike": {
                    "author": { "pardeike": {} },
                    "entries": { "net.pardeike.harmony": {
                        "name": "Harmony", "category": "Libraries", "description": "patching",
                        "versions": { "2.4.3": { "artifacts": [{ "url": format!("{base}/rml/0Harmony.dll"), "sha256": sha(HARMONY), "filename": "0Harmony.dll", "installLocation": "/rml_libs" }] } }
                    } }
                },
                "net.raidriar796": {
                    "author": { "Raidriar": { "icon": "https://codeberg.org/avatars/x" } },
                    "entries": {
                        "net.raidriar796.IkCulling": {
                            "name": "IkCulling", "category": "Optimization", "description": "Disables the IK of Users who are behind you or far away.",
                            "versions": {
                                "2.8.1": { "artifacts": [{ "url": format!("{base}/ik/2.8.1/IkCulling.dll"), "sha256": sha(IK_OLD) }] },
                                "2.8.3": { "artifacts": [{ "url": format!("{base}/ik/2.8.3/IkCulling.dll"), "sha256": ik_sha }], "changelog": "fix" }
                            }
                        },
                        "net.raidriar796.HeadlessUserCulling": {
                            "name": "HeadlessUserCulling", "category": "Optimization", "description": "server", "platforms": ["headless"],
                            "versions": { "1.1.3": { "artifacts": [{ "url": format!("{base}/h.dll"), "sha256": sha(b"h") }] } }
                        },
                        "net.raidriar796.Old": {
                            "name": "Old", "category": "Misc", "description": "old", "flags": ["broken"],
                            "versions": { "1.0.0": { "artifacts": [{ "url": format!("{base}/o.dll"), "sha256": sha(b"o") }] } }
                        },
                        "net.raidriar796.Rival": {
                            "name": "Rival", "category": "Misc", "description": "conflicts with IkCulling",
                            "versions": { "1.0.0": {
                                "artifacts": [{ "url": format!("{base}/r.dll"), "sha256": sha(b"r") }],
                                "conflicts": { "net.raidriar796.IkCulling": { "version": "*" } }
                            } }
                        }
                    }
                }
            }
        })
    }

    fn state_with(base: &str, tag: &str) -> ThunderstoreState {
        let mut s = super::super::install::tests::state_for(base, tag);
        s.rml = RmlSource::new(format!("{base}/manifest.json"), s.library_path.with_file_name("rml-manifest.json"));
        s
    }

    fn resonite(tag: &str) -> PathBuf {
        let game = tempdir(tag);
        fs::write(game.join("Resonite.exe"), b"exe").unwrap();
        fs::write(game.join("BepisLoader.dll"), b"loader").unwrap();
        fs::write(game.join("hostfxr.dll"), b"hook").unwrap();
        game
    }

    fn quiet() -> Arc<dyn Fn(InstallEvent) + Send + Sync> {
        Arc::new(|_| {})
    }

    #[test]
    fn the_list_keeps_only_files_it_can_place_safely() {
        let base = "https://example.org";
        let mut m = manifest(base, &sha(IK));
        let entries = &mut m["objects"]["net.raidriar796"]["entries"];
        entries["net.raidriar796.Bad"] = json!({
            "name": "Bad", "description": "", "category": "Misc",
            "versions": {
                "1.0.0": { "artifacts": [{ "url": "http://example.org/a.dll", "sha256": sha(b"a") }] },
                "1.1.0": { "artifacts": [{ "url": "https://example.org/a.dll", "sha256": "123" }] },
                "1.2.0": { "artifacts": [{ "url": "https://example.org/a.dll", "sha256": sha(b"a"), "installLocation": "/rml_mods/../.." }] },
                "1.3.0": { "artifacts": [{ "url": "https://example.org/a.dll", "sha256": sha(b"a"), "filename": "..\\evil.dll" }] }
            }
        });
        entries["net.raidriar796.Spaces"] = json!({
            "name": "Spaces", "description": "", "category": "Misc",
            "versions": { "1.0.0": { "artifacts": [{ "url": "https://example.org/My%20Mod.dll", "sha256": sha(b"s") }] } }
        });
        let catalog = Catalog::parse(serde_json::to_vec(&m).unwrap().as_slice()).unwrap();
        assert!(catalog.get("net.raidriar796.Bad").is_none(), "aucune version sûre");
        assert_eq!(catalog.get("net.raidriar796.Spaces").unwrap().versions[0].files, ["rml_mods/My Mod.dll"]);
        let rml = catalog.get("com.resonitemodloader.resonitemodloader").unwrap();
        assert_eq!(rml.versions[0].files, ["Libraries/ResoniteModLoader.dll"]);
        assert_eq!(rml.versions[0].dependencies[0].name, "Harmony");
        let ik = catalog.get("net.raidriar796.IkCulling").unwrap();
        assert_eq!(ik.latest, "2.8.3");
        assert_eq!(ik.versions.iter().map(|v| v.version.as_str()).collect::<Vec<_>>(), ["2.8.3", "2.8.1"]);
        assert_eq!(ik.author, "Raidriar");
        assert!(!catalog.get("net.raidriar796.HeadlessUserCulling").unwrap().compatible);
        assert!(satisfies(Some("2.4.3"), "^2.4.3") && !satisfies(Some("2.4.2"), "^2.4.3") && satisfies(Some("1.0.0"), "*"));
        assert!(!satisfies(None, "*"));
        assert!(newer("2.8.3", Some("2.8.1")) && !newer("2.8.1", Some("2.8.3")));
    }

    #[tokio::test]
    async fn installs_rml_harmony_then_the_mod_where_rml_reads_them() {
        let game = resonite("rml-game");
        fs::create_dir_all(game.join("BepInEx/plugins/DevL0rd-ResoniteModLoaderLoader")).unwrap();
        let server = fake_server_with(|base| {
            vec![
                json_response(200, &manifest(base.trim_end_matches("/v1"), &sha(IK)).to_string()),
                file_response(HARMONY),
                file_response(RML),
                file_response(IK_OLD),
            ]
        });
        let s = state_with(server.base.trim_end_matches("/v1"), "rml-state");
        let g = s.configure(COMMUNITY, "Resonite".into(), None, game.to_str().unwrap()).unwrap();
        let out = s.rml_install("net.raidriar796.IkCulling", Some("2.8.1".into()), &[], quiet()).await.unwrap();
        assert_eq!(out.installed, ["Harmony", "ResoniteModLoader", "IkCulling"]);
        assert_eq!(fs::read(game.join("rml_libs/0Harmony.dll")).unwrap(), HARMONY);
        assert_eq!(fs::read(game.join("Libraries/ResoniteModLoader.dll")).unwrap(), RML);
        assert_eq!(fs::read(game.join("rml_mods/IkCulling.dll")).unwrap(), IK_OLD);
        assert!(!game.join(STAGING_DIR).exists());

        let ik = &out.game.mods[&mod_id("net.raidriar796.IkCulling")];
        assert!(!ik.auto && ik.version.as_deref() == Some("2.8.1") && ik.file_name.as_deref() == Some("rml:net.raidriar796.IkCulling"));
        let rml = &out.game.mods[&mod_id(RML_ID)];
        assert!(rml.auto && rml.required_by == [ik.mod_id]);
        let harmony = &out.game.mods[&mod_id("net.pardeike.harmony")];
        assert!(harmony.auto && harmony.required_by == [rml.mod_id]);
        let status = s.rml_status().unwrap();
        assert!(status.present && status.managed && status.via_bepis);

        // Une version plus récente est dans la liste.
        let installed: Vec<_> = out.game.mods.values().map(|m| (m.mod_id, m.file_name.clone().unwrap(), m.version.clone())).collect();
        let updates = s.rml_updates(&installed).await;
        assert_eq!(updates.len(), 1);
        assert_eq!((updates[0].mod_id, updates[0].version.as_str()), (ik.mod_id, "2.8.3"));

        // Désinstallé : RML n'est plus utile.
        let removed = s.remove(g.game_id, ik.mod_id).unwrap();
        assert_eq!(removed.orphans.iter().map(|o| o.name.as_str()).collect::<Vec<_>>(), ["ResoniteModLoader"]);
        assert!(!game.join("rml_mods/IkCulling.dll").exists());
    }

    #[tokio::test]
    async fn rml_installed_by_hand_is_kept_and_identical_files_are_adopted() {
        let game = resonite("rml-hand");
        fs::create_dir_all(game.join("Libraries")).unwrap();
        fs::write(game.join("Libraries/ResoniteModLoader.dll"), b"rml of the user").unwrap();
        fs::create_dir_all(game.join("rml_libs")).unwrap();
        fs::write(game.join("rml_libs/0Harmony.dll"), b"harmony of the user").unwrap();
        fs::create_dir_all(game.join("rml_mods")).unwrap();
        fs::write(game.join("rml_mods/IkCulling.dll"), IK).unwrap();
        let server = fake_server_with(|base| vec![json_response(200, &manifest(base.trim_end_matches("/v1"), &sha(IK)).to_string())]);
        let s = state_with(server.base.trim_end_matches("/v1"), "rml-hand-state");
        s.configure(COMMUNITY, "Resonite".into(), None, game.to_str().unwrap()).unwrap();
        let out = s.rml_install("net.raidriar796.IkCulling", None, &[], quiet()).await.unwrap();
        assert_eq!(out.installed, ["IkCulling"], "RML et Harmony, installés à la main, ne sont pas touchés");
        assert_eq!(server.count(), 1, "le mod identique déjà là n'est pas retéléchargé");
        assert_eq!(fs::read(game.join("Libraries/ResoniteModLoader.dll")).unwrap(), b"rml of the user");
        assert_eq!(out.game.mods[&mod_id("net.raidriar796.IkCulling")].files, ["rml_mods/IkCulling.dll"]);
        let status = s.rml_status().unwrap();
        assert!(status.present && !status.managed && !status.via_bepis);
    }

    #[tokio::test]
    async fn files_must_match_the_list() {
        let game = resonite("rml-sha");
        fs::create_dir_all(game.join("Libraries")).unwrap();
        fs::write(game.join("Libraries/ResoniteModLoader.dll"), b"rml").unwrap();
        let server = fake_server_with(|base| {
            vec![json_response(200, &manifest(base.trim_end_matches("/v1"), &sha(IK)).to_string()), file_response(b"tampered")]
        });
        let s = state_with(server.base.trim_end_matches("/v1"), "rml-sha-state");
        s.configure(COMMUNITY, "Resonite".into(), None, game.to_str().unwrap()).unwrap();
        let err = s.rml_install("net.raidriar796.IkCulling", None, &[], quiet()).await.unwrap_err();
        assert!(matches!(err, AppError::Corrupted), "{err:?}");
        assert!(!game.join("rml_mods/IkCulling.dll").exists());
        assert!(!game.join(STAGING_DIR).exists());
    }

    #[tokio::test]
    async fn broken_server_only_and_conflicting_mods_are_refused_before_any_download() {
        let game = resonite("rml-refuse");
        fs::create_dir_all(game.join("Libraries")).unwrap();
        fs::write(game.join("Libraries/ResoniteModLoader.dll"), b"rml").unwrap();
        let server = fake_server_with(|base| {
            vec![json_response(200, &manifest(base.trim_end_matches("/v1"), &sha(IK)).to_string()), file_response(IK)]
        });
        let s = state_with(server.base.trim_end_matches("/v1"), "rml-refuse-state");
        s.configure(COMMUNITY, "Resonite".into(), None, game.to_str().unwrap()).unwrap();
        let err = s.rml_install("net.raidriar796.Old", None, &[], quiet()).await.unwrap_err();
        assert!(err.to_string().contains("cassé"), "{err}");
        let err = s.rml_install("net.raidriar796.HeadlessUserCulling", None, &[], quiet()).await.unwrap_err();
        assert!(matches!(err, AppError::WrongPlatform(ref m) if m.contains("serveurs")), "{err:?}");
        s.rml_install("net.raidriar796.IkCulling", None, &[], quiet()).await.unwrap();
        let err = s.rml_install("net.raidriar796.Rival", None, &[], quiet()).await.unwrap_err();
        assert!(err.to_string().contains("ne marche pas avec « IkCulling »"), "{err}");
        assert_eq!(server.count(), 2, "liste + IkCulling seulement");
    }

    #[tokio::test]
    async fn rml_brings_the_bepinex_plugin_that_loads_it() {
        let game = resonite("rml-rmll");
        let rmll = crate::installer::tests::make_zip(&[("plugins/ResoniteModLoaderLoader/ResoniteModLoaderLoader.dll", b"rmll"), ("manifest.json", b"{}")]);
        let wrapper = crate::installer::tests::make_zip(&[("plugins/BepisResoniteWrapper/BepisResoniteWrapper.dll", b"w"), ("manifest.json", b"{}")]);
        let server = fake_server_with(move |base| {
            let latest = |v: &str, deps: &[&str]| {
                let deps: Vec<String> = deps.iter().map(|d| format!("\"{d}\"")).collect();
                json_response(200, &format!(r#"{{"latest":{{"version_number":"{v}","dependencies":[{}],"icon":null}}}}"#, deps.join(",")))
            };
            vec![
                json_response(200, &manifest(base.trim_end_matches("/v1"), &sha(IK)).to_string()),
                latest("0.1.0", &["ResoniteModding-BepisResoniteWrapper-1.0.2", "ResoniteModding-BepisLoader-1.6.0"]),
                latest("1.0.2", &[]),
                file_response(&wrapper),
                file_response(&rmll),
                file_response(HARMONY),
                file_response(RML),
                file_response(IK),
            ]
        });
        let s = state_with(server.base.trim_end_matches("/v1"), "rml-rmll-state");
        s.configure(COMMUNITY, "Resonite".into(), None, game.to_str().unwrap()).unwrap();
        let out = s.rml_install("net.raidriar796.IkCulling", None, &[], quiet()).await.unwrap();
        assert!(game.join("BepInEx/plugins/DevL0rd-ResoniteModLoaderLoader/ResoniteModLoaderLoader/ResoniteModLoaderLoader.dll").is_file());
        assert!(game.join("rml_mods/IkCulling.dll").is_file());
        let rmll = &out.game.mods[&loader_loader_id()];
        assert!(rmll.auto && rmll.required_by == [mod_id(RML_ID)]);
        assert!(s.rml_status().unwrap().via_bepis);
    }
}
