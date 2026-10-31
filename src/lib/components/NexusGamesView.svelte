<script lang="ts">
  import { onMount, untrack } from "svelte";
  import { nexus, nexusGameImage, type NexusGame, type NexusGameSort } from "$lib/api";
  import { count, exact } from "$lib/format";
  import { tr } from "$lib/i18n.svelte";
  import { nav } from "$lib/nav.svelte";
  import { launcherNames, nexusGameOf, nexusStore } from "$lib/nexus.svelte";
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
  const mine = $derived(Object.values(nexusStore.games).filter((g) => nexusStore.count(g) > 0));

  // Jeux installés sur ce PC (Steam, Epic) qui ont une page sur Nexus Mods.
  onMount(() => {
    if (nexusStore.detected === null) nexusStore.detectGames();
    // Déjà cherchés : actualisés en arrière-plan à l'ouverture de la page.
    else nexusStore.detectGames(true, true);
  });
  const onPc = $derived(
    [...nexusStore.installedGames].sort((a, b) => a.nexus.name.localeCompare(b.nexus.name, "fr")),
  );
  let showAllPc = $state(false);
  const PC_PREVIEW = 12;
  const pcShown = $derived(showAllPc ? onPc : onPc.slice(0, PC_PREVIEW));

  function openInstalled(g: (typeof onPc)[number]) {
    nav.openNexusGame({
      id: g.nexus.id,
      name: g.nexus.name,
      domainName: g.nexus.domain_name,
      // Chiffres chargés par la page du jeu.
      modCount: 0,
      downloadCount: 0,
      genre: null,
    });
  }
</script>

