//! Erreurs renvoyées à l'interface.
//!
//! Chaque erreur est sérialisée en `{ kind, message, retryAfter }` pour que le
//! frontend puisse réagir selon le type (clé invalide, session expirée…).

use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};

/// Les messages (dans la langue de l'interface) sont dans `impl Display` plus bas.
#[derive(Debug)]
pub enum AppError {
    MissingApiKey,
    Unauthorized { message: String, error_ref: Option<i64> },
    Forbidden { message: String, error_ref: Option<i64> },
    NotFound,
    RateLimited(u64),
    Api { status: u16, message: String, error_ref: Option<i64> },
    Network(String),
    Decode(String),
    InvalidApiPath(String),
    Storage(String),
    InvalidArgument(String),

    // ------------------------------------------------------------ Compte
    NotLoggedIn,
    SessionExpired,
    EmailLoginUnavailable(String),
    InvalidCode(String),
    InvalidToken,

    // ------------------------------------------------------------ Installation
    NoGameFolder,
    InstallInProgress,
    NoModfile,
    VirusFlagged,
    Corrupted,
    Cancelled,
    /// Fichiers déjà là ; les mods qu'on peut désactiver pour faire de la place
    /// (vide : rien à proposer, un fichier n'appartient à aucun mod).
    Conflict(String, Vec<crate::installer::Owner>),
    Io(String),
    BadArchive(String),
    DownloadRefused(String),
    LoginRequiredForDownload,
    GameClosed,
    UnsupportedArchive(String),
    UnknownLayout(String),
    WrongPlatform(String),
    NeedGameDir(String),

    // ------------------------------------------------------------ Outils de mods
    ToolUnavailable(String),
    ToolMismatch(String),
    ToolPresent(String),

    // ------------------------------------------------------------ Nexus Mods
    NexusKeyMissing,
    NexusKeyInvalid,
    NexusPremiumRequired,
    NexusLinkExpired,
    NexusNotFound,
    NexusRateLimited(u64),
    NexusApi { status: u16, message: String },
    NexusNetwork(String),
    NexusDecode(String),
    /// Refus expliqué de Nexus (recommandation impossible…).
    NexusRefused(String),

    // ------------------------------------------------------------ Thunderstore
    ThunderstoreNotFound,
    ThunderstoreRateLimited(u64),
    ThunderstoreApi { status: u16, message: String },
    ThunderstoreNetwork(String),
    /// Jeu dont le chargeur de mods n'est pas encore pris en charge.
    ThunderstoreUnsupported(String),

    // ------------------------------------------------------------ ResoniteModLoader
    RmlUnavailable(String),
    RmlDownload { name: String, host: String, detail: String },

    // ------------------------------------------------------------ Listes des communautés
    ListUnavailable { list: String, detail: String },
    ListDownload { name: String, host: String, detail: String },

    // ---------------------------------------------------------------- Profils partagés
    ShareInvalid(String),
    ShareCodeUnknown,

    // ---------------------------------------------------------------- Traduction des descriptions
    /// Aucun modèle pour traduire depuis cette langue.
    TranslationUnsupported(String),
    /// Téléchargement d'un modèle impossible.
    TranslationUnavailable(String),

    // ---------------------------------------------------------------- Lancer un jeu
    LaunchFailed(String),

    /// Mod déclaré incompatible avec des mods activés (qu'on peut désactiver).
    Incompatible(String, Vec<crate::installer::Owner>),
}

