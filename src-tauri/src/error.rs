//! Erreurs renvoyées à l'interface.
//!
//! Chaque erreur est sérialisée en `{ kind, message, retryAfter }` pour que le
//! frontend puisse réagir selon le type (clé invalide, limite de requêtes…).

use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("Aucune clé API n'est configurée. Ajoute ta clé dans les réglages.")]
    MissingApiKey,

    #[error("mod.io a refusé la clé API : {0}")]
    Unauthorized(String),

    #[error("mod.io a refusé l'accès : {0}")]
    Forbidden(String),

    #[error("Élément introuvable sur mod.io.")]
    NotFound,

    #[error("Trop de requêtes envoyées à mod.io. Nouvel essai possible dans {0} s.")]
    RateLimited(u64),

    #[error("mod.io a renvoyé une erreur {status} : {message}")]
    Api { status: u16, message: String },

    #[error("Impossible de joindre mod.io. Vérifie ta connexion internet. ({0})")]
    Network(String),

    #[error("Réponse inattendue de mod.io : {0}")]
    Decode(String),

    #[error("Chemin d'API invalide : {0}")]
    InvalidApiPath(String),

    #[error("Impossible d'enregistrer les réglages : {0}")]
    Storage(String),

    #[error("Paramètre invalide : {0}")]
    InvalidArgument(String),
}

impl AppError {
    fn kind(&self) -> &'static str {
        match self {
            AppError::MissingApiKey => "missing_api_key",
            AppError::Unauthorized(_) => "unauthorized",
            AppError::Forbidden(_) => "forbidden",
            AppError::NotFound => "not_found",
            AppError::RateLimited(_) => "rate_limited",
            AppError::Api { .. } => "api",
            AppError::Network(_) => "network",
            AppError::Decode(_) => "decode",
            AppError::InvalidApiPath(_) => "invalid_api_path",
            AppError::Storage(_) => "storage",
            AppError::InvalidArgument(_) => "invalid_argument",
        }
    }
}

impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut s = serializer.serialize_struct("AppError", 3)?;
        s.serialize_field("kind", self.kind())?;
        s.serialize_field("message", &self.to_string())?;
        let retry_after = match self {
            AppError::RateLimited(secs) => Some(*secs),
            _ => None,
        };
        s.serialize_field("retryAfter", &retry_after)?;
        s.end()
    }
}

pub type AppResult<T> = Result<T, AppError>;
