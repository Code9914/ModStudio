<script lang="ts">
  import {
    asAppError,
    nexus,
    type AppError,
    type NexusFile,
    type NexusGame,
    type NexusModDetail,
    type ToolOffer,
  } from "$lib/api";
  import { bytes } from "$lib/format";
  import { nav } from "$lib/nav.svelte";
  import { nexusStore, questFile, requirementMatches } from "$lib/nexus.svelte";
  import ErrorNotice from "./ErrorNotice.svelte";
  import Icon from "./Icon.svelte";
  import NexusFolderSetup from "./NexusFolderSetup.svelte";
  import NexusToolPrompt from "./NexusToolPrompt.svelte";
  import ProgressBar from "./ProgressBar.svelte";

  let {
    game,
    mod,
    files,
  }: { game: NexusGame; mod: NexusModDetail; files: NexusFile[] | null } = $props();

  const key = $derived(nexusStore.key(game.id, mod.modId));
  const record = $derived(nexusStore.installed(game.id, mod.modId));
  const config = $derived(nexusStore.game(game.id));
  const progress = $derived(nexusStore.progress[key] ?? null);
  const waiting = $derived(nexusStore.waiting[key] ?? null);
  const outdated = $derived(nexusStore.outdated(game.id, mod.modId));
  const busy = $derived(progress !== null || waiting !== null);
  const gameDir = $derived(config?.extraFolders?.game ?? null);

  // Outil de mods du jeu (UE4SS…) : proposé si ce mod en a besoin et qu'il manque.
  const gameTool = $derived(nexusStore.setups[game.id]?.tool ?? null);
  const requiredTool = $derived(
    gameTool && !gameTool.present && mod.requirements.some((r) => requirementMatches(gameTool, r.modName))
      ? gameTool
      : null,
  );
  /** Signalé par l'installation : le mod ne se chargera pas sans cet outil. */
  let neededTool = $state<ToolOffer | null>(null);
  /** Outil proposé, gardé affiché jusqu'à la fin (même une fois installé). */
  let shownTool = $state<ToolOffer | null>(null);
  $effect(() => {
    const t = neededTool ?? requiredTool;
    if (t && !shownTool) shownTool = t;
  });
  $effect(() => {
    const id = game.id;
    const domain = game.domainName;
    if (domain && !nexusStore.setups[id]) nexusStore.loadSetup(id, domain).catch(() => {});
  });
  const toolFolder = $derived(config?.rules === "bonelab" ? gameDir : (config?.folder ?? null));

  /** Chargeurs de mods déjà installés dans le jeu (`bepinex`, `melonloader`). */
  const gameLoaders = $derived(nexusStore.setups[game.id]?.engine?.tools ?? []);

  /** Préférence pour la version du mod faite pour le chargeur installé (« BepInEx » plutôt que « Standalone »). */
  function loaderScore(f: NexusFile): number {
    const text = `${f.name} ${f.description ?? ""}`.toLowerCase();
    let score = 0;
    for (const [loader, pattern, other] of [
      ["bepinex", /bepinex/, /melon/],
      ["melonloader", /melon/, /bepinex/],
    ] as const) {
      if (!gameLoaders.includes(loader)) continue;
      if (pattern.test(text)) score += 2;
      if (other.test(text)) score -= 1;
      if (/standalone|autonome/.test(text)) score -= 2;
    }
    return score;
  }

  /** Fichier conseillé : le plus récent des fichiers principaux (version PC), pour le chargeur du jeu. */
  const recommended = $derived.by(() => {
    if (!files) return null;
    const current = files.filter((f) => ["MAIN", "OPTIONAL", "UPDATE", "MISCELLANEOUS"].includes(f.category));
    const pc = current.filter((f) => !questFile(f.name));
    const pool = pc.length ? pc : current;
    const main = pool.filter((f) => f.category === "MAIN");
    const list = main.length ? main : pool;
    return (
      [...list].sort(
        (a, b) => loaderScore(b) - loaderScore(a) || Number(b.primary) - Number(a.primary) || b.date - a.date,
      )[0] ?? null
    );
  });
  const waitingFile = $derived(waiting ? (files?.find((f) => f.fileId === waiting.fileId) ?? null) : null);

  let setupOpen = $state(false);
  let pendingFile = $state<NexusFile | null>(null);
  let confirming = $state(false);
  let removing = $state(false);
  let error = $state<AppError | null>(null);
  let note = $state<string | null>(null);

  /** Lance l'installation d'un fichier (appelé aussi par la liste des fichiers). */
  export function install(file: NexusFile) {
    error = null;
    note = null;
    if (!nexusStore.ready) {
      nav.openNexusAccount(nav.view);
      return;
    }
    if (!nexusStore.game(game.id)) {
      pendingFile = file;
      setupOpen = true;
      return;
    }
    run(file);
  }

  async function run(file: NexusFile) {
    const updating = record !== null;
    try {
      const r = await nexusStore.request(game.id, game.domainName, mod.modId, file.fileId, file.name);
      note = [updating ? "Mis à jour." : "Installé.", r.notice].filter(Boolean).join(" ");
      if (r.needsTool) neededTool = r.needsTool;
    } catch (e) {
      const err = asAppError(e);
      if (err.kind === "cancelled") note = "Téléchargement annulé.";
      else if (err.kind === "no_game_folder" || err.kind === "need_game_dir") {
        if (err.kind === "need_game_dir") error = err;
        pendingFile = file;
        setupOpen = true;
      } else error = err;
    }
  }

  function cancel() {
    if (waiting) nexusStore.stopWaiting(game.id, game.domainName, mod.modId);
    else nexusStore.cancel(mod.modId);
  }

  function reopen() {
    if (waiting) nexusStore.reopen(game.id, waiting.domain, mod.modId, waitingFile?.name ?? null);
  }

  let confirmTimer: ReturnType<typeof setTimeout> | undefined;

  async function uninstall() {
    if (!confirming) {
      confirming = true;
      clearTimeout(confirmTimer);
      confirmTimer = setTimeout(() => (confirming = false), 4000);
      return;
    }
    confirming = false;
    removing = true;
    error = null;
    note = null;
    try {
      const left = await nexusStore.uninstall(game.id, mod.modId);
      note = left
        ? `Désinstallé. Le dossier « ${left} » contient des fichiers ajoutés depuis, il a été laissé en place.`
        : "Désinstallé.";
    } catch (e) {
      error = asAppError(e);
    } finally {
      removing = false;
    }
  }

  function reveal() {
    nexus.reveal(game.id, mod.modId).catch((e) => (error = asAppError(e)));
  }
