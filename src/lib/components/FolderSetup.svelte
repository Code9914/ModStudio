<script lang="ts">
  import { onMount, untrack } from "svelte";
  import { open } from "@tauri-apps/plugin-dialog";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { api, asAppError, type AppError, type FolderSuggestion, type Layout } from "$lib/api";
  import { installs, layoutLabels } from "$lib/installs.svelte";
  import ErrorNotice from "./ErrorNotice.svelte";
  import Icon from "./Icon.svelte";

  let {
    gameId,
    gameName,
    icon = null,
    guideUrl = null,
    onsaved,
    oncancel,
  }: {
    gameId: number;
    gameName: string;
    icon?: string | null;
    /** Guide ou wiki de modding fourni par le jeu sur mod.io. */
    guideUrl?: string | null;
    onsaved: () => void;
    oncancel: () => void;
  } = $props();

  const current = $derived(installs.game(gameId));
  const locked = $derived(current ? Object.keys(current.mods).length > 0 : false);

  // Valeurs de départ du formulaire (modifiables ensuite).
  let folder = $state(untrack(() => installs.game(gameId)?.folder ?? ""));
  let layout = $state<Layout>(untrack(() => installs.game(gameId)?.layout ?? "subfolder"));
  let suggestion = $state<FolderSuggestion | null>(null);
  let gameRoot = $state(false);
  let saving = $state(false);
  let error = $state<AppError | null>(null);

  onMount(async () => {
    try {
      suggestion = await api.folderSuggestion(gameId);
    } catch {
      suggestion = null;
    }
  });

  // Alerte si le dossier ressemble au dossier d'installation du jeu.
  $effect(() => {
    const f = folder;
    gameRoot = false;
    if (!f) return;
    api
      .inspectFolder(f)
      .then((info) => {
        if (folder === f) gameRoot = info.looksLikeGameRoot;
      })
      .catch(() => {});
  });

  const usingSuggestion = $derived(!!suggestion && folder === suggestion.path);

  function useSuggestion() {
    if (!suggestion) return;
    folder = suggestion.path;
    layout = suggestion.layout;
    error = null;
  }

  async function pick() {
    error = null;
    const picked = await open({
      directory: true,
      multiple: false,
      title: `Dossier des mods de ${gameName}`,
      defaultPath: folder || undefined,
    });
    if (typeof picked === "string") folder = picked;
  }

  async function save() {
    saving = true;
    error = null;
    try {
      // Le dossier habituel n'existe pas toujours encore : on le crée alors.
      const create = usingSuggestion && !suggestion?.exists;
      await installs.setFolder(gameId, gameName, icon, folder, layout, create);
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

  {#if suggestion && !locked}
    <div class="suggestion" class:active={usingSuggestion}>
      <p class="small">
        <span class="strong">Dossier habituel des mods de {gameName}</span>
        <span class="path">{suggestion.path}</span>
        {#if !suggestion.exists}<span class="muted">Il n'existe pas encore : ModStudio le créera.</span>{/if}
      </p>
      {#if !usingSuggestion}
        <button class="btn btn-primary" type="button" onclick={useSuggestion}>Utiliser ce dossier</button>
      {:else}
        <span class="chip chip-ok">Choisi</span>
      {/if}
    </div>
  {:else if !locked}
    <details class="help">
      <summary>Comment trouver le bon dossier ?</summary>
      <ul class="small">
        <li>Regarde la description des mods sur mod.io : les auteurs indiquent souvent où mettre leurs fichiers.</li>
        <li>
          Cherche « {gameName} mods folder » ou consulte le guide de modding du jeu{#if guideUrl}
            (<button class="link" type="button" onclick={() => openUrl(guideUrl!)}
              >guide fourni par le jeu <Icon name="external" size={12} /></button
            >){/if}.
        </li>
        <li>
          C'est rarement le dossier d'installation du jeu (celui qui contient son .exe). Souvent, c'est un dossier
          « Mods » dans Documents ou dans AppData.
        </li>
        <li>Si le jeu gère mod.io lui-même, le plus simple est de t'abonner : il installera les mods tout seul.</li>
      </ul>
    </details>
  {/if}

  <div class="folder">
    <input class="field" readonly value={folder} placeholder="Aucun dossier choisi" aria-label="Dossier des mods" />
    <button class="btn" type="button" onclick={pick} disabled={locked}>Choisir…</button>
  </div>
  {#if locked}
    <p class="muted small">Pour changer de dossier, désinstalle d'abord les mods de ce jeu.</p>
  {/if}

  {#if gameRoot && !locked}
    <p class="warning small" role="alert">
      <Icon name="warning" size={16} />
      <span>
        Ce dossier ressemble au dossier d'installation du jeu (il contient son programme). Les jeux chargent rarement
        leurs mods à cet endroit : vérifie que c'est bien le dossier des mods.
      </span>
    </p>
  {/if}

  <fieldset class="layouts">
    <legend class="small strong">Rangement des fichiers</legend>
    {#each Object.entries(layoutLabels) as [value, label] (value)}
      <label class="layout">
        <input type="radio" name="layout-{gameId}" {value} bind:group={layout} />
        <span>
          <span class="strong">{label.title}</span>
          <span class="muted small">{label.help}</span>
        </span>
      </label>
    {/each}
  </fieldset>

  {#if error}<ErrorNotice {error} />{/if}

  <div class="actions">
    <button class="btn btn-primary" type="button" onclick={save} disabled={!folder || saving}>
      {saving ? "Vérification…" : gameRoot ? "Enregistrer quand même" : "Enregistrer"}
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

  .suggestion {
    display: grid;
    gap: 0.6rem;
    justify-items: start;
    padding: 0.8rem 0.9rem;
    border-radius: var(--r-md);
    background: var(--raise);
    border: 1px solid transparent;
  }

  .suggestion.active {
    border-color: var(--ok);
  }

  .suggestion p {
    display: grid;
    gap: 0.2rem;
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
    padding: 0.7rem 0.8rem;
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
