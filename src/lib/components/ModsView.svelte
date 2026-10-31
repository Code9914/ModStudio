<script lang="ts">
  import { untrack } from "svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import {
    api,
    asAppError,
    gameClosed,
    pcFileId,
    type AppError,
    type Game,
    type Mod,
    type ModSort,
    type TagOption,
  } from "$lib/api";
  import ErrorNotice from "./ErrorNotice.svelte";
  import { count, exact } from "$lib/format";
  import { tr } from "$lib/i18n.svelte";
  import { installs } from "$lib/installs.svelte";
  import { nav } from "$lib/nav.svelte";
  import { PagedList } from "$lib/paged.svelte";
  import Icon from "./Icon.svelte";
  import LoadMore from "./LoadMore.svelte";
  import Thumb from "./Thumb.svelte";

  let { game }: { game: Game } = $props();

  let query = $state("");
  let search = $state("");
  let sort = $state<ModSort>("-downloads_today");
  /** Tags choisis, par groupe. */
  let picked = $state<Record<string, string[]>>({});
  /** Groupes de tags dépliés en entier. */
  let expanded = $state<Record<string, boolean>>({});

  const tags = $derived(Object.values(picked).flat());

  const list = new PagedList<Mod>(
    (offset, limit) => api.listMods({ gameId: game.id, query: search, tags, sort, offset, limit }),
    30,
  );

  $effect(() => {
    const q = query.trim();
    const t = setTimeout(() => (search = q), 350);
    return () => clearTimeout(t);
  });

  // Jeu fermé aux applications tierces : inutile de demander ses mods.
  const closed = $derived(gameClosed(game));
  let siteError = $state<AppError | null>(null);

  async function openSite() {
    siteError = null;
    try {
      await api.openModioWindow(game.profile_url!, game.name);
    } catch (e) {
      siteError = asAppError(e);
    }
  }

  $effect(() => {
    search;
    sort;
    tags;
    if (!closed) untrack(() => list.reload());
  });

  // Mods suivis dans ce jeu, pour le badge « Abonné ».
  $effect(() => {
    const id = game.id;
    untrack(() => nav.loadSubs(id));
  });

  // Le jeu installe-t-il ses mods lui-même via mod.io sur ce PC ?
  $effect(() => {
    const id = game.id;
    untrack(() => installs.detectManaged(id));
  });
  const managed = $derived(installs.managed[game.id] ?? null);

  const sorts: { value: ModSort; label: string }[] = [
    { value: "-downloads_today", label: tr("Tendances du jour", "Trending today") },
    { value: "-downloads_total", label: tr("Plus téléchargés", "Most downloaded") },
    { value: "-subscribers_total", label: tr("Plus d'abonnés", "Most subscribers") },
    { value: "-ratings_weighted_aggregate", label: tr("Mieux notés", "Top rated") },
    { value: "-date_live", label: tr("Nouveautés", "Newest") },
    { value: "-date_updated", label: tr("Récemment mis à jour", "Recently updated") },
    { value: "name", label: tr("Nom (A à Z)", "Name (A to Z)") },
  ];

  const groups = $derived(game.tag_options.filter((o) => !o.hidden && o.tags.length > 0 && o.name));
  const banner = $derived(game.header?.original ?? game.logo?.thumb_1280x720 ?? game.logo?.original ?? null);
  const unit = $derived(game.ugc_name?.toLowerCase() || "mods");

  const COLLAPSED = 10;

  function groupName(o: TagOption) {
    return o.name_localized || o.name || "";
  }

  function toggle(o: TagOption, tag: string) {
    const key = o.name ?? "";
    const current = picked[key] ?? [];
    const single = o.type === "dropdown";
    const next = current.includes(tag) ? current.filter((t) => t !== tag) : single ? [tag] : [...current, tag];
    picked = { ...picked, [key]: next };
  }

  function clearTags() {
    picked = {};
  }

  function rating(m: Mod) {
    const s = m.stats;
    return s && s.ratings_total > 0 ? tr(`${s.ratings_percentage_positive} %`, `${s.ratings_percentage_positive}%`) : null;
  }

  function author(m: Mod) {
    return m.submitted_by?.username ?? m.submitted_by?.display_name_portal ?? null;
  }
</script>

