<script lang="ts">
  import { untrack } from "svelte";
  import { open } from "@tauri-apps/plugin-dialog";
  import { asAppError, type AppError, type Layout } from "$lib/api";
  import { installs, layoutLabels } from "$lib/installs.svelte";
  import ErrorNotice from "./ErrorNotice.svelte";

  let {
    gameId,
    gameName,
    icon = null,
    onsaved,
    oncancel,
  }: {
    gameId: number;
    gameName: string;
    icon?: string | null;
    onsaved: () => void;
    oncancel: () => void;
  } = $props();

  const current = $derived(installs.game(gameId));
  const locked = $derived(current ? Object.keys(current.mods).length > 0 : false);

  // Valeurs de départ du formulaire (modifiables ensuite).
  let folder = $state(untrack(() => installs.game(gameId)?.folder ?? ""));
  let layout = $state<Layout>(untrack(() => installs.game(gameId)?.layout ?? "subfolder"));
  let saving = $state(false);
  let error = $state<AppError | null>(null);

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
      await installs.setFolder(gameId, gameName, icon, folder, layout);
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
  <p class="muted small">
    Choisis le dossier dans lequel le jeu charge ses mods. En cas de doute, regarde la page du jeu sur mod.io ou son
    guide de modding.
  </p>

  <div class="folder">
    <input class="field" readonly value={folder} placeholder="Aucun dossier choisi" aria-label="Dossier des mods" />
    <button class="btn" type="button" onclick={pick} disabled={locked}>Choisir…</button>
  </div>
  {#if locked}
    <p class="muted small">Pour changer de dossier, désinstalle d'abord les mods de ce jeu.</p>
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
      {saving ? "Vérification…" : "Enregistrer"}
    </button>
    <button class="btn btn-ghost" type="button" onclick={oncancel}>Annuler</button>
  </div>
</div>

<style>
  .setup {
    display: grid;
    gap: 0.8rem;
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

  .folder {
    display: flex;
    gap: 0.4rem;
  }

  .folder .field {
    font-size: var(--t-sm);
    text-overflow: ellipsis;
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
