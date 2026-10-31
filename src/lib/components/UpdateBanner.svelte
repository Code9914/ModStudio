<script lang="ts">
  // Bandeau « nouvelle version de ModStudio », en bas à droite de la fenêtre.
  import { updates } from "$lib/updates.svelte";
  import { english, tr } from "$lib/i18n.svelte";
  import Icon from "./Icon.svelte";
  import ProgressBar from "./ProgressBar.svelte";
</script>

{#if updates.available && (!updates.dismissed || updates.progress)}
  <aside class="update" aria-live="polite" aria-label={tr("Mise à jour de ModStudio", "ModStudio update")}>
    <p class="title"><Icon name="download" size={16} />{tr(
        `ModStudio ${updates.available.version} est disponible`,
        `ModStudio ${updates.available.version} is available`,
      )}
    </p>
    {#if updates.progress}
      <ProgressBar progress={updates.progress} />
      <p class="small muted">
        {tr(
          "ModStudio redémarrera tout seul une fois la mise à jour installée.",
          "ModStudio will restart on its own once the update is installed.",
        )}
      </p>
    {:else if updates.packageManager}
      <p class="small muted">
        {#if english}
          You have version {updates.current ?? updates.available.currentVersion}, installed from the AUR: update it with
          your package manager (for example <code>paru -Syu</code> or <code>yay -Syu</code>).
        {:else}
          Tu as la version {updates.current ?? updates.available.currentVersion}, installée depuis l'AUR : mets-la à jour
          avec ton gestionnaire de paquets (par exemple <code>paru -Syu</code> ou <code>yay -Syu</code>).
        {/if}
      </p>
      <div class="row">
        <button class="btn btn-ghost" onclick={() => (updates.dismissed = true)}>{tr("Compris", "Got it")}</button>
      </div>
    {:else}
      <p class="small muted">
        {tr(
          `Tu as la version ${updates.current ?? updates.available.currentVersion}. Tes réglages, ta clé Nexus et tes mods installés sont gardés.`,
          `You have version ${updates.current ?? updates.available.currentVersion}. Your settings, Nexus key and installed mods are kept.`,
        )}
      </p>
      {#if updates.error}<p class="small error">
          {tr(`La mise à jour a échoué : ${updates.error}`, `The update failed: ${updates.error}`)}
        </p>{/if}
      <div class="row">
        <button class="btn btn-primary" onclick={() => updates.install()}>{tr("Mettre à jour", "Update")}</button>
        <button class="btn btn-ghost" onclick={() => (updates.dismissed = true)}>{tr("Plus tard", "Later")}</button>
      </div>
    {/if}
  </aside>
{/if}

<style>
  .update {
    position: fixed;
    right: 1.25rem;
    bottom: 1.25rem;
    z-index: 50;
    width: min(22rem, calc(100vw - 2.5rem));
    display: grid;
    gap: 0.6rem;
    padding: 1rem 1.1rem;
    border-radius: var(--r-md);
    background: var(--panel);
    border: 1px solid var(--raise);
    border-left: 3px solid var(--accent-fill);
    box-shadow: 0 12px 32px rgb(0 0 0 / 0.35);
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

  .small {
    font-size: var(--t-sm);
  }

  .error {
    color: var(--danger);
  }

  .row {
    display: flex;
    gap: 0.4rem;
  }
</style>
