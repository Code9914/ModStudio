<script lang="ts">
  // Le jeu sur ce PC : son dossier, et son chargeur de mods.
  import { open } from "@tauri-apps/plugin-dialog";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { asAppError, cl, type AppError, type ClGame } from "$lib/api";
  import { listsStore } from "$lib/lists.svelte";
  import { launcherNames } from "$lib/nexus.svelte";
  import ErrorNotice from "./ErrorNotice.svelte";
  import Icon from "./Icon.svelte";
  import ProgressBar from "./ProgressBar.svelte";

  let { game }: { game: ClGame } = $props();

  let busy = $state(false);
  let error = $state<AppError | null>(null);
  let copied = $state(false);
  let launched = $state(false);

  const config = $derived(listsStore.config(game));
  const detected = $derived(listsStore.detectedFor(game));
  const hasMods = $derived(config ? listsStore.count(config) > 0 : false);
  const loader = $derived(listsStore.loaders[game.id] ?? null);
  const job = $derived(listsStore.progress[game.gameId] ?? null);
  const loaderMod = $derived(loader?.id ? listsStore.mod(game, loader.id) : null);
  const installingLoader = $derived(!!job && !!loaderMod && job.modId === loaderMod.modId);

  $effect(() => {
    if (config) listsStore.refreshLoader(game);
  });

  async function use(folder: string) {
    busy = true;
    error = null;
    try {
      await listsStore.configure(game, folder);
    } catch (e) {
      error = asAppError(e);
    } finally {
      busy = false;
    }
  }

  async function pick() {
    error = null;
    const picked = await open({
      directory: true,
      multiple: false,
      title: `Dossier d'installation de ${game.name}`,
      defaultPath: config?.folder || detected?.path || undefined,
    });
    if (typeof picked === "string") await use(picked);
  }

  async function installLoader() {
    if (!loaderMod) return;
    error = null;
    try {
      await listsStore.install(game, loaderMod);
    } catch (e) {
      const err = asAppError(e);
      if (err.kind !== "cancelled") error = err;
    }
  }

  async function launch() {
    error = null;
    try {
      await cl.launch(game.id);
      launched = true;
      setTimeout(() => (launched = false), 4000);
      setTimeout(() => listsStore.refreshLoader(game), 15000);
    } catch (e) {
      error = asAppError(e);
    }
  }

  async function copy(text: string) {
    try {
      await navigator.clipboard.writeText(text);
      copied = true;
      setTimeout(() => (copied = false), 2000);
    } catch {
      /* presse-papiers indisponible : le texte reste sélectionnable */
    }
  }
</script>

