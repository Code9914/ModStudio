// Recherche globale : un mot cherché en même temps dans toutes les sources.
// Nexus Mods cherche dans tous ses jeux ; Thunderstore et mod.io, dans les jeux
// du joueur (avec des mods, ou installés sur ce PC) ; les listes des
// communautés, dans toutes leurs listes (peu nombreuses, gardées en mémoire).
import {
  api,
  asAppError,
  nexus,
  nexusGameImage,
  ts,
  type AppError,
  type ClGame,
  type ClMod,
  type Game,
  type Mod,
  type NexusGame,
  type NexusMod,
  type RmlMod,
  type TsCommunity,
  type TsPackage,
} from "./api";
import { exact } from "./format";
import { tr } from "./i18n.svelte";
import { installs } from "./installs.svelte";
import { listsStore } from "./lists.svelte";
import { nav } from "./nav.svelte";
import { nexusStore } from "./nexus.svelte";
import { normalizeName, tsStore } from "./thunderstore.svelte";

export type SearchSource = "modio" | "nexus" | "ts" | "cl";

export const searchSources: Record<SearchSource, string> = {
  modio: "mod.io",
  nexus: "Nexus Mods",
  ts: "Thunderstore",
  cl: tr("Communautés", "Communities"),
};

export interface SearchGame {
  key: string;
  source: SearchSource;
  name: string;
  image: string | null;
  detail: string;
  score: number;
  open: () => void;
}

export interface SearchHit {
  key: string;
  name: string;
  summary: string;
  author: string | null;
  image: string | null;
  /** Jeu du mod, quand le groupe en mélange plusieurs (Nexus). */
  game: string | null;
  downloads: number | null;
  /** « Obsolète », « Cassé »… */
  flag: string | null;
  installed: () => boolean;
  open: () => void;
}

export interface SearchGroup {
  key: string;
  source: SearchSource;
  title: string;
  image: string | null;
  /** Jeu du joueur (mods installés ou jeu sur ce PC) : montré même pendant le chargement. */
  mine: boolean;
  loading: boolean;
  error: AppError | null;
  hits: SearchHit[];
  /** Résultats affichés (les suivants avec « Plus de résultats »). */
  shown: number;
  total: number;
  /** Ouvre la liste du jeu sur cette recherche (absent : « Plus de résultats » ici). */
  openList: (() => void) | null;
}

const MIN = 2;
const PER_GAME = 5;
const NEXUS_PAGE = 24;
const NEXUS_SHOWN = 6;
const MAX_TS = 6;
const MAX_MODIO = 4;

/** 3 : nom identique, 2 : commence par, 1 : contient, 0 : non. */
export function nameScore(name: string, term: string): number {
  const n = normalizeName(name);
  const t = normalizeName(term);
  if (!t || !n) return 0;
  if (n === t) return 3;
  if (n.startsWith(t)) return 2;
  return n.includes(t) ? 1 : 0;
}

/** Pour les listes en mémoire : le nom d'abord, sinon tous les mots dans la description ou l'auteur. */
function localScore(m: { name: string; description: string; author: string }, term: string): number {
  const byName = nameScore(m.name, term);
  if (byName) return byName;
  const words = term.split(/\s+/).map(normalizeName).filter((w) => w.length >= 2);
  if (!words.length) return 0;
  const hay = normalizeName(`${m.name} ${m.author} ${m.description}`);
  return words.every((w) => hay.includes(w)) ? 0.5 : 0;
}

function localSearch<T extends { name: string; description: string; author: string }>(list: T[], term: string): T[] {
  return list
    .map((m) => ({ m, s: localScore(m, term) }))
    .filter((x) => x.s > 0)
    .sort((a, b) => b.s - a.s)
    .map((x) => x.m);
}

function steamCapsule(appId: number) {
  return `https://cdn.cloudflare.steamstatic.com/steam/apps/${appId}/capsule_231x87.jpg`;
}

