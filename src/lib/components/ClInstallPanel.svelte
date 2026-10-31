<script lang="ts">
  // Installer, mettre à jour, désactiver ou désinstaller un mod d'une liste de communauté.
  import { asAppError, cl, type AppError, type ClGame, type ClMod } from "$lib/api";
  import { tr } from "$lib/i18n.svelte";
  import { listsStore } from "$lib/lists.svelte";
  import ClSetup from "./ClSetup.svelte";
  import ErrorNotice from "./ErrorNotice.svelte";
  import Icon from "./Icon.svelte";
  import ModToggle from "./ModToggle.svelte";
  import ProgressBar from "./ProgressBar.svelte";

  let { game, mod }: { game: ClGame; mod: ClMod } = $props();

  let error = $state<AppError | null>(null);
  let note = $state<string | null>(null);
  let busy = $state(false);
  let confirmRemove = $state(false);
  let orphans = $state<{ modId: number; name: string }[]>([]);

  const config = $derived(listsStore.config(game));
  const record = $derived(listsStore.installed(game.gameId, mod.modId));
  const job = $derived(listsStore.progress[game.gameId] ?? null);
  const mine = $derived(job?.modId === mod.modId);
  const latest = $derived(record ? listsStore.latest(game.gameId, mod.modId) : null);
  const loader = $derived(listsStore.loaders[game.id] ?? null);

  const blocked = $derived(
    mod.broken ? (mod.note ?? tr("Ce mod est signalé comme cassé.", "This mod is marked as broken.")) : !mod.compatible ? mod.note : null,
  );

  /** Ce qui sera installé avec ce mod. */
  const missing = $derived.by(() => {
    const names = (mod.versions[0]?.dependencies ?? [])
      .filter((d) => !d.optional)
      .filter((d) => {
        const dep = listsStore.mod(game, d.id);
        return !dep || !listsStore.installed(game.gameId, dep.modId);
      })
      .map((d) => d.name);
    if (!mod.loader && loader && !loader.present && loader.installable) names.unshift(loader.name);
    return [...new Set(names)];
  });

  /** Où viennent les fichiers, quand la liste ne publie pas leur empreinte. */
  const unchecked = $derived(
    game.id === "raft"
      ? tr(
          "RaftModding ne publie pas d'empreinte pour ses fichiers : ils sont seulement analysés par VirusTotal sur son site.",
          "RaftModding doesn't publish hashes for its files: they're only scanned by VirusTotal on its website.",
        )
      : game.id === "outer-wilds"
        ? tr(
            "La base d'OWML ne publie pas d'empreinte : le fichier vient de la page GitHub du mod.",
            "The OWML database doesn't publish hashes: the file comes from the mod's GitHub page.",
          )
        : null,
  );

  const neededBy = $derived(
    record && config
      ? Object.values(config.mods)
          .filter((m) => m.modId !== mod.modId && record.requiredBy.includes(m.modId))
          .map((m) => m.name)
      : [],
  );

  export async function install(version?: string) {
    error = null;
    note = null;
    orphans = [];
    try {
      const r = await listsStore.install(game, mod, version);
      const others = r.installed.filter((n) => n !== mod.name);
      if (others.length)
        note = tr(
          `Installé avec ${others.length > 1 ? "les mods nécessaires" : "le mod nécessaire"} : ${others.join(", ")}.`,
          `Installed with ${others.length > 1 ? "the required mods" : "the required mod"}: ${others.join(", ")}.`,
        );
    } catch (e) {
      const err = asAppError(e);
      if (err.kind !== "cancelled") error = err;
    }
  }

  async function toggle(enabled: boolean) {
    busy = true;
    error = null;
    try {
      await listsStore.setEnabled(game.gameId, mod.modId, enabled);
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
      const r = await listsStore.uninstall(game.gameId, mod.modId);
      orphans = r.orphans;
      if (r.leftFolder)
        note = tr(
          `Le dossier « ${r.leftFolder} » contient des fichiers créés depuis : il a été laissé en place.`,
          `The "${r.leftFolder}" folder contains files created since the install: it was left in place.`,
        );
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
      // Un à un : le chargeur peut ne servir plus qu'à eux.
      let queue = orphans;
      while (queue.length) {
        const next: { modId: number; name: string }[] = [];
        for (const o of queue) next.push(...(await listsStore.uninstall(game.gameId, o.modId)).orphans);
        queue = next;
      }
      orphans = [];
    } catch (e) {
      error = asAppError(e);
    } finally {
      busy = false;
    }
  }