<section class="setup" aria-label="{game.name} sur ce PC">
  <div class="where">
    <div class="where-text">
      {#if config}
        <p class="label">Dossier du jeu</p>
        <p class="path" title={config.folder}>{config.folder}</p>
      {:else if detected}
        <p class="label">Trouvé dans {launcherNames[detected.launcher] ?? detected.launcher}</p>
        <p class="path" title={detected.path}>{detected.path}</p>
      {:else}
        <p class="label">Dossier du jeu</p>
        <p class="small">
          Choisis le dossier où {game.name} est installé (celui qui contient son exécutable) : les mods y sont rangés là où
          {game.loader} les lit.
        </p>
      {/if}
    </div>
    <div class="row">
      {#if !config && detected}
        <button class="btn btn-primary" onclick={() => use(detected.path)} disabled={busy}>Installer les mods dans ce dossier</button>
        <button class="btn" onclick={pick} disabled={busy}>Choisir un autre dossier…</button>
      {:else if !config}
        <button class="btn btn-primary" onclick={pick} disabled={busy}>Choisir le dossier du jeu</button>
      {:else}
        <button class="btn" onclick={() => cl.reveal(game.gameId).catch((e) => (error = asAppError(e)))}>Ouvrir</button>
        {#if !hasMods}<button class="btn btn-ghost" onclick={pick} disabled={busy}>Changer</button>{/if}
      {/if}
    </div>
  </div>

  {#if config && loader}
    <div class="loader" class:ready={loader.present && !loader.note} class:warn={!!loader.note}>
      {#if loader.present}
        <p class="small ok">
          <Icon name="check" size={15} />
          <span>
            <span class="strong">{loader.name}</span>{loader.version ? ` ${loader.version}` : ""} est installé{loader.managed
              ? ""
              : " (sans ModStudio)"}{loader.note ? "." : " : les mods se chargeront au lancement du jeu."}
          </span>
        </p>
      {:else if loader.installable}
        <p class="small">
          {game.name} charge ses mods avec <span class="strong">{loader.name}</span>. Il n'est pas encore installé :
          ModStudio l'installera avec le premier mod.
        </p>
        {#if installingLoader && job}
          <ProgressBar progress={job} oncancel={() => listsStore.cancel(game)} />
        {:else if loaderMod}
          <div><button class="btn" onclick={installLoader} disabled={!!job}>Installer {loader.name} maintenant</button></div>
        {/if}
      {:else}
        <p class="small">
          {game.name} charge ses mods avec <span class="strong">{loader.name}</span>, qui s'installe avec son propre
          programme : ModStudio installe les mods, pas le chargeur.
        </p>
        {#if game.id === "raft"}
          <div>
            <button class="btn" onclick={() => openUrl("https://www.raftmodding.com/loader")}>
              Télécharger Raft Mod Loader <Icon name="external" size={14} />
            </button>
          </div>
        {/if}
      {/if}

      {#if loader.note}
        <p class="small note"><Icon name="warning" size={15} /><span>{loader.note}</span></p>
      {/if}

      {#if game.id === "outer-wilds" && loader.present}
        <div class="row">
          <button class="btn" onclick={launch}>{launched ? "OWML se lance…" : "Lancer avec OWML"}</button>
        </div>
      {/if}

      {#if loader.launchOption && (loader.present || loader.installable) && (!detected || detected.launcher === "steam")}
        <div class="proton">
          <p class="small">
            Sous Linux, ajoute ceci aux options de lancement du jeu dans Steam (Propriétés, Général) : sans ça, Proton ne
            charge pas {loader.name}.
          </p>
          <div class="proton-row">
            <code>{loader.launchOption}</code>
            <button class="btn btn-ghost" onclick={() => copy(loader.launchOption ?? "")}>{copied ? "Copié" : "Copier"}</button>
          </div>
        </div>
      {/if}
    </div>
  {/if}

  {#if error}<ErrorNotice {error} />{/if}
</section>

<style>
  .setup {
    display: grid;
    gap: 0.9rem;
    padding: 1rem 1.15rem;
    margin-bottom: 1.75rem;
    border-radius: var(--r-md);
    background: var(--panel);
    max-width: 60rem;
    min-width: 0;
  }

  .where {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 0.75rem 1.5rem;
  }

  .where-text {
    display: grid;
    gap: 0.2rem;
    min-width: 0;
    flex: 1 1 22rem;
  }

  .label {
    font-size: var(--t-xs);
    font-weight: 700;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--muted);
  }

  .path {
    font-size: var(--t-sm);
    overflow-wrap: anywhere;
  }

  .row {
    display: flex;
    flex-wrap: wrap;
    gap: 0.4rem;
  }

  .small {
    font-size: var(--t-sm);
  }

  .strong {
    font-weight: 600;
  }

  .loader {
    display: grid;
    gap: 0.6rem;
    padding: 0.8rem 0.9rem;
    border-radius: var(--r-md);
    background: var(--raise);
    border-left: 3px solid var(--accent-fill);
  }

  .loader.ready {
    border-left-color: var(--ok);
  }

  .loader.warn {
    border-left-color: var(--warn, var(--accent));
  }

  .ok,
  .note {
    display: flex;
    align-items: flex-start;
    gap: 0.45rem;
  }

  .ok :global(svg) {
    flex: none;
    color: var(--ok);
    margin-top: 0.15rem;
  }

  .note :global(svg) {
    flex: none;
    color: var(--warn, var(--accent));
    margin-top: 0.15rem;
  }

  .proton {
    display: grid;
    gap: 0.4rem;
  }

  .proton-row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.4rem;
  }

  .proton code {
    padding: 0.2em 0.5em;
    border-radius: var(--r-sm);
    background: var(--ink);
    font-size: 0.9em;
    overflow-wrap: anywhere;
    user-select: all;
  }
</style>
