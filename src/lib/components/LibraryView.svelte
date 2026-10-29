<script lang="ts">
  import { untrack } from "svelte";
  import { SvelteSet } from "svelte/reactivity";
  import { api, asAppError, type AppError, type Game, type Mod, type MyMods } from "$lib/api";
  import { ago, bytes, exact } from "$lib/format";
  import { nav } from "$lib/nav.svelte";
  import ErrorNotice from "./ErrorNotice.svelte";
  import Icon from "./Icon.svelte";
  import Thumb from "./Thumb.svelte";

  let data = $state<MyMods | null>(null);
  let error = $state<AppError | null>(null);
  let loading = $state(false);
  let query = $state("");
  /** Mods dont on s'est désabonné depuis l'ouverture de la page. */
  const removed = new SvelteSet<number>();
  const busy = new SvelteSet<number>();
  const failed = new SvelteSet<number>();
  let seq = 0;

  async function load() {
    const mine = ++seq;
    loading = true;
    error = null;
    try {
      const result = await api.listMyMods();
      if (mine !== seq) return;
      data = result;
      removed.clear();
      failed.clear();
    } catch (e) {
      if (mine === seq) error = asAppError(e);
    } finally {
      if (mine === seq) loading = false;
    }
  }

  // Recharge à la connexion, au changement de compte, et à chaque visite.
  $effect(() => {
    const userId = nav.auth?.user?.id;
    const visible = nav.view.name === "library";
    if (nav.loggedIn && visible) untrack(() => load());
    if (!nav.loggedIn) data = null;
    userId;
  });

  type Group = { game: Game; mods: Mod[] };

  const groups = $derived.by<Group[]>(() => {
    if (!data) return [];
    const q = query.trim().toLowerCase();
    const games = new Map(data.games.map((g) => [g.id, g]));
    const byGame = new Map<number, Mod[]>();
    for (const m of data.mods) {
      if (q && !(m.name ?? "").toLowerCase().includes(q)) continue;
      const list = byGame.get(m.game_id) ?? [];
      list.push(m);
      byGame.set(m.game_id, list);
    }
    return [...byGame.entries()]
      .map(([id, mods]) => ({
        game: games.get(id) ?? ({ id, name: `Jeu ${id}`, tag_options: [] } as unknown as Game),
        mods,
      }))
      .sort((a, b) => (a.game.name ?? "").localeCompare(b.game.name ?? "", "fr"));
  });

  const total = $derived(data ? data.mods.length - removed.size : 0);
  const gameCount = $derived(data ? new Set(data.mods.map((m) => m.game_id)).size : 0);

  async function toggle(game: Game, m: Mod) {
    busy.add(m.id);
    failed.delete(m.id);
    try {
      if (removed.has(m.id)) {
        await api.subscribe(game.id, m.id);
        removed.delete(m.id);
        if (nav.subsGameId === game.id) nav.subs.add(m.id);
      } else {
        await api.unsubscribe(game.id, m.id);
        removed.add(m.id);
        if (nav.subsGameId === game.id) nav.subs.delete(m.id);
      }
    } catch (e) {
      const err = asAppError(e);
      if (err.kind === "session_expired" || err.kind === "not_logged_in") {
        error = err;
      } else {
        failed.add(m.id);
      }
    } finally {
      busy.delete(m.id);
    }
  }

  function author(m: Mod) {
    return m.submitted_by?.username ?? m.submitted_by?.display_name_portal ?? null;
  }
</script>