</script>

<section class="panel" aria-label="Installation sur ce PC">
  <h2>Sur ce PC</h2>

  {#if setupOpen}
    <NexusFolderSetup
      gameId={game.id}
      domain={game.domainName}
      gameName={game.name}
      onsaved={() => {
        setupOpen = false;
        error = null;
        if (pendingFile) {
          const f = pendingFile;
          pendingFile = null;
          run(f);
        }
      }}
      oncancel={() => {
        setupOpen = false;
        pendingFile = null;
      }}
    />
  {:else if waiting}
    <div class="waiting" role="status">
      <p class="small">
        <span class="strong">Clique sur « Slow download » sur la page Nexus Mods.</span>
        L'installation{waitingFile ? ` de « ${waitingFile.name} »` : ""} démarrera toute seule.
      </p>
      <div class="row">
        <button class="btn" onclick={reopen}>Rouvrir la page</button>
        <button class="btn btn-ghost" onclick={cancel}>Annuler</button>
      </div>
    </div>
  {:else if progress}
    <ProgressBar {progress} oncancel={cancel} />
  {:else if record}
    <p class="status">
      <span class="chip chip-ok">Installé</span>
      <span class="small muted">
        {record.version ? `version ${record.version}` : "version sans numéro"}, {bytes(record.size)}
      </span>
    </p>
    {#if record.fileName}<p class="small">{record.fileName}</p>{/if}
    {#if outdated}
      <p class="small">
        Ce fichier a été remplacé sur Nexus Mods par une version plus récente.
        {#if recommended && recommended.fileId !== record.modfileId}Installe « {recommended.name} » pour mettre à jour.{/if}
      </p>
      {#if recommended && recommended.fileId !== record.modfileId}
        <button class="btn btn-primary wide" onclick={() => install(recommended!)} disabled={busy}>
          <Icon name="download" size={15} />Mettre à jour
        </button>
      {/if}
    {/if}
    {#if config}
      <p class="small muted folder" title={config.folder}>Dans {config.folder}</p>
      {#if record.parts?.length && gameDir}
        <p class="small muted folder" title={gameDir}>Et dans le dossier du jeu : {gameDir}</p>
      {/if}
    {/if}
    <div class="row">
      <button class="btn" onclick={reveal}>Ouvrir le dossier</button>
      <button class="btn" class:danger={confirming} onclick={uninstall} disabled={removing}>
        {removing ? "Désinstallation…" : confirming ? "Confirmer" : "Désinstaller"}
      </button>
    </div>
  {:else if files === null}
    <p class="small muted">Chargement des fichiers…</p>
  {:else if !recommended}
    <p class="small muted">Aucun fichier à télécharger pour ce mod.</p>
  {:else}
    <button class="btn btn-primary wide" onclick={() => install(recommended!)} disabled={busy}>
      <Icon name="download" size={15} />{nexusStore.ready ? "Installer" : "Ajouter ma clé pour installer"}
    </button>
    <p class="small muted">
      Fichier conseillé : « {recommended.name} »{recommended.version ? `, version ${recommended.version}` : ""},
      {bytes(recommended.sizeInBytes || recommended.size * 1024)}. Les autres fichiers sont dans la liste.
    </p>
    {#if config}
      <p class="small muted folder">
        Dans <span title={config.folder}>{config.folder}</span>
        <button class="link" onclick={() => (setupOpen = true)}>Changer</button>
      </p>
    {:else}
      <p class="small muted">Tu choisiras ensuite où installer les mods de {game.name}.</p>
    {/if}
  {/if}

  {#if nexusStore.ready && !nexusStore.premium && !record && !busy && !setupOpen}
    <p class="small muted">
      Sans compte Premium, Nexus Mods demande de passer par la page du fichier : ModStudio l'ouvre pour toi.
    </p>
  {/if}

  {#if error}<ErrorNotice {error} />{/if}
  {#if note}<p class="note small" role="status">{note}</p>{/if}

  {#if shownTool && config && !setupOpen}
    <div class="needs">
      <NexusToolPrompt
        {game}
        tool={shownTool}
        folder={toolFolder}
        reason={record ? `Ce mod ne se chargera pas sans ${shownTool.name}.` : `Ce mod a besoin de ${shownTool.name}.`}
      />
    </div>
  {/if}
</section>

<style>
  .panel {
    display: grid;
    gap: 0.75rem;
    padding: 1.25rem;
    border-radius: var(--r-md);
    background: var(--panel);
    min-width: 0;
    overflow-wrap: anywhere;
  }

  h2 {
    font-size: var(--t-md);
    font-weight: 700;
  }

  .small {
    font-size: var(--t-sm);
  }

  .strong {
    font-weight: 600;
    color: var(--text);
  }

  .status {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.5rem;
  }

  .row {
    display: flex;
    flex-wrap: wrap;
    gap: 0.4rem;
  }

  .wide {
    justify-content: center;
  }

  .danger {
    background: var(--danger-bg);
    border-color: var(--danger);
    color: var(--danger);
  }

  .folder {
    overflow-wrap: anywhere;
  }

  .link {
    padding: 0;
    border: 0;
    background: none;
    color: var(--accent);
    text-decoration: underline;
    text-underline-offset: 3px;
    font-size: inherit;
    margin-left: 0.25rem;
  }

  .note {
    color: var(--ok);
  }

  .needs {
    padding: 0.8rem 0.9rem;
    border-radius: var(--r-md);
    background: var(--raise);
    border-left: 3px solid var(--accent-fill);
  }

  .waiting {
    display: grid;
    gap: 0.7rem;
    padding: 0.8rem 0.9rem;
    border-radius: var(--r-md);
    background: var(--raise);
    border-left: 3px solid var(--accent-fill);
  }
</style>
