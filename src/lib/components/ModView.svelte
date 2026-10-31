<script lang="ts">
  import DOMPurify from "dompurify";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { api, asAppError, pcFileId, type AppError, type Dependency, type Game, type Mod, type Modfile } from "$lib/api";
  import { ago, bytes, count, date, exact, platformName } from "$lib/format";
  import { tr } from "$lib/i18n.svelte";
  import { nav } from "$lib/nav.svelte";
  import ErrorNotice from "./ErrorNotice.svelte";
  import Icon from "./Icon.svelte";
  import InstallPanel from "./InstallPanel.svelte";
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
    "Overwhelmingly Positive": tr("Extrêmement positives", "Overwhelmingly positive"),
    "Very Positive": tr("Très positives", "Very positive"),
    "Mostly Positive": tr("Plutôt positives", "Mostly positive"),
    Positive: tr("Positives", "Positive"),
    Mixed: tr("Mitigées", "Mixed"),
    Negative: tr("Négatives", "Negative"),
    "Mostly Negative": tr("Plutôt négatives", "Mostly negative"),
    "Very Negative": tr("Très négatives", "Very negative"),
    "Overwhelmingly Negative": tr("Extrêmement négatives", "Overwhelmingly negative"),
  };

  const rating = $derived.by(() => {
    const s = mod?.stats;
    if (!s || s.ratings_total === 0) return null;
    const text = s.ratings_display_text ?? "";
    return { label: ratingLabels[text] ?? text, percent: s.ratings_percentage_positive, total: s.ratings_total };
  });

  /** Suffixe ordinal anglais : 1st, 2nd, 3rd, 4th, 11th… */
  function ordinalSuffix(n: number) {
    if (n % 100 >= 11 && n % 100 <= 13) return "th";
    return n % 10 === 1 ? "st" : n % 10 === 2 ? "nd" : n % 10 === 3 ? "rd" : "th";
  }

  // Fichier PC actuel : `modfile`, ou celui indiqué pour Windows quand le jeu
  // publie un fichier par plateforme.
  const latest = $derived.by(() => {
    const id = pcFileId(mod);
    if (!id) return null;
    if (mod?.modfile?.id === id) return mod.modfile;
    return files?.find((f) => f.id === id) ?? null;
  });

  // ------------------------------------------------------------ Abonnement

  let subBusy = $state(false);
  let subError = $state<AppError | null>(null);
  let subNote = $state<string | null>(null);
  let withDeps = $state(true);

  const subscribed = $derived(mod ? nav.subs.has(mod.id) : false);
  const missingDeps = $derived((deps ?? []).filter((d) => !nav.subs.has(d.mod_id)));

  // Ouvert depuis « Mes mods » : on charge les abonnements de ce jeu.
  $effect(() => {
    if (nav.subsGameId !== game.id) nav.loadSubs(game.id);
  });

  // Le message de confirmation concerne le mod affiché seulement.
  $effect(() => {
    modId;
    subNote = null;
    subError = null;
  });

  async function toggleSubscription() {
    if (!mod) return;
    if (!nav.loggedIn) {
      nav.openAccount(nav.view);
      return;
    }
    const target = mod;
    subBusy = true;
    subError = null;
    subNote = null;
    try {
      if (nav.subs.has(target.id)) {
        await api.unsubscribe(game.id, target.id);
        nav.subs.delete(target.id);
        subNote = tr("Désabonné.", "Unsubscribed.");
      } else {
        const deps = withDeps ? missingDeps : [];
        await api.subscribe(game.id, target.id);
        nav.subs.add(target.id);
        let added = 0;
        let failed = 0;
        for (const d of deps) {
          try {
            await api.subscribe(game.id, d.mod_id);
            nav.subs.add(d.mod_id);
            added++;
          } catch {
            failed++;
          }
        }
        const parts = [
          tr(
            "Abonné. Le jeu téléchargera ce mod à son prochain lancement s'il gère mod.io.",
            "Subscribed. The game will download this mod the next time it starts, if it supports mod.io.",
          ),
        ];
        if (added)
          parts.push(
            added > 1
              ? tr(`Les ${added} mods nécessaires ont aussi été ajoutés.`, `The ${added} required mods were also added.`)
              : tr("Le mod nécessaire a aussi été ajouté.", "The required mod was also added."),
          );
        if (failed)
          parts.push(
            failed > 1
              ? tr(`${failed} mods nécessaires n'ont pas pu être ajoutés.`, `${failed} required mods couldn't be added.`)
              : tr("Un mod nécessaire n'a pas pu être ajouté.", "One required mod couldn't be added."),
          );
        subNote = parts.join(" ");
      }
    } catch (e) {
      subError = asAppError(e);
    } finally {
      subBusy = false;
    }
  }

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
    if (f.virus_positive === 1) return { text: tr("Signalé par l'antivirus", "Flagged by the antivirus"), warn: true };
    if (f.virus_status === 0) return { text: tr("Analyse antivirus en attente", "Virus scan pending"), warn: false };
    if (f.virus_status >= 2) return { text: tr("Non analysé par l'antivirus", "Not scanned by the antivirus"), warn: false };
    return null;
  }
