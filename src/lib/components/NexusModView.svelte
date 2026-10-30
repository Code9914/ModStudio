<script lang="ts">
  import DOMPurify from "dompurify";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { asAppError, nexus, nexusModUrl, type AppError, type NexusFile, type NexusGame, type NexusModDetail } from "$lib/api";
  import { bbcodeToHtml, plainText } from "$lib/bbcode";
  import { ago, bytes, date, exact } from "$lib/format";
  import { nav } from "$lib/nav.svelte";
  import { fileCategories, isoToUnix, nexusStore, questFile } from "$lib/nexus.svelte";
  import ErrorNotice from "./ErrorNotice.svelte";
  import Icon from "./Icon.svelte";
  import NexusInstallPanel from "./NexusInstallPanel.svelte";
  import Thumb from "./Thumb.svelte";

  let { game, modId }: { game: NexusGame; modId: number } = $props();

  let mod = $state<NexusModDetail | null>(null);
  let error = $state<AppError | null>(null);
  let files = $state<NexusFile[] | null>(null);
  let filesError = $state<AppError | null>(null);
  let attempt = $state(0);
  let panel = $state<ReturnType<typeof NexusInstallPanel> | null>(null);

  $effect(() => {
    const id = modId;
    attempt;
    let cancelled = false;
    mod = null;
    error = null;
    files = null;
    filesError = null;
    nexus
      .getMod(game.id, id)
      .then((m) => !cancelled && (mod = m))
      .catch((e) => !cancelled && (error = asAppError(e)));
    nexus
      .listFiles(game.id, id)
      .then((f) => !cancelled && (files = f))
      .catch((e) => !cancelled && (filesError = asAppError(e)));
    return () => {
      cancelled = true;
    };
  });

  // La description vient de l'auteur du mod : convertie puis assainie.
  const description = $derived.by(() => {
    if (!mod?.description) return "";
    return DOMPurify.sanitize(bbcodeToHtml(mod.description), {
      FORBID_TAGS: ["style", "form", "input", "button", "textarea", "select", "iframe", "script", "object", "embed"],
      FORBID_ATTR: ["style", "class", "id", "srcset"],
    });
  });

  const record = $derived(mod ? nexusStore.installed(game.id, mod.modId) : null);
  const progressing = $derived(
    mod ? !!nexusStore.progress[nexusStore.key(game.id, mod.modId)] || !!nexusStore.waiting[nexusStore.key(game.id, mod.modId)] : false,
  );

  /** Jeu VR dont ModStudio n'installe que la version PC (Bonelab). */
  const pcOnly = $derived(game.id === 4832);

  /** Fichiers par catégorie, dans l'ordre du site ; les retirés sont cachés. */
  const groups = $derived.by(() => {
    if (!files) return [];
    const order = ["MAIN", "UPDATE", "OPTIONAL", "MISCELLANEOUS", "OLD_VERSION", "ARCHIVED"];
    return order
      .map((cat) => ({ cat, label: fileCategories[cat] ?? cat, list: files!.filter((f) => f.category === cat) }))
      .filter((g) => g.list.length > 0);
  });

  function open(url: string | null | undefined) {
    if (url && /^https?:\/\//i.test(url)) openUrl(url);
  }

  /** Les liens de la description s'ouvrent dans le navigateur, jamais dans l'appli. */
  function onDescriptionClick(event: MouseEvent) {
    const link = (event.target as HTMLElement).closest("a");
    if (!link) return;
    event.preventDefault();
    open(link.getAttribute("href"));
  }

  function fileDescription(f: NexusFile) {
    return (f.description ?? "")
      .replace(/<br\s*\/?>/gi, "\n")
      .replace(/\[\/?[a-z*]+(?:=[^\]]*)?\]/gi, "")
      .replace(/\n{3,}/g, "\n\n")
      .trim();
  }

  function openRequirement(r: NexusModDetail["requirements"][number]) {
    if (!r.externalRequirement && r.gameId === game.id && r.modId > 0) nav.openNexusMod(r.modId);
    else open(r.url);
  }
</script>