class Search {
  /** Ce qui est tapé. */
  query = $state("");
  /** Ce qui est cherché (après une courte pause de frappe). */
  term = $state("");
  games = $state<SearchGame[]>([]);
  groups = $state<SearchGroup[]>([]);
  #seq = 0;
  #timer: ReturnType<typeof setTimeout> | undefined;
  /** Fiches des jeux mod.io (pour ouvrir un mod). */
  #modioGames = new Map<number, Game>();

  get busy() {
    return this.groups.some((g) => g.loading);
  }

  /** À chaque frappe : la recherche part après une courte pause. */
  type(value: string) {
    this.query = value;
    clearTimeout(this.#timer);
    const next = value.trim();
    if (next.length < MIN) {
      this.#seq++;
      this.term = "";
      this.games = [];
      this.groups = [];
      return;
    }
    this.#timer = setTimeout(() => {
      if (next !== this.term) this.run(next);
    }, 350);
  }

  /** Relance la recherche en cours (après une erreur). */
  retry() {
    if (this.term) this.run(this.term);
  }

  run(term: string) {
    const seq = ++this.#seq;
    this.term = term;
    this.games = [];
    this.groups = [];
    this.#searchGames(seq, term);
    this.#searchMods(seq, term);
  }

  // ------------------------------------------------------------ Groupes

  #add(seq: number, group: Omit<SearchGroup, "loading" | "error" | "hits" | "total" | "shown"> & { shown?: number }) {
    if (seq !== this.#seq) return;
    this.groups.push({ loading: true, error: null, hits: [], total: 0, shown: PER_GAME, ...group });
  }

  #group(seq: number, key: string): SearchGroup | null {
    if (seq !== this.#seq) return null;
    return this.groups.find((g) => g.key === key) ?? null;
  }

  async #fill(seq: number, key: string, load: () => Promise<{ hits: SearchHit[]; total: number }>) {
    try {
      const { hits, total } = await load();
      const g = this.#group(seq, key);
      if (!g) return;
      g.hits = hits;
      g.total = total;
    } catch (e) {
      const g = this.#group(seq, key);
      if (g) g.error = asAppError(e);
    } finally {
      const g = this.#group(seq, key);
      if (g) g.loading = false;
    }
  }

  // ------------------------------------------------------------ Jeux

  async #searchGames(seq: number, term: string) {
    const found: SearchGame[] = [];
    const push = (list: SearchGame[]) => {
      if (seq !== this.#seq) return;
      found.push(...list);
      // Les plus proches du mot d'abord, puis l'ordre des sources.
      this.games = [...found].sort((a, b) => b.score - a.score).slice(0, 12);
    };
    const tasks: Promise<void>[] = [];

    tasks.push(
      tsStore.loadCommunities().then(() => {
        const list = (tsStore.communities ?? [])
          .map((c) => ({ c, s: nameScore(c.name, term) }))
          .filter((x) => x.s > 0)
          .slice(0, 6);
        push(
          list.map(({ c, s }) => ({
            key: `ts:${c.identifier}`,
            source: "ts" as const,
            name: c.name,
            image: c.communityIconUrl ?? c.coverImageUrl,
            detail: `${exact(c.totalPackageCount)} mods`,
            score: s,
            open: () => nav.openTsGame(c),
          })),
        );
      }),
    );

    tasks.push(
      (listsStore.loaded ? Promise.resolve() : listsStore.load()).then(() => {
        push(
          listsStore.games
            .map((g) => ({ g, s: nameScore(g.name, term) }))
            .filter((x) => x.s > 0)
            .map(({ g, s }) => ({
              key: `cl:${g.id}`,
              source: "cl" as const,
              name: g.name,
              image: steamCapsule(g.steamAppId),
              detail: g.listName,
              score: s,
              open: () => nav.openClGame(g),
            })),
        );
      }),
    );

    tasks.push(
      nexus.listGames({ query: term, sort: "downloads", offset: 0, limit: 6 }).then((page) => {
        push(
          page.data.map((g) => ({
            key: `nexus:${g.id}`,
            source: "nexus" as const,
            name: g.name,
            image: nexusGameImage(g.id, "thumbnail"),
            detail: `${exact(g.modCount)} mods`,
            score: nameScore(g.name, term) || 0.5,
            open: () => nav.openNexusGame(g),
          })),
        );
      }),
    );

    if (nav.settings?.hasApiKey) {
      tasks.push(
        api.listGames({ query: term, offset: 0, limit: 6 }).then((page) => {
          for (const g of page.data) this.#modioGames.set(g.id, g);
          push(
            page.data.map((g) => ({
              key: `modio:${g.id}`,
              source: "modio" as const,
              name: g.name ?? "",
              image: g.icon?.thumb_64x64 ?? g.logo?.thumb_320x180 ?? null,
              detail: `${exact(g.stats?.mods_count_total ?? 0)} mods`,
              score: nameScore(g.name ?? "", term) || 0.5,
              open: () => nav.openGame(g),
            })),
          );
        }),
      );
    }

    // Une source injoignable n'empêche pas les autres de répondre.
    await Promise.allSettled(tasks);
  }

  // ------------------------------------------------------------ Mods

  async #searchMods(seq: number, term: string) {
    // Jeux trouvés sur ce PC : ils décident où chercher (Thunderstore, listes).
    if (nexusStore.detected === null) await nexusStore.detectGames().catch(() => {});
    await Promise.allSettled([tsStore.loadCommunities(), listsStore.loaded ? null : listsStore.load()]);
    if (seq !== this.#seq) return;

    const communities = myCommunities();
    for (const { community, mine } of communities) this.#tsGroup(seq, term, community, mine);
    const resonite = communities.find((c) => c.community.identifier === "resonite");
    if (resonite) this.#rmlGroup(seq, term, resonite.community);
    if (nav.settings?.hasApiKey) for (const g of myModioGames()) this.#modioGroup(seq, term, g);
    this.#nexusGroup(seq, term);
    for (const game of listsStore.games) this.#clGroup(seq, term, game);
  }

  #nexusGroup(seq: number, term: string) {
    const key = "nexus";
    this.#add(seq, { key, source: "nexus", title: tr("Nexus Mods, tous les jeux", "Nexus Mods, all games"), image: null, mine: true, openList: null, shown: NEXUS_SHOWN });
    // Les jeux du joueur d'abord, puis l'ordre de Nexus (mentions « J'aime »).
    const mineIds = new Set<number>([
      ...Object.values(nexusStore.games).filter((g) => nexusStore.count(g) > 0).map((g) => g.gameId),
      ...(nexusStore.detected ?? []).flatMap((d) => (d.nexus ? [d.nexus.id] : [])),
    ]);
    this.#fill(seq, key, async () => {
      const page = await nexus.searchMods(term, 0, NEXUS_PAGE);
      const sorted = [...page.data].sort((a, b) => Number(mineIds.has(b.game?.id ?? 0)) - Number(mineIds.has(a.game?.id ?? 0)));
      return { hits: sorted.map(nexusHit), total: page.result_total };
    });
  }

  /** « Plus de résultats » : les suivants, puis la page suivante de Nexus. */
  async more(group: SearchGroup) {
    if (group.shown < group.hits.length) {
      group.shown += NEXUS_SHOWN;
      return;
    }
    if (group.source !== "nexus" || group.hits.length >= group.total || group.loading) return;
    const seq = this.#seq;
    group.loading = true;
    try {
      const page = await nexus.searchMods(this.term, group.hits.length, NEXUS_PAGE);
      const g = this.#group(seq, group.key);
      if (!g) return;
      const seen = new Set(g.hits.map((h) => h.key));
      g.hits = [...g.hits, ...page.data.map(nexusHit).filter((h) => !seen.has(h.key))];
      g.shown += NEXUS_SHOWN;
    } catch (e) {
      const g = this.#group(seq, group.key);
      if (g) g.error = asAppError(e);
    } finally {
      const g = this.#group(seq, group.key);
      if (g) g.loading = false;
    }
  }

  #tsGroup(seq: number, term: string, c: TsCommunity, mine: boolean) {
    const key = `ts:${c.identifier}`;
    this.#add(seq, {
      key,
      source: "ts",
      title: c.name,
      image: c.communityIconUrl ?? c.coverImageUrl,
      mine,
      openList: () => {
        nav.listSearch = { list: "tsMods", game: c.id, query: term };
        nav.openTsGame(c);
      },
    });
    this.#fill(seq, key, async () => {
      const page = await ts.packages({ community: c.identifier, query: term, ordering: "most-downloaded", offset: 0, limit: PER_GAME * 2 });
      const list = nav.settings?.hideMature ? page.data.filter((p) => !p.isNsfw) : page.data;
      return { hits: list.map((p) => tsHit(c, p)), total: page.result_total };
    });
  }

  #rmlGroup(seq: number, term: string, c: TsCommunity) {
    const key = "rml";
    this.#add(seq, {
      key,
      source: "ts",
      title: "Resonite · ResoniteModLoader",
      image: c.communityIconUrl ?? c.coverImageUrl,
      mine: true,
      openList: () => {
        nav.listSearch = { list: "rml", game: c.id, query: term };
        nav.openTsGame(c);
        nav.tsTab = "rml";
      },
    });
    this.#fill(seq, key, async () => {
      await tsStore.loadRml();
      if (tsStore.rmlError && !tsStore.rmlMods) throw tsStore.rmlError;
      const found = localSearch(tsStore.rmlMods ?? [], term);
      return { hits: found.map((m) => rmlHit(c, m)), total: found.length };
    });
  }

  #modioGroup(seq: number, term: string, g: { gameId: number; gameName: string; icon: string | null }) {
    const key = `modio:${g.gameId}`;
    const game = async () => {
      let full = this.#modioGames.get(g.gameId);
      if (!full) {
        full = await api.getGame(g.gameId);
        this.#modioGames.set(g.gameId, full);
      }
      return full;
    };
    this.#add(seq, {
      key,
      source: "modio",
      title: g.gameName,
      image: g.icon,
      mine: true,
      openList: () => {
        game()
          .then((full) => {
            nav.listSearch = { list: "mods", game: full.id, query: term };
            nav.openGame(full);
          })
          .catch(() => nav.openInstalled());
      },
    });
    this.#fill(seq, key, async () => {
      const page = await api.listMods({ gameId: g.gameId, query: term, sort: "-downloads_total", offset: 0, limit: PER_GAME * 2 });
      return { hits: page.data.map((m) => modioHit(g.gameId, m, game)), total: page.result_total };
    });
  }

  #clGroup(seq: number, term: string, game: ClGame) {
    const key = `cl:${game.id}`;
    const mine = !!listsStore.config(game) || !!listsStore.detectedFor(game);
    this.#add(seq, {
      key,
      source: "cl",
      title: game.name,
      image: steamCapsule(game.steamAppId),
      mine,
      openList: () => {
        nav.listSearch = { list: "clMods", game: game.id, query: term };
        nav.openClGame(game);
      },
    });
    this.#fill(seq, key, async () => {
      await listsStore.loadMods(game);
      const error = listsStore.errors[game.id];
      if (error && !listsStore.mods[game.id]) throw error;
      const found = localSearch(listsStore.mods[game.id] ?? [], term);
      return { hits: found.map((m) => clHit(game, m)), total: found.length };
    });
  }
}

