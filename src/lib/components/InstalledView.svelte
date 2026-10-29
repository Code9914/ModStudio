<script lang="ts">
  import { untrack } from "svelte";
  import { SvelteMap, SvelteSet } from "svelte/reactivity";
  import { api, asAppError, type AppError, type GameInstall, type InstalledMod } from "$lib/api";
  import { ago, bytes, exact } from "$lib/format";
  import { installs, layoutLabels } from "$lib/installs.svelte";
  import { nav } from "$lib/nav.svelte";
  import ErrorNotice from "./ErrorNotice.svelte";
  import FolderSetup from "./FolderSetup.svelte";
  import Icon from "./Icon.svelte";
  import ProgressBar from "./ProgressBar.svelte";
  import Thumb from "./Thumb.svelte";

  let error = $state<AppError | null>(null);
  let query = $state("");
  /** Jeu dont on modifie le dossier. */
  let editing = $state<number | null>(null);
  const rowErrors = new SvelteMap<number, string>();
  const notes = new SvelteMap<number, string>();
  const confirming = new SvelteSet<number>();
  const removing = new SvelteSet<number>();
  const updatingAll = new SvelteSet<number>();

  async function refresh() {
    error = null;
    try {
      await installs.load();
      await Promise.all(Object.keys(installs.games).map((id) => installs.checkUpdates(Number(id))));
    } catch (e) {
      error = asAppError(e);
    }
  }

  // Rafraîchi à chaque visite de la page.
  $effect(() => {
    if (nav.view.name === "installed") untrack(() => refresh());
  });

  const games = $derived(
    Object.values(installs.games).sort((a, b) => a.gameName.localeCompare(b.gameName, "fr")),
  );

  function mods(g: GameInstall): InstalledMod[] {
    const q = query.trim().toLowerCase();
    return Object.values(g.mods)
      .filter((m) => !q || m.name.toLowerCase().includes(q))
      .sort((a, b) => a.name.localeCompare(b.name, "fr"));
  }

  function outdatedIn(g: GameInstall) {
    return Object.values(g.mods).filter((m) => installs.outdated(g.gameId, m.modId));
  }

  const total = $derived(games.reduce((n, g) => n + Object.keys(g.mods).length, 0));
  const withMods = $derived(games.filter((g) => Object.keys(g.mods).length > 0).length);

  async function update(g: GameInstall, m: InstalledMod) {
    rowErrors.delete(m.modId);
    notes.delete(m.modId);
    try {
      await installs.install(g.gameId, m.modId);
      notes.set(m.modId, "Mis à jour.");
    } catch (e) {
      const err = asAppError(e);
      if (err.kind !== "cancelled") rowErrors.set(m.modId, err.message);
    }
  }

  async function updateAll(g: GameInstall) {
    updatingAll.add(g.gameId);
    for (const m of outdatedIn(g)) await update(g, m);
    updatingAll.delete(g.gameId);
  }

  const timers = new Map<number, ReturnType<typeof setTimeout>>();

  async function uninstall(g: GameInstall, m: InstalledMod) {
    if (!confirming.has(m.modId)) {
      confirming.add(m.modId);
      clearTimeout(timers.get(m.modId));
      timers.set(m.modId, setTimeout(() => confirming.delete(m.modId), 4000));
      return;
    }
    confirming.delete(m.modId);
    removing.add(m.modId);
    rowErrors.delete(m.modId);
    try {
      const r = await installs.uninstall(g.gameId, m.modId);
      if (r.leftFolder) {
        notes.set(-g.gameId, `« ${m.name} » désinstallé. Son dossier contient des fichiers ajoutés depuis, il a été laissé en place.`);
      }
      if (r.orphans.length) orphans.set(g.gameId, { parent: m.name, mods: r.orphans });
    } catch (e) {
      rowErrors.set(m.modId, asAppError(e).message);
    } finally {
      removing.delete(m.modId);
    }
  }

  /** Mods nécessaires devenus inutiles après une désinstallation, par jeu. */
  const orphans = new SvelteMap<number, { parent: string; mods: { modId: number; name: string }[] }>();

  async function removeOrphans(g: GameInstall) {
    const entry = orphans.get(g.gameId);
    orphans.delete(g.gameId);
    if (!entry) return;
    for (const o of entry.mods) {
      removing.add(o.modId);
      try {
        await installs.uninstall(g.gameId, o.modId);
      } catch (e) {
        rowErrors.set(o.modId, asAppError(e).message);
      } finally {
        removing.delete(o.modId);
      }
    }
  }

  function parentNames(g: GameInstall, m: InstalledMod) {
    return m.requiredBy.map((id) => g.mods[id]?.name ?? `mod ${id}`);
  }

  async function openMod(g: GameInstall, m: InstalledMod) {
    try {
      const game = await api.getGame(g.gameId);
      nav.openModFromLibrary(game, m.modId, "installed");
    } catch (e) {
      rowErrors.set(m.modId, asAppError(e).message);
    }
  }

  function reveal(gameId: number, modId: number | null) {
    api.revealInstall(gameId, modId).catch((e) => (error = asAppError(e)));
  }