<div class="view">
  <header class="head">
    <div>
      <h1>Mes mods</h1>
      {#if data}
        <p class="muted">
          {exact(total)}
          {total > 1 ? "abonnements" : "abonnement"}
          {#if gameCount > 0}dans {gameCount} {gameCount > 1 ? "jeux" : "jeu"}{/if}
        </p>
      {/if}
    </div>
    {#if nav.loggedIn && data && data.mods.length > 0}
      <div class="tools">
        <label class="search">
          <span class="visually-hidden">Filtrer mes mods</span>
          <Icon name="search" />
          <input class="field" type="search" placeholder="Filtrer mes mods" bind:value={query} />
        </label>
        <button class="btn" onclick={load} disabled={loading}>
          <Icon name="refresh" size={15} />{loading ? "Actualisation…" : "Actualiser"}
        </button>
      </div>
    {/if}
  </header>

  {#if !nav.loggedIn}
    <div class="empty">
      <p>Connecte ton compte mod.io pour retrouver tes abonnements ici.</p>
      <button class="btn btn-primary" onclick={() => nav.openAccount({ name: "library" })}>Se connecter</button>
    </div>
  {:else if error}
    <ErrorNotice {error} onretry={load} />
  {:else if !data}
    <div class="skeleton" aria-busy="true" aria-label="Chargement de tes abonnements">
      {#each Array(5) as _, i (i)}<div class="sk-row"></div>{/each}
    </div>
  {:else if data.mods.length === 0}
    <div class="empty">
      <p>Tu n'es abonné à aucun mod pour l'instant. Ouvre un jeu, choisis un mod et clique sur « S'abonner ».</p>
      <button class="btn btn-primary" onclick={() => nav.openGames()}>Parcourir les jeux</button>
    </div>
  {:else}
    <p class="hint muted">Les jeux qui gèrent mod.io téléchargent ces mods à leur prochain lancement.</p>

    {#if groups.length === 0}
      <div class="empty">
        <p>Aucun de tes mods ne correspond à « {query.trim()} ».</p>
        <button class="btn" onclick={() => (query = "")}>Effacer le filtre</button>
      </div>
    {/if}

    {#each groups as g (g.game.id)}
      <section class="group">
        <div class="group-head">
          {#if g.game.icon?.thumb_64x64}
            <img class="game-icon" src={g.game.icon.thumb_64x64} alt="" width="28" height="28" />
          {/if}
          <h2>{g.game.name}</h2>
          <span class="muted count">{g.mods.length}</span>
          <button class="btn btn-ghost browse" onclick={() => nav.openGame(g.game)}>Parcourir ce jeu</button>
        </div>

        <ul class="rows">
          {#each g.mods as m (m.id)}
            {@const gone = removed.has(m.id)}
            <li class="row" class:gone>
              <button class="row-main" onclick={() => nav.openModFromLibrary(g.game, m.id)}>
                <span class="row-thumb">
                  <Thumb src={m.logo?.thumb_320x180 ?? m.logo?.original} label={m.name ?? ""} />
                </span>
                <span class="row-text">
                  <span class="row-name">{m.name ?? "Sans nom"}</span>
                  <span class="row-meta">
                    {#if author(m)}<span>par {author(m)}</span>{/if}
                    <span>mis à jour {ago(m.date_updated)}</span>
                    {#if m.modfile?.version}<span>version {m.modfile.version}</span>{/if}
                    {#if m.modfile?.filesize}<span>{bytes(m.modfile.filesize)}</span>{/if}
                  </span>
                </span>
              </button>
              <div class="row-action">
                {#if failed.has(m.id)}<span class="row-error">Échec, réessaie</span>{/if}
                {#if gone}<span class="muted small">Désabonné</span>{/if}
                <button class="btn" class:btn-ghost={!gone} onclick={() => toggle(g.game, m)} disabled={busy.has(m.id)}>
                  {#if busy.has(m.id)}…{:else if gone}Se réabonner{:else}Se désabonner{/if}
                </button>
              </div>
            </li>
          {/each}
        </ul>
      </section>
    {/each}

    {#if data.truncated}
      <p class="muted small">Seuls les 3 000 premiers abonnements sont affichés.</p>
    {/if}
  {/if}
</div>

<style>
  .view {
    height: 100%;
    overflow-y: auto;
    padding: 2.25rem 2.5rem 3rem;
    display: grid;
    align-content: start;
    gap: 1.25rem;
  }

  .head {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-end;
    justify-content: space-between;
    gap: 1.25rem 2rem;
  }

  h1 {
    font-size: var(--t-3xl);
    font-weight: 800;
    margin-bottom: 0.3rem;
  }

  .tools {
    display: flex;
    gap: 0.6rem;
  }

  .search {
    position: relative;
    display: block;
    width: min(20rem, 40vw);
    color: var(--muted);
  }

  .search :global(svg) {
    position: absolute;
    left: 0.75rem;
    top: 50%;
    translate: 0 -50%;
    pointer-events: none;
  }

  .search .field {
    padding-left: 2.2rem;
  }

  .hint {
    font-size: var(--t-sm);
    margin-top: -0.5rem;
  }

  .group {
    display: grid;
    gap: 0.6rem;
    margin-top: 1rem;
  }

  .group-head {
    display: flex;
    align-items: center;
    gap: 0.65rem;
  }

  .game-icon {
    border-radius: var(--r-sm);
  }

  h2 {
    font-size: var(--t-xl);
    font-weight: 700;
  }

  .count {
    font-size: var(--t-md);
  }

  .browse {
    margin-left: auto;
    color: var(--accent);
  }

  .rows {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 0.4rem;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 1rem;
    padding: 0.5rem 0.75rem 0.5rem 0.5rem;
    border-radius: var(--r-md);
    background: var(--panel);
  }

  .row.gone .row-main {
    opacity: 0.5;
  }

  .row-main {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 1rem;
    padding: 0;
    border: 0;
    background: none;
    text-align: left;
  }

  .row-main:hover .row-name {
    color: var(--accent);
  }

  .row-thumb {
    flex: none;
    width: 7.5rem;
  }

  .row-thumb :global(.thumb) {
    aspect-ratio: 16 / 9;
    border-radius: var(--r-sm);
    overflow: hidden;
    background: var(--raise);
    position: relative;
  }

  .row-thumb :global(.thumb img) {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .row-thumb :global(.fallback) {
    font-size: var(--t-xl);
  }

  .row-text {
    display: grid;
    gap: 0.2rem;
    min-width: 0;
  }

  .row-name {
    font-family: var(--font-display);
    font-weight: 650;
    font-size: var(--t-lg);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .row-meta {
    display: flex;
    flex-wrap: wrap;
    gap: 0.1rem 0.9rem;
    font-size: var(--t-sm);
    color: var(--muted);
  }

  .row-action {
    display: flex;
    align-items: center;
    gap: 0.6rem;
    flex: none;
  }

  .row-error {
    color: var(--danger);
    font-size: var(--t-sm);
    font-weight: 600;
  }

  .small {
    font-size: var(--t-sm);
  }

  .empty {
    display: grid;
    justify-items: start;
    gap: 1rem;
    padding: 1rem 0;
    max-width: 40rem;
  }

  .skeleton {
    display: grid;
    gap: 0.4rem;
  }

  .sk-row {
    height: 4.6rem;
    border-radius: var(--r-md);
    background: var(--panel);
  }
</style>
