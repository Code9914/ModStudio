//! Langue des messages du backend (erreurs, remarques, textes des listes) :
//! celle de l'interface, envoyée au démarrage par `set_locale`.

use std::sync::atomic::{AtomicBool, Ordering};

static ENGLISH: AtomicBool = AtomicBool::new(false);

/// L'interface est en anglais.
pub fn english() -> bool {
    ENGLISH.load(Ordering::Relaxed)
}

/// Le texte dans la langue de l'interface, pour un texte fixe.
pub fn pick(fr: &'static str, en: &'static str) -> &'static str {
    if english() { en } else { fr }
}

/// `tr!("Installé {name}", "Installed {name}")` : le texte (`String`) dans la
/// langue de l'interface. Les deux textes prennent les mêmes arguments.
#[macro_export]
macro_rules! tr {
    ($fr:literal, $en:literal $(,)?) => {
        if $crate::i18n::english() { format!($en) } else { format!($fr) }
    };
    ($fr:literal, $en:literal, $($arg:tt)+) => {
        if $crate::i18n::english() { format!($en, $($arg)+) } else { format!($fr, $($arg)+) }
    };
}

/// Langue choisie dans l'interface (`fr` ou `en`).
#[tauri::command]
pub fn set_locale(locale: String) {
    ENGLISH.store(locale == "en", Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    #[test]
    fn french_by_default() {
        let n = 3;
        assert_eq!(crate::tr!("{n} mods installés", "{n} mods installed"), "3 mods installés");
        assert_eq!(crate::tr!("{} sur {}", "{} of {}", 1, 2), "1 sur 2");
        assert_eq!(super::pick("Oui", "Yes"), "Oui");
    }
}
