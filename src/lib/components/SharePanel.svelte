<script lang="ts">
  // Partager les mods activés d'un jeu : un code à coller, ou un fichier.
  import { onMount } from "svelte";
  import { save } from "@tauri-apps/plugin-dialog";
  import { asAppError, share, type AppError, type GameInstall, type ShareExport, type ShareSource } from "$lib/api";
  import { copyText, fileNameFor, profileFilters } from "$lib/share.svelte";
  import { english, tr } from "$lib/i18n.svelte";
  import ErrorNotice from "./ErrorNotice.svelte";
  import Icon from "./Icon.svelte";

  let { source, game, onclose }: { source: ShareSource; game: GameInstall; onclose: () => void } = $props();

  let result = $state<ShareExport | null>(null);
  let error = $state<AppError | null>(null);
  let copied = $state<"code" | "r2" | null>(null);
  let saved = $state<string | null>(null);

  // Code r2modman (jeux Thunderstore) : publié sur Thunderstore à la demande.
  // Publiés pour tous : pas de réglages sans le demander (certains mods y gardent un jeton).
  let withConfigs = $state(false);
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
    const path = await save({ title: tr("Enregistrer le profil", "Save profile"), defaultPath: fileNameFor(result.profile.name), filters: profileFilters.save });
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

<section class="share" aria-label={tr(`Partager les mods de ${game.gameName}`, `Share ${game.gameName} mods`)}>
  <div class="head">
    <p class="title"><Icon name="share" size={16} />{tr("Partager ces mods", "Share these mods")}</p>
    <button class="btn btn-ghost close" onclick={onclose} aria-label={tr("Fermer", "Close")}><Icon name="close" size={15} /></button>
  </div>

  {#if error && !result}
    <ErrorNotice {error} />
  {:else if !result}
    <p class="small muted">{tr("Préparation du code…", "Preparing the code…")}</p>
  {:else}
    <p class="small">
      {#if english}
        Profile "{result.profile.name}": {result.profile.mods.length}
        {result.profile.mods.length === 1 ? "enabled mod" : "enabled mods"}{result.profile.mods.some((m) => m.dep) ? ", required mods included" : ""}.
        Send this code to a friend: in ModStudio, they paste it in "Installed", then "Import a profile", and everything
        installs in one go{source === "ts" || source === "cl" ? ", in the same versions" : ""}.
      {:else}
        Profil « {result.profile.name} » : {result.profile.mods.length}
        {result.profile.mods.length > 1 ? "mods activés" : "mod activé"}{result.profile.mods.some((m) => m.dep) ? ", mods nécessaires compris" : ""}.
        Envoie ce code à un ami : dans ModStudio, il le colle dans « Installés », puis « Importer un profil », et tout
        s'installe d'un coup{source === "ts" || source === "cl" ? ", dans les mêmes versions" : ""}.
      {/if}
    </p>
    <div class="code-row">
      <label class="visually-hidden" for="share-code-{source}-{game.gameId}">{tr("Code du profil", "Profile code")}</label>
      <textarea id="share-code-{source}-{game.gameId}" class="field code" readonly rows="4" value={result.code} onfocus={(e) => e.currentTarget.select()}></textarea>
    </div>
    <div class="row">
      <button class="btn btn-primary" onclick={() => copy(result!.code, "code")}>{copied === "code" ? tr("Copié", "Copied") : tr("Copier le code", "Copy code")}</button>
      <button class="btn" onclick={saveFile}>{tr("Enregistrer dans un fichier…", "Save to a file…")}</button>
    </div>
    {#if saved}<p class="small ok" role="status">{tr(`Profil enregistré : ${saved}`, `Profile saved: ${saved}`)}</p>{/if}
    {#if error}<ErrorNotice {error} />{/if}
    {#if result.leftOut > 0}
      <p class="small muted">
        {result.leftOut > 1
          ? tr(`${result.leftOut} mods désactivés ne font pas partie du profil.`, `${result.leftOut} disabled mods aren't part of the profile.`)
          : tr("Un mod désactivé ne fait pas partie du profil.", "One disabled mod isn't part of the profile.")}
      </p>
    {/if}
    {#if source === "nexus"}
      <p class="small muted">
        {tr(
          "Ton ami aura besoin de sa clé API Nexus Mods ; sans compte Premium, la page de chaque fichier s'ouvre pour qu'il clique sur « Slow download ».",
          'Your friend will need their own Nexus Mods API key; without a Premium account, each file\'s page opens so they can click "Slow download".',
        )}
      </p>
    {/if}

    {#if source === "ts"}
      <div class="r2">
        <p class="label">{tr("Pour r2modman et Gale", "For r2modman and Gale")}</p>
        <p class="small muted">
          {tr(
            "Un code r2modman marche aussi dans r2modman, Gale et Thunderstore Mod Manager. Comme avec r2modman, le profil est publié sur Thunderstore (sans compte) pour que ton ami le récupère.",
            "An r2modman code also works in r2modman, Gale and Thunderstore Mod Manager. As with r2modman, the profile is published on Thunderstore (no account needed) so your friend can fetch it.",
          )}
          {#if rmlCount > 0}
            {tr(
              "Les mods de ResoniteModLoader n'y sont pas : r2modman ne les connaît pas.",
              "ResoniteModLoader mods aren't included: r2modman doesn't know about them.",
            )}
          {/if}
        </p>
        {#if r2Code}
          <div class="row">
            <code class="r2-code">{r2Code}</code>
            <button class="btn" onclick={() => copy(r2Code!, "r2")}>{copied === "r2" ? tr("Copié", "Copied") : tr("Copier", "Copy")}</button>
          </div>
        {:else}
          <label class="check small">
            <input type="checkbox" bind:checked={withConfigs} />
            {tr("Joindre les réglages des mods (dossier BepInEx/config)", "Include the mods' settings (BepInEx/config folder)")}
          </label>
          {#if withConfigs}
            <p class="small warn">
              {tr(
                "Les réglages sont publiés avec le profil : vérifie qu'ils ne contiennent rien de personnel (jeton Twitch, lien Discord…).",
                "The settings are published with the profile: check that they contain nothing personal (Twitch token, Discord link…).",
              )}
            </p>
          {/if}
          <div>
            <button class="btn" onclick={makeR2} disabled={r2Busy}>{r2Busy ? tr("Publication…", "Publishing…") : tr("Créer un code r2modman", "Create an r2modman code")}</button>
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

  .warn {
    color: var(--warn, var(--accent));
  }

  .ok {
    color: var(--ok);
    overflow-wrap: anywhere;
  }
</style>
