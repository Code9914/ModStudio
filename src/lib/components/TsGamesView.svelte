<script lang="ts">
  import { onMount } from "svelte";
  import type { TsCommunity } from "$lib/api";
  import { count, exact } from "$lib/format";
  import { tr } from "$lib/i18n.svelte";
  import { nav } from "$lib/nav.svelte";
  import { launcherNames, nexusStore } from "$lib/nexus.svelte";
  import { inview } from "$lib/paged.svelte";
  import { normalizeName, tsStore } from "$lib/thunderstore.svelte";
  import ErrorNotice from "./ErrorNotice.svelte";
  import Icon from "./Icon.svelte";
  import Thumb from "./Thumb.svelte";

  let query = $state("");
  let sort = $state<"downloads" | "mods" | "name">("downloads");
  const PAGE = 48;
  let shown = $state(PAGE);

  onMount(() => {
    tsStore.loadCommunities();
    if (nexusStore.detected === null) nexusStore.detectGames();
    else nexusStore.detectGames(true, true);
  });

  const search = $derived(normalizeName(query));
  $effect(() => {
    search;
    sort;
    shown = PAGE;
  });

  const all = $derived.by(() => {
    const list = (tsStore.communities ?? []).filter((c) => !search || normalizeName(c.name).includes(search));
    if (sort === "name") return [...list].sort((a, b) => a.name.localeCompare(b.name, "fr", { sensitivity: "base" }));
    if (sort === "mods") return [...list].sort((a, b) => b.totalPackageCount - a.totalPackageCount);
    return list; // déjà triés par téléchargements
  });

  /** Jeux où des mods Thunderstore sont installés. */
  const mine = $derived(Object.values(tsStore.games).filter((g) => tsStore.count(g) > 0));

  /** Jeux installés sur ce PC qui ont des mods sur Thunderstore. */
  const onPc = $derived.by(() => {
    if (!tsStore.communities || !nexusStore.detected) return [];
    const byName = new Map(tsStore.communities.map((c) => [normalizeName(c.name), c]));
    const out: { community: TsCommunity; launcher: string }[] = [];
    const seen = new Set<number>();
    for (const g of nexusStore.detected) {
      const c = byName.get(normalizeName(g.name));
      if (c && !seen.has(c.id)) {
        seen.add(c.id);
        out.push({ community: c, launcher: g.launcher });
      }
    }
    return out.sort((a, b) => a.community.name.localeCompare(b.community.name, "fr"));
  });
</script>

