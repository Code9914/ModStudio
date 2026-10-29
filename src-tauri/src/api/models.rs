//! Objets de l'API mod.io (sous-ensemble utilisé par le client).
//!
//! Tous les champs passent par `lenient` : un champ absent, `null`, ou d'un
//! type inattendu (l'API renvoie parfois `[]` à la place d'un objet vide)
//! prend sa valeur par défaut au lieu de faire échouer toute la réponse.

use serde::de::{DeserializeOwned, Deserializer};
use serde::{Deserialize, Serialize};
use serde_json::Value;

fn lenient<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned + Default,
{
    let value = Value::deserialize(deserializer)?;
    Ok(serde_json::from_value(value).unwrap_or_default())
}

/// Réponse paginée : `{ data, result_count, result_offset, result_limit, result_total }`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(bound(deserialize = "T: DeserializeOwned"))]
pub struct Page<T> {
    #[serde(default = "Vec::new", deserialize_with = "lenient_vec")]
    pub data: Vec<T>,
    #[serde(default, deserialize_with = "lenient")]
    pub result_count: u32,
    #[serde(default, deserialize_with = "lenient")]
    pub result_offset: u32,
    #[serde(default, deserialize_with = "lenient")]
    pub result_limit: u32,
    #[serde(default, deserialize_with = "lenient")]
    pub result_total: u32,
}

/// Variante de `lenient` pour les listes : les éléments illisibles sont
/// ignorés un par un plutôt que de vider toute la liste.
fn lenient_vec<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned,
{
    let value = Value::deserialize(deserializer)?;
    Ok(match value {
        Value::Array(items) => items
            .into_iter()
            .filter_map(|item| serde_json::from_value(item).ok())
            .collect(),
        _ => Vec::new(),
    })
}

