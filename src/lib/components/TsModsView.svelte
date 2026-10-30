<script lang="ts">
  import { untrack } from "svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { ts, type TsCommunity, type TsOrdering, type TsPackage } from "$lib/api";
  import { count, exact } from "$lib/format";
  import { nav } from "$lib/nav.svelte";
  import { PagedList } from "$lib/paged.svelte";
  import { tsStore } from "$lib/thunderstore.svelte";
  import ErrorNotice from "./ErrorNotice.svelte";
  import Icon from "./Icon.svelte";
  import LoadMore from "./LoadMore.svelte";
  import Thumb from "./Thumb.svelte";
  import TsGameSetup from "./TsGameSetup.svelte";

  let { game }: { game: TsCommunity } = $props();

  let query = $state("");
  let search = $state("");
  let sort = $state<TsOrdering>("most-downloaded");
  let bannerFailed = $state(false);

  const list = new PagedList<TsPackage>(
    // Deux pages de l'API (20 mods chacune) à la fois.
    (offset, limit) => ts.packages({ community: game.identifier, query: search, ordering: sort, offset, limit }),
    40,
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

  // Fiche complète du jeu (image, chiffres) quand il est ouvert depuis la barre latérale.
  const info = $derived(tsStore.community(game.identifier) ?? game);
  $effect(() => {
    untrack(() => tsStore.loadCommunities());
  });

  // Mises à jour des mods installés de ce jeu, pour les badges.
  $effect(() => {
    const id = game.id;
    untrack(() => {
      if (tsStore.game(id)) tsStore.checkUpdates(id);
    });
  });

  // Contenu adulte masqué si l'option est cochée dans les réglages.
  const hideMature = $derived(nav.settings?.hideMature === true);
  const items = $derived(hideMature ? list.items.filter((m) => !m.isNsfw) : list.items);

  const sorts: { value: TsOrdering; label: string }[] = [
    { value: "most-downloaded", label: "Plus téléchargés" },
    { value: "top-rated", label: "Mieux notés" },
    { value: "last-updated", label: "Récemment mis à jour" },
    { value: "newest", label: "Nouveautés" },
  ];
</script>

<div class="view">
  <section class="banner" class:plain={bannerFailed || !info.heroImageUrl}>
    {#if info.heroImageUrl && !bannerFailed}
      <img class="banner-img" src={info.heroImageUrl} alt="" onerror={() => (bannerFailed = true)} />
    {/if}
    <div class="banner-text">
      <button class="btn btn-ghost crumb" onclick={() => nav.openTsGames()}>
        <Icon name="back" />Tous les jeux Thunderstore
      </button>
      <div class="title-row">
        {#if info.coverImageUrl}
          <img class="poster" src={info.coverImageUrl} alt="" />
        {/if}
        <h1>{info.name}</h1>
      </div>
      <div class="banner-stats">
        <span class="chip">Thunderstore</span>
        {#if info.loader}<span class="chip" title="Chargeur de mods du jeu">{info.loader}</span>{/if}
        {#if info.totalPackageCount > 0}
          <span><strong>{exact(info.totalPackageCount)}</strong> mods</span>
          <span><strong>{count(info.totalDownloadCount)}</strong> téléchargements</span>
        {/if}
        <button class="btn btn-ghost" onclick={() => openUrl(`https://thunderstore.io/c/${game.identifier}/`)}>
          Page du jeu sur Thunderstore <Icon name="external" size={14} />
        </button>
      </div>
    </div>
  </section>

  <div class="body">
    {#key game.id}<TsGameSetup game={info} />{/key}

    <div class="toolbar">
      <label class="search">
        <span class="visually-hidden">Rechercher dans les mods de {info.name}</span>
        <Icon name="search" />
        <input class="field" type="search" placeholder="Rechercher dans les mods" bind:value={query} />
      </label>
      <label>
        <span class="visually-hidden">Trier les mods</span>
        <select class="field" bind:value={sort}>
          {#each sorts as s (s.value)}
            <option value={s.value}>{s.label}</option>
          {/each}
        </select>
      </label>
      {#if list.loaded}
        <p class="muted">{exact(list.total)} résultats</p>
      {/if}
    </div>

    {#if list.error && list.items.length === 0}
      <ErrorNotice error={list.error} onretry={() => list.retry()} />
    {:else if !list.loaded}
      <ul class="card-grid" aria-busy="true" aria-label="Chargement des mods">
        {#each Array(12) as _, i (i)}
          <li class="skeleton"><div class="thumb"></div><div class="line"></div></li>
        {/each}
      </ul>
    {:else if list.items.length === 0}
      <div class="empty">
        <p>Aucun mod ne correspond{search ? ` à « ${search} »` : ""}.</p>
        {#if search}<button class="btn" onclick={() => (query = "")}>Effacer la recherche</button>{/if}
      </div>
    {:else}
      <ul class="card-grid">
        {#each items as m (m.id)}
          <li>
            <button class="card" onclick={() => nav.openTsMod({ namespace: m.namespace, name: m.name })}>
              <Thumb
                src={m.iconUrl}
                label={m.name}
                badges={[
                  ...(tsStore.installed(game.id, m.id)
                    ? tsStore.outdated(game.id, m.id)
                      ? [{ text: "Mise à jour", tone: "accent" as const }]
                      : [{ text: "Installé", tone: "ok" as const }]
                    : []),
                  ...(m.isDeprecated ? [{ text: "Obsolète", tone: "warn" as const }] : []),
                  ...(m.isNsfw ? [{ text: "Adulte", tone: "warn" as const }] : []),
                ]}
              />
              <span class="card-title">{m.name.replaceAll("_", " ")}</span>
              <span class="card-meta"><span class="author">par {m.namespace}</span></span>
              {#if m.description}<span class="desc">{m.description}</span>{/if}
              <span class="card-meta">
                <span title="Téléchargements"><Icon name="download" size={14} />{count(m.downloadCount)}</span>
                <span title="Notes"><Icon name="thumb" size={14} />{count(m.ratingCount)}</span>
              </span>
            </button>
          </li>
        {/each}
      </ul>
    {/if}

    {#if list.items.length > 0}<LoadMore {list} noun="mods" />{/if}
  </div>
</div>

<style>
  .view {
    height: 100%;
    overflow-y: auto;
  }

  .banner {
    position: relative;
    min-height: 15rem;
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

  .title-row {
    display: flex;
    align-items: flex-end;
    gap: 1.25rem;
  }

  .poster {
    width: 6.5rem;
    aspect-ratio: 3 / 4;
    object-fit: cover;
    border-radius: var(--r-md);
    box-shadow: 0 8px 24px rgb(0 0 0 / 0.35);
    flex: none;
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

  /* Icônes carrées de Thunderstore (256 × 256). */
  .body :global(.card-grid) {
    grid-template-columns: repeat(auto-fill, minmax(calc(170px * var(--thumb-scale)), 1fr));
  }

  .body :global(.card-grid .thumb) {
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
    -webkit-line-clamp: 2;
    line-clamp: 2;
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

  .empty {
    display: grid;
    justify-items: start;
    gap: 1rem;
    padding: 2rem 0;
  }

  @media (max-width: 1150px) {
    .body,
    .banner-text {
      padding-inline: 1.75rem;
    }
  }
</style>
