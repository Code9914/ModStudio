// Bouton « Jouer » : retrouve, parmi les jeux trouvés sur ce PC (Steam, Epic,
// Heroic), celui d'un jeu de ModStudio.
import type { DetectedGame } from "./api";
import { nexusStore } from "./nexus.svelte";
import { normalizeName } from "./thunderstore.svelte";

export interface PlayTarget {
  /** Nom du jeu dans ModStudio. */
  name?: string | null;
  /** Dossiers du jeu connus de ModStudio (dossier des mods, dossier du jeu). */
  folders?: (string | null | undefined)[];
  steamAppId?: number | null;
}

const norm = (p: string) => p.replace(/\\/g, "/").replace(/\/+$/, "").toLowerCase();

/** Le jeu trouvé sur ce PC qui correspond : numéro Steam, puis dossier, puis nom. */
export function detectedGame(t: PlayTarget): DetectedGame | null {
  const list = nexusStore.detected ?? [];
  if (t.steamAppId) {
    const d = list.find((d) => d.launcher === "steam" && d.appId === String(t.steamAppId));
    if (d) return d;
  }
  for (const folder of t.folders ?? []) {
    if (!folder) continue;
    const f = norm(folder);
    const d = list.find((d) => {
      const p = norm(d.path);
      return f === p || f.startsWith(`${p}/`);
    });
    if (d) return d;
  }
  if (t.name) {
    const target = normalizeName(t.name);
    return list.find((d) => normalizeName(d.name) === target) ?? null;
  }
  return null;
}
