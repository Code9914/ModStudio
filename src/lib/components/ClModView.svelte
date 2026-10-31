<script lang="ts">
  // Fiche d'un mod d'une liste de communauté.
  import { onMount } from "svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import type { ClDep, ClGame } from "$lib/api";
  import { date, exact } from "$lib/format";
  import { tr } from "$lib/i18n.svelte";
  import { listsStore } from "$lib/lists.svelte";
  import { nav } from "$lib/nav.svelte";
  import { isoToUnix } from "$lib/nexus.svelte";
  import ClInstallPanel from "./ClInstallPanel.svelte";
  import ErrorNotice from "./ErrorNotice.svelte";
  import Icon from "./Icon.svelte";
  import Thumb from "./Thumb.svelte";

  let { game, id }: { game: ClGame; id: string } = $props();

  let panel = $state<ClInstallPanel | null>(null);

  onMount(() => {
    listsStore.loadMods(game);
    if (!listsStore.loaders[game.id]) listsStore.refreshLoader(game);
  });

  const mods = $derived(listsStore.mods[game.id] ?? null);
  const error = $derived(listsStore.errors[game.id] ?? null);
  const mod = $derived(listsStore.mod(game, id));
  const record = $derived(mod ? listsStore.installed(game.gameId, mod.modId) : null);
  const job = $derived(listsStore.progress[game.gameId] ?? null);
  const current = $derived(mod?.versions[0] ?? null);
  const deps = $derived((current?.dependencies ?? []).filter((d) => !d.optional));
  const optional = $derived((current?.dependencies ?? []).filter((d) => d.optional));

  function open(url: string | null | undefined) {
    if (url && /^https:\/\//i.test(url)) openUrl(url);
  }

  function installedDep(d: ClDep) {
    const dep = listsStore.mod(game, d.id);
    return !!dep && !!listsStore.installed(game.gameId, dep.modId);
  }

  const VERSIONS_SHOWN = 6;
  let allVersions = $state(false);
</script>

<div class="view">
  <div class="top">
    <button class="btn btn-ghost crumb" onclick={() => nav.back()}>
      <Icon name="back" />{nav.clStack.length > 1
        ? tr("Retour", "Back")
        : nav.clOrigin === "installed"
          ? tr("Retour aux mods installés", "Back to installed mods")
          : tr(`Retour à ${game.name}`, `Back to ${game.name}`)}
    </button>
  </div>

  {#if error && !mods}
    <div class="pad"><ErrorNotice {error} onretry={() => listsStore.loadMods(game, true)} /></div>
  {:else if !mods}
    <div class="pad skeleton" aria-busy="true" aria-label={tr("Chargement du mod", "Loading mod")}>
      <div class="line title"></div>
      <div class="line short"></div>
      <div class="line"></div>
    </div>
  {:else if !mod}
    <div class="pad"><p>{tr(`Ce mod n'est plus dans la liste ${game.listName}.`, `This mod is no longer in the ${game.listName} list.`)}</p></div>
  {:else}
    <header class="head">
      <div class="icon"><Thumb src={mod.icon} fallback={mod.iconFallback} label={mod.name} /></div>
      <div class="head-text">
        <h1>{mod.name}</h1>
        <p class="byline">
          {#if mod.author}<span>{tr("par", "by")} <strong>{mod.author}</strong></span>{/if}
          {#if mod.updated}<span class="muted">{tr("mis à jour le", "updated on")} {date(isoToUnix(mod.updated))}</span>{/if}
          <span class="chip">{game.listName}</span>
          {#if mod.category}<span class="chip">{mod.category}</span>{/if}
          {#if mod.loader}<span class="chip">{tr("Chargeur de mods", "Mod loader")}</span>{/if}
          {#if mod.broken}<span class="chip chip-warn">{tr("Cassé", "Broken")}</span>{:else if mod.deprecated}<span class="chip chip-warn">{tr("Obsolète", "Outdated")}</span>{/if}
        </p>
      </div>
      <div class="actions">
        {#if mod.page}
          <button class="btn" onclick={() => open(mod.page)}>{tr("Page du mod", "Mod page")} <Icon name="external" size={14} /></button>
        {/if}
      </div>
    </header>

    <div class="layout">
      <main class="main">
        {#if mod.note}
          <p class="warn small"><Icon name="warning" size={15} /><span>{mod.note}</span></p>
        {/if}
        {#if mod.description}
          <p class="description">{mod.description}</p>
        {:else}
          <p class="muted">{tr("Pas de description dans la liste : la page du mod en dit plus.", "No description in the list: the mod page has more details.")}</p>
        {/if}

        <section class="versions">
          <h2>{mod.versions.length > 1 ? "Versions" : "Version"}</h2>
          <ol class="version-list">
            {#each allVersions ? mod.versions : mod.versions.slice(0, VERSIONS_SHOWN) as v, i (v.version)}
              <li class="version">
                <div class="version-head">
                  <span class="version-name">{v.version}</span>
                  {#if i === 0}<span class="chip">{tr("Dernière", "Latest")}</span>{/if}
                  {#if record?.version === v.version}<span class="chip chip-ok">{tr("Installée", "Installed")}</span>{/if}
                  {#each v.gameVersions as gv (gv)}<span class="muted small">{gv}</span>{/each}
                  {#if v.releaseUrl}
                    <button class="btn btn-ghost small-btn" onclick={() => open(v.releaseUrl)}>
                      {tr("Notes de version", "Release notes")} <Icon name="external" size={12} />
                    </button>
                  {/if}
                  {#if i > 0 && record?.version !== v.version && listsStore.config(game) && !mod.broken && mod.compatible}
                    <button class="btn btn-ghost small-btn" onclick={() => panel?.install(v.version)} disabled={!!job}>
                      {tr("Installer celle-ci", "Install this one")}
                    </button>
                  {/if}
                </div>
                {#if v.changelog}<p class="changelog small">{v.changelog}</p>{/if}
              </li>
            {/each}
          </ol>
          {#if mod.versions.length > VERSIONS_SHOWN}
            <button class="btn btn-ghost" onclick={() => (allVersions = !allVersions)}>
              {allVersions ? tr("Moins de versions", "Fewer versions") : tr(`Les ${mod.versions.length} versions`, `All ${mod.versions.length} versions`)}
            </button>
          {/if}
        </section>
      </main>

      <aside class="side">
        {#key mod.modId}
          <ClInstallPanel bind:this={panel} {game} {mod} />
        {/key}
        <dl class="stats">
          <div>
            <dt>Version</dt>
            <dd>{mod.latest}</dd>
          </div>
          {#if mod.downloads !== null}
            <div>
              <dt>{tr("Téléchargements", "Downloads")}</dt>
              <dd>{exact(mod.downloads)}</dd>
            </div>
          {/if}
          {#if current?.files.length}
            <div>
              <dt>{tr("Fichiers installés", "Installed files")}</dt>
              <dd class="files">
                {#each current.files.slice(0, 8) as f (f)}<code>{f.replace("{data}", tr("<données du jeu>", "<game data>"))}</code>{/each}
                {#if current.files.length > 8}<span class="muted">{tr(`et ${current.files.length - 8} autres`, `and ${current.files.length - 8} more`)}</span>{/if}
              </dd>
            </div>
          {/if}
          {#if mod.tags.length}
            <div>
              <dt>{tr("Mots-clés", "Tags")}</dt>
              <dd class="small-dd">{mod.tags.join(", ")}</dd>
            </div>
          {/if}
        </dl>

        {#if deps.length}
          <section class="side-block">
            <h2>{tr("Nécessite aussi", "Also requires")}</h2>
            <ul class="deps">
              {#each deps as d (d.id)}
                <li>
                  <button class="dep" onclick={() => listsStore.mod(game, d.id) && nav.openClMod(d.id)} disabled={!listsStore.mod(game, d.id)}>
                    <span class="dep-text"><span class="dep-name">{d.name}</span></span>
                    {#if installedDep(d)}<span class="dep-ok">{tr("Installé", "Installed")}</span>{/if}
                  </button>
                </li>
              {/each}
            </ul>
            <p class="muted small">{tr("ModStudio les installe en même temps que ce mod.", "ModStudio installs them along with this mod.")}</p>
          </section>
        {/if}

        {#if optional.length}
          <section class="side-block">
            <h2>{tr("Peut aussi utiliser", "Can also use")}</h2>
            <ul class="deps">
              {#each optional as d (d.id)}
                <li>
                  <button class="dep" onclick={() => listsStore.mod(game, d.id) && nav.openClMod(d.id)} disabled={!listsStore.mod(game, d.id)}>
                    <span class="dep-text"><span class="dep-name">{d.name}</span></span>
                    {#if installedDep(d)}<span class="dep-ok">{tr("Installé", "Installed")}</span>{/if}
                  </button>
                </li>
              {/each}
            </ul>
          </section>
        {/if}
      </aside>
    </div>
  {/if}
</div>

<style>
  .view {
    height: 100%;
    overflow-y: auto;
    padding-bottom: 3rem;
  }

  .top {
    padding: 1.25rem 2.5rem 0;
  }

  .crumb {
    margin-left: -0.5rem;
  }

  .pad {
    padding: 1.5rem 2.5rem;
  }

  .head {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-end;
    gap: 1.25rem 1.75rem;
    padding: 1rem 2.5rem 1.75rem;
  }

  .icon {
    width: 7.5rem;
    flex: none;
  }

  .icon :global(.thumb) {
    aspect-ratio: 1;
    border-radius: var(--r-md);
    overflow: hidden;
    background: var(--raise);
  }

  .icon :global(.thumb img) {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .head-text {
    display: grid;
    gap: 0.5rem;
    flex: 1 1 24rem;
    max-width: 46rem;
  }

  h1 {
    font-size: clamp(var(--t-3xl), 4.2vw, var(--t-4xl));
    font-weight: 800;
    letter-spacing: -0.025em;
    text-wrap: balance;
    overflow-wrap: anywhere;
  }

  .byline {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.4rem 1rem;
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    justify-content: flex-end;
    gap: 0.6rem;
    margin-left: auto;
  }

  .layout {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 19rem;
    gap: 3rem;
    padding: 0 2.5rem;
  }

  .main {
    display: grid;
    align-content: start;
    gap: 1.75rem;
    min-width: 0;
  }

  .description {
    max-width: 46rem;
    font-size: var(--t-lg);
    line-height: 1.55;
    white-space: pre-line;
    overflow-wrap: anywhere;
  }

  .warn {
    display: flex;
    align-items: flex-start;
    gap: 0.45rem;
    max-width: 42rem;
    padding: 0.7rem 0.9rem;
    border-radius: var(--r-md);
    background: var(--panel);
  }

  .warn :global(svg) {
    flex: none;
    color: var(--warn, var(--accent));
    margin-top: 0.15rem;
  }

  h2 {
    font-size: var(--t-xl);
    font-weight: 700;
    margin-bottom: 0.9rem;
  }

  .version-list {
    list-style: none;
    margin: 0 0 0.6rem;
    padding: 0;
    display: grid;
    gap: 0.4rem;
    max-width: 46rem;
  }

  .version {
    display: grid;
    gap: 0.35rem;
    padding: 0.55rem 0.8rem;
    border-radius: var(--r-md);
    background: var(--panel);
  }

  .version-head {
    min-height: 1.8rem;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.4rem 0.9rem;
  }

  .version-name {
    font-family: var(--font-display);
    font-weight: 650;
  }

  .changelog {
    color: var(--muted);
    white-space: pre-line;
    overflow-wrap: anywhere;
  }

  .small-btn {
    font-size: var(--t-sm);
  }

  .version-head .small-btn:first-of-type {
    margin-left: auto;
  }

  .side {
    display: grid;
    align-content: start;
    gap: 2rem;
    min-width: 0;
  }

  .stats {
    margin: 0;
    display: grid;
    gap: 1.1rem;
    padding: 1.25rem;
    border-radius: var(--r-md);
    background: var(--panel);
  }

  .stats div {
    display: grid;
    gap: 0.1rem;
  }

  dt {
    font-size: var(--t-sm);
    color: var(--muted);
  }

  dd {
    margin: 0;
    font-family: var(--font-display);
    font-size: var(--t-xl);
    font-weight: 650;
  }

  dd.small-dd {
    font-size: var(--t-md);
  }

  dd.files {
    display: grid;
    gap: 0.25rem;
    font-family: inherit;
    font-size: var(--t-xs);
    font-weight: 500;
  }

  dd.files code {
    overflow-wrap: anywhere;
  }

  .side-block h2 {
    font-size: var(--t-md);
    margin-bottom: 0.6rem;
  }

  .deps {
    list-style: none;
    margin: 0 0 0.6rem;
    padding: 0;
    display: grid;
    gap: 0.4rem;
  }

  .dep {
    display: flex;
    align-items: center;
    gap: 0.6rem;
    width: 100%;
    padding: 0.45rem 0.6rem;
    border: 0;
    border-radius: var(--r-md);
    background: var(--panel);
    text-align: left;
    color: var(--text);
  }

  .dep:disabled {
    cursor: default;
  }

  .dep-text {
    display: grid;
    min-width: 0;
  }

  .dep:not(:disabled):hover .dep-name {
    color: var(--accent);
  }

  .dep-name {
    font-weight: 600;
    font-size: var(--t-sm);
    overflow-wrap: anywhere;
  }

  .dep-ok {
    margin-left: auto;
    font-size: var(--t-xs);
    font-weight: 600;
    color: var(--ok);
  }

  .small {
    font-size: var(--t-sm);
  }

  .skeleton {
    display: grid;
    gap: 0.9rem;
  }

  .skeleton .line {
    height: 1.2rem;
    border-radius: var(--r-sm);
    background: var(--raise);
  }

  .skeleton .title {
    height: 2.6rem;
    width: 45%;
  }

  .skeleton .short {
    width: 25%;
  }

  @media (max-width: 1050px) {
    .layout {
      grid-template-columns: 1fr;
    }
  }
</style>