// ---------------------------------------------------------------- Portée

/** Jeux Thunderstore du joueur : avec des mods ou configurés, puis trouvés sur ce PC. */
function myCommunities(): { community: TsCommunity; mine: boolean }[] {
  const out = new Map<number, { community: TsCommunity; weight: number }>();
  for (const g of Object.values(tsStore.games)) {
    if (!g.domain) continue;
    const c = tsStore.communityOf(g);
    out.set(c.id, { community: c, weight: 1000 + tsStore.count(g) });
  }
  const byName = new Map((tsStore.communities ?? []).map((c) => [normalizeName(c.name), c]));
  for (const d of nexusStore.detected ?? []) {
    const c = byName.get(normalizeName(d.name));
    if (c && c.supported && !out.has(c.id)) out.set(c.id, { community: c, weight: 0 });
  }
  return [...out.values()]
    .sort((a, b) => b.weight - a.weight)
    .slice(0, MAX_TS)
    .map(({ community }) => ({ community, mine: true }));
}

/** Jeux mod.io où des mods sont installés. */
function myModioGames() {
  return Object.values(installs.games)
    .filter((g) => Object.keys(g.mods).length > 0)
    .sort((a, b) => Object.keys(b.mods).length - Object.keys(a.mods).length)
    .slice(0, MAX_MODIO);
}

