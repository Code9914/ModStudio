// Nexus Mods : compte, mods installés et téléchargements, partagés par les vues.
import { listen } from "@tauri-apps/api/event";
import {
  asAppError,
  nexus as api,
  type AppError,
  type DetectedGame,
  type GameInstall,
  type InstalledMod,
  type NexusAuthView,
  type NexusFile,
  type NexusGame,
  type NexusGameSetup,
  type NexusInstalled,
  type NexusUpdate,
  type NxmLink,
  type ProfileEdit,
  type ToolOffer,
} from "./api";
import { describe, type Progress } from "./installs.svelte";

/** Page Nexus à afficher dans ModStudio. */
export type SiteRequest =
  | { kind: "download"; gameId: number; domain: string; modId: number; fileId: number; fileName: string | null }
  | { kind: "login" };

interface Pending {
  gameId: number;
  fileId: number;
  requiredBy?: number;
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
  /** Page Nexus affichée par-dessus le contenu (téléchargement ou connexion). */
  site = $state<SiteRequest | null>(null);
  /** Jeux Steam et Epic installés sur ce PC (`null` : pas encore cherchés). */
  detected = $state<DetectedGame[] | null>(null);
  detecting = $state(false);
  detectError = $state<AppError | null>(null);
  /** Dossiers, moteur et outil conseillé, par jeu. */
  setups = $state<Record<number, NexusGameSetup>>({});
  /** Installations d'outils en cours, par jeu. */
  toolProgress = $state<Record<number, Progress>>({});

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
      // La page s'est fermée d'elle-même (lien capté) : on retire le cadre.
      await listen("nexus-site-closed", () => {
        this.site = null;
      });
    } catch {
      this.#listening = false;
    }
  }

  /**
   * Installe un fichier. En Premium, directement ; sinon la page du fichier
   * s'ouvre et l'installation démarre quand l'utilisateur y clique sur
   * « Slow download ».
   */
  async request(
    gameId: number,
    domain: string,
    modId: number,
    fileId: number,
    fileName: string | null = null,
    requiredBy?: number,
  ): Promise<NexusInstalled> {
    if (this.premium) {
      try {
        return await this.#install(gameId, domain, modId, fileId, undefined, requiredBy);
      } catch (e) {
        if (asAppError(e).kind !== "nexus_premium_required") throw e;
      }
    }
    return this.#viaWebsite(gameId, domain, modId, fileId, fileName, requiredBy);
  }

  #viaWebsite(
    gameId: number,
    domain: string,
    modId: number,
    fileId: number,
    fileName: string | null,
    requiredBy?: number,
  ): Promise<NexusInstalled> {
    const k = `${domain}:${modId}`;
    this.#pending.get(k)?.reject(cancelled());
    return new Promise((resolve, reject) => {
      this.#pending.set(k, { gameId, fileId, requiredBy, resolve, reject });
      this.waiting[keyOf(gameId, modId)] = { fileId, domain };
      // La page s'ouvre dès que le cadre qui l'accueille est en place.
      this.site = { kind: "download", gameId, domain, modId, fileId, fileName };
    });
  }

  /** La page n'a pas pu s'ouvrir : le téléchargement en attente échoue. */
  siteFailed(request: SiteRequest, error: AppError) {
    if (this.site === request) this.site = null;
    if (request.kind !== "download") return;
    const k = `${request.domain}:${request.modId}`;
    const p = this.#pending.get(k);
    this.#pending.delete(k);
    delete this.waiting[keyOf(request.gameId, request.modId)];
    p?.reject(error);
  }

  /** Rouvre la page d'un téléchargement en attente. */
  reopen(gameId: number, domain: string, modId: number, fileName: string | null = null) {
    const w = this.waiting[keyOf(gameId, modId)];
    if (w) this.site = { kind: "download", gameId, domain, modId, fileId: w.fileId, fileName };
  }

  /** Page de connexion à Nexus Mods (et de copie de la clé API). */
  openLogin() {
    this.site = { kind: "login" };
  }

  /** Ferme la page Nexus ; un téléchargement qui l'attendait est annulé. */
  closeSite() {
    const s = this.site;
    this.site = null;
    api.siteClose().catch(() => {});
    if (s?.kind === "download") this.stopWaiting(s.gameId, s.domain, s.modId);
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
    if (this.site?.kind === "download" && this.site.modId === link.modId) this.site = null;
    this.#install(p.gameId, link.domain, link.modId, link.fileId, link, p.requiredBy).then(p.resolve, p.reject);
  }

  async #install(gameId: number, domain: string, modId: number, fileId: number, nxm?: NxmLink, requiredBy?: number) {
    const k = keyOf(gameId, modId);
    this.progress[k] = { label: "Préparation…", ratio: null };
    try {
      const result = await api.install(
        { gameId, gameDomain: domain, modId, fileId, nxmKey: nxm?.key, nxmExpires: nxm?.expires, requiredBy },
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

  /** Cherche les jeux installés (Steam, Epic). */
  async detectGames(refresh = false) {
    if (this.detecting) return;
    this.detecting = true;
    this.detectError = null;
    try {
      this.detected = await api.detectGames(refresh);
      // Les réglages en cache dépendent des jeux trouvés.
      if (refresh) this.setups = {};
    } catch (e) {
      this.detectError = asAppError(e);
      this.detected ??= [];
    } finally {
      this.detecting = false;
    }
  }

  /** Jeux installés qui ont une page sur Nexus Mods. */
  get installedGames(): (DetectedGame & { nexus: NonNullable<DetectedGame["nexus"]> })[] {
    const seen = new Set<number>();
    return (this.detected ?? []).filter((g): g is DetectedGame & { nexus: NonNullable<DetectedGame["nexus"]> } => {
      if (!g.nexus || seen.has(g.nexus.id)) return false;
      seen.add(g.nexus.id);
      return true;
    });
  }

  /** Le jeu installé sur ce PC qui correspond à un jeu Nexus. */
  installedCopy(gameId: number): DetectedGame | null {
    return this.detected?.find((g) => g.nexus?.id === gameId) ?? null;
  }

  async loadSetup(gameId: number, domain: string): Promise<NexusGameSetup> {
    const s = await api.gameSetup(gameId, domain);
    this.setups[gameId] = s;
    return s;
  }

  /** Installe l'outil de mods d'un jeu (UE4SS, BepInEx, MelonLoader). */
  async installTool(gameId: number, domain: string, tool: string) {
    this.toolProgress[gameId] = { label: "Préparation…", ratio: null };
    try {
      await api.installTool(gameId, tool, (e) => {
        this.toolProgress[gameId] = describe(e);
      });
    } catch (e) {
      throw asAppError(e);
    } finally {
      delete this.toolProgress[gameId];
      await Promise.allSettled([this.load(), this.loadSetup(gameId, domain)]);
    }
  }

  cancelTool(gameId: number) {
    api.cancelTool(gameId).catch(() => {});
  }

  async uninstallTool(gameId: number, domain: string, tool: string) {
    try {
      return await api.uninstallTool(gameId, tool);
    } finally {
      await Promise.allSettled([this.load(), this.loadSetup(gameId, domain)]);
    }
  }

  /** Utilise le dossier d'installation trouvé dans Steam ou Epic pour un jeu. */
  async useInstalledFolder(game: NexusGame, path: string) {
    await api.configureGame({
      gameId: game.id,
      domain: game.domainName,
      gameName: game.name,
      icon: `https://images.nexusmods.com/images/games/v2/${game.id}/thumbnail.jpg`,
      folder: path,
      layout: "subfolder",
    });
    await Promise.allSettled([this.load(), this.loadSetup(game.id, game.domainName)]);
  }

  /** Active ou désactive un mod (ses fichiers sont mis de côté). */
  async setEnabled(gameId: number, modId: number, enabled: boolean) {
    this.games[gameId] = await api.setEnabled(gameId, modId, enabled);
  }

  async applyProfile(gameId: number, name: string) {
    const r = await api.applyProfile(gameId, name);
    this.games[gameId] = r.game;
    return r.issues;
  }

  async editProfiles(gameId: number, edit: ProfileEdit) {
    this.games[gameId] = await api.editProfiles(gameId, edit);
  }

  /** Renvoie le dossier laissé en place et les mods nécessaires devenus inutiles. */
  async uninstall(gameId: number, modId: number) {
    const result = await api.uninstall(gameId, modId);
    const game = this.games[gameId];
    if (game) {
      delete game.mods[modId];
      for (const m of Object.values(game.mods)) m.requiredBy = m.requiredBy.filter((id) => id !== modId);
    }
    delete this.updates[keyOf(gameId, modId)];
    return result;
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

const WELCOME_KEY = "modstudio.nexusWelcomeDone";

/** L'accueil Nexus du premier lancement a déjà été vu (clé ajoutée ou « Plus tard »). */
export function welcomeDone(): boolean {
  try {
    return localStorage.getItem(WELCOME_KEY) === "1";
  } catch {
    return true;
  }
}

export function markWelcomeDone() {
  try {
    localStorage.setItem(WELCOME_KEY, "1");
  } catch {
    /* stockage indisponible : l'accueil reviendra, sans gravité */
  }
}

/** Noms des moteurs de jeu. */
export const engineNames: Record<string, string> = {
  unreal: "Unreal Engine",
  "unity-mono": "Unity",
  "unity-il2cpp": "Unity (IL2CPP)",
  bethesda: "Creation Engine",
};

export const launcherNames: Record<string, string> = { steam: "Steam", epic: "Epic Games" };

/** L'outil correspond-il à un mod nécessaire indiqué sur la page d'un mod ? */
export function requirementMatches(tool: ToolOffer, requirementName: string): boolean {
  const n = requirementName.toLowerCase();
  if (tool.id === "ue4ss") return n.includes("ue4ss");
  if (tool.id.startsWith("bepinex")) return n.includes("bepinex");
  if (tool.id === "melonloader") return n.includes("melonloader") || n.includes("melon loader");
  return n.includes(tool.id.replace(/\d+$/, "")) || n.includes(tool.name.toLowerCase());
}

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

/** Préférence pour la version d'un mod faite pour le chargeur installé (« BepInEx » plutôt que « Standalone »). */
function loaderScore(f: NexusFile, loaders: string[]): number {
  const text = `${f.name} ${f.description ?? ""}`.toLowerCase();
  let score = 0;
  for (const [loader, pattern, other] of [
    ["bepinex", /bepinex/, /melon/],
    ["melonloader", /melon/, /bepinex/],
  ] as const) {
    if (!loaders.includes(loader)) continue;
    if (pattern.test(text)) score += 2;
    if (other.test(text)) score -= 1;
    if (/standalone|autonome/.test(text)) score -= 2;
  }
  return score;
}

/**
 * Fichier conseillé d'un mod : le plus récent des fichiers principaux (version
 * PC), fait pour le chargeur de mods du jeu (`loaders` : outils présents).
 */
export function recommendedFile(files: NexusFile[], loaders: string[] = []): NexusFile | null {
  const current = files.filter((f) => ["MAIN", "OPTIONAL", "UPDATE", "MISCELLANEOUS"].includes(f.category));
  const pc = current.filter((f) => !questFile(f.name));
  const pool = pc.length ? pc : current;
  const main = pool.filter((f) => f.category === "MAIN");
  const list = main.length ? main : pool;
  return (
    [...list].sort(
      (a, b) =>
        loaderScore(b, loaders) - loaderScore(a, loaders) || Number(b.primary) - Number(a.primary) || b.date - a.date,
    )[0] ?? null
  );
}

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
