//! API publique de Thunderstore (sans compte ni clé) : jeux (« communautés »),
//! mods, fiches, versions, README.

use std::time::Duration;

use reqwest::{redirect, Client, StatusCode, Url};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

pub const API_BASE: &str = "https://thunderstore.io";
const USER_AGENT: &str = concat!("ModStudio/", env!("CARGO_PKG_VERSION"), " (client de mods non officiel)");

/// Un jeu sur Thunderstore.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all(serialize = "camelCase"))]
pub struct Community {
    /// Identifiant stable du jeu dans ModStudio (voir `stable_id`).
    #[serde(default, skip_deserializing)]
    pub id: i64,
    /// Chargeur de mods du jeu (`BepInEx`, `MelonLoader`, `Shimloader (UE4SS)`…).
    #[serde(default, skip_deserializing)]
    pub loader: Option<String>,
    /// ModStudio sait installer ses mods.
    #[serde(default, skip_deserializing)]
    pub supported: bool,
    pub identifier: String,
    pub name: String,
    #[serde(default)]
    pub cover_image_url: Option<String>,
    #[serde(default)]
    pub hero_image_url: Option<String>,
    #[serde(default)]
    pub community_icon_url: Option<String>,
    #[serde(default)]
    pub total_download_count: i64,
    #[serde(default)]
    pub total_package_count: i64,
    #[serde(default = "listed", skip_serializing)]
    pub is_listed: bool,
}

fn listed() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Category {
    pub name: String,
    #[serde(default)]
    pub slug: String,
}

/// Un mod dans une liste.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all(serialize = "camelCase"))]
pub struct Package {
    /// Identifiant stable pour les listes de l'interface (voir `stable_id`).
    #[serde(default, skip_deserializing)]
    pub id: i64,
    pub namespace: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub icon_url: Option<String>,
    #[serde(default)]
    pub download_count: i64,
    #[serde(default)]
    pub rating_count: i64,
    #[serde(default)]
    pub categories: Vec<Category>,
    #[serde(default)]
    pub is_deprecated: bool,
    #[serde(default)]
    pub is_nsfw: bool,
    #[serde(default)]
    pub is_pinned: bool,
    #[serde(default)]
    pub last_updated: Option<String>,
    #[serde(default)]
    pub size: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all(serialize = "camelCase"))]
pub struct Dependency {
    #[serde(default, skip_deserializing)]
    pub id: i64,
    pub namespace: String,
    pub name: String,
    #[serde(default)]
    pub version_number: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub icon_url: Option<String>,
    #[serde(default)]
    pub is_removed: bool,
    #[serde(default)]
    pub is_unavailable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Team {
    pub name: String,
}

/// Fiche d'un mod (dernière version).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all(serialize = "camelCase"))]
pub struct PackageDetail {
    #[serde(default, skip_deserializing)]
    pub id: i64,
    pub namespace: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub icon_url: Option<String>,
    #[serde(default)]
    pub download_count: i64,
    #[serde(default)]
    pub rating_count: i64,
    #[serde(default)]
    pub categories: Vec<Category>,
    #[serde(default)]
    pub community_identifier: String,
    #[serde(default)]
    pub community_name: String,
    #[serde(default)]
    pub dependencies: Vec<Dependency>,
    #[serde(default)]
    pub latest_version_number: String,
    #[serde(default)]
    pub website_url: Option<String>,
    #[serde(default)]
    pub is_deprecated: bool,
    #[serde(default)]
    pub is_nsfw: bool,
    #[serde(default)]
    pub version_count: i64,
    #[serde(default)]
    pub version_created: Option<String>,
    #[serde(default)]
    pub package_created: Option<String>,
    #[serde(default)]
    pub size: i64,
    #[serde(default)]
    pub team: Option<Team>,
    #[serde(default)]
    pub dependant_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all(serialize = "camelCase"))]
pub struct Version {
    pub version_number: String,
    #[serde(default)]
    pub datetime_created: Option<String>,
    #[serde(default)]
    pub download_count: i64,
}

