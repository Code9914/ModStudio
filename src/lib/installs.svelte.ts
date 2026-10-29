// Mods installés à la main : état partagé par toutes les vues.
import {
  api,
  asAppError,
  type AppError,
  type GameInstall,
  type InstallEvent,
  type InstalledMod,
  type LatestFile,
  type Layout,
} from "./api";
import { bytes } from "./format";

export interface Progress {
  label: string;
  /** Entre 0 et 1, ou null quand la durée est inconnue. */
  ratio: number | null;
}

function describe(e: InstallEvent): Progress {
  switch (e.phase) {
    case "preparing":
      return { label: "Préparation…", ratio: null };
    case "downloading": {
      const ratio = e.total > 0 ? Math.min(1, e.received / e.total) : null;
      const amount = e.total > 0 ? `${bytes(e.received)} sur ${bytes(e.total)}` : bytes(e.received);
      return { label: `Téléchargement : ${amount}`, ratio };
    }
    case "verifying":
      return { label: "Vérification du fichier…", ratio: null };
    case "extracting":
      return { label: "Extraction…", ratio: e.total > 0 ? e.done / e.total : null };
    case "finishing":
      return { label: "Mise en place des fichiers…", ratio: null };
  }
}

class Installs {
  /** Configuration et mods installés, par identifiant de jeu. */
  games = $state<Record<number, GameInstall>>({});
  /** Installations en cours, par identifiant de mod. */
  progress = $state<Record<number, Progress>>({});
  /** Dernière version publiée des mods installés, par identifiant de mod. */
  latest = $state<Record<number, LatestFile>>({});
  loaded = $state(false);

  async load() {
    const list = await api.installOverview();
    this.games = Object.fromEntries(list.map((g) => [g.gameId, g]));
    this.loaded = true;
  }

  game(gameId: number): GameInstall | null {
    return this.games[gameId] ?? null;
  }

  installed(gameId: number, modId: number): InstalledMod | null {
    return this.games[gameId]?.mods[modId] ?? null;
  }

  /** Une version plus récente existe (comparée au fichier publié connu). */
  outdated(gameId: number, modId: number, currentModfileId?: number | null): boolean {
    const record = this.installed(gameId, modId);
    if (!record) return false;
    const latest = currentModfileId || this.latest[modId]?.modfileId;
    return !!latest && latest !== record.modfileId;
  }

  async setFolder(gameId: number, gameName: string, icon: string | null, folder: string, layout: Layout, create = false) {
    const g = await api.setGameFolder(gameId, gameName, icon, folder, layout, create);
    this.games[gameId] = g;
    return g;
  }

  /** `requiredBy` : mod qui a besoin de celui-ci (installation d'un mod nécessaire). */
  async install(gameId: number, modId: number, requiredBy: number | null = null): Promise<InstalledMod> {
    this.progress[modId] = { label: "Préparation…", ratio: null };
    try {
      const record = await api.installMod(
        gameId,
        modId,
        (e) => {
          this.progress[modId] = describe(e);
        },
        requiredBy,
      );
      const game = this.games[gameId];
      if (game) game.mods[modId] = record;
      this.latest[modId] = { modId, modfileId: record.modfileId, version: record.version };
      return record;
    } catch (e) {
      throw asAppError(e);
    } finally {
      delete this.progress[modId];
    }
  }

  cancel(modId: number) {
    api.cancelInstall(modId).catch(() => {});
  }

  async uninstall(gameId: number, modId: number) {
    const result = await api.uninstallMod(gameId, modId);
    const game = this.games[gameId];
    if (game) {
      delete game.mods[modId];
      // Même mise à jour que le backend : ce mod n'a plus besoin des autres.
      for (const m of Object.values(game.mods)) {
        if (m.requiredBy.includes(modId)) m.requiredBy = m.requiredBy.filter((id) => id !== modId);
      }
    }
    return result;
  }

  async checkUpdates(gameId: number): Promise<AppError | null> {
    try {
      for (const l of await api.latestInstalledFiles(gameId)) this.latest[l.modId] = l;
      return null;
    } catch (e) {
      return asAppError(e);
    }
  }
}

export const installs = new Installs();

export const layoutLabels: Record<Layout, { title: string; help: string }> = {
  subfolder: {
    title: "Un sous-dossier par mod",
    help: "Chaque mod est extrait dans son propre dossier. Le plus simple à ranger et à retirer.",
  },
  flat: {
    title: "Extraire directement dans le dossier",
    help: "Pour les jeux qui veulent le contenu des mods directement dans leur dossier de mods.",
  },
  archive: {
    title: "Garder l'archive telle quelle",
    help: "Pour les jeux qui chargent les fichiers .zip sans les extraire.",
  },
};
