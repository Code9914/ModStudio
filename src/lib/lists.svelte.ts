// Jeux dont la communauté tient sa propre liste de mods : état partagé par toutes les vues.
import { asAppError, cl, type AppError, type ClDeclared, type ClGame, type ClInstalled, type ClLoader, type ClMod, type DetectedGame, type GameInstall, type ProfileEdit } from "./api";
import { tr } from "./i18n.svelte";
import { describe, type Progress } from "./installs.svelte";
import { nexusStore } from "./nexus.svelte";
import { normalizeName } from "./thunderstore.svelte";

const keyOf = (gameId: number, modId: number) => `${gameId}:${modId}`;

class ListsStore {
  /** Les jeux pris en charge (fixes). */
  games = $state<ClGame[]>([]);
  /** Configuration et mods installés, par identifiant de jeu. */
  installs = $state<Record<number, GameInstall>>({});
  loaded = $state(false);
  /** Liste de chaque jeu (par son identifiant texte). */
  mods = $state<Record<string, ClMod[]>>({});
  errors = $state<Record<string, AppError>>({});
  loaders = $state<Record<string, ClLoader>>({});
  /** Installation en cours, par jeu. */
  progress = $state<Record<number, Progress & { modId: number }>>({});
  /** Nouvelle version publiée, par `jeu:mod`. */
  updates = $state<Record<string, string>>({});
  /** Incompatibilités déclarées des mods installés, par jeu (identifiant texte). */
  declared = $state<Record<string, ClDeclared[]>>({});

  #loading: Record<string, Promise<void> | undefined> = {};

  async load() {
    const [games, overview] = await Promise.all([cl.games(), cl.overview()]);
    this.games = games;
    this.installs = Object.fromEntries(overview.map((g) => [g.gameId, g]));
    this.loaded = true;
  }

  game(id: string): ClGame | null {
    return this.games.find((g) => g.id === id) ?? null;
  }

  /** Jeu de la bibliothèque → sa fiche. */
  gameOf(g: GameInstall): ClGame | null {
    return this.games.find((x) => x.gameId === g.gameId) ?? null;
  }

  config(game: ClGame): GameInstall | null {
    return this.installs[game.gameId] ?? null;
  }

  count(g: GameInstall) {
    return Object.keys(g.mods).length;
  }

  /** Jeu installé sur ce PC (Steam…) qui correspond. */
  detectedFor(game: ClGame): DetectedGame | null {
    const detected = nexusStore.detected ?? [];
    const steamId = String(game.steamAppId);
    const target = normalizeName(game.name);
    return (
      detected.find((g) => g.launcher === "steam" && g.appId === steamId) ??
      detected.find((g) => normalizeName(g.name) === target) ??
      null
    );
  }

  installed(gameId: number, modId: number) {
    return this.installs[gameId]?.mods[modId] ?? null;
  }

  outdated(gameId: number, modId: number) {
    return !!this.updates[keyOf(gameId, modId)];
  }

  latest(gameId: number, modId: number): string | null {
    return this.updates[keyOf(gameId, modId)] ?? null;
  }

