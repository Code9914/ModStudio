<script lang="ts">
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { asAppError, type AppError } from "$lib/api";
  import { tr } from "$lib/i18n.svelte";
  import { nav } from "$lib/nav.svelte";
  import { markWelcomeDone, nexusStore } from "$lib/nexus.svelte";
  import ErrorNotice from "./ErrorNotice.svelte";
  import Icon from "./Icon.svelte";

  const KEYS_PAGE = "https://www.nexusmods.com/settings/api-keys";

  let key = $state("");
  let busy = $state(false);
  let error = $state<AppError | null>(null);
  let windowOpened = $state(false);
  let keyField = $state<HTMLInputElement | null>(null);

  const welcome = $derived(nav.nexusWelcome);

  function openLogin() {
    error = null;
    windowOpened = true;
    nexusStore.openLogin();
  }

  // Retour de la page de connexion : le champ de la clé attend le collage.
  let wasOpen = false;
  $effect(() => {
    const open = nexusStore.site?.kind === "login";
    if (wasOpen && !open) keyField?.focus();
    wasOpen = open;
  });

  function later() {
    markWelcomeDone();
    nav.nexusWelcome = false;
    nav.openNexusGames();
  }

  const auth = $derived(nexusStore.auth);
  const user = $derived(auth?.user ?? null);

  async function save(event: SubmitEvent) {
    event.preventDefault();
    busy = true;
    error = null;
    try {
      await nexusStore.setKey(key);
      key = "";
      markWelcomeDone();
      const next = nav.afterNexusKey;
      nav.afterNexusKey = null;
      if (next) nav.view = next;
      else if (nav.nexusWelcome) {
        nav.nexusWelcome = false;
        nav.openNexusGames();
      }
    } catch (e) {
      error = asAppError(e);
    } finally {
      busy = false;
    }
  }

  async function forget() {
    busy = true;
    error = null;
    try {
      await nexusStore.forgetKey();
    } catch (e) {
      error = asAppError(e);
    } finally {
      busy = false;
    }
  }
</script>

