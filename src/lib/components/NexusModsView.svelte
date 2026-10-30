<script lang="ts">
  import { untrack } from "svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { nexus, nexusGameImage, type NexusGame, type NexusMod, type NexusModSort } from "$lib/api";
  import { count, exact } from "$lib/format";
  import { nav } from "$lib/nav.svelte";
  import { nexusStore } from "$lib/nexus.svelte";
  import { PagedList } from "$lib/paged.svelte";
  import ErrorNotice from "./ErrorNotice.svelte";
  import Icon from "./Icon.svelte";
  import LoadMore from "./LoadMore.svelte";
  import NexusGameTools from "./NexusGameTools.svelte";
  import Thumb from "./Thumb.svelte";

  let { game }: { game: NexusGame } = $props();

  let query = $state("");
  let search = $state("");
  let sort = $state<NexusModSort>("endorsements");
  let bannerFailed = $state(false);
  let posterFailed = $state(false);

  const list = new PagedList<NexusMod>(
    (offset, limit) => nexus.listMods({ gameDomain: game.domainName, query: search, sort, offset, limit }),
    30,
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

  // Ouvert depuis « Tes jeux » : les chiffres du jeu ne sont pas encore connus.
  let stats = $state<NexusGame | null>(null);
  $effect(() => {
    const domain = game.domainName;
    if (game.modCount > 0 || !domain) {
      stats = game;
      return;
    }
    nexus
      .getGame(domain)
      .then((g) => (stats = g))
      .catch(() => (stats = game));
  });

  // Mises à jour des mods installés de ce jeu, pour les badges.
  $effect(() => {
    const id = game.id;
    untrack(() => {
      if (nexusStore.game(id)) nexusStore.checkUpdates(id);
    });
  });

  const sorts: { value: NexusModSort; label: string }[] = [
    { value: "endorsements", label: "Plus recommandés" },
    { value: "downloads", label: "Plus téléchargés" },
    { value: "updated", label: "Récemment mis à jour" },
    { value: "created", label: "Nouveautés" },
    { value: "name", label: "Nom (A à Z)" },
  ];
</script>

<div class="view">
  <section class="banner" class:plain={bannerFailed}>
    {#if !bannerFailed}
      <img class="banner-img" src={nexusGameImage(game.id, "hero")} alt="" onerror={() => (bannerFailed = true)} />
    {/if}
    <div class="banner-text">
      <button class="btn btn-ghost crumb" onclick={() => nav.openNexusGames()}>
        <Icon name="back" />Tous les jeux Nexus
      </button>
      <div class="title-row">
        {#if !posterFailed}
          <img class="poster" src={nexusGameImage(game.id, "tile")} alt="" onerror={() => (posterFailed = true)} />
        {/if}
        <h1>{game.name}</h1>
      </div>
      <div class="banner-stats">
        <span class="chip">Nexus Mods</span>
        {#if stats}
          <span><strong>{exact(stats.modCount)}</strong> mods</span>
          <span><strong>{count(stats.downloadCount)}</strong> téléchargements</span>
        {/if}
        <button class="btn btn-ghost" onclick={() => openUrl(`https://www.nexusmods.com/games/${game.domainName}`)}>
          Page du jeu sur Nexus Mods <Icon name="external" size={14} />
        </button>
      </div>
    </div>
  </section>

  <div class="body">
    {#key game.id}<NexusGameTools {game} />{/key}

    <div class="toolbar">
      <label class="search">
        <span class="visually-hidden">Rechercher dans les mods de {game.name}</span>
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
        {#each Array(9) as _, i (i)}
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
        {#each list.items as m (m.id)}
          <li>
            <button class="card" onclick={() => nav.openNexusMod(m.modId)}>
              <Thumb
                src={m.thumbnailUrl ?? m.pictureUrl}
                label={m.name}
                badges={[
                  ...(nexusStore.installed(game.id, m.modId)
                    ? nexusStore.outdated(game.id, m.modId)
                      ? [{ text: "Mise à jour", tone: "accent" as const }]
                      : [{ text: "Installé", tone: "ok" as const }]
                    : []),
                  ...(m.adultContent ? [{ text: "Adulte", tone: "warn" as const }] : []),
                ]}
              />
              <span class="card-title">{m.name}</span>
              <span class="card-meta">
                {#if m.author}<span class="author">par {m.author}</span>{/if}
              </span>
              <span class="card-meta">
                <span title="Recommandations"><Icon name="thumb" size={14} />{count(m.endorsements)}</span>
                <span title="Téléchargements"><Icon name="download" size={14} />{count(m.downloads)}</span>
                {#if m.category}<span>{m.category}</span>{/if}
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
    aspect-ratio: 2 / 3;
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

  .body :global(.card-grid) {
    grid-template-columns: repeat(auto-fill, minmax(210px, 1fr));
  }

  .author {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 100%;
    display: block;
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

  @media (max-width: 1150px) {
    .body,
    .banner-text {
      padding-inline: 1.75rem;
    }
  }
</style>
