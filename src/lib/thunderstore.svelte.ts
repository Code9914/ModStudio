// Mods installés depuis Thunderstore : état partagé par toutes les vues.
import {
  asAppError,
  rml,
  ts,
  type AppError,
  type DetectedGame,
  type GameInstall,
  type InstallEvent,
  type InstalledMod,
  type ProfileEdit,
  type RmlMod,
  type RmlStatus,
  type TsCommunity,
  type TsInstalled,
  type TsLoader,
} from "./api";
import { tr } from "./i18n.svelte";
import { describe, type Progress } from "./installs.svelte";
import { nexusStore } from "./nexus.svelte";

const keyOf = (gameId: number, modId: number) => `${gameId}:${modId}`;

/** Nom comparable d'un jeu (« R.E.P.O. » et « REPO », « Risk of Rain 2 » et « Risk of Rain 2™ »). */
export function normalizeName(name: string) {
  return name
    .normalize("NFKD")
    .replace(/[̀-ͯ]/g, "")
    .toLowerCase()
    .replace(/[™®©]/g, "")
    .replace(/[^a-z0-9]+/g, "");
}

class TsStore {
  /** Configuration et mods installés, par identifiant de jeu. */
  games = $state<Record<number, GameInstall>>({});
  loaded = $state(false);
  /** Jeux de Thunderstore (`null` : pas encore chargés). */
  communities = $state<TsCommunity[] | null>(null);
  communitiesError = $state<AppError | null>(null);
  /** Installation en cours, par jeu : le mod demandé et l'étape. */
  progress = $state<Record<number, Progress & { modId: number }>>({});
  /** Nouvelle version publiée, par `jeu:mod`. */
  updates = $state<Record<string, string>>({});
  /** Chargeur de mods présent, par jeu. */
  loaders = $state<Record<number, TsLoader>>({});
  /** Liste vérifiée des mods de ResoniteModLoader (`null` : pas encore chargée). */
  rmlMods = $state<RmlMod[] | null>(null);
  rmlError = $state<AppError | null>(null);
  /** ResoniteModLoader dans le dossier de Resonite. */
  rmlStatus = $state<RmlStatus | null>(null);

  #communitiesLoading: Promise<void> | null = null;
  #rmlLoading: Promise<void> | null = null;

  async load() {
    const list = await ts.overview();
    this.games = Object.fromEntries(list.map((g) => [g.gameId, g]));
    this.loaded = true;
  }

