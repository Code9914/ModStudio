<script lang="ts">
  // Installer, mettre à jour, désactiver ou désinstaller un mod Thunderstore.
  import { asAppError, ts, type AppError, type TsCommunity, type TsPackageDetail } from "$lib/api";
  import { isLoaderName, tsStore } from "$lib/thunderstore.svelte";
  import ErrorNotice from "./ErrorNotice.svelte";
  import Icon from "./Icon.svelte";
  import ModToggle from "./ModToggle.svelte";
  import ProgressBar from "./ProgressBar.svelte";
  import TsGameSetup from "./TsGameSetup.svelte";

  let { game, mod }: { game: TsCommunity; mod: TsPackageDetail } = $props();

  let error = $state<AppError | null>(null);
  let note = $state<string | null>(null);
  let busy = $state(false);
  let confirmRemove = $state(false);
  let orphans = $state<{ modId: number; name: string }[]>([]);

  const config = $derived(tsStore.game(game.id));
  const record = $derived(tsStore.installed(game.id, mod.id));
  const job = $derived(tsStore.progress[game.id] ?? null);
  const mine = $derived(job?.modId === mod.id);
  const latest = $derived(record ? tsStore.latest(game.id, mod.id) : null);
  const loader = $derived(tsStore.loaders[game.id] ?? null);

  /** Mods nécessaires qui seront installés avec celui-ci (le chargeur du jeu sous son vrai nom). */
  const missing = $derived.by(() => {
    const ready = !!loader?.suitable;
    const names = mod.dependencies
      .filter((d) => !tsStore.installed(game.id, d.id) && !(isLoaderName(d.name) && ready))
      .map((d) => (isLoaderName(d.name) && loader?.requiredLabel ? loader.requiredLabel : d.name.replaceAll("_", " ")));
    // Aucun mod ne le demande, mais le jeu en a besoin : installé d'abord.
    if (loader?.needed && !ready && !mod.dependencies.some((d) => isLoaderName(d.name)) && !isLoaderName(mod.name)) {
      names.unshift(loader.requiredLabel ?? loader.needed.name);
    }
    return [...new Set(names)];
  });
  const unsupported = $derived(loader ? !loader.supported : !game.supported);

  /** Installé, et dont d'autres mods installés ont besoin. */
  const neededBy = $derived(
    record && config
      ? Object.values(config.mods)
          .filter((m) => m.modId !== mod.id && record.requiredBy.includes(m.modId))
          .map((m) => m.name)
      : [],
  );

  export async function install(version?: string) {
    error = null;
    note = null;
    orphans = [];
    try {
      const r = await tsStore.install(game, mod.namespace, mod.name, mod.id, version);
      const others = r.installed.filter((n) => n !== mod.name.replaceAll("_", " "));
      const parts = [];
      if (others.length) parts.push(`Installé avec ${others.length > 1 ? "les mods nécessaires" : "le mod nécessaire"} : ${others.join(", ")}.`);
      if (r.keptLoader) parts.push(`${loader?.name ?? "BepInEx"} était déjà installé : il a été gardé tel quel.`);
      note = parts.join(" ") || null;
    } catch (e) {
      const err = asAppError(e);
      if (err.kind !== "cancelled") error = err;
    }
  }

  async function toggle(enabled: boolean) {
    busy = true;
    error = null;
    try {
      await tsStore.setEnabled(game.id, mod.id, enabled);
    } catch (e) {
      error = asAppError(e);
    } finally {
      busy = false;
    }
  }

  let removeTimer: ReturnType<typeof setTimeout> | undefined;

  async function remove() {
    if (!confirmRemove) {
      confirmRemove = true;
      clearTimeout(removeTimer);
      removeTimer = setTimeout(() => (confirmRemove = false), 4000);
      return;
    }
    confirmRemove = false;
    busy = true;
    error = null;
    note = null;
    try {
      const r = await tsStore.uninstall(game.id, mod.id);
      orphans = r.orphans;
      if (r.leftFolder) note = `Le dossier « ${r.leftFolder} » contient des fichiers créés depuis : il a été laissé en place.`;
    } catch (e) {
      error = asAppError(e);
    } finally {
      busy = false;
    }
  }

  async function removeOrphans() {
    busy = true;
    error = null;
    try {
      for (const o of orphans) await tsStore.uninstall(game.id, o.modId);
      orphans = [];
    } catch (e) {
      error = asAppError(e);
    } finally {
      busy = false;
    }
  }
