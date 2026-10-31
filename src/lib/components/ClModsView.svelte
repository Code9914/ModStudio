<script lang="ts">
  // Page d'un jeu : dossier, chargeur, et les mods de la liste de sa communauté.
  import { onMount } from "svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import type { ClGame } from "$lib/api";
  import { count, exact } from "$lib/format";
  import { tr } from "$lib/i18n.svelte";
  import { listsStore } from "$lib/lists.svelte";
  import { nav } from "$lib/nav.svelte";
  import { inview } from "$lib/paged.svelte";
  import { normalizeName } from "$lib/thunderstore.svelte";
  import ClSetup from "./ClSetup.svelte";
  import ErrorNotice from "./ErrorNotice.svelte";
  import Icon from "./Icon.svelte";
  import Thumb from "./Thumb.svelte";

  let { game }: { game: ClGame } = $props();

  let query = $state("");
  let category = $state("");
  type Sort = "name" | "downloads" | "updated";
  /** Tri choisi ; par défaut les plus téléchargés quand la liste donne ce chiffre. */
  let chosen = $state<Sort | null>(null);
  let showAll = $state(false);
  let bannerFailed = $state(false);
  const PAGE = 48;
  let shown = $state(PAGE);

  onMount(() => {
    listsStore.loadMods(game);
    const config = listsStore.config(game);
    if (config) listsStore.checkUpdates(game.gameId);
  });

  const mods = $derived(listsStore.mods[game.id] ?? null);
  const error = $derived(listsStore.errors[game.id] ?? null);
  const loader = $derived(listsStore.loaders[game.id] ?? null);
  const search = $derived(normalizeName(query));
  const hasDownloads = $derived((mods ?? []).some((m) => m.downloads !== null));
  const hasDates = $derived((mods ?? []).some((m) => m.updated));
  const sort = $derived<Sort>(chosen ?? (hasDownloads ? "downloads" : "name"));

  $effect(() => {
    search;
    category;
    showAll;
    sort;
    shown = PAGE;
  });

  const categories = $derived(
    [...new Set((mods ?? []).map((m) => m.category).filter((c): c is string => !!c))].sort((a, b) => a.localeCompare(b, "fr")),
  );

  const list = $derived.by(() => {
    const out = (mods ?? []).filter((m) => {
      const installed = !!listsStore.installed(game.gameId, m.modId);
      if (!showAll && !installed && (m.deprecated || m.broken || !m.compatible)) return false;
      if (category && m.category !== category) return false;
      if (!search) return true;
      return [m.name, m.description, m.author, ...m.tags].some((t) => normalizeName(t).includes(search));
    });
    if (sort === "downloads") return [...out].sort((a, b) => (b.downloads ?? 0) - (a.downloads ?? 0));
    if (sort === "updated") return [...out].sort((a, b) => (b.updated ?? "").localeCompare(a.updated ?? ""));
    return out;
  });
</script>

