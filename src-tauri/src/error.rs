//! Erreurs renvoyées à l'interface.
//!
//! Chaque erreur est sérialisée en `{ kind, message, retryAfter }` pour que le
//! frontend puisse réagir selon le type (clé invalide, session expirée…).

use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("Aucune clé API n'est configurée. Ajoute ta clé dans les réglages.")]
    MissingApiKey,

    #[error("mod.io a refusé la clé API : {message}")]
    Unauthorized { message: String, error_ref: Option<i64> },

    #[error("mod.io a refusé l'accès : {message}")]
    Forbidden { message: String, error_ref: Option<i64> },

    #[error("Élément introuvable sur mod.io.")]
    NotFound,

    #[error("Trop de requêtes envoyées à mod.io. Nouvel essai possible dans {0} s.")]
    RateLimited(u64),

    #[error("mod.io a renvoyé une erreur {status} : {message}")]
    Api { status: u16, message: String, error_ref: Option<i64> },

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

    // ------------------------------------------------------------ Compte

    #[error("Connecte-toi à ton compte mod.io pour faire ça.")]
    NotLoggedIn,

    #[error("Ta session mod.io a expiré ou a été révoquée. Reconnecte-toi.")]
    SessionExpired,

    #[error("mod.io n'autorise pas la connexion par e-mail avec cette clé API ({0}). Utilise plutôt un jeton d'accès.")]
    EmailLoginUnavailable(String),

    #[error("{0}")]
    InvalidCode(String),

    #[error("mod.io a refusé ce jeton d'accès. Vérifie que tu l'as copié en entier et qu'il a les droits de lecture et d'écriture.")]
    InvalidToken,
}

impl AppError {
    fn kind(&self) -> &'static str {
        match self {
            AppError::MissingApiKey => "missing_api_key",
            AppError::Unauthorized { .. } => "unauthorized",
            AppError::Forbidden { .. } => "forbidden",
            AppError::NotFound => "not_found",
            AppError::RateLimited(_) => "rate_limited",
            AppError::Api { .. } => "api",
            AppError::Network(_) => "network",
            AppError::Decode(_) => "decode",
            AppError::InvalidApiPath(_) => "invalid_api_path",
            AppError::Storage(_) => "storage",
            AppError::InvalidArgument(_) => "invalid_argument",
            AppError::NotLoggedIn => "not_logged_in",
            AppError::SessionExpired => "session_expired",
            AppError::EmailLoginUnavailable(_) => "email_login_unavailable",
            AppError::InvalidCode(_) => "invalid_code",
            AppError::InvalidToken => "invalid_token",
        }
    }

    /// Code d'erreur mod.io (`error_ref`), quand mod.io en fournit un.
    pub fn error_ref(&self) -> Option<i64> {
        match self {
            AppError::Unauthorized { error_ref, .. }
            | AppError::Forbidden { error_ref, .. }
            | AppError::Api { error_ref, .. } => *error_ref,
            _ => None,
        }
    }

    pub fn status(&self) -> Option<u16> {
        match self {
            AppError::Unauthorized { .. } => Some(401),
            AppError::Forbidden { .. } => Some(403),
            AppError::NotFound => Some(404),
            AppError::RateLimited(_) => Some(429),
            AppError::Api { status, .. } => Some(*status),
            _ => None,
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
