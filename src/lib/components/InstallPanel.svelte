<script lang="ts">
  import { api, asAppError, pcFileId, type AppError, type Dependency, type Game, type Mod, type Modfile } from "$lib/api";
  import { bytes } from "$lib/format";
  import { tr } from "$lib/i18n.svelte";
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
      note = tr(
        `Abonné. ${game.name ?? "Le jeu"} le téléchargera à son prochain lancement.`,
        `Subscribed. ${game.name ?? "The game"} will download it the next time it starts.`,
      );
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
      if (err.kind === "cancelled") note = tr("Installation annulée.", "Installation cancelled.");
      else if (err.kind === "no_game_folder") setupOpen = true;
      else error = err;
      return;
    }
    let failed = 0;
    for (const [i, d] of extra.entries()) {
      depId = d.mod_id;
      const depName = d.name ?? `mod ${d.mod_id}`;
      depLabel = tr(`Mod nécessaire ${i + 1} sur ${extra.length} : ${depName}`, `Required mod ${i + 1} of ${extra.length}: ${depName}`);
      try {
        await installs.install(game.id, d.mod_id, mod.id);
      } catch {
        failed++;
      }
    }
    depId = null;
    depLabel = null;
    const parts = [updating ? tr("Mis à jour.", "Updated.") : tr("Installé.", "Installed.")];
    const added = extra.length - failed;
    if (added)
      parts.push(
        added > 1
          ? tr(`${added} mods nécessaires installés aussi.`, `${added} required mods were also installed.`)
          : tr("Le mod nécessaire a été installé aussi.", "The required mod was also installed."),
      );
    if (failed)
      parts.push(
        failed > 1
          ? tr(`${failed} mods nécessaires n'ont pas pu être installés.`, `${failed} required mods couldn't be installed.`)
          : tr("Un mod nécessaire n'a pas pu être installé.", "One required mod couldn't be installed."),
      );
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
        ? tr(
            `Désinstallé. Le dossier « ${r.leftFolder} » contient des fichiers ajoutés depuis, il a été laissé en place.`,
            `Uninstalled. The "${r.leftFolder}" folder contains files added since then, so it was left in place.`,
          )
        : tr("Désinstallé.", "Uninstalled.");
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
      ? tr(
          `Impossible de désinstaller : ${failed.join(", ")}. Ferme le jeu puis réessaie depuis la page Installés.`,
          `Couldn't uninstall: ${failed.join(", ")}. Close the game, then try again from the Installed page.`,
        )
      : tr("Désinstallé, avec les mods nécessaires qui ne servaient plus.", "Uninstalled, along with the required mods that were no longer needed.");
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

