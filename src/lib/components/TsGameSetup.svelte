<script lang="ts">
  // Le jeu sur ce PC pour Thunderstore : son dossier (là où est l'exécutable),
  // et le chargeur de mods présent.
  import { open } from "@tauri-apps/plugin-dialog";
  import { asAppError, ts, type AppError, type TsCommunity } from "$lib/api";
  import { launcherNames, nexusStore } from "$lib/nexus.svelte";
  import { tsStore } from "$lib/thunderstore.svelte";
  import ErrorNotice from "./ErrorNotice.svelte";
  import Icon from "./Icon.svelte";
  import ProgressBar from "./ProgressBar.svelte";

  let { game }: { game: TsCommunity } = $props();

  let busy = $state(false);
  let error = $state<AppError | null>(null);
  let copied = $state(false);

  const config = $derived(tsStore.game(game.id));
  const detected = $derived(tsStore.detectedFor(game));
  const hasMods = $derived(config ? tsStore.count(config) > 0 : false);
  const loader = $derived(tsStore.loaders[game.id] ?? null);
  const job = $derived(tsStore.progress[game.id] ?? null);
  const fixing = $derived(!!job && !!loader?.fix && job.modId === loader.fix.id);

  /** Remplace le BepInEx qui ne convient pas par le bon. */
  async function fixLoader() {
    const fix = loader?.fix;
    if (!fix) return;
    error = null;
    try {
      await tsStore.install(game, fix.namespace, fix.name, fix.id);
    } catch (e) {
      const err = asAppError(e);
      if (err.kind !== "cancelled") error = err;
    }
  }

  $effect(() => {
    const id = game.id;
    if (config) tsStore.refreshLoader(id);
  });

  async function use(folder: string) {
    busy = true;
    error = null;
    try {
      await tsStore.configure(game, folder);
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

  async function copyLaunchOption() {
    if (!loader?.launchOption) return;
    try {
      await navigator.clipboard.writeText(loader.launchOption);
      copied = true;
      setTimeout(() => (copied = false), 2000);
    } catch {
      /* presse-papiers indisponible : le texte reste sélectionnable */
    }
  }

  function reveal() {
    ts.reveal(game.id).catch((e) => (error = asAppError(e)));
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
          Choisis le dossier où {game.name} est installé (celui qui contient son exécutable) : les mods y sont rangés
          dans BepInEx, comme le ferait un gestionnaire de mods.
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
        <button class="btn" onclick={reveal}>Ouvrir</button>
        {#if !hasMods}<button class="btn btn-ghost" onclick={pick} disabled={busy}>Changer</button>{/if}
      {/if}
    </div>
  </div>

  {#if config && loader}
    <div class="loader" class:ready={!!loader.name && loader.suitable} class:wrong={!!loader.name && !loader.suitable}>
      {#if loader.name && !loader.suitable}
        <p class="small bad">
          <Icon name="warning" size={15} />
          <span>
            <span class="strong">{loader.name}</span> est installé, mais {game.name} est un jeu Unity
            {loader.engine === "il2cpp" ? "IL2CPP" : "Mono"} : il lui faut
            {loader.engine === "il2cpp" ? "BepInEx 6 (IL2CPP)" : "BepInEx pour Mono"}. Les mods ne se chargent pas.
          </span>
        </p>
        {#if fixing && job}
          <ProgressBar progress={job} oncancel={() => tsStore.cancel(game)} />
        {:else if loader.fix && loader.managed}
          <div>
            <button class="btn btn-primary" onclick={fixLoader} disabled={!!job}>
              Installer {loader.engine === "il2cpp" ? "BepInEx 6 (IL2CPP)" : "le bon BepInEx"} à la place
            </button>
          </div>
        {:else if !loader.managed}
          <p class="small muted">
            Il a été installé sans ModStudio : retire-le (dossier BepInEx\core, winhttp.dll, doorstop_config.ini), puis
            réinstalle un mod.
          </p>
        {/if}
      {:else if loader.name}
        <p class="small ok">
          <Icon name="check" size={15} />
          <span><span class="strong">{loader.name}</span> est installé : les mods se chargeront au lancement du jeu.</span>
        </p>
        {#if loader.engine === "il2cpp"}
          <p class="small muted">
            Le premier lancement après son installation est plus long : BepInEx prépare le jeu (une fenêtre de console
            s'ouvre), puis les suivants sont normaux.
          </p>
        {/if}
      {:else}
        <p class="small">
          BepInEx n'est pas encore installé : il le sera avec le premier mod qui en a besoin (presque tous).
        </p>
      {/if}
      {#if loader.launchOption && (!detected || detected.launcher === "steam")}
        <div class="proton">
          <p class="small">
            Sous Linux, ajoute ceci aux options de lancement du jeu dans Steam (Propriétés, Général) : sans ça, Proton ne
            charge pas {loader.name ?? "BepInEx"}.
          </p>
          <div class="proton-row">
            <code>{loader.launchOption}</code>
            <button class="btn btn-ghost" onclick={copyLaunchOption}>{copied ? "Copié" : "Copier"}</button>
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

  .loader.wrong {
    border-left-color: var(--danger);
  }

  .bad {
    display: flex;
    align-items: flex-start;
    gap: 0.45rem;
  }

  .bad :global(svg) {
    flex: none;
    color: var(--danger);
    margin-top: 0.15rem;
  }

  .loader.ready {
    border-left-color: var(--ok);
  }

  .ok {
    display: flex;
    align-items: flex-start;
    gap: 0.45rem;
  }

  .ok :global(svg) {
    flex: none;
    color: var(--ok);
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
