<script lang="ts">
  // Collections d'un jeu Nexus : listes de mods prêtes à installer d'un coup.
  import { untrack } from "svelte";
  import { nexus, type NexusCollectionCard, type NexusCollectionSort, type NexusGame } from "$lib/api";
  import { bytes, count, exact } from "$lib/format";
  import { tr } from "$lib/i18n.svelte";
  import { nav } from "$lib/nav.svelte";
  import { PagedList } from "$lib/paged.svelte";
  import { collectionInstall } from "$lib/collections.svelte";
  import ErrorNotice from "./ErrorNotice.svelte";
  import Icon from "./Icon.svelte";
  import LoadMore from "./LoadMore.svelte";
  import Thumb from "./Thumb.svelte";

  let { game }: { game: NexusGame } = $props();

  let query = $state("");
  let search = $state("");
  let sort = $state<NexusCollectionSort>("endorsements");

  const list = new PagedList<NexusCollectionCard>(
    (offset, limit) => nexus.collections({ gameDomain: game.domainName, query: search, sort, offset, limit }),
    24,
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

  const sorts: { value: NexusCollectionSort; label: string }[] = [
    { value: "endorsements", label: tr("Plus recommandées", "Most endorsed") },
    { value: "downloads", label: tr("Plus téléchargées", "Most downloaded") },
    { value: "rating", label: tr("Mieux notées", "Highest rated") },
    { value: "updated", label: tr("Récemment mises à jour", "Recently updated") },
  ];
</script>

<p class="intro muted">
  {tr(
    "Une collection est une liste de mods choisie par un joueur, avec l'ordre dans lequel ils doivent s'installer. ModStudio les installe un par un ; quand deux mods remplacent le même fichier, celui que la collection place après gagne, et l'autre est gardé de côté.",
    "A collection is a list of mods picked by a player, with the order they should be installed in. ModStudio installs them one by one; when two mods replace the same file, the one the collection places later wins, and the other is kept aside.",
  )}
</p>

<div class="toolbar">
  <label class="search">
    <span class="visually-hidden">{tr(`Rechercher dans les collections de ${game.name}`, `Search ${game.name} collections`)}</span>
    <Icon name="search" />
    <input class="field" type="search" placeholder={tr("Rechercher une collection", "Search collections")} bind:value={query} />
  </label>
  <label>
    <span class="visually-hidden">{tr("Trier les collections", "Sort collections")}</span>
    <select class="field" bind:value={sort}>
      {#each sorts as s (s.value)}
        <option value={s.value}>{s.label}</option>
      {/each}
    </select>
  </label>
  {#if list.loaded}
    <p class="muted">{exact(list.total)} {tr("collections", list.total === 1 ? "collection" : "collections")}</p>
  {/if}
</div>

{#if list.error && list.items.length === 0}
  <ErrorNotice error={list.error} onretry={() => list.retry()} />
{:else if !list.loaded}
  <ul class="card-grid" aria-busy="true" aria-label={tr("Chargement des collections", "Loading collections")}>
    {#each Array(6) as _, i (i)}
      <li class="skeleton"><div class="thumb"></div><div class="line"></div></li>
    {/each}
  </ul>
{:else if list.items.length === 0}
  <div class="empty">
    <p>
      {search
        ? tr(`Aucune collection ne correspond à « ${search} ».`, `No collections match "${search}".`)
        : tr(`Pas encore de collection pour ${game.name}.`, `No collections for ${game.name} yet.`)}
    </p>
    {#if search}<button class="btn" onclick={() => (query = "")}>{tr("Effacer la recherche", "Clear search")}</button>{/if}
  </div>
{:else}
  <ul class="card-grid">
    {#each list.items as c (c.id)}
      <li>
        <button class="card" onclick={() => nav.openNexusCollection(c.slug)}>
          <Thumb
            src={c.image}
            label={c.name}
            badges={[
              ...(collectionInstall.on(c.slug)
                ? [{ text: collectionInstall.busy ? tr("Installation…", "Installing…") : tr("Installée", "Installed"), tone: "ok" as const }]
                : []),
              ...(c.adult ? [{ text: tr("Adulte", "Adult"), tone: "warn" as const }] : []),
            ]}
          />
          <span class="card-title">{c.name}</span>
          <span class="card-meta">
            {#if c.author}<span class="author">{tr("par", "by")} {c.author}</span>{/if}
          </span>
          <span class="card-meta">
            <span title={tr("Mods", "Mods")}><Icon name="grid" size={14} />{c.modCount} mods</span>
            {#if c.size > 0}<span>{bytes(c.size)}</span>{/if}
            <span title={tr("Recommandations", "Endorsements")}><Icon name="thumb" size={14} />{count(c.endorsements)}</span>
          </span>
        </button>
      </li>
    {/each}
  </ul>
{/if}

{#if list.items.length > 0}<LoadMore {list} noun={tr("collections", "collections")} />{/if}

<style>
  .intro {
    max-width: 52rem;
    margin-bottom: 1.25rem;
    font-size: var(--t-sm);
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
</style>
