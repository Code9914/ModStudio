// Installation d'une collection Nexus : ses mods un par un, dans l'ordre de la
// collection (le mod placé après gagne sur les fichiers en commun). Gardée ici
// pour continuer pendant qu'on navigue ailleurs dans ModStudio.
import {
  asAppError,
  nexus as api,
  type AppError,
  type CollectionPlan,
  type FomodInstaller,
  type NexusCollectionDetail,
  type NexusInstalled,
  type NexusInstallOutcome,
} from "./api";
import { nexusStore } from "./nexus.svelte";
import { tr } from "./i18n.svelte";

/**
 * `pending` : à installer ; `working` : en cours ; `installed` : installé par
 * la collection ; `kept` : déjà là, gardé tel quel ; `skipped` : optionnel non
 * choisi ou options abandonnées ; `failed` : erreur (on peut réessayer).
 */
export type ItemStatus = "pending" | "working" | "installed" | "kept" | "skipped" | "failed";

export interface QueueItem {
  modId: number;
  fileId: number;
  name: string;
  fileName: string;
  rank: number;
  optional: boolean;
  status: ItemStatus;
  error: AppError | null;
  notice: string | null;
  instructions: string | null;
  choices: string[];
}

interface Fomod {
  token: string;
  installer: FomodInstaller;
  modId: number;
  name: string;
  choices: string[];
  resolve: (r: NexusInstalled | null) => void;
}

class CollectionInstall {
  slug = $state<string | null>(null);
  name = $state("");
  gameId = $state(0);
  domain = $state("");
  items = $state<QueueItem[]>([]);
  plan = $state<CollectionPlan | null>(null);
  /** `planning` : lecture de l'ordre ; `installing` : en cours ; `paused` ; `done`. */
  phase = $state<"idle" | "planning" | "installing" | "paused" | "done">("idle");
  error = $state<AppError | null>(null);
  /** Options FOMOD à choisir pour le mod en cours. */
  fomod = $state<Fomod | null>(null);
  #stop = false;

  /** Installation de cette collection en cours ou terminée (affichée sur sa page). */
  on(slug: string) {
    return this.slug === slug && this.phase !== "idle";
  }

  get busy() {
    return this.phase === "planning" || this.phase === "installing";
  }

  get counts() {
    const done = this.items.filter((i) => i.status === "installed" || i.status === "kept").length;
    const todo = this.items.filter((i) => i.status !== "skipped").length;
    const failed = this.items.filter((i) => i.status === "failed").length;
    return { done, todo, failed };
  }

  item(fileId: number) {
    return this.items.find((i) => i.fileId === fileId) ?? null;
  }

  /** Installe une collection : ses mods requis, et les optionnels choisis (`optional` : numéros de fichier). */
  async start(detail: NexusCollectionDetail, gameId: number, optional: Set<number>) {
    if (this.busy) return;
    this.slug = detail.slug;
    this.name = detail.name;
    this.gameId = gameId;
    this.domain = detail.game.domainName;
    this.items = [];
    this.plan = null;
    this.error = null;
    this.phase = "planning";
    try {
      const plan = await api.collectionPlan(detail.slug, detail.game.domainName, detail.revision);
      if (this.slug !== detail.slug) return;
      this.plan = plan;
      const byFile = new Map(detail.mods.map((m) => [m.fileId, m]));
      const installed = nexusStore.games[gameId]?.mods ?? {};
      this.items = plan.order.flatMap((e) => {
        const m = byFile.get(e.fileId);
        if (!m) return [];
        const status: ItemStatus = m.optional && !optional.has(m.fileId) ? "skipped" : installed[m.modId] ? "kept" : "pending";
        return [
          {
            modId: m.modId,
            fileId: m.fileId,
            name: m.name,
            fileName: m.fileName,
            rank: e.rank,
            optional: m.optional,
            status,
            error: null,
            notice: null,
            instructions: e.instructions,
            choices: e.choices,
          },
        ];
      });
      // Déjà installés : rangés à leur place dans l'ordre de la collection.
      const kept = this.items.filter((i) => i.status === "kept");
      if (kept.length > 0) {
        const game = await api.collectionMark(
          gameId,
          detail.slug,
          kept.map((i) => [i.modId, i.rank]),
        );
        nexusStore.games[gameId] = game;
      }
      await this.#run();
    } catch (e) {
      this.error = asAppError(e);
      this.phase = this.items.length > 0 ? "paused" : "idle";
    }
  }

