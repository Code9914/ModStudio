// Apparence de ModStudio : couleurs, thème, tailles. Gardée sur cet ordinateur
// (localStorage) et appliquée par des attributs sur <html> : src/app.html la
// pose déjà avant le premier affichage, pour éviter un éclair d'anciennes couleurs.
import { tr } from "./i18n.svelte";

export type ThemeMode = "system" | "dark" | "light";
export type Size = "small" | "normal" | "large";
export type Corners = "square" | "soft" | "round";

export interface Appearance {
  theme: ThemeMode;
  /** Préréglage choisi, ou `custom`. */
  palette: string;
  /** Les deux couleurs du dégradé (identiques pour une seule couleur). */
  colorA: string;
  colorB: string;
  /** Surfaces teintées par la première couleur. */
  tint: boolean;
  text: Size;
  thumbs: Size;
  corners: Corners;
  /** Jeux qui ont des mods listés dans la barre latérale. */
  railGames: boolean;
}

export interface Palette {
  id: string;
  name: string;
  a: string;
  b: string;
}

export const palettes: Palette[] = [
  { id: "mauve-bleu", name: tr("Mauve et bleu", "Mauve and blue"), a: "#c39bff", b: "#8db4ff" },
  { id: "mauve", name: tr("Mauve", "Mauve"), a: "#b48cff", b: "#dba6ff" },
  { id: "bleu", name: tr("Bleu", "Blue"), a: "#7faeff", b: "#72d2ff" },
  { id: "rose", name: tr("Rose", "Pink"), a: "#ff9ccb", b: "#ffb19c" },
  { id: "menthe", name: tr("Menthe", "Mint"), a: "#6fdcb8", b: "#86c9ff" },
  { id: "ambre", name: tr("Ambre", "Amber"), a: "#f2b544", b: "#f7cf6e" },
];

export const defaults: Appearance = {
  theme: "system",
  palette: "mauve-bleu",
  colorA: palettes[0].a,
  colorB: palettes[0].b,
  tint: true,
  text: "normal",
  thumbs: "normal",
  corners: "soft",
  railGames: true,
};

const KEY = "modstudio.appearance";
const HEX = /^#[0-9a-f]{6}$/i;

function oneOf<T extends string>(value: unknown, allowed: readonly T[], fallback: T): T {
  return allowed.includes(value as T) ? (value as T) : fallback;
}

/** Réglages enregistrés, chaque valeur inconnue remplacée par celle par défaut. */
function load(): Appearance {
  let saved: Partial<Appearance> = {};
  try {
    saved = JSON.parse(localStorage.getItem(KEY) ?? "{}") ?? {};
  } catch {
    /* rien d'enregistré ou stockage indisponible */
  }
  return {
    theme: oneOf(saved.theme, ["system", "dark", "light"], defaults.theme),
    palette: typeof saved.palette === "string" ? saved.palette : defaults.palette,
    colorA: typeof saved.colorA === "string" && HEX.test(saved.colorA) ? saved.colorA : defaults.colorA,
    colorB: typeof saved.colorB === "string" && HEX.test(saved.colorB) ? saved.colorB : defaults.colorB,
    tint: typeof saved.tint === "boolean" ? saved.tint : defaults.tint,
    text: oneOf(saved.text, ["small", "normal", "large"], defaults.text),
    thumbs: oneOf(saved.thumbs, ["small", "normal", "large"], defaults.thumbs),
    corners: oneOf(saved.corners, ["square", "soft", "round"], defaults.corners),
    railGames: typeof saved.railGames === "boolean" ? saved.railGames : defaults.railGames,
  };
}

/** Pose les réglages sur <html> (même logique que le script de src/app.html). */
export function apply(a: Appearance) {
  const root = document.documentElement;
  const set = (name: string, value: string | null) => {
    if (value === null) delete root.dataset[name];
    else root.dataset[name] = value;
  };
  set("theme", a.theme === "system" ? null : a.theme);
  set("tint", a.tint ? null : "off");
  set("text", a.text === "normal" ? null : a.text);
  set("thumbs", a.thumbs === "normal" ? null : a.thumbs);
  set("corners", a.corners === "soft" ? null : a.corners);
  root.style.setProperty("--accent-a", a.colorA);
  root.style.setProperty("--accent-b", a.colorB);
  // Barre de titre de Windows assortie au thème choisi.
  import("@tauri-apps/api/window")
    .then(({ getCurrentWindow }) => getCurrentWindow().setTheme(a.theme === "system" ? null : a.theme))
    .catch(() => {});
}

class AppearanceStore {
  value = $state<Appearance>(load());

  constructor() {
    if (typeof document !== "undefined") apply(this.value);
  }

  set(patch: Partial<Appearance>) {
    this.value = { ...this.value, ...patch };
    try {
      localStorage.setItem(KEY, JSON.stringify(this.value));
    } catch {
      /* gardé pour cette session seulement */
    }
    apply(this.value);
  }

  choosePalette(p: Palette) {
    this.set({ palette: p.id, colorA: p.a, colorB: p.b });
  }

  /** Couleurs choisies à la main. */
  setCustom(a: string, b: string) {
    this.set({ palette: "custom", colorA: a, colorB: b });
  }

  reset() {
    this.set({ ...defaults });
  }
}

export const appearance = new AppearanceStore();
