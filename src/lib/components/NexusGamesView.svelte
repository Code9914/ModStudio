<script lang="ts">
  import { untrack } from "svelte";
  import { nexus, nexusGameImage, type NexusGame, type NexusGameSort } from "$lib/api";
  import { count, exact } from "$lib/format";
  import { nav } from "$lib/nav.svelte";
  import { nexusStore } from "$lib/nexus.svelte";
  import { PagedList } from "$lib/paged.svelte";
  import ErrorNotice from "./ErrorNotice.svelte";
  import Icon from "./Icon.svelte";
  import LoadMore from "./LoadMore.svelte";
  import Thumb from "./Thumb.svelte";

  let query = $state("");
  let search = $state("");
  let sort = $state<NexusGameSort>("downloads");

  const list = new PagedList<NexusGame>(
    (offset, limit) => nexus.listGames({ query: search, sort, offset, limit }),
    48,
  );

  $effect(() => {
    const q = query.trim();
    const t = setTimeout(() => (search = q), 350);
    return () => clearTimeout(t);
  });

  $effect(() => {
    search;
    sort;
    untrack(() => list.reload());
  });

  /** Jeux où des mods Nexus sont déjà installés : affichés en premier. */
  const mine = $derived(Object.values(nexusStore.games).filter((g) => Object.keys(g.mods).length > 0));
</script>

<div class="view">
  <header class="head">
    <div>
      <p class="source">Nexus Mods</p>
      <h1>Jeux</h1>
      {#if list.loaded}
        <p class="muted">{exact(list.total)} jeux {search ? `pour « ${search} »` : "sur Nexus Mods"}</p>
      {/if}
    </div>
    <div class="tools">
      <label class="search">
        <span class="visually-hidden">Rechercher un jeu sur Nexus Mods</span>
        <Icon name="search" />
        <input class="field" type="search" placeholder="Rechercher un jeu" bind:value={query} />
      </label>
      <label>
        <span class="visually-hidden">Trier les jeux</span>
        <select class="field" bind:value={sort}>
          <option value="downloads">Plus téléchargés</option>
          <option value="mods">Plus de mods</option>
          <option value="name">Nom (A à Z)</option>
        </select>
      </label>
    </div>
  </header>

  {#if !nexusStore.ready && nexusStore.authLoaded}
    <div class="key-hint">
      <p>
        Tu peux parcourir Nexus Mods sans compte. Pour installer des mods, ajoute ta clé API Nexus Mods : elle se
        crée en deux clics sur le site.
      </p>
      <button class="btn btn-primary" onclick={() => nav.openNexusAccount(nav.view)}>Ajouter ma clé</button>
    </div>
  {/if}

  {#if mine.length > 0 && !search}
    <section class="mine" aria-label="Jeux avec des mods installés">
      <h2>Tes jeux</h2>
      <div class="mine-list">
        {#each mine as g (g.gameId)}
          <button
            class="chip mine-chip"
            onclick={() =>
              nav.openNexusGame({
                id: g.gameId,
                name: g.gameName,
                domainName: g.domain ?? "",
                modCount: 0,
                downloadCount: 0,
                genre: null,
              })}
          >
            <img src={nexusGameImage(g.gameId, "thumbnail")} alt="" width="18" height="18" />
            {g.gameName}
            <span class="muted">{Object.keys(g.mods).length}</span>
          </button>
        {/each}
      </div>
    </section>
  {/if}

  {#if list.error && list.items.length === 0}
    <ErrorNotice error={list.error} onretry={() => list.retry()} />
  {:else if !list.loaded}
    <ul class="card-grid posters" aria-busy="true" aria-label="Chargement des jeux">
      {#each Array(12) as _, i (i)}
        <li class="skeleton"><div class="thumb"></div><div class="line"></div></li>
      {/each}
    </ul>
  {:else if list.items.length === 0}
    <div class="empty">
      <p>Aucun jeu ne correspond à « {search} ».</p>
      <button class="btn" onclick={() => (query = "")}>Effacer la recherche</button>
    </div>
  {:else}
    <!-- Affiches en portrait : l'image « hero » de Nexus est volontairement
         floue pour beaucoup de jeux (fond de page), l'affiche est nette. -->
    <ul class="card-grid posters">
      {#each list.items as game (game.id)}
        <li>
          <button class="card" onclick={() => nav.openNexusGame(game)}>
            <Thumb src={nexusGameImage(game.id, "tile")} label={game.name} />
            <span class="card-title">{game.name}</span>
            <span class="card-meta">
              <span>{exact(game.modCount)} mods</span>
              <span title="Téléchargements"><Icon name="download" size={14} />{count(game.downloadCount)}</span>
            </span>
          </button>
        </li>
      {/each}
    </ul>
  {/if}

  {#if list.items.length > 0}<LoadMore {list} noun="jeux" />{/if}
</div>

<style>
  .view {
    height: 100%;
    overflow-y: auto;
    padding: 2.25rem 2.5rem 3rem;
  }

  .head {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-end;
    justify-content: space-between;
    gap: 1.25rem 2rem;
    margin-bottom: 2rem;
  }

  .source {
    font-size: var(--t-xs);
    font-weight: 700;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--accent);
    margin-bottom: 0.2rem;
  }

  h1 {
    font-size: var(--t-3xl);
    font-weight: 800;
    margin-bottom: 0.3rem;
  }

  h2 {
    font-size: var(--t-md);
    font-weight: 700;
    margin-bottom: 0.6rem;
  }

  .tools {
    display: flex;
    gap: 0.6rem;
  }

  .search {
    position: relative;
    display: block;
    width: min(22rem, 50vw);
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

  .key-hint {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.75rem 1.25rem;
    padding: 0.9rem 1.1rem;
    margin-bottom: 1.75rem;
    border-radius: var(--r-md);
    background: var(--panel);
    max-width: 52rem;
  }

  .key-hint p {
    flex: 1 1 24rem;
    font-size: var(--t-sm);
  }

  .mine {
    margin-bottom: 2rem;
  }

  .mine-list {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
  }

  .mine-chip {
    display: inline-flex;
    align-items: center;
    gap: 0.5rem;
    padding-block: 0.35rem;
  }

  .mine-chip img {
    border-radius: 3px;
  }

  .posters {
    grid-template-columns: repeat(auto-fill, minmax(135px, 1fr));
    gap: 1.5rem 1.1rem;
  }

  .posters :global(.thumb) {
    aspect-ratio: 2 / 3;
  }

  .posters :global(.card-title) {
    font-size: var(--t-md);
  }

  .skeleton .thumb {
    aspect-ratio: 2 / 3;
    border-radius: var(--r-md);
    background: var(--raise);
  }

  .skeleton .line {
    height: 1rem;
    width: 60%;
    margin-top: 0.7rem;
    border-radius: var(--r-sm);
    background: var(--raise);
  }

  .empty {
    display: grid;
    justify-items: start;
    gap: 1rem;
    padding: 2rem 0;
  }
</style>