// ---------------------------------------------------------------- Résultats

function nexusGameOfMod(m: NexusMod): NexusGame | null {
  if (!m.game) return null;
  return { id: m.game.id, name: m.game.name ?? m.game.domainName, domainName: m.game.domainName, modCount: 0, downloadCount: 0, genre: null };
}

function nexusHit(m: NexusMod): SearchHit {
  const game = nexusGameOfMod(m);
  return {
    key: `${m.game?.id ?? 0}:${m.modId}`,
    name: m.name,
    summary: m.summary ?? "",
    author: m.author ?? m.uploader?.name ?? null,
    image: m.thumbnailUrl ?? m.pictureUrl,
    game: game?.name ?? null,
    downloads: m.downloads,
    flag: m.adultContent ? tr("Adulte", "Adult") : null,
    installed: () => (game ? !!nexusStore.installed(game.id, m.modId) : false),
    open: () => game && nav.openNexusModFrom(game, m.modId, "search"),
  };
}

function tsHit(c: TsCommunity, p: TsPackage): SearchHit {
  return {
    key: `${p.namespace}-${p.name}`,
    name: p.name.replaceAll("_", " "),
    summary: p.description,
    author: p.namespace,
    image: p.iconUrl,
    game: null,
    downloads: p.downloadCount,
    flag: p.isDeprecated ? tr("Obsolète", "Outdated") : p.isNsfw ? tr("Adulte", "Adult") : null,
    installed: () => !!tsStore.installed(c.id, p.id),
    open: () => nav.openTsModFrom(c, { namespace: p.namespace, name: p.name }, "search"),
  };
}