<div class="view">
  <header class="head">
    <div>
      <p class="source">Nexus Mods</p>
      <h1>{tr("Jeux", "Games")}</h1>
      {#if list.loaded}
        <p class="muted">
          {exact(list.total)}
          {tr("jeux", list.total === 1 ? "game" : "games")}
          {search ? tr(`pour « ${search} »`, `for "${search}"`) : tr("sur Nexus Mods", "on Nexus Mods")}
        </p>
      {/if}
    </div>
    <div class="tools">
      <label class="search">
        <span class="visually-hidden">{tr("Rechercher un jeu sur Nexus Mods", "Search for a game on Nexus Mods")}</span>
        <Icon name="search" />
        <input class="field" type="search" placeholder={tr("Rechercher un jeu", "Search games")} bind:value={query} />
      </label>
      <label>
        <span class="visually-hidden">{tr("Trier les jeux", "Sort games")}</span>
        <select class="field" bind:value={sort}>
          <option value="downloads">{tr("Plus téléchargés", "Most downloaded")}</option>
          <option value="mods">{tr("Plus de mods", "Most mods")}</option>
          <option value="name">{tr("Nom (A à Z)", "Name (A to Z)")}</option>
        </select>
      </label>
    </div>
  </header>

  {#if !nexusStore.ready && nexusStore.authLoaded}
    <div class="key-hint">
      <p>
        {tr(
          "Tu peux parcourir Nexus Mods sans compte. Pour installer des mods, ajoute ta clé API Nexus Mods : elle se crée en deux clics sur le site.",
          "You can browse Nexus Mods without an account. To install mods, add your Nexus Mods API key: it takes two clicks to create on the site.",
        )}
      </p>
      <button class="btn btn-primary" onclick={() => nav.openNexusAccount(nav.view)}>{tr("Ajouter ma clé", "Add my key")}</button>
    </div>
  {/if}

  {#if mine.length > 0 && !search}
    <section class="mine" aria-label={tr("Jeux avec des mods installés", "Games with installed mods")}>
      <h2>{tr("Tes jeux", "Your games")}</h2>
      <div class="mine-list">
        {#each mine as g (g.gameId)}
          <button
            class="chip mine-chip"
            onclick={() => nav.openNexusGame(nexusGameOf(g))}
          >
            <img src={nexusGameImage(g.gameId, "thumbnail")} alt="" width="18" height="18" />
            {g.gameName}
            <span class="muted">{nexusStore.count(g)}</span>
          </button>
        {/each}
      </div>
    </section>
  {/if}

  {#if !search && (onPc.length > 0 || (nexusStore.detecting && nexusStore.detected === null))}
    <section class="on-pc" aria-label={tr("Jeux installés sur ce PC", "Games installed on this PC")}>
      <div class="section-head">
        <h2>{tr("Installés sur ce PC", "Installed on this PC")}</h2>
        {#if onPc.length > 0}<span class="muted">{onPc.length}</span>{/if}
        <button
          class="btn btn-ghost"
          onclick={() => nexusStore.detectGames(true)}
          disabled={nexusStore.detecting}
          title={tr("Chercher de nouveau les jeux Steam et Epic Games", "Scan again for Steam and Epic Games games")}
        >
          <Icon name="refresh" size={15} />{nexusStore.detecting ? tr("Recherche…", "Searching…") : tr("Actualiser", "Refresh")}
        </button>
      </div>
      {#if onPc.length === 0}
        <ul class="card-grid posters" aria-busy="true" aria-label={tr("Recherche des jeux installés", "Searching for installed games")}>
          {#each Array(6) as _, i (i)}
            <li class="skeleton"><div class="thumb"></div><div class="line"></div></li>
          {/each}
        </ul>
      {:else}
        <ul class="card-grid posters">
          {#each pcShown as g (g.nexus.id)}
            {@const config = nexusStore.game(g.nexus.id)}
            {@const n = config ? Object.keys(config.mods).length : 0}
            <li>
              <button class="card" onclick={() => openInstalled(g)}>
                <Thumb
                  src={nexusGameImage(g.nexus.id, "tile")}
                  label={g.nexus.name}
                  badges={[
                    { text: launcherNames[g.launcher] ?? g.launcher, tone: "neutral" as const },
                    ...(n > 0 ? [{ text: `${n} ${n > 1 ? "mods" : "mod"}`, tone: "ok" as const }] : []),
                  ]}
                />
                <span class="card-title">{g.nexus.name}</span>
                <span class="card-meta"><span>{exact(g.nexus.mods)} {tr("mods sur Nexus", "mods on Nexus")}</span></span>
              </button>
            </li>
          {/each}
        </ul>
        {#if onPc.length > PC_PREVIEW}
          <button class="btn more" onclick={() => (showAllPc = !showAllPc)}>
            {showAllPc ? tr("Afficher moins", "Show less") : tr(`Afficher les ${onPc.length} jeux`, `Show all ${onPc.length} games`)}
          </button>
        {/if}
      {/if}
    </section>
    <h2 class="all-title">{tr("Tous les jeux", "All games")}</h2>
  {/if}

  {#if list.error && list.items.length === 0}
    <ErrorNotice error={list.error} onretry={() => list.retry()} />
  {:else if !list.loaded}
    <ul class="card-grid posters" aria-busy="true" aria-label={tr("Chargement des jeux", "Loading games")}>
      {#each Array(12) as _, i (i)}
        <li class="skeleton"><div class="thumb"></div><div class="line"></div></li>
      {/each}
    </ul>
  {:else if list.items.length === 0}
    <div class="empty">
      <p>{tr(`Aucun jeu ne correspond à « ${search} ».`, `No games match "${search}".`)}</p>
      <button class="btn" onclick={() => (query = "")}>{tr("Effacer la recherche", "Clear search")}</button>
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
              <span title={tr("Téléchargements", "Downloads")}><Icon name="download" size={14} />{count(game.downloadCount)}</span>
            </span>
          </button>
        </li>
      {/each}
    </ul>
  {/if}

  {#if list.items.length > 0}<LoadMore {list} noun={tr("jeux", "games")} />{/if}
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

  .on-pc {
    margin-bottom: 2.25rem;
  }

  .section-head {
    display: flex;
    align-items: baseline;
    gap: 0.6rem;
    margin-bottom: 0.6rem;
  }

  .section-head h2 {
    margin-bottom: 0;
  }

  .section-head .btn {
    margin-left: auto;
  }

  .more {
    margin-top: 1rem;
  }

  .all-title {
    margin-bottom: 0.9rem;
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
    grid-template-columns: repeat(auto-fill, minmax(calc(135px * var(--thumb-scale)), 1fr));
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
