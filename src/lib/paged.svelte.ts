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

  constructor(fetch: (offset: number, limit: number) => Promise<Page<T>>, limit = 30) {
    this.#fetch = fetch;
    this.#limit = limit;
  }

  get hasMore() {
    return this.items.length < this.total;
  }

  reload() {
    this.items = [];
    this.total = 0;
    this.loaded = false;
    return this.#load(0, true);
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