function rmlHit(c: TsCommunity, m: RmlMod): SearchHit {
  return {
    key: m.id,
    name: m.name,
    summary: m.description,
    author: m.author,
    image: m.authorIcon ?? m.authorAvatar,
    game: null,
    downloads: null,
    flag: m.broken ? tr("Cassé", "Broken") : m.deprecated ? tr("Obsolète", "Outdated") : null,
    installed: () => !!tsStore.installed(c.id, m.modId),
    open: () => nav.openRmlModFrom(c, m.id, "search"),
  };
}

function modioHit(gameId: number, m: Mod, game: () => Promise<Game>): SearchHit {
  return {
    key: String(m.id),
    name: m.name ?? "",
    summary: m.summary ?? "",
    author: m.submitted_by?.username ?? null,
    image: m.logo?.thumb_320x180 ?? null,
    game: null,
    downloads: m.stats?.downloads_total ?? null,
    flag: null,
    installed: () => !!installs.installed(gameId, m.id),
    open: () => {
      game()
        .then((full) => nav.openModFromLibrary(full, m.id, "search"))
        .catch(() => nav.openInstalled());
    },
  };
}

function clHit(game: ClGame, m: ClMod): SearchHit {
  return {
    key: m.id,
    name: m.name,
    summary: m.description,
    author: m.author,
    image: m.icon ?? m.iconFallback,
    game: null,
    downloads: m.downloads,
    flag: m.broken ? tr("Cassé", "Broken") : m.deprecated ? tr("Obsolète", "Outdated") : null,
    installed: () => !!listsStore.installed(game.gameId, m.modId),
    open: () => nav.openClModFrom(game, m.id, "search"),
  };
}

export const search = new Search();
