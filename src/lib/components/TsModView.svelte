<script lang="ts">
  import DOMPurify from "dompurify";
  import { marked } from "marked";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { asAppError, ts, tsPackageUrl, type AppError, type TsCommunity, type TsPackageView } from "$lib/api";
  import { ago, bytes, date, exact } from "$lib/format";
  import { nav } from "$lib/nav.svelte";
  import { isoToUnix } from "$lib/nexus.svelte";
  import { isLoaderName, tsStore } from "$lib/thunderstore.svelte";
  import ErrorNotice from "./ErrorNotice.svelte";
  import Icon from "./Icon.svelte";
  import Thumb from "./Thumb.svelte";
  import TsInstallPanel from "./TsInstallPanel.svelte";

  let { game, namespace, name }: { game: TsCommunity; namespace: string; name: string } = $props();

  let view = $state<TsPackageView | null>(null);
  let error = $state<AppError | null>(null);
  let attempt = $state(0);
  let panel = $state<TsInstallPanel | null>(null);

  $effect(() => {
    attempt;
    const [ns, n] = [namespace, name];
    let cancelled = false;
    view = null;
    error = null;
    ts.package(game.identifier, ns, n)
      .then((v) => !cancelled && (view = v))
      .catch((e) => !cancelled && (error = asAppError(e)));
    return () => {
      cancelled = true;
    };
  });

  const mod = $derived(view?.detail ?? null);
  const title = $derived(mod ? mod.name.replaceAll("_", " ") : name.replaceAll("_", " "));
  const record = $derived(mod ? tsStore.installed(game.id, mod.id) : null);
  const job = $derived(tsStore.progress[game.id] ?? null);

  // Le README vient de l'auteur du mod : converti puis assaini.
  const readme = $derived.by(() => {
    if (!view?.readme) return "";
    // Beaucoup de README commencent par le nom du mod, déjà en titre de la page.
    let source = view.readme.replace(/^\uFEFF?\s*/, "");
    const first = source.match(/^#\s+(.+?)\s*#*\s*(?:\r?\n|$)/);
    if (first && same(first[1], view.detail.name)) source = source.slice(first[0].length);
    const html = marked.parse(source, { async: false, gfm: true, breaks: false }) as string;
    return DOMPurify.sanitize(html, {
      FORBID_TAGS: ["style", "form", "input", "button", "textarea", "select", "iframe", "script", "object", "embed"],
      FORBID_ATTR: ["style", "class", "id", "srcset"],
    });
  });

  /** « More_Suits », « More Suits » et « **MoreSuits** » désignent le même mod. */
  function same(a: string, b: string) {
    const norm = (s: string) => s.toLowerCase().replace(/[^a-z0-9]+/g, "");
    return norm(a) === norm(b);
  }

  function open(url: string | null | undefined) {
    if (url && /^https?:\/\//i.test(url)) openUrl(url);
  }

  /** Les liens du README s'ouvrent dans le navigateur, jamais dans l'appli. */
  function onReadmeClick(event: MouseEvent) {
    const link = (event.target as HTMLElement).closest("a");
    if (!link) return;
    event.preventDefault();
    open(link.getAttribute("href"));
  }

  const VERSIONS_SHOWN = 8;
  let allVersions = $state(false);
</script>

<div class="view">
  <div class="top">
    <button class="btn btn-ghost crumb" onclick={() => nav.back()}>
      <Icon name="back" />{nav.tsModStack.length > 1
        ? "Retour"
        : nav.tsOrigin === "installed"
          ? "Retour aux mods installés"
          : `Retour à ${game.name}`}
    </button>
  </div>

  {#if error}
    <div class="pad"><ErrorNotice {error} onretry={() => attempt++} /></div>
  {:else if !mod || !view}
    <div class="pad skeleton" aria-busy="true" aria-label="Chargement du mod">
      <div class="line title"></div>
      <div class="line short"></div>
      <div class="line"></div>
    </div>
  {:else}
    <header class="head">
      <div class="icon"><Thumb src={mod.iconUrl} label={title} /></div>
      <div class="head-text">
        <h1>{title}</h1>
        <p class="byline">
          <span>par <strong>{mod.team?.name ?? mod.namespace}</strong></span>
          {#if mod.versionCreated}<span class="muted">mis à jour {ago(isoToUnix(mod.versionCreated))}</span>{/if}
          <span class="chip">Thunderstore</span>
          {#if mod.isDeprecated}<span class="chip chip-warn">Obsolète</span>{/if}
          {#if mod.isNsfw}<span class="chip chip-warn">Contenu adulte</span>{/if}
        </p>
        {#if mod.description}<p class="lead">{mod.description}</p>{/if}
      </div>
      <div class="actions">
        {#if mod.websiteUrl && /^https?:\/\//i.test(mod.websiteUrl)}
          <button class="btn" onclick={() => open(mod.websiteUrl)}>Site du mod <Icon name="external" size={14} /></button>
        {/if}
        <button class="btn" onclick={() => open(tsPackageUrl(game.identifier, mod.namespace, mod.name))}>
          Ouvrir sur Thunderstore <Icon name="external" size={14} />
        </button>
      </div>
    </header>

    <div class="layout">
      <main class="main">
        {#if mod.isDeprecated}
          <p class="warn small">
            <Icon name="warning" size={15} />
            <span>Son auteur l'a marqué obsolète : il n'est plus mis à jour, et un autre mod le remplace peut-être.</span>
          </p>
        {/if}

        {#if readme}
          <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
          <div class="prose" onclick={onReadmeClick}>{@html readme}</div>
        {:else}
          <p class="muted">Pas de description détaillée pour ce mod.</p>
        {/if}

        {#if view.versions.length}
          <section class="versions">
            <h2>Versions</h2>
            <ol class="version-list">
              {#each allVersions ? view.versions : view.versions.slice(0, VERSIONS_SHOWN) as v, i (v.versionNumber)}
                <li class="version">
                  <span class="version-name">{v.versionNumber}</span>
                  {#if i === 0}<span class="chip">Dernière</span>{/if}
                  {#if record?.version === v.versionNumber}<span class="chip chip-ok">Installée</span>{/if}
                  <span class="muted small">{date(isoToUnix(v.datetimeCreated))}</span>
                  <span class="muted small">{exact(v.downloadCount)} téléchargements</span>
                  {#if i > 0 && record?.version !== v.versionNumber && tsStore.game(game.id)}
                    <button class="btn btn-ghost small-btn" onclick={() => panel?.install(v.versionNumber)} disabled={!!job}>
                      Installer celle-ci
                    </button>
                  {/if}
                </li>
              {/each}
            </ol>
            {#if view.versions.length > VERSIONS_SHOWN}
              <button class="btn btn-ghost" onclick={() => (allVersions = !allVersions)}>
                {allVersions ? "Moins de versions" : `Les ${view.versions.length} versions`}
              </button>
            {/if}
          </section>
        {/if}
      </main>

      <aside class="side">
        {#key mod.id}
          <TsInstallPanel bind:this={panel} {game} {mod} />
        {/key}
        <dl class="stats">
          <div>
            <dt>Téléchargements</dt>
            <dd>{exact(mod.downloadCount)}</dd>
          </div>
          <div>
            <dt>Notes</dt>
            <dd>{exact(mod.ratingCount)}</dd>
          </div>
          <div>
            <dt>Version</dt>
            <dd>{mod.latestVersionNumber}</dd>
          </div>
          {#if mod.size > 0}
            <div>
              <dt>Taille</dt>
              <dd class="small-dd">{bytes(mod.size)}</dd>
            </div>
          {/if}
          {#if mod.categories.length}
            <div>
              <dt>Catégories</dt>
              <dd class="small-dd">{mod.categories.map((c) => c.name).join(", ")}</dd>
            </div>
          {/if}
          {#if mod.packageCreated}
            <div>
              <dt>Publié le</dt>
              <dd class="small-dd">{date(isoToUnix(mod.packageCreated))}</dd>
            </div>
          {/if}
        </dl>

        {#if mod.dependencies.length}
          <section class="side-block">
            <h2>Nécessite aussi</h2>
            <ul class="deps">
              {#each mod.dependencies as d (d.id)}
                <li>
                  <button class="dep" onclick={() => nav.openTsMod({ namespace: d.namespace, name: d.name })}>
                    {#if d.iconUrl}<img src={d.iconUrl} alt="" width="28" height="28" loading="lazy" />{/if}
                    <span class="dep-text">
                      <span class="dep-name">{d.name.replaceAll("_", " ")}</span>
                      <span class="muted dep-ns">{d.namespace}</span>
                    </span>
                    {#if tsStore.installed(game.id, d.id) || (isLoaderName(d.name) && tsStore.loaders[game.id]?.name)}
                      <span class="dep-ok">Installé</span>
                    {/if}
                  </button>
                </li>
              {/each}
            </ul>
            <p class="muted small">ModStudio les installe en même temps que ce mod (dernière version de chacun).</p>
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

  .prose {
    max-width: 46rem;
    font-size: var(--t-md);
    line-height: 1.65;
    overflow-wrap: anywhere;
  }

  .prose :global(h1),
  .prose :global(h2) {
    font-family: var(--font-display);
    font-size: var(--t-2xl);
    line-height: 1.2;
    margin: 1.5em 0 0.5em;
  }

  .prose :global(h3) {
    font-family: var(--font-display);
    font-size: var(--t-xl);
    margin: 1.5em 0 0.5em;
  }

  .prose :global(h4),
  .prose :global(h5),
  .prose :global(h6) {
    font-family: var(--font-display);
    font-size: var(--t-lg);
    line-height: 1.25;
    margin: 1.3em 0 0.4em;
  }

  .prose :global(:is(h1, h2, h3, h4, h5, h6):first-child) {
    margin-top: 0;
  }

  .prose :global(p),
  .prose :global(ul),
  .prose :global(ol),
  .prose :global(blockquote),
  .prose :global(pre),
  .prose :global(details) {
    margin: 0.6em 0 1em;
  }

  .prose :global(li + li) {
    margin-top: 0.2em;
  }

  .prose :global(a) {
    color: var(--accent);
  }

  .prose :global(img) {
    max-width: 100%;
    height: auto;
    border-radius: var(--r-sm);
    vertical-align: middle;
  }

  .prose :global(blockquote) {
    padding-left: 1rem;
    border-left: 3px solid var(--line);
    color: var(--muted);
  }

  .prose :global(pre) {
    padding: 0.8rem;
    overflow-x: auto;
    font-size: var(--t-sm);
    background: var(--raise);
    border-radius: var(--r-sm);
  }

  .prose :global(code) {
    padding: 0.1em 0.4em;
    font-size: 0.9em;
    background: var(--raise);
    border-radius: var(--r-sm);
  }

  .prose :global(pre code) {
    padding: 0;
    font-size: inherit;
    background: none;
  }

  .prose :global(table) {
    display: block;
    width: max-content;
    max-width: 100%;
    overflow-x: auto;
    margin: 0.6em 0 1em;
    border-collapse: collapse;
    font-size: var(--t-sm);
  }

  .prose :global(th),
  .prose :global(td) {
    padding: 0.4rem 0.9rem;
    border-bottom: 1px solid var(--line);
    text-align: left;
    vertical-align: top;
  }

  .prose :global(th) {
    font-weight: 600;
    color: var(--muted);
  }

  .prose :global(hr) {
    border: 0;
    border-top: 1px solid var(--line);
    margin: 1.4em 0;
  }

  .prose :global(details) {
    padding-left: 0.9rem;
    border-left: 2px solid var(--line);
  }

  .prose :global(summary) {
    cursor: pointer;
    color: var(--accent);
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
    min-height: 2.9rem;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.4rem 0.9rem;
    padding: 0.55rem 0.8rem;
    border-radius: var(--r-md);
    background: var(--panel);
  }

  .version-name {
    font-family: var(--font-display);
    font-weight: 650;
  }

  .small-btn {
    margin-left: auto;
    font-size: var(--t-sm);
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

  .dep img {
    border-radius: var(--r-sm);
    flex: none;
  }

  .dep-text {
    display: grid;
    min-width: 0;
  }

  .dep:hover .dep-name {
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