<div class="view">
  <section class="banner" class:plain={bannerFailed}>
    {#if !bannerFailed}
      <img class="banner-img" src={game.hero} alt="" onerror={() => (bannerFailed = true)} />
    {/if}
    <div class="banner-text">
      <button class="btn btn-ghost crumb" onclick={() => nav.openClGames()}>
        <Icon name="back" />{tr("Tous les jeux des communautés", "All community games")}
      </button>
      <h1>{game.name}</h1>
      <div class="banner-stats">
        <span class="chip">{game.listName}</span>
        <span class="chip" title={tr("Chargeur de mods du jeu", "The game's mod loader")}>{game.loader}</span>
        {#if mods}<span><strong>{exact(mods.length)}</strong> mods</span>{/if}
        <button class="btn btn-ghost" onclick={() => openUrl(game.listUrl)}>
          {tr("Site de la liste", "List website")} <Icon name="external" size={14} />
        </button>
      </div>
    </div>
  </section>

  <div class="body">
    <p class="about muted small">{game.about}</p>
    {#key game.id}<ClSetup {game} />{/key}

    <div class="toolbar">
      <label class="search">
        <span class="visually-hidden">{tr(`Rechercher dans les mods de ${game.name}`, `Search ${game.name} mods`)}</span>
        <Icon name="search" />
        <input class="field" type="search" placeholder={tr("Rechercher dans les mods", "Search mods")} bind:value={query} />
      </label>
      {#if categories.length > 1}
        <label>
          <span class="visually-hidden">{tr("Catégorie", "Category")}</span>
          <select class="field" bind:value={category}>
            <option value="">{tr("Toutes les catégories", "All categories")}</option>
            {#each categories as c (c)}<option value={c}>{c}</option>{/each}
          </select>
        </label>
      {/if}
      {#if hasDownloads || hasDates}
        <label>
          <span class="visually-hidden">{tr("Trier les mods", "Sort mods")}</span>
          <select class="field" value={sort} onchange={(e) => (chosen = e.currentTarget.value as Sort)}>
            <option value="name">{tr("Nom (A à Z)", "Name (A to Z)")}</option>
            {#if hasDownloads}<option value="downloads">{tr("Plus téléchargés", "Most downloaded")}</option>{/if}
            {#if hasDates}<option value="updated">{tr("Récemment mis à jour", "Recently updated")}</option>{/if}
          </select>
        </label>
      {/if}
      <label class="check small">
        <input type="checkbox" bind:checked={showAll} />
        {tr("Afficher les mods obsolètes ou cassés", "Show outdated or broken mods")}
      </label>
      {#if mods}<p class="muted">{exact(list.length)} mods</p>{/if}
    </div>

    {#if error && !mods}
      <ErrorNotice {error} onretry={() => listsStore.loadMods(game, true)} />
    {:else if !mods}
      <ul class="card-grid" aria-busy="true" aria-label={tr("Chargement des mods", "Loading mods")}>
        {#each Array(12) as _, i (i)}
          <li class="skeleton"><div class="thumb"></div><div class="line"></div></li>
        {/each}
      </ul>
    {:else if mods.length === 0}
      <div class="empty">
        <p>
          {loader?.note ??
            (listsStore.config(game)
              ? tr("Aucun mod dans la liste pour l'instant.", "No mods in the list yet.")
              : tr(
                  "Choisis d'abord le dossier du jeu : la liste dépend de la version installée.",
                  "Choose the game folder first: the list depends on the installed version.",
                ))}
        </p>
      </div>
    {:else if list.length === 0}
      <div class="empty">
        <p>{tr(`Aucun mod ne correspond${query.trim() ? ` à « ${query.trim()} »` : ""}.`, `No mods match${query.trim() ? ` "${query.trim()}"` : ""}.`)}</p>
        {#if query || category}<button class="btn" onclick={() => ((query = ""), (category = ""))}>{tr("Effacer les filtres", "Clear filters")}</button>{/if}
      </div>
    {:else}
      <ul class="card-grid">
        {#each list.slice(0, shown) as m (m.id)}
          {@const record = listsStore.installed(game.gameId, m.modId)}
          <li>
            <button class="card" onclick={() => nav.openClMod(m.id)}>
              <Thumb
                src={m.icon}
                fallback={m.iconFallback}
                label={m.name}
                badges={[
                  ...(record
                    ? listsStore.outdated(game.gameId, m.modId)
                      ? [{ text: tr("Mise à jour", "Update"), tone: "accent" as const }]
                      : [{ text: tr("Installé", "Installed"), tone: "ok" as const }]
                    : []),
                  ...(m.loader ? [{ text: tr("Chargeur", "Loader"), tone: "neutral" as const }] : []),
                  ...(m.broken ? [{ text: tr("Cassé", "Broken"), tone: "warn" as const }] : m.deprecated ? [{ text: tr("Obsolète", "Outdated"), tone: "warn" as const }] : []),
                ]}
              />
              <span class="card-title">{m.name}</span>
              {#if m.author}<span class="card-meta"><span class="author">{tr("par", "by")} {m.author}</span></span>{/if}
              {#if m.description}<span class="desc">{m.description}</span>{/if}
              <span class="card-meta">
                {#if m.category}<span>{m.category}</span>{/if}
                <span>{m.latest}</span>
                {#if m.downloads !== null}<span title={tr("Téléchargements", "Downloads")}><Icon name="download" size={14} />{count(m.downloads)}</span>{/if}
              </span>
            </button>
          </li>
        {/each}
      </ul>
      {#if list.length > shown}
        {#key shown}
          <div class="more" use:inview={() => (shown = Math.min(shown + PAGE, list.length))}>
            <span class="muted">{tr(`${exact(shown)} mods affichés sur ${exact(list.length)}`, `Showing ${exact(shown)} of ${exact(list.length)} mods`)}</span>
          </div>
        {/key}
      {/if}
    {/if}
  </div>
</div>

<style>
  .view {
    height: 100%;
    overflow-y: auto;
  }

  .banner {
    position: relative;
    min-height: 14rem;
    display: flex;
    align-items: flex-end;
    overflow: hidden;
    background: var(--panel);
  }

  .banner-img {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .banner:not(.plain)::after {
    content: "";
    position: absolute;
    inset: 0;
    background: linear-gradient(to top, var(--ink) 4%, transparent 75%),
      linear-gradient(to right, var(--scrim) 0%, transparent 65%);
  }

  .banner-text {
    position: relative;
    z-index: 1;
    padding: 1.25rem 2.5rem 1.75rem;
    display: grid;
    justify-items: start;
    gap: 0.6rem;
    max-width: 60rem;
  }

  .crumb {
    margin-left: -0.5rem;
    color: var(--text);
  }

  h1 {
    font-size: clamp(var(--t-3xl), 5vw, 3.75rem);
    font-weight: 800;
    letter-spacing: -0.025em;
    text-wrap: balance;
  }

  .banner-stats {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.4rem 1.5rem;
    font-size: var(--t-md);
  }

  .banner-stats strong {
    font-family: var(--font-display);
    font-size: var(--t-lg);
  }

  .banner-stats .btn-ghost {
    color: var(--accent);
  }

  .body {
    padding: 1.5rem 2.5rem 3rem;
  }

  .about {
    max-width: 50rem;
    margin-bottom: 1rem;
  }

  .toolbar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.75rem 1rem;
    margin-bottom: 1.75rem;
  }

  .search {
    position: relative;
    display: block;
    flex: 1 1 18rem;
    max-width: 34rem;
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

  .check {
    display: flex;
    align-items: center;
    gap: 0.45rem;
  }

  .card-grid {
    grid-template-columns: repeat(auto-fill, minmax(calc(170px * var(--thumb-scale)), 1fr));
  }

  .card-grid :global(.thumb) {
    aspect-ratio: 1;
  }

  .author {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 100%;
    display: block;
  }

  .desc {
    display: -webkit-box;
    -webkit-line-clamp: 3;
    line-clamp: 3;
    -webkit-box-orient: vertical;
    overflow: hidden;
    font-size: var(--t-sm);
    color: var(--muted);
    text-align: left;
  }

  .skeleton .thumb {
    aspect-ratio: 1;
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

  .more {
    display: flex;
    align-items: center;
    gap: 1rem;
    margin-top: 1.75rem;
  }

  .empty {
    display: grid;
    justify-items: start;
    gap: 1rem;
    padding: 2rem 0;
    max-width: 46rem;
  }

  .small {
    font-size: var(--t-sm);
  }

  @media (max-width: 1150px) {
    .body,
    .banner-text {
      padding-inline: 1.75rem;
    }
  }
</style>