<div class="view">
  <section class="banner" class:plain={!banner}>
    {#if banner}
      <img class="banner-img" src={banner} alt="" />
    {/if}
    <div class="banner-text">
      <button class="btn btn-ghost crumb" onclick={() => nav.openGames()}>
        <Icon name="back" />{tr("Tous les jeux", "All games")}
      </button>
      <h1>{game.name}</h1>
      <div class="banner-stats">
        <span><strong>{exact(game.stats?.mods_count_total)}</strong> {unit}</span>
        <span><strong>{count(game.stats?.mods_downloads_total)}</strong> {tr("téléchargements", "downloads")}</span>
        <span><strong>{count(game.stats?.mods_subscribers_total)}</strong> {tr("abonnements", "subscriptions")}</span>
        {#if game.profile_url}
          <button class="btn btn-ghost" onclick={() => openUrl(game.profile_url!)}>
            {tr("Page du jeu sur mod.io", "Game page on mod.io")} <Icon name="external" size={14} />
          </button>
        {/if}
      </div>
      {#if managed}
        <p class="managed">
          <Icon name="check" size={16} />
          {tr(
            "Ce jeu installe ses mods lui-même grâce à mod.io : abonne-toi à ceux qui t'intéressent, il les téléchargera à son prochain lancement.",
            "This game installs its own mods through mod.io: subscribe to the ones you like and it will download them the next time it starts.",
          )}
        </p>
      {/if}
    </div>
  </section>

  {#if closed}
    <div class="closed">
      <h2>{tr(`Les mods de ${game.name} ne sont visibles que sur mod.io`, `${game.name} mods are only available on mod.io`)}</h2>
      <p>
        {tr(
          `L'éditeur de ${game.name} n'autorise pas les applications tierces comme ModStudio à accéder à ses mods. Tu peux les parcourir et t'y abonner sur le site officiel de mod.io, qui s'ouvre dans une fenêtre de ModStudio (connecte-toi une première fois à ton compte mod.io dedans). Tes abonnements arriveront dans le jeu comme d'habitude.`,
          `The publisher of ${game.name} doesn't allow third-party apps like ModStudio to access its mods. You can browse them and subscribe on the official mod.io website, which opens in a ModStudio window (sign in to your mod.io account there the first time). Your subscriptions will reach the game as usual.`,
        )}
      </p>
      {#if game.profile_url}
        <div class="closed-actions">
          <button class="btn btn-primary" onclick={openSite}>{tr("Voir les mods sur mod.io", "View the mods on mod.io")}</button>
          <button class="btn btn-ghost" onclick={() => openUrl(game.profile_url!)}>
            {tr("Ouvrir dans mon navigateur", "Open in my browser")} <Icon name="external" size={14} />
          </button>
        </div>
        {#if siteError}<ErrorNotice error={siteError} />{/if}
      {/if}
    </div>
  {:else}
  <div class="body">
    <aside class="filters" aria-label={tr("Filtres", "Filters")}>
      <label class="filter-block">
        <span class="filter-title">{tr("Trier par", "Sort by")}</span>
        <select class="field" bind:value={sort}>
          {#each sorts as s (s.value)}
            <option value={s.value}>{s.label}</option>
          {/each}
        </select>
      </label>

      {#each groups as o (o.name)}
        {@const all = o.tags}
        {@const shown = expanded[o.name ?? ""] ? all : all.slice(0, COLLAPSED)}
        <fieldset class="filter-block">
          <legend class="filter-title">{groupName(o)}</legend>
          <div class="chips">
            {#each shown as tag (tag)}
              <button
                class="chip"
                aria-pressed={(picked[o.name ?? ""] ?? []).includes(tag)}
                onclick={() => toggle(o, tag)}>{tag}</button
              >
            {/each}
          </div>
          {#if all.length > COLLAPSED}
            <button
              class="btn btn-ghost more"
              onclick={() => (expanded = { ...expanded, [o.name ?? ""]: !expanded[o.name ?? ""] })}
            >
              {expanded[o.name ?? ""] ? tr("Réduire", "Show less") : tr(`Afficher les ${all.length} tags`, `Show all ${all.length} tags`)}
            </button>
          {/if}
        </fieldset>
      {/each}

      {#if tags.length > 0}
        <div class="filter-block">
          {#if tags.length > 1}<p class="muted hint">{tr(`Seuls les ${unit} qui ont tous les tags choisis s'affichent.`, `Only ${unit} with all the selected tags are shown.`)}</p>{/if}
          <button class="btn" onclick={clearTags}>{tags.length > 1 ? tr(`Retirer les ${tags.length} tags`, `Remove the ${tags.length} tags`) : tr("Retirer le tag", "Remove the tag")}</button>
        </div>
      {/if}
    </aside>

    <div class="results">
      <div class="toolbar">
        <label class="search">
          <span class="visually-hidden">{tr(`Rechercher dans les ${unit} de ${game.name}`, `Search ${game.name} ${unit}`)}</span>
          <Icon name="search" />
          <input class="field" type="search" placeholder={tr(`Rechercher dans les ${unit}`, `Search ${unit}`)} bind:value={query} />
        </label>
        {#if list.loaded}
          <p class="muted">{exact(list.total)} {tr("résultats", list.total === 1 ? "result" : "results")}</p>
        {/if}
      </div>

      {#if !list.loaded && !list.error}
        <ul class="card-grid" aria-busy="true" aria-label={tr(`Chargement des ${unit}`, `Loading ${unit}`)}>
          {#each Array(9) as _, i (i)}
            <li class="skeleton"><div class="thumb"></div><div class="line"></div></li>
          {/each}
        </ul>
      {:else if list.loaded && list.items.length === 0}
        <div class="empty">
          <p>{tr(`Aucun ${unit.replace(/s$/, "")} ne correspond à ces critères.`, `No ${unit} match these criteria.`)}</p>
          <div class="empty-actions">
            {#if search}<button class="btn" onclick={() => (query = "")}>{tr("Effacer la recherche", "Clear search")}</button>{/if}
            {#if tags.length}<button class="btn" onclick={clearTags}>{tags.length > 1 ? tr("Retirer les tags", "Remove the tags") : tr("Retirer le tag", "Remove the tag")}</button>{/if}
          </div>
        </div>
      {:else}
        <ul class="card-grid">
          {#each list.items as m (m.id)}
            <li>
              <button class="card" onclick={() => nav.openMod(m.id)}>
                <Thumb
                  src={m.logo?.thumb_640x360 ?? m.logo?.thumb_320x180 ?? m.logo?.original}
                  label={m.name ?? ""}
                  badges={[
                    ...(installs.installed(game.id, m.id)
                      ? installs.outdated(game.id, m.id, pcFileId(m))
                        ? [{ text: tr("Mise à jour", "Update"), tone: "accent" as const }]
                        : [{ text: tr("Installé", "Installed"), tone: "ok" as const }]
                      : []),
                    ...(nav.subs.has(m.id) ? [{ text: tr("Abonné", "Subscribed"), tone: "ok" as const }] : []),
                    ...(m.maturity_option > 0 ? [{ text: tr("Adulte", "Adult"), tone: "warn" as const }] : []),
                  ]}
                />
                <span class="card-title">{m.name ?? tr("Sans nom", "Untitled")}</span>
                <span class="card-meta">
                  {#if author(m)}<span class="author">{tr("par", "by")} {author(m)}</span>{/if}
                </span>
                <span class="card-meta">
                  <span title={tr("Téléchargements", "Downloads")}><Icon name="download" size={14} />{count(m.stats?.downloads_total)}</span>
                  <span title={tr("Abonnés", "Subscribers")}><Icon name="users" size={14} />{count(m.stats?.subscribers_total)}</span>
                  {#if rating(m)}
                    <span title={tr("Avis positifs", "Positive ratings")}><Icon name="thumb" size={14} />{rating(m)}</span>
                  {/if}
                </span>
              </button>
            </li>
          {/each}
        </ul>
      {/if}

      <LoadMore {list} noun={unit} />
    </div>
  </div>
  {/if}
</div>

<style>
  .view {
    height: 100%;
    overflow-y: auto;
  }

  /* La bannière du jeu : le seul moment spectaculaire de l'appli. */
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

  .closed {
    display: grid;
    justify-items: start;
    gap: 0.9rem;
    max-width: 42rem;
    padding: 2rem 2.5rem 3rem;
  }

  .closed h2 {
    font-size: var(--t-xl);
    font-weight: 700;
  }

  .closed-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 0.4rem;
  }

  .managed {
    display: flex;
    align-items: flex-start;
    gap: 0.5rem;
    max-width: 44rem;
    padding: 0.55rem 0.8rem;
    border-radius: var(--r-md);
    background: var(--ok-bg);
    color: var(--ok);
    font-size: var(--t-sm);
    font-weight: 500;
  }

  .managed :global(svg) {
    flex: none;
    margin-top: 0.15rem;
  }

  .body {
    display: grid;
    grid-template-columns: 15rem 1fr;
    gap: 2.5rem;
    padding: 1.5rem 2.5rem 3rem;
  }

  .filters {
    display: grid;
    align-content: start;
    gap: 1.6rem;
    position: sticky;
    top: 1.5rem;
    max-height: calc(100vh - 3rem);
    overflow-y: auto;
    padding-right: 0.25rem;
  }

  .filter-block {
    display: grid;
    gap: 0.55rem;
    border: 0;
    margin: 0;
    padding: 0;
    min-width: 0;
  }

  .filter-title {
    font-family: var(--font-display);
    font-weight: 650;
    font-size: var(--t-md);
    padding: 0;
  }

  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 0.4rem;
  }

  .more {
    justify-self: start;
    font-size: var(--t-xs);
    padding-inline: 0;
  }

  .hint {
    font-size: var(--t-sm);
  }

  .toolbar {
    display: flex;
    align-items: center;
    gap: 1rem;
    margin-bottom: 1.75rem;
  }

  .search {
    position: relative;
    display: block;
    flex: 1;
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

  .results :global(.card-grid) {
    grid-template-columns: repeat(auto-fill, minmax(calc(200px * var(--thumb-scale)), 1fr));
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

  .empty-actions {
    display: flex;
    gap: 0.5rem;
  }

  @media (max-width: 1150px) {
    .body {
      grid-template-columns: 12rem minmax(0, 1fr);
      gap: 1.75rem;
      padding-inline: 1.75rem;
    }

    .banner-text {
      padding-inline: 1.75rem;
    }

    .results :global(.card-grid) {
      grid-template-columns: repeat(auto-fill, minmax(calc(180px * var(--thumb-scale)), 1fr));
    }
  }

  @media (max-width: 760px) {
    .body {
      grid-template-columns: 1fr;
    }

    .filters {
      position: static;
      max-height: none;
    }
  }
</style>