<div class="view">
  <header class="head">
    <div>
      <p class="source">Thunderstore</p>
      <h1>{tr("Jeux", "Games")}</h1>
      {#if tsStore.communities}
        <p class="muted">
          {tr(
            `${exact(all.length)} jeux ${search ? `pour « ${query.trim()} »` : "sur Thunderstore"}`,
            `${exact(all.length)} games ${search ? `for "${query.trim()}"` : "on Thunderstore"}`,
          )}
        </p>
      {/if}
    </div>
    <div class="tools">
      <label class="search">
        <span class="visually-hidden">{tr("Rechercher un jeu sur Thunderstore", "Search for a game on Thunderstore")}</span>
        <Icon name="search" />
        <input class="field" type="search" placeholder={tr("Rechercher un jeu", "Search for a game")} bind:value={query} />
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

  <p class="intro muted">
    {tr(
      "Thunderstore rassemble les mods de nombreux jeux (Lethal Company, Valheim, R.E.P.O.…). ModStudio sait ce que chaque jeu utilise pour charger ses mods (BepInEx, MelonLoader…) : il l'installe s'il manque, puis chaque mod avec ceux dont il a besoin, rangé comme le ferait un gestionnaire de mods. Pas besoin de compte.",
      "Thunderstore brings together mods for many games (Lethal Company, Valheim, R.E.P.O.…). ModStudio knows what each game uses to load its mods (BepInEx, MelonLoader…): it installs it if it's missing, then each mod along with the ones it needs, organized the way a mod manager would. No account needed.",
    )}
  </p>

  {#if mine.length > 0 && !search}
    <section class="mine" aria-label={tr("Jeux avec des mods installés", "Games with installed mods")}>
      <h2>{tr("Tes jeux", "Your games")}</h2>
      <div class="mine-list">
        {#each mine as g (g.gameId)}
          <button class="chip mine-chip" onclick={() => nav.openTsGame(tsStore.communityOf(g))}>
            {#if g.icon}<img src={g.icon} alt="" width="18" height="18" />{/if}
            {g.gameName}
            <span class="muted">{tsStore.count(g)}</span>
          </button>
        {/each}
      </div>
    </section>
  {/if}

  {#if !search && onPc.length > 0}
    <section class="on-pc" aria-label={tr("Jeux installés sur ce PC", "Games installed on this PC")}>
      <div class="section-head">
        <h2>{tr("Installés sur ce PC", "Installed on this PC")}</h2>
        <span class="muted">{onPc.length}</span>
      </div>
      <ul class="card-grid posters">
        {#each onPc as { community: c, launcher } (c.id)}
          {@const config = tsStore.game(c.id)}
          {@const n = config ? tsStore.count(config) : 0}
          <li>
            <button class="card" onclick={() => nav.openTsGame(c)}>
              <Thumb
                src={c.coverImageUrl}
                label={c.name}
                badges={[
                  { text: launcherNames[launcher] ?? launcher, tone: "neutral" as const },
                  ...(n > 0 ? [{ text: `${n} ${n > 1 ? "mods" : "mod"}`, tone: "ok" as const }] : []),
                  ...(!c.supported ? [{ text: tr("Pas encore pris en charge", "Not supported yet"), tone: "neutral" as const }] : []),
                ]}
              />
              <span class="card-title">{c.name}</span>
              <span class="card-meta"><span>{exact(c.totalPackageCount)} mods</span></span>
            </button>
          </li>
        {/each}
      </ul>
    </section>
    <h2 class="all-title">{tr("Tous les jeux", "All games")}</h2>
  {/if}

  {#if tsStore.communitiesError && !tsStore.communities}
    <ErrorNotice error={tsStore.communitiesError} onretry={() => tsStore.loadCommunities(true)} />
  {:else if !tsStore.communities}
    <ul class="card-grid posters" aria-busy="true" aria-label={tr("Chargement des jeux", "Loading games")}>
      {#each Array(12) as _, i (i)}
        <li class="skeleton"><div class="thumb"></div><div class="line"></div></li>
      {/each}
    </ul>
  {:else if all.length === 0}
    <div class="empty">
      <p>{tr(`Aucun jeu ne correspond à « ${query.trim()} ».`, `No games match "${query.trim()}".`)}</p>
      <button class="btn" onclick={() => (query = "")}>{tr("Effacer la recherche", "Clear search")}</button>
    </div>
  {:else}
    <ul class="card-grid posters">
      {#each all.slice(0, shown) as c (c.id)}
        {@const config = tsStore.game(c.id)}
        {@const n = config ? tsStore.count(config) : 0}
        <li>
          <button class="card" onclick={() => nav.openTsGame(c)}>
            <Thumb
              src={c.coverImageUrl}
              label={c.name}
              badges={[
                ...(n > 0 ? [{ text: `${n} ${n > 1 ? "mods" : "mod"}`, tone: "ok" as const }] : []),
                ...(!c.supported ? [{ text: tr("Pas encore pris en charge", "Not supported yet"), tone: "neutral" as const }] : []),
              ]}
            />
            <span class="card-title">{c.name}</span>
            <span class="card-meta">
              <span>{exact(c.totalPackageCount)} mods</span>
              <span title={tr("Téléchargements", "Downloads")}><Icon name="download" size={14} />{count(c.totalDownloadCount)}</span>
            </span>
            {#if c.loader}<span class="card-meta"><span>{c.loader}</span></span>{/if}
          </button>
        </li>
      {/each}
    </ul>
    {#if all.length > shown}
      <!-- La suite s'affiche en défilant ; recréé à chaque ajout pour remplir l'écran. -->
      {#key shown}
        <div class="more" use:inview={() => (shown = Math.min(shown + PAGE, all.length))}>
          <span class="muted">{tr(`${exact(shown)} jeux affichés sur ${exact(all.length)}`, `Showing ${exact(shown)} of ${exact(all.length)} games`)}</span>
        </div>
      {/key}
    {/if}
  {/if}
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
    margin-bottom: 1.25rem;
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

  .intro {
    max-width: 46rem;
    font-size: var(--t-sm);
    margin-bottom: 2rem;
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
    object-fit: cover;
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

  .all-title {
    margin-bottom: 0.9rem;
  }

  .posters {
    grid-template-columns: repeat(auto-fill, minmax(calc(135px * var(--thumb-scale)), 1fr));
    gap: 1.5rem 1.1rem;
  }

  /* Affiches de Thunderstore : 360 × 480. */
  .posters :global(.thumb) {
    aspect-ratio: 3 / 4;
  }

  .posters :global(.card-title) {
    font-size: var(--t-md);
  }

  .skeleton .thumb {
    aspect-ratio: 3 / 4;
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
  }
</style>