impl std::fmt::Display for AppError {
    /// Le message, dans la langue de l'interface (français par défaut).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use crate::i18n::pick;
        let text: String = match self {
            // Messages déjà rédigés par l'appelant.
            AppError::InvalidCode(m)
            | AppError::UnknownLayout(m)
            | AppError::WrongPlatform(m)
            | AppError::ToolMismatch(m)
            | AppError::NexusRefused(m)
            | AppError::ShareInvalid(m)
            | AppError::Incompatible(m, _) => return f.write_str(m),
            AppError::MissingApiKey => pick(
                "Aucune clé API n'est configurée. Ajoute ta clé dans les réglages.",
                "No API key is set. Add your key in Settings.",
            )
            .into(),
            AppError::Unauthorized { message, .. } => crate::tr!(
                "mod.io a refusé la clé API : {message}",
                "mod.io rejected the API key: {message}"
            ),
            AppError::Forbidden { message, .. } => crate::tr!(
                "mod.io a refusé l'accès : {message}",
                "mod.io denied access: {message}"
            ),
            AppError::NotFound => pick(
                "Élément introuvable sur mod.io.",
                "Item not found on mod.io.",
            )
            .into(),
            AppError::RateLimited(secs) => crate::tr!(
                "Trop de requêtes envoyées à mod.io. Nouvel essai possible dans {secs} s.",
                "Too many requests sent to mod.io. You can try again in {secs} s."
            ),
            AppError::Api { status, message, .. } => crate::tr!(
                "mod.io a renvoyé une erreur {status} : {message}",
                "mod.io returned error {status}: {message}"
            ),
            AppError::Network(d) => crate::tr!(
                "Impossible de joindre mod.io. Vérifie ta connexion internet. ({d})",
                "Can't reach mod.io. Check your internet connection. ({d})"
            ),
            AppError::Decode(d) => crate::tr!(
                "Réponse inattendue de mod.io : {d}",
                "Unexpected response from mod.io: {d}"
            ),
            AppError::InvalidApiPath(d) => crate::tr!(
                "Chemin d'API invalide : {d}",
                "Invalid API path: {d}"
            ),
            AppError::Storage(d) => crate::tr!(
                "Impossible d'enregistrer les réglages : {d}",
                "Couldn't save the settings: {d}"
            ),
            AppError::InvalidArgument(d) => crate::tr!(
                "Paramètre invalide : {d}",
                "Invalid parameter: {d}"
            ),
            AppError::NotLoggedIn => pick(
                "Connecte-toi à ton compte mod.io pour faire ça.",
                "Sign in to your mod.io account to do that.",
            )
            .into(),
            AppError::SessionExpired => pick(
                "Ta session mod.io a expiré ou a été révoquée. Reconnecte-toi.",
                "Your mod.io session has expired or was revoked. Sign in again.",
            )
            .into(),
            AppError::EmailLoginUnavailable(d) => crate::tr!(
                "mod.io n'autorise pas la connexion par e-mail avec cette clé API ({d}). Utilise plutôt un jeton d'accès.",
                "mod.io doesn't allow email sign-in with this API key ({d}). Use an access token instead."
            ),
            AppError::InvalidToken => pick(
                "mod.io a refusé ce jeton d'accès. Vérifie que tu l'as copié en entier et qu'il a les droits de lecture et d'écriture.",
                "mod.io rejected this access token. Check that you copied all of it and that it has read and write permissions.",
            )
            .into(),
            AppError::NoGameFolder => pick(
                "Choisis d'abord le dossier où installer les mods de ce jeu.",
                "First choose the folder where this game's mods should be installed.",
            )
            .into(),
            AppError::InstallInProgress => pick(
                "Ce mod est déjà en cours d'installation.",
                "This mod is already being installed.",
            )
            .into(),
            AppError::NoModfile => pick(
                "Ce mod n'a aucun fichier publié pour PC (Windows).",
                "This mod has no file published for PC (Windows).",
            )
            .into(),
            AppError::VirusFlagged => pick(
                "Installation bloquée : l'antivirus de mod.io a signalé ce fichier.",
                "Installation blocked: mod.io's antivirus flagged this file.",
            )
            .into(),
            AppError::Corrupted => pick(
                "Le fichier téléchargé est abîmé (somme de contrôle différente). Réessaie.",
                "The downloaded file is damaged (checksum mismatch). Try again.",
            )
            .into(),
            AppError::Cancelled => pick(
                "Installation annulée.",
                "Installation cancelled.",
            )
            .into(),
            AppError::Conflict(d, _) => crate::tr!(
                "Des fichiers existent déjà à cet endroit et ModStudio ne les écrase pas : {d}",
                "Files already exist at this location and ModStudio won't overwrite them: {d}"
            ),
            AppError::Io(d) => crate::tr!(
                "Erreur de fichier : {d}",
                "File error: {d}"
            ),
            AppError::BadArchive(d) => crate::tr!(
                "Archive illisible : {d}",
                "Unreadable archive: {d}"
            ),
            AppError::DownloadRefused(d) => crate::tr!(
                "Le téléchargement de ce fichier a été refusé : {d}",
                "The download of this file was refused: {d}"
            ),
            AppError::LoginRequiredForDownload => pick(
                "Ce jeu demande d'être connecté à mod.io pour télécharger ses mods.",
                "This game requires you to be signed in to mod.io to download its mods.",
            )
            .into(),
            AppError::GameClosed => pick(
                "L'éditeur de ce jeu n'autorise pas les applications tierces comme ModStudio à accéder à ses mods. Tu peux les parcourir et t'y abonner sur mod.io.",
                "This game's publisher doesn't allow third-party apps like ModStudio to access its mods. You can browse them and subscribe to them on mod.io.",
            )
            .into(),
            AppError::UnsupportedArchive(d) => crate::tr!(
                "ModStudio ne peut pas installer cette archive ({d}). Installe ce fichier à la main en suivant la page du mod.",
                "ModStudio can't install this archive ({d}). Install this file manually by following the mod's page."
            ),
            AppError::NeedGameDir(d) => crate::tr!(
                "Ce mod s'installe dans le dossier du jeu. Indique où {d} est installé.",
                "This mod installs into the game folder. Specify where {d} is installed."
            ),
            AppError::ToolUnavailable(d) => crate::tr!(
                "Impossible de récupérer {d}. Vérifie ta connexion internet puis réessaie.",
                "Couldn't get {d}. Check your internet connection, then try again."
            ),
            AppError::ToolPresent(d) => crate::tr!(
                "{d} est déjà installé dans le dossier de ce jeu.",
                "{d} is already installed in this game's folder."
            ),
            AppError::NexusKeyMissing => pick(
                "Ajoute ta clé API Nexus Mods (dans « Compte Nexus ») pour télécharger des mods.",
                "Add your Nexus Mods API key (in \"Nexus account\") to download mods.",
            )
            .into(),
            AppError::NexusKeyInvalid => pick(
                "Nexus Mods a refusé ta clé API. Vérifie que tu l'as copiée en entier, ou crée-en une nouvelle.",
                "Nexus Mods rejected your API key. Check that you copied all of it, or create a new one.",
            )
            .into(),
            AppError::NexusPremiumRequired => pick(
                "Sans compte Premium, Nexus Mods demande de passer par son site pour chaque téléchargement.",
                "Without a Premium account, Nexus Mods requires you to go through its website for each download.",
            )
            .into(),
            AppError::NexusLinkExpired => pick(
                "Le lien de téléchargement de Nexus Mods a expiré ou ne correspond pas à ton compte. Recommence depuis la page du fichier.",
                "The Nexus Mods download link has expired or doesn't match your account. Start again from the file's page.",
            )
            .into(),
            AppError::NexusNotFound => pick(
                "Élément introuvable sur Nexus Mods.",
                "Item not found on Nexus Mods.",
            )
            .into(),
            AppError::NexusRateLimited(secs) => crate::tr!(
                "Trop de requêtes envoyées à Nexus Mods. Nouvel essai possible dans {secs} s.",
                "Too many requests sent to Nexus Mods. You can try again in {secs} s."
            ),
            AppError::NexusApi { status, message } => crate::tr!(
                "Nexus Mods a renvoyé une erreur {status} : {message}",
                "Nexus Mods returned error {status}: {message}"
            ),
            AppError::NexusNetwork(d) => crate::tr!(
                "Impossible de joindre Nexus Mods. Vérifie ta connexion internet. ({d})",
                "Can't reach Nexus Mods. Check your internet connection. ({d})"
            ),
            AppError::NexusDecode(d) => crate::tr!(
                "Réponse inattendue de Nexus Mods : {d}",
                "Unexpected response from Nexus Mods: {d}"
            ),
            AppError::ThunderstoreNotFound => pick(
                "Élément introuvable sur Thunderstore.",
                "Item not found on Thunderstore.",
            )
            .into(),
            AppError::ThunderstoreRateLimited(secs) => crate::tr!(
                "Trop de requêtes envoyées à Thunderstore. Nouvel essai possible dans {secs} s.",
                "Too many requests sent to Thunderstore. You can try again in {secs} s."
            ),
            AppError::ThunderstoreApi { status, message } => crate::tr!(
                "Thunderstore a renvoyé une erreur {status} : {message}",
                "Thunderstore returned error {status}: {message}"
            ),
            AppError::ThunderstoreNetwork(d) => crate::tr!(
                "Impossible de joindre Thunderstore. Vérifie ta connexion internet. ({d})",
                "Can't reach Thunderstore. Check your internet connection. ({d})"
            ),
            AppError::ThunderstoreUnsupported(d) => crate::tr!(
                "Ce jeu charge ses mods avec {d} : ModStudio ne sait pas encore les installer. Utilise r2modman ou Gale pour ce jeu.",
                "This game loads its mods with {d}: ModStudio can't install them yet. Use r2modman or Gale for this game."
            ),
            AppError::RmlUnavailable(d) => crate::tr!(
                "Impossible de récupérer la liste des mods de ResoniteModLoader. Vérifie ta connexion internet. ({d})",
                "Couldn't get the ResoniteModLoader mod list. Check your internet connection. ({d})"
            ),
            AppError::RmlDownload { name, host, detail } => crate::tr!(
                "Impossible de télécharger « {name} » depuis {host} ({detail}). Réessaie plus tard.",
                "Couldn't download \"{name}\" from {host} ({detail}). Try again later."
            ),
            AppError::ListUnavailable { list, detail } => crate::tr!(
                "Impossible de récupérer la liste « {list} ». Vérifie ta connexion internet. ({detail})",
                "Couldn't get the \"{list}\" list. Check your internet connection. ({detail})"
            ),
            AppError::ListDownload { name, host, detail } => crate::tr!(
                "Impossible de télécharger « {name} » depuis {host} ({detail}). Réessaie plus tard.",
                "Couldn't download \"{name}\" from {host} ({detail}). Try again later."
            ),
            AppError::ShareCodeUnknown => pick(
                "Ce code r2modman est introuvable : il a peut-être expiré, ou il manque des caractères.",
                "This r2modman code wasn't found: it may have expired, or some characters are missing.",
            )
            .into(),
            AppError::TranslationUnsupported(lang) => crate::tr!(
                "ModStudio ne sait pas encore traduire depuis cette langue ({lang}).",
                "ModStudio can't translate from this language yet ({lang})."
            ),
            AppError::LaunchFailed(d) => crate::tr!(
                "Impossible de lancer le jeu : {d}",
                "Couldn't start the game: {d}"
            ),
            AppError::TranslationUnavailable(d) => crate::tr!(
                "Impossible de télécharger le modèle de traduction. Vérifie ta connexion internet. ({d})",
                "Couldn't download the translation model. Check your internet connection. ({d})"
            ),
        };
        f.write_str(&text)
    }
}

