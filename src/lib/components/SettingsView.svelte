<script lang="ts">
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { api, type AppError } from "$lib/api";
  import { nav } from "$lib/nav.svelte";
  import ErrorNotice from "./ErrorNotice.svelte";
  import Icon from "./Icon.svelte";

  const first = $derived(!nav.settings?.hasApiKey);

  let apiKey = $state("");
  let apiPath = $state(nav.settings?.apiPath ?? "https://api.mod.io/v1");
  let showKey = $state(false);
  let saving = $state(false);
  let saved = $state(false);
  let error = $state<AppError | null>(null);

  async function save(event: SubmitEvent) {
    event.preventDefault();
    saving = true;
    saved = false;
    error = null;
    try {
      const wasFirst = first;
      nav.settings = await api.saveSettings(apiKey.trim() || null, apiPath);
      apiPath = nav.settings.apiPath;
      apiKey = "";
      saved = true;
      nav.reset(false);
      if (wasFirst) nav.openGames();
    } catch (e) {
      error = e as AppError;
    } finally {
      saving = false;
    }
  }

  async function toggleMature(event: Event) {
    const value = (event.currentTarget as HTMLInputElement).checked;
    try {
      nav.settings = await api.setHideMature(value);
      nav.reset(true);
    } catch (e) {
      error = e as AppError;
    }
  }
</script>

<div class="view">
  <div class="inner">
    {#if first}
      <h1>Bienvenue dans Atelier</h1>
      <p class="intro">
        Atelier parcourt les mods de mod.io depuis ton bureau. Pour commencer, il lui faut ta clé API mod.io :
        elle est gratuite et donne un accès en lecture seule.
      </p>
    {:else}
      <h1>Réglages</h1>
    {/if}

    <section class="block">
      <h2>Clé API mod.io</h2>
      <ol class="steps">
        <li>
          Ouvre la page
          <button class="link" type="button" onclick={() => openUrl("https://mod.io/me/access")}>
            mod.io/me/access <Icon name="external" size={13} />
          </button>
          et connecte-toi.
        </li>
        <li>Dans la section API, crée une clé si tu n'en as pas encore.</li>
        <li>Copie la clé et le chemin d'API affiché juste à côté, puis colle-les ici.</li>
      </ol>

      <form class="form" onsubmit={save}>
        <label class="row">
          <span class="label">Clé API</span>
          <span class="key">
            <input
              class="field"
              type={showKey ? "text" : "password"}
              autocomplete="off"
              spellcheck="false"
              placeholder={nav.settings?.apiKeyHint ? `Clé enregistrée (${nav.settings.apiKeyHint})` : "32 caractères"}
              bind:value={apiKey}
              required={first}
            />
            <button class="btn btn-ghost" type="button" onclick={() => (showKey = !showKey)}>
              {showKey ? "Masquer" : "Afficher"}
            </button>
          </span>
          {#if !first}
            <span class="help muted">Laisse vide pour garder la clé actuelle.</span>
          {/if}
        </label>

        <label class="row">
          <span class="label">Chemin d'API</span>
          <input class="field" type="url" spellcheck="false" bind:value={apiPath} />
          <span class="help muted">De la forme https://u-12345.modapi.io/v1. Par défaut : https://api.mod.io/v1.</span>
        </label>

        <div class="submit">
          <button class="btn btn-primary" type="submit" disabled={saving}>
            {saving ? "Test de la clé…" : "Enregistrer et tester"}
          </button>
          {#if saved}
            <p class="ok" role="status">Clé enregistrée, mod.io répond.</p>
          {/if}
        </div>

        {#if error}
          <ErrorNotice {error} />
        {/if}
      </form>
    </section>

    {#if !first}
      <section class="block">
        <h2>Contenu</h2>
        <label class="toggle">
          <input type="checkbox" checked={nav.settings?.hideMature} onchange={toggleMature} />
          <span>
            Masquer les mods pour adultes
            <span class="help muted">Les mods marqués comme contenu adulte par leur auteur n'apparaissent plus dans les listes.</span>
          </span>
        </label>
      </section>
    {/if}

    <p class="disclaimer muted">
      Atelier est un client non officiel. Il n'est ni affilié à mod.io ni soutenu par mod.io. La clé API est
      enregistrée sur cet ordinateur uniquement.
    </p>
  </div>
</div>

<style>
  .view {
    height: 100%;
    overflow-y: auto;
    padding: 2.5rem;
  }

  .inner {
    max-width: 40rem;
    display: grid;
    gap: 2.25rem;
  }

  h1 {
    font-size: var(--t-3xl);
    font-weight: 800;
  }

  .intro {
    font-size: var(--t-lg);
    margin-top: -1.25rem;
  }

  h2 {
    font-size: var(--t-xl);
    font-weight: 700;
    margin-bottom: 0.9rem;
  }

  .block {
    display: grid;
  }

  .steps {
    margin: 0 0 1.5rem;
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
  }

  .form {
    display: grid;
    gap: 1.25rem;
  }

  .row {
    display: grid;
    gap: 0.35rem;
  }

  .label {
    font-weight: 600;
    font-size: var(--t-sm);
  }

  .key {
    display: flex;
    gap: 0.4rem;
  }

  .help {
    font-size: var(--t-sm);
  }

  .submit {
    display: flex;
    align-items: center;
    gap: 1rem;
  }

  .ok {
    color: var(--ok);
    font-weight: 600;
    font-size: var(--t-sm);
  }

  .toggle {
    display: flex;
    align-items: flex-start;
    gap: 0.75rem;
    cursor: pointer;
  }

  .toggle input {
    width: 1.1rem;
    height: 1.1rem;
    margin-top: 0.2rem;
    accent-color: var(--accent-fill);
  }

  .toggle .help {
    display: block;
  }

  .disclaimer {
    font-size: var(--t-sm);
  }
</style>