  loadMods(game: ClGame, refresh = false): Promise<void> {
    if (this.mods[game.id] && !refresh) return Promise.resolve();
    this.#loading[game.id] ??= (async () => {
      delete this.errors[game.id];
      try {
        this.mods[game.id] = await cl.mods(game.id, refresh);
      } catch (e) {
        this.errors[game.id] = asAppError(e);
      } finally {
        delete this.#loading[game.id];
      }
    })();
    return this.#loading[game.id]!;
  }

  /** Relit les incompatibilités déclarées des mods installés (après un changement). */
  async loadDeclared(game: ClGame) {
    if (!this.config(game)) {
      delete this.declared[game.id];
      return;
    }
    try {
      this.declared[game.id] = await cl.declared(game.id);
    } catch {
      /* liste injoignable : pas d'avertissement */
    }
  }

  /** Mods installés et activés (identifiant de la liste → nom). */
  #enabled(game: ClGame): Map<string, string> {
    const prefix = `${game.id}:`;
    const out = new Map<string, string>();
    for (const m of Object.values(this.config(game)?.mods ?? {})) {
      if (!m.disabled && m.fileName?.startsWith(prefix)) out.set(m.fileName.slice(prefix.length).toLowerCase(), m.name);
    }
    return out;
  }

  /** Noms des mods activés déclarés incompatibles avec `mod` (par la liste ou par l'un d'eux). */
  incompatibleWith(game: ClGame, mod: ClMod): string[] {
    const enabled = this.#enabled(game);
    const id = mod.id.toLowerCase();
    const names = new Set<string>();
    for (const c of mod.versions[0]?.conflicts ?? []) {
      const name = enabled.get(c.id.toLowerCase());
      if (name && c.id.toLowerCase() !== id) names.add(name);
    }
    for (const d of this.declared[game.id] ?? []) {
      if (d.id.toLowerCase() !== id && enabled.has(d.id.toLowerCase()) && d.conflicts.some((c) => c.toLowerCase() === id)) names.add(d.name);
    }
    return [...names];
  }

  /** Paires de mods activés déclarés incompatibles entre eux. */
  incompatiblePairs(game: ClGame): [string, string][] {
    const enabled = this.#enabled(game);
    const seen = new Set<string>();
    const out: [string, string][] = [];
    for (const d of this.declared[game.id] ?? []) {
      for (const c of d.conflicts) {
        const other = enabled.get(c.toLowerCase());
        if (!other || c.toLowerCase() === d.id.toLowerCase()) continue;
        const pair = [d.name, other].sort() as [string, string];
        const key = pair.join("\u0000");
        if (!seen.has(key)) {
          seen.add(key);
          out.push(pair);
        }
      }
    }
    return out;
  }

  mod(game: ClGame, id: string): ClMod | null {
    const key = id.toLowerCase();
    return this.mods[game.id]?.find((m) => m.id.toLowerCase() === key) ?? null;
  }

  async configure(game: ClGame, folder: string) {
    const g = await cl.configure(game.id, folder);
    this.installs[g.gameId] = g;
    // La liste peut dépendre du jeu installé (version de Beat Saber).
    delete this.mods[game.id];
    this.loadMods(game);
    this.refreshLoader(game);
    return g;
  }

  async refreshLoader(game: ClGame) {
    if (!this.config(game)) return;
    try {
      this.loaders[game.id] = await cl.loader(game.id);
    } catch {
      delete this.loaders[game.id];
    }
  }

  /** Installe un mod, ce dont il a besoin et le chargeur s'il manque. */
  async install(game: ClGame, mod: ClMod, version?: string | null): Promise<ClInstalled> {
    const gameId = game.gameId;
    let step = "";
    this.progress[gameId] = { modId: mod.modId, label: tr("Préparation…", "Preparing…"), ratio: null };
    try {
      const result = await cl.install(game.id, mod.id, version ?? null, (e) => {
        if (e.phase === "step") {
          step = e.total > 1 ? tr(`${e.name} (${e.index} sur ${e.total}), `, `${e.name} (${e.index} of ${e.total}), `) : "";
          this.progress[gameId] = { modId: mod.modId, label: tr(`${step}préparation…`, `${step}preparing…`), ratio: null };
          return;
        }
        const p = describe(e);
        this.progress[gameId] = {
          modId: mod.modId,
          label: step ? `${step}${p.label.charAt(0).toLowerCase()}${p.label.slice(1)}` : p.label,
          ratio: p.ratio,
        };
      });
      this.installs[gameId] = result.game;
      for (const m of Object.values(result.game.mods)) {
        if (this.updates[keyOf(gameId, m.modId)] === m.version) delete this.updates[keyOf(gameId, m.modId)];
      }
      this.refreshLoader(game);
      this.loadDeclared(game);
      return result;
    } catch (e) {
      throw asAppError(e);
    } finally {
      delete this.progress[gameId];
    }
  }

  cancel(game: ClGame) {
    cl.cancel(game.id).catch(() => {});
  }

  async setEnabled(gameId: number, modId: number, enabled: boolean) {
    this.installs[gameId] = await cl.setEnabled(gameId, modId, enabled);
    const game = this.games.find((g) => g.gameId === gameId);
    if (game) this.loadDeclared(game);
  }

  async applyProfile(gameId: number, name: string) {
    const r = await cl.applyProfile(gameId, name);
    this.installs[gameId] = r.game;
    const game = this.games.find((g) => g.gameId === gameId);
    if (game) this.refreshLoader(game);
    return r.issues;
  }

  async editProfiles(gameId: number, edit: ProfileEdit) {
    this.installs[gameId] = await cl.editProfiles(gameId, edit);
  }

  async uninstall(gameId: number, modId: number) {
    const result = await cl.uninstall(gameId, modId);
    const g = this.installs[gameId];
    if (g) {
      delete g.mods[modId];
      for (const m of Object.values(g.mods)) m.requiredBy = m.requiredBy.filter((id) => id !== modId);
    }
    delete this.updates[keyOf(gameId, modId)];
    const game = this.games.find((x) => x.gameId === gameId);
    if (game) {
      this.refreshLoader(game);
      this.loadDeclared(game);
    }
    return result;
  }

  async checkUpdates(gameId: number): Promise<AppError | null> {
    try {
      for (const u of await cl.updates(gameId)) this.updates[keyOf(gameId, u.modId)] = u.version;
      return null;
    } catch (e) {
      return asAppError(e);
    }
  }
}

export const listsStore = new ListsStore();
