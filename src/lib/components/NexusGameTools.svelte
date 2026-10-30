<script lang="ts">
  // Le jeu sur ce PC : dossier trouvé dans Steam / Epic, moteur, et outil de
  // mods à installer (UE4SS, BepInEx, MelonLoader, script extender).
  import { asAppError, nexus, type AppError, type NexusGame } from "$lib/api";
  import { engineNames, launcherNames, nexusStore } from "$lib/nexus.svelte";
  import ErrorNotice from "./ErrorNotice.svelte";
  import Icon from "./Icon.svelte";
  import NexusFolderSetup from "./NexusFolderSetup.svelte";
  import NexusToolPrompt from "./NexusToolPrompt.svelte";

  let { game }: { game: NexusGame } = $props();

  let loadError = $state<AppError | null>(null);
  let error = $state<AppError | null>(null);
  let setupOpen = $state(false);
  let busy = $state(false);
  let confirmRemove = $state(false);
  let note = $state<string | null>(null);

  const setup = $derived(nexusStore.setups[game.id] ?? null);
  const config = $derived(setup?.config ?? null);
  const installed = $derived(setup?.installed ?? null);
  const engine = $derived(setup?.engine ?? null);
  const tool = $derived(setup?.tool ?? null);
  const hasMods = $derived(config ? Object.keys(config.mods).length > 0 : false);
  /** Configuré ailleurs que dans le dossier du jeu trouvé (et rien d'installé) : on propose de changer. */
  const betterFolder = $derived(
    !!installed && !!config && !config.rules && !!setup?.autoRules && !hasMods && config.folder !== installed.path,
  );
  const toolFolder = $derived(
    config?.rules === "bonelab" ? (config.extraFolders?.game ?? setup?.gameDir ?? null) : (config?.folder ?? null),
  );
  const managedTool = $derived(tool && config?.tools ? (config.tools[tool.id] ?? null) : null);

  // Sous Linux, Proton ne charge la DLL du chargeur que si on le lui demande.
  const protonValue = $derived(tool?.launchOption?.match(/"([^"]+)"/)?.[1] ?? null);
  let copied = $state(false);
  async function copyLaunchOption() {
    if (!tool?.launchOption) return;
    try {
      await navigator.clipboard.writeText(tool.launchOption);
      copied = true;
      setTimeout(() => (copied = false), 2000);
    } catch {
      /* presse-papiers indisponible : le texte reste sélectionnable */
    }
  }

  $effect(() => {
    const id = game.id;
    const domain = game.domainName;
    loadError = null;
    if (!domain) return;
    nexusStore.loadSetup(id, domain).catch((e) => {
      if (id === game.id) loadError = asAppError(e);
    });
  });

  async function useFolder() {
    if (!installed) return;
    busy = true;
    error = null;
    try {
      await nexusStore.useInstalledFolder(game, installed.path);
    } catch (e) {
      error = asAppError(e);
    } finally {
      busy = false;
    }
  }

  let removeTimer: ReturnType<typeof setTimeout> | undefined;

  async function removeTool() {
    if (!tool) return;
    if (!confirmRemove) {
      confirmRemove = true;
      clearTimeout(removeTimer);
      removeTimer = setTimeout(() => (confirmRemove = false), 4000);
      return;
    }
    confirmRemove = false;
    busy = true;
    error = null;
    note = null;
    try {
      const left = await nexusStore.uninstallTool(game.id, game.domainName, tool.id);
      note = left
        ? `${tool.name} est retiré. Le dossier « ${left} » contient des fichiers créés depuis (réglages, journaux) : il a été laissé en place.`
        : `${tool.name} est retiré.`;
    } catch (e) {
      error = asAppError(e);
    } finally {
      busy = false;
    }
  }

  function reveal() {
    nexus.reveal(game.id).catch((e) => (error = asAppError(e)));
  }

  const popularity = $derived(
    tool && tool.popular > 0 ? `${tool.popular} des ${tool.sampled} mods les plus recommandés de ce jeu en ont besoin.` : null,
  );
</script>

