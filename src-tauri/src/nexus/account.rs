//! Clé API Nexus Mods : vérification, rangement dans le trousseau, état du compte.
//!
//! La clé personnelle se crée sur <https://next.nexusmods.com/settings/api-keys>.
//! Elle sert à demander les liens de téléchargement ; parcourir les mods n'en
//! a pas besoin.

use serde::Serialize;

use super::models::ValidateInfo;
use super::NexusState;
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NexusUser {
    pub member_id: i64,
    pub name: String,
    pub is_premium: bool,
    pub is_supporter: bool,
    pub profile_url: Option<String>,
}

impl From<ValidateInfo> for NexusUser {
    fn from(v: ValidateInfo) -> Self {
        Self {
            member_id: v.user_id,
            name: v.name,
            is_premium: v.is_premium,
            is_supporter: v.is_supporter,
            profile_url: v.profile_url,
        }
    }
}

#[derive(Debug, Default)]
pub struct NexusAuth {
    loaded: bool,
    key: Option<String>,
    user: Option<NexusUser>,
    persisted: bool,
    storage_warning: Option<String>,
    /// La clé enregistrée a été refusée par Nexus Mods.
    rejected: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NexusAuthView {
    pub logged_in: bool,
    pub user: Option<NexusUser>,
    pub persisted: bool,
    pub storage_warning: Option<String>,
    /// La clé enregistrée n'est plus acceptée (révoquée ?).
    pub rejected: bool,
}

impl NexusAuth {
    fn view(&self) -> NexusAuthView {
        NexusAuthView {
            logged_in: self.key.is_some() && !self.rejected,
            user: self.user.clone(),
            persisted: self.persisted,
            storage_warning: self.storage_warning.clone(),
            rejected: self.rejected,
        }
    }
}

/// Une clé API Nexus est une suite de caractères sans espace.
pub fn normalize_key(raw: &str) -> AppResult<String> {
    let key = raw.trim();
    if key.is_empty() {
        return Err(AppError::InvalidArgument("colle ta clé API Nexus Mods.".into()));
    }
    if key.len() > 1024 || key.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(AppError::InvalidArgument(
            "cette clé ne ressemble pas à une clé API Nexus Mods (elle ne doit pas contenir d'espaces).".into(),
        ));
    }
    Ok(key.to_string())
}

impl NexusState {
    fn ensure_loaded(&self) {
        let mut auth = self.auth.write().expect("compte nexus");
        if auth.loaded {
            return;
        }
        auth.loaded = true;
        match self.secrets.load() {
            Ok(Some(key)) if !key.trim().is_empty() => {
                auth.key = Some(key.trim().to_string());
                auth.persisted = true;
            }
            Ok(_) => {}
            Err(e) => auth.storage_warning = Some(format!("Trousseau du système illisible : {e}")),
        }
    }

    /// Clé API à utiliser pour une requête.
    pub(crate) fn key(&self) -> AppResult<String> {
        self.ensure_loaded();
        let auth = self.auth.read().expect("compte nexus");
        match (&auth.key, auth.rejected) {
            (Some(k), false) => Ok(k.clone()),
            (Some(_), true) => Err(AppError::NexusKeyInvalid),
            (None, _) => Err(AppError::NexusKeyMissing),
        }
    }

    /// Une requête a montré que la clé ne fonctionne plus.
    pub(crate) fn mark_rejected(&self) {
        self.auth.write().expect("compte nexus").rejected = true;
    }

    async fn validate(&self, key: &str) -> AppResult<NexusUser> {
        let info: ValidateInfo = self.client.rest("/users/validate.json", key, &[]).await?;
        Ok(info.into())
    }

    /// État du compte ; vérifie la clé enregistrée au premier appel.
    pub async fn status(&self) -> AppResult<NexusAuthView> {
        self.ensure_loaded();
        let pending = {
            let auth = self.auth.read().expect("compte nexus");
            match (&auth.key, &auth.user, auth.rejected) {
                (Some(k), None, false) => Some(k.clone()),
                _ => None,
            }
        };
        if let Some(key) = pending {
            match self.validate(&key).await {
                Ok(user) => self.auth.write().expect("compte nexus").user = Some(user),
                Err(AppError::NexusKeyInvalid) => self.mark_rejected(),
                // Hors ligne : la clé reste utilisable, on réessaiera plus tard.
                Err(_) => {}
            }
        }
        Ok(self.auth.read().expect("compte nexus").view())
    }