<section class="panel" aria-label={tr("Installation sur ce PC", "Installation on this PC")}>
  <h2>{tr("Sur ce PC", "On this PC")}</h2>

  {#if setupOpen}
    <FolderSetup
      gameId={game.id}
      gameName={game.name ?? tr(`Jeu ${game.id}`, `Game ${game.id}`)}
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
      {#if record.disabled}<span class="chip">{tr("Désactivé", "Disabled")}</span>{:else}<span class="chip chip-ok">{tr("Installé", "Installed")}</span>{/if}
      <span class="small muted">
        {record.version ? `version ${record.version}` : tr("version sans numéro", "unnumbered version")}, {bytes(record.size)}
      </span>
    </p>
    {#if record.disabled}
      <p class="small muted">{tr("Ses fichiers sont mis de côté : le jeu ne le charge plus.", "Its files are set aside: the game no longer loads it.")}</p>
    {/if}
    {#if record.auto && parents.length}
      <p class="small muted">
        {tr(`Installé comme mod nécessaire de « ${parents.join(" », « ")} ».`, `Installed as a required mod for "${parents.join('", "')}".`)}
      </p>
    {/if}
    {#if outdated}
      <p class="small">
        {tr(
          `Une nouvelle version est disponible${fileVersion ? ` (${fileVersion})` : ""}.`,
          `A new version is available${fileVersion ? ` (${fileVersion})` : ""}.`,
        )}
      </p>
      <button class="btn btn-primary" onclick={start} disabled={busy}>
        <Icon name="download" size={15} />{tr("Mettre à jour", "Update")}
      </button>
    {/if}
    {#if config}
      <p class="small muted folder" title={config.folder}>{tr("Dans", "In")} {config.folder}</p>
    {/if}
    <div class="row">
      <button class="btn" onclick={() => setEnabled(!!record?.disabled)} disabled={toggling || removing}>
        {toggling ? "…" : record.disabled ? tr("Réactiver", "Re-enable") : tr("Désactiver", "Disable")}
      </button>
      <button class="btn" onclick={() => reveal(true)} disabled={record.disabled}>{tr("Ouvrir le dossier", "Open folder")}</button>
      <button class="btn" class:danger={confirming} onclick={uninstall} disabled={removing}>
        {removing ? tr("Désinstallation…", "Uninstalling…") : confirming ? tr("Confirmer", "Confirm") : tr("Désinstaller", "Uninstall")}
      </button>
    </div>
  {:else if managed && !showManual}
    <div class="managed">
      <p class="small">
        <span class="strong">{tr(`${game.name} installe ses mods lui-même grâce à mod.io.`, `${game.name} installs its own mods through mod.io.`)}</span>
        {tr("Abonne-toi : le jeu téléchargera ce mod à son prochain lancement.", "Subscribe and the game will download this mod the next time it starts.")}
      </p>
      <p class="small muted folder" title={managed.folder}>
        {tr("Dossier du jeu :", "Game folder:")} {managed.folder}{managed.mods > 0
          ? `, ${managed.mods} ${tr(
              managed.mods > 1 ? "mods déjà téléchargés" : "mod déjà téléchargé",
              managed.mods === 1 ? "mod already downloaded" : "mods already downloaded",
            )}`
          : ""}
      </p>
      {#if subscribed}
        <span class="chip chip-ok">{tr("Abonné", "Subscribed")}</span>
      {:else}
        <button class="btn btn-primary wide" onclick={subscribe} disabled={subBusy}>
          {#if !nav.loggedIn}{tr("Se connecter pour s'abonner", "Sign in to subscribe")}{:else if subBusy}{tr("Abonnement…", "Subscribing…")}{:else}<Icon
              name="plus"
              size={15}
            />{tr("S'abonner", "Subscribe")}{/if}
        </button>
      {/if}
    </div>
    <button class="link small manual" onclick={() => (showManual = true)}>{tr("Installer quand même à la main", "Install manually anyway")}</button>
  {:else}
    {#if !fileId}
      <p class="small muted">
        {tr(
          "Ce mod n'a pas de fichier pour PC (Windows) : il est peut-être réservé à une autre plateforme.",
          "This mod has no file for PC (Windows): it may be meant for another platform.",
        )}
      </p>
    {:else}
      <button class="btn btn-primary wide" onclick={start} disabled={busy}>
        <Icon name="download" size={15} />{tr(
          `Installer${fileVersion ? ` la version ${fileVersion}` : ""}`,
          `Install${fileVersion ? ` version ${fileVersion}` : ""}`,
        )}
      </button>
    {/if}
    {#if fileId && missingDeps.length > 0}
      <label class="check">
        <input type="checkbox" bind:checked={withDeps} />
        {missingDeps.length > 1
          ? tr(`Installer aussi les ${missingDeps.length} mods nécessaires`, `Also install the ${missingDeps.length} required mods`)
          : tr("Installer aussi le mod nécessaire", "Also install the required mod")}
      </label>
    {/if}
    {#if config}
      <p class="small muted folder">
        {tr("Dans", "In")} <span title={config.folder}>{config.folder}</span>
        <button class="link" onclick={() => (setupOpen = true)}>{tr("Changer", "Change")}</button>
      </p>
    {:else}
      <p class="small muted">{tr(`Tu choisiras ensuite le dossier des mods de ${game.name}.`, `Next, you'll choose the mods folder for ${game.name}.`)}</p>
    {/if}
  {/if}

  {#if error}<ErrorNotice {error} onretry={error.kind === "conflict" ? undefined : start} />{/if}
  {#if note}<p class="note small" role="status">{note}</p>{/if}
  {#if orphans.length}
    <div class="orphans">
      <p class="small">
        {orphans.length > 1
          ? tr(
              `${orphans.map((o) => `« ${o.name} »`).join(", ")} avaient été installés avec ce mod et ne servent plus à aucun autre.`,
              `${orphans.map((o) => `"${o.name}"`).join(", ")} were installed with this mod and are no longer needed by any other.`,
            )
          : tr(
              `« ${orphans[0].name} » avait été installé avec ce mod et ne sert plus à aucun autre.`,
              `"${orphans[0].name}" was installed with this mod and is no longer needed by any other.`,
            )}
        {orphans.length > 1 ? tr("Les désinstaller aussi ?", "Uninstall them too?") : tr("Le désinstaller aussi ?", "Uninstall it too?")}
      </p>
      <div class="row">
        <button class="btn btn-primary" onclick={removeOrphans} disabled={removingOrphans}>
          {removingOrphans ? tr("Désinstallation…", "Uninstalling…") : tr("Désinstaller aussi", "Uninstall too")}
        </button>
        <button class="btn btn-ghost" onclick={() => (orphans = [])}>{tr("Garder", "Keep")}</button>
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
