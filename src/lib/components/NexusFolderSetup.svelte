<script lang="ts">
  import { onMount } from "svelte";
  import { open } from "@tauri-apps/plugin-dialog";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { api, asAppError, nexus, nexusGameImage, type AppError, type Layout, type NexusGameSetup } from "$lib/api";
  import { layoutLabels } from "$lib/installs.svelte";
  import { nexusStore } from "$lib/nexus.svelte";
  import ErrorNotice from "./ErrorNotice.svelte";
  import Icon from "./Icon.svelte";

  let {
    gameId,
    domain,
    gameName,
    onsaved,
    oncancel,
  }: {
    gameId: number;
    domain: string;
    gameName: string;
    onsaved: () => void;
    oncancel: () => void;
  } = $props();

  let setup = $state<NexusGameSetup | null>(null);
  let loadError = $state<AppError | null>(null);
  let folder = $state("");
  let layout = $state<Layout>("subfolder");
  let gameDir = $state("");
  let gameRoot = $state(false);
  let saving = $state(false);
  let error = $state<AppError | null>(null);

  const rules = $derived(setup?.rules ?? null);
  const locked = $derived(setup?.config ? Object.keys(setup.config.mods).length > 0 : false);
  const usingSuggestion = $derived(!!setup?.suggestedFolder && folder === setup.suggestedFolder);

  onMount(async () => {
    try {
      const s = await nexus.gameSetup(gameId);
      setup = s;
      folder = s.config?.folder ?? "";
      layout = s.config?.layout ?? (s.rules ? "flat" : "subfolder");
      gameDir = s.gameDir ?? "";
      // Jeu à règles : le dossier habituel est choisi d'office.
      if (!folder && s.rules && s.suggestedFolder) folder = s.suggestedFolder;
    } catch (e) {
      loadError = asAppError(e);
    }
  });

  // Alerte si le dossier des mods ressemble au dossier d'installation du jeu.
  $effect(() => {
    const f = folder;
    gameRoot = false;
    if (!f || rules) return;
    api
      .inspectFolder(f)
      .then((info) => {
        if (folder === f) gameRoot = info.looksLikeGameRoot;
      })
      .catch(() => {});
  });

  async function pick(target: "mods" | "game") {
    error = null;
    const picked = await open({
      directory: true,
      multiple: false,
      title: target === "mods" ? `Dossier des mods de ${gameName}` : `Dossier d'installation de ${gameName}`,
      defaultPath: (target === "mods" ? folder : gameDir) || undefined,
    });
    if (typeof picked !== "string") return;
    if (target === "mods") folder = picked;
    else gameDir = picked;
  }

  async function save() {
    saving = true;
    error = null;
    try {
      await nexus.configureGame({
        gameId,
        domain,
        gameName,
        icon: nexusGameImage(gameId, "thumbnail"),
        folder,
        layout: rules ? "flat" : layout,
        create: usingSuggestion && !setup?.suggestedFolderExists,
        gameDir: gameDir || null,
      });
      await nexusStore.load();
      onsaved();
    } catch (e) {
      error = asAppError(e);
    } finally {
      saving = false;
    }
  }
</script>

