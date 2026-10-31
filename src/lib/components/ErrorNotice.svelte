<script lang="ts">
  import { onDestroy } from "svelte";
  import { asAppError, type AppError } from "$lib/api";
  import { ownerNames, stepAside, type ModSource } from "$lib/conflicts";
  import { nav } from "$lib/nav.svelte";
  import { nexusStore } from "$lib/nexus.svelte";
  import { tr } from "$lib/i18n.svelte";
  import Icon from "./Icon.svelte";

  let {
    error,
    onretry,
    source,
    redo,
  }: {
    error: AppError;
    onretry?: () => void;
    /** Bibliothèque de l'action qui a échoué : permet de lever un conflit de fichiers. */
    source?: ModSource;
    /** Action à refaire une fois les mods en conflit désactivés (sinon `onretry`). */
    redo?: (() => void) | null;
  } = $props();

  // Conflit de fichiers avec d'autres mods de la même source : on peut les désactiver.
  const owners = $derived(error.kind === "conflict" || error.kind === "incompatible" ? (error.conflictWith ?? []) : []);
  const again = $derived(redo ?? onretry ?? null);
  const canStepAside = $derived(owners.length > 0 && !!source && !!again);
  let resolving = $state(false);
  let stepError = $state<string | null>(null);

  async function resolve() {
    if (!source || !again) return;
    resolving = true;
    stepError = null;
    try {
      await stepAside(source, owners);
      again();
    } catch (e) {
      stepError = asAppError(e).message;
    } finally {
      resolving = false;
    }
  }

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
  const needsLogin = $derived(error.kind === "not_logged_in" || error.kind === "session_expired");
  const needsNexusKey = $derived(error.kind === "nexus_key_missing" || error.kind === "nexus_key_invalid");

  // Une session expirée change l'état de connexion affiché partout.
  $effect(() => {
    if (error.kind === "session_expired") nav.refreshAuth();
    if (error.kind === "nexus_key_invalid") nexusStore.refreshAuth();
  });
</script>

<div class="notice" role="alert">
  <span class="icon"><Icon name="warning" size={20} /></span>
  <div class="body">
    <p>{error.message}</p>
    {#if error.kind === "rate_limited" && remaining > 0}
      <p class="muted">{tr(`Nouvel essai possible dans ${remaining} s.`, `You can try again in ${remaining} s.`)}</p>
    {/if}
    {#if canStepAside && error.kind === "incompatible"}
      <p class="muted">
        {owners.length > 1
          ? tr(
              "Tu peux les désactiver (sans les désinstaller) : ils restent dans « Installés », prêts à être réactivés.",
              'You can disable them (without uninstalling): they stay in "Installed", ready to be enabled again.',
            )
          : tr(
              "Tu peux le désactiver (sans le désinstaller) : il reste dans « Installés », prêt à être réactivé.",
              'You can disable it (without uninstalling): it stays in "Installed", ready to be enabled again.',
            )}
      </p>
      {#if stepError}<p class="step-error">{stepError}</p>{/if}
    {:else if canStepAside}
      <p class="muted">
        {owners.length > 1
          ? tr(
              `Ces fichiers sont à ${ownerNames(owners)}. Tu peux les désactiver (sans les désinstaller) pour faire de la place : ils restent dans « Installés », prêts à être réactivés.`,
              `These files belong to ${ownerNames(owners)}. You can disable them (without uninstalling) to make room: they stay in "Installed", ready to be enabled again.`,
            )
          : tr(
              `Ces fichiers sont à ${ownerNames(owners)}. Tu peux le désactiver (sans le désinstaller) pour faire de la place : il reste dans « Installés », prêt à être réactivé.`,
              `These files belong to ${ownerNames(owners)}. You can disable it (without uninstalling) to make room: it stays in "Installed", ready to be enabled again.`,
            )}
      </p>
      {#if stepError}<p class="step-error">{stepError}</p>{/if}
    {/if}
  </div>
  <div class="actions">
    {#if needsSettings}
      <button class="btn btn-primary" onclick={() => nav.openSettings()}>{tr("Ouvrir les réglages", "Open settings")}</button>
    {/if}
    {#if needsLogin}
      <button class="btn btn-primary" onclick={() => nav.openAccount(nav.view)}>{tr("Se connecter", "Sign in")}</button>
    {/if}
    {#if needsNexusKey && nav.view.name !== "nexusAccount"}
      <button class="btn btn-primary" onclick={() => nav.openNexusAccount(nav.view)}>{tr("Ma clé Nexus", "My Nexus key")}</button>
    {/if}
    {#if canStepAside}
      <button class="btn btn-primary" onclick={resolve} disabled={resolving}>
        {resolving
          ? tr("Désactivation…", "Disabling…")
          : owners.length > 1
            ? tr(`Les désactiver et réessayer`, `Disable them and try again`)
            : tr(`Désactiver « ${owners[0].name} » et réessayer`, `Disable "${owners[0].name}" and try again`)}
      </button>
    {/if}
    {#if onretry && error.kind !== "conflict" && error.kind !== "incompatible"}
      <button class="btn" onclick={onretry} disabled={remaining > 0}>{tr("Réessayer", "Try again")}</button>
    {/if}
  </div>
</div>

<style>
  .notice {
    display: flex;
    flex-wrap: wrap;
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

  .step-error {
    color: var(--danger);
    font-size: var(--t-sm);
  }

  .body {
    flex: 1 1 12rem;
    min-width: 0;
    overflow-wrap: anywhere;
    display: grid;
    gap: 0.2rem;
    font-size: var(--t-md);
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
    flex: 0 1 auto;
    min-width: 0;
  }

  /* Panneau étroit (fiche d'un mod) : les longs libellés passent à la ligne. */
  .actions :global(.btn) {
    white-space: normal;
    text-align: left;
  }
</style>
