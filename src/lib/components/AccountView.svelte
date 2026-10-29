<script lang="ts">
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { api, asAppError, type AppError } from "$lib/api";
  import { nav } from "$lib/nav.svelte";
  import ErrorNotice from "./ErrorNotice.svelte";
  import Icon from "./Icon.svelte";

  type Method = "email" | "token";

  let method = $state<Method>("email");
  let step = $state<"email" | "code">("email");
  let email = $state("");
  let code = $state("");
  let token = $state("");
  let busy = $state(false);
  let error = $state<AppError | null>(null);
  let emailRefused = $state(false);
  let resent = $state(false);

  const auth = $derived(nav.auth);
  const user = $derived(auth?.user ?? null);
  const name = $derived(user?.username ?? user?.display_name_portal ?? "ton compte");

  function choose(m: Method) {
    method = m;
    error = null;
  }

  async function run(action: () => Promise<void>) {
    busy = true;
    error = null;
    try {
      await action();
    } catch (e) {
      const err = asAppError(e);
      if (err.kind === "email_login_unavailable") {
        emailRefused = true;
        method = "token";
      } else {
        error = err;
      }
    } finally {
      busy = false;
    }
  }

  function sendCode(event?: SubmitEvent) {
    event?.preventDefault();
    run(async () => {
      await api.requestCode(email);
      resent = step === "code";
      step = "code";
      code = "";
    });
  }

  function loggedIn(view: Awaited<ReturnType<typeof api.exchangeCode>>) {
    nav.setAuth(view);
    step = "email";
    code = "";
    token = "";
    const next = nav.afterLogin;
    nav.afterLogin = null;
    if (next) nav.view = next;
  }

  function submitCode(event: SubmitEvent) {
    event.preventDefault();
    run(async () => loggedIn(await api.exchangeCode(code)));
  }

  function submitToken(event: SubmitEvent) {
    event.preventDefault();
    run(async () => loggedIn(await api.useToken(token)));
  }

  function logout() {
    run(async () => nav.setAuth(await api.logout()));
  }
</script>