<div class="setup">
  <p class="title">Où installer les mods de {gameName} ?</p>

  {#if loadError}
    <ErrorNotice error={loadError} />
  {:else if !setup}
    <p class="muted small">Recherche des dossiers du jeu…</p>
  {:else if rules === "bonelab"}
    <p class="small muted">
      ModStudio range chaque mod au bon endroit : le contenu (avatars, cartes, armes) dans le dossier des mods, les mods
      de code MelonLoader dans le dossier du jeu. Les versions Quest sont ignorées.
    </p>

    <div class="block">
      <p class="small strong">Contenu (avatars, cartes, armes…)</p>
      <p class="small path" class:muted={!folder}>{folder || "Aucun dossier choisi"}</p>
      <button class="btn" type="button" onclick={() => pick("mods")} disabled={locked}>Choisir un autre dossier…</button>
      {#if usingSuggestion}
        <p class="small muted">
          Dossier habituel de Bonelab{setup.suggestedFolderExists ? "" : " (il n'existe pas encore : ModStudio le créera)"}.
        </p>
      {:else if setup.suggestedFolder && !locked}
        <button class="link small" type="button" onclick={() => (folder = setup!.suggestedFolder!)}>
          Utiliser le dossier habituel de Bonelab
        </button>
      {/if}
    </div>

    <div class="block">
      <p class="small strong">Dossier du jeu (mods de code)</p>
      <p class="small path" class:muted={!gameDir}>{gameDir || "Pas encore trouvé"}</p>
      <button class="btn" type="button" onclick={() => pick("game")}>{gameDir ? "Choisir un autre dossier…" : "Choisir le dossier…"}</button>
      {#if gameDir && setup.gameDirDetected && gameDir === setup.gameDir}
        <p class="small muted">Trouvé dans ta bibliothèque Steam.</p>
      {:else if !gameDir}
        <p class="small muted">
          Seulement pour les mods de code (.dll). C'est le dossier qui contient BONELAB_Steam_Windows64.exe.
        </p>
      {/if}
      {#if gameDir && gameDir === setup.gameDir && !setup.melonloader}
        <p class="small warning">
          <Icon name="warning" size={16} />
          <span>
            MelonLoader n'est pas installé dans ce dossier : les mods de code en ont besoin.
            <button class="link" type="button" onclick={() => openUrl("https://github.com/LavaGang/MelonLoader#readme")}
              >Installer MelonLoader <Icon name="external" size={12} /></button
            >
          </span>
        </p>
      {/if}
    </div>
  {:else}
    <details class="help">
      <summary>Comment trouver le bon dossier ?</summary>
      <ul class="small">
        <li>L'onglet « Description » de la page du mod indique presque toujours où mettre ses fichiers.</li>
        <li>Cherche « {gameName} mods folder », ou le guide de modding du jeu sur Nexus Mods.</li>
        <li>
          Pour certains jeux (Skyrim, Fallout…), le gestionnaire officiel Vortex reste plus adapté : il gère l'ordre de
          chargement des plugins.
        </li>
      </ul>
    </details>

    <div class="folder">
      <input class="field" readonly value={folder} placeholder="Aucun dossier choisi" aria-label="Dossier des mods" />
      <button class="btn" type="button" onclick={() => pick("mods")} disabled={locked}>Choisir…</button>
    </div>

    {#if gameRoot && !locked}
      <p class="warning small" role="alert">
        <Icon name="warning" size={16} />
        <span>
          Ce dossier ressemble au dossier d'installation du jeu. Certains jeux y chargent leurs mods, mais c'est rare :
          vérifie les instructions du mod.
        </span>
      </p>
    {/if}

    <fieldset class="layouts">
      <legend class="small strong">Rangement des fichiers</legend>
      {#each Object.entries(layoutLabels) as [value, label] (value)}
        <label class="layout">
          <input type="radio" name="nx-layout-{gameId}" {value} bind:group={layout} />
          <span>
            <span class="strong">{label.title}</span>
            <span class="muted small">{label.help}</span>
          </span>
        </label>
      {/each}
    </fieldset>
  {/if}

  {#if locked}
    <p class="muted small">Pour changer le dossier des mods, désinstalle d'abord les mods de ce jeu.</p>
  {/if}

  {#if error}<ErrorNotice {error} />{/if}

  <div class="actions">
    <button class="btn btn-primary" type="button" onclick={save} disabled={!setup || !folder || saving}>
      {saving ? "Vérification…" : "Enregistrer"}
    </button>
    <button class="btn btn-ghost" type="button" onclick={oncancel}>Annuler</button>
  </div>
</div>

<style>
  .setup {
    display: grid;
    gap: 0.8rem;
    min-width: 0;
  }

  .title {
    font-family: var(--font-display);
    font-weight: 700;
    font-size: var(--t-lg);
  }

  .small {
    font-size: var(--t-sm);
  }

  .strong {
    font-weight: 600;
  }

  .block {
    display: grid;
    gap: 0.4rem;
    padding: 0.8rem 0.9rem;
    border-radius: var(--r-md);
    background: var(--raise);
    justify-items: start;
  }

  .path {
    overflow-wrap: anywhere;
  }

  .help summary {
    cursor: pointer;
    font-size: var(--t-sm);
    color: var(--accent);
    width: fit-content;
  }

  .help ul {
    margin: 0.5rem 0 0;
    padding-left: 1.1rem;
    display: grid;
    gap: 0.35rem;
    color: var(--muted);
  }

  .link {
    display: inline-flex;
    align-items: center;
    gap: 0.15rem;
    padding: 0;
    border: 0;
    background: none;
    color: var(--accent);
    text-decoration: underline;
    text-underline-offset: 3px;
    font-size: inherit;
  }

  .warning {
    display: flex;
    align-items: flex-start;
    gap: 0.5rem;
    padding: 0.6rem 0.7rem;
    border-radius: var(--r-md);
    background: var(--danger-bg);
  }

  .warning :global(svg) {
    flex: none;
    color: var(--danger);
    margin-top: 0.1rem;
  }

  .folder {
    display: flex;
    gap: 0.4rem;
  }

  .folder .field {
    font-size: var(--t-sm);
    text-overflow: ellipsis;
    min-width: 0;
  }

  .layouts {
    border: 0;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 0.5rem;
  }

  .layouts legend {
    padding: 0;
    margin-bottom: 0.3rem;
  }

  .layout {
    display: flex;
    align-items: flex-start;
    gap: 0.55rem;
    cursor: pointer;
  }

  .layout input {
    margin-top: 0.3rem;
    accent-color: var(--accent-fill);
  }

  .layout > span {
    display: grid;
    gap: 0.05rem;
    font-size: var(--t-sm);
  }

  .actions {
    display: flex;
    gap: 0.4rem;
  }
</style>
