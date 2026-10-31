<script lang="ts">
  // Partager les mods activés d'un jeu : un code à coller, ou un fichier.
  import { onMount } from "svelte";
  import { save } from "@tauri-apps/plugin-dialog";
  import { asAppError, share, type AppError, type GameInstall, type ShareExport, type ShareSource } from "$lib/api";
  import { copyText, fileNameFor, profileFilters } from "$lib/share.svelte";
  import ErrorNotice from "./ErrorNotice.svelte";
  import Icon from "./Icon.svelte";

  let { source, game, onclose }: { source: ShareSource; game: GameInstall; onclose: () => void } = $props();

  let result = $state<ShareExport | null>(null);
  let error = $state<AppError | null>(null);
  let copied = $state<"code" | "r2" | null>(null);
  let saved = $state<string | null>(null);

  // Code r2modman (jeux Thunderstore) : publié sur Thunderstore à la demande.
  let withConfigs = $state(true);
  let r2Code = $state<string | null>(null);
  let r2Busy = $state(false);
  let r2Error = $state<AppError | null>(null);

  onMount(async () => {
    try {
      result = await share.export(source, game.gameId);
    } catch (e) {
      error = asAppError(e);
    }
  });

  const rmlCount = $derived(result?.profile.mods.filter((m) => m.id.startsWith("rml:")).length ?? 0);

  async function copy(text: string, what: "code" | "r2") {
    if (await copyText(text)) {
      copied = what;
      setTimeout(() => (copied = null), 2000);
    }
  }

  async function saveFile() {
    if (!result) return;
    error = null;
    saved = null;
    const path = await save({ title: "Enregistrer le profil", defaultPath: fileNameFor(result.profile.name), filters: profileFilters.save });
    if (!path) return;
    try {
      await share.saveFile(path, result.profile);
      saved = path;
    } catch (e) {
      error = asAppError(e);
    }
  }

  async function makeR2() {
    if (!result) return;
    r2Busy = true;
    r2Error = null;
    try {
      r2Code = await share.r2modman(game.gameId, result.profile.name, withConfigs);
    } catch (e) {
      r2Error = asAppError(e);
    } finally {
      r2Busy = false;
    }
  }
</script>

<section class="share" aria-label="Partager les mods de {game.gameName}">
  <div class="head">
    <p class="title"><Icon name="share" size={16} />Partager ces mods</p>
    <button class="btn btn-ghost close" onclick={onclose} aria-label="Fermer"><Icon name="close" size={15} /></button>
  </div>

  {#if error && !result}
    <ErrorNotice {error} />
  {:else if !result}
    <p class="small muted">Préparation du code…</p>
  {:else}
    <p class="small">
      Profil « {result.profile.name} » : {result.profile.mods.length}
      {result.profile.mods.length > 1 ? "mods activés" : "mod activé"}{result.profile.mods.some((m) => m.dep) ? ", mods nécessaires compris" : ""}.
      Envoie ce code à un ami : dans ModStudio, il le colle dans « Installés », puis « Importer un profil », et tout
      s'installe d'un coup{source === "ts" || source === "cl" ? ", dans les mêmes versions" : ""}.
    </p>
    <div class="code-row">
      <label class="visually-hidden" for="share-code-{source}-{game.gameId}">Code du profil</label>
      <textarea id="share-code-{source}-{game.gameId}" class="field code" readonly rows="4" value={result.code} onfocus={(e) => e.currentTarget.select()}></textarea>
    </div>
    <div class="row">
      <button class="btn btn-primary" onclick={() => copy(result!.code, "code")}>{copied === "code" ? "Copié" : "Copier le code"}</button>
      <button class="btn" onclick={saveFile}>Enregistrer dans un fichier…</button>
    </div>
    {#if saved}<p class="small ok" role="status">Profil enregistré : {saved}</p>{/if}
    {#if error}<ErrorNotice {error} />{/if}
    {#if result.leftOut > 0}
      <p class="small muted">
        {result.leftOut > 1 ? `${result.leftOut} mods désactivés ne font pas partie du profil.` : "Un mod désactivé ne fait pas partie du profil."}
      </p>
    {/if}
    {#if source === "nexus"}
      <p class="small muted">
        Ton ami aura besoin de sa clé API Nexus Mods ; sans compte Premium, la page de chaque fichier s'ouvre pour qu'il
        clique sur « Slow download ».
      </p>
    {/if}

    {#if source === "ts"}
      <div class="r2">
        <p class="label">Pour r2modman et Gale</p>
        <p class="small muted">
          Un code r2modman marche aussi dans r2modman, Gale et Thunderstore Mod Manager. Comme avec r2modman, le profil
          est publié sur Thunderstore (sans compte) pour que ton ami le récupère.
          {#if rmlCount > 0}
            Les mods de ResoniteModLoader n'y sont pas : r2modman ne les connaît pas.
          {/if}
        </p>
        {#if r2Code}
          <div class="row">
            <code class="r2-code">{r2Code}</code>
            <button class="btn" onclick={() => copy(r2Code!, "r2")}>{copied === "r2" ? "Copié" : "Copier"}</button>
          </div>
        {:else}
          <label class="check small">
            <input type="checkbox" bind:checked={withConfigs} />
            Joindre les réglages des mods (dossier BepInEx/config)
          </label>
          <div>
            <button class="btn" onclick={makeR2} disabled={r2Busy}>{r2Busy ? "Publication…" : "Créer un code r2modman"}</button>
          </div>
        {/if}
        {#if r2Error}<ErrorNotice error={r2Error} />{/if}
      </div>
    {/if}
  {/if}
</section>

<style>
  .share {
    display: grid;
    gap: 0.65rem;
    max-width: 44rem;
    padding: 1rem 1.1rem;
    margin-bottom: 0.9rem;
    border-radius: var(--r-md);
    background: var(--panel);
    border-left: 3px solid var(--accent-fill);
  }

  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
  }

  .title {
    display: flex;
    align-items: center;
    gap: 0.45rem;
    font-weight: 700;
  }

  .title :global(svg) {
    color: var(--accent);
  }

  .close {
    padding: 0.3rem;
  }

  .code {
    width: 100%;
    font-family: var(--font-mono, ui-monospace, monospace);
    font-size: var(--t-xs);
    word-break: break-all;
    resize: vertical;
  }

  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.4rem;
  }

  .r2 {
    display: grid;
    gap: 0.5rem;
    padding-top: 0.7rem;
    border-top: 1px solid var(--line);
  }

  .label {
    font-size: var(--t-xs);
    font-weight: 700;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--muted);
  }

  .r2-code {
    padding: 0.3em 0.6em;
    border-radius: var(--r-sm);
    background: var(--ink);
    font-size: var(--t-sm);
    user-select: all;
    overflow-wrap: anywhere;
  }

  .check {
    display: flex;
    align-items: center;
    gap: 0.45rem;
  }

  .small {
    font-size: var(--t-sm);
  }

  .ok {
    color: var(--ok);
    overflow-wrap: anywhere;
  }
</style>