  loadCommunities(refresh = false): Promise<void> {
    if (this.communities && !refresh) return Promise.resolve();
    this.#communitiesLoading ??= (async () => {
      this.communitiesError = null;
      try {
        this.communities = await ts.communities(refresh);
      } catch (e) {
        this.communitiesError = asAppError(e);
      } finally {
        this.#communitiesLoading = null;
      }
    })();
    return this.#communitiesLoading;
  }

  community(identifier: string): TsCommunity | null {
    return this.communities?.find((c) => c.identifier === identifier) ?? null;
  }

  /** Jeu de la bibliothèque → fiche du jeu (même hors ligne, depuis ce qui est enregistré). */
  communityOf(g: GameInstall): TsCommunity {
    return (
      this.community(g.domain ?? "") ?? {
        id: g.gameId,
        identifier: g.domain ?? "",
        name: g.gameName,
        coverImageUrl: g.icon,
        heroImageUrl: null,
        communityIconUrl: g.icon,
        totalDownloadCount: 0,
        totalPackageCount: 0,
        loader: null,
        // Des mods y sont installés : ModStudio les a pris en charge.
        supported: true,
      }
    );
  }

  /** Jeu installé sur ce PC (Steam, Epic) qui correspond à ce jeu Thunderstore. */
  detectedFor(c: TsCommunity): DetectedGame | null {
    const target = normalizeName(c.name);
    return (nexusStore.detected ?? []).find((g) => normalizeName(g.name) === target) ?? null;
  }

  count(g: GameInstall): number {
    return Object.keys(g.mods).length;
  }

  game(gameId: number): GameInstall | null {
    return this.games[gameId] ?? null;
  }

  installed(gameId: number, modId: number): InstalledMod | null {
    return this.games[gameId]?.mods[modId] ?? null;
  }

  outdated(gameId: number, modId: number) {
    return !!this.updates[keyOf(gameId, modId)];
  }

  latest(gameId: number, modId: number): string | null {
    return this.updates[keyOf(gameId, modId)] ?? null;
  }

  async configure(c: TsCommunity, folder: string) {
    const g = await ts.configure(c.identifier, c.name, c.communityIconUrl ?? c.coverImageUrl, folder);
    this.games[g.gameId] = g;
    this.refreshLoader(g.gameId);
    return g;
  }

  async refreshLoader(gameId: number) {
    try {
      this.loaders[gameId] = await ts.loader(gameId);
    } catch {
      delete this.loaders[gameId];
    }
  }

  /** Installe un mod et ce dont il a besoin. */
  install(c: TsCommunity, namespace: string, name: string, modId: number, version?: string | null): Promise<TsInstalled> {
    return this.#run(c.id, modId, (onEvent) => ts.install({ community: c.identifier, namespace, name, version }, onEvent));
  }

  /** Installe un mod de ResoniteModLoader, ce dont il a besoin, et RML s'il manque. */
  async installRml(c: TsCommunity, mod: RmlMod, version?: string | null): Promise<TsInstalled> {
    try {
      return await this.#run(c.id, mod.modId, (onEvent) => rml.install(mod.id, version ?? null, onEvent));
    } finally {
      this.refreshRmlStatus();
    }
  }

  /** Suit l'installation (étapes, progression) puis met la bibliothèque à jour. */
  async #run(gameId: number, modId: number, start: (onEvent: (e: InstallEvent) => void) => Promise<TsInstalled>): Promise<TsInstalled> {
    let step = "";
    this.progress[gameId] = { modId, label: tr("Préparation…", "Preparing…"), ratio: null };
    try {
      const result = await start((e) => {
        if (e.phase === "step") {
          // Un seul mod : pas besoin de le nommer à chaque étape.
          step = e.total > 1 ? tr(`${e.name} (${e.index} sur ${e.total}), `, `${e.name} (${e.index} of ${e.total}), `) : "";
          this.progress[gameId] = { modId, label: tr(`${step}préparation…`, `${step}preparing…`), ratio: null };
          return;
        }
        const p = describe(e);
        this.progress[gameId] = { modId, label: step ? `${step}${p.label.charAt(0).toLowerCase()}${p.label.slice(1)}` : p.label, ratio: p.ratio };
      });
      this.games[gameId] = result.game;
      for (const id of Object.keys(result.game.mods)) {
        const m = result.game.mods[Number(id)];
        if (this.updates[keyOf(gameId, m.modId)] === m.version) delete this.updates[keyOf(gameId, m.modId)];
      }
      this.refreshLoader(gameId);
      return result;
    } catch (e) {
      throw asAppError(e);
    } finally {
      delete this.progress[gameId];
    }
  }

  loadRml(refresh = false): Promise<void> {
    if (this.rmlMods && !refresh) return Promise.resolve();
    this.#rmlLoading ??= (async () => {
      this.rmlError = null;
      try {
        this.rmlMods = await rml.mods(refresh);
      } catch (e) {
        this.rmlError = asAppError(e);
      } finally {
        this.#rmlLoading = null;
      }
    })();
    return this.#rmlLoading;
  }

  rmlMod(id: string): RmlMod | null {
    const key = id.toLowerCase();
    return this.rmlMods?.find((m) => m.id.toLowerCase() === key) ?? null;
  }

  async refreshRmlStatus() {
    try {
      this.rmlStatus = await rml.status();
    } catch {
      this.rmlStatus = null;
    }
  }

  cancel(c: TsCommunity) {
    ts.cancel(c.identifier).catch(() => {});
  }

  async setEnabled(gameId: number, modId: number, enabled: boolean) {
    this.games[gameId] = await ts.setEnabled(gameId, modId, enabled);
  }

  async applyProfile(gameId: number, name: string) {
    const r = await ts.applyProfile(gameId, name);
    this.games[gameId] = r.game;
    this.refreshLoader(gameId);
    return r.issues;
  }

  async editProfiles(gameId: number, edit: ProfileEdit) {
    this.games[gameId] = await ts.editProfiles(gameId, edit);
  }

  async uninstall(gameId: number, modId: number) {
    const result = await ts.uninstall(gameId, modId);
    const game = this.games[gameId];
    if (game) {
      delete game.mods[modId];
      for (const m of Object.values(game.mods)) m.requiredBy = m.requiredBy.filter((id) => id !== modId);
    }
    delete this.updates[keyOf(gameId, modId)];
    this.refreshLoader(gameId);
    if (game?.domain === RML_COMMUNITY) this.refreshRmlStatus();
    return result;
  }

  async checkUpdates(gameId: number): Promise<AppError | null> {
    try {
      for (const u of await ts.updates(gameId)) this.updates[keyOf(gameId, u.modId)] = u.version;
      return null;
    } catch (e) {
      return asAppError(e);
    }
  }
}

