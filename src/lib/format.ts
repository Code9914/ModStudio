// Mise en forme des nombres, tailles et dates, dans la langue de l'interface.
import { intlLocale, tr } from "./i18n.svelte";

const compact = new Intl.NumberFormat(intlLocale, { notation: "compact", maximumFractionDigits: 1 });
const full = new Intl.NumberFormat(intlLocale);
const dateFmt = new Intl.DateTimeFormat(intlLocale, { day: "numeric", month: "long", year: "numeric" });
const relative = new Intl.RelativeTimeFormat(intlLocale, { numeric: "auto" });

// Intl utilise l'espace fine insécable (U+202F), absente de certaines polices :
// on la remplace par une espace insécable classique.
const nbsp = (s: string) => s.replace(/\u202f/g, "\u00a0");

/** 12 345 → « 12 k » */
export function count(n: number | null | undefined): string {
  return nbsp(compact.format(n ?? 0));
}

/** 12 345 → « 12 345 » */
export function exact(n: number | null | undefined): string {
  return nbsp(full.format(n ?? 0));
}

export function bytes(n: number | null | undefined): string {
  const size = n ?? 0;
  if (size <= 0) return "—";
  const units = tr("o Ko Mo Go To", "B KB MB GB TB").split(" ");
  const i = Math.min(Math.floor(Math.log(size) / Math.log(1024)), units.length - 1);
  const value = size / 1024 ** i;
  return nbsp(`${value.toLocaleString(intlLocale, { maximumFractionDigits: i === 0 ? 0 : 1 })}\u00a0${units[i]}`);
}

/** Horodatage Unix (secondes) → « 3 mars 2026 ». */
export function date(ts: number | null | undefined): string {
  if (!ts) return "—";
  return dateFmt.format(new Date(ts * 1000));
}

/** Horodatage Unix (secondes) → « il y a 3 jours ». */
export function ago(ts: number | null | undefined): string {
  if (!ts) return "—";
  const diff = ts * 1000 - Date.now();
  const steps: [Intl.RelativeTimeFormatUnit, number][] = [
    ["year", 365 * 24 * 3600e3],
    ["month", 30 * 24 * 3600e3],
    ["week", 7 * 24 * 3600e3],
    ["day", 24 * 3600e3],
    ["hour", 3600e3],
    ["minute", 60e3],
  ];
  for (const [unit, ms] of steps) {
    if (Math.abs(diff) >= ms) return relative.format(Math.round(diff / ms), unit);
  }
  return tr("à l'instant", "just now");
}

/** Libellé lisible d'une plateforme mod.io. */
export function platformName(p: string | null): string {
  const names: Record<string, string> = {
    windows: "Windows",
    mac: "macOS",
    linux: "Linux",
    android: "Android",
    ios: "iOS",
    xboxone: "Xbox One",
    xboxseriesx: "Xbox Series",
    ps4: "PS4",
    ps5: "PS5",
    switch: "Switch",
    oculus: "Meta Quest",
    source: "Source",
  };
  return p ? (names[p] ?? p) : "";
}
