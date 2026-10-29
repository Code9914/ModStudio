<script lang="ts">
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { asAppError, type AppError } from "$lib/api";
  import { nav } from "$lib/nav.svelte";
  import { nexusStore } from "$lib/nexus.svelte";
  import ErrorNotice from "./ErrorNotice.svelte";
  import Icon from "./Icon.svelte";

  const KEYS_PAGE = "https://www.nexusmods.com/settings/api-keys";

  let key = $state("");
  let busy = $state(false);
  let error = $state<AppError | null>(null);

  const auth = $derived(nexusStore.auth);
  const user = $derived(auth?.user ?? null);

  async function save(event: SubmitEvent) {
    event.preventDefault();
    busy = true;
    error = null;
    try {
      await nexusStore.setKey(key);
      key = "";
      const next = nav.afterNexusKey;
      nav.afterNexusKey = null;
      if (next) nav.view = next;
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
    {#if auth?.loggedIn}
      <h1>Compte Nexus Mods</h1>
      <section class="profile">
        <span class="avatar" aria-hidden="true"><Icon name="user" size={30} /></span>
        <div class="who">
          <p class="name">{user?.name ?? "Clé enregistrée"}</p>
          <p class="muted">
            {#if !user}
              Clé enregistrée (Nexus Mods n'a pas pu être joint pour la vérifier).
            {:else if user.isPremium}
              Compte Premium : les téléchargements démarrent directement depuis ModStudio.
            {:else}
              Compte gratuit : chaque téléchargement passe par la page du fichier sur Nexus Mods, que ModStudio ouvre
              pour toi.
            {/if}
          </p>
          {#if user?.profileUrl}
            <button class="link" onclick={() => openUrl(user!.profileUrl!)}>
              Voir mon profil sur Nexus Mods <Icon name="external" size={13} />
            </button>
          {/if}
        </div>
      </section>

      {#if auth.storageWarning}
        <p class="warning">{auth.storageWarning}</p>
      {:else if auth.persisted}
        <p class="muted small">
          Ta clé est enregistrée de façon sécurisée dans le trousseau du système (le Gestionnaire d'identification sous
          Windows).
        </p>
      {/if}

      <div class="row-actions">
        <button class="btn btn-primary" onclick={() => nav.openNexusGames()}>
          <Icon name="grid" size={16} />Parcourir les jeux
        </button>
        <button class="btn" onclick={forget} disabled={busy}>Oublier la clé</button>
      </div>
      <p class="muted small">
        Oublier la clé la retire de cet ordinateur. Pour la désactiver complètement, supprime-la sur la page des clés API
        de Nexus Mods.
      </p>
      {#if error}<ErrorNotice {error} />{/if}
    {:else}
      <h1>Clé API Nexus Mods</h1>
      <p class="intro">
        Parcourir Nexus Mods ne demande rien. Pour installer des mods, ModStudio a besoin de ta clé API personnelle : elle
        lui permet de demander les liens de téléchargement à ton nom.
      </p>

      {#if auth?.rejected}
        <p class="warning">Nexus Mods refuse la clé enregistrée (elle a peut-être été supprimée). Ajoute-en une nouvelle.</p>
      {/if}

      <form class="form" onsubmit={save}>
        <ol class="steps">
          <li>
            Ouvre
            <button class="link" type="button" onclick={() => openUrl(KEYS_PAGE)}>
              la page des clés API de Nexus Mods <Icon name="external" size={13} />
            </button>
            et connecte-toi.
          </li>
          <li>Copie ta clé personnelle (« Personal API Key »), en la créant si tu n'en as pas encore.</li>
          <li>Colle-la ici.</li>
        </ol>
        <label class="field-row">
          <span class="label">Clé API</span>
          <input class="field" type="password" autocomplete="off" spellcheck="false" required bind:value={key} />
        </label>
        <div>
          <button class="btn btn-primary" type="submit" disabled={busy || !key.trim()}>
            {busy ? "Vérification…" : "Enregistrer la clé"}
          </button>
        </div>
      </form>

      {#if error}<ErrorNotice {error} />{/if}

      <div class="explain">
        <h2>Premium ou pas ?</h2>
        <p class="small">
          Avec un compte Premium, ModStudio télécharge et installe directement. Avec un compte gratuit, Nexus Mods veut
          que chaque téléchargement passe par son site : ModStudio ouvre la page du fichier dans une fenêtre, tu cliques
          sur « Slow download », et l'installation démarre toute seule.
        </p>
        <p class="muted small">
          ModStudio n'envoie ta clé qu'à Nexus Mods, et ne la montre jamais. C'est un client non officiel, pour ton usage
          personnel.
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

  .label {
    font-weight: 600;
    font-size: var(--t-sm);
  }

  .row-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
  }

  .steps {
    margin: 0;
    padding-left: 1.25rem;
    display: grid;
    gap: 0.35rem;
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