</script>

<div class="view">
  <div class="top">
    <button class="btn btn-ghost crumb" onclick={() => nav.back()}>
      <Icon name="back" />{nav.modStack.length > 1
        ? tr("Retour", "Back")
        : nav.modOrigin === "library"
          ? tr("Retour à Mes mods", "Back to My mods")
          : nav.modOrigin === "installed"
            ? tr("Retour aux mods installés", "Back to installed mods")
            : tr(`Retour à ${game.name}`, `Back to ${game.name}`)}
    </button>
  </div>

  {#if error}
    <div class="pad"><ErrorNotice {error} onretry={() => attempt++} /></div>
  {:else if !mod}
    <div class="pad skeleton" aria-busy="true" aria-label={tr("Chargement du mod", "Loading mod")}>
      <div class="line title"></div>
      <div class="line short"></div>
      <div class="hero"></div>
    </div>
  {:else}
    <header class="head">
      <div class="head-text">
        <h1>{mod.name}</h1>
        <p class="byline">
          {#if author}<span>{tr("par", "by")} <strong>{author}</strong></span>{/if}
          <span class="muted">{tr("mis à jour", "updated")} {ago(mod.date_updated)}</span>
          {#if subscribed}<span class="chip chip-ok">{tr("Abonné", "Subscribed")}</span>{/if}
          {#if mod.maturity_option > 0}<span class="chip chip-warn">{tr("Contenu adulte", "Adult content")}</span>{/if}
        </p>
      </div>
      <div class="head-side">
        <div class="actions">
          {#if mod.homepage_url}
            <button class="btn" onclick={() => open(mod?.homepage_url)}>
              {tr("Site de l'auteur", "Author's website")} <Icon name="external" size={14} />
            </button>
          {/if}
          {#if mod.profile_url}
            <button class="btn" onclick={() => open(mod?.profile_url)}>
              {tr("Ouvrir sur mod.io", "Open on mod.io")} <Icon name="external" size={14} />
            </button>
          {/if}
          {#if !nav.loggedIn}
            <button class="btn btn-primary" onclick={toggleSubscription}>{tr("Se connecter pour s'abonner", "Sign in to subscribe")}</button>
          {:else if subscribed}
            <button class="btn" onclick={toggleSubscription} disabled={subBusy}>
              {subBusy ? tr("Désabonnement…", "Unsubscribing…") : tr("Se désabonner", "Unsubscribe")}
            </button>
          {:else}
            <button class="btn btn-primary" onclick={toggleSubscription} disabled={subBusy}>
              <Icon name="plus" size={15} />{subBusy ? tr("Abonnement…", "Subscribing…") : tr("S'abonner", "Subscribe")}
            </button>
          {/if}
        </div>
        {#if nav.loggedIn && !subscribed && missingDeps.length > 0}
          <label class="with-deps">
            <input type="checkbox" bind:checked={withDeps} />
            {missingDeps.length > 1
              ? tr(
                  `M'abonner aussi aux ${missingDeps.length} mods nécessaires`,
                  `Also subscribe to the ${missingDeps.length} required mods`,
                )
              : tr("M'abonner aussi au mod nécessaire", "Also subscribe to the required mod")}
          </label>
        {/if}
      </div>
    </header>

    {#if subNote || subError}
      <div class="sub-feedback">
        {#if subError}
          <ErrorNotice error={subError} onretry={toggleSubscription} />
        {:else}
          <p class="sub-note" role="status"><Icon name="check" size={16} />{subNote}</p>
        {/if}
      </div>
    {/if}

    <div class="layout">
      <main class="main">
        <figure class="gallery">
          <div class="hero">
            <Thumb src={gallery[active]?.full} label={mod.name ?? ""} />
          </div>
          {#if gallery.length > 1}
            <div class="strip" role="group" aria-label={tr("Images du mod", "Mod images")}>
              {#each gallery as img, i (img.full)}
                <button
                  class="strip-item"
                  aria-label={tr(`Image ${i + 1} sur ${gallery.length}`, `Image ${i + 1} of ${gallery.length}`)}
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
                {tr("Vidéo", "Video")} {mod.media.youtube.length > 1 ? i + 1 : ""} {tr("sur YouTube", "on YouTube")} <Icon name="external" size={14} />
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
            <p class="muted">{tr("Chargement des versions…", "Loading versions…")}</p>
          {:else if files.length === 0}
            <p class="muted">{tr("Aucun fichier publié pour ce mod.", "No files published for this mod.")}</p>
          {:else}
            <ol class="file-list">
              {#each files as f (f.id)}
                {@const scan = scanLabel(f)}
                <li class="file">
                  <div class="file-head">
                    <span class="file-version">{f.version || tr("Sans numéro", "Unnumbered")}</span>
                    {#if f.id === latest?.id}<span class="chip">{tr("Version actuelle", "Current version")}</span>{/if}
                    {#if scan}<span class="chip" class:chip-warn={scan.warn}>{scan.text}</span>{/if}
                  </div>
                  <p class="file-meta muted">
                    <span>{date(f.date_added)}</span>
                    <span>{bytes(f.filesize)}</span>
                    {#if f.filename}<span class="filename">{f.filename}</span>{/if}
                  </p>
                  {#if f.changelog?.trim()}
                    <details>
                      <summary>{tr("Notes de version", "Release notes")}</summary>
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
        {#key mod.id}
          <InstallPanel {game} {mod} {deps} {files} />
        {/key}
        <dl class="stats">
          <div>
            <dt>{tr("Téléchargements", "Downloads")}</dt>
            <dd>{exact(mod.stats?.downloads_total)}</dd>
            <dd class="muted small">{tr(`${exact(mod.stats?.downloads_today)} aujourd'hui`, `${exact(mod.stats?.downloads_today)} today`)}</dd>
          </div>
          <div>
            <dt>{tr("Abonnés", "Subscribers")}</dt>
            <dd>{exact(mod.stats?.subscribers_total)}</dd>
          </div>
          {#if mod.stats?.popularity_rank_position}
            <div>
              <dt>{tr("Popularité", "Popularity")}</dt>
              <dd>{exact(mod.stats.popularity_rank_position)}<sup>{tr(mod.stats.popularity_rank_position === 1 ? "er" : "e", ordinalSuffix(mod.stats.popularity_rank_position))}</sup></dd>
              <dd class="muted small">{tr("sur", "of")} {exact(mod.stats.popularity_rank_total_mods)} {game.ugc_name?.toLowerCase() || "mods"}</dd>
            </div>
          {/if}
          <div>
            <dt>{tr("Évaluations", "Ratings")}</dt>
            {#if rating}
              <dd>{tr(`${rating.percent} % positives`, `${rating.percent}% positive`)}</dd>
              <dd class="meter" aria-hidden="true"><span style="width: {rating.percent}%"></span></dd>
              <dd class="muted small">
                {rating.label}, {tr(`${count(rating.total)} avis`, `${count(rating.total)} ${rating.total === 1 ? "rating" : "ratings"}`)}
              </dd>
            {:else}
              <dd class="muted">{tr("Pas encore évalué", "Not rated yet")}</dd>
            {/if}
          </div>
          {#if latest}
            <div>
              <dt>{tr("Dernière version", "Latest version")}</dt>
              <dd>{latest.version || tr("Sans numéro", "Unnumbered")}</dd>
              <dd class="muted small">{bytes(latest.filesize)}, {tr("le", "on")} {date(latest.date_added)}</dd>
            </div>
          {/if}
          <div>
            <dt>{tr("Publié le", "Published")}</dt>
            <dd>{date(mod.date_live ?? mod.date_added)}</dd>
          </div>
        </dl>

        {#if mod.platforms.length}
          <section class="side-block">
            <h2>{tr("Plateformes", "Platforms")}</h2>
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
            <h2>{tr("Nécessite aussi", "Also requires")}</h2>
            {#if depsError}
              <ErrorNotice error={depsError} onretry={() => attempt++} />
            {:else if !deps}
              <p class="muted">{tr("Chargement…", "Loading…")}</p>
            {:else if deps.length === 0}
              <p class="muted">{tr("Aucune dépendance listée.", "No dependencies listed.")}</p>
            {:else}
              <ul class="deps">
                {#each deps as d (d.mod_id)}
                  <li>
                    <button class="dep" onclick={() => nav.openMod(d.mod_id)}>
                      <span class="dep-thumb"><Thumb src={d.logo?.thumb_320x180} label={d.name ?? ""} /></span>
                      <span class="dep-text">
                        <span class="dep-name">{d.name ?? `Mod ${d.mod_id}`}</span>
                        {#if nav.subs.has(d.mod_id)}<span class="dep-sub">{tr("Abonné", "Subscribed")}</span>{/if}
                      </span>
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

  .head-side {
    display: grid;
    justify-items: end;
    gap: 0.6rem;
  }

  .with-deps {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    font-size: var(--t-sm);
    color: var(--muted);
    cursor: pointer;
  }

  .with-deps input {
    accent-color: var(--accent-fill);
    width: 1rem;
    height: 1rem;
    margin: 0;
  }

  .sub-feedback {
    padding: 0 2.5rem 1.5rem;
  }

  .sub-note {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    width: fit-content;
    padding: 0.6rem 0.9rem;
    border-radius: var(--r-md);
    background: var(--ok-bg);
    color: var(--ok);
    font-size: var(--t-md);
    font-weight: 500;
  }

  .dep-text {
    display: grid;
    gap: 0.1rem;
  }

  .dep-sub {
    font-size: var(--t-xs);
    font-weight: 600;
    color: var(--ok);
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
