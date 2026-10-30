<script lang="ts">
  // Bandeau « nouvelle version de ModStudio », en bas à droite de la fenêtre.
  import { updates } from "$lib/updates.svelte";
  import Icon from "./Icon.svelte";
  import ProgressBar from "./ProgressBar.svelte";
</script>

{#if updates.available && (!updates.dismissed || updates.progress)}
  <aside class="update" aria-live="polite" aria-label="Mise à jour de ModStudio">
    <p class="title"><Icon name="download" size={16} />ModStudio {updates.available.version} est disponible</p>
    {#if updates.progress}
      <ProgressBar progress={updates.progress} />
      <p class="small muted">ModStudio redémarrera tout seul une fois la mise à jour installée.</p>
    {:else}
      <p class="small muted">
        Tu as la version {updates.current ?? updates.available.currentVersion}. Tes réglages, ta clé Nexus et tes mods
        installés sont gardés.
      </p>
      {#if updates.error}<p class="small error">La mise à jour a échoué : {updates.error}</p>{/if}
      <div class="row">
        <button class="btn btn-primary" onclick={() => updates.install()}>Mettre à jour</button>
        <button class="btn btn-ghost" onclick={() => (updates.dismissed = true)}>Plus tard</button>
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
