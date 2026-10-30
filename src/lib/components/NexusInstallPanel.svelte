<script lang="ts">
  import { SvelteSet } from "svelte/reactivity";
  import {
    asAppError,
    nexus,
    type AppError,
    type FomodInstaller,
    type NexusFile,
    type NexusGame,
    type NexusInstallOutcome,
    type NexusInstalled,
    type NexusModDetail,
    type ToolOffer,
  } from "$lib/api";
  import { bytes } from "$lib/format";
  import { nav } from "$lib/nav.svelte";
  import { nexusStore, recommendedFile, requirementMatches } from "$lib/nexus.svelte";
  import ErrorNotice from "./ErrorNotice.svelte";
  import FomodWizard from "./FomodWizard.svelte";
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

  // ------------------------------------------------------------ Mods nécessaires
  /** Mods nécessaires hébergés sur Nexus pour ce jeu, pas encore installés. */
  const missingDeps = $derived(
    mod.requirements.filter(
      (r, i, all) =>
        !r.externalRequirement &&
        r.gameId === game.id &&
        r.modId > 0 &&
        r.modId !== mod.modId &&
        !nexusStore.installed(game.id, r.modId) &&
        all.findIndex((x) => x.modId === r.modId) === i,
    ),
  );
  /** Mods nécessaires décochés par l'utilisateur. */
  const skipDeps = new SvelteSet<number>();
  /** Mod nécessaire en cours d'installation. */
  let currentDep = $state<{ modId: number; name: string; index: number; total: number } | null>(null);
  const depKey = $derived(currentDep ? nexusStore.key(game.id, currentDep.modId) : null);
  const depProgress = $derived(depKey ? (nexusStore.progress[depKey] ?? null) : null);
  const depWaiting = $derived(depKey ? (nexusStore.waiting[depKey] ?? null) : null);
  /** Mods nécessaires installés avec celui-ci et devenus inutiles après sa désinstallation. */
  let orphans = $state<{ modId: number; name: string }[]>([]);

  const busy = $derived(progress !== null || waiting !== null || currentDep !== null);
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
  /** L'outil a été installé depuis ce panneau : son message de réussite reste affiché. */
  let toolInstalledHere = $state(false);
  $effect(() => {
    const t = neededTool ?? requiredTool;
    if (t && !shownTool) shownTool = t;
  });
  // Outil installé ailleurs entre-temps (script extender installé comme un mod…) : plus rien à proposer.
  $effect(() => {
    if (shownTool && !toolInstalledHere && gameTool?.id === shownTool.id && gameTool.present) {
      shownTool = null;
      neededTool = null;
    }
  });
  $effect(() => {
    const id = game.id;
    const domain = game.domainName;
    if (domain && !nexusStore.setups[id]) nexusStore.loadSetup(id, domain).catch(() => {});
  });
  const toolFolder = $derived(config?.rules === "bonelab" ? gameDir : (config?.folder ?? null));

  /** Chargeurs de mods déjà installés dans le jeu (`bepinex`, `melonloader`). */
  const gameLoaders = $derived(nexusStore.setups[game.id]?.engine?.tools ?? []);

  /** Fichier conseillé : le plus récent des fichiers principaux (version PC), pour le chargeur du jeu. */
  const recommended = $derived(files ? recommendedFile(files, gameLoaders) : null);
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

  // ------------------------------------------------------------ Installateurs FOMOD
  /** Options à choisir pour terminer une installation. */
  let fomod = $state<{
    token: string;
    installer: FomodInstaller;
    modId: number;
    name: string;
    resolve: (r: NexusInstalled | null) => void;
  } | null>(null);

  /** Résultat d'une installation : affiche les options FOMOD s'il y en a à choisir. */
  function settle(outcome: NexusInstallOutcome, modId: number, name: string): Promise<NexusInstalled | null> {
    if (outcome.status === "installed") return Promise.resolve(outcome);
    return new Promise((resolve) => {
      fomod = { token: outcome.token, installer: outcome.installer, modId, name, resolve };
    });
  }

  async function finishFomod(selections: number[][][]): Promise<string | null> {
    const f = fomod;
    if (!f) return null;
    try {
      const r = await nexusStore.finishFomod(game.id, f.modId, f.token, selections);
      if (r.status === "choices") {
        // L'archive choisie a ses propres options : l'assistant continue.
        fomod = { ...f, token: r.token, installer: r.installer };
        return null;
      }
      fomod = null;
      f.resolve(r);
      return null;
    } catch (e) {
      const err = asAppError(e);
      // Choix incomplet : on reste dans l'installateur.
      if (err.kind === "invalid_argument") return err.message;
      fomod = null;
      error = err;
      f.resolve(null);
      return null;
    }
  }

  function cancelFomod() {
    const f = fomod;
    if (!f) return;
    nexusStore.cancelFomod(f.token);
    fomod = null;
    f.resolve(null);
  }

  /**
   * Installe les mods nécessaires cochés, l'un après l'autre (chacun passe par
   * la page Nexus pour un compte gratuit). Renvoie leurs noms, ou `null` si
   * l'un d'eux n'a pas pu être installé.
   */
  async function runDeps(): Promise<string[] | null> {
    const deps = missingDeps.filter((r) => !skipDeps.has(r.modId));
    const done: string[] = [];
    for (const [i, r] of deps.entries()) {
      currentDep = { modId: r.modId, name: r.modName, index: i + 1, total: deps.length };
      try {
        const file = recommendedFile(await nexus.listFiles(game.id, r.modId), gameLoaders);
        if (!file) throw { kind: "nexus_not_found", message: "aucun fichier à télécharger.", retryAfter: null };
        const outcome = await nexusStore.request(game.id, game.domainName, r.modId, file.fileId, file.name, mod.modId);
        const res = await settle(outcome, r.modId, r.modName);
        if (!res) {
          currentDep = null;
          if (!error) note = `Installation annulée (mod nécessaire « ${r.modName} »).`;
          return null;
        }
        done.push(res.record.name);
        if (res.needsTool && !neededTool) neededTool = res.needsTool;
      } catch (e) {
        const err = asAppError(e);
        currentDep = null;
        if (err.kind === "cancelled") note = `Installation annulée (mod nécessaire « ${r.modName} »).`;
        else error = { ...err, message: `Mod nécessaire « ${r.modName} » : ${err.message}` };
        return null;
      }
    }
    currentDep = null;
    return done;
  }

  /** Installe seulement les mods nécessaires manquants (mod déjà installé). */
  async function installDeps() {
    error = null;
    note = null;
    const done = await runDeps();
    if (done?.length) note = `Mods nécessaires installés : ${done.join(", ")}.`;
  }

  async function run(file: NexusFile) {
    const updating = record !== null;
    orphans = [];
    // Les mods nécessaires d'abord (pas lors d'une mise à jour).
    const deps = updating ? [] : await runDeps();
    if (deps === null) return;
    try {
      const outcome = await nexusStore.request(game.id, game.domainName, mod.modId, file.fileId, file.name);
      const r = await settle(outcome, mod.modId, mod.name);
      if (!r) {
        if (!error) note = "Installation annulée.";
        return;
      }
      note = [
        updating ? "Mis à jour." : "Installé.",
        deps.length ? `Mods nécessaires installés avant lui : ${deps.join(", ")}.` : null,
        r.notice,
      ]
        .filter(Boolean)
        .join(" ");
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
    if (currentDep) {
      if (depWaiting) nexusStore.stopWaiting(game.id, game.domainName, currentDep.modId);
      else nexusStore.cancel(currentDep.modId);
    } else if (waiting) nexusStore.stopWaiting(game.id, game.domainName, mod.modId);
    else nexusStore.cancel(mod.modId);
  }

  function reopen() {
    if (currentDep && depWaiting) nexusStore.reopen(game.id, depWaiting.domain, currentDep.modId, currentDep.name);
    else if (waiting) nexusStore.reopen(game.id, waiting.domain, mod.modId, waitingFile?.name ?? null);
  }

  async function removeOrphans() {
    const list = orphans;
    orphans = [];
    const failed: string[] = [];
    for (const o of list) {
      try {
        await nexusStore.uninstall(game.id, o.modId);
      } catch {
        failed.push(o.name);
      }
    }
    note = failed.length
      ? `Impossible de désinstaller ${failed.map((n) => `« ${n} »`).join(", ")} : ferme le jeu puis réessaie depuis « Installés ».`
      : `${list.map((o) => `« ${o.name} »`).join(", ")} ${list.length > 1 ? "désinstallés" : "désinstallé"} aussi.`;
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
      const r = await nexusStore.uninstall(game.id, mod.modId);
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

  function reveal() {
    nexus.reveal(game.id, mod.modId).catch((e) => (error = asAppError(e)));
  }

  let toggling = $state(false);

  async function setEnabled(enabled: boolean) {
    toggling = true;
    error = null;
    note = null;
    try {
      await nexusStore.setEnabled(game.id, mod.modId, enabled);
      note = enabled ? "Réactivé." : "Désactivé : ses fichiers sont mis de côté, le jeu ne le charge plus.";
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
  {:else if currentDep}
    <div class="dep-run" role="status">
      <p class="small">
        <span class="strong">Mod nécessaire {currentDep.index}/{currentDep.total} : {currentDep.name}</span>
        {#if !depWaiting && !depProgress}<span class="muted">, préparation…</span>{/if}
      </p>
      {#if depWaiting}
        <p class="small">Clique sur « Slow download » sur la page Nexus Mods : l'installation démarrera toute seule.</p>
        <div class="row">
          <button class="btn" onclick={reopen}>Rouvrir la page</button>
          <button class="btn btn-ghost" onclick={cancel}>Annuler</button>
        </div>
      {:else if depProgress}
        <ProgressBar progress={depProgress} oncancel={cancel} />
      {/if}
      <p class="small muted">« {mod.name} » s'installera juste après.</p>
    </div>
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
      {#if record.disabled}<span class="chip">Désactivé</span>{:else}<span class="chip chip-ok">Installé</span>{/if}
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
      <button class="btn" onclick={() => setEnabled(!!record?.disabled)} disabled={toggling || removing}>
        {toggling ? "…" : record.disabled ? "Réactiver" : "Désactiver"}
      </button>
      <button class="btn" onclick={reveal} disabled={record.disabled}>Ouvrir le dossier</button>
      <button class="btn" class:danger={confirming} onclick={uninstall} disabled={removing}>
        {removing ? "Désinstallation…" : confirming ? "Confirmer" : "Désinstaller"}
      </button>
    </div>
    {#if missingDeps.length}
      <div class="deps">
        <p class="small">
          <span class="strong">Mods nécessaires manquants :</span>
          {missingDeps.map((r) => r.modName).join(", ")}.
        </p>
        <button class="btn" onclick={installDeps} disabled={busy}>
          <Icon name="download" size={15} />{missingDeps.length > 1 ? "Les installer" : "L'installer"}
        </button>
      </div>
    {/if}
  {:else if files === null}
    <p class="small muted">Chargement des fichiers…</p>
  {:else if !recommended}
    <p class="small muted">Aucun fichier à télécharger pour ce mod.</p>
  {:else}
    {#if missingDeps.length && nexusStore.ready}
      <div class="deps">
        <p class="small strong">Ce mod a besoin de :</p>
        {#each missingDeps as r (r.modId)}
          <label class="dep">
            <input
              type="checkbox"
              checked={!skipDeps.has(r.modId)}
              onchange={(e) => (e.currentTarget.checked ? skipDeps.delete(r.modId) : skipDeps.add(r.modId))}
            />
            <span>
              <span>{r.modName}</span>
              {#if r.notes}<span class="muted small note-dep">{r.notes}</span>{/if}
            </span>
          </label>
        {/each}
        <p class="small muted">
          ModStudio les installe d'abord{nexusStore.premium ? "" : ", une page Nexus par fichier comme pour ce mod"}.
        </p>
      </div>
    {/if}
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

  {#if fomod}
    {#key fomod.token}
      <FomodWizard installer={fomod.installer} modName={fomod.name} onfinish={finishFomod} oncancel={cancelFomod} />
    {/key}
  {/if}

  {#if orphans.length}
    <div class="deps">
      <p class="small">
        {orphans.map((o) => `« ${o.name} »`).join(", ")}
        {orphans.length > 1 ? "avaient été installés" : "avait été installé"} avec ce mod et ne
        {orphans.length > 1 ? "servent" : "sert"} plus à aucun autre.
      </p>
      <div class="row">
        <button class="btn btn-primary" onclick={removeOrphans}>
          {orphans.length > 1 ? "Les désinstaller aussi" : "Le désinstaller aussi"}
        </button>
        <button class="btn btn-ghost" onclick={() => (orphans = [])}>Garder</button>
      </div>
    </div>
  {/if}

  {#if shownTool && config && !setupOpen}
    <div class="needs">
      <NexusToolPrompt
        {game}
        tool={shownTool}
        folder={toolFolder}
        onstart={() => (toolInstalledHere = true)}
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

  .deps,
  .dep-run {
    display: grid;
    gap: 0.5rem;
    justify-items: start;
    padding: 0.8rem 0.9rem;
    border-radius: var(--r-md);
    background: var(--raise);
  }

  .dep-run {
    justify-items: stretch;
    border-left: 3px solid var(--accent-fill);
  }

  .dep {
    display: flex;
    align-items: flex-start;
    gap: 0.5rem;
    font-size: var(--t-sm);
    cursor: pointer;
  }

  .dep input {
    margin-top: 0.2rem;
    accent-color: var(--accent-fill);
  }

  .dep > span {
    display: grid;
  }

  .note-dep {
    font-size: var(--t-xs);
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
