<script lang="ts">
  import { onMount } from "svelte";
  import { api, asAppError, type AppError } from "$lib/api";
  import { installs } from "$lib/installs.svelte";
  import { nav } from "$lib/nav.svelte";
  import AccountView from "$lib/components/AccountView.svelte";
  import ErrorNotice from "$lib/components/ErrorNotice.svelte";
  import GamesView from "$lib/components/GamesView.svelte";
  import Icon from "$lib/components/Icon.svelte";
  import InstalledView from "$lib/components/InstalledView.svelte";
  import LibraryView from "$lib/components/LibraryView.svelte";
  import ModsView from "$lib/components/ModsView.svelte";
  import ModView from "$lib/components/ModView.svelte";
  import NexusAccountView from "$lib/components/NexusAccountView.svelte";
  import NexusGamesView from "$lib/components/NexusGamesView.svelte";
  import NexusModsView from "$lib/components/NexusModsView.svelte";
  import NexusModView from "$lib/components/NexusModView.svelte";
  import NexusSite from "$lib/components/NexusSite.svelte";
  import SettingsView from "$lib/components/SettingsView.svelte";
  import { nexusGameImage } from "$lib/api";
  import { nexusStore, welcomeDone } from "$lib/nexus.svelte";

  let startError = $state<AppError | null>(null);

  async function start() {
    startError = null;
    // Nexus Mods fonctionne même sans clé mod.io.
    nexusStore.listenNxm();
    checkNexus();
    try {
      const [settings] = await Promise.all([api.getSettings(), nexusStore.refreshAuth()]);
      nav.settings = settings;
      if (!nexusStore.ready && !welcomeDone()) {
        // Premier lancement : connexion à Nexus Mods d'abord.
        nav.openNexusWelcome();
      } else if (!settings.hasApiKey) {
        if (nexusStore.ready) nav.openNexusGames();
        else nav.openSettings();
      }
      if (settings.hasApiKey) {
        nav.refreshAuth();
        checkInstalled();
      }
    } catch (e) {
      startError = asAppError(e);
    }
  }

  async function checkNexus() {
    try {
      await nexusStore.load();
      await Promise.all(Object.keys(nexusStore.games).map((id) => nexusStore.checkUpdates(Number(id))));
    } catch {
      /* la page Installés affichera l'erreur */
    }
  }

  /** Charge les mods installés et cherche leurs mises à jour, sans bloquer. */
  async function checkInstalled() {
    try {
      await installs.load();
      await Promise.all(Object.keys(installs.games).map((id) => installs.checkUpdates(Number(id))));
    } catch {
      /* la page Installés affichera l'erreur */
    }
  }

  const updates = $derived(
    Object.values(installs.games).reduce(
      (n, g) => n + Object.values(g.mods).filter((m) => installs.outdated(g.gameId, m.modId)).length,
      0,
    ) +
      Object.values(nexusStore.games).reduce(
        (n, g) => n + Object.values(g.mods).filter((m) => nexusStore.outdated(g.gameId, m.modId)).length,
        0,
      ),
  );

  onMount(start);

  function onKey(event: KeyboardEvent) {
    const typing = (event.target as HTMLElement)?.closest("input, textarea, select");
    if (typing) return;
    if ((event.key === "Escape" || (event.altKey && event.key === "ArrowLeft")) && nav.view.name !== "games") {
      const v = nav.view.name;
      if (v === "mod" || v === "mods" || v === "nexusMod" || v === "nexusMods") nav.back();
      else if (v === "nexusAccount") nav.openNexusGames();
      else if (v !== "nexusGames") nav.openGames();
    }
  }

  const ready = $derived(nav.settings?.hasApiKey === true);
  const user = $derived(nav.auth?.user ?? null);
  const avatar = $derived(user?.avatar?.thumb_50x50 ?? user?.avatar?.thumb_100x100 ?? user?.avatar?.original ?? null);
</script>

<svelte:window onkeydown={onKey} />

