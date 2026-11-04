<script lang="ts">
  // Page d'une collection Nexus : ses mods, et leur installation dans l'ordre
  // de la collection.
  import DOMPurify from "dompurify";
  import { SvelteSet } from "svelte/reactivity";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { asAppError, nexus, type AppError, type NexusCollectionDetail, type NexusCollectionMod, type NexusGame } from "$lib/api";
  import { bbcodeToHtml, plainText } from "$lib/bbcode";
  import { bytes, count, exact } from "$lib/format";
  import { tr } from "$lib/i18n.svelte";
  import { nav } from "$lib/nav.svelte";
  import { nexusStore } from "$lib/nexus.svelte";
  import { collectionInstall, type ItemStatus } from "$lib/collections.svelte";
  import ErrorNotice from "./ErrorNotice.svelte";
  import Icon from "./Icon.svelte";
  import NexusFolderSetup from "./NexusFolderSetup.svelte";
  import ProgressBar from "./ProgressBar.svelte";
  import Thumb from "./Thumb.svelte";
  import Translatable from "./Translatable.svelte";

  let { game, slug }: { game: NexusGame; slug: string } = $props();

  let detail = $state<NexusCollectionDetail | null>(null);
  let error = $state<AppError | null>(null);
  let attempt = $state(0);
  let setupOpen = $state(false);
  /** Mods optionnels cochés (numéros de fichier). */
  const chosen = new SvelteSet<number>();

  $effect(() => {
    attempt;
    let cancelled = false;
    error = null;
    nexus
      .collection(slug, game.domainName)
      .then((d) => !cancelled && (detail = d))
      .catch((e) => !cancelled && (error = asAppError(e)));
    return () => {
      cancelled = true;
    };
  });

  // La description vient du créateur de la collection : convertie puis assainie.
  const description = $derived.by(() => {
    if (!detail?.description) return "";
    return DOMPurify.sanitize(bbcodeToHtml(detail.description), {
      FORBID_TAGS: ["style", "form", "input", "button", "textarea", "select", "iframe", "script", "object", "embed"],
      FORBID_ATTR: ["style", "class", "id", "srcset"],
    });
  });

  const configured = $derived(!!nexusStore.games[game.id]);
  const active = $derived(collectionInstall.on(slug));
  const running = $derived(active && collectionInstall.busy);

  function installedFile(m: NexusCollectionMod) {
    return nexusStore.installed(game.id, m.modId);
  }

  /** Mods à installer : requis absents, et optionnels cochés absents. */
  const missing = $derived(
    (detail?.mods ?? []).filter((m) => !installedFile(m) && (!m.optional || chosen.has(m.fileId))),
  );
  const missingSize = $derived(missing.reduce((n, m) => n + m.size, 0));
  const required = $derived((detail?.mods ?? []).filter((m) => !m.optional));
  const optional = $derived((detail?.mods ?? []).filter((m) => m.optional));

  /** Ordre affiché : celui de l'installation une fois lu, sinon celui de la liste. */
  function ordered(list: NexusCollectionMod[]) {
    if (!active || collectionInstall.items.length === 0) return list;
    const rank = new Map(collectionInstall.items.map((i) => [i.fileId, i.rank]));
    return [...list].sort((a, b) => (rank.get(a.fileId) ?? 1e9) - (rank.get(b.fileId) ?? 1e9));
  }

  const statusLabels: Record<ItemStatus, string> = {
    pending: tr("En attente", "Waiting"),
    working: tr("En cours…", "In progress…"),
    installed: tr("Installé", "Installed"),
    kept: tr("Déjà installé", "Already installed"),
    skipped: tr("Passé", "Skipped"),
    failed: tr("Erreur", "Error"),
  };

  function status(m: NexusCollectionMod): { text: string; tone: "ok" | "accent" | "warn" | "muted" } {
    const item = active ? collectionInstall.item(m.fileId) : null;
    if (item && item.status !== "skipped") {
      const tone = item.status === "installed" || item.status === "kept" ? "ok" : item.status === "failed" ? "warn" : "accent";
      return { text: statusLabels[item.status], tone };
    }
    const rec = installedFile(m);
    if (rec) {
      return rec.modfileId === m.fileId
        ? { text: tr("Installé", "Installed"), tone: "ok" }
        : { text: tr("Autre version installée", "Other version installed"), tone: "ok" };
    }
    return m.optional ? { text: tr("Optionnel", "Optional"), tone: "muted" } : { text: tr("À installer", "To install"), tone: "muted" };
  }

  async function install() {
    if (!detail) return;
    if (!configured) {
      setupOpen = true;
      return;
    }
    await collectionInstall.start(detail, game.id, new Set(chosen));
  }

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

  const current = $derived(active ? collectionInstall.items.find((i) => i.status === "working") ?? null : null);
  const currentProgress = $derived(current ? nexusStore.progress[nexusStore.key(game.id, current.modId)] : null);
  const currentWaiting = $derived(current ? !!nexusStore.waiting[nexusStore.key(game.id, current.modId)] : false);
  const counts = $derived(collectionInstall.counts);
  const failed = $derived(active ? collectionInstall.items.filter((i) => i.status === "failed") : []);
  const notices = $derived(active ? collectionInstall.items.filter((i) => i.notice) : []);
  const extras = $derived(active ? (collectionInstall.plan?.extras ?? []) : []);