{#if loadError}
  <ErrorNotice error={loadError} />
{:else if setup && (config || installed || setupOpen)}
  <section class="tools" aria-label="{game.name} sur ce PC">
    {#if setupOpen}
      <NexusFolderSetup
        gameId={game.id}
        domain={game.domainName}
        gameName={game.name}
        onsaved={() => {
          setupOpen = false;
          nexusStore.loadSetup(game.id, game.domainName).catch(() => {});
        }}
        oncancel={() => (setupOpen = false)}
      />
    {:else}
      <div class="where">
        <div class="where-text">
          {#if config}
            <p class="label">Dossier des mods</p>
            <p class="path" title={config.folder}>{config.folder}</p>
          {:else if installed}
            <p class="label">Trouvé dans {launcherNames[installed.launcher] ?? installed.launcher}</p>
            <p class="path" title={installed.path}>{installed.path}</p>
          {/if}
          <p class="facts">
            {#if installed && config}<span class="chip">{launcherNames[installed.launcher] ?? installed.launcher}</span>{/if}
            {#if engine}<span class="chip">{engineNames[engine.engine] ?? engine.engine}</span>{/if}
            {#if engine && config?.rules && config.rules !== "bonelab"}
              <span class="muted">Chaque mod est rangé au bon endroit selon le moteur du jeu.</span>
            {/if}
          </p>
        </div>
        <div class="row">
          <!-- BONELAB (4832) a deux dossiers : son propre réglage s'en charge. -->
          {#if !config && installed && setup.autoRules && game.id !== 4832}
            <button class="btn btn-primary" onclick={useFolder} disabled={busy}>Installer les mods dans ce dossier</button>
            <button class="btn" onclick={() => (setupOpen = true)}>Choisir un autre dossier…</button>
          {:else if !config}
            <!-- Moteur sans rangement automatique : dossier et rangement au choix. -->
            <button class="btn btn-primary" onclick={() => (setupOpen = true)}>Choisir où installer les mods</button>
          {:else}
            <button class="btn" onclick={reveal}>Ouvrir</button>
            {#if !hasMods}<button class="btn btn-ghost" onclick={() => (setupOpen = true)}>Changer</button>{/if}
          {/if}
        </div>
      </div>

      {#if betterFolder && installed}
        <p class="small hint">
          <Icon name="warning" size={15} />
          <span>
            Le jeu est installé dans {installed.path}. En y installant les mods, ModStudio les range lui-même au bon
            endroit.
            <button class="link" onclick={useFolder} disabled={busy}>Utiliser ce dossier</button>
          </span>
        </p>
      {/if}

      {#if tool}
        <div class="tool" class:ready={tool.present} class:stale={!!managedTool && !!tool.update}>
          {#if tool.present}
            <p class="small tool-ok">
              <Icon name="check" size={15} />
              <span>
                <span class="strong">{tool.name}</span>
                {managedTool ? `installé par ModStudio${managedTool.version ? ` (${managedTool.version})` : ""}` : "est installé"}{managedTool && tool.update
                  ? "."
                  : " : les mods qui en ont besoin se chargeront."}
              </span>
            </p>
            {#if managedTool}
              <button class="btn btn-ghost" class:danger={confirmRemove} onclick={removeTool} disabled={busy}>
                {confirmRemove ? "Confirmer" : `Retirer ${tool.name}`}
              </button>
            {/if}
            {#if managedTool && tool.update}
              <NexusToolPrompt {game} {tool} folder={toolFolder} />
            {/if}
            {#if tool.launchOption && installed?.launcher === "steam"}
              <div class="proton">
                <p class="small">
                  Sous Linux, ajoute ceci aux options de lancement du jeu dans Steam (Propriétés, Général) : sans ça,
                  Proton ne charge pas {tool.name}.
                </p>
                <div class="proton-row">
                  <code>{tool.launchOption}</code>
                  <button class="btn btn-ghost" onclick={copyLaunchOption}>{copied ? "Copié" : "Copier"}</button>
                </div>
              </div>
            {:else if protonValue && installed?.launcher === "epic"}
              <p class="small proton">
                Sous Linux, dans Heroic : ajoute la variable d'environnement <code>WINEDLLOVERRIDES</code> avec la valeur
                <code>{protonValue}</code> (réglages du jeu), sinon Wine ne charge pas {tool.name}.
              </p>
            {/if}
          {:else if config}
            <NexusToolPrompt {game} {tool} folder={toolFolder} reason={popularity ?? `${tool.name} n'est pas installé.`} />
          {:else}
            <p class="small muted">
              {tool.name} n'est pas installé. {popularity ?? ""}
              Choisis d'abord le dossier des mods : ModStudio pourra ensuite l'installer pour toi.
            </p>
          {/if}
        </div>
      {/if}

      {#if error}<ErrorNotice {error} />{/if}
      {#if note}<p class="small note" role="status">{note}</p>{/if}
    {/if}
  </section>
{/if}

<style>
  .tools {
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

  .facts {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.35rem 0.5rem;
    font-size: var(--t-sm);
    margin-top: 0.2rem;
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

  .tool {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 0.6rem 1rem;
    padding: 0.8rem 0.9rem;
    border-radius: var(--r-md);
    background: var(--raise);
    border-left: 3px solid var(--accent-fill);
  }

  .proton {
    display: grid;
    gap: 0.4rem;
    flex-basis: 100%;
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

  .tool.ready {
    border-left-color: var(--ok);
  }

  .tool.ready.stale {
    border-left-color: var(--accent-fill);
  }

  .tool > :global(.prompt) {
    flex: 1 1 100%;
  }

  .tool-ok {
    display: flex;
    align-items: flex-start;
    gap: 0.45rem;
    flex: 1 1 20rem;
  }

  .tool-ok :global(svg) {
    flex: none;
    color: var(--ok);
    margin-top: 0.15rem;
  }

  .hint {
    display: flex;
    align-items: flex-start;
    gap: 0.45rem;
  }

  .hint :global(svg) {
    flex: none;
    color: var(--accent);
    margin-top: 0.15rem;
  }

  .link {
    padding: 0;
    border: 0;
    background: none;
    color: var(--accent);
    text-decoration: underline;
    text-underline-offset: 3px;
    font-size: inherit;
  }

  .danger {
    background: var(--danger-bg);
    border-color: var(--danger);
    color: var(--danger);
  }

  .note {
    color: var(--ok);
  }
</style>
