//! Mods suivis (« Track ») et recommandés (« Endorse ») sur Nexus Mods, avec
//! la clé API de l'utilisateur.

use std::collections::HashMap;

use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::browse::check_domain;
use super::models::NexusMod;
use super::NexusState;
use crate::error::{AppError, AppResult};

/// Un mod, désigné par son jeu (`skyrimspecialedition`) et son numéro.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ModRef {
    pub domain: String,
    pub mod_id: i64,
}

/// Mods suivis et avis donnés par l'utilisateur.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Social {
    pub tracked: Vec<ModRef>,
    pub endorsed: Vec<ModRef>,
    /// « Ne pas recommander » : Nexus ne demande plus.
    pub abstained: Vec<ModRef>,
}

#[derive(Deserialize)]
struct RawTracked {
    mod_id: i64,
    domain_name: String,
}

#[derive(Deserialize)]
struct RawEndorsement {
    mod_id: i64,
    domain_name: String,
    #[serde(default)]
    status: String,
}

#[derive(Deserialize)]
struct RawStatus {
    #[serde(default)]
    status: Option<String>,
}

/// Messages de refus de Nexus pour une recommandation, en clair.
fn endorse_refusal(message: &str) -> String {
    match message.trim() {
        "NOT_DOWNLOADED_MOD" => crate::tr!(
            "Nexus Mods n'accepte une recommandation qu'une fois le mod téléchargé.",
            "Nexus Mods only accepts an endorsement once you've downloaded the mod."
        ),
        "TOO_SOON_AFTER_DOWNLOAD" => crate::tr!(
            "Nexus Mods demande d'attendre un peu après le téléchargement avant de recommander un mod. Réessaie dans quelques minutes.",
            "Nexus Mods asks you to wait a bit after downloading before endorsing a mod. Try again in a few minutes."
        ),
        "IS_OWN_MOD" => crate::tr!(
            "Tu ne peux pas recommander un mod que tu as publié.",
            "You can't endorse a mod you published."
        ),
        other => crate::tr!("Nexus Mods a refusé : {other}", "Nexus Mods refused: {other}"),
    }
}

impl NexusState {
    /// Mods suivis et recommandés par l'utilisateur.
    pub async fn social(&self) -> AppResult<Social> {
        let key = self.key()?;
        let tracked: Vec<RawTracked> = self.client.rest("/user/tracked_mods.json", &key, &[]).await?;
        let endorsements: Vec<RawEndorsement> = self.client.rest("/user/endorsements.json", &key, &[]).await?;
        let mut social = Social {
            tracked: tracked.into_iter().map(|t| ModRef { domain: t.domain_name, mod_id: t.mod_id }).collect(),
            ..Default::default()
        };
        for e in endorsements {
            let r = ModRef { domain: e.domain_name, mod_id: e.mod_id };
            match e.status.as_str() {
                "Endorsed" => social.endorsed.push(r),
                "Abstained" => social.abstained.push(r),
                _ => {}
            }
        }
        Ok(social)
    }

    /// Suit un mod (ou arrête de le suivre).
    pub async fn set_tracked(&self, domain: &str, mod_id: i64, tracked: bool) -> AppResult<()> {
        check_domain(domain)?;
        if mod_id <= 0 {
            return Err(AppError::InvalidArgument(crate::tr!("identifiant {mod_id}", "ID {mod_id}")));
        }
        let key = self.key()?;
        let method = if tracked { Method::POST } else { Method::DELETE };
        let query = [("domain_name", domain.to_string())];
        let form = [("mod_id", mod_id.to_string())];
        match self.client.rest_write::<Value>(method, "/user/tracked_mods.json", &key, &query, &form).await {
            Ok(_) => Ok(()),
            // Déjà plus suivi : rien à faire.
            Err(AppError::NexusNotFound) if !tracked => Ok(()),
            Err(e) => Err(e),
        }
    }

    /// Recommande un mod, ou indique ne pas vouloir le recommander. Renvoie
    /// l'état donné par Nexus (`Endorsed`, `Abstained`).
    pub async fn endorse(&self, domain: &str, mod_id: i64, version: Option<String>, endorse: bool) -> AppResult<String> {
        check_domain(domain)?;
        if mod_id <= 0 {
            return Err(AppError::InvalidArgument(crate::tr!("identifiant {mod_id}", "ID {mod_id}")));
        }
        let key = self.key()?;
        let action = if endorse { "endorse" } else { "abstain" };
        let path = format!("/games/{domain}/mods/{mod_id}/{action}.json");
        let form = [("version", version.unwrap_or_default())];
        match self.client.rest_write::<RawStatus>(Method::POST, &path, &key, &[], &form).await {
            Ok(r) => Ok(r.status.unwrap_or_else(|| if endorse { "Endorsed" } else { "Abstained" }.to_string())),
            Err(AppError::NexusApi { status: 403, message }) => Err(AppError::NexusRefused(endorse_refusal(&message))),
            Err(e) => Err(e),
        }
    }