</script>

<section class="panel" aria-label="Installation">
  {#if !config}
    {#if !unsupported}<p class="small">Pour installer ce mod, indique d'abord où se trouve {game.name}.</p>{/if}
    <TsGameSetup {game} />
  {:else if mine && job}
    <ProgressBar progress={job} oncancel={() => tsStore.cancel(game)} />
  {:else if record}
    <div class="state">
      <p class="installed">
        <Icon name="check" size={16} />
        <span>Installé{record.version ? ` (${record.version})` : ""}{record.disabled ? ", désactivé" : ""}</span>
      </p>
      <ModToggle enabled={!record.disabled} name={record.name} busy={busy || !!job} ontoggle={toggle} />
    </div>
    {#if latest && !record.disabled}
      <button class="btn btn-primary" onclick={() => install()} disabled={!!job}>
        <Icon name="download" size={15} />Mettre à jour vers {latest}
      </button>
    {/if}
    {#if record.auto}
      <p class="small muted">
        Installé comme mod nécessaire{neededBy.length ? ` de ${neededBy.join(", ")}` : ""}.
      </p>
    {/if}
    <div class="row">
      <button class="btn" onclick={() => ts.reveal(game.id, mod.id).catch((e) => (error = asAppError(e)))}>Ouvrir le dossier</button>
      <button class="btn btn-ghost" class:danger={confirmRemove} onclick={remove} disabled={busy || !!job}>
        {confirmRemove ? "Confirmer" : "Désinstaller"}
      </button>
    </div>
  {:else if unsupported}
    <p class="small">
      {game.name} charge ses mods avec {loader?.requiredLabel ?? game.loader ?? "un chargeur inconnu"} : ModStudio ne
      sait pas encore les installer. Pour ce jeu, utilise un gestionnaire comme r2modman ou Gale.
    </p>
  {:else}
    <button class="btn btn-primary big" onclick={() => install()} disabled={!!job}>
      <Icon name="download" size={16} />Installer
    </button>
    {#if job}
      <p class="small muted">Une autre installation est en cours pour ce jeu.</p>
    {:else if missing.length}
      <p class="small muted">
        Avec {missing.length > 1 ? `les ${missing.length} mods nécessaires` : "le mod nécessaire"} : {missing.join(", ")}.
      </p>
    {/if}
  {/if}

  {#if orphans.length}
    <div class="orphans">
      <p class="small">
        {orphans.length > 1 ? "Ces mods nécessaires ne servent plus" : "Ce mod nécessaire ne sert plus"} :
        {orphans.map((o) => o.name).join(", ")}.
      </p>
      <div class="row">
        <button class="btn" onclick={removeOrphans} disabled={busy}>Les désinstaller aussi</button>
        <button class="btn btn-ghost" onclick={() => (orphans = [])}>Les garder</button>
      </div>
    </div>
  {/if}

  {#if error}<ErrorNotice {error} />{/if}
  {#if note}<p class="small note" role="status">{note}</p>{/if}
</section>

<style>
  .panel {
    display: grid;
    gap: 0.8rem;
    padding: 1.1rem 1.15rem;
    border-radius: var(--r-md);
    background: var(--panel);
    min-width: 0;
  }

  /* Le choix du dossier, dans la colonne étroite. */
  .panel :global(.setup) {
    margin: 0;
    padding: 0;
    background: none;
  }

  .state {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.75rem;
  }

  .installed {
    display: flex;
    align-items: center;
    gap: 0.45rem;
    font-weight: 600;
  }

  .installed :global(svg) {
    color: var(--ok);
  }

  .row {
    display: flex;
    flex-wrap: wrap;
    gap: 0.4rem;
  }

  .big {
    justify-content: center;
    padding-block: 0.7rem;
    font-size: var(--t-md);
  }

  .small {
    font-size: var(--t-sm);
  }

  .orphans {
    display: grid;
    gap: 0.5rem;
    padding-top: 0.6rem;
    border-top: 1px solid var(--line);
  }

  .danger {
    background: var(--danger-bg);
    border-color: var(--danger);
    color: var(--danger);
  }

  .note {
    color: var(--ok);
  }
</style>