<div class="view">
  <div class="inner">
    {#if !welcome}
      <button class="btn btn-ghost crumb" onclick={() => nav.openSettings()}><Icon name="back" />{tr("Réglages", "Settings")}</button>
    {/if}
    {#if auth?.loggedIn}
      <h1>{tr("Compte Nexus Mods", "Nexus Mods account")}</h1>
      <section class="profile">
        <span class="avatar" aria-hidden="true"><Icon name="user" size={30} /></span>
        <div class="who">
          <p class="name">{user?.name ?? tr("Clé enregistrée", "Saved key")}</p>
          <p class="muted">
            {#if !user}
              {tr(
                "Clé enregistrée (Nexus Mods n'a pas pu être joint pour la vérifier).",
                "Key saved (Nexus Mods couldn't be reached to check it).",
              )}
            {:else if user.isPremium}
              {tr(
                "Compte Premium : les téléchargements démarrent directement depuis ModStudio.",
                "Premium account: downloads start directly from ModStudio.",
              )}
            {:else}
              {tr(
                "Compte gratuit : chaque téléchargement passe par la page du fichier sur Nexus Mods, que ModStudio ouvre pour toi.",
                "Free account: each download goes through the file page on Nexus Mods, which ModStudio opens for you.",
              )}
            {/if}
          </p>
          {#if user?.profileUrl}
            <button class="link" onclick={() => openUrl(user!.profileUrl!)}>
              {tr("Voir mon profil sur Nexus Mods", "View my profile on Nexus Mods")} <Icon name="external" size={13} />
            </button>
          {/if}
        </div>
      </section>

      {#if auth.storageWarning}
        <p class="warning">{auth.storageWarning}</p>
      {:else if auth.persisted}
        <p class="muted small">
          {tr(
            "Ta clé est enregistrée de façon sécurisée dans le trousseau du système (le Gestionnaire d'identification sous Windows).",
            "Your key is stored securely in the system keychain (Credential Manager on Windows).",
          )}
        </p>
      {/if}

      <div class="row-actions">
        <button class="btn btn-primary" onclick={() => nav.openNexusGames()}>
          <Icon name="grid" size={16} />{tr("Parcourir les jeux", "Browse games")}
        </button>
        <button class="btn" onclick={forget} disabled={busy}>{tr("Oublier la clé", "Forget the key")}</button>
      </div>
      <p class="muted small">
        {tr(
          "Oublier la clé la retire de cet ordinateur. Pour la désactiver complètement, supprime-la sur la page des clés API de Nexus Mods.",
          "Forgetting the key removes it from this computer. To disable it completely, delete it on the Nexus Mods API keys page.",
        )}
      </p>
      {#if error}<ErrorNotice {error} />{/if}
    {:else}
      {#if welcome}
        <p class="eyebrow">{tr("Bienvenue dans ModStudio", "Welcome to ModStudio")}</p>
        <h1>{tr("Connecte ton compte Nexus Mods", "Connect your Nexus Mods account")}</h1>
      {:else}
        <h1>{tr("Connexion à Nexus Mods", "Sign in to Nexus Mods")}</h1>
      {/if}
      <p class="intro">
        {tr(
          "Parcourir les mods ne demande rien. Pour les installer, connecte-toi une fois à Nexus Mods dans ModStudio et donne-lui ta clé API personnelle : elle lui permet de demander les téléchargements à ton nom.",
          "Browsing mods requires nothing. To install them, sign in to Nexus Mods once in ModStudio and give it your personal API key: it lets ModStudio request downloads on your behalf.",
        )}
      </p>

      {#if auth?.rejected}
        <p class="warning">
          {tr(
            "Nexus Mods refuse la clé enregistrée (elle a peut-être été supprimée). Ajoute-en une nouvelle.",
            "Nexus Mods rejects the saved key (it may have been deleted). Add a new one.",
          )}
        </p>
      {/if}

      <form class="form" onsubmit={save}>
        <ol class="steps">
          <li>
            <span class="step-title">{tr("Connecte-toi à Nexus Mods.", "Sign in to Nexus Mods.")}</span>
            <span class="muted">
              {tr(
                "ModStudio garde ta connexion : tu n'auras plus à te connecter pour télécharger.",
                "ModStudio keeps you signed in: you won't have to sign in again to download.",
              )}
            </span>
            <span class="step-actions">
              <button class="btn btn-primary" type="button" onclick={openLogin}>
                {windowOpened
                  ? tr("Rouvrir la page Nexus Mods", "Reopen the Nexus Mods page")
                  : tr("Se connecter à Nexus Mods", "Sign in to Nexus Mods")}
              </button>
              <button class="link" type="button" onclick={() => openUrl(KEYS_PAGE)}>
                {tr("ou dans mon navigateur", "or in my browser")} <Icon name="external" size={13} />
              </button>
            </span>
          </li>
          <li>
            <span class="step-title">{tr("Copie ta clé personnelle.", "Copy your personal key.")}</span>
            <span class="muted">
              {tr(
                "Une fois connecté, tu arrives sur la page des clés API : copie la « Personal API Key » (crée-la si tu n'en as pas encore), puis clique sur « J'ai copié ma clé ».",
                'Once signed in, you land on the API keys page: copy the "Personal API Key" (create it if you don\'t have one yet), then click "I\'ve copied my key".',
              )}
            </span>
          </li>
          <li>
            <span class="step-title">{tr("Colle-la ici.", "Paste it here.")}</span>
            <label class="field-row">
              <span class="visually-hidden">{tr("Clé API Nexus Mods", "Nexus Mods API key")}</span>
              <input
                class="field"
                type="password"
                autocomplete="off"
                spellcheck="false"
                placeholder={tr("Clé API", "API key")}
                required
                bind:this={keyField}
                bind:value={key}
              />
            </label>
          </li>
        </ol>
        <div class="row-actions">
          <button class="btn btn-primary" type="submit" disabled={busy || !key.trim()}>
            {busy ? tr("Vérification…", "Checking…") : tr("Enregistrer la clé", "Save the key")}
          </button>
          {#if welcome}
            <button class="btn btn-ghost" type="button" onclick={later}>{tr("Plus tard", "Later")}</button>
          {/if}
        </div>
      </form>

      {#if error}<ErrorNotice {error} />{/if}

      <div class="explain">
        <h2>{tr("Premium ou pas ?", "Premium or not?")}</h2>
        <p class="small">
          {tr(
            "Avec un compte Premium, ModStudio télécharge et installe directement. Avec un compte gratuit, Nexus Mods veut que chaque téléchargement passe par son site : ModStudio ouvre la page du fichier dans une fenêtre, tu cliques sur « Slow download », et l'installation démarre toute seule.",
            'With a Premium account, ModStudio downloads and installs directly. With a free account, Nexus Mods wants every download to go through its site: ModStudio opens the file page in a window, you click "Slow download", and the installation starts on its own.',
          )}
        </p>
        <p class="muted small">
          {tr(
            "ModStudio n'envoie ta clé qu'à Nexus Mods, et ne la montre jamais. C'est un client non officiel, pour ton usage personnel.",
            "ModStudio only sends your key to Nexus Mods, and never shows it. It's an unofficial client, for your personal use.",
          )}
        </p>
      </div>
    {/if}
  </div>
</div>

<style>
  .view {
    height: 100%;
    overflow-y: auto;
    padding: 2.5rem;
  }

  .inner {
    max-width: 38rem;
    display: grid;
    gap: 1.6rem;
  }

  .crumb {
    justify-self: start;
    margin: -0.75rem 0 -0.75rem -0.5rem;
  }

  h1 {
    font-size: var(--t-3xl);
    font-weight: 800;
  }

  h2 {
    font-size: var(--t-lg);
    font-weight: 700;
  }

  .intro {
    font-size: var(--t-lg);
    margin-top: -0.6rem;
  }

  .form {
    display: grid;
    gap: 1.1rem;
  }

  .field-row {
    display: grid;
    gap: 0.35rem;
  }


  .row-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
  }

  .steps {
    margin: 0;
    padding: 0;
    list-style: none;
    counter-reset: step;
    display: grid;
    gap: 1.25rem;
  }

  .steps li {
    position: relative;
    display: grid;
    gap: 0.35rem;
    padding-left: 2.4rem;
    counter-increment: step;
  }

  .steps li::before {
    content: counter(step);
    position: absolute;
    left: 0;
    top: -0.05rem;
    width: 1.6rem;
    height: 1.6rem;
    display: grid;
    place-items: center;
    border-radius: 50%;
    background: var(--raise);
    font-weight: 700;
    font-size: var(--t-sm);
  }

  .step-title {
    font-weight: 600;
  }

  .step-actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.5rem 1rem;
    margin-top: 0.2rem;
  }

  .eyebrow {
    font-size: var(--t-xs);
    font-weight: 700;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--accent);
    margin-bottom: -1.2rem;
  }

  .link {
    display: inline-flex;
    align-items: center;
    gap: 0.2rem;
    padding: 0;
    border: 0;
    background: none;
    color: var(--accent);
    text-decoration: underline;
    text-underline-offset: 3px;
    font-size: inherit;
  }

  .warning {
    padding: 0.8rem 1rem;
    border-radius: var(--r-md);
    background: var(--danger-bg);
    color: var(--text);
    font-size: var(--t-md);
  }

  .explain {
    display: grid;
    gap: 0.6rem;
    padding: 1.1rem 1.2rem;
    border-radius: var(--r-md);
    background: var(--panel);
  }

  .small {
    font-size: var(--t-sm);
  }

  .profile {
    display: flex;
    align-items: center;
    gap: 1.1rem;
  }

  .avatar {
    width: 4.5rem;
    height: 4.5rem;
    border-radius: 50%;
    background: var(--raise);
    flex: none;
    display: grid;
    place-items: center;
    color: var(--muted);
  }

  .who {
    display: grid;
    gap: 0.15rem;
    justify-items: start;
  }

  .name {
    font-family: var(--font-display);
    font-size: var(--t-2xl);
    font-weight: 750;
  }
</style>
