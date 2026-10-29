<script lang="ts">
  import { untrack } from "svelte";
  import { api, gameClosed, type Game, type GameSort } from "$lib/api";
  import { count, exact } from "$lib/format";
  import { nav } from "$lib/nav.svelte";
  import { PagedList } from "$lib/paged.svelte";
  import Icon from "./Icon.svelte";
  import LoadMore from "./LoadMore.svelte";
  import Thumb from "./Thumb.svelte";

  let query = $state("");
  let search = $state("");
  let sort = $state<GameSort>("name");

  const list = new PagedList<Game>(
    (offset, limit) => api.listGames({ query: search, sort, offset, limit }),
    48,
  );

  // Recherche : on attend une courte pause dans la frappe.
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

  function logo(g: Game) {
    return g.logo?.thumb_640x360 ?? g.logo?.thumb_320x180 ?? g.logo?.original;
  }
</script>

<div class="view">
  <header class="head">
    <div>
      <h1>Jeux</h1>
      {#if list.loaded}
        <p class="muted">{exact(list.total)} jeux {search ? `pour « ${search} »` : "sur mod.io"}</p>
      {/if}
    </div>
    <div class="tools">
      <label class="search">
        <span class="visually-hidden">Rechercher un jeu</span>
        <Icon name="search" />
        <input class="field" type="search" placeholder="Rechercher un jeu" bind:value={query} />
      </label>
      <label>
        <span class="visually-hidden">Trier les jeux</span>
        <select class="field" bind:value={sort}>
          <option value="name">Nom (A à Z)</option>
          <option value="-date_updated">Récemment mis à jour</option>
        </select>
      </label>
    </div>
  </header>

  {#if !list.loaded && !list.error}
    <ul class="card-grid" aria-busy="true" aria-label="Chargement des jeux">
      {#each Array(12) as _, i (i)}
        <li class="skeleton"><div class="thumb"></div><div class="line"></div></li>
      {/each}
    </ul>
  {:else if list.loaded && list.items.length === 0}
    <div class="empty">
      <p>Aucun jeu ne correspond à « {search} ».</p>
      <button class="btn" onclick={() => (query = "")}>Effacer la recherche</button>
    </div>
  {:else}
    <ul class="card-grid">
      {#each list.items as game (game.id)}
        <li>
          <button class="card" onclick={() => nav.openGame(game)}>
            <Thumb
              src={logo(game)}
              label={game.name ?? ""}
              badges={gameClosed(game) ? [{ text: "Sur mod.io uniquement", tone: "neutral" }] : []}
            />
            <span class="card-title">{game.name ?? "Sans nom"}</span>
            <span class="card-meta">
              <span>{exact(game.stats?.mods_count_total)} {game.ugc_name?.toLowerCase() || "mods"}</span>
              <span><Icon name="download" size={14} />{count(game.stats?.mods_downloads_total)}</span>
            </span>
          </button>
        </li>
      {/each}
    </ul>
  {/if}

  <LoadMore {list} noun="jeux" />
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

  .skeleton .thumb {
    aspect-ratio: 16 / 9;
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