  async #run() {
    this.#stop = false;
    this.phase = "installing";
    for (const item of this.items) {
      if (this.#stop) {
        this.phase = "paused";
        return;
      }
      if (item.status !== "pending") continue;
      item.status = "working";
      item.error = null;
      try {
        // Titre de la page du site (compte gratuit) : le mod et son fichier.
        const title = item.fileName && item.fileName !== item.name ? `${item.name} — ${item.fileName}` : item.name;
        const outcome = await nexusStore.request(this.gameId, this.domain, item.modId, item.fileId, title, undefined, {
          slug: this.slug!,
          rank: item.rank,
        });
        const done = await this.#settle(outcome, item);
        if (done) {
          item.status = "installed";
          item.notice =
            done.notice ??
            (done.needsTool
              ? tr(`A besoin de ${done.needsTool.name} pour se charger.`, `Needs ${done.needsTool.name} to load.`)
              : null);
        } else {
          // Options abandonnées (passé), ou erreur pendant leur installation.
          item.status = item.error ? "failed" : "skipped";
        }
      } catch (e) {
        const err = asAppError(e);
        if (err.kind === "cancelled") {
          // Page du site fermée : on s'arrête là, sans perdre la place.
          item.status = "pending";
          this.phase = "paused";
          return;
        }
        item.status = "failed";
        item.error = err;
      }
    }
    this.phase = "done";
  }

  #settle(outcome: NexusInstallOutcome, item: QueueItem): Promise<NexusInstalled | null> {
    if (outcome.status === "installed") return Promise.resolve(outcome);
    return new Promise((resolve) => {
      this.fomod = { token: outcome.token, installer: outcome.installer, modId: item.modId, name: item.name, choices: item.choices, resolve };
    });
  }

  async finishFomod(selections: number[][][]): Promise<string | null> {
    const f = this.fomod;
    if (!f) return null;
    try {
      const r = await nexusStore.finishFomod(this.gameId, f.modId, f.token, selections);
      if (r.status === "choices") {
        this.fomod = { ...f, token: r.token, installer: r.installer };
        return null;
      }
      this.fomod = null;
      f.resolve(r);
      return null;
    } catch (e) {
      const err = asAppError(e);
      if (err.kind === "invalid_argument") return err.message;
      this.fomod = null;
      const item = this.items.find((i) => i.modId === f.modId && i.status === "working");
      if (item) item.error = err;
      f.resolve(null);
      return null;
    }
  }

  cancelFomod() {
    const f = this.fomod;
    if (!f) return;
    nexusStore.cancelFomod(f.token);
    this.fomod = null;
    f.resolve(null);
  }

  /** S'arrête après le mod en cours. */
  pause() {
    this.#stop = true;
  }

  /** Reprend là où l'installation s'est arrêtée (avec les mods en erreur). */
  resume() {
    if (this.busy || !this.slug) return;
    for (const i of this.items) if (i.status === "failed") i.status = "pending";
    this.error = null;
    this.#run().catch((e) => {
      this.error = asAppError(e);
      this.phase = "paused";
    });
  }

  /** Ferme le suivi d'une installation terminée ou en pause. */
  close() {
    if (this.busy) return;
    this.slug = null;
    this.items = [];
    this.plan = null;
    this.error = null;
    this.phase = "idle";
  }
}

export const collectionInstall = new CollectionInstall();
