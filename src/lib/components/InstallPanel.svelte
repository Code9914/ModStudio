<script lang="ts">
  import { api, asAppError, pcFileId, type AppError, type Dependency, type Game, type Mod, type Modfile } from "$lib/api";
  import { bytes } from "$lib/format";
  import { installs } from "$lib/installs.svelte";
  import ErrorNotice from "./ErrorNotice.svelte";
  import FolderSetup from "./FolderSetup.svelte";
  import Icon from "./Icon.svelte";
  import ProgressBar from "./ProgressBar.svelte";

  let {
    game,
    mod,
    deps,
    files = null,
  }: { game: Game; mod: Mod; deps: Dependency[] | null; files?: Modfile[] | null } = $props();

  const record = $derived(installs.installed(game.id, mod.id));
  const config = $derived(installs.game(game.id));
  /** Fichier PC actuel (vide pour un mod réservé au Quest, par exemple). */
  const fileId = $derived(pcFileId(mod));
  const fileVersion = $derived(
    (mod.modfile?.id === fileId ? mod.modfile?.version : files?.find((f) => f.id === fileId)?.version) || null,
  );
  const outdated = $derived(installs.outdated(game.id, mod.id, fileId));
  const missingDeps = $derived((deps ?? []).filter((d) => !installs.installed(game.id, d.mod_id)));

  let withDeps = $state(true);
  let setupOpen = $state(false);
  let pendingInstall = $state(false);
  let depId = $state<number | null>(null);
  let depLabel = $state<string | null>(null);
  let confirming = $state(false);
  let removing = $state(false);
  let error = $state<AppError | null>(null);
  let note = $state<string | null>(null);

  const progress = $derived(installs.progress[mod.id] ?? (depId ? installs.progress[depId] : null) ?? null);
  const busy = $derived(progress !== null || depId !== null);

  function start() {
    error = null;
    note = null;
    if (!installs.game(game.id)) {
      setupOpen = true;
      pendingInstall = true;
      return;
    }
    run();
  }

  async function run() {
    const updating = record !== null;
    const extra = withDeps && !updating ? [...missingDeps] : [];
    try {
      await installs.install(game.id, mod.id);
    } catch (e) {
      const err = asAppError(e);
      if (err.kind === "cancelled") note = "Installation annulée.";
      else if (err.kind === "no_game_folder") setupOpen = true;
      else error = err;
      return;
    }
    let failed = 0;
    for (const [i, d] of extra.entries()) {
      depId = d.mod_id;
      depLabel = `Mod nécessaire ${i + 1} sur ${extra.length} : ${d.name ?? `mod ${d.mod_id}`}`;
      try {
        await installs.install(game.id, d.mod_id);
      } catch {
        failed++;
      }
    }
    depId = null;
    depLabel = null;
    const parts = [updating ? "Mis à jour." : "Installé."];
    const added = extra.length - failed;
    if (added) parts.push(added > 1 ? `${added} mods nécessaires installés aussi.` : "Le mod nécessaire a été installé aussi.");
    if (failed) parts.push(failed > 1 ? `${failed} mods nécessaires n'ont pas pu être installés.` : "Un mod nécessaire n'a pas pu être installé.");
    note = parts.join(" ");
  }

  function cancel() {
    installs.cancel(depId ?? mod.id);
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
      const r = await installs.uninstall(game.id, mod.id);
      note = r.leftFolder
        ? `Désinstallé. Le dossier « ${r.leftFolder} » contient des fichiers ajoutés depuis, il a été laissé en place.`
        : "Désinstallé.";
    } catch (e) {
      error = asAppError(e);
    } finally {
      removing = false;
    }
  }

  function reveal(withMod: boolean) {
    api.revealInstall(game.id, withMod ? mod.id : null).catch((e) => (error = asAppError(e)));
  }

  function icon() {
    return game.icon?.thumb_64x64 ?? null;
  }
</script>

<section class="panel" aria-label="Installation sur ce PC">
  <h2>Sur ce PC</h2>

  {#if setupOpen}
    <FolderSetup
      gameId={game.id}
      gameName={game.name ?? `Jeu ${game.id}`}
      icon={icon()}
      onsaved={() => {
        setupOpen = false;
        if (pendingInstall) {
          pendingInstall = false;
          run();
        }
      }}
      oncancel={() => {
        setupOpen = false;
        pendingInstall = false;
      }}
    />
  {:else if progress}
    {#if depLabel}<p class="small">{depLabel}</p>{/if}
    <ProgressBar {progress} oncancel={cancel} />
  {:else if record}
    <p class="status">
      <span class="chip chip-ok">Installé</span>
      <span class="small muted">
        {record.version ? `version ${record.version}` : "version sans numéro"}, {bytes(record.size)}
      </span>
    </p>
    {#if outdated}
      <p class="small">Une nouvelle version est disponible{fileVersion ? ` (${fileVersion})` : ""}.</p>
      <button class="btn btn-primary" onclick={start} disabled={busy}>
        <Icon name="download" size={15} />Mettre à jour
      </button>
    {/if}
    {#if config}
      <p class="small muted folder" title={config.folder}>Dans {config.folder}</p>
    {/if}
    <div class="row">
      <button class="btn" onclick={() => reveal(true)}>Ouvrir le dossier</button>
      <button class="btn" class:danger={confirming} onclick={uninstall} disabled={removing}>
        {removing ? "Désinstallation…" : confirming ? "Confirmer" : "Désinstaller"}
      </button>
    </div>
  {:else}
    {#if !fileId}
      <p class="small muted">Ce mod n'a pas de fichier pour PC (Windows) : il est peut-être réservé à une autre plateforme.</p>
    {:else}
      <button class="btn btn-primary wide" onclick={start} disabled={busy}>
        <Icon name="download" size={15} />Installer{fileVersion ? ` la version ${fileVersion}` : ""}
      </button>
    {/if}
    {#if fileId && missingDeps.length > 0}
      <label class="check">
        <input type="checkbox" bind:checked={withDeps} />
        {missingDeps.length > 1
          ? `Installer aussi les ${missingDeps.length} mods nécessaires`
          : "Installer aussi le mod nécessaire"}
      </label>
    {/if}
    {#if config}
      <p class="small muted folder">
        Dans <span title={config.folder}>{config.folder}</span>
        <button class="link" onclick={() => (setupOpen = true)}>Changer</button>
      </p>
    {:else}
      <p class="small muted">Tu choisiras ensuite le dossier des mods de {game.name}.</p>
    {/if}
  {/if}

  {#if error}<ErrorNotice {error} onretry={error.kind === "conflict" ? undefined : start} />{/if}
  {#if note}<p class="note small" role="status">{note}</p>{/if}
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

  .check {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    font-size: var(--t-sm);
    color: var(--muted);
    cursor: pointer;
  }

  .check input {
    accent-color: var(--accent-fill);
    margin: 0;
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
</style>
