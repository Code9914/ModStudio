<script lang="ts">
  import { onMount } from "svelte";
  import { api, asAppError, type AppError } from "$lib/api";
  import { nav } from "$lib/nav.svelte";
  import ErrorNotice from "$lib/components/ErrorNotice.svelte";
  import GamesView from "$lib/components/GamesView.svelte";
  import Icon from "$lib/components/Icon.svelte";
  import ModsView from "$lib/components/ModsView.svelte";
  import ModView from "$lib/components/ModView.svelte";
  import SettingsView from "$lib/components/SettingsView.svelte";

  let startError = $state<AppError | null>(null);

  async function start() {
    startError = null;
    try {
      nav.settings = await api.getSettings();
      if (!nav.settings.hasApiKey) nav.openSettings();
    } catch (e) {
      startError = asAppError(e);
    }
  }

  onMount(start);

  function onKey(event: KeyboardEvent) {
    const typing = (event.target as HTMLElement)?.closest("input, textarea, select");
    if (typing) return;
    if ((event.key === "Escape" || (event.altKey && event.key === "ArrowLeft")) && nav.view.name !== "games") {
      if (nav.view.name === "settings") nav.openGames();
      else nav.back();
    }
  }

  const ready = $derived(nav.settings?.hasApiKey === true);
</script>

<svelte:window onkeydown={onKey} />

<div class="app">
  <nav class="rail" aria-label="Navigation principale">
    <p class="wordmark">Atelier</p>

    <ul class="nav">
      <li>
        <button
          class="nav-item"
          aria-current={nav.view.name === "games" ? "page" : undefined}
          disabled={!ready}
          onclick={() => nav.openGames()}
        >
          <Icon name="grid" size={18} />Jeux
        </button>
      </li>
      {#if nav.game}
        <li>
          <button
            class="nav-item sub"
            aria-current={nav.view.name === "mods" || nav.view.name === "mod" ? "page" : undefined}
            onclick={() => nav.game && nav.openGame(nav.game)}
          >
            {#if nav.game.icon?.thumb_64x64}
              <img src={nav.game.icon.thumb_64x64} alt="" width="18" height="18" />
            {/if}
            <span class="ellipsis">{nav.game.name}</span>
          </button>
        </li>
      {/if}
      <li>
        <button
          class="nav-item"
          aria-current={nav.view.name === "settings" ? "page" : undefined}
          onclick={() => nav.openSettings()}
        >
          <Icon name="gear" size={18} />Réglages
        </button>
      </li>
    </ul>

    <p class="version muted">Version 0.1, client non officiel</p>
  </nav>

  <main class="stage">
    {#if startError}
      <div class="pane padded"><ErrorNotice error={startError} onretry={start} /></div>
    {:else if nav.settings}
      {#if ready}
        {#key nav.session}
          <div class="pane" hidden={nav.view.name !== "games"}><GamesView /></div>
          {#if nav.game}
            {#key nav.game.id}
              <div class="pane" hidden={nav.view.name !== "mods"}><ModsView game={nav.game} /></div>
            {/key}
          {/if}
        {/key}
        {#if nav.view.name === "mod" && nav.game}
          <div class="pane"><ModView game={nav.game} modId={nav.view.modId} /></div>
        {/if}
      {/if}
      {#if nav.view.name === "settings"}
        <div class="pane"><SettingsView /></div>
      {/if}
    {/if}
  </main>
</div>

<style>
  .app {
    display: grid;
    grid-template-columns: 13.5rem minmax(0, 1fr);
    height: 100vh;
  }

  .rail {
    display: flex;
    flex-direction: column;
    gap: 1.5rem;
    padding: 1.5rem 0.9rem 1.25rem;
    background: var(--panel);
    min-width: 0;
  }

  .wordmark {
    font-family: var(--font-display);
    font-weight: 800;
    font-size: var(--t-2xl);
    letter-spacing: -0.03em;
    padding: 0 0.6rem;
  }

  .nav {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 0.2rem;
  }

  .nav-item {
    display: flex;
    align-items: center;
    gap: 0.65rem;
    width: 100%;
    padding: 0.55rem 0.6rem;
    border: 0;
    border-radius: var(--r-sm);
    background: none;
    color: var(--muted);
    font-weight: 600;
    font-size: var(--t-md);
    text-align: left;
  }

  .nav-item:hover:not(:disabled) {
    color: var(--text);
  }

  .nav-item:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }

  .nav-item[aria-current="page"] {
    background: var(--raise);
    color: var(--text);
    box-shadow: inset 3px 0 0 var(--accent-fill);
  }

  .nav-item.sub {
    padding-left: 1.6rem;
    font-weight: 500;
  }

  .nav-item img {
    border-radius: 3px;
    flex: none;
  }

  .ellipsis {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .version {
    margin-top: auto;
    padding: 0 0.6rem;
    font-size: var(--t-xs);
  }

  .stage {
    position: relative;
    min-width: 0;
    overflow: hidden;
  }

  .pane {
    position: absolute;
    inset: 0;
  }

  .pane.padded {
    padding: 2.5rem;
  }
</style>
