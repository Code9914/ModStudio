<script lang="ts">
  import { onDestroy } from "svelte";
  import type { AppError } from "$lib/api";
  import { nav } from "$lib/nav.svelte";
  import Icon from "./Icon.svelte";

  let { error, onretry }: { error: AppError; onretry?: () => void } = $props();

  // Compte à rebours quand mod.io demande d'attendre.
  let remaining = $state(0);
  let timer: ReturnType<typeof setInterval> | undefined;

  $effect(() => {
    clearInterval(timer);
    const start = error.retryAfter ?? 0;
    remaining = start;
    if (start > 0) {
      timer = setInterval(() => {
        remaining = Math.max(0, remaining - 1);
        if (remaining === 0) clearInterval(timer);
      }, 1000);
    }
  });

  onDestroy(() => clearInterval(timer));

  const needsSettings = $derived(
    error.kind === "missing_api_key" || error.kind === "unauthorized" || error.kind === "invalid_api_path",
  );
</script>

<div class="notice" role="alert">
  <span class="icon"><Icon name="warning" size={20} /></span>
  <div class="body">
    <p>{error.message}</p>
    {#if error.kind === "rate_limited" && remaining > 0}
      <p class="muted">Nouvel essai possible dans {remaining} s.</p>
    {/if}
  </div>
  <div class="actions">
    {#if needsSettings}
      <button class="btn btn-primary" onclick={() => nav.openSettings()}>Ouvrir les réglages</button>
    {/if}
    {#if onretry}
      <button class="btn" onclick={onretry} disabled={remaining > 0}>Réessayer</button>
    {/if}
  </div>
</div>

<style>
  .notice {
    display: flex;
    align-items: flex-start;
    gap: 0.9rem;
    padding: 1rem 1.1rem;
    border-radius: var(--r-md);
    background: var(--danger-bg);
    max-width: 60rem;
  }

  .icon {
    color: var(--danger);
    flex: none;
    padding-top: 0.1rem;
  }

  .body {
    flex: 1;
    display: grid;
    gap: 0.2rem;
    font-size: var(--t-md);
  }

  .actions {
    display: flex;
    gap: 0.5rem;
    flex: none;
  }
</style>
