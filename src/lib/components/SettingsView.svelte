<script lang="ts">
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { api, type AppError } from "$lib/api";
  import { nav } from "$lib/nav.svelte";
  import { updates } from "$lib/updates.svelte";
  import { tr } from "$lib/i18n.svelte";
  import AppearanceSettings from "./AppearanceSettings.svelte";
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
      <h1>{tr("Bienvenue dans ModStudio", "Welcome to ModStudio")}</h1>
      <p class="intro">
        {tr(
          "ModStudio parcourt les mods de mod.io depuis ton bureau. Pour commencer, il lui faut ta clé API mod.io : elle est gratuite et donne un accès en lecture seule.",
          "ModStudio browses mod.io mods from your desktop. To get started, it needs your mod.io API key: it's free and gives read-only access.",
        )}
      </p>
    {:else}
      <h1>{tr("Réglages", "Settings")}</h1>
      <AppearanceSettings />
    {/if}

    <section class="block">
      <h2>{tr("Clé API mod.io", "mod.io API key")}</h2>
      <ol class="steps">
        <li>
          {tr("Ouvre la page", "Open the page")}
          <button class="link" type="button" onclick={() => openUrl("https://mod.io/me/access")}>
            mod.io/me/access <Icon name="external" size={13} />
          </button>
          {tr("et connecte-toi.", "and sign in.")}
        </li>
        <li>{tr("Dans la section API, crée une clé si tu n'en as pas encore.", "In the API section, create a key if you don't have one yet.")}</li>
        <li>
          {tr(
            "Copie la clé et le chemin d'API affiché juste à côté, puis colle-les ici.",
            "Copy the key and the API path shown right next to it, then paste them here.",
          )}
        </li>
      </ol>

      <form class="form" onsubmit={save}>
        <label class="row">
          <span class="label">{tr("Clé API", "API key")}</span>
          <span class="key">
            <input
              class="field"
              type={showKey ? "text" : "password"}
              autocomplete="off"
              spellcheck="false"
              placeholder={nav.settings?.apiKeyHint
                ? tr(`Clé enregistrée (${nav.settings.apiKeyHint})`, `Saved key (${nav.settings.apiKeyHint})`)
                : tr("32 caractères", "32 characters")}
              bind:value={apiKey}
              required={first}
            />
            <button class="btn btn-ghost" type="button" onclick={() => (showKey = !showKey)}>
              {showKey ? tr("Masquer", "Hide") : tr("Afficher", "Show")}
            </button>
          </span>
          {#if !first}
            <span class="help muted">{tr("Laisse vide pour garder la clé actuelle.", "Leave empty to keep the current key.")}</span>
          {/if}
        </label>

        <label class="row">
          <span class="label">{tr("Chemin d'API", "API path")}</span>
          <input class="field" type="url" spellcheck="false" bind:value={apiPath} />
          <span class="help muted">
            {tr(
              "De la forme https://u-12345.modapi.io/v1. Par défaut : https://api.mod.io/v1.",
              "In the form https://u-12345.modapi.io/v1. Default: https://api.mod.io/v1.",
            )}
          </span>
        </label>

        <div class="submit">
          <button class="btn btn-primary" type="submit" disabled={saving}>
            {saving ? tr("Test de la clé…", "Testing the key…") : tr("Enregistrer et tester", "Save and test")}
          </button>
          {#if saved}
            <p class="ok" role="status">{tr("Clé enregistrée, mod.io répond.", "Key saved, mod.io is responding.")}</p>
          {/if}
        </div>

        {#if error}
          <ErrorNotice {error} />
        {/if}
      </form>
    </section>

    {#if first}
      <AppearanceSettings />
    {:else}
      <section class="block">
        <h2>{tr("Mises à jour de ModStudio", "ModStudio updates")}</h2>
        {#if !updates.enabled}
          <p class="help muted">
            {tr(
              "Version de développement : elle se met à jour toute seule à chaque lancement de « ModStudio (dev) ».",
              'Development version: it updates itself every time "ModStudio (dev)" starts.',
            )}
          </p>
        {:else}
          <p class="help muted">
            {tr(
              `Version installée : ${updates.current ?? "…"}. ModStudio cherche une nouvelle version à chaque lancement et te la propose ; la mise à jour est signée et garde tes réglages et tes mods.`,
              `Installed version: ${updates.current ?? "…"}. ModStudio looks for a new version every time it starts and offers it to you; the update is signed and keeps your settings and mods.`,
            )}
          </p>
          <div class="update-row">
            <button class="btn" type="button" onclick={() => updates.check(true)} disabled={updates.checking}>
              <Icon name="refresh" size={15} />{updates.checking ? tr("Recherche…", "Checking…") : tr("Rechercher une mise à jour", "Check for updates")}
            </button>
            {#if updates.available && updates.packageManager}
              <span class="help">
                {tr(
                  `Version ${updates.available.version} disponible : mets ModStudio à jour avec ton gestionnaire de paquets (installé depuis l'AUR).`,
                  `Version ${updates.available.version} available: update ModStudio with your package manager (installed from the AUR).`,
                )}
              </span>
            {:else if updates.available}
              <button class="btn btn-primary" type="button" onclick={() => updates.install()} disabled={!!updates.progress}>
                {tr(`Installer la version ${updates.available.version}`, `Install version ${updates.available.version}`)}
              </button>
            {:else if updates.checked && !updates.checking && !updates.error}
              <span class="help muted">{tr("ModStudio est à jour.", "ModStudio is up to date.")}</span>
            {/if}
          </div>
          {#if updates.error}<p class="help update-error">
              {tr(`Recherche impossible : ${updates.error}`, `Couldn't check for updates: ${updates.error}`)}
            </p>{/if}
        {/if}
      </section>

      <section class="block">
        <h2>{tr("Contenu", "Content")}</h2>
        <label class="toggle">
          <input type="checkbox" checked={nav.settings?.hideMature} onchange={toggleMature} />
          <span>
            {tr("Masquer les mods pour adultes", "Hide mature mods")}
            <span class="help muted">
              {tr(
                "Les mods marqués comme contenu adulte par leur auteur n'apparaissent plus dans les listes.",
                "Mods marked as mature content by their author no longer appear in lists.",
              )}
            </span>
          </span>
        </label>
      </section>
    {/if}

    <p class="disclaimer muted">
      {tr(
        "ModStudio est un client non officiel. Il n'est ni affilié à mod.io ni soutenu par mod.io. La clé API est enregistrée sur cet ordinateur uniquement.",
        "ModStudio is an unofficial client. It is not affiliated with or endorsed by mod.io. The API key is stored on this computer only.",
      )}
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

  .update-row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.5rem 0.75rem;
    margin-top: 0.75rem;
  }

  .update-error {
    color: var(--danger);
    margin-top: 0.5rem;
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
