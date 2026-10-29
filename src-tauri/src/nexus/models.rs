//! Objets renvoyés par Nexus Mods, et versions envoyées à l'interface.
//!
//! Comme pour mod.io, chaque champ est lu avec indulgence : une valeur
//! absente ou d'un type inattendu prend sa valeur par défaut.

use serde::de::Deserializer;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::api::models::{lenient, lenient_vec};

/// Nombre envoyé tantôt en nombre, tantôt en texte (`BigInt` GraphQL).
fn number<'de, D: Deserializer<'de>>(deserializer: D) -> Result<i64, D::Error> {
    Ok(match Value::deserialize(deserializer)? {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)).unwrap_or(0),
        Value::String(s) => s.trim().parse::<i64>().unwrap_or(0),
        _ => 0,
    })
}

/// Booléen parfois envoyé en nombre (`primary: 0/1`).
fn flag<'de, D: Deserializer<'de>>(deserializer: D) -> Result<bool, D::Error> {
    Ok(match Value::deserialize(deserializer)? {
        Value::Bool(b) => b,
        Value::Number(n) => n.as_i64().unwrap_or(0) != 0,
        _ => false,
    })
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NexusGame {
    #[serde(default, deserialize_with = "number")]
    pub id: i64,
    #[serde(default, deserialize_with = "lenient")]
    pub name: String,
    #[serde(default, deserialize_with = "lenient")]
    pub domain_name: String,
    #[serde(default, deserialize_with = "number")]
    pub mod_count: i64,
    #[serde(default, deserialize_with = "number")]
    pub download_count: i64,
    #[serde(default, deserialize_with = "lenient")]
    pub genre: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NexusUploader {
    #[serde(default, deserialize_with = "lenient")]
    pub name: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub avatar: Option<String>,
    #[serde(default, deserialize_with = "number")]
    pub member_id: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NexusGameRef {
    #[serde(default, deserialize_with = "number")]
    pub id: i64,
    #[serde(default, deserialize_with = "lenient")]
    pub domain_name: String,
    #[serde(default, deserialize_with = "lenient")]
    pub name: Option<String>,
}

/// Mod tel qu'affiché dans les listes.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NexusMod {
    #[serde(default, deserialize_with = "number")]
    pub mod_id: i64,
    #[serde(default, deserialize_with = "lenient")]
    pub uid: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub name: String,
    #[serde(default, deserialize_with = "lenient")]
    pub summary: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub author: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub adult_content: bool,
    #[serde(default, deserialize_with = "number")]
    pub endorsements: i64,
    #[serde(default, deserialize_with = "number")]
    pub downloads: i64,
    #[serde(default, deserialize_with = "lenient")]
    pub updated_at: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub created_at: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub version: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub category: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub thumbnail_url: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub picture_url: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub status: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub uploader: Option<NexusUploader>,
    #[serde(default, deserialize_with = "lenient")]
    pub game: Option<NexusGameRef>,
}

/// Mod nécessaire, tel que listé par l'auteur.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NexusRequirement {
    #[serde(default, deserialize_with = "number")]
    pub mod_id: i64,
    #[serde(default, deserialize_with = "number")]
    pub game_id: i64,
    #[serde(default, deserialize_with = "lenient")]
    pub mod_name: String,
    #[serde(default, deserialize_with = "lenient")]
    pub url: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub external_requirement: bool,
    #[serde(default, deserialize_with = "lenient")]
    pub notes: Option<String>,
}

/// Fiche complète d'un mod (description et mods nécessaires en plus).
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NexusModDetail {
    #[serde(flatten)]
    pub summary: NexusMod,
    pub description: Option<String>,
    pub requirements: Vec<NexusRequirement>,
}

/// Forme brute renvoyée par GraphQL pour `legacyMods`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawModDetail {
    #[serde(flatten)]
    pub summary: NexusMod,
    #[serde(default, deserialize_with = "lenient")]
    pub description: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub mod_requirements: RawRequirements,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawRequirements {
    #[serde(default, deserialize_with = "lenient")]
    pub nexus_requirements: RawRequirementPage,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct RawRequirementPage {
    #[serde(default, deserialize_with = "lenient_vec")]
    pub nodes: Vec<NexusRequirement>,
}

impl From<RawModDetail> for NexusModDetail {
    fn from(raw: RawModDetail) -> Self {
        Self {
            summary: raw.summary,
            description: raw.description,
            requirements: raw.mod_requirements.nexus_requirements.nodes,
        }
    }
}

/// Un fichier publié pour un mod (version principale, optionnelle, ancienne…).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NexusFile {
    #[serde(default, deserialize_with = "number")]
    pub file_id: i64,
    #[serde(default, deserialize_with = "lenient")]
    pub uid: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub name: String,
    #[serde(default, deserialize_with = "lenient")]
    pub version: Option<String>,
    /// `MAIN`, `OPTIONAL`, `UPDATE`, `MISCELLANEOUS`, `OLD_VERSION`, `ARCHIVED`, `REMOVED`.
    #[serde(default, deserialize_with = "lenient")]
    pub category: String,
    /// Date de mise en ligne (secondes Unix).
    #[serde(default, deserialize_with = "number")]
    pub date: i64,
    #[serde(default, deserialize_with = "number")]
    pub size_in_bytes: i64,
    /// Taille en Ko (quand `sizeInBytes` manque).
    #[serde(default, deserialize_with = "number")]
    pub size: i64,
    #[serde(default, deserialize_with = "lenient")]
    pub description: Option<String>,
    #[serde(default, deserialize_with = "flag")]
    pub primary: bool,
    /// Nom du fichier sur le serveur (`711 Sandbox 1.0-148-1-0-0-1721933027.zip`).
    #[serde(default, deserialize_with = "lenient")]
    pub uri: Option<String>,
}

impl NexusFile {
    pub fn bytes(&self) -> i64 {
        if self.size_in_bytes > 0 { self.size_in_bytes } else { self.size.max(0) * 1024 }
    }
}

/// Réponse de `/v1/users/validate.json` (l'e-mail et la clé ne sont pas gardés).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ValidateInfo {
    #[serde(default, deserialize_with = "number")]
    pub user_id: i64,
    #[serde(default, deserialize_with = "lenient")]
    pub name: String,
    #[serde(default, deserialize_with = "lenient")]
    pub is_premium: bool,
    #[serde(default, deserialize_with = "lenient")]
    pub is_supporter: bool,
    #[serde(default, deserialize_with = "lenient")]
    pub profile_url: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct DownloadLink {
    #[serde(rename = "URI", default, deserialize_with = "lenient")]
    pub uri: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_graphql_shapes() {
        let g: NexusGame = serde_json::from_str(
            r#"{"id":4832,"name":"BONELAB","domainName":"bonelab","modCount":138,"downloadCount":"468956","genre":null}"#,
        )
        .unwrap();
        assert_eq!((g.id, g.download_count, g.domain_name.as_str()), (4832, 468956, "bonelab"));

        let f: NexusFile = serde_json::from_str(
            r#"{"fileId":316,"uid":"20753281974588","name":"711 Sandbox 1.0 PC","version":"1.0.0","category":"MAIN",
                "date":1721933027,"sizeInBytes":"653925305","size":638599,"primary":0,"scanned":3}"#,
        )
        .unwrap();
        assert_eq!(f.bytes(), 653925305);
        assert!(!f.primary);
        let small: NexusFile = serde_json::from_str(r#"{"fileId":1,"size":2,"sizeInBytes":null}"#).unwrap();
        assert_eq!(small.bytes(), 2048);

        let raw: RawModDetail = serde_json::from_str(
            r#"{"modId":148,"name":"7-11","adultContent":false,"description":"[b]x[/b]",
                "modRequirements":{"nexusRequirements":{"nodes":[{"modId":"12","gameId":"4832","modName":"Lib","url":"https://x"}]}},
                "uploader":{"name":"BaBa","memberId":5}}"#,
        )
        .unwrap();
        let detail = NexusModDetail::from(raw);
        assert_eq!(detail.requirements[0].mod_id, 12);
        let out = serde_json::to_value(&detail).unwrap();
        assert_eq!(out["modId"], 148);
        assert_eq!(out["description"], "[b]x[/b]");
    }

    #[test]
    fn reads_rest_shapes() {
        let v: ValidateInfo =
            serde_json::from_str(r#"{"user_id":7,"key":"secret","name":"Code","is_premium":false,"is_supporter":true,"email":"a@b"}"#)
                .unwrap();
        assert_eq!((v.user_id, v.is_supporter), (7, true));
        let links: Vec<DownloadLink> =
            serde_json::from_str(r#"[{"name":"Nexus CDN","short_name":"Nexus CDN","URI":"https://cf-files.nexusmods.com/x"}]"#)
                .unwrap();
        assert_eq!(links[0].uri, "https://cf-files.nexusmods.com/x");
    }
}
