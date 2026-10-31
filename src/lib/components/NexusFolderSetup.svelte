<script lang="ts">
  import { onMount } from "svelte";
  import { open } from "@tauri-apps/plugin-dialog";
  import { api, asAppError, nexus, nexusGameImage, type AppError, type Layout, type NexusGameSetup } from "$lib/api";
  import { tr } from "$lib/i18n.svelte";
  import { layoutLabels } from "$lib/installs.svelte";
  import { engineNames, launcherNames, nexusStore } from "$lib/nexus.svelte";
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
  /** Dossier du jeu trouvé dans Steam ou Epic, dont le moteur est reconnu. */
  const usingInstalled = $derived(!!setup?.installed && folder === setup.installed.path);
  const engineKnown = $derived(usingInstalled && !!setup?.engine && !!setup?.autoRules);

  onMount(async () => {
    try {
      const s = await nexusStore.loadSetup(gameId, domain);
      setup = s;
      folder = s.config?.folder ?? "";
      layout = s.config?.layout ?? (s.rules ? "flat" : "subfolder");
      gameDir = s.gameDir ?? "";
      // Jeu à règles : le dossier habituel est choisi d'office.
      if (!folder && s.rules && s.suggestedFolder) folder = s.suggestedFolder;
      // Jeu installé trouvé dans Steam ou Epic : son dossier est proposé.
      if (!folder && !s.rules && s.installed && s.engine && s.autoRules) folder = s.installed.path;
    } catch (e) {
      loadError = asAppError(e);
    }
  });

  // Alerte si le dossier des mods ressemble au dossier d'installation du jeu.
  $effect(() => {
    const f = folder;
    gameRoot = false;
    if (!f || rules || f === setup?.installed?.path) return;
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
      title:
        target === "mods"
          ? tr(`Dossier des mods de ${gameName}`, `${gameName} mods folder`)
          : tr(`Dossier d'installation de ${gameName}`, `${gameName} installation folder`),
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
        layout: rules || engineKnown ? "flat" : layout,
        create: usingSuggestion && !setup?.suggestedFolderExists,
        gameDir: gameDir || null,
      });
      await nexusStore.load();
      nexusStore.loadSetup(gameId, domain).catch(() => {});
      onsaved();
    } catch (e) {
      error = asAppError(e);
    } finally {
      saving = false;
    }
  }
</script>

