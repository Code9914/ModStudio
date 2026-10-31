// Langue de l'interface : français ou anglais, choisie dans les Réglages (par
// défaut, celle du système). Chaque texte est écrit dans les deux langues là
// où il s'affiche : tr("Installer", "Install").
import { invoke } from "@tauri-apps/api/core";

export type Locale = "fr" | "en";

const KEY = "modstudio.locale";

function stored(): Locale | null {
  try {
    const v = localStorage.getItem(KEY);
    return v === "fr" || v === "en" ? v : null;
  } catch {
    return null;
  }
}

function system(): Locale {
  const langs = typeof navigator === "undefined" ? [] : [...(navigator.languages ?? []), navigator.language];
  return langs.some((l) => l?.toLowerCase().startsWith("fr")) ? "fr" : "en";
}

/** Langue choisie (`null` : celle du système). */
export const chosenLocale: Locale | null = stored();
/** Langue de l'interface pour toute la session (changer de langue recharge l'appli). */
export const locale: Locale = chosenLocale ?? system();
export const english = locale === "en";

/** Le texte dans la langue de l'interface. */
export function tr(fr: string, en: string): string {
  return english ? en : fr;
}

/** Locale pour Intl (nombres, dates). */
export const intlLocale = english ? "en-US" : "fr-FR";

if (typeof document !== "undefined") document.documentElement.lang = locale;

/**
 * Le backend écrit ses messages (erreurs, remarques) dans la même langue :
 * envoyée avant toute autre commande.
 */
export const backendLocale: Promise<void> = invoke("set_locale", { locale }).then(
  () => undefined,
  () => undefined,
);

/** Change de langue (`null` : celle du système) puis recharge l'interface. */
export function setLocale(next: Locale | null) {
  try {
    if (next) localStorage.setItem(KEY, next);
    else localStorage.removeItem(KEY);
  } catch {
    /* stockage indisponible : la langue du système reste */
  }
  location.reload();
}