<div class="view">
  <div class="inner">
    {#if auth?.loggedIn}
      <h1>Compte mod.io</h1>

      <section class="profile">
        {#if user?.avatar?.thumb_100x100 || user?.avatar?.original}
          <img class="avatar" src={user?.avatar?.thumb_100x100 ?? user?.avatar?.original} alt="" />
        {:else}
          <span class="avatar fallback" aria-hidden="true"><Icon name="user" size={30} /></span>
        {/if}
        <div class="who">
          <p class="name">{name}</p>
          <p class="muted">
            {auth.source === "manual" ? "Connecté avec un jeton d'accès créé sur mod.io" : "Connecté par code e-mail"}
          </p>
          {#if user?.profile_url}
            <button class="link" onclick={() => openUrl(user!.profile_url!)}>
              Voir mon profil sur mod.io <Icon name="external" size={13} />
            </button>
          {/if}
        </div>
      </section>

      {#if auth.storageWarning}
        <p class="warning">{auth.storageWarning}</p>
      {:else if auth.persisted}
        <p class="muted small">
          Ta connexion est enregistrée de façon sécurisée dans le trousseau du système (le Gestionnaire
          d'identification sous Windows).
        </p>
      {/if}

      <div class="row-actions">
        <button class="btn btn-primary" onclick={() => nav.openLibrary()}>
          <Icon name="library" size={16} />Voir mes mods
        </button>
        <button class="btn" onclick={logout} disabled={busy}>{busy ? "Déconnexion…" : "Se déconnecter"}</button>
      </div>
      {#if auth.source === "manual"}
        <p class="muted small">
          Se déconnecter oublie le jeton sur cet ordinateur. Pour le révoquer complètement, supprime-le sur
          mod.io/me/access.
        </p>
      {/if}
      {#if error}<ErrorNotice {error} />{/if}
    {:else}
      <h1>Connexion à mod.io</h1>
      <p class="intro">
        Connecte ton compte pour t'abonner à des mods. Les jeux qui gèrent mod.io téléchargent tes abonnements
        tout seuls à leur prochain lancement.
      </p>

      {#if auth?.expired}
        <p class="warning">Ta session précédente a expiré. Reconnecte-toi.</p>
      {/if}

      <div class="switch" role="tablist" aria-label="Méthode de connexion">
        <button role="tab" aria-selected={method === "email"} onclick={() => choose("email")}>Code par e-mail</button>
        <button role="tab" aria-selected={method === "token"} onclick={() => choose("token")}>Jeton d'accès</button>
      </div>

      {#if method === "email"}
        {#if step === "email"}
          <form class="form" onsubmit={sendCode}>
            <label class="field-row">
              <span class="label">Adresse e-mail de ton compte mod.io</span>
              <input class="field" type="email" autocomplete="email" required bind:value={email} />
            </label>
            <div>
              <button class="btn btn-primary" type="submit" disabled={busy || !email.trim()}>
                {busy ? "Envoi…" : "Recevoir un code"}
              </button>
            </div>
          </form>
        {:else}
          <form class="form" onsubmit={submitCode}>
            <p>
              {resent ? "Un nouveau code a été envoyé" : "Un code a été envoyé"} à <strong>{email.trim()}</strong>.
              Il n'est valable que peu de temps.
            </p>
            <label class="field-row">
              <span class="label">Code reçu par e-mail</span>
              <input
                class="field code"
                autocomplete="one-time-code"
                spellcheck="false"
                maxlength="12"
                required
                bind:value={code}
              />
            </label>
            <div class="row-actions">
              <button class="btn btn-primary" type="submit" disabled={busy || !code.trim()}>
                {busy ? "Connexion…" : "Se connecter"}
              </button>
              <button class="btn btn-ghost" type="button" onclick={() => sendCode()} disabled={busy}>
                Renvoyer un code
              </button>
              <button class="btn btn-ghost" type="button" onclick={() => ((step = "email"), (error = null))}>
                Changer d'adresse
              </button>
            </div>
          </form>
        {/if}
      {:else}
        <form class="form" onsubmit={submitToken}>
          {#if emailRefused}
            <p class="warning">
              mod.io refuse la connexion par e-mail avec ta clé API personnelle. Un jeton d'accès fait la même chose.
            </p>
          {/if}
          <ol class="steps">
            <li>
              Ouvre
              <button class="link" type="button" onclick={() => openUrl("https://mod.io/me/access")}>
                mod.io/me/access <Icon name="external" size={13} />
              </button>
            </li>
            <li>Dans la section OAuth 2, crée un jeton d'accès avec les droits de lecture et d'écriture.</li>
            <li>Copie le jeton et colle-le ici.</li>
          </ol>
          <label class="field-row">
            <span class="label">Jeton d'accès</span>
            <input class="field" type="password" autocomplete="off" spellcheck="false" required bind:value={token} />
          </label>
          <div>
            <button class="btn btn-primary" type="submit" disabled={busy || !token.trim()}>
              {busy ? "Vérification…" : "Se connecter"}
            </button>
          </div>
        </form>
      {/if}

      {#if error}<ErrorNotice {error} />{/if}

      <p class="muted small">
        En te connectant, tu acceptes les
        <button class="link" onclick={() => openUrl("https://mod.io/terms")}>conditions d'utilisation</button>
        et la
        <button class="link" onclick={() => openUrl("https://mod.io/privacy")}>politique de confidentialité</button>
        de mod.io. ModStudio n'enregistre ni ton adresse e-mail ni ton mot de passe.
      </p>
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

  .intro {
    font-size: var(--t-lg);
    margin-top: -0.6rem;
  }

  .switch {
    display: inline-flex;
    width: fit-content;
    padding: 0.25rem;
    gap: 0.25rem;
    border-radius: var(--r-md);
    background: var(--panel);
  }

  .switch button {
    padding: 0.45rem 0.9rem;
    border: 0;
    border-radius: var(--r-sm);
    background: none;
    color: var(--muted);
    font-weight: 600;
    font-size: var(--t-sm);
  }

  .switch button[aria-selected="true"] {
    background: var(--raise);
    color: var(--text);
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

  .code {
    max-width: 12rem;
    font-family: var(--font-display);
    font-size: var(--t-xl);
    font-weight: 700;
    letter-spacing: 0.2em;
    text-transform: uppercase;
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
    object-fit: cover;
    background: var(--raise);
    flex: none;
  }

  .avatar.fallback {
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