export const tsStore = new TsStore();

/** Le jeu Thunderstore qui reçoit aussi les mods de ResoniteModLoader. */
export const RML_COMMUNITY = "resonite";

/** Catégories de la liste de ResoniteModLoader. */
const RML_CATEGORIES: Record<string, string> = {
  "Asset Importing": "Import de fichiers",
  Audio: "Audio",
  "Bug Workaround": "Correctifs",
  "Context Menu": "Menu contextuel",
  Controls: "Contrôles",
  Dashboard: "Tableau de bord",
  "Hardware Integration": "Matériel",
  Inspectors: "Inspecteurs",
  Libraries: "Bibliothèques",
  Memes: "Pour rire",
  Misc: "Divers",
  Optimization: "Optimisation",
  Plugins: "Plugins",
  Protoflux: "ProtoFlux",
  "Technical Tweaks": "Réglages techniques",
  Template: "Modèles",
  "Visual Tweaks": "Apparence",
  Wizards: "Assistants",
};

export function rmlCategory(category: string) {
  return tr(RML_CATEGORIES[category] ?? category, category);
}

/** Systèmes visés par un mod de ResoniteModLoader, en clair. */
export function rmlPlatforms(platforms: string[]) {
  const names: Record<string, string> = {
    headless: tr("serveurs (headless)", "servers (headless)"),
    windows: "Windows",
    linux: "Linux",
    "linux-native": "Linux",
    "linux-wine": "Linux (Proton)",
    android: "Android",
  };
  return [...new Set(platforms.map((p) => names[p] ?? p))].join(", ");
}

/** `Équipe-Nom` d'un mod installé. */
export function splitFullName(fileName: string | null | undefined): { namespace: string; name: string } | null {
  const i = fileName?.indexOf("-") ?? -1;
  if (!fileName || i <= 0) return null;
  return { namespace: fileName.slice(0, i), name: fileName.slice(i + 1) };
}

/** Chargeurs publiés sur Thunderstore (hors BepInExPack et MelonLoader, reconnus à leur nom). */
const LOADER_PACKAGES = new Set(
  [
    "ResoniteModding-BepisLoader",
    "northstar-Northstar",
    "GodotModding-GodotModLoader",
    "0xFFF7-votv_shimloader",
    "Thunderstore-unreal_shimloader",
    "Thunderstore-lovely",
    "ReturnOfModding-ReturnOfModding",
    "Hell2Modding-Hell2Modding",
    "NotNet-GDWeave",
    "UMM-UMM",
    "ADOFAI_Modding-UMM_ADOFAI",
    "ReDoIngMods-Rivet",
    "xiaoye97-BepInEx",
    "1F31A-BepInEx_Valheim_Full",
    "bbepis-BepInEx_Rogue_Tower",
    "HomeSafetyHotline_Modding-MelonLoader_HomeSafetyHotline",
    "SHMCD_Modding-MelonLoader_SHMCD",
  ].map((id) => id.toLowerCase()),
);

/** Le chargeur du jeu lui-même (`BepInExPack`, `MelonLoader`, `BepisLoader`…). */
export function isLoaderName(name: string, namespace = "") {
  const n = name.toLowerCase();
  return n.startsWith("bepinexpack") || n === "melonloader" || LOADER_PACKAGES.has(`${namespace}-${name}`.toLowerCase());
}
