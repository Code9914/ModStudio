<script lang="ts">
  // Recherche globale : jeux et mods de toutes les sources sur une seule page.
  import { count } from "$lib/format";
  import { tr } from "$lib/i18n.svelte";
  import { nav } from "$lib/nav.svelte";
  import { search, searchSources, type SearchGroup } from "$lib/search.svelte";
  import ErrorNotice from "./ErrorNotice.svelte";
  import Icon from "./Icon.svelte";

  let input = $state<HTMLInputElement | null>(null);

  // Le champ prend le focus à chaque ouverture de la page.
  $effect(() => {
    if (nav.view.name === "search") queueMicrotask(() => input?.select());
  });

  function onKey(event: KeyboardEvent) {
    if (event.key !== "Escape") return;
    event.preventDefault();
    if (search.query) search.type("");
    else nav.back();
  }

  /** Groupes affichés : avec des résultats, ou en cours pour les jeux du joueur. */
  const visible = $derived(
    search.groups
      .filter((g) => g.hits.length > 0 || (g.loading && g.mine) || (g.error && g.mine))
      .sort((a, b) => rank(a) - rank(b)),
  );

  /** Jeux du joueur d'abord (Thunderstore, mod.io, listes), puis Nexus, puis les autres listes. */
  function rank(g: SearchGroup) {
    if (g.source === "nexus") return 1;
    return g.mine ? 0 : 2;
  }

  const done = $derived(!search.busy);
  const nothing = $derived(done && search.term && search.games.length === 0 && visible.length === 0);
</script>

