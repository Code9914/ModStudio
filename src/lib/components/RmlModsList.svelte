<script lang="ts">
  // Mods de ResoniteModLoader (liste vérifiée de la communauté), dans la page de Resonite.
  import { onMount } from "svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import type { TsCommunity } from "$lib/api";
  import { exact } from "$lib/format";
  import { english, tr } from "$lib/i18n.svelte";
  import { nav } from "$lib/nav.svelte";
  import { inview } from "$lib/paged.svelte";
  import { normalizeName, rmlCategory, rmlPlatforms, tsStore } from "$lib/thunderstore.svelte";
  import ErrorNotice from "./ErrorNotice.svelte";
  import Icon from "./Icon.svelte";
  import Thumb from "./Thumb.svelte";

  let { game }: { game: TsCommunity } = $props();

  let query = $state("");
  let category = $state("");
  let showAll = $state(false);
  const PAGE = 48;
  let shown = $state(PAGE);

  onMount(() => {
    tsStore.loadRml();
    tsStore.refreshRmlStatus();
  });

  const status = $derived(tsStore.rmlStatus);
  const search = $derived(normalizeName(query));
  $effect(() => {
    search;
    category;
    showAll;
    shown = PAGE;
  });

  const categories = $derived(
    [...new Set((tsStore.rmlMods ?? []).map((m) => m.category))].sort((a, b) => rmlCategory(a).localeCompare(rmlCategory(b), "fr")),
  );

  const list = $derived.by(() => {
    const mods = tsStore.rmlMods ?? [];
    return mods.filter((m) => {
      const installed = !!tsStore.installed(game.id, m.modId);
      // Obsolètes, cassés et mods de serveur : cachés, sauf s'ils sont installés.
      if (!showAll && !installed && (m.deprecated || m.broken || !m.compatible)) return false;
      if (category && m.category !== category) return false;
      if (!search) return true;
      return [m.name, m.description, m.author, ...m.tags].some((t) => normalizeName(t).includes(search));
    });
  });
</script>

