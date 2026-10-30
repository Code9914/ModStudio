<script lang="ts">
  // Proposition d'installer l'outil de mods d'un jeu (UE4SS, BepInEx,
  // MelonLoader), après confirmation. Les script extenders des jeux Bethesda
  // s'installent depuis leur page Nexus Mods, comme un mod.
  import { asAppError, type AppError, type NexusGame, type ToolOffer } from "$lib/api";
  import { nav } from "$lib/nav.svelte";
  import { nexusStore } from "$lib/nexus.svelte";
  import ErrorNotice from "./ErrorNotice.svelte";
  import Icon from "./Icon.svelte";
  import ProgressBar from "./ProgressBar.svelte";

  let {
    game,
    tool,
    reason = null,
    folder = null,
    oninstalled,
  }: {
    game: NexusGame;
    tool: ToolOffer;
    /** Pourquoi l'installer (« Ce mod a besoin de UE4SS… »). */
    reason?: string | null;
    /** Dossier du jeu où l'outil sera copié. */
    folder?: string | null;
    oninstalled?: () => void;
  } = $props();

  let confirming = $state(false);
  let error = $state<AppError | null>(null);
  let done = $state(false);

  const progress = $derived(nexusStore.toolProgress[game.id] ?? null);
  const fromNexus = $derived(tool.nexusMod !== null);

  async function install() {
    confirming = false;
    error = null;
    try {
      await nexusStore.installTool(game.id, game.domainName, tool.id);
      done = true;
      oninstalled?.();
    } catch (e) {
      const err = asAppError(e);
      if (err.kind !== "cancelled") error = err;
    }
  }
</script>

<div class="prompt" class:done>
  {#if done}
    <p class="small ok" role="status">
      <Icon name="check" size={15} />
      <span>
        {tool.name} est installé. Lance le jeu une fois : {tool.name} se prépare au premier démarrage, puis charge les mods.
      </span>
    </p>
  {:else if progress}
    <p class="small strong">Installation de {tool.name}…</p>
    <ProgressBar {progress} oncancel={() => nexusStore.cancelTool(game.id)} />
  {:else if confirming}
    <p class="small">
      ModStudio va télécharger <span class="strong">{tool.name}</span> depuis sa page officielle sur GitHub et le copier
      dans le dossier du jeu{folder ? "" : "."}
      {#if folder}<span class="path">({folder})</span>.{/if}
      Tu pourras le retirer depuis ModStudio à tout moment.
    </p>
    <div class="row">
      <button class="btn btn-primary" onclick={install}>Installer {tool.name}</button>
      <button class="btn btn-ghost" onclick={() => (confirming = false)}>Annuler</button>
    </div>
  {:else}
    <p class="small">
      {#if reason}<span class="strong">{reason}</span>{/if}
      {#if fromNexus}
        {tool.name} est le {tool.description}.
        {#if tool.gameVersion}Ton jeu est en version {tool.gameVersion} : prends le fichier de {tool.name} qui lui correspond.{/if}
      {:else}
        {tool.name} est un {tool.description}.
      {/if}
    </p>
    <div class="row">
      {#if fromNexus}
        <button class="btn btn-primary" onclick={() => nav.openNexusMod(tool.nexusMod!)}>
          Installer {tool.name} depuis Nexus Mods
        </button>
      {:else}
        <button class="btn btn-primary" onclick={() => (confirming = true)}>
          <Icon name="download" size={15} />Installer {tool.name}
        </button>
      {/if}
    </div>
  {/if}
  {#if error}<ErrorNotice {error} />{/if}
</div>

<style>
  .prompt {
    display: grid;
    gap: 0.6rem;
    min-width: 0;
  }

  .small {
    font-size: var(--t-sm);
  }

  .strong {
    font-weight: 600;
    color: var(--text);
  }

  .path {
    overflow-wrap: anywhere;
    color: var(--muted);
  }

  .row {
    display: flex;
    flex-wrap: wrap;
    gap: 0.4rem;
  }

  .ok {
    display: flex;
    align-items: flex-start;
    gap: 0.45rem;
    color: var(--ok);
  }

  .ok :global(svg) {
    flex: none;
    margin-top: 0.15rem;
  }
</style>