<div class="app">
  <nav class="rail" aria-label="Navigation principale">
    <p class="wordmark">ModStudio</p>

    <p class="section">mod.io</p>
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
            aria-current={nav.view.name === "mods" || (nav.view.name === "mod" && nav.modOrigin === "mods")
              ? "page"
              : undefined}
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
          aria-current={nav.view.name === "library" || (nav.view.name === "mod" && nav.modOrigin === "library")
            ? "page"
            : undefined}
          disabled={!ready}
          onclick={() => nav.openLibrary()}
        >
          <Icon name="library" size={18} />Mes mods
        </button>
      </li>
    </ul>

    <p class="section">Nexus Mods</p>
    <ul class="nav">
      <li>
        <button
          class="nav-item"
          aria-label="Jeux Nexus Mods"
          aria-current={nav.view.name === "nexusGames" ? "page" : undefined}
          onclick={() => nav.openNexusGames()}
        >
          <Icon name="grid" size={18} />Jeux
        </button>
      </li>
      {#if nav.nexusGame}
        <li>
          <button
            class="nav-item sub"
            aria-current={nav.view.name === "nexusMods" || (nav.view.name === "nexusMod" && nav.nexusOrigin === "nexusMods")
              ? "page"
              : undefined}
            onclick={() => nav.nexusGame && nav.openNexusGame(nav.nexusGame)}
          >
            <img
              src={nexusGameImage(nav.nexusGame.id, "thumbnail")}
              alt=""
              width="18"
              height="18"
              onerror={(e) => ((e.currentTarget as HTMLImageElement).style.visibility = "hidden")}
            />
            <span class="ellipsis">{nav.nexusGame.name}</span>
          </button>
        </li>
      {/if}
    </ul>

    <ul class="nav">
      <li>
        <button
          class="nav-item"
          aria-current={nav.view.name === "installed" ||
          (nav.view.name === "mod" && nav.modOrigin === "installed") ||
          (nav.view.name === "nexusMod" && nav.nexusOrigin === "installed")
            ? "page"
            : undefined}
          onclick={() => nav.openInstalled()}
        >
          <Icon name="download" size={18} />Installés
          {#if updates > 0}<span class="badge" aria-label="{updates} mises à jour">{updates}</span>{/if}
        </button>
      </li>
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

    <div class="rail-foot">
      {#if ready}
        <button
          class="account"
          aria-current={nav.view.name === "account" ? "page" : undefined}
          aria-label={nav.loggedIn ? "Mon compte mod.io" : "Se connecter à mod.io"}
          onclick={() => nav.openAccount()}
        >
          {#if nav.loggedIn}
            {#if avatar}
              <img class="account-avatar" src={avatar} alt="" />
            {:else}
              <span class="account-avatar placeholder"><Icon name="user" size={16} /></span>
            {/if}
            <span class="account-text">
              <span class="ellipsis account-name">{user?.username ?? user?.display_name_portal ?? "Connecté"}</span>
              <span class="account-sub">Compte mod.io</span>
            </span>
          {:else}
            <span class="account-avatar placeholder"><Icon name="user" size={16} /></span>
            <span class="account-text">
              <span class="account-name">Se connecter</span>
              <span class="account-sub">pour s'abonner</span>
            </span>
          {/if}
        </button>
      {/if}
      <button
        class="account"
        aria-current={nav.view.name === "nexusAccount" ? "page" : undefined}
        aria-label={nexusStore.ready ? "Mon compte Nexus Mods" : "Ajouter ma clé Nexus Mods"}
        onclick={() => nav.openNexusAccount()}
      >
        <span class="account-avatar placeholder nexus"><Icon name="user" size={16} /></span>
        <span class="account-text">
          {#if nexusStore.ready}
            <span class="ellipsis account-name">{nexusStore.auth?.user?.name ?? "Clé enregistrée"}</span>
            <span class="account-sub">Nexus Mods{nexusStore.premium ? ", Premium" : ""}</span>
          {:else}
            <span class="account-name">Nexus Mods</span>
            <span class="account-sub">ajouter ma clé</span>
          {/if}
        </span>
      </button>
      <p class="version muted">Version 0.5, client non officiel</p>
    </div>
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
          <div class="pane" hidden={nav.view.name !== "library"}><LibraryView /></div>
          <div class="pane" hidden={nav.view.name !== "installed"}><InstalledView /></div>
        {/key}
        {#if nav.view.name === "mod" && nav.game}
          <div class="pane"><ModView game={nav.game} modId={nav.view.modId} /></div>
        {/if}
        {#if nav.view.name === "account"}
          <div class="pane"><AccountView /></div>
        {/if}
      {/if}
      {#key nav.session}
        <div class="pane" hidden={nav.view.name !== "nexusGames"}><NexusGamesView /></div>
        {#if nav.nexusGame}
          {#key nav.nexusGame.id}
            <div class="pane" hidden={nav.view.name !== "nexusMods"}><NexusModsView game={nav.nexusGame} /></div>
          {/key}
        {/if}
        {#if !ready}
          <div class="pane" hidden={nav.view.name !== "installed"}><InstalledView /></div>
        {/if}
      {/key}
      {#if nav.view.name === "nexusMod" && nav.nexusGame}
        <div class="pane"><NexusModView game={nav.nexusGame} modId={nav.view.modId} /></div>
      {/if}
      {#if nav.view.name === "nexusAccount"}
        <div class="pane"><NexusAccountView /></div>
      {/if}
      {#if nav.view.name === "settings"}
        <div class="pane"><SettingsView /></div>
      {/if}
    {/if}
    {#if nexusStore.site}
      {#key nexusStore.site}
        <NexusSite request={nexusStore.site} />
      {/key}
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
    grid-template-columns: minmax(0, 1fr);
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

  .nav-item[aria-current="page"],
  .account[aria-current="page"] {
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

  .section {
    margin: 0 0 -1rem;
    padding: 0 0.6rem;
    font-size: var(--t-xs);
    font-weight: 700;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--muted);
  }

  .account-avatar.nexus {
    color: var(--accent);
  }

  .ellipsis {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }

  .rail-foot {
    margin-top: auto;
    display: grid;
    gap: 0.9rem;
  }

  .account {
    display: flex;
    align-items: center;
    gap: 0.65rem;
    width: 100%;
    padding: 0.5rem 0.6rem;
    border: 0;
    border-radius: var(--r-sm);
    background: none;
    text-align: left;
    color: var(--text);
  }

  .account:hover .account-name {
    color: var(--accent);
  }

  .account-avatar {
    width: 2rem;
    height: 2rem;
    border-radius: 50%;
    object-fit: cover;
    flex: none;
    background: var(--raise);
  }

  .account-avatar.placeholder {
    display: grid;
    place-items: center;
    color: var(--muted);
  }

  .account-text {
    display: grid;
    min-width: 0;
    line-height: 1.25;
  }

  .account-name {
    font-weight: 600;
    font-size: var(--t-sm);
  }

  .account-sub {
    font-size: var(--t-xs);
    color: var(--muted);
  }

  .version {
    padding: 0 0.6rem;
    font-size: var(--t-xs);
  }

  .badge {
    margin-left: auto;
    min-width: 1.3rem;
    padding: 0 0.35rem;
    border-radius: 999px;
    background: var(--accent-fill);
    color: var(--on-accent);
    font-size: var(--t-xs);
    font-weight: 700;
    text-align: center;
    line-height: 1.3rem;
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
