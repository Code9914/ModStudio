// Mises à jour des mods installés, toutes sources confondues : la liste commune
// (badge « Installés »), une vérification au lancement puis régulière tant que
// ModStudio est ouvert, et une notification quand une nouvelle mise à jour sort
// pendant qu'il est en arrière-plan.
import { isPermissionGranted, sendNotification } from "@tauri-apps/plugin-notification";
import type { GameInstall, InstalledMod } from "./api";
import { installs } from "./installs.svelte";
import { nexusStore } from "./nexus.svelte";
import { tsStore } from "./thunderstore.svelte";
import { listsStore } from "./lists.svelte";
import { tr } from "./i18n.svelte";

export type UpdateSource = "modio" | "nexus" | "ts" | "cl";

export interface PendingUpdate {
  source: UpdateSource;
  gameId: number;
  gameName: string;
  modId: number;
  name: string;
  /** Cette mise à jour précise : change quand une autre version sort. */
  key: string;
}

/** Délai entre deux vérifications régulières. */
export const CHECK_HOURS = 3;
const CHECK_EVERY = CHECK_HOURS * 60 * 60 * 1000;
/** Le délai est contrôlé souvent : la mise en veille de l'ordinateur suspend les minuteries. */
const TICK = 10 * 60 * 1000;
const WATCH_KEY = "modstudio.modUpdates.watch";
const NOTIFY_KEY = "modstudio.modUpdates.notify";
/** Mises à jour déjà vues (badge à l'écran ou notification), pour ne pas les annoncer deux fois. */
const SEEN_KEY = "modstudio.modUpdates.seen";
const SEEN_MAX = 300;

/** Réglage activé sauf s'il a été coupé. */
function readFlag(key: string): boolean {
  try {
    return localStorage.getItem(key) !== "0";
  } catch {
    return true;
  }
}

function writeFlag(key: string, value: boolean) {
  try {
    if (value) localStorage.removeItem(key);
    else localStorage.setItem(key, "0");
  } catch {
    /* réglage gardé pour cette session seulement */
  }
}

function readSeen(): string[] {
  try {
    const v: unknown = JSON.parse(localStorage.getItem(SEEN_KEY) ?? "[]");
    return Array.isArray(v) ? v.filter((k): k is string => typeof k === "string") : [];
  } catch {
    return [];
  }
}

function writeSeen(keys: string[]) {
  try {
    localStorage.setItem(SEEN_KEY, JSON.stringify(keys.slice(-SEEN_MAX)));
  } catch {
    /* au pire, une mise à jour sera annoncée une deuxième fois */
  }
}

function collect(
  source: UpdateSource,
  games: Record<number, GameInstall>,
  outdated: (gameId: number, modId: number) => boolean,
  target: (gameId: number, m: InstalledMod) => string,
): PendingUpdate[] {
  const out: PendingUpdate[] = [];
  for (const g of Object.values(games)) {
    for (const m of Object.values(g.mods)) {
      // Comme « Tout mettre à jour » : un mod désactivé attend d'être réactivé.
      if (m.disabled || !outdated(g.gameId, m.modId)) continue;
      out.push({
        source,
        gameId: g.gameId,
        gameName: g.gameName,
        modId: m.modId,
        name: m.name,
        key: `${source}:${g.gameId}:${m.modId}:${target(g.gameId, m)}`,
      });
    }
  }
  return out;
}

/** Texte de la notification pour ces nouvelles mises à jour. */
export function describeUpdates(fresh: PendingUpdate[]): string {
  const n = fresh.length;
  const games = [...new Set(fresh.map((u) => u.gameName))];
  let what: string;
  if (n === 1) {
    const u = fresh[0];
    what = tr(`« ${u.name} » (${u.gameName}) a une nouvelle version.`, `"${u.name}" (${u.gameName}) has a new version.`);
  } else if (games.length === 1) {
    what = tr(`${games[0]} : ${n} mods ont une nouvelle version.`, `${games[0]}: ${n} mods have a new version.`);
  } else {
    const shown = games.slice(0, 3).join(", ") + (games.length > 3 ? "…" : "");
    what = tr(`${n} mods ont une nouvelle version (${shown}).`, `${n} mods have a new version (${shown}).`);
  }
  const how =
    n === 1 ? tr("Mets-le à jour depuis « Installés ».", 'Update it from "Installed".') : tr("Mets-les à jour depuis « Installés ».", 'Update them from "Installed".');
  return `${what}\n${how}`;
}