<div class="view">
  <header class="head">
    <h1>{tr("Rechercher", "Search")}</h1>
    <label class="big-search">
      <span class="visually-hidden">{tr("Rechercher un mod ou un jeu", "Search for a mod or a game")}</span>
      <Icon name="search" size={20} />
      <input
        bind:this={input}
        class="field"
        type="search"
        placeholder={tr("Un mod ou un jeu, dans toutes les sources", "A mod or a game, across every source")}
        value={search.query}
        oninput={(e) => search.type(e.currentTarget.value)}
        onkeydown={onKey}
        autocomplete="off"
        spellcheck="false"
      />
      {#if search.busy}<span class="spinner" aria-label={tr("Recherche en cours", "Searching")}></span>{/if}
    </label>
    <p class="scope muted">
      {tr(
        "Nexus Mods : tous les jeux. Thunderstore et mod.io : tes jeux (avec des mods, ou installés sur ce PC). Communautés : toutes leurs listes.",
        "Nexus Mods: every game. Thunderstore and mod.io: your games (with mods, or installed on this PC). Communities: all their lists.",
      )}
    </p>
  </header>

  {#if !search.term}
    <div class="intro">
      <p>
        {tr(
          "Tape au moins deux lettres : le nom d'un mod (« More Company », « SkyUI »…) ou d'un jeu.",
          'Type at least two letters: the name of a mod ("More Company", "SkyUI"…) or of a game.',
        )}
      </p>
      <p class="muted small">
        {tr("Raccourci : Ctrl + K depuis n'importe quelle page.", "Shortcut: Ctrl + K from any page.")}
      </p>
    </div>
  {:else}
    {#if search.games.length}
      <section class="games" aria-label={tr("Jeux", "Games")}>
        <h2>{tr("Jeux", "Games")}</h2>
        <ul class="game-list">
          {#each search.games as g (g.key)}
            <li>
              <button class="game" onclick={g.open}>
                {#if g.image}
                  <img src={g.image} alt="" width="40" height="40" loading="lazy" onerror={(e) => ((e.currentTarget as HTMLImageElement).style.visibility = "hidden")} />
                {:else}
                  <span class="game-gap" aria-hidden="true"></span>
                {/if}
                <span class="game-text">
                  <span class="game-name">{g.name}</span>
                  <span class="game-meta muted">{searchSources[g.source]} · {g.detail}</span>
                </span>
              </button>
            </li>
          {/each}
        </ul>
      </section>
    {/if}

    {#each visible as g (g.key)}
      <section class="group" aria-label={g.title}>
        <div class="group-head">
          {#if g.image}
            <img class="group-icon" src={g.image} alt="" width="28" height="28" onerror={(e) => ((e.currentTarget as HTMLImageElement).style.visibility = "hidden")} />
          {/if}
          <h2>{g.title}</h2>
          {#if g.source !== "nexus"}<span class="chip">{searchSources[g.source]}</span>{/if}
          {#if !g.loading && g.total > 0}
            <span class="muted small">{tr(`${g.total} ${g.total > 1 ? "résultats" : "résultat"}`, `${g.total} ${g.total === 1 ? "result" : "results"}`)}</span>
          {/if}
        </div>

        {#if g.error && g.hits.length === 0}
          <ErrorNotice error={g.error} onretry={() => search.retry()} />
        {:else if g.loading && g.hits.length === 0}
          <ul class="hits" aria-busy="true">
            {#each Array(3) as _, i (i)}
              <li class="skeleton"><span class="thumb"></span><span class="line"></span></li>
            {/each}
          </ul>
        {:else}
          <ul class="hits">
            {#each g.hits.slice(0, g.shown) as h (h.key)}
              <li>
                <button class="hit" onclick={h.open}>
                  {#if h.image}
                    <img class="thumb" src={h.image} alt="" loading="lazy" onerror={(e) => ((e.currentTarget as HTMLImageElement).style.visibility = "hidden")} />
                  {:else}
                    <span class="thumb initial" aria-hidden="true">{(h.name.trim()[0] ?? "?").toUpperCase()}</span>
                  {/if}
                  <span class="hit-text">
                    <span class="hit-title">
                      <span class="hit-name">{h.name}</span>
                      {#if h.installed()}<span class="chip chip-ok">{tr("Installé", "Installed")}</span>{/if}
                      {#if h.flag}<span class="chip chip-warn">{h.flag}</span>{/if}
                    </span>
                    {#if h.summary}<span class="hit-summary muted">{h.summary}</span>{/if}
                    <span class="hit-meta muted">
                      {#if h.game}<span class="hit-game">{h.game}</span>{/if}
                      {#if h.author}<span>{tr("par", "by")} {h.author}</span>{/if}
                      {#if h.downloads !== null}<span class="downloads"><Icon name="download" size={13} />{count(h.downloads)}</span>{/if}
                    </span>
                  </span>
                </button>
              </li>
            {/each}
          </ul>
          {#if g.openList && g.total > g.shown}
            <button class="btn btn-ghost more" onclick={g.openList}>
              {tr(`Voir les ${g.total} résultats dans ${g.title}`, `See all ${g.total} results in ${g.title}`)}
            </button>
          {:else if !g.openList && (g.shown < g.hits.length || g.hits.length < g.total)}
            <button class="btn btn-ghost more" onclick={() => search.more(g)} disabled={g.loading}>
              {g.loading ? tr("Chargement…", "Loading…") : tr("Plus de résultats", "More results")}
            </button>
          {/if}
        {/if}
      </section>
    {/each}

    {#if nothing}
      <div class="empty">
        <p>{tr(`Rien trouvé pour « ${search.term} ».`, `Nothing found for "${search.term}".`)}</p>
        <p class="muted small">
          {tr(
            "Essaie un autre mot, ou une partie du nom seulement. Pour Thunderstore et mod.io, ouvre d'abord le jeu : seuls tes jeux sont parcourus ici.",
            "Try another word, or just part of the name. For Thunderstore and mod.io, open the game first: only your games are searched here.",
          )}
        </p>
      </div>
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
    gap: 2rem;
  }

  .head {
    display: grid;
    gap: 0.75rem;
    max-width: 52rem;
  }

  h1 {
    font-size: var(--t-3xl);
    font-weight: 800;
  }

  h2 {
    font-size: var(--t-md);
    font-weight: 700;
  }

  .big-search {
    position: relative;
    display: block;
    color: var(--muted);
  }

  .big-search :global(svg) {
    position: absolute;
    left: 0.95rem;
    top: 50%;
    translate: 0 -50%;
    pointer-events: none;
  }

  .big-search .field {
    padding: 0.85rem 2.75rem 0.85rem 2.9rem;
    font-size: var(--t-lg);
    border-radius: var(--r-md);
  }

  .spinner {
    position: absolute;
    right: 1rem;
    top: 50%;
    width: 1.1rem;
    height: 1.1rem;
    margin-top: -0.55rem;
    border: 2px solid var(--line);
    border-top-color: var(--accent);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  @keyframes spin {
    to {
      rotate: 1turn;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .spinner {
      animation-duration: 2.4s;
    }
  }

  .scope {
    font-size: var(--t-sm);
  }

  .small {
    font-size: var(--t-sm);
  }

  .intro {
    display: grid;
    gap: 0.4rem;
    max-width: 46rem;
  }

  /* ------------------------------------------------------------ Jeux */

  .games {
    display: grid;
    gap: 0.75rem;
  }

  .game-list {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(15rem, 1fr));
    gap: 0.6rem;
    list-style: none;
    margin: 0;
    padding: 0;
  }

  .game {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    width: 100%;
    padding: 0.55rem 0.7rem;
    border-radius: var(--r-md);
    border: 1px solid var(--line);
    background: var(--raise);
    text-align: left;
  }

  .game:hover {
    border-color: var(--muted);
  }

  .game img,
  .game-gap {
    flex: none;
    width: 40px;
    height: 40px;
    border-radius: var(--r-sm);
    object-fit: cover;
  }

  .game-text {
    display: grid;
    min-width: 0;
  }

  .game-name {
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .game-meta {
    font-size: var(--t-xs);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* ------------------------------------------------------------ Mods */

  .group {
    display: grid;
    gap: 0.6rem;
    max-width: 60rem;
  }

  .group-head {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.4rem 0.7rem;
  }

  .group-icon {
    border-radius: var(--r-sm);
    object-fit: cover;
  }

  .hits {
    display: grid;
    gap: 0.25rem;
    list-style: none;
    margin: 0;
    padding: 0;
  }

  .hit {
    display: flex;
    align-items: flex-start;
    gap: 0.9rem;
    width: 100%;
    padding: 0.55rem 0.6rem;
    border: 1px solid transparent;
    border-radius: var(--r-md);
    background: none;
    text-align: left;
  }

  .hit:hover {
    background: var(--raise);
  }

  .thumb {
    flex: none;
    width: calc(96px * var(--thumb-scale, 1));
    aspect-ratio: 16 / 9;
    border-radius: var(--r-sm);
    object-fit: cover;
    background: var(--raise);
  }

  .thumb.initial {
    display: grid;
    place-items: center;
    font-family: var(--font-display);
    font-size: var(--t-xl);
    font-weight: 800;
    color: var(--muted);
  }

  .hit-text {
    display: grid;
    gap: 0.15rem;
    min-width: 0;
  }

  .hit-title {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.25rem 0.5rem;
  }

  .hit-name {
    font-weight: 650;
  }

  .hit-summary {
    font-size: var(--t-sm);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .hit-meta {
    display: flex;
    flex-wrap: wrap;
    gap: 0.2rem 0.8rem;
    font-size: var(--t-xs);
  }

  .hit-game {
    font-weight: 600;
    color: var(--text);
  }

  .downloads {
    display: inline-flex;
    align-items: center;
    gap: 0.25rem;
  }

  .skeleton {
    display: flex;
    align-items: center;
    gap: 0.9rem;
    padding: 0.55rem 0.6rem;
  }

  .skeleton .thumb {
    display: block;
  }

  .skeleton .line {
    display: block;
    height: 0.9rem;
    width: min(18rem, 50%);
    border-radius: var(--r-sm);
    background: var(--raise);
  }

  .more {
    justify-self: start;
    color: var(--accent);
  }

  .empty {
    display: grid;
    gap: 0.4rem;
    max-width: 46rem;
  }
</style>