</script>

<div class="view">
  <div class="top">
    <button class="btn btn-ghost crumb" onclick={() => nav.back()}>
      <Icon name="back" />{tr(`Collections de ${game.name}`, `${game.name} collections`)}
    </button>
  </div>

  {#if error}
    <div class="pad"><ErrorNotice {error} onretry={() => attempt++} /></div>
  {:else if !detail}
    <div class="pad" aria-busy="true">
      <div class="skeleton-hero"></div>
      <div class="skeleton-line"></div>
    </div>
  {:else}
    <header class="head">
      <div class="hero"><Thumb src={detail.header ?? detail.image} label={detail.name} /></div>
      <div class="head-text">
        <p class="eyebrow">{tr("Collection Nexus Mods", "Nexus Mods collection")}{detail.category ? ` · ${detail.category}` : ""}</p>
        <h1>{detail.name}</h1>
        <p class="meta">
          {#if detail.author}<span>{tr("par", "by")} <strong>{detail.author}</strong></span>{/if}
          <span>{detail.mods.length} mods{required.length !== detail.mods.length ? ` (${optional.length} ${tr("optionnels", "optional")})` : ""}</span>
          {#if detail.size > 0}<span>{bytes(detail.size)}</span>{/if}
          <span title={tr("Recommandations", "Endorsements")}><Icon name="thumb" size={14} /> {count(detail.endorsements)}</span>
          <span title={tr("Téléchargements", "Downloads")}><Icon name="download" size={14} /> {count(detail.downloads)}</span>
          <span class="muted">{tr(`Version ${detail.revision}`, `Revision ${detail.revision}`)}</span>
        </p>
        <div class="actions">
          {#if !nexusStore.ready}
            <button class="btn btn-primary" onclick={() => nav.openNexusAccount({ name: "nexusCollection" })}>{tr("Ajouter ma clé Nexus Mods", "Add my Nexus Mods key")}</button>
          {:else if running}
            <button class="btn" onclick={() => collectionInstall.pause()}>{tr("Mettre en pause", "Pause")}</button>
          {:else if active && collectionInstall.phase === "paused"}
            <button class="btn btn-primary" onclick={() => collectionInstall.resume()}>{tr("Reprendre l'installation", "Resume installation")}</button>
          {:else if active && failed.length > 0}
            <button class="btn btn-primary" onclick={() => collectionInstall.resume()}>
              <Icon name="refresh" size={15} />
              {failed.length === 1
                ? tr("Réessayer le mod en erreur", "Retry the failed mod")
                : tr(`Réessayer les ${failed.length} mods en erreur`, `Retry the ${failed.length} failed mods`)}
            </button>
          {:else if missing.length > 0}
            <button class="btn btn-primary" onclick={install} disabled={collectionInstall.busy}>
              <Icon name="download" size={15} />
              {missing.length === 1
                ? tr(`Installer 1 mod (${bytes(missingSize)})`, `Install 1 mod (${bytes(missingSize)})`)
                : tr(`Installer ${missing.length} mods (${bytes(missingSize)})`, `Install ${missing.length} mods (${bytes(missingSize)})`)}
            </button>
          {:else}
            <span class="all-set"><Icon name="check" size={15} /> {tr("Tous ses mods sont installés", "All its mods are installed")}</span>
          {/if}
          {#if active && !running}
            <button class="btn btn-ghost" onclick={() => collectionInstall.close()}>{tr("Fermer le suivi", "Close progress")}</button>
          {/if}
          <button class="btn btn-ghost" onclick={() => open(`https://www.nexusmods.com/games/${game.domainName}/collections/${slug}`)}>
            {tr("Voir sur Nexus Mods", "View on Nexus Mods")} <Icon name="external" size={14} />
          </button>
        </div>
        {#if collectionInstall.busy && !active}
          <p class="small muted">{tr(`Une autre collection s'installe (${collectionInstall.name}) : attends qu'elle se termine.`, `Another collection is being installed (${collectionInstall.name}): wait for it to finish.`)}</p>
        {/if}
        {#if nexusStore.ready && !nexusStore.premium && missing.length > 0 && !active}
          <p class="small muted">
            {tr(
              "Sans compte Premium, la page de chaque mod s'ouvre dans ModStudio : clique sur « Slow download » pour chacun, l'installation continue toute seule ensuite.",
              'Without a Premium account, each mod\'s page opens in ModStudio: click "Slow download" for each one, the installation then carries on by itself.',
            )}
          </p>
        {/if}
      </div>
    </header>

    {#if setupOpen}
      <div class="pad">
        <NexusFolderSetup
          gameId={game.id}
          domain={game.domainName}
          gameName={game.name}
          onsaved={() => {
            setupOpen = false;
            install();
          }}
          oncancel={() => (setupOpen = false)}
        />
      </div>
    {/if}

    {#if active}
      <section class="progress-box" aria-live="polite">
        {#if collectionInstall.phase === "planning"}
          <p>{tr("Lecture de l'ordre de la collection…", "Reading the collection's order…")}</p>
          <ProgressBar progress={{ label: tr("Préparation…", "Preparing…"), ratio: null }} />
        {:else}
          <p>
            <strong>{counts.done} / {counts.todo}</strong>
            {tr("mods en place", "mods in place")}
            {#if counts.failed > 0}<span class="warn">· {counts.failed} {tr("en erreur", "with errors")}</span>{/if}
            {#if collectionInstall.phase === "done"}· {tr("terminé", "done")}{:else if collectionInstall.phase === "paused"}· {tr("en pause", "paused")}{/if}
          </p>
          {#if current}
            <p class="small">
              {current.name}
              {#if currentWaiting}
                — <button class="link" onclick={() => nexusStore.reopen(game.id, game.domainName, current.modId, current.fileName)}>
                  {tr("clique sur « Slow download » sur sa page", 'click "Slow download" on its page')}
                </button>
              {/if}
            </p>
            {#if currentProgress}<ProgressBar progress={currentProgress} />{/if}
          {/if}
          {#if collectionInstall.plan && !collectionInstall.plan.fromManifest}
            <p class="small muted">
              {tr(
                "L'archive de la collection n'a pas pu être lue : les mods s'installent dans l'ordre de sa liste.",
                "The collection's archive couldn't be read: mods are installed in the order of its list.",
              )}
            </p>
          {/if}
        {/if}
        {#if collectionInstall.error}<ErrorNotice error={collectionInstall.error} onretry={() => collectionInstall.resume()} />{/if}
      </section>
    {/if}

    <div class="layout">
      <main class="main">
        {#if detail.summary || description}
          <Translatable sources={[{ text: detail.summary ? plainText(detail.summary) : "" }, { text: description, html: true }]}>
            {#snippet children([summary, body])}
              {#if summary}<p class="lead">{summary}</p>{/if}
              {#if body}
                <details class="desc">
                  <summary>{tr("Description", "Description")}</summary>
                  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
                  <div class="prose" onclick={onDescriptionClick}>{@html body}</div>
                </details>
              {/if}
            {/snippet}
          </Translatable>
        {/if}

        {#if detail.installInfo}
          <section class="note">
            <h2>{tr("Conseils du créateur", "Creator's notes")}</h2>
            <p class="pre">{detail.installInfo}</p>
          </section>
        {/if}

        <section>
          <h2>{tr("Mods de la collection", "Mods in the collection")} <span class="muted">{exact(required.length)}</span></h2>
          <ul class="mods">
            {#each ordered(required) as m (m.fileId)}
              {@const st = status(m)}
              {@const item = active ? collectionInstall.item(m.fileId) : null}
              <li class="mod">
                <div class="mini"><Thumb src={m.image} label={m.name} /></div>
                <div class="mod-text">
                  <button class="link name" onclick={() => nav.openNexusMod(m.modId)}>{m.name}</button>
                  <span class="small muted">{m.fileName}{m.version ? ` · ${m.version}` : ""} · {bytes(m.size)}</span>
                  {#if item?.error}<span class="small warn">{item.error.message}</span>{/if}
                  {#if item?.notice}<span class="small">{item.notice}</span>{/if}
                  {#if item?.instructions}<span class="small muted">{tr("Conseil :", "Tip:")} {item.instructions}</span>{/if}
                </div>
                <span class="state {st.tone}">{st.text}</span>
              </li>
            {/each}
          </ul>
        </section>

        {#if optional.length > 0}
          <section>
            <h2>{tr("Mods optionnels", "Optional mods")} <span class="muted">{exact(optional.length)}</span></h2>
            <p class="small muted">{tr("Coche ceux que tu veux installer avec la collection.", "Tick the ones you want to install with the collection.")}</p>
            <ul class="mods">
              {#each ordered(optional) as m (m.fileId)}
                {@const st = status(m)}
                {@const item = active ? collectionInstall.item(m.fileId) : null}
                <li class="mod">
                  {#if !installedFile(m) && !active}
                    <input
                      type="checkbox"
                      class="pick"
                      aria-label={tr(`Installer « ${m.name} »`, `Install "${m.name}"`)}
                      checked={chosen.has(m.fileId)}
                      onchange={(e) => (e.currentTarget.checked ? chosen.add(m.fileId) : chosen.delete(m.fileId))}
                    />
                  {/if}
                  <div class="mini"><Thumb src={m.image} label={m.name} /></div>
                  <div class="mod-text">
                    <button class="link name" onclick={() => nav.openNexusMod(m.modId)}>{m.name}</button>
                    <span class="small muted">{m.fileName}{m.version ? ` · ${m.version}` : ""} · {bytes(m.size)}</span>
                    {#if item?.error}<span class="small warn">{item.error.message}</span>{/if}
                  </div>
                  <span class="state {st.tone}">{st.text}</span>
                </li>
              {/each}
            </ul>
          </section>
        {/if}
      </main>

      <aside class="side">
        {#if failed.length > 0 && !running}
          <section class="box">
            <h2>{tr("Pas installés", "Not installed")}</h2>
            <ul class="small">
              {#each failed as i (i.fileId)}<li><strong>{i.name}</strong> : {i.error?.message}</li>{/each}
            </ul>
          </section>
        {/if}
        {#if notices.length > 0}
          <section class="box">
            <h2>{tr("À savoir", "Good to know")}</h2>
            <ul class="small">
              {#each notices as i (i.fileId)}<li><strong>{i.name}</strong> : {i.notice}</li>{/each}
            </ul>
          </section>
        {/if}
        {#if detail.external.length > 0 || extras.length > 0}
          <section class="box">
            <h2>{tr("À faire à la main", "To do by hand")}</h2>
            <p class="small muted">
              {tr(
                "La collection demande aussi ceci, que ModStudio n'installe pas lui-même :",
                "The collection also asks for this, which ModStudio doesn't install itself:",
              )}
            </p>
            <ul class="small extras">
              {#each detail.external as e, i (i)}
                <li>
                  {#if e.url}<button class="link" onclick={() => open(e.url)}>{e.name} <Icon name="external" size={12} /></button>{:else}{e.name}{/if}
                  {#if e.optional}<span class="muted">({tr("optionnel", "optional")})</span>{/if}
                </li>
              {/each}
              {#each extras as e, i (i)}
                <li>
                  {#if e.url}<button class="link" onclick={() => open(e.url)}>{e.name} <Icon name="external" size={12} /></button>{:else}{e.name}{/if}
                  <span class="muted">
                    {e.kind === "bundle"
                      ? tr("(fichiers fournis dans la collection)", "(files included in the collection)")
                      : e.kind === "other_game"
                        ? tr("(mod d'un autre jeu)", "(mod for another game)")
                        : ""}
                  </span>
                  {#if e.instructions}<p class="muted">{e.instructions}</p>{/if}
                </li>
              {/each}
            </ul>
          </section>
        {/if}
        <section class="box">
          <h2>{tr("Fichiers en commun", "Shared files")}</h2>
          <p class="small muted">
            {tr(
              "Quand deux mods de la collection remplacent le même fichier, celui qu'elle place après gagne. ModStudio garde l'autre de côté et le remet si tu désactives ou retires le gagnant.",
              "When two mods in the collection replace the same file, the one it places later wins. ModStudio keeps the other aside and puts it back if you disable or remove the winner.",
            )}
          </p>
        </section>
      </aside>
    </div>
  {/if}
</div>

<style>
  .view {
    height: 100%;
    overflow-y: auto;
  }

  .top {
    padding: 1rem 2.5rem 0;
  }

  .crumb {
    margin-left: -0.5rem;
  }

  .pad {
    padding: 1rem 2.5rem;
  }

  .head {
    display: grid;
    grid-template-columns: minmax(14rem, 22rem) 1fr;
    gap: 1.75rem;
    padding: 1rem 2.5rem 1.5rem;
    align-items: start;
  }

  .eyebrow {
    font-size: var(--t-xs);
    font-weight: 700;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--accent);
  }

  h1 {
    font-size: clamp(var(--t-2xl), 3.5vw, var(--t-3xl));
    font-weight: 800;
    letter-spacing: -0.02em;
    text-wrap: balance;
    margin: 0.2rem 0 0.5rem;
  }

  .head-text {
    display: grid;
    gap: 0.6rem;
    align-content: start;
  }

  .meta {
    display: flex;
    flex-wrap: wrap;
    gap: 0.3rem 1.1rem;
    color: var(--muted);
  }

  .meta strong {
    color: var(--text);
  }

  .meta span {
    display: inline-flex;
    align-items: center;
    gap: 0.25rem;
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.5rem 0.75rem;
  }

  .all-set {
    display: inline-flex;
    align-items: center;
    gap: 0.35rem;
    color: var(--ok);
    font-weight: 600;
  }

  .progress-box {
    margin: 0 2.5rem 1.5rem;
    padding: 1rem 1.2rem;
    border-radius: var(--r-md);
    background: var(--panel);
    display: grid;
    gap: 0.5rem;
  }

  .warn {
    color: var(--warn, var(--danger));
  }

  .layout {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 20rem;
    gap: 2rem;
    padding: 0 2.5rem 3rem;
  }

  .main {
    display: grid;
    gap: 1.75rem;
    align-content: start;
  }

  h2 {
    font-size: var(--t-lg);
    font-weight: 700;
    margin-bottom: 0.6rem;
  }

  h2 .muted {
    font-weight: 500;
    font-size: var(--t-md);
  }

  .lead {
    font-size: var(--t-md);
  }

  .desc summary {
    cursor: pointer;
    font-weight: 600;
    margin-bottom: 0.5rem;
  }

  .pre {
    white-space: pre-wrap;
  }

  .note {
    padding: 1rem 1.2rem;
    border-radius: var(--r-md);
    background: var(--panel);
  }

  .mods {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 0.4rem;
  }

  .mod {
    display: flex;
    align-items: center;
    gap: 0.8rem;
    padding: 0.5rem 0.6rem;
    border-radius: var(--r-md);
    background: var(--panel);
  }

  .mini {
    width: 5.5rem;
    flex: none;
  }

  .mod-text {
    display: grid;
    gap: 0.1rem;
    min-width: 0;
    flex: 1;
  }

  .mod-text span {
    overflow-wrap: anywhere;
  }

  .name {
    font-weight: 650;
    text-align: left;
  }

  .link {
    padding: 0;
    border: 0;
    background: none;
    color: var(--accent);
    font: inherit;
    cursor: pointer;
    text-align: left;
  }

  .link:hover {
    text-decoration: underline;
  }

  .state {
    flex: none;
    font-size: var(--t-xs);
    font-weight: 700;
    padding: 0.2rem 0.55rem;
    border-radius: 999px;
    background: var(--raise);
    color: var(--muted);
  }

  .state.ok {
    color: var(--ok);
  }

  .state.accent {
    color: var(--accent);
  }

  .state.warn {
    color: var(--warn, var(--danger));
  }

  .pick {
    width: 1.1rem;
    height: 1.1rem;
    flex: none;
    accent-color: var(--accent-fill);
  }

  .side {
    display: grid;
    gap: 1rem;
    align-content: start;
  }

  .box {
    padding: 1rem 1.1rem;
    border-radius: var(--r-md);
    background: var(--panel);
    display: grid;
    gap: 0.5rem;
  }

  .box h2 {
    font-size: var(--t-md);
    margin: 0;
  }

  .box ul {
    margin: 0;
    padding-left: 1.1rem;
    display: grid;
    gap: 0.35rem;
  }

  .extras p {
    margin: 0.15rem 0 0;
  }

  .skeleton-hero {
    height: 12rem;
    border-radius: var(--r-md);
    background: var(--raise);
  }

  .skeleton-line {
    height: 1.2rem;
    width: 40%;
    margin-top: 1rem;
    border-radius: var(--r-sm);
    background: var(--raise);
  }

  @media (max-width: 1150px) {
    .head,
    .layout {
      grid-template-columns: 1fr;
      padding-inline: 1.75rem;
    }

    .top,
    .pad {
      padding-inline: 1.75rem;
    }

    .progress-box {
      margin-inline: 1.75rem;
    }

    .head .hero {
      max-width: 26rem;
    }
  }
</style>