<div class="view">
  <div class="top">
    <button class="btn btn-ghost crumb" onclick={() => nav.back()}>
      <Icon name="back" />{nav.nexusModStack.length > 1
        ? "Retour"
        : nav.nexusOrigin === "installed"
          ? "Retour aux mods installés"
          : `Retour à ${game.name}`}
    </button>
  </div>

  {#if error}
    <div class="pad"><ErrorNotice {error} onretry={() => attempt++} /></div>
  {:else if !mod}
    <div class="pad skeleton" aria-busy="true" aria-label="Chargement du mod">
      <div class="line title"></div>
      <div class="line short"></div>
      <div class="hero"></div>
    </div>
  {:else}
    <header class="head">
      <div class="head-text">
        <h1>{mod.name}</h1>
        <p class="byline">
          {#if mod.author}<span>par <strong>{mod.author}</strong></span>{/if}
          <span class="muted">mis à jour {ago(isoToUnix(mod.updatedAt))}</span>
          <span class="chip">Nexus Mods</span>
          {#if mod.adultContent}<span class="chip chip-warn">Contenu adulte</span>{/if}
        </p>
      </div>
      <div class="actions">
        <button class="btn" onclick={() => open(nexusModUrl(game.domainName, mod!.modId))}>
          Ouvrir sur Nexus Mods <Icon name="external" size={14} />
        </button>
      </div>
    </header>

    <div class="layout">
      <main class="main">
        <div class="hero">
          <Thumb src={mod.pictureUrl ?? mod.thumbnailUrl} label={mod.name} />
        </div>

        {#if mod.summary}
          <p class="lead">{plainText(mod.summary)}</p>
        {/if}

        {#if description}
          <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
          <div class="prose" onclick={onDescriptionClick}>{@html description}</div>
        {/if}

        <section class="files">
          <h2>Fichiers</h2>
          {#if filesError}
            <ErrorNotice error={filesError} onretry={() => attempt++} />
          {:else if !files}
            <p class="muted">Chargement des fichiers…</p>
          {:else if groups.length === 0}
            <p class="muted">Aucun fichier publié pour ce mod.</p>
          {:else}
            {#each groups as g (g.cat)}
              {#snippet fileList()}
                <ol class="file-list">
                  {#each g.list as f (f.fileId)}
                    {@const desc = fileDescription(f)}
                    <li class="file">
                      <div class="file-head">
                        <span class="file-name">{f.name}</span>
                        {#if f.version}<span class="chip">{f.version}</span>{/if}
                        {#if record?.modfileId === f.fileId}<span class="chip chip-ok">Installé</span>{/if}
                        {#if questFile(f.name)}<span class="chip">Quest</span>{/if}
                      </div>
                      <p class="file-meta muted">
                        <span>{date(f.date)}</span>
                        <span>{bytes(f.sizeInBytes || f.size * 1024)}</span>
                      </p>
                      {#if desc}<p class="file-desc">{desc}</p>{/if}
                      {#if record?.modfileId !== f.fileId}
                        {#if pcOnly && questFile(f.name)}
                          <p class="muted small">Version Quest : elle ne s'installe pas sur PC.</p>
                        {:else}
                          <div>
                            <button class="btn" onclick={() => panel?.install(f)} disabled={progressing}>
                              <Icon name="download" size={14} />{record ? "Installer ce fichier à la place" : "Installer ce fichier"}
                            </button>
                          </div>
                        {/if}
                      {/if}
                    </li>
                  {/each}
                </ol>
              {/snippet}
              {#if g.cat === "OLD_VERSION" || g.cat === "ARCHIVED"}
                <details class="group">
                  <summary>{g.label} ({g.list.length})</summary>
                  {@render fileList()}
                </details>
              {:else}
                <div class="group">
                  <h3>{g.label}</h3>
                  {@render fileList()}
                </div>
              {/if}
            {/each}
          {/if}
        </section>
      </main>

      <aside class="side">
        {#key mod.modId}
          <NexusInstallPanel bind:this={panel} {game} {mod} {files} />
        {/key}
        <dl class="stats">
          <div>
            <dt>Recommandations</dt>
            <dd>{exact(mod.endorsements)}</dd>
          </div>
          <div>
            <dt>Téléchargements</dt>
            <dd>{exact(mod.downloads)}</dd>
          </div>
          {#if mod.version}
            <div>
              <dt>Version</dt>
              <dd>{mod.version}</dd>
            </div>
          {/if}
          {#if mod.category}
            <div>
              <dt>Catégorie</dt>
              <dd class="small-dd">{mod.category}</dd>
            </div>
          {/if}
          <div>
            <dt>Publié le</dt>
            <dd class="small-dd">{date(isoToUnix(mod.createdAt))}</dd>
          </div>
        </dl>

        {#if mod.requirements.length}
          <section class="side-block">
            <h2>Nécessite aussi</h2>
            <ul class="deps">
              {#each mod.requirements as r (`${r.gameId}-${r.modId}-${r.modName}`)}
                <li>
                  <button class="dep" onclick={() => openRequirement(r)}>
                    <span class="dep-name">{r.modName || `Mod ${r.modId}`}</span>
                    {#if r.externalRequirement || r.gameId !== game.id}<Icon name="external" size={13} />{/if}
                    {#if nexusStore.installed(game.id, r.modId)}<span class="dep-ok">Installé</span>{/if}
                  </button>
                  {#if r.notes}<p class="muted small">{r.notes}</p>{/if}
                </li>
              {/each}
            </ul>
            <p class="muted small">
              ModStudio installe ceux qui sont sur Nexus Mods en même temps que ce mod ; les autres (liens externes)
              s'installent depuis leur page, ou depuis la page du jeu pour UE4SS, BepInEx et MelonLoader.
            </p>
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
    justify-content: space-between;
    gap: 1.25rem 2rem;
    padding: 1rem 2.5rem 1.75rem;
  }

  .head-text {
    display: grid;
    gap: 0.6rem;
    max-width: 46rem;
  }

  h1 {
    font-size: clamp(var(--t-3xl), 4.2vw, var(--t-4xl));
    font-weight: 800;
    letter-spacing: -0.025em;
    text-wrap: balance;
  }

  .byline {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.4rem 1rem;
  }

  .actions {
    display: flex;
    gap: 0.6rem;
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

  .hero :global(.thumb) {
    aspect-ratio: 16 / 9;
    border-radius: var(--r-md);
    overflow: hidden;
    background: var(--raise);
    max-width: 52rem;
  }

  .hero :global(.thumb img) {
    width: 100%;
    height: 100%;
    object-fit: contain;
    background: #000;
  }

  .lead {
    font-size: var(--t-xl);
    line-height: 1.45;
    max-width: 42rem;
  }

  .prose {
    max-width: 42rem;
    font-size: var(--t-md);
    line-height: 1.65;
    overflow-wrap: anywhere;
  }

  .prose :global(h3) {
    font-family: var(--font-display);
    font-size: var(--t-xl);
    margin: 1.6em 0 0.5em;
  }

  .prose :global(h4) {
    font-family: var(--font-display);
    font-size: var(--t-lg);
    line-height: 1.25;
    margin: 1.3em 0 0.4em;
  }

  .prose :global(h3:first-child),
  .prose :global(h4:first-child) {
    margin-top: 0;
  }

  .prose :global(ul),
  .prose :global(ol),
  .prose :global(blockquote),
  .prose :global(pre),
  .prose :global(details) {
    margin: 0.6em 0 1em;
  }

  .prose :global(img) {
    max-width: 100%;
    height: auto;
    border-radius: var(--r-sm);
    margin: 0.6em 0;
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

  /* Code dans le texte (directement dans .prose aussi), pas dans un bloc. */
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

  /* Tableaux : défilent seuls s'ils sont trop larges. */
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

  /* Spoilers : un trait sur le côté, pour voir où chacun finit (ils s'imbriquent souvent). */
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

  h3,
  .group summary {
    font-family: var(--font-display);
    font-size: var(--t-lg);
    font-weight: 650;
    margin: 0.4rem 0 0.7rem;
  }

  .group summary {
    cursor: pointer;
    color: var(--muted);
  }

  .group {
    margin-bottom: 1.25rem;
  }

  .file-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 0.75rem;
  }

  .file {
    padding: 0.9rem 1rem;
    border-radius: var(--r-md);
    background: var(--panel);
    display: grid;
    gap: 0.4rem;
  }

  .file-head {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.5rem;
  }

  .file-name {
    font-family: var(--font-display);
    font-weight: 650;
    font-size: var(--t-lg);
  }

  .file-meta {
    display: flex;
    flex-wrap: wrap;
    gap: 0.2rem 1rem;
    font-size: var(--t-sm);
  }

  .file-desc {
    white-space: pre-wrap;
    font-size: var(--t-sm);
    max-height: 9rem;
    overflow-y: auto;
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
    gap: 0.5rem;
    width: 100%;
    padding: 0.5rem 0.6rem;
    border: 0;
    border-radius: var(--r-md);
    background: var(--panel);
    text-align: left;
    color: var(--text);
  }

  .dep:hover .dep-name {
    color: var(--accent);
  }

  .dep-name {
    font-weight: 600;
    font-size: var(--t-sm);
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

  .skeleton .hero {
    aspect-ratio: 16 / 9;
    max-width: 52rem;
    border-radius: var(--r-md);
    background: var(--raise);
    margin-top: 1rem;
  }

  @media (max-width: 1050px) {
    .layout {
      grid-template-columns: 1fr;
    }
  }
</style>