// ---------------------------------------------------------------- Images

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Logo {
    #[serde(default, deserialize_with = "lenient")]
    pub original: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub thumb_320x180: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub thumb_640x360: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub thumb_1280x720: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Icon {
    #[serde(default, deserialize_with = "lenient")]
    pub original: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub thumb_64x64: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub thumb_128x128: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub thumb_256x256: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Header {
    #[serde(default, deserialize_with = "lenient")]
    pub original: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Image {
    #[serde(default, deserialize_with = "lenient")]
    pub original: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub thumb_320x180: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub thumb_1280x720: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Avatar {
    #[serde(default, deserialize_with = "lenient")]
    pub original: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub thumb_50x50: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub thumb_100x100: Option<String>,
}

// ---------------------------------------------------------------- Jeux

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Game {
    #[serde(default, deserialize_with = "lenient")]
    pub id: i64,
    #[serde(default, deserialize_with = "lenient")]
    pub name: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub name_id: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub summary: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub profile_url: Option<String>,
    /// Guide ou wiki de modding fourni par le jeu.
    #[serde(default, deserialize_with = "lenient")]
    pub instructions_url: Option<String>,
    /// Options d'accès à l'API : bit 1 = ouvert aux applications tierces,
    /// bit 2 = téléchargements directs autorisés.
    #[serde(default, deserialize_with = "lenient")]
    pub api_access_options: Option<i64>,
    #[serde(default, deserialize_with = "lenient")]
    pub ugc_name: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub date_updated: Option<i64>,
    #[serde(default, deserialize_with = "lenient")]
    pub icon: Option<Icon>,
    #[serde(default, deserialize_with = "lenient")]
    pub logo: Option<Logo>,
    #[serde(default, deserialize_with = "lenient")]
    pub header: Option<Header>,
    #[serde(default, deserialize_with = "lenient")]
    pub stats: Option<GameStats>,
    #[serde(default = "Vec::new", deserialize_with = "lenient_vec")]
    pub tag_options: Vec<TagOption>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GameStats {
    #[serde(default, deserialize_with = "lenient")]
    pub mods_count_total: i64,
    #[serde(default, deserialize_with = "lenient")]
    pub mods_downloads_total: i64,
    #[serde(default, deserialize_with = "lenient")]
    pub mods_subscribers_total: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TagOption {
    #[serde(default, deserialize_with = "lenient")]
    pub name: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub name_localized: Option<String>,
    /// `dropdown` (un seul choix) ou `checkboxes` (plusieurs choix).
    #[serde(rename = "type", default, deserialize_with = "lenient")]
    pub kind: Option<String>,
    #[serde(default = "Vec::new", deserialize_with = "lenient_vec")]
    pub tags: Vec<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub hidden: bool,
}

// ---------------------------------------------------------------- Mods

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Mod {
    #[serde(default, deserialize_with = "lenient")]
    pub id: i64,
    #[serde(default, deserialize_with = "lenient")]
    pub game_id: i64,
    #[serde(default, deserialize_with = "lenient")]
    pub name: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub name_id: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub summary: Option<String>,
    /// HTML fourni par l'auteur du mod : à assainir avant affichage.
    #[serde(default, deserialize_with = "lenient")]
    pub description: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub description_plaintext: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub profile_url: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub homepage_url: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub date_added: Option<i64>,
    #[serde(default, deserialize_with = "lenient")]
    pub date_updated: Option<i64>,
    #[serde(default, deserialize_with = "lenient")]
    pub date_live: Option<i64>,
    #[serde(default, deserialize_with = "lenient")]
    pub maturity_option: i64,
    #[serde(default, deserialize_with = "lenient")]
    pub dependencies: bool,
    #[serde(default, deserialize_with = "lenient")]
    pub logo: Option<Logo>,
    #[serde(default, deserialize_with = "lenient")]
    pub submitted_by: Option<User>,
    #[serde(default, deserialize_with = "lenient")]
    pub media: Option<Media>,
    #[serde(default, deserialize_with = "lenient")]
    pub modfile: Option<Modfile>,
    #[serde(default = "Vec::new", deserialize_with = "lenient_vec")]
    pub tags: Vec<Tag>,
    #[serde(default, deserialize_with = "lenient")]
    pub stats: Option<ModStats>,
    #[serde(default = "Vec::new", deserialize_with = "lenient_vec")]
    pub platforms: Vec<ModPlatform>,
}

impl Mod {
    /// Fichier à installer sur PC. Pour les jeux qui publient un fichier par
    /// plateforme (PC, Quest…), `modfile` peut être vide : on se rabat alors
    /// sur le fichier en ligne pour Windows.
    pub fn pc_modfile_id(&self) -> Option<i64> {
        self.modfile.as_ref().map(|f| f.id).filter(|id| *id > 0).or_else(|| {
            self.platforms
                .iter()
                .find(|p| p.platform.as_deref().is_some_and(|n| n.eq_ignore_ascii_case(super::client::TARGET_PLATFORM)))
                .and_then(|p| p.modfile_live)
                .filter(|id| *id > 0)
        })
    }

    /// Retire les champs lourds inutiles dans une liste.
    pub fn into_summary(mut self) -> Self {
        self.description = None;
        self.description_plaintext = None;
        if let Some(media) = self.media.as_mut() {
            media.images.clear();
        }
        self
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct User {
    #[serde(default, deserialize_with = "lenient")]
    pub id: i64,
    #[serde(default, deserialize_with = "lenient")]
    pub username: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub display_name_portal: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub profile_url: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub avatar: Option<Avatar>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Media {
    #[serde(default = "Vec::new", deserialize_with = "lenient_vec")]
    pub images: Vec<Image>,
    #[serde(default = "Vec::new", deserialize_with = "lenient_vec")]
    pub youtube: Vec<String>,
    #[serde(default = "Vec::new", deserialize_with = "lenient_vec")]
    pub sketchfab: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Tag {
    #[serde(default, deserialize_with = "lenient")]
    pub name: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub name_localized: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModStats {
    #[serde(default, deserialize_with = "lenient")]
    pub popularity_rank_position: i64,
    #[serde(default, deserialize_with = "lenient")]
    pub popularity_rank_total_mods: i64,
    #[serde(default, deserialize_with = "lenient")]
    pub downloads_today: i64,
    #[serde(default, deserialize_with = "lenient")]
    pub downloads_total: i64,
    #[serde(default, deserialize_with = "lenient")]
    pub subscribers_total: i64,
    #[serde(default, deserialize_with = "lenient")]
    pub ratings_total: i64,
    #[serde(default, deserialize_with = "lenient")]
    pub ratings_positive: i64,
    #[serde(default, deserialize_with = "lenient")]
    pub ratings_negative: i64,
    #[serde(default, deserialize_with = "lenient")]
    pub ratings_percentage_positive: i64,
    #[serde(default, deserialize_with = "lenient")]
    pub ratings_display_text: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModPlatform {
    #[serde(default, deserialize_with = "lenient")]
    pub platform: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub modfile_live: Option<i64>,
}

// ---------------------------------------------------------------- Fichiers

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Modfile {
    #[serde(default, deserialize_with = "lenient")]
    pub id: i64,
    #[serde(default, deserialize_with = "lenient")]
    pub mod_id: i64,
    #[serde(default, deserialize_with = "lenient")]
    pub date_added: Option<i64>,
    /// 0 = pas encore analysé, 1 = analysé, 2 = trop gros, 3 = introuvable, 4 = erreur.
    #[serde(default, deserialize_with = "lenient")]
    pub virus_status: i64,
    /// 1 = l'antivirus a signalé le fichier.
    #[serde(default, deserialize_with = "lenient")]
    pub virus_positive: i64,
    #[serde(default, deserialize_with = "lenient")]
    pub filesize: i64,
    #[serde(default, deserialize_with = "lenient")]
    pub filesize_uncompressed: i64,
    #[serde(default, deserialize_with = "lenient")]
    pub filename: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub version: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub changelog: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub filehash: Option<FileHash>,
    /// Lien de téléchargement temporaire : utilisé par le backend seulement.
    #[serde(default, deserialize_with = "lenient", skip_serializing)]
    pub download: Option<Download>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Download {
    #[serde(default, deserialize_with = "lenient")]
    pub binary_url: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FileHash {
    #[serde(default, deserialize_with = "lenient")]
    pub md5: Option<String>,
}

// ---------------------------------------------------------------- Dépendances

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Dependency {
    #[serde(default, deserialize_with = "lenient")]
    pub mod_id: i64,
    #[serde(default, deserialize_with = "lenient")]
    pub name: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub name_id: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub dependency_depth: Option<i64>,
    #[serde(default, deserialize_with = "lenient")]
    pub logo: Option<Logo>,
}

// ---------------------------------------------------------------- Connexion

/// Réponse de `/oauth/emailexchange`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct AccessToken {
    #[serde(default, deserialize_with = "lenient")]
    pub access_token: String,
    /// Horodatage Unix d'expiration (un an par défaut).
    #[serde(default, deserialize_with = "lenient")]
    pub date_expires: Option<i64>,
}

// ---------------------------------------------------------------- Erreurs API

/// Corps d'erreur mod.io : `{ "error": { "code": 401, "error_ref": 11005, "message": "…" } }`.
#[derive(Debug, Default, Deserialize)]
pub struct ApiErrorBody {
    #[serde(default, deserialize_with = "lenient")]
    pub error: ApiErrorDetail,
}

#[derive(Debug, Default, Deserialize)]
pub struct ApiErrorDetail {
    #[serde(default, deserialize_with = "lenient")]
    pub error_ref: Option<i64>,
    #[serde(default, deserialize_with = "lenient")]
    pub message: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tolerates_nulls_and_php_empty_arrays() {
        let raw = r#"{
            "data": [
                {
                    "id": 12, "game_id": 3, "name": "Clear Skies",
                    "logo": [], "homepage_url": null, "tags": null,
                    "stats": { "downloads_total": 1500, "ratings_display_text": "Very Positive" },
                    "modfile": { "id": 9, "filesize": 2048, "filehash": { "md5": "abc" } },
                    "media": { "images": [ { "original": "https://img/1.png" }, 42 ] }
                },
                "pas un mod"
            ],
            "result_count": 1, "result_offset": 0, "result_limit": 30, "result_total": 1
        }"#;
        let page: Page<Mod> = serde_json::from_str(raw).expect("page lisible");
        assert_eq!(page.data.len(), 1, "l'élément illisible est ignoré, pas toute la page");
        let m = &page.data[0];
        assert_eq!(m.id, 12);
        assert!(m.logo.as_ref().and_then(|l| l.thumb_320x180.as_ref()).is_none());
        assert!(m.tags.is_empty());
        assert_eq!(m.stats.as_ref().unwrap().downloads_total, 1500);
        assert_eq!(m.media.as_ref().unwrap().images.len(), 1);
        assert_eq!(m.modfile.as_ref().unwrap().filehash.as_ref().unwrap().md5.as_deref(), Some("abc"));
    }

    #[test]
    fn reads_game_tag_options() {
        let raw = r#"{ "id": 5, "name": "Jeu", "tag_options": [
            { "name": "Type", "type": "checkboxes", "tags": ["Carte", "Arme"], "hidden": false, "tag_count_map": [] }
        ], "stats": { "mods_count_total": 120 } }"#;
        let g: Game = serde_json::from_str(raw).unwrap();
        assert_eq!(g.tag_options[0].kind.as_deref(), Some("checkboxes"));
        assert_eq!(g.tag_options[0].tags, vec!["Carte", "Arme"]);
        assert_eq!(g.stats.unwrap().mods_count_total, 120);
        assert_eq!(g.api_access_options, None, "absent : inconnu, pas « fermé »");

        let closed: Game = serde_json::from_str(r#"{"id":10779,"name":"Incredibox","api_access_options":2}"#).unwrap();
        assert_eq!(closed.api_access_options, Some(2));
    }
}