/// Fiche complète pour l'interface : versions (récentes d'abord) et README.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageView {
    pub detail: PackageDetail,
    pub versions: Vec<Version>,
    pub readme: Option<String>,
}

/// Dernière version d'un mod, pour installer ses dépendances et voir les mises à jour.
#[derive(Debug, Clone, Deserialize)]
pub struct Latest {
    pub version_number: String,
    /// `Namespace-Nom-1.2.3`.
    #[serde(default)]
    pub dependencies: Vec<String>,
    #[serde(default)]
    pub icon: Option<String>,
}

#[derive(Deserialize)]
struct LatestWrapper {
    latest: Latest,
}

#[derive(Deserialize)]
struct Listing<T> {
    #[serde(default)]
    count: i64,
    #[serde(default = "Vec::new")]
    results: Vec<T>,
}

#[derive(Deserialize)]
struct Readme {
    #[serde(default)]
    markdown: String,
}

// ---------------------------------------------------------------- Vérifications

/// Identifiant d'un jeu (`lethal-company`).
pub fn check_community(c: &str) -> AppResult<()> {
    let ok = !c.is_empty() && c.len() <= 64 && c.chars().all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-');
    if ok { Ok(()) } else { Err(AppError::InvalidArgument(format!("jeu « {c} »"))) }
}

/// Équipe ou nom d'un mod (`notnotnotswipez`, `MoreCompany`).
pub fn check_part(s: &str) -> AppResult<()> {
    let ok = !s.is_empty() && s.len() <= 128 && s.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_');
    if ok { Ok(()) } else { Err(AppError::InvalidArgument(format!("nom « {s} »"))) }
}

/// Numéro de version (`1.14.0`).
pub fn check_version(v: &str) -> AppResult<()> {
    let parts: Vec<&str> = v.split('.').collect();
    let ok = (1..=4).contains(&parts.len()) && parts.iter().all(|p| !p.is_empty() && p.len() <= 9 && p.chars().all(|c| c.is_ascii_digit()));
    if ok { Ok(()) } else { Err(AppError::InvalidArgument(format!("version « {v} »"))) }
}

/// `Namespace-Nom-1.2.3` → (équipe, nom, version).
pub fn parse_dependency(full: &str) -> Option<(String, String, String)> {
    let mut parts = full.split('-');
    let (ns, name, version) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() || check_part(ns).is_err() || check_part(name).is_err() || check_version(version).is_err() {
        return None;
    }
    Some((ns.to_string(), name.to_string(), version.to_string()))
}

/// Identifiant numérique stable d'un nom Thunderstore (jeu ou `Équipe-Nom`),
/// sur 53 bits pour rester exact en JavaScript.
pub fn stable_id(s: &str) -> i64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.to_ascii_lowercase().bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    ((h & ((1u64 << 53) - 1)) as i64).max(1)
}

/// Les téléchargements ne viennent que de Thunderstore (et de son CDN).
pub fn download_host_allowed(url: &Url, base: &str) -> bool {
    let Some(host) = url.host_str() else { return false };
    let base_host = Url::parse(base).ok().and_then(|u| u.host_str().map(str::to_string));
    (url.scheme() == "https" && (host == "thunderstore.io" || host.ends_with(".thunderstore.io")))
        || base_host.as_deref() == Some(host)
}

// ---------------------------------------------------------------- Client

pub struct TsClient {
    http: Client,
    downloads: Client,
    base: String,
}

