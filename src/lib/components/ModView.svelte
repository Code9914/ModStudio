<script lang="ts">
  import DOMPurify from "dompurify";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { api, asAppError, type AppError, type Dependency, type Game, type Mod, type Modfile } from "$lib/api";
  import { ago, bytes, count, date, exact, platformName } from "$lib/format";
  import { nav } from "$lib/nav.svelte";
  import ErrorNotice from "./ErrorNotice.svelte";
  import Icon from "./Icon.svelte";
  import Thumb from "./Thumb.svelte";

  let { game, modId }: { game: Game; modId: number } = $props();

  let mod = $state<Mod | null>(null);
  let error = $state<AppError | null>(null);
  let files = $state<Modfile[] | null>(null);
  let filesError = $state<AppError | null>(null);
  let deps = $state<Dependency[] | null>(null);
  let depsError = $state<AppError | null>(null);
  let active = $state(0);
  let attempt = $state(0);

  $effect(() => {
    const id = modId;
    attempt;
    let cancelled = false;
    mod = null;
    error = null;
    files = null;
    filesError = null;
    deps = null;
    depsError = null;
    active = 0;

    api
      .getMod(game.id, id)
      .then((m) => {
        if (cancelled) return;
        mod = m;
        if (m.dependencies) {
          api
            .listDependencies(game.id, id)
            .then((d) => !cancelled && (deps = d))
            .catch((e) => !cancelled && (depsError = asAppError(e)));
        } else {
          deps = [];
        }
      })
      .catch((e) => !cancelled && (error = asAppError(e)));

    api
      .listModFiles(game.id, id)
      .then((f) => !cancelled && (files = f))
      .catch((e) => !cancelled && (filesError = asAppError(e)));

    return () => {
      cancelled = true;
    };
  });

  // ------------------------------------------------------------ Dérivés

  const gallery = $derived.by(() => {
    if (!mod) return [];
    const list: { full: string; small: string }[] = [];
    const logoFull = mod.logo?.thumb_1280x720 ?? mod.logo?.original;
    if (logoFull) list.push({ full: logoFull, small: mod.logo?.thumb_320x180 ?? logoFull });
    for (const img of mod.media?.images ?? []) {
      const full = img.thumb_1280x720 ?? img.original;
      if (full) list.push({ full, small: img.thumb_320x180 ?? full });
    }
    return list;
  });

  // La description vient de l'auteur du mod : on l'assainit avant affichage.
  const description = $derived.by(() => {
    if (!mod?.description) return "";
    return DOMPurify.sanitize(mod.description, {
      FORBID_TAGS: ["style", "form", "input", "button", "textarea", "select", "iframe", "script", "object", "embed"],
      FORBID_ATTR: ["style", "class", "id", "srcset"],
    });
  });

  const author = $derived(mod?.submitted_by?.username ?? mod?.submitted_by?.display_name_portal ?? null);

  const ratingLabels: Record<string, string> = {
    "Overwhelmingly Positive": "Extrêmement positives",
    "Very Positive": "Très positives",
    "Mostly Positive": "Plutôt positives",
    Positive: "Positives",
    Mixed: "Mitigées",
    Negative: "Négatives",
    "Mostly Negative": "Plutôt négatives",
    "Very Negative": "Très négatives",
    "Overwhelmingly Negative": "Extrêmement négatives",
  };

  const rating = $derived.by(() => {
    const s = mod?.stats;
    if (!s || s.ratings_total === 0) return null;
    const text = s.ratings_display_text ?? "";
    return { label: ratingLabels[text] ?? text, percent: s.ratings_percentage_positive, total: s.ratings_total };
  });

  const latest = $derived(mod?.modfile && mod.modfile.id ? mod.modfile : null);

  // ------------------------------------------------------------ Actions

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

  function scanLabel(f: Modfile): { text: string; warn: boolean } | null {
    if (f.virus_positive === 1) return { text: "Signalé par l'antivirus", warn: true };
    if (f.virus_status === 0) return { text: "Analyse antivirus en attente", warn: false };
    if (f.virus_status >= 2) return { text: "Non analysé par l'antivirus", warn: false };
    return null;
  }
