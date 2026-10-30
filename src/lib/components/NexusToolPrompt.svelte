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
    onstart,
    oninstalled,
  }: {
    game: NexusGame;
    tool: ToolOffer;
    /** Pourquoi l'installer (« Ce mod a besoin de UE4SS… »). */
    reason?: string | null;
    /** Dossier du jeu où l'outil sera copié. */
    folder?: string | null;
    /** L'installation démarre depuis ce panneau. */
    onstart?: () => void;
    oninstalled?: () => void;
  } = $props();

  let confirming = $state(false);
  let error = $state<AppError | null>(null);
  let done = $state(false);

  const progress = $derived(nexusStore.toolProgress[game.id] ?? null);
  const fromNexus = $derived(tool.nexusMod !== null);
  /** Déjà installé par ModStudio : remplacement par une autre version. */
  const updating = $derived(tool.managed && !!tool.update);
  const source = $derived(
    tool.id === "bepinex6" ? "le site officiel des versions de BepInEx" : "sa page officielle sur GitHub",
  );

  async function install() {
    confirming = false;
    error = null;
    onstart?.();
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
        {tool.name} est {updating ? "à jour" : "installé"}. Lance le jeu une fois : {tool.name} se prépare au premier démarrage
        (ça peut prendre une minute), puis charge les mods.
      </span>
    </p>
  {:else if progress}
    <p class="small strong">{updating ? "Mise à jour" : "Installation"} de {tool.name}…</p>
    <ProgressBar {progress} oncancel={() => nexusStore.cancelTool(game.id)} />
  {:else if confirming && updating}
    <p class="small">
      ModStudio va remplacer <span class="strong">{tool.name} {tool.installedVersion ?? ""}</span> par la version
      <span class="strong">{tool.update}</span>, téléchargée depuis {source}. Tes mods et leurs réglages restent en place.
    </p>
    <div class="row">
      <button class="btn btn-primary" onclick={install}>Mettre à jour {tool.name}</button>
      <button class="btn btn-ghost" onclick={() => (confirming = false)}>Annuler</button>
    </div>
  {:else if confirming}
    <p class="small">
      ModStudio va télécharger <span class="strong">{tool.name}</span>
      {#if tool.requiredVersion}(version {tool.requiredVersion}){/if} depuis {source} et le copier dans le dossier du
      jeu{folder ? "" : "."}
      {#if folder}<span class="path">({folder})</span>.{/if}
      Tu pourras le retirer depuis ModStudio à tout moment.
    </p>
    <div class="row">
      <button class="btn btn-primary" onclick={install}>Installer {tool.name}</button>
      <button class="btn btn-ghost" onclick={() => (confirming = false)}>Annuler</button>
    </div>
  {:else if updating}
    <p class="small">
      {#if tool.requiredVersion && tool.update === tool.requiredVersion}
        <span class="strong">Les mods de ce jeu demandent {tool.name} {tool.requiredVersion}.</span>
        La version installée ({tool.installedVersion ?? "inconnue"}) risque de ne pas les charger.
      {:else}
        <span class="strong">Nouvelle version de {tool.name} : {tool.update}.</span>
        Version installée : {tool.installedVersion ?? "inconnue"}.
      {/if}
    </p>
    <div class="row">
      <button class="btn btn-primary" onclick={() => (confirming = true)}>
        <Icon name="refresh" size={15} />Mettre à jour {tool.name}
      </button>
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
