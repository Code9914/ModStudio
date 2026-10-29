//! Réglages persistants (clé API, chemin d'API, filtres).
//!
//! v0.1 : stockés dans `settings.json` du dossier de configuration de l'appli.
//! La clé API mod.io est en lecture seule ; le jeton OAuth de la v0.2 ira,
//! lui, dans le trousseau du système.

use std::fs;
use std::path::Path;

use reqwest::Url;
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

pub const DEFAULT_API_PATH: &str = "https://api.mod.io/v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub api_key: String,
    #[serde(default = "default_api_path")]
    pub api_path: String,
    #[serde(default)]
    pub hide_mature: bool,
}

fn default_api_path() -> String {
    DEFAULT_API_PATH.to_string()
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            api_path: default_api_path(),
            hide_mature: false,
        }
    }
}

/// Ce que l'interface voit des réglages : jamais la clé complète.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsView {
    pub has_api_key: bool,
    pub api_key_hint: Option<String>,
    pub api_path: String,
    pub hide_mature: bool,
}

impl From<&Settings> for SettingsView {
    fn from(s: &Settings) -> Self {
        let hint = if s.api_key.is_empty() {
            None
        } else {
            let tail: String = s
                .api_key
                .chars()
                .rev()
                .take(4)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            Some(format!("••••{tail}"))
        };
        Self {
            has_api_key: !s.api_key.is_empty(),
            api_key_hint: hint,
            api_path: s.api_path.clone(),
            hide_mature: s.hide_mature,
        }
    }
}

pub fn load(path: &Path) -> Settings {
    fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

pub fn save(path: &Path, settings: &Settings) -> AppResult<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| AppError::Storage(e.to_string()))?;
    }
    let raw =
        serde_json::to_string_pretty(settings).map_err(|e| AppError::Storage(e.to_string()))?;
    fs::write(path, raw).map_err(|e| AppError::Storage(e.to_string()))?;

    // Le fichier contient la clé API : on le rend lisible par l'utilisateur seul.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

/// Nettoie et vérifie un chemin d'API saisi par l'utilisateur.
///
/// Accepte `https://api.mod.io/v1`, `https://u-123.modapi.io/v1`,
/// `https://g-123.modapi.io/v1` et l'environnement de test `*.test.mod.io`.
/// Refuse tout autre domaine pour ne jamais envoyer la clé ailleurs.
pub fn normalize_api_path(input: &str) -> AppResult<String> {
    let trimmed = input.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return Ok(default_api_path());
    }

    let with_scheme = if trimmed.contains("://") {
        trimmed.to_string()
    } else {
        format!("https://{trimmed}")
    };

    let url = Url::parse(&with_scheme)
        .map_err(|_| AppError::InvalidApiPath(format!("« {trimmed} » n'est pas une adresse valide.")))?;

    if url.scheme() != "https" {
        return Err(AppError::InvalidApiPath(
            "l'adresse doit commencer par https://".into(),
        ));
    }

    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    let allowed = host == "api.mod.io"
        || host.ends_with(".modapi.io")
        || host.ends_with(".test.mod.io")
        || host == "api.test.mod.io";
    if !allowed {
        return Err(AppError::InvalidApiPath(format!(
            "le domaine « {host} » n'appartient pas à mod.io."
        )));
    }

    let path = url.path().trim_end_matches('/');
    let path = if path.is_empty() { "/v1" } else { path };
    Ok(format!("https://{host}{path}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_paths() {
        assert_eq!(normalize_api_path("").unwrap(), DEFAULT_API_PATH);
        assert_eq!(
            normalize_api_path(" https://u-42.modapi.io/v1/ ").unwrap(),
            "https://u-42.modapi.io/v1"
        );
        assert_eq!(
            normalize_api_path("u-42.modapi.io").unwrap(),
            "https://u-42.modapi.io/v1"
        );
        assert_eq!(
            normalize_api_path("https://g-7.test.mod.io/v1").unwrap(),
            "https://g-7.test.mod.io/v1"
        );
    }

    #[test]
    fn rejects_foreign_or_insecure_hosts() {
        assert!(normalize_api_path("https://evil.example.com/v1").is_err());
        assert!(normalize_api_path("http://api.mod.io/v1").is_err());
        assert!(normalize_api_path("https://mod.io.evil.com/v1").is_err());
    }

    #[test]
    fn view_never_exposes_full_key() {
        let s = Settings {
            api_key: "0123456789abcdef".into(),
            ..Default::default()
        };
        let v = SettingsView::from(&s);
        assert_eq!(v.api_key_hint.as_deref(), Some("••••cdef"));
    }
}