impl std::error::Error for AppError {}

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
            AppError::Conflict(..) => "conflict",
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
            AppError::ThunderstoreUnsupported(_) => "thunderstore_unsupported",
            AppError::RmlUnavailable(_) | AppError::RmlDownload { .. } => "network",
            AppError::ListUnavailable { .. } | AppError::ListDownload { .. } => "network",
            AppError::ShareInvalid(_) => "share_invalid",
            AppError::ShareCodeUnknown => "not_found",
            AppError::TranslationUnsupported(_) => "translation_unsupported",
            AppError::TranslationUnavailable(_) => "network",
            AppError::LaunchFailed(_) => "launch_failed",
            AppError::Incompatible(..) => "incompatible",
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
        let mut s = serializer.serialize_struct("AppError", 4)?;
        s.serialize_field("kind", self.kind())?;
        s.serialize_field("message", &self.to_string())?;
        let retry_after = match self {
            AppError::RateLimited(secs) | AppError::NexusRateLimited(secs) | AppError::ThunderstoreRateLimited(secs) => Some(*secs),
            _ => None,
        };
        s.serialize_field("retryAfter", &retry_after)?;
        // Conflit qu'on peut lever en désactivant ces mods.
        let step_aside = match self {
            AppError::Conflict(_, owners) | AppError::Incompatible(_, owners) if !owners.is_empty() => Some(owners),
            _ => None,
        };
        s.serialize_field("conflictWith", &step_aside)?;
        s.end()
    }
}

pub type AppResult<T> = Result<T, AppError>;