</script>

<div class="view">
  <div class="top">
    <button class="btn btn-ghost crumb" onclick={() => nav.back()}>
      <Icon name="back" />{nav.modStack.length > 1 ? "Retour" : `Retour à ${game.name}`}
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
          {#if author}<span>par <strong>{author}</strong></span>{/if}
          <span class="muted">mis à jour {ago(mod.date_updated)}</span>
          {#if mod.maturity_option > 0}<span class="chip chip-warn">Contenu adulte</span>{/if}
        </p>
      </div>
      <div class="actions">
        {#if mod.homepage_url}
          <button class="btn" onclick={() => open(mod?.homepage_url)}>
            Site de l'auteur <Icon name="external" size={14} />
          </button>
        {/if}
        {#if mod.profile_url}
          <button class="btn btn-primary" onclick={() => open(mod?.profile_url)}>
            Ouvrir sur mod.io <Icon name="external" size={14} />
          </button>
        {/if}
      </div>
    </header>

    <div class="layout">
      <main class="main">
        <figure class="gallery">
          <div class="hero">
            <Thumb src={gallery[active]?.full} label={mod.name ?? ""} />
          </div>
          {#if gallery.length > 1}
            <div class="strip" role="group" aria-label="Images du mod">
              {#each gallery as img, i (img.full)}
                <button
                  class="strip-item"
                  aria-label="Image {i + 1} sur {gallery.length}"
                  aria-pressed={i === active}
                  onclick={() => (active = i)}
                >
                  <img src={img.small} alt="" loading="lazy" />
                </button>
              {/each}
            </div>
          {/if}
        </figure>

        {#if mod.media?.youtube?.length}
          <div class="videos">
            {#each mod.media.youtube as url, i (url)}
              <button class="btn" onclick={() => open(url)}>
                Vidéo {mod.media.youtube.length > 1 ? i + 1 : ""} sur YouTube <Icon name="external" size={14} />
              </button>
            {/each}
          </div>
        {/if}

        {#if mod.summary}
          <p class="lead">{mod.summary}</p>
        {/if}

        {#if description}
          <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
          <div class="prose" onclick={onDescriptionClick}>{@html description}</div>
        {:else if mod.description_plaintext}
          <p class="prose plain">{mod.description_plaintext}</p>
        {/if}

        <section class="files">
          <h2>Versions</h2>
          {#if filesError}
            <ErrorNotice error={filesError} onretry={() => attempt++} />
          {:else if !files}
            <p class="muted">Chargement des versions…</p>
          {:else if files.length === 0}
            <p class="muted">Aucun fichier publié pour ce mod.</p>
          {:else}
            <ol class="file-list">
              {#each files as f (f.id)}
                {@const scan = scanLabel(f)}
                <li class="file">
                  <div class="file-head">
                    <span class="file-version">{f.version || "Sans numéro"}</span>
                    {#if f.id === latest?.id}<span class="chip">Version actuelle</span>{/if}
                    {#if scan}<span class="chip" class:chip-warn={scan.warn}>{scan.text}</span>{/if}
                  </div>
                  <p class="file-meta muted">
                    <span>{date(f.date_added)}</span>
                    <span>{bytes(f.filesize)}</span>
                    {#if f.filename}<span class="filename">{f.filename}</span>{/if}
                  </p>
                  {#if f.changelog?.trim()}
                    <details>
                      <summary>Notes de version</summary>
                      <p class="changelog">{f.changelog}</p>
                    </details>
                  {/if}
                </li>
              {/each}
            </ol>
          {/if}
        </section>
      </main>

      <aside class="side">
        <dl class="stats">
          <div>
            <dt>Téléchargements</dt>
            <dd>{exact(mod.stats?.downloads_total)}</dd>
            <dd class="muted small">{exact(mod.stats?.downloads_today)} aujourd'hui</dd>
          </div>
          <div>
            <dt>Abonnés</dt>
            <dd>{exact(mod.stats?.subscribers_total)}</dd>
          </div>
          {#if mod.stats?.popularity_rank_position}
            <div>
              <dt>Popularité</dt>
              <dd>{exact(mod.stats.popularity_rank_position)}<sup>{mod.stats.popularity_rank_position === 1 ? "er" : "e"}</sup></dd>
              <dd class="muted small">sur {exact(mod.stats.popularity_rank_total_mods)} {game.ugc_name?.toLowerCase() || "mods"}</dd>
            </div>
          {/if}
          <div>
            <dt>Évaluations</dt>
            {#if rating}
              <dd>{rating.percent} % positives</dd>
              <dd class="meter" aria-hidden="true"><span style="width: {rating.percent}%"></span></dd>
              <dd class="muted small">{rating.label}, {count(rating.total)} avis</dd>
            {:else}
              <dd class="muted">Pas encore évalué</dd>
            {/if}
          </div>
          {#if latest}
            <div>
              <dt>Dernière version</dt>
              <dd>{latest.version || "Sans numéro"}</dd>
              <dd class="muted small">{bytes(latest.filesize)}, le {date(latest.date_added)}</dd>
            </div>
          {/if}
          <div>
            <dt>Publié le</dt>
            <dd>{date(mod.date_live ?? mod.date_added)}</dd>
          </div>
        </dl>

        {#if mod.platforms.length}
          <section class="side-block">
            <h2>Plateformes</h2>
            <div class="chips">
              {#each mod.platforms as p (p.platform)}
                <span class="chip">{platformName(p.platform)}</span>
              {/each}
            </div>
          </section>
        {/if}

        {#if mod.tags.length}
          <section class="side-block">
            <h2>Tags</h2>
            <div class="chips">
              {#each mod.tags as t (t.name)}
                <span class="chip">{t.name_localized || t.name}</span>
              {/each}
            </div>
          </section>
        {/if}

        {#if mod.dependencies}
          <section class="side-block">
            <h2>Nécessite aussi</h2>
            {#if depsError}
              <ErrorNotice error={depsError} onretry={() => attempt++} />
            {:else if !deps}
              <p class="muted">Chargement…</p>
            {:else if deps.length === 0}
              <p class="muted">Aucune dépendance listée.</p>
            {:else}
              <ul class="deps">
                {#each deps as d (d.mod_id)}
                  <li>
                    <button class="dep" onclick={() => nav.openMod(d.mod_id)}>
                      <span class="dep-thumb"><Thumb src={d.logo?.thumb_320x180} label={d.name ?? ""} /></span>
                      <span class="dep-name">{d.name ?? `Mod ${d.mod_id}`}</span>
                    </button>
                  </li>
                {/each}
              </ul>
            {/if}
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
    grid-template-columns: minmax(0, 1fr) 18rem;
    gap: 3rem;
    padding: 0 2.5rem;
  }

  .main {
    display: grid;
    align-content: start;
    gap: 1.75rem;
    min-width: 0;
  }

  .gallery {
    margin: 0;
    display: grid;
    gap: 0.75rem;
  }

  .hero :global(.thumb) {
    aspect-ratio: 16 / 9;
    border-radius: var(--r-md);
    overflow: hidden;
    background: var(--raise);
  }

  .hero :global(.thumb img) {
    width: 100%;
    height: 100%;
    object-fit: contain;
    background: #000;
  }

  .strip {
    display: flex;
    gap: 0.5rem;
    overflow-x: auto;
    padding: 0.25rem 0.1rem;
  }

  .strip-item {
    flex: none;
    width: 7.5rem;
    aspect-ratio: 16 / 9;
    padding: 0;
    border: 0;
    border-radius: var(--r-sm);
    overflow: hidden;
    background: var(--raise);
    opacity: 0.55;
    outline: 2px solid transparent;
    outline-offset: 2px;
  }

  .strip-item img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .strip-item:hover,
  .strip-item[aria-pressed="true"] {
    opacity: 1;
  }

  .strip-item[aria-pressed="true"] {
    outline-color: var(--accent);
  }

  .videos {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
  }

  .lead {
    font-size: var(--t-xl);
    line-height: 1.45;
    max-width: 42rem;
  }

  /* Description HTML de l'auteur, rendue sobrement. */
  .prose {
    max-width: 42rem;
    font-size: var(--t-md);
    line-height: 1.65;
    overflow-wrap: anywhere;
  }

  .prose.plain {
    white-space: pre-wrap;
  }

  .prose :global(h1),
  .prose :global(h2),
  .prose :global(h3),
  .prose :global(h4) {
    font-family: var(--font-display);
    font-size: var(--t-xl);
    margin: 1.6em 0 0.5em;
  }

  .prose :global(p),
  .prose :global(ul),
  .prose :global(ol),
  .prose :global(blockquote),
  .prose :global(pre) {
    margin: 0 0 1em;
  }

  .prose :global(img) {
    height: auto;
    border-radius: var(--r-sm);
    margin: 1em 0;
  }

  .prose :global(blockquote) {
    padding-left: 1rem;
    border-left: 3px solid var(--line);
    color: var(--muted);
  }

  .prose :global(pre),
  .prose :global(code) {
    font-size: var(--t-sm);
    background: var(--raise);
    border-radius: var(--r-sm);
  }

  .prose :global(pre) {
    padding: 0.8rem;
    overflow-x: auto;
  }

  .prose :global(table) {
    border-collapse: collapse;
    margin-bottom: 1em;
  }

  .prose :global(td),
  .prose :global(th) {
    padding: 0.3rem 0.6rem;
    border: 1px solid var(--line);
  }

  h2 {
    font-size: var(--t-xl);
    font-weight: 700;
    margin-bottom: 0.9rem;
  }

  .files {
    margin-top: 1rem;
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
    gap: 0.3rem;
  }

  .file-head {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.5rem;
  }

  .file-version {
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

  .filename {
    overflow-wrap: anywhere;
  }

  details summary {
    cursor: pointer;
    font-size: var(--t-sm);
    color: var(--accent);
    width: fit-content;
  }

  .changelog {
    white-space: pre-wrap;
    font-size: var(--t-sm);
    margin-top: 0.5rem;
    max-height: 18rem;
    overflow-y: auto;
  }

  .side {
    display: grid;
    align-content: start;
    gap: 2rem;
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

  dd.small,
  dd.muted {
    font-family: var(--font-body);
    font-size: var(--t-sm);
    font-weight: 400;
  }

  .meter {
    height: 6px;
    margin: 0.35rem 0 0.2rem;
    border-radius: 999px;
    background: var(--raise);
    overflow: hidden;
  }

  .meter span {
    display: block;
    height: 100%;
    background: var(--accent-fill);
  }

  .side-block h2 {
    font-size: var(--t-md);
    margin-bottom: 0.6rem;
  }

  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 0.4rem;
  }

  .deps {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 0.6rem;
  }

  .dep {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    width: 100%;
    padding: 0.35rem;
    border: 0;
    border-radius: var(--r-md);
    background: transparent;
    text-align: left;
  }

  .dep:hover {
    background: var(--panel);
  }

  .dep-thumb {
    flex: none;
    width: 4.5rem;
  }

  .dep-thumb :global(.thumb) {
    aspect-ratio: 16 / 9;
    border-radius: var(--r-sm);
    overflow: hidden;
    background: var(--raise);
  }

  .dep-thumb :global(.thumb img) {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .dep-thumb :global(.fallback) {
    font-size: var(--t-lg);
  }

  .dep-name {
    font-weight: 600;
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