</script>

<section class="panel" aria-label={tr("Installation", "Installation")}>
  {#if !config}
    <p class="small">{tr(`Pour installer ce mod, indique d'abord où se trouve ${game.name}.`, `To install this mod, first tell ModStudio where ${game.name} is.`)}</p>
    <ClSetup {game} />
  {:else if mine && job}
    <ProgressBar progress={job} oncancel={() => listsStore.cancel(game)} />
  {:else if record}
    <div class="state">
      <p class="installed">
        <Icon name="check" size={16} />
        <span>{tr("Installé", "Installed")}{record.version ? ` (${record.version})` : ""}{record.disabled ? tr(", désactivé", ", disabled") : ""}</span>
      </p>
      <ModToggle enabled={!record.disabled} name={record.name} busy={busy || !!job} ontoggle={toggle} />
    </div>
    {#if latest && !record.disabled}
      <button class="btn btn-primary" onclick={() => install()} disabled={!!job}>
        <Icon name="download" size={15} />{tr(`Mettre à jour vers ${latest}`, `Update to ${latest}`)}
      </button>
    {/if}
    {#if record.auto}
      <p class="small muted">
        {tr(
          `Installé comme mod nécessaire${neededBy.length ? ` de ${neededBy.join(", ")}` : ""}.`,
          `Installed as a required mod${neededBy.length ? ` for ${neededBy.join(", ")}` : ""}.`,
        )}
      </p>
    {/if}
    <div class="row">
      <button class="btn" onclick={() => cl.reveal(game.gameId, mod.modId).catch((e) => (error = asAppError(e)))}>{tr("Ouvrir le dossier", "Open folder")}</button>
      <button class="btn btn-ghost" class:danger={confirmRemove} onclick={remove} disabled={busy || !!job}>
        {confirmRemove ? tr("Confirmer", "Confirm") : tr("Désinstaller", "Uninstall")}
      </button>
    </div>
  {:else if blocked}
    <p class="small">{blocked}</p>
  {:else if mod.loader && loader?.present}
    <p class="small">
      {tr(
        `${loader.name} est déjà installé dans le dossier du jeu (sans ModStudio) : il est gardé tel quel.`,
        `${loader.name} is already installed in the game folder (without ModStudio): it's kept as is.`,
      )}
    </p>
  {:else}
    <button class="btn btn-primary big" onclick={() => install()} disabled={!!job}>
      <Icon name="download" size={16} />{tr("Installer", "Install")}
    </button>
    {#if job}
      <p class="small muted">{tr("Une autre installation est en cours pour ce jeu.", "Another installation is in progress for this game.")}</p>
    {:else if missing.length}
      <p class="small muted">
        {tr(
          `Avec ${missing.length > 1 ? `les ${missing.length} mods nécessaires` : "le mod nécessaire"} : ${missing.join(", ")}.`,
          `With ${missing.length > 1 ? `the ${missing.length} required mods` : "the required mod"}: ${missing.join(", ")}.`,
        )}
      </p>
    {/if}
    {#if unchecked}<p class="small muted">{unchecked}</p>{/if}
  {/if}

  {#if orphans.length}
    <div class="orphans">
      <p class="small">
        {tr(
          `${orphans.length > 1 ? "Ces mods nécessaires ne servent plus" : "Ce mod nécessaire ne sert plus"} : ${orphans.map((o) => o.name).join(", ")}.`,
          `${orphans.length > 1 ? "These required mods are no longer needed" : "This required mod is no longer needed"}: ${orphans.map((o) => o.name).join(", ")}.`,
        )}
      </p>
      <div class="row">
        <button class="btn" onclick={removeOrphans} disabled={busy}>{tr("Les désinstaller aussi", "Uninstall them too")}</button>
        <button class="btn btn-ghost" onclick={() => (orphans = [])}>{tr("Les garder", "Keep them")}</button>
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