<div class="rml">
  <div class="about small">
    <p>
      {tr(
        "Mods pour ResoniteModLoader (RML), un autre chargeur que BepisLoader. La liste est vérifiée par la communauté (c'est celle de Resolute) et chaque fichier est contrôlé avant d'être installé.",
        "Mods for ResoniteModLoader (RML), a different mod loader from BepisLoader. The list is verified by the community (it's the one Resolute uses) and every file is checked before it's installed.",
      )}
    </p>
    {#if status?.present && !status.managed && !status.viaBepis}
      <p>
        {#if english}
          ResoniteModLoader is already installed manually: it starts with the
          <code>{status.launchOption}</code> option in the game's properties on Steam. ModStudio leaves it alone.
        {:else}
          ResoniteModLoader est déjà installé à la main : il se lance avec l'option
          <code>{status.launchOption}</code> dans les propriétés du jeu sur Steam. ModStudio n'y touche pas.
        {/if}
      </p>
    {:else if status?.present && status.viaBepis}
      <p class="ok">
        <Icon name="check" size={15} />{tr(
          `ResoniteModLoader${status.version ? ` ${status.version}` : ""} est installé et chargé par BepisLoader : pas d'option de lancement à ajouter.`,
          `ResoniteModLoader${status.version ? ` ${status.version}` : ""} is installed and loaded by BepisLoader: no launch option to add.`,
        )}
      </p>
    {:else}
      <p>
        {tr(
          "ModStudio installe ResoniteModLoader avec le premier mod, ainsi que le plugin qui le fait charger par BepisLoader : pas d'option de lancement à ajouter dans Steam.",
          "ModStudio installs ResoniteModLoader with the first mod, along with the plugin that makes BepisLoader load it: no launch option to add in Steam.",
        )}
      </p>
    {/if}
  </div>

  <div class="toolbar">
    <label class="search">
      <span class="visually-hidden">{tr("Rechercher dans les mods de ResoniteModLoader", "Search ResoniteModLoader mods")}</span>
      <Icon name="search" />
      <input class="field" type="search" placeholder={tr("Rechercher dans les mods", "Search mods")} bind:value={query} />
    </label>
    <label>
      <span class="visually-hidden">{tr("Catégorie", "Category")}</span>
      <select class="field" bind:value={category}>
        <option value="">{tr("Toutes les catégories", "All categories")}</option>
        {#each categories as c (c)}
          <option value={c}>{rmlCategory(c)}</option>
        {/each}
      </select>
    </label>
    <label class="check small">
      <input type="checkbox" bind:checked={showAll} />
      {tr("Afficher les mods obsolètes, cassés ou pour serveur", "Show outdated, broken or server mods")}
    </label>
    {#if tsStore.rmlMods}<p class="muted">{exact(list.length)} mods</p>{/if}
  </div>

  {#if tsStore.rmlError && !tsStore.rmlMods}
    <ErrorNotice error={tsStore.rmlError} onretry={() => tsStore.loadRml(true)} />
  {:else if !tsStore.rmlMods}
    <ul class="card-grid" aria-busy="true" aria-label={tr("Chargement des mods", "Loading mods")}>
      {#each Array(12) as _, i (i)}
        <li class="skeleton"><div class="thumb"></div><div class="line"></div></li>
      {/each}
    </ul>
  {:else if list.length === 0}
    <div class="empty">
      <p>{tr(`Aucun mod ne correspond${query.trim() ? ` à « ${query.trim()} »` : ""}.`, `No mods match${query.trim() ? ` "${query.trim()}"` : ""}.`)}</p>
      {#if query || category}
        <button class="btn" onclick={() => ((query = ""), (category = ""))}>{tr("Effacer les filtres", "Clear filters")}</button>
      {/if}
    </div>
  {:else}
    <ul class="card-grid">
      {#each list.slice(0, shown) as m (m.id)}
        {@const record = tsStore.installed(game.id, m.modId)}
        <li>
          <button class="card" onclick={() => nav.openRmlMod(m.id)}>
            <Thumb
              src={m.authorIcon}
              fallback={m.authorAvatar}
              label={m.name}
              badges={[
                ...(record
                  ? tsStore.outdated(game.id, m.modId)
                    ? [{ text: tr("Mise à jour", "Update"), tone: "accent" as const }]
                    : [{ text: tr("Installé", "Installed"), tone: "ok" as const }]
                  : []),
                ...(m.broken ? [{ text: tr("Cassé", "Broken"), tone: "warn" as const }] : m.deprecated ? [{ text: tr("Obsolète", "Outdated"), tone: "warn" as const }] : []),
                ...(!m.compatible ? [{ text: rmlPlatforms(m.platforms), tone: "neutral" as const }] : []),
              ]}
            />
            <span class="card-title">{m.name}</span>
            <span class="card-meta"><span class="author">{tr("par", "by")} {m.author}</span></span>
            {#if m.description}<span class="desc">{m.description}</span>{/if}
            <span class="card-meta">
              <span>{rmlCategory(m.category)}</span>
              <span>{m.latest}</span>
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

  <p class="muted small source">
    {tr("Liste :", "List:")}
    <button class="link" onclick={() => openUrl("https://github.com/resonite-modding-group/resonite-mod-manifest")}>
      resonite-mod-manifest <Icon name="external" size={12} />
    </button>
  </p>
</div>

<style>
  .rml {
    display: grid;
    gap: 1.5rem;
  }

  .about {
    display: grid;
    gap: 0.5rem;
    max-width: 50rem;
    color: var(--muted);
  }

  .about code {
    padding: 0.1em 0.4em;
    border-radius: var(--r-sm);
    background: var(--raise);
    color: var(--text);
    font-size: 0.92em;
    user-select: all;
  }

  .ok {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    color: var(--text);
  }

  .ok :global(svg) {
    color: var(--ok);
    flex: none;
  }

  .toolbar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.75rem 1rem;
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

  /* Pas d'icônes dans la liste : le portrait de l'auteur, en carré. */
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
  }

  .empty {
    display: grid;
    justify-items: start;
    gap: 1rem;
    padding: 2rem 0;
  }

  .small {
    font-size: var(--t-sm);
  }

  .link {
    display: inline-flex;
    align-items: center;
    gap: 0.25rem;
    padding: 0;
    border: 0;
    background: none;
    color: var(--accent);
    font: inherit;
    cursor: pointer;
  }
</style>