function focused(): boolean {
  return typeof document !== "undefined" && document.visibilityState === "visible" && document.hasFocus();
}

class ModUpdates {
  /** Vérifier régulièrement tant que ModStudio est ouvert. */
  watch = $state(readFlag(WATCH_KEY));
  /** Prévenir par une notification quand ModStudio est en arrière-plan. */
  notify = $state(readFlag(NOTIFY_KEY));
  lastCheck = $state<number | null>(null);
  checking = $state(false);
  /** Une source n'a pas répondu à la dernière vérification. */
  incomplete = $state(false);

  /** Mods installés et activés qui ont une nouvelle version. */
  list = $derived<PendingUpdate[]>([
    ...collect(
      "modio",
      installs.games,
      (g, m) => installs.outdated(g, m),
      (_, m) => `${m.modfileId}>${installs.latest[m.modId]?.modfileId ?? ""}`,
    ),
    ...collect(
      "nexus",
      nexusStore.games,
      (g, m) => nexusStore.outdated(g, m),
      (_, m) => m.fileUid ?? m.fileName ?? "",
    ),
    ...collect(
      "ts",
      tsStore.games,
      (g, m) => tsStore.outdated(g, m),
      (g, m) => `${m.version ?? ""}>${tsStore.latest(g, m.modId) ?? ""}`,
    ),
    ...collect(
      "cl",
      listsStore.installs,
      (g, m) => listsStore.outdated(g, m),
      (g, m) => `${m.version ?? ""}>${listsStore.latest(g, m.modId) ?? ""}`,
    ),
  ]);

  #running: Promise<void> | null = null;
  #timer: ReturnType<typeof setInterval> | null = null;

  setWatch(value: boolean) {
    this.watch = value;
    writeFlag(WATCH_KEY, value);
  }

  setNotify(value: boolean) {
    this.notify = value;
    writeFlag(NOTIFY_KEY, value);
  }

  /** Première vérification (mods installés chargés), puis régulièrement si c'est activé. */
  start() {
    void this.check();
    if (this.#timer) return;
    this.#timer = setInterval(() => {
      if (this.watch && !this.checking && Date.now() - (this.lastCheck ?? 0) >= CHECK_EVERY) void this.check();
    }, TICK);
  }

  /** Cherche les nouvelles versions de tous les mods installés (une seule vérification à la fois). */
  check(): Promise<void> {
    this.#running ??= this.#check().finally(() => (this.#running = null));
    return this.#running;
  }

  async #check() {
    this.checking = true;
    try {
      const errors = await Promise.all([
        ...Object.keys(installs.games).map((id) => installs.checkUpdates(Number(id))),
        ...Object.keys(nexusStore.games).map((id) => nexusStore.checkUpdates(Number(id))),
        ...Object.keys(tsStore.games).map((id) => tsStore.checkUpdates(Number(id))),
        ...Object.keys(listsStore.installs).map((id) => listsStore.checkUpdates(Number(id))),
      ]);
      this.incomplete = errors.some((e) => e !== null);
      this.lastCheck = Date.now();
      await this.#announce();
    } finally {
      this.checking = false;
    }
  }

  /** Annonce les mises à jour jamais vues, par une notification si ModStudio est en arrière-plan. */
  async #announce() {
    const seen = readSeen();
    const known = new Set(seen);
    const fresh = this.list.filter((u) => !known.has(u.key));
    if (fresh.length === 0) return;
    writeSeen([...seen, ...fresh.map((u) => u.key)]);
    // À l'écran, le badge de « Installés » suffit.
    if (!this.notify || focused()) return;
    try {
      if (await isPermissionGranted()) {
        sendNotification({ title: tr("Mises à jour de mods", "Mod updates"), body: describeUpdates(fresh) });
      }
    } catch {
      /* notifications indisponibles : le badge reste */
    }
  }
}

export const modUpdates = new ModUpdates();
