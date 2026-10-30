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

    // ------------------------------------------------------------ Installation

    #[error("Choisis d'abord le dossier où installer les mods de ce jeu.")]
    NoGameFolder,

    #[error("Ce mod est déjà en cours d'installation.")]
    InstallInProgress,

    #[error("Ce mod n'a aucun fichier publié pour PC (Windows).")]
    NoModfile,

    #[error("Installation bloquée : l'antivirus de mod.io a signalé ce fichier.")]
    VirusFlagged,

    #[error("Le fichier téléchargé est abîmé (somme de contrôle différente). Réessaie.")]
    Corrupted,

    #[error("Installation annulée.")]
    Cancelled,

    #[error("Des fichiers existent déjà à cet endroit et ModStudio ne les écrase pas : {0}")]
    Conflict(String),

    #[error("Erreur de fichier : {0}")]
    Io(String),

    #[error("Archive illisible : {0}")]
    BadArchive(String),

    #[error("Le téléchargement de ce fichier a été refusé : {0}")]
    DownloadRefused(String),

    #[error("Ce jeu demande d'être connecté à mod.io pour télécharger ses mods.")]
    LoginRequiredForDownload,

    #[error("L'éditeur de ce jeu n'autorise pas les applications tierces comme ModStudio à accéder à ses mods. Tu peux les parcourir et t'y abonner sur mod.io.")]
    GameClosed,

    #[error("ModStudio ne peut pas installer cette archive ({0}). Installe ce fichier à la main en suivant la page du mod.")]
    UnsupportedArchive(String),

    #[error("{0}")]
    UnknownLayout(String),

    #[error("{0}")]
    WrongPlatform(String),

    #[error("Ce mod s'installe dans le dossier du jeu. Indique où {0} est installé.")]
    NeedGameDir(String),

    // ------------------------------------------------------------ Outils de mods

    #[error("Impossible de récupérer {0}. Vérifie ta connexion internet puis réessaie.")]
    ToolUnavailable(String),

    #[error("{0}")]
    ToolMismatch(String),

    #[error("{0} est déjà installé dans le dossier de ce jeu.")]
    ToolPresent(String),

    // ------------------------------------------------------------ Nexus Mods

    #[error("Ajoute ta clé API Nexus Mods (dans « Compte Nexus ») pour télécharger des mods.")]
    NexusKeyMissing,

    #[error("Nexus Mods a refusé ta clé API. Vérifie que tu l'as copiée en entier, ou crée-en une nouvelle.")]
    NexusKeyInvalid,

    #[error("Sans compte Premium, Nexus Mods demande de passer par son site pour chaque téléchargement.")]
    NexusPremiumRequired,

    #[error("Le lien de téléchargement de Nexus Mods a expiré ou ne correspond pas à ton compte. Recommence depuis la page du fichier.")]
    NexusLinkExpired,

    #[error("Élément introuvable sur Nexus Mods.")]
    NexusNotFound,

    #[error("Trop de requêtes envoyées à Nexus Mods. Nouvel essai possible dans {0} s.")]
    NexusRateLimited(u64),

    #[error("Nexus Mods a renvoyé une erreur {status} : {message}")]
    NexusApi { status: u16, message: String },

    #[error("Impossible de joindre Nexus Mods. Vérifie ta connexion internet. ({0})")]
    NexusNetwork(String),

    #[error("Réponse inattendue de Nexus Mods : {0}")]
    NexusDecode(String),

    /// Refus expliqué de Nexus (recommandation impossible…).
    #[error("{0}")]
    NexusRefused(String),

    // ------------------------------------------------------------ Thunderstore
    #[error("Élément introuvable sur Thunderstore.")]
    ThunderstoreNotFound,

    #[error("Trop de requêtes envoyées à Thunderstore. Nouvel essai possible dans {0} s.")]
    ThunderstoreRateLimited(u64),

    #[error("Thunderstore a renvoyé une erreur {status} : {message}")]
    ThunderstoreApi { status: u16, message: String },

    #[error("Impossible de joindre Thunderstore. Vérifie ta connexion internet. ({0})")]
    ThunderstoreNetwork(String),
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
            AppError::NoGameFolder => "no_game_folder",
            AppError::InstallInProgress => "install_in_progress",
            AppError::NoModfile => "no_modfile",
            AppError::VirusFlagged => "virus_flagged",
            AppError::Corrupted => "corrupted",
            AppError::Cancelled => "cancelled",
            AppError::Conflict(_) => "conflict",
            AppError::Io(_) => "io",
            AppError::BadArchive(_) => "bad_archive",
            AppError::DownloadRefused(_) => "download_refused",
            AppError::LoginRequiredForDownload => "login_required_for_download",
            AppError::GameClosed => "game_closed",
            AppError::UnsupportedArchive(_) => "unsupported_archive",
            AppError::UnknownLayout(_) => "unknown_layout",
            AppError::WrongPlatform(_) => "wrong_platform",
            AppError::NeedGameDir(_) => "need_game_dir",
            AppError::ToolUnavailable(_) => "tool_unavailable",
            AppError::ToolMismatch(_) => "tool_mismatch",
            AppError::ToolPresent(_) => "tool_present",
            AppError::NexusKeyMissing => "nexus_key_missing",
            AppError::NexusKeyInvalid => "nexus_key_invalid",
            AppError::NexusPremiumRequired => "nexus_premium_required",
            AppError::NexusLinkExpired => "nexus_link_expired",
            AppError::NexusNotFound => "nexus_not_found",
            AppError::NexusRateLimited(_) => "rate_limited",
            AppError::NexusApi { .. } => "nexus_api",
            AppError::NexusNetwork(_) => "network",
            AppError::NexusDecode(_) => "decode",
            AppError::NexusRefused(_) => "nexus_refused",
            AppError::ThunderstoreNotFound => "not_found",
            AppError::ThunderstoreRateLimited(_) => "rate_limited",
            AppError::ThunderstoreApi { .. } => "thunderstore_api",
            AppError::ThunderstoreNetwork(_) => "network",
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
            AppError::Api { status, .. } | AppError::NexusApi { status, .. } | AppError::ThunderstoreApi { status, .. } => Some(*status),
            AppError::NexusNotFound | AppError::ThunderstoreNotFound => Some(404),
            AppError::NexusRateLimited(_) | AppError::ThunderstoreRateLimited(_) => Some(429),
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
            AppError::RateLimited(secs) | AppError::NexusRateLimited(secs) | AppError::ThunderstoreRateLimited(secs) => Some(*secs),
            _ => None,
        };
        s.serialize_field("retryAfter", &retry_after)?;
        s.end()
    }
}

pub type AppResult<T> = Result<T, AppError>;
