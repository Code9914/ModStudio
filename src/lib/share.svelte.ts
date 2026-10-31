// Profils partagés : libellés et petits outils communs à l'export et à l'import.
import type { ShareSource } from "./api";

export const sourceLabels: Record<ShareSource, string> = {
  modio: "mod.io",
  nexus: "Nexus Mods",
  ts: "Thunderstore",
  cl: "Communautés",
};

/** Filtres des fenêtres « Enregistrer » et « Ouvrir ». */
export const profileFilters = {
  save: [{ name: "Profil ModStudio", extensions: ["modstudio"] }],
  open: [
    { name: "Profils (ModStudio, r2modman)", extensions: ["modstudio", "r2z", "json", "txt"] },
    { name: "Tous les fichiers", extensions: ["*"] },
  ],
};

/** Nom de fichier sûr pour un profil. */
export function fileNameFor(name: string) {
  const clean = name.replace(/[\\/:*?"<>|\u0000-\u001f]+/g, " ").replace(/\s+/g, " ").trim();
  return `${clean || "Profil"}.modstudio`;
}

/** Même identifiant que ModStudio donne à un mod de Thunderstore (FNV-1a, 53 bits). */
export function stableId(s: string): number {
  let h = 0xcbf29ce484222325n;
  for (const b of new TextEncoder().encode(s.toLowerCase())) {
    h ^= BigInt(b);
    h = (h * 0x100000001b3n) & 0xffffffffffffffffn;
  }
  const id = Number(h & ((1n << 53n) - 1n));
  return Math.max(id, 1);
}

/** Copie dans le presse-papiers ; `false` s'il est indisponible. */
export async function copyText(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    return false;
  }
}