impl TsClient {
    pub fn new(base: impl Into<String>) -> Self {
        let base = base.into().trim_end_matches('/').to_string();
        let policy_base = base.clone();
        let policy = redirect::Policy::custom(move |attempt| {
            if attempt.previous().len() > 5 {
                attempt.error("trop de redirections")
            } else if download_host_allowed(attempt.url(), &policy_base) {
                attempt.follow()
            } else {
                attempt.stop()
            }
        });
        let http = Client::builder()
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(20))
            .connect_timeout(Duration::from_secs(10))
            .build()
            .expect("client HTTP");
        let downloads = Client::builder()
            .user_agent(USER_AGENT)
            .redirect(policy)
            .connect_timeout(Duration::from_secs(15))
            .read_timeout(Duration::from_secs(60))
            .build()
            .expect("client HTTP");
        Self { http, downloads, base }
    }

    pub fn downloads(&self) -> &Client {
        &self.downloads
    }

    async fn get<T: DeserializeOwned>(&self, path: &str, query: &[(&str, String)]) -> AppResult<T> {
        let url = format!("{}{}", self.base, path);
        let response = self
            .http
            .get(&url)
            .query(query)
            .send()
            .await
            .map_err(|e| AppError::ThunderstoreNetwork(e.without_url().to_string()))?;
        let status = response.status();
        if status == StatusCode::NOT_FOUND {
            return Err(AppError::ThunderstoreNotFound);
        }
        if status == StatusCode::TOO_MANY_REQUESTS {
            let retry = response
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.trim().parse().ok())
                .unwrap_or(30);
            return Err(AppError::ThunderstoreRateLimited(retry));
        }
        if !status.is_success() {
            return Err(AppError::ThunderstoreApi {
                status: status.as_u16(),
                message: status.canonical_reason().unwrap_or("erreur").to_string(),
            });
        }
        let bytes = response.bytes().await.map_err(|e| AppError::ThunderstoreNetwork(e.without_url().to_string()))?;
        serde_json::from_slice(&bytes).map_err(|e| AppError::ThunderstoreApi { status: 200, message: format!("réponse inattendue ({e})") })
    }

    /// Ce que chaque jeu demande pour charger ses mods (schéma de l'écosystème).
    pub async fn ecosystem_raw(&self) -> AppResult<Vec<u8>> {
        let response = self
            .http
            .get(format!("{}/api/experimental/schema/dev/latest/", self.base))
            .timeout(Duration::from_secs(60))
            .send()
            .await
            .map_err(|e| AppError::ThunderstoreNetwork(e.without_url().to_string()))?;
        if !response.status().is_success() {
            return Err(AppError::ThunderstoreApi { status: response.status().as_u16(), message: "schéma de l'écosystème".into() });
        }
        let bytes = response.bytes().await.map_err(|e| AppError::ThunderstoreNetwork(e.without_url().to_string()))?;
        Ok(bytes.to_vec())
    }

    /// Tous les jeux listés sur Thunderstore.
    pub async fn communities(&self) -> AppResult<Vec<Community>> {
        let list: Listing<Community> = self.get("/api/cyberstorm/community/", &[]).await?;
        Ok(list
            .results
            .into_iter()
            .filter(|c| c.is_listed && check_community(&c.identifier).is_ok())
            .map(|mut c| {
                c.id = stable_id(&c.identifier);
                c
            })
            .collect())
    }

    /// Une page de mods d'un jeu. `ordering` : `most-downloaded`, `top-rated`,
    /// `newest`, `last-updated`.
    pub async fn packages(&self, community: &str, query: &str, ordering: &str, page: u32, page_size: u32) -> AppResult<(i64, Vec<Package>)> {
        check_community(community)?;
        let ordering = match ordering {
            "most-downloaded" | "top-rated" | "newest" | "last-updated" => ordering,
            other => return Err(AppError::InvalidArgument(format!("tri « {other} »"))),
        };
        let mut params = vec![
            ("page", page.max(1).to_string()),
            ("page_size", page_size.clamp(1, 100).to_string()),
            ("ordering", ordering.to_string()),
        ];
        let q: String = query.trim().chars().take(100).collect();
        if !q.is_empty() {
            params.push(("q", q));
        }
        let list: Listing<Package> = self.get(&format!("/api/cyberstorm/listing/{community}/"), &params).await?;
        let packages = list
            .results
            .into_iter()
            .map(|mut p| {
                p.id = stable_id(&format!("{}-{}", p.namespace, p.name));
                p
            })
            .collect();
        Ok((list.count, packages))
    }

    /// Fiche d'un mod, ses versions et son README.
    pub async fn package(&self, community: &str, namespace: &str, name: &str) -> AppResult<PackageView> {
        check_community(community)?;
        check_part(namespace)?;
        check_part(name)?;
        let mut detail: PackageDetail = self.get(&format!("/api/cyberstorm/listing/{community}/{namespace}/{name}/"), &[]).await?;
        detail.id = stable_id(&format!("{namespace}-{name}"));
        for d in &mut detail.dependencies {
            d.id = stable_id(&format!("{}-{}", d.namespace, d.name));
        }
        let mut versions: Vec<Version> = self
            .get(&format!("/api/cyberstorm/package/{namespace}/{name}/versions/"), &[])
            .await
            .unwrap_or_default();
        versions.sort_by(|a, b| b.datetime_created.cmp(&a.datetime_created));
        let readme = match check_version(&detail.latest_version_number) {
            Ok(()) => self
                .get::<Readme>(&format!("/api/experimental/package/{namespace}/{name}/{}/readme/", detail.latest_version_number), &[])
                .await
                .ok()
                .map(|r| r.markdown)
                .filter(|m| !m.trim().is_empty()),
            Err(_) => None,
        };
        Ok(PackageView { detail, versions, readme })
    }

    /// Dernière version d'un mod (tous jeux confondus : le nom est unique).
    pub async fn latest(&self, namespace: &str, name: &str) -> AppResult<Latest> {
        check_part(namespace)?;
        check_part(name)?;
        let w: LatestWrapper = self.get(&format!("/api/experimental/package/{namespace}/{name}/"), &[]).await?;
        Ok(w.latest)
    }

    /// Une version précise d'un mod (ses dépendances).
    pub async fn version(&self, namespace: &str, name: &str, version: &str) -> AppResult<Latest> {
        check_part(namespace)?;
        check_part(name)?;
        check_version(version)?;
        self.get(&format!("/api/experimental/package/{namespace}/{name}/{version}/"), &[]).await
    }

    /// Adresse de téléchargement d'une version.
    pub fn download_url(&self, namespace: &str, name: &str, version: &str) -> AppResult<String> {
        check_part(namespace)?;
        check_part(name)?;
        check_version(version)?;
        Ok(format!("{}/package/download/{namespace}/{name}/{version}/", self.base))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_names_and_dependencies() {
        assert!(check_community("lethal-company").is_ok());
        assert!(check_community("../x").is_err());
        assert!(check_part("notnotnotswipez").is_ok());
        assert!(check_part("More-Company").is_err());
        assert!(check_version("1.14.0").is_ok());
        assert!(check_version("1.x").is_err());
        assert_eq!(
            parse_dependency("BepInEx-BepInExPack-5.4.2100"),
            Some(("BepInEx".into(), "BepInExPack".into(), "5.4.2100".into()))
        );
        assert_eq!(parse_dependency("a-b"), None);
        assert_eq!(parse_dependency("a-b-c-1.0.0"), None);
    }

    #[test]
    fn ids_are_stable_and_safe_for_javascript() {
        let id = stable_id("notnotnotswipez-MoreCompany");
        assert_eq!(id, stable_id("NotNotNotSwipez-morecompany"), "sans tenir compte de la casse");
        assert!(id > 0 && id < (1i64 << 53));
        assert_ne!(id, stable_id("BepInEx-BepInExPack"));
    }

    #[test]
    fn downloads_stay_on_thunderstore() {
        let ok = |u: &str| download_host_allowed(&Url::parse(u).unwrap(), API_BASE);
        assert!(ok("https://thunderstore.io/package/download/a/b/1.0.0/"));
        assert!(ok("https://gcdn.thunderstore.io/live/repository/packages/a-b-1.0.0.zip"));
        assert!(!ok("http://gcdn.thunderstore.io/x.zip"));
        assert!(!ok("https://thunderstore.io.evil.example/x.zip"));
    }
}