    /// Vérifie puis enregistre une nouvelle clé.
    pub async fn set_key(&self, raw: &str) -> AppResult<NexusAuthView> {
        self.ensure_loaded();
        let key = normalize_key(raw)?;
        let user = self.validate(&key).await?;
        let (persisted, warning) = match self.secrets.save(&key) {
            Ok(()) => (true, None),
            Err(e) => (
                false,
                Some(format!(
                    "La clé n'a pas pu être rangée dans le trousseau du système ({e}) : elle sera oubliée à la fermeture."
                )),
            ),
        };
        let mut auth = self.auth.write().expect("compte nexus");
        auth.key = Some(key);
        auth.user = Some(user);
        auth.persisted = persisted;
        auth.storage_warning = warning;
        auth.rejected = false;
        Ok(auth.view())
    }

    /// Oublie la clé (elle reste valable sur Nexus Mods tant qu'elle n'y est pas supprimée).
    pub fn forget_key(&self) -> AppResult<NexusAuthView> {
        self.ensure_loaded();
        let warning = self.secrets.clear().err().map(|e| format!("La clé n'a pas pu être retirée du trousseau : {e}"));
        let mut auth = self.auth.write().expect("compte nexus");
        *auth = NexusAuth {
            loaded: true,
            storage_warning: warning,
            ..NexusAuth::default()
        };
        Ok(auth.view())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::MemoryStore;
    use crate::installer::tests::tempdir;
    use crate::testutil::{fake_server, json_response};

    fn state(base: &str, store: MemoryStore) -> NexusState {
        NexusState::new(base.trim_end_matches("/v1"), Box::new(store), tempdir("nx-acc").join("installed-nexus.json"))
    }

    #[test]
    fn keys_are_checked_before_use() {
        assert_eq!(normalize_key("  abc==  ").unwrap(), "abc==");
        assert!(normalize_key("").is_err());
        assert!(normalize_key("a b").is_err());
    }

    #[tokio::test]
    async fn set_key_validates_and_stores_it() {
        let server = fake_server(vec![json_response(
            200,
            r#"{"user_id":7,"key":"k","name":"Code","is_premium":false,"is_supporter":false,"email":"x@y"}"#,
        )]);
        let s = state(&server.base, MemoryStore::default());
        assert!(matches!(s.key(), Err(AppError::NexusKeyMissing)));
        let view = s.set_key(" ma-cle ").await.unwrap();
        assert!(view.logged_in && view.persisted);
        assert_eq!(view.user.as_ref().unwrap().name, "Code");
        assert_eq!(s.key().unwrap(), "ma-cle");
        assert_eq!(server.request(0).header("apikey").as_deref(), Some("ma-cle"));
        assert!(!view.user.as_ref().unwrap().is_premium);

        let view = s.forget_key().unwrap();
        assert!(!view.logged_in);
        assert!(matches!(s.key(), Err(AppError::NexusKeyMissing)));
    }

    #[tokio::test]
    async fn a_revoked_key_is_reported() {
        let server = fake_server(vec![json_response(401, r#"{"message":"Please provide a valid API Key"}"#)]);
        let s = state(&server.base, MemoryStore::with(Some("vieille-cle".into())));
        let view = s.status().await.unwrap();
        assert!(view.rejected && !view.logged_in);
        assert!(matches!(s.key(), Err(AppError::NexusKeyInvalid)));
    }

    #[tokio::test]
    async fn a_refused_new_key_is_not_saved() {
        let server = fake_server(vec![json_response(401, r#"{"message":"bad"}"#)]);
        let s = state(&server.base, MemoryStore::default());
        assert!(matches!(s.set_key("fausse").await, Err(AppError::NexusKeyInvalid)));
        assert!(matches!(s.key(), Err(AppError::NexusKeyMissing)));
    }
}
