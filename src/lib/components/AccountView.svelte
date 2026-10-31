<script lang="ts">
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { api, asAppError, type AppError } from "$lib/api";
  import { english, tr } from "$lib/i18n.svelte";
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
  const name = $derived(user?.username ?? user?.display_name_portal ?? tr("ton compte", "your account"));

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
      <h1>{tr("Compte mod.io", "mod.io account")}</h1>

      <section class="profile">
        {#if user?.avatar?.thumb_100x100 || user?.avatar?.original}
          <img class="avatar" src={user?.avatar?.thumb_100x100 ?? user?.avatar?.original} alt="" />
        {:else}
          <span class="avatar fallback" aria-hidden="true"><Icon name="user" size={30} /></span>
        {/if}
        <div class="who">
          <p class="name">{name}</p>
          <p class="muted">
            {auth.source === "manual"
              ? tr("Connecté avec un jeton d'accès créé sur mod.io", "Signed in with an access token created on mod.io")
              : tr("Connecté par code e-mail", "Signed in with an email code")}
          </p>
          {#if user?.profile_url}
            <button class="link" onclick={() => openUrl(user!.profile_url!)}>
              {tr("Voir mon profil sur mod.io", "View my profile on mod.io")} <Icon name="external" size={13} />
            </button>
          {/if}
        </div>
      </section>

      {#if auth.storageWarning}
        <p class="warning">{auth.storageWarning}</p>
      {:else if auth.persisted}
        <p class="muted small">
          {tr(
            "Ta connexion est enregistrée de façon sécurisée dans le trousseau du système (le Gestionnaire d'identification sous Windows).",
            "Your sign-in is stored securely in the system keychain (Credential Manager on Windows).",
          )}
        </p>
      {/if}

      <div class="row-actions">
        <button class="btn btn-primary" onclick={() => nav.openLibrary()}>
          <Icon name="library" size={16} />{tr("Voir mes mods", "View my mods")}
        </button>
        <button class="btn" onclick={logout} disabled={busy}>{busy ? tr("Déconnexion…", "Signing out…") : tr("Se déconnecter", "Sign out")}</button>
      </div>
      {#if auth.source === "manual"}
        <p class="muted small">
          {tr(
            "Se déconnecter oublie le jeton sur cet ordinateur. Pour le révoquer complètement, supprime-le sur mod.io/me/access.",
            "Signing out forgets the token on this computer. To revoke it completely, delete it at mod.io/me/access.",
          )}
        </p>
      {/if}
      {#if error}<ErrorNotice {error} />{/if}
    {:else}
      <h1>{tr("Connexion à mod.io", "Sign in to mod.io")}</h1>
      <p class="intro">
        {tr(
          "Connecte ton compte pour t'abonner à des mods. Les jeux qui gèrent mod.io téléchargent tes abonnements tout seuls à leur prochain lancement.",
          "Connect your account to subscribe to mods. Games that support mod.io download your subscriptions on their own the next time they start.",
        )}
      </p>

      {#if auth?.expired}
        <p class="warning">{tr("Ta session précédente a expiré. Reconnecte-toi.", "Your previous session has expired. Please sign in again.")}</p>
      {/if}

      <div class="switch" role="tablist" aria-label={tr("Méthode de connexion", "Sign-in method")}>
        <button role="tab" aria-selected={method === "email"} onclick={() => choose("email")}>{tr("Code par e-mail", "Email code")}</button>
        <button role="tab" aria-selected={method === "token"} onclick={() => choose("token")}>{tr("Jeton d'accès", "Access token")}</button>
      </div>

      {#if method === "email"}
        {#if step === "email"}
          <form class="form" onsubmit={sendCode}>
            <label class="field-row">
              <span class="label">{tr("Adresse e-mail de ton compte mod.io", "Your mod.io account email address")}</span>
              <input class="field" type="email" autocomplete="email" required bind:value={email} />
            </label>
            <div>
              <button class="btn btn-primary" type="submit" disabled={busy || !email.trim()}>
                {busy ? tr("Envoi…", "Sending…") : tr("Recevoir un code", "Get a code")}
              </button>
            </div>
          </form>
        {:else}
          <form class="form" onsubmit={submitCode}>
            <p>
              {resent ? tr("Un nouveau code a été envoyé", "A new code was sent") : tr("Un code a été envoyé", "A code was sent")}
              {tr("à", "to")} <strong>{email.trim()}</strong>.
              {tr("Il n'est valable que peu de temps.", "It's only valid for a short time.")}
            </p>
            <label class="field-row">
              <span class="label">{tr("Code reçu par e-mail", "Code received by email")}</span>
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
                {busy ? tr("Connexion…", "Signing in…") : tr("Se connecter", "Sign in")}
              </button>
              <button class="btn btn-ghost" type="button" onclick={() => sendCode()} disabled={busy}>
                {tr("Renvoyer un code", "Resend code")}
              </button>
              <button class="btn btn-ghost" type="button" onclick={() => ((step = "email"), (error = null))}>
                {tr("Changer d'adresse", "Change email")}
              </button>
            </div>
          </form>
        {/if}
      {:else}
        <form class="form" onsubmit={submitToken}>
          {#if emailRefused}
            <p class="warning">
              {tr(
                "mod.io refuse la connexion par e-mail avec ta clé API personnelle. Un jeton d'accès fait la même chose.",
                "mod.io doesn't allow email sign-in with your personal API key. An access token does the same thing.",
              )}
            </p>
          {/if}
          <ol class="steps">
            <li>
              {tr("Ouvre", "Open")}
              <button class="link" type="button" onclick={() => openUrl("https://mod.io/me/access")}>
                mod.io/me/access <Icon name="external" size={13} />
              </button>
            </li>
            <li>
              {tr(
                "Dans la section OAuth 2, crée un jeton d'accès avec les droits de lecture et d'écriture.",
                "In the OAuth 2 section, create an access token with read and write permissions.",
              )}
            </li>
            <li>{tr("Copie le jeton et colle-le ici.", "Copy the token and paste it here.")}</li>
          </ol>
          <label class="field-row">
            <span class="label">{tr("Jeton d'accès", "Access token")}</span>
            <input class="field" type="password" autocomplete="off" spellcheck="false" required bind:value={token} />
          </label>
          <div>
            <button class="btn btn-primary" type="submit" disabled={busy || !token.trim()}>
              {busy ? tr("Vérification…", "Checking…") : tr("Se connecter", "Sign in")}
            </button>
          </div>
        </form>
      {/if}

      {#if error}<ErrorNotice {error} />{/if}

      <p class="muted small">
        {#if english}
          By signing in, you accept the mod.io
          <button class="link" onclick={() => openUrl("https://mod.io/terms")}>terms of use</button>
          and
          <button class="link" onclick={() => openUrl("https://mod.io/privacy")}>privacy policy</button>.
          ModStudio doesn't store your email address or your password.
        {:else}
          En te connectant, tu acceptes les
          <button class="link" onclick={() => openUrl("https://mod.io/terms")}>conditions d'utilisation</button>
          et la
          <button class="link" onclick={() => openUrl("https://mod.io/privacy")}>politique de confidentialité</button>
          de mod.io. ModStudio n'enregistre ni ton adresse e-mail ni ton mot de passe.
        {/if}
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
