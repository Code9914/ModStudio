//! Jeton de connexion mod.io : format, expiration, et rangement sécurisé.
//!
//! Le jeton donne accès en écriture au compte (abonnements…). Il est rangé dans
//! le trousseau du système : Gestionnaire d'identification sous Windows,
//! Secret Service (GNOME Keyring, KWallet…) sous Linux, Trousseau sous macOS.

#[cfg(test)]
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

pub const KEYRING_SERVICE: &str = "fr.modstudio.app";
pub const KEYRING_ACCOUNT: &str = "mod.io";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TokenSource {
    /// Obtenu par code e-mail : l'appli peut le révoquer à la déconnexion.
    Email,
    /// Créé par l'utilisateur sur mod.io/me/access : on l'oublie seulement.
    Manual,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredToken {
    pub access_token: String,
    #[serde(default)]
    pub date_expires: Option<i64>,
    pub source: TokenSource,
}

impl StoredToken {
    pub fn is_expired(&self, now: i64) -> bool {
        matches!(self.date_expires, Some(t) if t > 0 && t <= now)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("jeton sérialisable")
    }

    pub fn from_json(raw: &str) -> Option<Self> {
        serde_json::from_str::<StoredToken>(raw)
            .ok()
            .filter(|t| !t.access_token.trim().is_empty())
    }
}

/// Endroit où le jeton est conservé entre deux lancements.
pub trait SecretStore: Send + Sync {
    fn load(&self) -> Result<Option<String>, String>;
    fn save(&self, value: &str) -> Result<(), String>;
    fn clear(&self) -> Result<(), String>;
}

/// Trousseau du système.
pub struct KeyringStore;

impl KeyringStore {
    fn entry() -> Result<keyring::Entry, String> {
        keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT).map_err(|e| e.to_string())
    }
}

impl SecretStore for KeyringStore {
    fn load(&self) -> Result<Option<String>, String> {
        match Self::entry()?.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    fn save(&self, value: &str) -> Result<(), String> {
        Self::entry()?.set_password(value).map_err(|e| e.to_string())
    }

    fn clear(&self) -> Result<(), String> {
        match Self::entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}

/// Stockage en mémoire : pour les tests.
#[cfg(test)]
#[derive(Default)]
pub struct MemoryStore {
    value: Mutex<Option<String>>,
    fail_saves: bool,
}

#[cfg(test)]
impl MemoryStore {
    pub fn with(value: Option<String>) -> Self {
        Self {
            value: Mutex::new(value),
            fail_saves: false,
        }
    }

    /// Simule un trousseau qui refuse d'enregistrer.
    pub fn failing() -> Self {
        Self {
            value: Mutex::new(None),
            fail_saves: true,
        }
    }

    pub fn get(&self) -> Option<String> {
        self.value.lock().unwrap().clone()
    }
}

#[cfg(test)]
impl SecretStore for MemoryStore {
    fn load(&self) -> Result<Option<String>, String> {
        Ok(self.get())
    }

    fn save(&self, value: &str) -> Result<(), String> {
        if self.fail_saves {
            return Err("trousseau indisponible".into());
        }
        *self.value.lock().unwrap() = Some(value.to_string());
        Ok(())
    }

    fn clear(&self) -> Result<(), String> {
        *self.value.lock().unwrap() = None;
        Ok(())
    }
}

/// Nettoie une adresse e-mail saisie.
pub fn normalize_email(input: &str) -> Option<String> {
    let email = input.trim();
    let (local, domain) = email.split_once('@')?;
    let valid = !local.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && email.len() <= 254
        && !email.chars().any(char::is_whitespace);
    valid.then(|| email.to_string())
}

/// Nettoie un code de sécurité saisi (« p8e 5t » → « P8E5T »).
pub fn normalize_code(input: &str) -> Option<String> {
    let code: String = input
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .collect::<String>()
        .to_uppercase();
    (code.len() >= 4 && code.len() <= 12 && code.chars().all(|c| c.is_ascii_alphanumeric()))
        .then_some(code)
}

/// Nettoie un jeton d'accès collé.
pub fn normalize_token(input: &str) -> Option<String> {
    let token = input.trim().trim_start_matches("Bearer ").trim();
    (token.len() >= 20 && !token.chars().any(char::is_whitespace)).then(|| token.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_roundtrip_and_expiry() {
        let t = StoredToken {
            access_token: "abc".into(),
            date_expires: Some(1000),
            source: TokenSource::Email,
        };
        let back = StoredToken::from_json(&t.to_json()).unwrap();
        assert_eq!(back.source, TokenSource::Email);
        assert!(back.is_expired(1000));
        assert!(!back.is_expired(999));
        assert!(StoredToken::from_json("pas du json").is_none());
        assert!(StoredToken::from_json(r#"{"access_token":" ","source":"manual"}"#).is_none());
        let no_expiry = StoredToken { date_expires: None, ..t };
        assert!(!no_expiry.is_expired(i64::MAX));
    }

    #[test]
    fn cleans_user_input() {
        assert_eq!(normalize_email("  moi@exemple.fr ").as_deref(), Some("moi@exemple.fr"));
        assert!(normalize_email("moi@exemple").is_none());
        assert!(normalize_email("moi exemple.fr").is_none());
        assert_eq!(normalize_code(" p8e 5t ").as_deref(), Some("P8E5T"));
        assert!(normalize_code("ab").is_none());
        assert!(normalize_code("abc!e").is_none());
        let long = "x".repeat(40);
        assert_eq!(normalize_token(&format!("Bearer {long}\n")).as_deref(), Some(long.as_str()));
        assert!(normalize_token("trop court").is_none());
    }
}