</script>

<div class="view">
  <header class="head">
    <div>
      <h1>Installés</h1>
      {#if installs.loaded}
        <p class="muted">
          {exact(total)}
          {total > 1 ? "mods installés" : "mod installé"}
          {#if withMods > 0}dans {withMods} {withMods > 1 ? "jeux" : "jeu"}{/if}
        </p>
      {/if}
    </div>
    {#if total > 0}
      <div class="tools">
        <label class="search">
          <span class="visually-hidden">Filtrer les mods installés</span>
          <Icon name="search" />
          <input class="field" type="search" placeholder="Filtrer les mods installés" bind:value={query} />
        </label>
        <button class="btn" onclick={refresh}><Icon name="refresh" size={15} />Vérifier les mises à jour</button>
      </div>
    {/if}
  </header>

  {#if error}
    <ErrorNotice {error} onretry={refresh} />
  {:else if installs.loaded && total === 0 && games.length === 0}
    <div class="empty">
      <p>
        Aucun mod installé avec ModStudio pour l'instant. Ouvre la fiche d'un mod et clique sur « Installer » : tu
        choisiras alors le dossier des mods du jeu.
      </p>
      <button class="btn btn-primary" onclick={() => nav.openGames()}>Parcourir les jeux</button>
    </div>
  {/if}

  {#each games as g (g.gameId)}
    {@const list = mods(g)}
    {@const outdated = outdatedIn(g)}
    <section class="group">
      <div class="group-head">
        {#if g.icon}<img class="game-icon" src={g.icon} alt="" width="28" height="28" />{/if}
        <h2>{g.gameName}</h2>
        <span class="muted count">{Object.keys(g.mods).length}</span>
        <div class="group-actions">
          {#if outdated.length > 0}
            <button class="btn btn-primary" onclick={() => updateAll(g)} disabled={updatingAll.has(g.gameId)}>
              <Icon name="download" size={15} />{updatingAll.has(g.gameId)
                ? "Mise à jour…"
                : outdated.length > 1
                  ? `Tout mettre à jour (${outdated.length})`
                  : "Mettre à jour (1)"}
            </button>
          {/if}
          <button class="btn btn-ghost" onclick={() => reveal(g.gameId, null)}>Ouvrir le dossier</button>
          <button class="btn btn-ghost" onclick={() => (editing = editing === g.gameId ? null : g.gameId)}>
            Réglages du jeu
          </button>
        </div>
      </div>
      <p class="folder muted">
        {g.folder}, {layoutLabels[g.layout].title.toLowerCase()}
      </p>

      {#if editing === g.gameId}
        <div class="editor">
          <FolderSetup
            gameId={g.gameId}
            gameName={g.gameName}
            icon={g.icon}
            onsaved={() => (editing = null)}
            oncancel={() => (editing = null)}
          />
        </div>
      {/if}

      {#if notes.get(-g.gameId)}<p class="note">{notes.get(-g.gameId)}</p>{/if}

      {#if orphans.get(g.gameId)}
        {@const o = orphans.get(g.gameId)!}
        <div class="orphans">
          <p class="small">
            {o.mods.map((x) => `« ${x.name} »`).join(", ")}
            {o.mods.length > 1 ? "avaient été installés" : "avait été installé"} avec « {o.parent} » et ne
            {o.mods.length > 1 ? "servent" : "sert"} plus à aucun autre mod.
          </p>
          <button class="btn btn-primary" onclick={() => removeOrphans(g)}>
            {o.mods.length > 1 ? "Les désinstaller aussi" : "Le désinstaller aussi"}
          </button>
          <button class="btn btn-ghost" onclick={() => orphans.delete(g.gameId)}>Garder</button>
        </div>
      {/if}

      {#if Object.keys(g.mods).length === 0}
        <p class="muted small">Aucun mod installé pour ce jeu.</p>
      {:else if list.length === 0}
        <p class="muted small">Aucun mod installé ne correspond à « {query.trim()} ».</p>
      {/if}

      <ul class="rows">
        {#each list as m (m.modId)}
          {@const progress = installs.progress[m.modId]}
          {@const latest = installs.latest[m.modId]}
          {@const isOutdated = installs.outdated(g.gameId, m.modId)}
          <li class="row">
            <button class="row-main" onclick={() => openMod(g, m)}>
              <span class="row-thumb"><Thumb src={m.logo} label={m.name} /></span>
              <span class="row-text">
                <span class="row-name">{m.name}</span>
                <span class="row-meta">
                  <span>{m.version ? `version ${m.version}` : "version sans numéro"}</span>
                  <span>{bytes(m.size)}</span>
                  <span>installé {ago(m.installedAt)}</span>
                  {#if m.auto && m.requiredBy.length}
                    <span>nécessaire à « {parentNames(g, m).join(" », « ")} »</span>
                  {/if}
                </span>
                {#if rowErrors.get(m.modId)}
                  <span class="row-error">{rowErrors.get(m.modId)}</span>
                {:else if notes.get(m.modId)}
                  <span class="row-note">{notes.get(m.modId)}</span>
                {/if}
              </span>
            </button>
            <div class="row-action">
              {#if progress}
                <div class="row-progress"><ProgressBar {progress} oncancel={() => installs.cancel(m.modId)} /></div>
              {:else}
                {#if isOutdated}
                  <span class="chip chip-accent">{latest?.version ? `Version ${latest.version}` : "Nouvelle version"}</span>
                  <button class="btn btn-primary" onclick={() => update(g, m)}>Mettre à jour</button>
                {:else if latest}
                  <span class="muted small">À jour</span>
                {/if}
                <button class="btn btn-ghost" onclick={() => reveal(g.gameId, m.modId)}>Ouvrir</button>
                <button
                  class="btn"
                  class:btn-ghost={!confirming.has(m.modId)}
                  class:danger={confirming.has(m.modId)}
                  onclick={() => uninstall(g, m)}
                  disabled={removing.has(m.modId)}
                >
                  {removing.has(m.modId) ? "…" : confirming.has(m.modId) ? "Confirmer" : "Désinstaller"}
                </button>
              {/if}
            </div>
          </li>
        {/each}
      </ul>
    </section>
  {/each}
</div>

<style>
  .view {
    height: 100%;
    overflow-y: auto;
    padding: 2.25rem 2.5rem 3rem;
    display: grid;
    align-content: start;
    gap: 1.25rem;
  }

  .head {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-end;
    justify-content: space-between;
    gap: 1.25rem 2rem;
  }

  h1 {
    font-size: var(--t-3xl);
    font-weight: 800;
    margin-bottom: 0.3rem;
  }

  .tools {
    display: flex;
    gap: 0.6rem;
  }

  .search {
    position: relative;
    display: block;
    width: min(18rem, 34vw);
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

  .group {
    display: grid;
    gap: 0.6rem;
    margin-top: 1rem;
  }

  .group-head {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.65rem;
  }

  .game-icon {
    border-radius: var(--r-sm);
  }

  h2 {
    font-size: var(--t-xl);
    font-weight: 700;
  }

  .count {
    font-size: var(--t-md);
  }

  .group-actions {
    margin-left: auto;
    display: flex;
    flex-wrap: wrap;
    gap: 0.3rem;
  }

  .folder {
    font-size: var(--t-sm);
    margin-top: -0.4rem;
    overflow-wrap: anywhere;
  }

  .editor {
    max-width: 34rem;
    padding: 1.1rem;
    border-radius: var(--r-md);
    background: var(--panel);
  }

  .orphans {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.6rem 0.8rem;
    padding: 0.8rem 1rem;
    border-radius: var(--r-md);
    background: var(--raise);
  }

  .orphans p {
    flex: 1 1 22rem;
  }

  .note,
  .row-note {
    color: var(--ok);
    font-size: var(--t-sm);
  }

  .small {
    font-size: var(--t-sm);
  }

  .rows {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 0.4rem;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 1rem;
    padding: 0.5rem 0.75rem 0.5rem 0.5rem;
    border-radius: var(--r-md);
    background: var(--panel);
  }

  .row-main {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 1rem;
    padding: 0;
    border: 0;
    background: none;
    text-align: left;
  }

  .row-main:hover .row-name {
    color: var(--accent);
  }

  .row-thumb {
    flex: none;
    width: 7.5rem;
  }

  .row-thumb :global(.thumb) {
    aspect-ratio: 16 / 9;
    border-radius: var(--r-sm);
    overflow: hidden;
    background: var(--raise);
    position: relative;
  }

  .row-thumb :global(.thumb img) {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .row-thumb :global(.fallback) {
    font-size: var(--t-xl);
  }

  .row-text {
    display: grid;
    gap: 0.2rem;
    min-width: 0;
  }

  .row-name {
    font-family: var(--font-display);
    font-weight: 650;
    font-size: var(--t-lg);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .row-meta {
    display: flex;
    flex-wrap: wrap;
    gap: 0.1rem 0.9rem;
    font-size: var(--t-sm);
    color: var(--muted);
  }

  .row-error {
    color: var(--danger);
    font-size: var(--t-sm);
  }

  .row-action {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    flex: none;
  }

  .row-progress {
    width: 16rem;
  }

  .danger {
    background: var(--danger-bg);
    border-color: var(--danger);
    color: var(--danger);
  }

  .empty {
    display: grid;
    justify-items: start;
    gap: 1rem;
    padding: 1rem 0;
    max-width: 40rem;
  }
</style>