    /// Fiches des mods suivis (tous jeux confondus), les plus récents d'abord.
    pub async fn tracked_mods(&self) -> AppResult<Vec<NexusMod>> {
        let social = self.social().await?;
        let catalog = self.catalog().await.unwrap_or_default();
        let ids_by_domain: HashMap<&str, i64> = catalog.games.iter().map(|g| (g.domain_name.as_str(), g.id)).collect();
        let ids: Vec<(i64, i64)> = social
            .tracked
            .iter()
            .filter_map(|t| ids_by_domain.get(t.domain.as_str()).map(|g| (*g, t.mod_id)))
            .collect();
        let mut mods = self.mods_by_ids(&ids).await?;
        mods.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(mods)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::MemoryStore;
    use crate::installer::tests::tempdir;
    use crate::testutil::{fake_server, json_response};

    fn ok_key() -> String {
        json_response(200, r#"{"user_id":7,"name":"Code","is_premium":false,"is_supporter":false}"#)
    }

    #[tokio::test]
    async fn reads_tracks_and_endorses() {
        let server = fake_server(vec![
            ok_key(),
            json_response(200, r#"[{"mod_id":38,"domain_name":"moonsweepers"}]"#),
            json_response(200, r#"[{"mod_id":38,"domain_name":"moonsweepers","status":"Endorsed","version":"1.0"},{"mod_id":9,"domain_name":"moonsweepers","status":"Abstained"},{"mod_id":2,"domain_name":"bonelab","status":"Undecided"}]"#),
            json_response(201, r#"{"message":"User 7 is now Tracking Mod: 57"}"#),
            json_response(404, r#"{"message":"Not tracking"}"#),
            json_response(200, r#"{"message":"Updated Nexus endorsement status","status":"Endorsed"}"#),
            json_response(403, r#"{"message":"NOT_DOWNLOADED_MOD","status":"Error"}"#),
        ]);
        let s = NexusState::new(server.base.trim_end_matches("/v1"), Box::new(MemoryStore::default()), tempdir("social").join("n.json"));
        s.set_key("cle").await.unwrap();

        let social = s.social().await.unwrap();
        assert_eq!(social.tracked, [ModRef { domain: "moonsweepers".into(), mod_id: 38 }]);
        assert_eq!(social.endorsed.len(), 1);
        assert_eq!(social.abstained[0].mod_id, 9);

        s.set_tracked("moonsweepers", 57, true).await.unwrap();
        let req = server.request(3);
        assert!(req.line.starts_with("POST /v1/user/tracked_mods.json?domain_name=moonsweepers"), "{}", req.line);
        assert_eq!(req.body, "mod_id=57");
        assert_eq!(req.header("apikey").as_deref(), Some("cle"));
        // Déjà plus suivi : pas une erreur.
        s.set_tracked("moonsweepers", 57, false).await.unwrap();
        assert!(server.request(4).line.starts_with("DELETE "));

        assert_eq!(s.endorse("moonsweepers", 57, Some("1.0.1".into()), true).await.unwrap(), "Endorsed");
        let req = server.request(5);
        assert!(req.line.starts_with("POST /v1/games/moonsweepers/mods/57/endorse.json"), "{}", req.line);
        assert_eq!(req.body, "version=1.0.1");
        let err = s.endorse("moonsweepers", 60, None, true).await.unwrap_err();
        assert!(matches!(&err, AppError::NexusRefused(m) if m.contains("téléchargé")), "{err}");

        assert!(s.set_tracked("../users", 1, true).await.is_err(), "jeu refusé avant la requête");
    }

    #[test]
    fn explains_endorsement_refusals() {
        assert!(endorse_refusal("NOT_DOWNLOADED_MOD").contains("téléchargé"));
        assert!(endorse_refusal("IS_OWN_MOD").contains("publié"));
        assert!(endorse_refusal("Autre chose").contains("Autre chose"));
    }
}
