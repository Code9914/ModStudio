// Conflits de fichiers : ModStudio n'écrase jamais les fichiers d'un autre mod.
// Quand ils appartiennent à des mods de la même source, on peut désactiver
// ceux-ci (sans les désinstaller) pour faire de la place.
import type { ConflictOwner } from "./api";
import { tr } from "./i18n.svelte";
import { installs } from "./installs.svelte";
import { listsStore } from "./lists.svelte";
import { nexusStore } from "./nexus.svelte";
import { tsStore } from "./thunderstore.svelte";

/** Bibliothèque des mods installés : mod.io, Nexus Mods, Thunderstore (avec ResoniteModLoader), listes. */
export type ModSource = "modio" | "nexus" | "ts" | "cl";

export async function stepAside(source: ModSource, owners: ConflictOwner[]) {
  for (const o of owners) {
    if (source === "modio") await installs.setEnabled(o.gameId, o.modId, false);
    else if (source === "nexus") await nexusStore.setEnabled(o.gameId, o.modId, false);
    else if (source === "ts") await tsStore.setEnabled(o.gameId, o.modId, false);
    else await listsStore.setEnabled(o.gameId, o.modId, false);
  }
}

/** « A », « A » et « B », « A », « B » et « C ». */
export function ownerNames(owners: ConflictOwner[]): string {
  const names = owners.map((o) => tr(`« ${o.name} »`, `"${o.name}"`));
  if (names.length <= 1) return names.join("");
  return `${names.slice(0, -1).join(", ")} ${tr("et", "and")} ${names[names.length - 1]}`;
}
