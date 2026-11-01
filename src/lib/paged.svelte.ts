// Liste paginée « afficher plus » avec protection contre les réponses
// arrivées dans le désordre (une recherche tapée vite ne doit pas
// afficher les résultats d'une frappe précédente).
import { asAppError, type AppError, type Page } from "./api";

export class PagedList<T extends { id: number }> {
  items = $state<T[]>([]);
  total = $state(0);
  loading = $state(false);
  error = $state<AppError | null>(null);
  /** Vrai une fois la première page reçue (évite un « aucun résultat » prématuré). */
  loaded = $state(false);

  #seq = 0;
  #fetch: (offset: number, limit: number) => Promise<Page<T>>;
  #limit: number;
  #cacheKey: (() => string | null) | null;

  /**
   * `cacheKey` : nom sous lequel garder la première page sur ce PC, pour
   * l'afficher tout de suite au prochain lancement pendant que la liste se
   * met à jour (`null` : pas de cache pour cette recherche).
   */
  constructor(fetch: (offset: number, limit: number) => Promise<Page<T>>, limit = 30, cacheKey: (() => string | null) | null = null) {
    this.#fetch = fetch;
    this.#limit = limit;
    this.#cacheKey = cacheKey;
  }

  get hasMore() {
    return this.items.length < this.total;
  }

  reload() {
    const cached = this.#readCache();
    this.items = cached?.data ?? [];
    this.total = cached?.total ?? 0;
    this.loaded = !!cached;
    return this.#load(0, true);
  }

  #storageKey(): string | null {
    const key = this.#cacheKey?.();
    return key ? `modstudio.cache.${key}` : null;
  }

  #readCache(): { data: T[]; total: number } | null {
    const key = this.#storageKey();
    if (!key) return null;
    try {
      const v = JSON.parse(localStorage.getItem(key) ?? "null");
      return v && Array.isArray(v.data) && typeof v.total === "number" ? v : null;
    } catch {
      return null;
    }
  }

  #writeCache(page: Page<T>) {
    const key = this.#storageKey();
    if (!key) return;
    try {
      localStorage.setItem(key, JSON.stringify({ data: page.data, total: page.result_total }));
    } catch {
      /* cache plein ou indisponible : la liste se charge simplement depuis le site */
    }
  }

  more() {
    if (this.loading || this.error || !this.hasMore) return;
    return this.#load(this.items.length, false);
  }

  retry() {
    this.error = null;
    return this.#load(this.items.length, this.items.length === 0);
  }

  async #load(offset: number, replace: boolean) {
    const seq = ++this.#seq;
    this.loading = true;
    this.error = null;
    try {
      const page = await this.#fetch(offset, this.#limit);
      if (seq !== this.#seq) return;
      if (replace) {
        this.items = page.data;
        if (offset === 0) this.#writeCache(page);
      } else {
        // Un mod publié entre deux pages décale les résultats : on dédoublonne.
        const seen = new Set(this.items.map((i) => i.id));
        this.items = [...this.items, ...page.data.filter((i) => !seen.has(i.id))];
      }
      this.total = page.result_total;
      this.loaded = true;
    } catch (e) {
      if (seq !== this.#seq) return;
      this.error = asAppError(e);
    } finally {
      if (seq === this.#seq) this.loading = false;
    }
  }
}

/** Action Svelte : appelle `callback` quand l'élément devient visible. */
export function inview(node: HTMLElement, callback: () => void) {
  let cb = callback;
  const observer = new IntersectionObserver(
    (entries) => {
      if (entries.some((e) => e.isIntersecting)) cb();
    },
    { rootMargin: "400px 0px" },
  );
  observer.observe(node);
  return {
    update(next: () => void) {
      cb = next;
    },
    destroy() {
      observer.disconnect();
    },
  };
}
