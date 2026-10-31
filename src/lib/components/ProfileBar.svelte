<script lang="ts">
  // Profils de mods d'un jeu : passer de l'un à l'autre, en créer, renommer, supprimer.
  import { asAppError, type GameInstall, type ProfileApplied, type ProfileEdit } from "$lib/api";
  import { tr } from "$lib/i18n.svelte";
  import Icon from "./Icon.svelte";

  let {
    game,
    onapply,
    onedit,
  }: {
    game: GameInstall;
    onapply: (name: string) => Promise<ProfileApplied["issues"]>;
    onedit: (edit: ProfileEdit) => Promise<void>;
  } = $props();

  const profiles = $derived(game.profiles ?? []);
  const active = $derived(game.activeProfile ?? null);

  let mode = $state<"idle" | "create" | "rename">("idle");
  let draft = $state("");
  let busy = $state(false);
  let error = $state<string | null>(null);
  let issues = $state<ProfileApplied["issues"]>([]);
  let confirmDelete = $state(false);

  async function run(task: () => Promise<void>) {
    busy = true;
    error = null;
    try {
      await task();
    } catch (e) {
      error = asAppError(e).message;
    } finally {
      busy = false;
    }
  }

  function apply(name: string) {
    if (name === active) return;
    issues = [];
    run(async () => {
      issues = await onapply(name);
    });
  }

  function submit(e: SubmitEvent) {
    e.preventDefault();
    const name = draft.trim();
    if (!name) return;
    const edit: ProfileEdit =
      mode === "rename" && active ? { action: "rename", name: active, newName: name } : { action: "create", name };
    run(async () => {
      await onedit(edit);
      mode = "idle";
      draft = "";
    });
  }

  function remove() {
    if (!active) return;
    if (!confirmDelete) {
      confirmDelete = true;
      setTimeout(() => (confirmDelete = false), 4000);
      return;
    }
    confirmDelete = false;
    run(() => onedit({ action: "delete", name: active }));
  }
</script>

<div class="profiles">
  {#if mode !== "idle"}
    <form class="row" onsubmit={submit}>
      <label class="visually-hidden" for="profile-name-{game.gameId}">{tr("Nom du profil", "Profile name")}</label>
      <input
        id="profile-name-{game.gameId}"
        class="field"
        maxlength="40"
        placeholder={mode === "create" ? tr("Nom du nouveau profil", "New profile name") : tr("Nouveau nom", "New name")}
        bind:value={draft}
      />
      <button class="btn btn-primary" type="submit" disabled={busy || !draft.trim()}>
        {mode === "create" ? tr("Créer", "Create") : tr("Renommer", "Rename")}
      </button>
      <button class="btn btn-ghost" type="button" onclick={() => ((mode = "idle"), (draft = ""))}>{tr("Annuler", "Cancel")}</button>
    </form>
    {#if mode === "create"}
      <p class="small muted">
        {tr("Le nouveau profil part des mods activés en ce moment.", "The new profile starts from the mods enabled right now.")}
      </p>
    {/if}
  {:else if profiles.length === 0}
    <button class="btn btn-ghost small-btn" onclick={() => (mode = "create")} title={tr("Garder plusieurs ensembles de mods pour ce jeu", "Keep several sets of mods for this game")}>
      <Icon name="plus" size={14} />{tr("Créer un profil", "Create a profile")}
    </button>
  {:else}
    <div class="row">
      <label class="select">
        <span class="label">{tr("Profil", "Profile")}</span>
        <select class="field" value={active ?? ""} onchange={(e) => apply(e.currentTarget.value)} disabled={busy}>
          {#each profiles as p (p.name)}
            <option value={p.name}
              >{p.name} ({p.mods.length}
              {tr(p.mods.length > 1 ? "mods" : "mod", p.mods.length === 1 ? "mod" : "mods")})</option
            >
          {/each}
        </select>
      </label>
      <button class="btn btn-ghost small-btn" onclick={() => (mode = "create")} disabled={busy}><Icon name="plus" size={14} />{tr("Nouveau", "New")}</button>
      <button class="btn btn-ghost small-btn" onclick={() => ((mode = "rename"), (draft = active ?? ""))} disabled={busy || !active}>{tr("Renommer", "Rename")}</button>
      <button class="btn btn-ghost small-btn" class:danger={confirmDelete} onclick={remove} disabled={busy || !active}>
        {confirmDelete ? tr("Confirmer", "Confirm") : tr("Supprimer", "Delete")}
      </button>
      {#if busy}<span class="small muted">{tr("Changement en cours…", "Switching…")}</span>{/if}
    </div>
  {/if}
  {#if error}<p class="small error">{error}</p>{/if}
  {#if issues.length}
    <div class="issues">
      <p class="small strong">{tr("Certains mods n'ont pas pu changer d'état :", "Some mods couldn't be switched:")}</p>
      <ul>
        {#each issues as i (i.modId)}
          <li class="small"><span class="strong">{i.name}</span>{tr(" : ", ": ")}{i.message}</li>
        {/each}
      </ul>
    </div>
  {/if}
</div>

<style>
  .profiles {
    display: grid;
    gap: 0.4rem;
    margin-bottom: 0.9rem;
  }

  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.4rem;
  }

  .row .field {
    width: auto;
    min-width: 12rem;
    padding-block: 0.35rem;
    font-size: var(--t-sm);
  }

  .select {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }

  .label {
    font-size: var(--t-xs);
    font-weight: 700;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--muted);
  }

  .small-btn {
    font-size: var(--t-sm);
    padding-block: 0.35rem;
  }

  .profiles > .small-btn {
    justify-self: start;
    margin-left: -0.5rem;
  }

  .small {
    font-size: var(--t-sm);
  }

  .strong {
    font-weight: 600;
  }

  .error {
    color: var(--danger);
  }

  .danger {
    background: var(--danger-bg);
    color: var(--danger);
  }

  .issues {
    padding: 0.6rem 0.8rem;
    border-radius: var(--r-md);
    background: var(--danger-bg);
  }

  .issues ul {
    margin: 0.3rem 0 0;
    padding-left: 1.1rem;
  }
</style>
