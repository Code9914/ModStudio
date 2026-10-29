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

/// Accès brut à des entrées nommées du trousseau (octets).
pub trait RawBackend: Send + Sync {
    fn get(&self, account: &str) -> Result<Option<Vec<u8>>, String>;
    fn set(&self, account: &str, value: &[u8]) -> Result<(), String>;
    fn delete(&self, account: &str) -> Result<(), String>;
}

/// Trousseau du système.
pub struct KeyringBackend;

impl KeyringBackend {
    fn entry(account: &str) -> Result<keyring::Entry, String> {
        keyring::Entry::new(KEYRING_SERVICE, account).map_err(|e| e.to_string())
    }
}

impl RawBackend for KeyringBackend {
    fn get(&self, account: &str) -> Result<Option<Vec<u8>>, String> {
        match Self::entry(account)?.get_secret() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    fn set(&self, account: &str, value: &[u8]) -> Result<(), String> {
        // `set_secret` range les octets tels quels ; `set_password` les
        // convertirait en UTF-16 sous Windows, ce qui double la taille.
        Self::entry(account)?.set_secret(value).map_err(|e| e.to_string())
    }

    fn delete(&self, account: &str) -> Result<(), String> {
        match Self::entry(account)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}

/// Le Gestionnaire d'identification Windows limite chaque entrée à 2 560
/// octets, or un jeton mod.io peut dépasser cette taille. Les valeurs longues
/// sont donc découpées en morceaux, rangés dans des entrées séparées ;
/// l'entrée principale indique alors le nombre de morceaux.
pub struct ChunkedStore<B: RawBackend> {
    backend: B,
    /// Nom de l'entrée principale (`mod.io`, `nexusmods`…).
    account: String,
}

const CHUNK_SIZE: usize = 2000;
const MAX_CHUNKS: usize = 16;
const CHUNK_HEADER: &str = "modstudio-chunks:";

impl<B: RawBackend> ChunkedStore<B> {
    /// Jeton de connexion mod.io.
    pub fn new(backend: B) -> Self {
        Self::for_account(backend, KEYRING_ACCOUNT)
    }

    /// Autre secret, rangé sous son propre nom d'entrée.
    pub fn for_account(backend: B, account: &str) -> Self {
        Self {
            backend,
            account: account.to_string(),
        }
    }

    fn chunk_account(&self, index: usize) -> String {
        format!("{}.part{}", self.account, index + 1)
    }

    /// Nombre de morceaux annoncé par l'entrée principale, le cas échéant.
    fn chunk_count(head: &[u8]) -> Option<usize> {
        std::str::from_utf8(head)
            .ok()?
            .strip_prefix(CHUNK_HEADER)?
            .parse::<usize>()
            .ok()
            .filter(|n| (1..=MAX_CHUNKS).contains(n))
    }
}

impl<B: RawBackend> SecretStore for ChunkedStore<B> {
    fn load(&self) -> Result<Option<String>, String> {
        let Some(head) = self.backend.get(&self.account)? else {
            return Ok(None);
        };
        let bytes = match Self::chunk_count(&head) {
            Some(n) => {
                let mut out = Vec::new();
                for i in 0..n {
                    match self.backend.get(&self.chunk_account(i))? {
                        Some(part) => out.extend(part),
                        // Morceau manquant : la valeur est inutilisable.
                        None => return Ok(None),
                    }
                }
                out
            }
            None => head,
        };
        String::from_utf8(bytes)
            .map(Some)
            .map_err(|_| "contenu du trousseau illisible".to_string())
    }

    fn save(&self, value: &str) -> Result<(), String> {
        self.clear()?;
        let bytes = value.as_bytes();
        if bytes.len() <= CHUNK_SIZE {
            return self.backend.set(&self.account, bytes);
        }
        let parts: Vec<&[u8]> = bytes.chunks(CHUNK_SIZE).collect();
        if parts.len() > MAX_CHUNKS {
            return Err("jeton trop long pour le trousseau".into());
        }
        for (i, part) in parts.iter().enumerate() {
            self.backend.set(&self.chunk_account(i), part)?;
        }
        // L'entrée principale est écrite en dernier : tant qu'elle n'existe
        // pas, aucune valeur à moitié rangée ne peut être relue.
        self.backend
            .set(&self.account, format!("{CHUNK_HEADER}{}", parts.len()).as_bytes())
    }

    fn clear(&self) -> Result<(), String> {
        if let Some(head) = self.backend.get(&self.account)? {
            if let Some(n) = Self::chunk_count(&head) {
                for i in 0..n {
                    self.backend.delete(&self.chunk_account(i))?;
                }
            }
        }
        self.backend.delete(&self.account)
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

    /// Faux trousseau qui applique la même limite que Windows (2 560 octets).
    #[derive(Default)]
    struct WindowsLike {
        entries: Mutex<std::collections::BTreeMap<String, Vec<u8>>>,
    }

    impl RawBackend for WindowsLike {
        fn get(&self, account: &str) -> Result<Option<Vec<u8>>, String> {
            Ok(self.entries.lock().unwrap().get(account).cloned())
        }
        fn set(&self, account: &str, value: &[u8]) -> Result<(), String> {
            if value.len() > 2560 {
                return Err(format!("{} octets > 2560", value.len()));
            }
            self.entries.lock().unwrap().insert(account.to_string(), value.to_vec());
            Ok(())
        }
        fn delete(&self, account: &str) -> Result<(), String> {
            self.entries.lock().unwrap().remove(account);
            Ok(())
        }
    }

    impl WindowsLike {
        fn names(&self) -> Vec<String> {
            self.entries.lock().unwrap().keys().cloned().collect()
        }
    }

    fn long_token() -> String {
        // Un jeton mod.io réaliste : JWT de plusieurs milliers de caractères.
        let jwt = format!("eyJ0eXAiOiJKV1Qi.{}.sig", "AbC9-_".repeat(700));
        StoredToken {
            access_token: jwt,
            date_expires: Some(4_102_444_800),
            source: TokenSource::Email,
        }
        .to_json()
    }

    #[test]
    fn long_values_are_split_under_the_windows_limit() {
        let store = ChunkedStore::new(WindowsLike::default());
        let value = long_token();
        assert!(value.len() > 2560 && value.len() < 2 * 2560);
        store.save(&value).unwrap();
        assert_eq!(store.backend.names(), vec!["mod.io", "mod.io.part1", "mod.io.part2", "mod.io.part3"]);
        assert_eq!(store.load().unwrap().as_deref(), Some(value.as_str()));
        assert!(StoredToken::from_json(&store.load().unwrap().unwrap()).is_some());
    }

    #[test]
    fn short_values_use_one_entry_and_overwrites_clean_old_parts() {
        let store = ChunkedStore::new(WindowsLike::default());
        store.save(&long_token()).unwrap();
        store.save(r#"{"access_token":"court","source":"manual"}"#).unwrap();
        assert_eq!(store.backend.names(), vec!["mod.io"]);
        assert!(store.load().unwrap().unwrap().contains("court"));
        store.clear().unwrap();
        assert!(store.backend.names().is_empty());
        assert_eq!(store.load().unwrap(), None);
    }

    #[test]
    fn missing_part_reads_as_absent() {
        let store = ChunkedStore::new(WindowsLike::default());
        store.save(&long_token()).unwrap();
        store.backend.delete("mod.io.part2").unwrap();
        assert_eq!(store.load().unwrap(), None);
        // Le nettoyage supprime quand même tout ce qui reste.
        store.clear().unwrap();
        assert!(store.backend.names().is_empty());
    }

    #[test]
    fn absurdly_long_values_are_refused() {
        let store = ChunkedStore::new(WindowsLike::default());
        assert!(store.save(&"x".repeat(CHUNK_SIZE * MAX_CHUNKS + 1)).is_err());
        assert!(store.backend.names().is_empty());
    }
}
