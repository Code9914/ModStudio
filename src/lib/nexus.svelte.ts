// Nexus Mods : compte, mods installés et téléchargements, partagés par les vues.
import { listen } from "@tauri-apps/api/event";
import {
  asAppError,
  nexus as api,
  type AppError,
  type GameInstall,
  type InstalledMod,
  type NexusAuthView,
  type NexusInstalled,
  type NexusUpdate,
  type NxmLink,
} from "./api";
import { describe, type Progress } from "./installs.svelte";

interface Pending {
  gameId: number;
  fileId: number;
  resolve: (r: NexusInstalled) => void;
  reject: (e: AppError) => void;
}

const cancelled = (): AppError => ({ kind: "cancelled", message: "Téléchargement annulé.", retryAfter: null });

/** Les identifiants de mods Nexus sont propres à chaque jeu. */
const keyOf = (gameId: number, modId: number) => `${gameId}:${modId}`;

class NexusStore {
  auth = $state<NexusAuthView | null>(null);
  authLoaded = $state(false);
  /** Dossiers et mods installés, par identifiant de jeu Nexus. */
  games = $state<Record<number, GameInstall>>({});
  loaded = $state(false);
  /** Installations en cours (clé `jeu:mod`). */
  progress = $state<Record<string, Progress>>({});
  /** Téléchargements qui attendent le clic sur la page du fichier (clé `jeu:mod`). */
  waiting = $state<Record<string, { fileId: number; domain: string }>>({});
  /** Fichiers installés remplacés sur Nexus (clé `jeu:mod`). */
  updates = $state<Record<string, NexusUpdate>>({});

  #pending = new Map<string, Pending>();
  #listening = false;

  get ready() {
    return this.auth?.loggedIn === true;
  }

  get premium() {
    return this.auth?.user?.isPremium === true;
  }

  key(gameId: number, modId: number) {
    return keyOf(gameId, modId);
  }

  async refreshAuth() {
    try {
      this.auth = await api.status();
    } catch {
      /* hors ligne : on garde l'état connu */
    } finally {
      this.authLoaded = true;
    }
  }

  async setKey(key: string) {
    this.auth = await api.setKey(key);
  }

  async forgetKey() {
    this.auth = await api.forgetKey();
  }

  async load() {
    const list = await api.overview();
    this.games = Object.fromEntries(list.map((g) => [g.gameId, g]));
    this.loaded = true;
  }

  game(gameId: number): GameInstall | null {
    return this.games[gameId] ?? null;
  }

  installed(gameId: number, modId: number): InstalledMod | null {
    return this.games[gameId]?.mods[modId] ?? null;
  }

  outdated(gameId: number, modId: number) {
    return this.updates[keyOf(gameId, modId)]?.superseded === true;
  }

  /** Écoute les liens `nxm://` captés dans la fenêtre de téléchargement. */
  async listenNxm() {
    if (this.#listening) return;
    this.#listening = true;
    try {
      await listen<NxmLink>("nexus-nxm", (event) => this.#onNxm(event.payload));
    } catch {
      this.#listening = false;
    }
  }

  /**
   * Installe un fichier. En Premium, directement ; sinon la page du fichier
   * s'ouvre et l'installation démarre quand l'utilisateur y clique sur
   * « Slow download ».
   */
  async request(gameId: number, domain: string, modId: number, fileId: number): Promise<NexusInstalled> {
    if (this.premium) {
      try {
        return await this.#install(gameId, domain, modId, fileId);
      } catch (e) {
        if (asAppError(e).kind !== "nexus_premium_required") throw e;
      }
    }
    return this.#viaWebsite(gameId, domain, modId, fileId);
  }

  #viaWebsite(gameId: number, domain: string, modId: number, fileId: number): Promise<NexusInstalled> {
    const k = `${domain}:${modId}`;
    this.#pending.get(k)?.reject(cancelled());
    return new Promise((resolve, reject) => {
      this.#pending.set(k, { gameId, fileId, resolve, reject });
      this.waiting[keyOf(gameId, modId)] = { fileId, domain };
      api.openDownload(domain, modId, fileId).catch((e) => {
        this.#pending.delete(k);
        delete this.waiting[keyOf(gameId, modId)];
        reject(asAppError(e));
      });
    });
  }

  /** Abandonne un téléchargement qui attendait la page du site. */
  stopWaiting(gameId: number, domain: string, modId: number) {
    const k = `${domain}:${modId}`;
    const p = this.#pending.get(k);
    this.#pending.delete(k);
    delete this.waiting[keyOf(gameId, modId)];
    p?.reject(cancelled());
  }

  #onNxm(link: NxmLink) {
    const k = `${link.domain}:${link.modId}`;
    const p = this.#pending.get(k);
    // Lien que ModStudio n'a pas demandé : ignoré.
    if (!p) return;
    this.#pending.delete(k);
    delete this.waiting[keyOf(p.gameId, link.modId)];
    this.#install(p.gameId, link.domain, link.modId, link.fileId, link).then(p.resolve, p.reject);
  }

  async #install(gameId: number, domain: string, modId: number, fileId: number, nxm?: NxmLink) {
    const k = keyOf(gameId, modId);
    this.progress[k] = { label: "Préparation…", ratio: null };
    try {
      const result = await api.install(
        { gameId, gameDomain: domain, modId, fileId, nxmKey: nxm?.key, nxmExpires: nxm?.expires },
        (e) => {
          this.progress[k] = describe(e);
        },
      );
      const game = this.games[gameId];
      if (game) game.mods[modId] = result.record;
      delete this.updates[k];
      // Le dossier du jeu a pu être détecté pendant l'installation.
      this.load().catch(() => {});
      return result;
    } catch (e) {
      throw asAppError(e);
    } finally {
      delete this.progress[k];
    }
  }

  cancel(modId: number) {
    api.cancelInstall(modId).catch(() => {});
  }

  /** Renvoie le dossier laissé en place s'il contient des fichiers ajoutés depuis. */
  async uninstall(gameId: number, modId: number) {
    const left = await api.uninstall(gameId, modId);
    const game = this.games[gameId];
    if (game) delete game.mods[modId];
    delete this.updates[keyOf(gameId, modId)];
    return left;
  }

  async checkUpdates(gameId: number): Promise<AppError | null> {
    try {
      for (const u of await api.updates(gameId)) this.updates[keyOf(gameId, u.modId)] = u;
      return null;
    } catch (e) {
      return asAppError(e);
    }
  }
}

export const nexusStore = new NexusStore();

/** Libellés des catégories de fichiers Nexus. */
export const fileCategories: Record<string, string> = {
  MAIN: "Fichiers principaux",
  OPTIONAL: "Fichiers optionnels",
  UPDATE: "Mises à jour",
  MISCELLANEOUS: "Divers",
  OLD_VERSION: "Anciennes versions",
  ARCHIVED: "Archivés",
  REMOVED: "Retirés",
};

/** Fichier destiné au Quest (Android) plutôt qu'au PC, d'après son nom. */
export function questFile(name: string) {
  return /\b(quest|android|oculus)\b/i.test(name) && !/\bpc(vr)?\b/i.test(name);
}

/** Date ISO de Nexus → secondes Unix. */
export function isoToUnix(iso: string | null | undefined): number | null {
  if (!iso) return null;
  const t = Date.parse(iso);
  return Number.isNaN(t) ? null : Math.floor(t / 1000);
}
