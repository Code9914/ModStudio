<script lang="ts">
  // Proposition d'installer l'outil de mods d'un jeu (UE4SS, BepInEx,
  // MelonLoader), après confirmation. Les script extenders des jeux Bethesda
  // s'installent depuis leur page Nexus Mods, comme un mod.
  import { asAppError, type AppError, type NexusGame, type ToolOffer } from "$lib/api";
  import { english, tr } from "$lib/i18n.svelte";
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
    tool.id === "bepinex6"
      ? tr("le site officiel des versions de BepInEx", "the official BepInEx builds site")
      : tr("sa page officielle sur GitHub", "its official GitHub page"),
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
        {tr(
          `${tool.name} est ${updating ? "à jour" : "installé"}. Lance le jeu une fois : ${tool.name} se prépare au premier démarrage (ça peut prendre une minute), puis charge les mods.`,
          `${tool.name} is ${updating ? "up to date" : "installed"}. Launch the game once: ${tool.name} sets itself up on first launch (this can take a minute), then loads the mods.`,
        )}
      </span>
    </p>
  {:else if progress}
    <p class="small strong">
      {tr(
        `${updating ? "Mise à jour" : "Installation"} de ${tool.name}…`,
        `${updating ? "Updating" : "Installing"} ${tool.name}…`,
      )}
    </p>
    <ProgressBar {progress} oncancel={() => nexusStore.cancelTool(game.id)} />
  {:else if confirming && updating}
    <p class="small">
      {#if english}
        ModStudio will replace <span class="strong">{tool.name} {tool.installedVersion ?? ""}</span> with version
        <span class="strong">{tool.update}</span>, downloaded from {source}. Your mods and their settings stay in place.
      {:else}
        ModStudio va remplacer <span class="strong">{tool.name} {tool.installedVersion ?? ""}</span> par la version
        <span class="strong">{tool.update}</span>, téléchargée depuis {source}. Tes mods et leurs réglages restent en place.
      {/if}
    </p>
    <div class="row">
      <button class="btn btn-primary" onclick={install}>{tr(`Mettre à jour ${tool.name}`, `Update ${tool.name}`)}</button>
      <button class="btn btn-ghost" onclick={() => (confirming = false)}>{tr("Annuler", "Cancel")}</button>
    </div>
  {:else if confirming}
    <p class="small">
      {#if english}
        ModStudio will download <span class="strong">{tool.name}</span>
        {#if tool.requiredVersion}(version {tool.requiredVersion}){/if} from {source} and copy it to the game
        folder{folder ? "" : "."}
        {#if folder}<span class="path">({folder})</span>.{/if}
        You can remove it from ModStudio at any time.
      {:else}
        ModStudio va télécharger <span class="strong">{tool.name}</span>
        {#if tool.requiredVersion}(version {tool.requiredVersion}){/if} depuis {source} et le copier dans le dossier du
        jeu{folder ? "" : "."}
        {#if folder}<span class="path">({folder})</span>.{/if}
        Tu pourras le retirer depuis ModStudio à tout moment.
      {/if}
    </p>
    <div class="row">
      <button class="btn btn-primary" onclick={install}>{tr(`Installer ${tool.name}`, `Install ${tool.name}`)}</button>
      <button class="btn btn-ghost" onclick={() => (confirming = false)}>{tr("Annuler", "Cancel")}</button>
    </div>
  {:else if updating}
    <p class="small">
      {#if tool.requiredVersion && tool.update === tool.requiredVersion}
        <span class="strong"
          >{tr(
            `Les mods de ce jeu demandent ${tool.name} ${tool.requiredVersion}.`,
            `This game's mods require ${tool.name} ${tool.requiredVersion}.`,
          )}</span
        >
        {tr(
          `La version installée (${tool.installedVersion ?? "inconnue"}) risque de ne pas les charger.`,
          `The installed version (${tool.installedVersion ?? "unknown"}) may not load them.`,
        )}
      {:else}
        <span class="strong"
          >{tr(`Nouvelle version de ${tool.name} : ${tool.update}.`, `New version of ${tool.name}: ${tool.update}.`)}</span
        >
        {tr(
          `Version installée : ${tool.installedVersion ?? "inconnue"}.`,
          `Installed version: ${tool.installedVersion ?? "unknown"}.`,
        )}
      {/if}
    </p>
    <div class="row">
      <button class="btn btn-primary" onclick={() => (confirming = true)}>
        <Icon name="refresh" size={15} />{tr(`Mettre à jour ${tool.name}`, `Update ${tool.name}`)}
      </button>
    </div>
  {:else}
    <p class="small">
      {#if reason}<span class="strong">{reason}</span>{/if}
      {#if fromNexus}
        {tr(`${tool.name} est le ${tool.description}.`, `${tool.name} is the ${tool.description}.`)}
        {#if tool.gameVersion}{tr(
            `Ton jeu est en version ${tool.gameVersion} : prends le fichier de ${tool.name} qui lui correspond.`,
            `Your game is version ${tool.gameVersion}: pick the ${tool.name} file that matches it.`,
          )}{/if}
      {:else}
        {tr(`${tool.name} est un ${tool.description}.`, `${tool.name} is a ${tool.description}.`)}
      {/if}
    </p>
    <div class="row">
      {#if fromNexus}
        <button class="btn btn-primary" onclick={() => nav.openNexusMod(tool.nexusMod!)}>
          {tr(`Installer ${tool.name} depuis Nexus Mods`, `Install ${tool.name} from Nexus Mods`)}
        </button>
      {:else}
        <button class="btn btn-primary" onclick={() => (confirming = true)}>
          <Icon name="download" size={15} />{tr(`Installer ${tool.name}`, `Install ${tool.name}`)}
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
