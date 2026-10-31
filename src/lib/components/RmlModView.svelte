<script lang="ts">
  // Fiche d'un mod de ResoniteModLoader (liste vérifiée de la communauté).
  import { onMount } from "svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import type { RmlDep, TsCommunity } from "$lib/api";
  import { english, tr } from "$lib/i18n.svelte";
  import { nav } from "$lib/nav.svelte";
  import { rmlCategory, rmlPlatforms, tsStore } from "$lib/thunderstore.svelte";
  import ErrorNotice from "./ErrorNotice.svelte";
  import Icon from "./Icon.svelte";
  import Translatable from "./Translatable.svelte";
  import RmlInstallPanel from "./RmlInstallPanel.svelte";
  import Thumb from "./Thumb.svelte";

  let { game, id }: { game: TsCommunity; id: string } = $props();

  let panel = $state<RmlInstallPanel | null>(null);

  onMount(() => {
    tsStore.loadRml();
    if (!tsStore.rmlStatus) tsStore.refreshRmlStatus();
  });

  const mod = $derived(tsStore.rmlMod(id));
  const record = $derived(mod ? tsStore.installed(game.id, mod.modId) : null);
  const job = $derived(tsStore.progress[game.id] ?? null);
  const current = $derived(mod?.versions[0] ?? null);
  const deps = $derived((current?.dependencies ?? []).filter((d) => !d.optional));
  const optional = $derived((current?.dependencies ?? []).filter((d) => d.optional));

  function open(url: string | null | undefined) {
    if (url && /^https:\/\//i.test(url)) openUrl(url);
  }

  function installedDep(d: RmlDep) {
    const dep = tsStore.rmlMod(d.id);
    return !!dep && !!tsStore.installed(game.id, dep.modId);
  }

  const VERSIONS_SHOWN = 6;
  let allVersions = $state(false);
</script>

<div class="view">
  <div class="top">
    <button class="btn btn-ghost crumb" onclick={() => nav.back()}>
      <Icon name="back" />{nav.rmlStack.length > 1
        ? tr("Retour", "Back")
        : nav.rmlOrigin === "installed"
          ? tr("Retour aux mods installés", "Back to installed mods")
          : tr(`Retour à ${game.name}`, `Back to ${game.name}`)}
    </button>
  </div>

  {#if tsStore.rmlError && !tsStore.rmlMods}
    <div class="pad"><ErrorNotice error={tsStore.rmlError} onretry={() => tsStore.loadRml(true)} /></div>
  {:else if !tsStore.rmlMods}
    <div class="pad skeleton" aria-busy="true" aria-label={tr("Chargement du mod", "Loading mod")}>
      <div class="line title"></div>
      <div class="line short"></div>
      <div class="line"></div>
    </div>
  {:else if !mod}
    <div class="pad">
      <p>{tr("Ce mod n'est plus dans la liste de ResoniteModLoader.", "This mod is no longer in the ResoniteModLoader list.")}</p>
    </div>
  {:else}
    <header class="head">
      <div class="icon"><Thumb src={mod.authorIcon} fallback={mod.authorAvatar} label={mod.name} /></div>
      <div class="head-text">
        <h1>{mod.name}</h1>
        <p class="byline">
          <span>{tr("par", "by")} <strong>{mod.author}</strong></span>
          <span class="chip">ResoniteModLoader</span>
          <span class="chip">{rmlCategory(mod.category)}</span>
          {#if mod.broken}<span class="chip chip-warn">{tr("Cassé", "Broken")}</span>{:else if mod.deprecated}<span class="chip chip-warn">{tr("Obsolète", "Outdated")}</span>{/if}
          {#if mod.platforms.length}<span class="chip">{rmlPlatforms(mod.platforms)}</span>{/if}
        </p>
        {#if mod.description}
          <Translatable sources={[{ text: mod.description }]}>
            {#snippet children([text])}<p class="lead">{text}</p>{/snippet}
          </Translatable>
        {/if}
      </div>
      <div class="actions">
        {#if mod.website}
          <button class="btn" onclick={() => open(mod.website)}>{tr("Site du mod", "Mod website")} <Icon name="external" size={14} /></button>
        {/if}
        {#if mod.source}
          <button class="btn" onclick={() => open(mod.source)}>{tr("Page du mod", "Mod page")} <Icon name="external" size={14} /></button>
        {/if}
      </div>
    </header>

    <div class="layout">
      <main class="main">
        {#if mod.broken}
          <p class="warn small">
            <Icon name="warning" size={15} />
            <span>{tr("La liste le signale comme cassé : il ne marche plus avec la version actuelle de Resonite.", "The list marks it as broken: it no longer works with the current version of Resonite.")}</span>
          </p>
        {:else if mod.deprecated}
          <p class="warn small">
            <Icon name="warning" size={15} />
            <span>{tr("Il n'est plus maintenu : souvent, ce qu'il apportait fait maintenant partie du jeu, ou un autre mod le remplace.", "It's no longer maintained: often, what it added is now part of the game, or another mod replaces it.")}</span>
          </p>
        {/if}

        <p class="muted small">
          {#if english}
            The full description and help are on the mod page{mod.source ? "" : " (link missing from the list)"}. Its
            settings are saved in the game's <code>rml_config</code> folder after the first launch.
          {:else}
            La description complète et l'aide se trouvent sur la page du mod{mod.source ? "" : " (lien absent de la liste)"}. Ses
            réglages sont enregistrés dans le dossier <code>rml_config</code> du jeu après le premier lancement.
          {/if}
        </p>

        <section class="versions">
          <h2>Versions</h2>
          <ol class="version-list">
            {#each allVersions ? mod.versions : mod.versions.slice(0, VERSIONS_SHOWN) as v, i (v.version)}
              <li class="version">
                <div class="version-head">
                  <span class="version-name">{v.version}</span>
                  {#if i === 0}<span class="chip">{tr("Dernière", "Latest")}</span>{/if}
                  {#if record?.version === v.version}<span class="chip chip-ok">{tr("Installée", "Installed")}</span>{/if}
                  {#if v.releaseUrl}
                    <button class="btn btn-ghost small-btn" onclick={() => open(v.releaseUrl)}>
                      {tr("Notes de version", "Release notes")} <Icon name="external" size={12} />
                    </button>
                  {/if}
                  {#if i > 0 && record?.version !== v.version && tsStore.game(game.id) && !mod.broken && mod.compatible}
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
          <RmlInstallPanel bind:this={panel} {game} {mod} />
        {/key}
        <dl class="stats">
          <div>
            <dt>Version</dt>
            <dd>{mod.latest}</dd>
          </div>
          <div>
            <dt>{tr("Catégorie", "Category")}</dt>
            <dd class="small-dd">{rmlCategory(mod.category)}</dd>
          </div>
          {#if current}
            <div>
              <dt>{tr("Fichiers installés", "Installed files")}</dt>
              <dd class="files">
                {#each current.files as f (f)}<code>{f}</code>{/each}
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
                  <button class="dep" onclick={() => tsStore.rmlMod(d.id) && nav.openRmlMod(d.id)} disabled={!tsStore.rmlMod(d.id)}>
                    <span class="dep-text">
                      <span class="dep-name">{d.name}</span>
                      <span class="muted dep-ns">{d.version && d.version !== "*" ? `version ${d.version}` : tr("toute version", "any version")}</span>
                    </span>
                    {#if installedDep(d)}<span class="dep-ok">{tr("Installé", "Installed")}</span>{/if}
                  </button>
                </li>
              {/each}
            </ul>
            <p class="muted small">{tr("ModStudio les installe en même temps que ce mod (dernière version de chacun).", "ModStudio installs them along with this mod (latest version of each).")}</p>
          </section>
        {/if}

        {#if optional.length}
          <section class="side-block">
            <h2>{tr("Peut aussi utiliser", "Can also use")}</h2>
            <ul class="deps">
              {#each optional as d (d.id)}
                <li>
                  <button class="dep" onclick={() => tsStore.rmlMod(d.id) && nav.openRmlMod(d.id)} disabled={!tsStore.rmlMod(d.id)}>
                    <span class="dep-text"><span class="dep-name">{d.name}</span></span>
                    {#if installedDep(d)}<span class="dep-ok">{tr("Installé", "Installed")}</span>{/if}
                  </button>
                </li>
              {/each}
            </ul>
          </section>
        {/if}

        {#if current?.conflicts.length}
          <section class="side-block">
            <h2>{tr("Ne marche pas avec", "Doesn't work with")}</h2>
            <ul class="deps">
              {#each current.conflicts as d (d.id)}
                <li>
                  <button class="dep" onclick={() => tsStore.rmlMod(d.id) && nav.openRmlMod(d.id)} disabled={!tsStore.rmlMod(d.id)}>
                    <span class="dep-text"><span class="dep-name">{d.name}</span></span>
                    {#if installedDep(d)}<span class="dep-warn">{tr("Installé", "Installed")}</span>{/if}
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

  .lead {
    font-size: var(--t-lg);
    line-height: 1.45;
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

  .dep-ns {
    font-size: var(--t-xs);
  }

  .dep-ok,
  .dep-warn {
    margin-left: auto;
    font-size: var(--t-xs);
    font-weight: 600;
    color: var(--ok);
  }

  .dep-warn {
    color: var(--warn, var(--accent));
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
