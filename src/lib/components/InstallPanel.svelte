<script lang="ts">
  import { api, asAppError, pcFileId, type AppError, type Dependency, type Game, type Mod, type Modfile } from "$lib/api";
  import { bytes } from "$lib/format";
  import { installs } from "$lib/installs.svelte";
  import { nav } from "$lib/nav.svelte";
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

  // Jeu qui installe ses mods lui-même via mod.io : l'abonnement suffit.
  $effect(() => {
    installs.detectManaged(game.id);
  });
  const managed = $derived(installs.managed[game.id] ?? null);
  const subscribed = $derived(nav.subs.has(mod.id));
  let subBusy = $state(false);
  let showManual = $state(false);

  async function subscribe() {
    if (!nav.loggedIn) {
      nav.openAccount(nav.view);
      return;
    }
    subBusy = true;
    error = null;
    try {
      await api.subscribe(game.id, mod.id);
      nav.subs.add(mod.id);
      note = `Abonné. ${game.name ?? "Le jeu"} le téléchargera à son prochain lancement.`;
    } catch (e) {
      error = asAppError(e);
    } finally {
      subBusy = false;
    }
  }

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
        await installs.install(game.id, d.mod_id, mod.id);
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
      orphans = r.orphans;
    } catch (e) {
      error = asAppError(e);
    } finally {
      removing = false;
    }
  }

  /** Mods nécessaires installés avec ce mod et dont plus rien n'a besoin. */
  let orphans = $state<{ modId: number; name: string }[]>([]);
  let removingOrphans = $state(false);

  async function removeOrphans() {
    removingOrphans = true;
    const failed: string[] = [];
    for (const o of orphans) {
      try {
        await installs.uninstall(game.id, o.modId);
      } catch {
        failed.push(o.name);
      }
    }
    removingOrphans = false;
    orphans = [];
    note = failed.length
      ? `Impossible de désinstaller : ${failed.join(", ")}. Ferme le jeu puis réessaie depuis la page Installés.`
      : "Désinstallé, avec les mods nécessaires qui ne servaient plus.";
  }

  const parents = $derived(
    (record?.requiredBy ?? []).map((id) => installs.installed(game.id, id)?.name ?? `mod ${id}`),
  );

  function reveal(withMod: boolean) {
    api.revealInstall(game.id, withMod ? mod.id : null).catch((e) => (error = asAppError(e)));
  }

  function icon() {
    return game.icon?.thumb_64x64 ?? null;
  }

  let toggling = $state(false);

  async function setEnabled(enabled: boolean) {
    toggling = true;
    error = null;
    try {
      await installs.setEnabled(game.id, mod.id, enabled);
    } catch (e) {
      error = asAppError(e);
    } finally {
      toggling = false;
    }
  }
</script>

<section class="panel" aria-label="Installation sur ce PC">
  <h2>Sur ce PC</h2>

  {#if setupOpen}
    <FolderSetup
      gameId={game.id}
      gameName={game.name ?? `Jeu ${game.id}`}
      icon={icon()}
      guideUrl={game.instructions_url}
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
      {#if record.disabled}<span class="chip">Désactivé</span>{:else}<span class="chip chip-ok">Installé</span>{/if}
      <span class="small muted">
        {record.version ? `version ${record.version}` : "version sans numéro"}, {bytes(record.size)}
      </span>
    </p>
    {#if record.disabled}
      <p class="small muted">Ses fichiers sont mis de côté : le jeu ne le charge plus.</p>
    {/if}
    {#if record.auto && parents.length}
      <p class="small muted">Installé comme mod nécessaire de « {parents.join(" », « ")} ».</p>
    {/if}
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
      <button class="btn" onclick={() => setEnabled(!!record?.disabled)} disabled={toggling || removing}>
        {toggling ? "…" : record.disabled ? "Réactiver" : "Désactiver"}
      </button>
      <button class="btn" onclick={() => reveal(true)} disabled={record.disabled}>Ouvrir le dossier</button>
      <button class="btn" class:danger={confirming} onclick={uninstall} disabled={removing}>
        {removing ? "Désinstallation…" : confirming ? "Confirmer" : "Désinstaller"}
      </button>
    </div>
  {:else if managed && !showManual}
    <div class="managed">
      <p class="small">
        <span class="strong">{game.name} installe ses mods lui-même grâce à mod.io.</span>
        Abonne-toi : le jeu téléchargera ce mod à son prochain lancement.
      </p>
      <p class="small muted folder" title={managed.folder}>
        Dossier du jeu : {managed.folder}{managed.mods > 0
          ? `, ${managed.mods} ${managed.mods > 1 ? "mods déjà téléchargés" : "mod déjà téléchargé"}`
          : ""}
      </p>
      {#if subscribed}
        <span class="chip chip-ok">Abonné</span>
      {:else}
        <button class="btn btn-primary wide" onclick={subscribe} disabled={subBusy}>
          {#if !nav.loggedIn}Se connecter pour s'abonner{:else if subBusy}Abonnement…{:else}<Icon
              name="plus"
              size={15}
            />S'abonner{/if}
        </button>
      {/if}
    </div>
    <button class="link small manual" onclick={() => (showManual = true)}>Installer quand même à la main</button>
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
  {#if orphans.length}
    <div class="orphans">
      <p class="small">
        {orphans.length > 1
          ? `${orphans.map((o) => `« ${o.name} »`).join(", ")} avaient été installés avec ce mod et ne servent plus à aucun autre.`
          : `« ${orphans[0].name} » avait été installé avec ce mod et ne sert plus à aucun autre.`}
        {orphans.length > 1 ? "Les désinstaller aussi ?" : "Le désinstaller aussi ?"}
      </p>
      <div class="row">
        <button class="btn btn-primary" onclick={removeOrphans} disabled={removingOrphans}>
          {removingOrphans ? "Désinstallation…" : "Désinstaller aussi"}
        </button>
        <button class="btn btn-ghost" onclick={() => (orphans = [])}>Garder</button>
      </div>
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

  .orphans {
    display: grid;
    gap: 0.6rem;
    padding: 0.8rem 0.9rem;
    border-radius: var(--r-md);
    background: var(--raise);
  }

  .managed {
    display: grid;
    gap: 0.6rem;
    justify-items: start;
  }

  .managed .wide {
    justify-self: stretch;
  }

  .strong {
    font-weight: 600;
    color: var(--text);
  }

  .manual {
    justify-self: start;
    margin-left: 0;
    font-size: var(--t-sm);
  }
</style>
