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
  const installingLoader = $derived(!!job && !!loader?.needed && job.modId === loader.needed.id);
  /** Thunderstore indique un chargeur que ModStudio ne sait pas installer (ou rien). */
  const unsupported = $derived(loader ? !loader.supported : !game.supported);
  const requiredLabel = $derived(loader?.requiredLabel ?? game.loader);
  /** Un mauvais BepInEx installé à la main n'est jamais retiré par ModStudio. */
  const canInstallLoader = $derived(!!loader?.needed && (!loader.name || loader.managed || loader.required !== "bepinex"));

  /** Installe le chargeur du jeu (ou remplace celui qui ne convient pas). */
  async function installLoader() {
    const fix = loader?.needed;
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

  let copiedArgs = $state(false);
  async function copy(text: string | null | undefined, done: (on: boolean) => void) {
    if (!text) return;
    try {
      await navigator.clipboard.writeText(text);
      done(true);
      setTimeout(() => done(false), 2000);
    } catch {
      /* presse-papiers indisponible : le texte reste sélectionnable */
    }
  }

  function reveal() {
    ts.reveal(game.id).catch((e) => (error = asAppError(e)));
  }
</script>

<section class="setup" aria-label="{game.name} sur ce PC">
  {#if unsupported}
    <p class="small unsupported">
      <Icon name="warning" size={15} />
      <span>
        {#if loader?.required === "mio" || requiredLabel === "Mio"}
          Le chargeur de mods de {game.name} n'est pas publié sur Thunderstore : ModStudio ne peut pas l'installer, ni
          ses mods. Pour ce jeu, utilise un gestionnaire comme r2modman ou Gale.
        {:else if requiredLabel}
          {game.name} charge ses mods avec <span class="strong">{requiredLabel}</span> : ModStudio ne sait pas encore
          les installer. Pour ce jeu, utilise un gestionnaire comme r2modman ou Gale.
        {:else}
          Thunderstore ne publie pas de règles d'installation pour {game.name} : ModStudio ne peut pas encore installer
          ses mods. Chacun explique son installation sur sa page.
        {/if}
      </span>
    </p>
  {:else}
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
          comme le ferait un gestionnaire de mods{requiredLabel ? `, avec ${requiredLabel}` : ""}.
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
            <span class="strong">{loader.name}</span> est installé, mais {game.name} a besoin de
            <span class="strong">{loader.requiredLabel}</span>{loader.engine === "il2cpp" && loader.required === "bepinex"
              ? " (c'est un jeu Unity IL2CPP)"
              : ""}. Les mods ne se chargent pas.
          </span>
        </p>
        {#if installingLoader && job}
          <ProgressBar progress={job} oncancel={() => tsStore.cancel(game)} />
        {:else if canInstallLoader}
          <div>
            <button class="btn btn-primary" onclick={installLoader} disabled={!!job}>
              Installer {loader.requiredLabel}{loader.required === "bepinex" ? " à la place" : ""}
            </button>
          </div>
        {:else}
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
          {game.name} charge ses mods avec <span class="strong">{loader.requiredLabel ?? "BepInEx"}</span>. Il n'est pas
          encore installé : ModStudio l'installera avec le premier mod.
        </p>
        {#if installingLoader && job}
          <ProgressBar progress={job} oncancel={() => tsStore.cancel(game)} />
        {:else if loader.needed}
          <div>
            <button class="btn" onclick={installLoader} disabled={!!job}>Installer {loader.requiredLabel} maintenant</button>
          </div>
        {/if}
      {/if}
      {#if loader.launchOption && (!detected || detected.launcher === "steam")}
        <div class="proton">
          <p class="small">
            Sous Linux, ajoute ceci aux options de lancement du jeu dans Steam (Propriétés, Général) : sans ça, Proton ne
            charge pas {loader.name ?? "BepInEx"}.
          </p>
          <div class="proton-row">
            <code>{loader.launchOption}</code>
            <button class="btn btn-ghost" onclick={() => copy(loader.launchOption, (on) => (copied = on))}>
              {copied ? "Copié" : "Copier"}
            </button>
          </div>
        </div>
      {/if}
      {#if loader.launchArgs && loader.suitable}
        <div class="proton">
          <p class="small">
            Ajoute ceci aux options de lancement du jeu (dans Steam : Propriétés, Général) : c'est ce qui démarre
            {loader.name} et charge les mods.
          </p>
          <div class="proton-row">
            <code>{loader.launchArgs}</code>
            <button class="btn btn-ghost" onclick={() => copy(loader.launchArgs, (on) => (copiedArgs = on))}>
              {copiedArgs ? "Copié" : "Copier"}
            </button>
          </div>
        </div>
      {/if}
    </div>
  {/if}

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

  .unsupported {
    display: flex;
    align-items: flex-start;
    gap: 0.45rem;
  }

  .unsupported :global(svg) {
    flex: none;
    color: var(--accent);
    margin-top: 0.15rem;
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
