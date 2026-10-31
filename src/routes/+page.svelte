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
  import ImportView from "$lib/components/ImportView.svelte";
  import LibraryView from "$lib/components/LibraryView.svelte";
  import ModsView from "$lib/components/ModsView.svelte";
  import ModView from "$lib/components/ModView.svelte";
  import NexusAccountView from "$lib/components/NexusAccountView.svelte";
  import NexusGamesView from "$lib/components/NexusGamesView.svelte";
  import NexusTrackedView from "$lib/components/NexusTrackedView.svelte";
  import NexusModsView from "$lib/components/NexusModsView.svelte";
  import NexusModView from "$lib/components/NexusModView.svelte";
  import NexusSite from "$lib/components/NexusSite.svelte";
  import SettingsView from "$lib/components/SettingsView.svelte";
  import TsGamesView from "$lib/components/TsGamesView.svelte";
  import TsModsView from "$lib/components/TsModsView.svelte";
  import TsModView from "$lib/components/TsModView.svelte";
  import RmlModView from "$lib/components/RmlModView.svelte";
  import ClGamesView from "$lib/components/ClGamesView.svelte";
  import ClModsView from "$lib/components/ClModsView.svelte";
  import ClModView from "$lib/components/ClModView.svelte";
  import { listsStore } from "$lib/lists.svelte";
  import { tsStore } from "$lib/thunderstore.svelte";
  import UpdateBanner from "$lib/components/UpdateBanner.svelte";
  import { updates as appUpdates } from "$lib/updates.svelte";
  import { appearance } from "$lib/appearance.svelte";
  import { nexusGameImage, type GameInstall } from "$lib/api";
  import { nexusGameOf, nexusStore, welcomeDone } from "$lib/nexus.svelte";

  let startError = $state<AppError | null>(null);

  async function start() {
    startError = null;
    // Nouvelle version de ModStudio ? (sans effet en mode développement)
    appUpdates.check();
    // Nexus Mods fonctionne même sans clé mod.io.
    nexusStore.listenNxm();
    checkNexus();
    checkThunderstore();
    checkLists();
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
      nexusStore.loadSocial();
      await Promise.all(Object.keys(nexusStore.games).map((id) => nexusStore.checkUpdates(Number(id))));
    } catch {
      /* la page Installés affichera l'erreur */
    }
  }

  async function checkThunderstore() {
    try {
      await tsStore.load();
      await Promise.all(Object.keys(tsStore.games).map((id) => tsStore.checkUpdates(Number(id))));
    } catch {
      /* la page Installés affichera l'erreur */
    }
  }

  async function checkLists() {
    try {
      await listsStore.load();
      await Promise.all(Object.keys(listsStore.installs).map((id) => listsStore.checkUpdates(Number(id))));
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
      ) +
      Object.values(tsStore.games).reduce(
        (n, g) => n + Object.values(g.mods).filter((m) => !m.disabled && tsStore.outdated(g.gameId, m.modId)).length,
        0,
      ) +
      Object.values(listsStore.installs).reduce(
        (n, g) => n + Object.values(g.mods).filter((m) => !m.disabled && listsStore.outdated(g.gameId, m.modId)).length,
        0,
      ),
  );

  onMount(start);

  // Retour sur ModStudio : jeux installés, outils ajoutés ou retirés entre-temps.
  function onReturn() {
    if (document.visibilityState === "visible") nexusStore.refreshLocal();
  }
  onMount(() => {
    window.addEventListener("focus", onReturn);
    document.addEventListener("visibilitychange", onReturn);
    // Et régulièrement tant que la fenêtre est ouverte (un jeu installé pendant qu'on regarde).
    const timer = setInterval(onReturn, 60_000);
    return () => {
      window.removeEventListener("focus", onReturn);
      document.removeEventListener("visibilitychange", onReturn);
      clearInterval(timer);
    };
  });

  function onKey(event: KeyboardEvent) {
    const typing = (event.target as HTMLElement)?.closest("input, textarea, select");
    if (typing) return;
    if ((event.key === "Escape" || (event.altKey && event.key === "ArrowLeft")) && nav.view.name !== "games") {
      const v = nav.view.name;
      if (v === "mod" || v === "mods" || v === "nexusMod" || v === "nexusMods" || v === "tsMod" || v === "tsMods" || v === "rmlMod" || v === "clMod" || v === "clMods" || v === "import") nav.back();
      else if (v === "nexusAccount" || v === "nexusTracked") nav.openNexusGames();
      else if (v !== "nexusGames" && v !== "tsGames" && v !== "clGames") nav.openGames();
    }
  }

  // Barre latérale : seulement les jeux qui ont au moins un mod installé
  // (un chargeur de mods compte comme un mod).
  const hasMods = (g: GameInstall) => Object.keys(g.mods).length > 0;
  const byName = (a: { name: string | null }, b: { name: string | null }) =>
    (a.name ?? "").localeCompare(b.name ?? "", "fr", { sensitivity: "base" });

  const modioRail = $derived(
    Object.values(installs.games)
      .filter(hasMods)
      .map((g) => ({ id: g.gameId, name: g.gameName, icon: g.icon }))
      .sort(byName),
  );

  const nexusRail = $derived(
    Object.values(nexusStore.games)
      .filter((g) => g.domain && nexusStore.count(g) > 0)
      .map(nexusGameOf)
      .sort(byName),
  );

  const tsRail = $derived(
    Object.values(tsStore.games)
      .filter((g) => g.domain && tsStore.count(g) > 0)
      .map((g) => tsStore.communityOf(g))
      .sort(byName),
  );

  const clRail = $derived(
    Object.values(listsStore.installs)
      .filter((g) => listsStore.count(g) > 0)
      .map((g) => listsStore.gameOf(g))
      .filter((g): g is NonNullable<typeof g> => !!g)
      .sort(byName),
  );
  const onClGame = $derived(nav.view.name === "clMods" || (nav.view.name === "clMod" && nav.clOrigin === "clMods"));
  const clGamesCurrent = $derived(
    nav.view.name === "clGames" || (onClGame && (!appearance.value.railGames || !clRail.some((g) => g.id === nav.clGame?.id))),
  );

  // Page d'un jeu (ou d'un de ses mods) ouverte depuis sa liste.
  const onModioGame = $derived(nav.view.name === "mods" || (nav.view.name === "mod" && nav.modOrigin === "mods"));
  const onNexusGame = $derived(
    nav.view.name === "nexusMods" || (nav.view.name === "nexusMod" && nav.nexusOrigin === "nexusMods"),
  );
  // Jeu sans mod ouvert, ou raccourcis des jeux masqués dans les réglages :
  // « Jeux » reste en surbrillance.
  const modioGamesCurrent = $derived(
    nav.view.name === "games" ||
      (onModioGame && (!appearance.value.railGames || !modioRail.some((g) => g.id === nav.game?.id))),
  );
  const nexusGamesCurrent = $derived(
    nav.view.name === "nexusGames" ||
      (onNexusGame && (!appearance.value.railGames || !nexusRail.some((g) => g.id === nav.nexusGame?.id))),
  );
  const onTsGame = $derived(
    nav.view.name === "tsMods" ||
      (nav.view.name === "tsMod" && nav.tsOrigin === "tsMods") ||
      (nav.view.name === "rmlMod" && nav.rmlOrigin === "tsMods"),
  );
  const tsGamesCurrent = $derived(
    nav.view.name === "tsGames" ||
      (onTsGame && (!appearance.value.railGames || !tsRail.some((g) => g.id === nav.tsGame?.id))),
  );

  /** Ouvre un jeu mod.io de la barre latérale (sa fiche est chargée au besoin). */
  async function openModioGame(id: number) {
    if (nav.game?.id === id) return nav.openGame(nav.game);
    try {
      nav.openGame(await api.getGame(id));
    } catch {
      // Hors ligne : ses mods restent accessibles dans « Installés ».
      nav.openInstalled();
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
          aria-current={modioGamesCurrent ? "page" : undefined}
          disabled={!ready}
          onclick={() => nav.openGames()}
        >
          <Icon name="grid" size={18} />Jeux
        </button>
      </li>
      {#if ready && appearance.value.railGames}
        {#each modioRail as g (g.id)}
          <li>
            <button
              class="nav-item sub"
              aria-current={onModioGame && nav.game?.id === g.id ? "page" : undefined}
              onclick={() => openModioGame(g.id)}
            >
              {#if g.icon}
                <img src={g.icon} alt="" width="18" height="18" />
              {:else}
                <span class="icon-gap" aria-hidden="true"></span>
              {/if}
              <span class="ellipsis">{g.name}</span>
            </button>
          </li>
        {/each}
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
          aria-current={nexusGamesCurrent ? "page" : undefined}
          onclick={() => nav.openNexusGames()}
        >
          <Icon name="grid" size={18} />Jeux
        </button>
      </li>
      {#if appearance.value.railGames}
        {#each nexusRail as g (g.id)}
          <li>
            <button
              class="nav-item sub"
              aria-current={onNexusGame && nav.nexusGame?.id === g.id ? "page" : undefined}
              onclick={() => nav.openNexusGame(nav.nexusGame?.id === g.id ? nav.nexusGame : g)}
            >
              <img
                src={nexusGameImage(g.id, "thumbnail")}
                alt=""
                width="18"
                height="18"
                onerror={(e) => ((e.currentTarget as HTMLImageElement).style.visibility = "hidden")}
              />
              <span class="ellipsis">{g.name}</span>
            </button>
          </li>
        {/each}
      {/if}
      {#if nexusStore.ready}
        <li>
          <button
            class="nav-item"
            aria-current={nav.view.name === "nexusTracked" || (nav.view.name === "nexusMod" && nav.nexusOrigin === "nexusTracked")
              ? "page"
              : undefined}
            onclick={() => nav.openNexusTracked()}
          >
            <Icon name="bell" size={18} />Mods suivis
          </button>
        </li>
      {/if}
    </ul>

    <p class="section">Thunderstore</p>
    <ul class="nav">
      <li>
        <button
          class="nav-item"
          aria-label="Jeux Thunderstore"
          aria-current={tsGamesCurrent ? "page" : undefined}
          onclick={() => nav.openTsGames()}
        >
          <Icon name="grid" size={18} />Jeux
        </button>
      </li>
      {#if appearance.value.railGames}
        {#each tsRail as g (g.id)}
          <li>
            <button
              class="nav-item sub"
              aria-current={onTsGame && nav.tsGame?.id === g.id ? "page" : undefined}
              onclick={() => nav.openTsGame(nav.tsGame?.id === g.id ? nav.tsGame : g)}
            >
              {#if g.communityIconUrl ?? g.coverImageUrl}
                <img
                  class="cover"
                  src={g.communityIconUrl ?? g.coverImageUrl}
                  alt=""
                  width="18"
                  height="18"
                  onerror={(e) => ((e.currentTarget as HTMLImageElement).style.visibility = "hidden")}
                />
              {:else}
                <span class="icon-gap" aria-hidden="true"></span>
              {/if}
              <span class="ellipsis">{g.name}</span>
            </button>
          </li>
        {/each}
      {/if}
    </ul>

    <p class="section">Communautés</p>
    <ul class="nav">
      <li>
        <button
          class="nav-item"
          aria-label="Jeux des communautés"
          aria-current={clGamesCurrent ? "page" : undefined}
          onclick={() => nav.openClGames()}
        >
          <Icon name="grid" size={18} />Jeux
        </button>
      </li>
      {#if appearance.value.railGames}
        {#each clRail as g (g.id)}
          <li>
            <button
              class="nav-item sub"
              aria-current={onClGame && nav.clGame?.id === g.id ? "page" : undefined}
              onclick={() => nav.openClGame(g)}
            >
              <img
                class="cover"
                src={`https://cdn.cloudflare.steamstatic.com/steam/apps/${g.steamAppId}/capsule_231x87.jpg`}
                alt=""
                width="18"
                height="18"
                onerror={(e) => ((e.currentTarget as HTMLImageElement).style.visibility = "hidden")}
              />
              <span class="ellipsis">{g.name}</span>
            </button>
          </li>
        {/each}
      {/if}
    </ul>

    <ul class="nav">
      <li>
        <button
          class="nav-item"
          aria-current={nav.view.name === "installed" ||
          nav.view.name === "import" ||
          (nav.view.name === "mod" && nav.modOrigin === "installed") ||
          (nav.view.name === "nexusMod" && nav.nexusOrigin === "installed") ||
          (nav.view.name === "tsMod" && nav.tsOrigin === "installed") ||
          (nav.view.name === "rmlMod" && nav.rmlOrigin === "installed") ||
          (nav.view.name === "clMod" && nav.clOrigin === "installed")
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
      <p class="version muted">Version 0.12, client non officiel</p>
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
      <div class="pane" hidden={nav.view.name !== "tsGames"}><TsGamesView /></div>
      {#if nav.tsGame}
        {#key nav.tsGame.id}
          <div class="pane" hidden={nav.view.name !== "tsMods"}><TsModsView game={nav.tsGame} /></div>
        {/key}
      {/if}
      {#if nav.view.name === "tsMod" && nav.tsGame}
        {#key `${nav.view.namespace}-${nav.view.modName}`}
          <div class="pane"><TsModView game={nav.tsGame} namespace={nav.view.namespace} name={nav.view.modName} /></div>
        {/key}
      {/if}
      {#if nav.view.name === "rmlMod" && nav.tsGame}
        {#key nav.view.id}
          <div class="pane"><RmlModView game={nav.tsGame} id={nav.view.id} /></div>
        {/key}
      {/if}
      <div class="pane" hidden={nav.view.name !== "clGames"}><ClGamesView /></div>
      {#if nav.clGame}
        {#key nav.clGame.id}
          <div class="pane" hidden={nav.view.name !== "clMods"}><ClModsView game={nav.clGame} /></div>
        {/key}
      {/if}
      {#if nav.view.name === "clMod" && nav.clGame}
        {#key `${nav.clGame.id}:${nav.view.id}`}
          <div class="pane"><ClModView game={nav.clGame} id={nav.view.id} /></div>
        {/key}
      {/if}
      {#if nav.view.name === "nexusTracked"}
        <div class="pane"><NexusTrackedView /></div>
      {/if}
      {#if nav.view.name === "import"}
        <div class="pane"><ImportView /></div>
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
    <UpdateBanner />
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
    /* Beaucoup de jeux avec des mods : la barre défile. */
    overflow: hidden auto;
    scrollbar-width: thin;
  }

  .icon-gap {
    width: 18px;
    flex: none;
  }

  .wordmark {
    font-family: var(--font-display);
    font-weight: 800;
    font-size: var(--t-2xl);
    letter-spacing: -0.03em;
    margin-inline: 0.6rem;
    justify-self: start;
    align-self: flex-start;
    /* Le nom de l'appli porte les deux couleurs choisies. */
    background: var(--accent-grad-ink);
    -webkit-background-clip: text;
    background-clip: text;
    color: transparent;
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

  /* Élément actif : un trait vertical aux deux couleurs. */
  .nav-item[aria-current="page"],
  .account[aria-current="page"] {
    background:
      linear-gradient(var(--accent-a-ink), var(--accent-b-ink)) left / 3px 100% no-repeat,
      var(--raise);
    color: var(--text);
  }

  .nav-item.sub {
    padding-left: 1.6rem;
    font-weight: 500;
  }

  .nav-item img {
    border-radius: 3px;
    flex: none;
  }

  .nav-item img.cover {
    object-fit: cover;
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