<div class="setup">
  <p class="title">{tr(`Où installer les mods de ${gameName} ?`, `Where should mods for ${gameName} be installed?`)}</p>

  {#if loadError}
    <ErrorNotice error={loadError} />
  {:else if !setup}
    <p class="muted small">{tr("Recherche des dossiers du jeu…", "Looking for the game folders…")}</p>
  {:else if rules === "bonelab"}
    <p class="small muted">
      {tr(
        "ModStudio range chaque mod au bon endroit : le contenu (avatars, cartes, armes) dans le dossier des mods, les mods de code MelonLoader dans le dossier du jeu. Les versions Quest sont ignorées.",
        "ModStudio puts each mod in the right place: content (avatars, maps, weapons) in the mods folder, MelonLoader code mods in the game folder. Quest versions are ignored.",
      )}
    </p>

    <div class="block">
      <p class="small strong">{tr("Contenu (avatars, cartes, armes…)", "Content (avatars, maps, weapons…)")}</p>
      <p class="small path" class:muted={!folder}>{folder || tr("Aucun dossier choisi", "No folder selected")}</p>
      <button class="btn" type="button" onclick={() => pick("mods")} disabled={locked}
        >{tr("Choisir un autre dossier…", "Choose another folder…")}</button
      >
      {#if usingSuggestion}
        <p class="small muted">
          {tr("Dossier habituel de Bonelab", "Usual Bonelab folder")}{setup.suggestedFolderExists
            ? ""
            : tr(" (il n'existe pas encore : ModStudio le créera)", " (it doesn't exist yet: ModStudio will create it)")}.
        </p>
      {:else if setup.suggestedFolder && !locked}
        <button class="link small" type="button" onclick={() => (folder = setup!.suggestedFolder!)}>
          {tr("Utiliser le dossier habituel de Bonelab", "Use the usual Bonelab folder")}
        </button>
      {/if}
    </div>

    <div class="block">
      <p class="small strong">{tr("Dossier du jeu (mods de code)", "Game folder (code mods)")}</p>
      <p class="small path" class:muted={!gameDir}>{gameDir || tr("Pas encore trouvé", "Not found yet")}</p>
      <button class="btn" type="button" onclick={() => pick("game")}>{gameDir
          ? tr("Choisir un autre dossier…", "Choose another folder…")
          : tr("Choisir le dossier…", "Choose the folder…")}</button>
      {#if gameDir && setup.gameDirDetected && gameDir === setup.gameDir}
        <p class="small muted">{tr("Trouvé dans ta bibliothèque Steam.", "Found in your Steam library.")}</p>
      {:else if !gameDir}
        <p class="small muted">
          {tr(
            "Seulement pour les mods de code (.dll). C'est le dossier qui contient BONELAB_Steam_Windows64.exe.",
            "Only for code mods (.dll). It's the folder that contains BONELAB_Steam_Windows64.exe.",
          )}
        </p>
      {/if}
      {#if gameDir && gameDir === setup.gameDir && !setup.melonloader}
        <p class="small warning">
          <Icon name="warning" size={16} />
          <span>
            {tr(
              "MelonLoader n'est pas installé dans ce dossier : les mods de code en ont besoin. Une fois ce réglage enregistré, ModStudio pourra l'installer pour toi depuis la page du jeu.",
              "MelonLoader isn't installed in this folder: code mods need it. Once this setting is saved, ModStudio can install it for you from the game page.",
            )}
          </span>
        </p>
      {/if}
    </div>
  {:else}
    <details class="help">
      <summary>{tr("Comment trouver le bon dossier ?", "How do I find the right folder?")}</summary>
      <ul class="small">
        <li>
          {tr(
            "L'onglet « Description » de la page du mod indique presque toujours où mettre ses fichiers.",
            'The "Description" tab of the mod page almost always says where to put its files.',
          )}
        </li>
        <li>
          {tr(
            `Cherche « ${gameName} mods folder », ou le guide de modding du jeu sur Nexus Mods.`,
            `Search for "${gameName} mods folder", or the game's modding guide on Nexus Mods.`,
          )}
        </li>
        <li>
          {tr(
            "Pour certains jeux (Skyrim, Fallout…), le gestionnaire officiel Vortex reste plus adapté : il gère l'ordre de chargement des plugins.",
            "For some games (Skyrim, Fallout…), the official Vortex manager is still a better fit: it handles plugin load order.",
          )}
        </li>
      </ul>
    </details>

    <div class="folder">
      <input class="field" readonly value={folder} placeholder={tr("Aucun dossier choisi", "No folder selected")}
        aria-label={tr("Dossier des mods", "Mods folder")} />
      <button class="btn" type="button" onclick={() => pick("mods")} disabled={locked}>{tr("Choisir…", "Choose…")}</button>
    </div>

    {#if engineKnown && setup.installed && setup.engine}
      {@const launcher = launcherNames[setup.installed.launcher] ?? setup.installed.launcher}
      {@const engine = engineNames[setup.engine.engine] ?? setup.engine.engine}
      <p class="small found">
        <Icon name="check" size={15} />
        <span>
          {tr(
            `Dossier du jeu trouvé dans ${launcher}. Le jeu utilise ${engine} : ModStudio range chaque mod au bon endroit.`,
            `Game folder found in ${launcher}. The game uses ${engine}: ModStudio puts each mod in the right place.`,
          )}
        </span>
      </p>
    {:else if setup.installed && !locked}
      {@const launcher = launcherNames[setup.installed.launcher] ?? setup.installed.launcher}
      <button class="link small" type="button" onclick={() => (folder = setup!.installed!.path)}>
        {tr(`Utiliser le dossier du jeu trouvé dans ${launcher}`, `Use the game folder found in ${launcher}`)}
      </button>
    {/if}

    {#if gameRoot && !locked}
      <p class="warning small" role="alert">
        <Icon name="warning" size={16} />
        <span>
          {tr(
            "Ce dossier ressemble au dossier d'installation du jeu. Certains jeux y chargent leurs mods, mais c'est rare : vérifie les instructions du mod.",
            "This folder looks like the game's installation folder. Some games load their mods from there, but it's rare: check the mod's instructions.",
          )}
        </span>
      </p>
    {/if}

    {#if !engineKnown}
    <fieldset class="layouts">
      <legend class="small strong">{tr("Rangement des fichiers", "File layout")}</legend>
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
  {/if}

  {#if locked}
    <p class="muted small">
      {tr(
        "Pour changer le dossier des mods, désinstalle d'abord les mods de ce jeu.",
        "To change the mods folder, uninstall this game's mods first.",
      )}
    </p>
  {/if}

  {#if error}<ErrorNotice {error} />{/if}

  <div class="actions">
    <button class="btn btn-primary" type="button" onclick={save} disabled={!setup || !folder || saving}>
      {saving ? tr("Vérification…", "Checking…") : tr("Enregistrer", "Save")}
    </button>
    <button class="btn btn-ghost" type="button" onclick={oncancel}>{tr("Annuler", "Cancel")}</button>
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

  .found {
    display: flex;
    align-items: flex-start;
    gap: 0.5rem;
  }

  .found :global(svg) {
    flex: none;
    color: var(--ok);
    margin-top: 0.15rem;
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
