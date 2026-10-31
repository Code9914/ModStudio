<script lang="ts">
  import { untrack } from "svelte";
  import { SvelteMap, SvelteSet } from "svelte/reactivity";
  import {
    api,
    asAppError,
    nexus,
    cl,
    clIdOf,
    rmlIdOf,
    ts,
    type AppError,
    type GameInstall,
    type InstalledMod,
    type InstalledTool,
  } from "$lib/api";
  import { engineNames, nexusGameOf, nexusStore } from "$lib/nexus.svelte";
  import { splitFullName, tsStore } from "$lib/thunderstore.svelte";
  import { listsStore } from "$lib/lists.svelte";
  import NexusFolderSetup from "./NexusFolderSetup.svelte";
  import ModToggle from "./ModToggle.svelte";
  import ProfileBar from "./ProfileBar.svelte";
  import SharePanel from "./SharePanel.svelte";
  import { ago, bytes, exact } from "$lib/format";
  import { installs, layoutLabels } from "$lib/installs.svelte";
  import { nav } from "$lib/nav.svelte";
  import { english, tr } from "$lib/i18n.svelte";
  import ErrorNotice from "./ErrorNotice.svelte";
  import FolderSetup from "./FolderSetup.svelte";
  import Icon from "./Icon.svelte";
  import PlayButton from "./PlayButton.svelte";
  import ProgressBar from "./ProgressBar.svelte";
  import Thumb from "./Thumb.svelte";

  let error = $state<AppError | null>(null);
  let query = $state("");
  /** Jeu dont on partage les mods (`source:jeu`). */
  let sharing = $state<string | null>(null);
  const toggleShare = (key: string) => (sharing = sharing === key ? null : key);
  /** Jeu dont on modifie le dossier. */
  let editing = $state<number | null>(null);
  const rowErrors = new SvelteMap<number, string>();
  const notes = new SvelteMap<number, string>();
  const confirming = new SvelteSet<number>();
  const removing = new SvelteSet<number>();
  const updatingAll = new SvelteSet<number>();

  async function refresh() {
    error = null;
    try {
      await Promise.all([installs.load(), nexusStore.load(), tsStore.load(), listsStore.load()]);
      await Promise.all([
        ...Object.keys(installs.games).map((id) => installs.checkUpdates(Number(id))),
        ...Object.keys(nexusStore.games).map((id) => nexusStore.checkUpdates(Number(id))),
        ...Object.keys(tsStore.games).map((id) => tsStore.checkUpdates(Number(id))),
        ...Object.keys(listsStore.installs).map((id) => listsStore.checkUpdates(Number(id))),
      ]);
      // Nouvelles versions des chargeurs de mods installés par ModStudio.
      for (const g of Object.values(nexusStore.games)) {
        if (g.tools && Object.keys(g.tools).length > 0) nexusStore.refreshSetup(g.gameId);
      }
    } catch (e) {
      error = asAppError(e);
    }
  }

  // Rafraîchi à chaque visite de la page.
  $effect(() => {
    if (nav.view.name === "installed") untrack(() => refresh());
  });

  /** Seuls les jeux qui ont au moins un mod installé apparaissent ici (un chargeur de mods compte comme un mod). */
  const hasMods = (g: GameInstall) => Object.keys(g.mods).length > 0;

  const games = $derived(
    Object.values(installs.games)
      .filter(hasMods)
      .sort((a, b) => a.gameName.localeCompare(b.gameName, "fr")),
  );

  function mods(g: GameInstall): InstalledMod[] {
    const q = query.trim().toLowerCase();
    return Object.values(g.mods)
      .filter((m) => !q || m.name.toLowerCase().includes(q))
      .sort((a, b) => a.name.localeCompare(b.name, "fr"));
  }

  /** Mods activés à mettre à jour (un mod désactivé ne se réactive pas tout seul). */
  function outdatedIn(g: GameInstall) {
    return Object.values(g.mods).filter((m) => !m.disabled && installs.outdated(g.gameId, m.modId));
  }

  const nexusGames = $derived(
    Object.values(nexusStore.games)
      .filter((g) => nexusStore.count(g) > 0)
      .sort((a, b) => a.gameName.localeCompare(b.gameName, "fr")),
  );

  const tsGames = $derived(
    Object.values(tsStore.games)
      .filter((g) => tsStore.count(g) > 0)
      .sort((a, b) => a.gameName.localeCompare(b.gameName, "fr")),
  );

  const clGames = $derived(
    Object.values(listsStore.installs)
      .filter((g) => listsStore.count(g) > 0)
      .sort((a, b) => a.gameName.localeCompare(b.gameName, "fr")),
  );

  const total = $derived(
    games.reduce((n, g) => n + Object.keys(g.mods).length, 0) +
      nexusGames.reduce((n, g) => n + nexusStore.count(g), 0) +
      tsGames.reduce((n, g) => n + tsStore.count(g), 0) +
      clGames.reduce((n, g) => n + listsStore.count(g), 0),
  );
  const withMods = $derived(games.length + nexusGames.length + tsGames.length + clGames.length);
  const loaded = $derived(installs.loaded && nexusStore.loaded && tsStore.loaded && listsStore.loaded);

  async function update(g: GameInstall, m: InstalledMod) {
    rowErrors.delete(m.modId);
    notes.delete(m.modId);
    try {
      await installs.install(g.gameId, m.modId);
      notes.set(m.modId, tr("Mis à jour.", "Updated."));
    } catch (e) {
      const err = asAppError(e);
      if (err.kind !== "cancelled") rowErrors.set(m.modId, err.message);
    }
  }

  async function updateAll(g: GameInstall) {
    updatingAll.add(g.gameId);
    for (const m of outdatedIn(g)) await update(g, m);
    updatingAll.delete(g.gameId);
  }

  const timers = new Map<number, ReturnType<typeof setTimeout>>();

  async function uninstall(g: GameInstall, m: InstalledMod) {
    if (!confirming.has(m.modId)) {
      confirming.add(m.modId);
      clearTimeout(timers.get(m.modId));
      timers.set(m.modId, setTimeout(() => confirming.delete(m.modId), 4000));
      return;
    }
    confirming.delete(m.modId);
    removing.add(m.modId);
    rowErrors.delete(m.modId);
    try {
      const r = await installs.uninstall(g.gameId, m.modId);
      if (r.leftFolder) {
        notes.set(-g.gameId, tr(
            `« ${m.name} » désinstallé. Son dossier contient des fichiers ajoutés depuis, il a été laissé en place.`,
            `"${m.name}" uninstalled. Its folder contains files added since, so it was left in place.`,
          ));
      }
      if (r.orphans.length) orphans.set(g.gameId, { parent: m.name, mods: r.orphans });
    } catch (e) {
      rowErrors.set(m.modId, asAppError(e).message);
    } finally {
      removing.delete(m.modId);
    }
  }

  // ------------------------------------------------------------ Activer / désactiver
  const toggling = new SvelteSet<string>();
  // Incompatibilités déclarées des mods des listes (avertissement par jeu).
  $effect(() => {
    if (!listsStore.loaded) return;
    untrack(() => {
      for (const g of Object.values(listsStore.installs)) {
        const game = listsStore.gameOf(g);
        if (game) listsStore.loadDeclared(game);
      }
    });
  });

  /** Mod qu'on ne peut pas réactiver : ses fichiers sont pris par d'autres mods (on propose de les désactiver). */
  let swap = $state<{ key: string; name: string; error: AppError; source: "modio" | "nexus" | "ts" | "cl"; redo: () => void } | null>(null);

  async function toggle(source: "modio" | "nexus" | "ts" | "cl", g: GameInstall, m: InstalledMod, enabled: boolean) {
    const k = `${source}:${g.gameId}:${m.modId}`;
    toggling.add(k);
    if (source === "modio") rowErrors.delete(m.modId);
    else if (source === "ts") tsErrors.delete(tsKey(g, m));
    else if (source === "cl") clErrors.delete(tsKey(g, m));
    else nxErrors.delete(nexusStore.key(g.gameId, m.modId));
    try {
      if (source === "modio") await installs.setEnabled(g.gameId, m.modId, enabled);
      else if (source === "ts") await tsStore.setEnabled(g.gameId, m.modId, enabled);
      else if (source === "cl") await listsStore.setEnabled(g.gameId, m.modId, enabled);
      else await nexusStore.setEnabled(g.gameId, m.modId, enabled);
      if (swap?.key === k) swap = null;
    } catch (e) {
      const err = asAppError(e);
      const message = err.message;
      if (enabled && err.conflictWith?.length) swap = { key: k, name: m.name, error: err, source, redo: () => toggle(source, g, m, true) };
      if (source === "modio") rowErrors.set(m.modId, message);
      else if (source === "ts") tsErrors.set(tsKey(g, m), message);
      else if (source === "cl") clErrors.set(tsKey(g, m), message);
      else nxErrors.set(nexusStore.key(g.gameId, m.modId), message);
    } finally {
      toggling.delete(k);
    }
  }

  /** Mods nécessaires devenus inutiles après une désinstallation, par jeu. */
  const orphans = new SvelteMap<number, { parent: string; mods: { modId: number; name: string }[] }>();

  async function removeOrphans(g: GameInstall) {
    const entry = orphans.get(g.gameId);
    orphans.delete(g.gameId);
    if (!entry) return;
    for (const o of entry.mods) {
      removing.add(o.modId);
      try {
        await installs.uninstall(g.gameId, o.modId);
      } catch (e) {
        rowErrors.set(o.modId, asAppError(e).message);
      } finally {
        removing.delete(o.modId);
      }
    }
  }

  function parentNames(g: GameInstall, m: InstalledMod) {
    return m.requiredBy.map((id) => g.mods[id]?.name ?? `mod ${id}`);
  }

  async function openMod(g: GameInstall, m: InstalledMod) {
    try {
      const game = await api.getGame(g.gameId);
      nav.openModFromLibrary(game, m.modId, "installed");
    } catch (e) {
      rowErrors.set(m.modId, asAppError(e).message);
    }
  }

  function reveal(gameId: number, modId: number | null) {
    api.revealInstall(gameId, modId).catch((e) => (error = asAppError(e)));
  }

  // ------------------------------------------------------------ Nexus Mods

  /** Les identifiants Nexus sont propres à chaque jeu : clés `jeu:mod`. */
  const nxKey = (g: GameInstall, m: InstalledMod) => nexusStore.key(g.gameId, m.modId);
  let nexusEditing = $state<number | null>(null);
  const nxErrors = new SvelteMap<string, string>();
  const nxConfirming = new SvelteSet<string>();
  const nxRemoving = new SvelteSet<string>();
  const nxNotes = new SvelteMap<number, string>();

  function nexusOpen(g: GameInstall, m: InstalledMod) {
    if (!g.domain) return;
    nav.openNexusModFromInstalled(nexusGameOf(g), m.modId);
  }

  async function nexusUninstall(g: GameInstall, m: InstalledMod) {
    const k = nxKey(g, m);
    if (!nxConfirming.has(k)) {
      nxConfirming.add(k);
      setTimeout(() => nxConfirming.delete(k), 4000);
      return;
    }
    nxConfirming.delete(k);
    nxRemoving.add(k);
    nxErrors.delete(k);
    try {
      const r = await nexusStore.uninstall(g.gameId, m.modId);
      if (r.leftFolder) {
        nxNotes.set(g.gameId, tr(
            `« ${m.name} » désinstallé. Son dossier contient des fichiers ajoutés depuis, il a été laissé en place.`,
            `"${m.name}" uninstalled. Its folder contains files added since, so it was left in place.`,
          ));
      }
      if (r.orphans.length) nxOrphans.set(g.gameId, { parent: m.name, mods: r.orphans });
    } catch (e) {
      nxErrors.set(k, asAppError(e).message);
    } finally {
      nxRemoving.delete(k);
    }
  }

  /** Mods nécessaires Nexus devenus inutiles après une désinstallation, par jeu. */
  const nxOrphans = new SvelteMap<number, { parent: string; mods: { modId: number; name: string }[] }>();

  async function removeNexusOrphans(g: GameInstall) {
    const entry = nxOrphans.get(g.gameId);
    nxOrphans.delete(g.gameId);
    if (!entry) return;
    for (const o of entry.mods) {
      const k = nexusStore.key(g.gameId, o.modId);
      nxRemoving.add(k);
      try {
        await nexusStore.uninstall(g.gameId, o.modId);
      } catch (e) {
        nxErrors.set(k, asAppError(e).message);
      } finally {
        nxRemoving.delete(k);
      }
    }
  }

  function nexusReveal(gameId: number, modId: number | null) {
    nexus.reveal(gameId, modId).catch((e) => (error = asAppError(e)));
  }

  function nexusMods(g: GameInstall): InstalledMod[] {
    return mods(g);
  }

  /** Chargeurs de mods d'un jeu, affichés comme des mods : ceux de ModStudio, puis ceux installés à la main. */
  type LoaderRow = { key: string; name: string; tool: InstalledTool | null; update: string | null };
  function loaders(g: GameInstall): LoaderRow[] {
    const q = query.trim().toLowerCase();
    const offer = nexusStore.setups[g.gameId]?.tool;
    const rows: LoaderRow[] = [
      ...Object.values(g.tools ?? {}).map((t) => ({
        key: `tool:${t.id}`,
        name: t.name,
        tool: t,
        update: offer?.managed && offer.id === t.id ? offer.update : null,
      })),
      ...(nexusStore.found[g.gameId] ?? []).map((l) => ({ key: `found:${l.id}`, name: l.name, tool: null, update: null })),
    ];
    return rows.filter((r) => !q || r.name.toLowerCase().includes(q));
  }

  const ruleLabels: Record<string, string> = {
    bonelab: tr("rangement automatique Bonelab", "automatic Bonelab layout"),
    unreal: tr(`rangement automatique (${engineNames.unreal})`, `automatic layout (${engineNames.unreal})`),
    unity: tr("rangement automatique (Unity)", "automatic layout (Unity)"),
    bethesda: tr(`rangement automatique (${engineNames.bethesda})`, `automatic layout (${engineNames.bethesda})`),
  };

  async function removeTool(g: GameInstall, toolId: string, name: string) {
    const k = `tool:${g.gameId}:${toolId}`;
    if (!nxConfirming.has(k)) {
      nxConfirming.add(k);
      setTimeout(() => nxConfirming.delete(k), 4000);
      return;
    }
    nxConfirming.delete(k);
    nxRemoving.add(k);
    try {
      const left = await nexusStore.uninstallTool(g.gameId, g.domain ?? "", toolId);
      nxNotes.set(
        g.gameId,
        left
          ? tr(
              `${name} retiré. Le dossier « ${left} » contient des fichiers créés depuis, il a été laissé en place.`,
              `${name} removed. The "${left}" folder contains files created since, so it was left in place.`,
            )
          : tr(`${name} retiré.`, `${name} removed.`),
      );
    } catch (e) {
      nxNotes.set(g.gameId, asAppError(e).message);
    } finally {
      nxRemoving.delete(k);
    }
  }

  // ------------------------------------------------------------ Thunderstore

  const tsKey = (g: GameInstall, m: { modId: number }) => `${g.gameId}:${m.modId}`;
  const tsErrors = new SvelteMap<string, string>();
  const tsNotes = new SvelteMap<string, string>();
  const tsConfirming = new SvelteSet<string>();
  const tsRemoving = new SvelteSet<string>();
  const tsGameNotes = new SvelteMap<number, string>();
  const tsOrphans = new SvelteMap<number, { parent: string; mods: { modId: number; name: string }[] }>();
  const tsUpdatingAll = new SvelteSet<number>();

  function tsOpen(g: GameInstall, m: InstalledMod) {
    const rmlId = rmlIdOf(m.fileName);
    if (rmlId && g.domain) return nav.openRmlModFromInstalled(tsStore.communityOf(g), rmlId);
    const ref = splitFullName(m.fileName);
    if (ref && g.domain) nav.openTsModFromInstalled(tsStore.communityOf(g), ref);
  }

  function tsOutdatedIn(g: GameInstall) {
    return Object.values(g.mods).filter((m) => !m.disabled && tsStore.outdated(g.gameId, m.modId));
  }

  async function tsUpdate(g: GameInstall, m: InstalledMod) {
    const rmlId = rmlIdOf(m.fileName);
    const ref = splitFullName(m.fileName);
    if ((!rmlId && !ref) || !g.domain) return;
    const k = tsKey(g, m);
    tsErrors.delete(k);
    tsNotes.delete(k);
    try {
      if (rmlId) {
        // Mod de ResoniteModLoader : mis à jour depuis sa liste.
        await tsStore.loadRml();
        const mod = tsStore.rmlMod(rmlId);
        if (!mod) throw tsStore.rmlError ?? { kind: "not_found", message: tr("Ce mod n'est plus dans la liste de ResoniteModLoader.", "This mod is no longer in the ResoniteModLoader list."), retryAfter: null };
        await tsStore.installRml(tsStore.communityOf(g), mod);
      } else if (ref) {
        await tsStore.install(tsStore.communityOf(g), ref.namespace, ref.name, m.modId);
      }
      tsNotes.set(k, tr("Mis à jour.", "Updated."));
    } catch (e) {
      const err = asAppError(e);
      if (err.kind !== "cancelled") tsErrors.set(k, err.message);
    }
  }

  async function tsUpdateAll(g: GameInstall) {
    tsUpdatingAll.add(g.gameId);
    for (const m of tsOutdatedIn(g)) await tsUpdate(g, m);
    tsUpdatingAll.delete(g.gameId);
  }

  async function tsUninstall(g: GameInstall, m: InstalledMod) {
    const k = tsKey(g, m);
    if (!tsConfirming.has(k)) {
      tsConfirming.add(k);
      setTimeout(() => tsConfirming.delete(k), 4000);
      return;
    }
    tsConfirming.delete(k);
    tsRemoving.add(k);
    tsErrors.delete(k);
    try {
      const r = await tsStore.uninstall(g.gameId, m.modId);
      if (r.leftFolder) {
        tsGameNotes.set(g.gameId, tr(
            `« ${m.name} » désinstallé. Son dossier contient des fichiers ajoutés depuis, il a été laissé en place.`,
            `"${m.name}" uninstalled. Its folder contains files added since, so it was left in place.`,
          ));
      }
      if (r.orphans.length) tsOrphans.set(g.gameId, { parent: m.name, mods: r.orphans });
    } catch (e) {
      tsErrors.set(k, asAppError(e).message);
    } finally {
      tsRemoving.delete(k);
    }
  }

  async function removeTsOrphans(g: GameInstall) {
    const entry = tsOrphans.get(g.gameId);
    tsOrphans.delete(g.gameId);
    if (!entry) return;
    for (const o of entry.mods) {
      const k = tsKey(g, o);
      tsRemoving.add(k);
      try {
        await tsStore.uninstall(g.gameId, o.modId);
      } catch (e) {
        tsErrors.set(k, asAppError(e).message);
      } finally {
        tsRemoving.delete(k);
      }
    }
  }

  function tsReveal(gameId: number, modId: number | null) {
    ts.reveal(gameId, modId).catch((e) => (error = asAppError(e)));
  }

  // ------------------------------------------------------------ Listes des communautés

  const clErrors = new SvelteMap<string, string>();
  const clNotes = new SvelteMap<string, string>();
  const clConfirming = new SvelteSet<string>();
  const clRemoving = new SvelteSet<string>();
  const clGameNotes = new SvelteMap<number, string>();
  const clOrphans = new SvelteMap<number, { parent: string; mods: { modId: number; name: string }[] }>();
  const clUpdatingAll = new SvelteSet<number>();

  function clOpen(g: GameInstall, m: InstalledMod) {
    const game = listsStore.gameOf(g);
    const id = clIdOf(m.fileName);
    if (game && id) nav.openClModFromInstalled(game, id);
  }

  function clOutdatedIn(g: GameInstall) {
    return Object.values(g.mods).filter((m) => !m.disabled && listsStore.outdated(g.gameId, m.modId));
  }

  async function clUpdate(g: GameInstall, m: InstalledMod) {
    const game = listsStore.gameOf(g);
    const id = clIdOf(m.fileName);
    if (!game || !id) return;
    const k = tsKey(g, m);
    clErrors.delete(k);
    clNotes.delete(k);
    try {
      await listsStore.loadMods(game);
      const mod = listsStore.mod(game, id);
      if (!mod) throw listsStore.errors[game.id] ?? { kind: "not_found", message: tr(`Ce mod n'est plus dans la liste ${game.listName}.`, `This mod is no longer in the ${game.listName} list.`), retryAfter: null };
      await listsStore.install(game, mod);
      clNotes.set(k, tr("Mis à jour.", "Updated."));
    } catch (e) {
      const err = asAppError(e);
      if (err.kind !== "cancelled") clErrors.set(k, err.message);
    }
  }

  async function clUpdateAll(g: GameInstall) {
    clUpdatingAll.add(g.gameId);
    for (const m of clOutdatedIn(g)) await clUpdate(g, m);
    clUpdatingAll.delete(g.gameId);
  }

  async function clUninstall(g: GameInstall, m: InstalledMod) {
    const k = tsKey(g, m);
    if (!clConfirming.has(k)) {
      clConfirming.add(k);
      setTimeout(() => clConfirming.delete(k), 4000);
      return;
    }
    clConfirming.delete(k);
    clRemoving.add(k);
    clErrors.delete(k);
    try {
      const r = await listsStore.uninstall(g.gameId, m.modId);
      if (r.leftFolder) {
        clGameNotes.set(g.gameId, tr(
            `« ${m.name} » désinstallé. Son dossier contient des fichiers ajoutés depuis, il a été laissé en place.`,
            `"${m.name}" uninstalled. Its folder contains files added since, so it was left in place.`,
          ));
      }
      if (r.orphans.length) clOrphans.set(g.gameId, { parent: m.name, mods: r.orphans });
    } catch (e) {
      clErrors.set(k, asAppError(e).message);
    } finally {
      clRemoving.delete(k);
    }
  }

  async function removeClOrphans(g: GameInstall) {
    const entry = clOrphans.get(g.gameId);
    clOrphans.delete(g.gameId);
    if (!entry) return;
    let queue = entry.mods;
    while (queue.length) {
      const next: { modId: number; name: string }[] = [];
      for (const o of queue) {
        const k = tsKey(g, o);
        clRemoving.add(k);
        try {
          next.push(...(await listsStore.uninstall(g.gameId, o.modId)).orphans);
        } catch (e) {
          clErrors.set(k, asAppError(e).message);
        } finally {
          clRemoving.delete(k);
        }
      }
      queue = next;
    }
  }
</script>

<div class="view">
  <header class="head">
    <div>
      <h1>{tr("Installés", "Installed")}</h1>
      {#if loaded}
        <p class="muted">
          {exact(total)}
          {tr(total > 1 ? "mods installés" : "mod installé", total === 1 ? "mod installed" : "mods installed")}
          {#if withMods > 0}{tr(
              `dans ${withMods} ${withMods > 1 ? "jeux" : "jeu"}`,
              `in ${withMods} ${withMods === 1 ? "game" : "games"}`,
            )}{/if}
        </p>
      {/if}
    </div>
    <div class="tools">
      {#if total > 0}
        <label class="search">
          <span class="visually-hidden">{tr("Filtrer les mods installés", "Filter installed mods")}</span>
          <Icon name="search" />
          <input class="field" type="search" placeholder={tr("Filtrer les mods installés", "Filter installed mods")} bind:value={query} />
        </label>
        <button class="btn" onclick={refresh}><Icon name="refresh" size={15} />{tr("Vérifier les mises à jour", "Check for updates")}</button>
      {/if}
      <button class="btn" onclick={() => nav.openImport()}><Icon name="import" size={15} />{tr("Importer un profil", "Import a profile")}</button>
    </div>
  </header>

  {#if swap}
    <div class="swap">
      <p class="swap-title">{tr(`Réactiver « ${swap.name} »`, `Enable "${swap.name}" again`)}</p>
      <ErrorNotice error={swap.error} source={swap.source} redo={swap.redo} />
    </div>
  {/if}

  {#if error}
    <ErrorNotice {error} onretry={refresh} />
  {:else if loaded && total === 0}
    <div class="empty">
      <p>
        {tr(
          "Aucun mod installé avec ModStudio pour l'instant. Ouvre la fiche d'un mod et clique sur « Installer » : tu choisiras alors le dossier des mods du jeu. Un ami t'a envoyé ses mods ? Clique sur « Importer un profil ».",
          'No mods installed with ModStudio yet. Open a mod\'s page and click "Install": you\'ll then choose the game\'s mods folder. A friend sent you their mods? Click "Import a profile".',
        )}
      </p>
      <div class="empty-actions">
        {#if nav.settings?.hasApiKey}
          <button class="btn btn-primary" onclick={() => nav.openGames()}>{tr("Parcourir mod.io", "Browse mod.io")}</button>
        {/if}
        <button class="btn" onclick={() => nav.openNexusGames()}>{tr("Parcourir Nexus Mods", "Browse Nexus Mods")}</button>
        <button class="btn" onclick={() => nav.openTsGames()}>{tr("Parcourir Thunderstore", "Browse Thunderstore")}</button>
        <button class="btn" onclick={() => nav.openClGames()}>{tr("Jeux des communautés", "Community games")}</button>
      </div>
    </div>
  {/if}

  {#each games as g (g.gameId)}
    {@const list = mods(g)}
    {@const outdated = outdatedIn(g)}
    <section class="group">
      <div class="group-head">
        {#if g.icon}<img class="game-icon" src={g.icon} alt="" width="28" height="28" />{/if}
        <h2>{g.gameName}</h2>
        <span class="chip">mod.io</span>
        <span class="muted count">{Object.keys(g.mods).length}</span>
        <div class="group-actions">
          <PlayButton name={g.gameName} folders={[g.extraFolders?.game, g.folder]} />
          {#if outdated.length > 0}
            <button class="btn btn-primary" onclick={() => updateAll(g)} disabled={updatingAll.has(g.gameId)}>
              <Icon name="download" size={15} />{updatingAll.has(g.gameId)
                ? tr("Mise à jour…", "Updating…")
                : outdated.length > 1
                  ? tr(`Tout mettre à jour (${outdated.length})`, `Update all (${outdated.length})`)
                  : tr("Mettre à jour (1)", "Update (1)")}
            </button>
          {/if}
          <button class="btn btn-ghost" onclick={() => toggleShare(`modio:${g.gameId}`)}><Icon name="share" size={15} />{tr("Partager", "Share")}</button>
          <button class="btn btn-ghost" onclick={() => reveal(g.gameId, null)}>{tr("Ouvrir le dossier", "Open folder")}</button>
          <button class="btn btn-ghost" onclick={() => (editing = editing === g.gameId ? null : g.gameId)}>
            {tr("Réglages du jeu", "Game settings")}
          </button>
        </div>
      </div>
      <p class="folder muted">
        {g.folder}, {layoutLabels[g.layout].title.toLowerCase()}
      </p>

      {#if editing === g.gameId}
        <div class="editor">
          <FolderSetup
            gameId={g.gameId}
            gameName={g.gameName}
            icon={g.icon}
            onsaved={() => (editing = null)}
            oncancel={() => (editing = null)}
          />
        </div>
      {/if}

      {#if sharing === `modio:${g.gameId}`}
        <SharePanel source="modio" game={g} onclose={() => (sharing = null)} />
      {/if}

      {#if Object.keys(g.mods).length > 0}
        <ProfileBar
          game={g}
          onapply={(name) => installs.applyProfile(g.gameId, name)}
          onedit={(edit) => installs.editProfiles(g.gameId, edit)}
        />
      {/if}

      {#if notes.get(-g.gameId)}<p class="note">{notes.get(-g.gameId)}</p>{/if}

      {#if orphans.get(g.gameId)}
        {@const o = orphans.get(g.gameId)!}
        <div class="orphans">
          <p class="small">
            {#if english}
              {o.mods.map((x) => `"${x.name}"`).join(", ")}
              {o.mods.length === 1 ? "was" : "were"} installed with "{o.parent}" and {o.mods.length === 1 ? "is" : "are"} no
              longer needed by any other mod.
            {:else}
              {o.mods.map((x) => `« ${x.name} »`).join(", ")}
              {o.mods.length > 1 ? "avaient été installés" : "avait été installé"} avec « {o.parent} » et ne
              {o.mods.length > 1 ? "servent" : "sert"} plus à aucun autre mod.
            {/if}
          </p>
          <button class="btn btn-primary" onclick={() => removeOrphans(g)}>
            {o.mods.length > 1 ? tr("Les désinstaller aussi", "Uninstall them too") : tr("Le désinstaller aussi", "Uninstall it too")}
          </button>
          <button class="btn btn-ghost" onclick={() => orphans.delete(g.gameId)}>{tr("Garder", "Keep")}</button>
        </div>
      {/if}

      {#if list.length === 0}
        <p class="muted small">
          {tr(`Aucun mod installé ne correspond à « ${query.trim()} ».`, `No installed mod matches "${query.trim()}".`)}
        </p>
      {/if}

      <ul class="rows">
        {#each list as m (m.modId)}
          {@const progress = installs.progress[m.modId]}
          {@const latest = installs.latest[m.modId]}
          {@const isOutdated = installs.outdated(g.gameId, m.modId)}
          <li class="row" class:off={m.disabled}>
            <button class="row-main" onclick={() => openMod(g, m)}>
              <span class="row-thumb"><Thumb src={m.logo} label={m.name} /></span>
              <span class="row-text">
                <span class="row-name">{m.name}</span>
                <span class="row-meta">
                  <span>{m.version ? `version ${m.version}` : tr("version sans numéro", "no version number")}</span>
                  <span>{bytes(m.size)}</span>
                  <span>{tr(`installé ${ago(m.installedAt)}`, `installed ${ago(m.installedAt)}`)}</span>
                  {#if m.disabled}<span class="chip">{tr("Désactivé", "Disabled")}</span>{/if}
                  {#if m.auto && m.requiredBy.length}
                    <span
                      >{tr(
                        `nécessaire à « ${parentNames(g, m).join(" », « ")} »`,
                        `required by "${parentNames(g, m).join('", "')}"`,
                      )}</span
                    >
                  {/if}
                </span>
                {#if rowErrors.get(m.modId)}
                  <span class="row-error">{rowErrors.get(m.modId)}</span>
                {:else if notes.get(m.modId)}
                  <span class="row-note">{notes.get(m.modId)}</span>
                {/if}
              </span>
            </button>
            <div class="row-action">
              {#if progress}
                <div class="row-progress"><ProgressBar {progress} oncancel={() => installs.cancel(m.modId)} /></div>
              {:else}
                <ModToggle
                  enabled={!m.disabled}
                  name={m.name}
                  busy={toggling.has(`modio:${g.gameId}:${m.modId}`)}
                  ontoggle={(on) => toggle("modio", g, m, on)}
                />
                {#if isOutdated}
                  <span class="chip chip-accent">{latest?.version ? `Version ${latest.version}` : tr("Nouvelle version", "New version")}</span>
                  <button class="btn btn-primary" onclick={() => update(g, m)}>{tr("Mettre à jour", "Update")}</button>
                {:else if latest}
                  <span class="muted small">{tr("À jour", "Up to date")}</span>
                {/if}
                <button class="btn btn-ghost" onclick={() => reveal(g.gameId, m.modId)} disabled={m.disabled}>{tr("Ouvrir", "Open")}</button>
                <button
                  class="btn"
                  class:btn-ghost={!confirming.has(m.modId)}
                  class:danger={confirming.has(m.modId)}
                  onclick={() => uninstall(g, m)}
                  disabled={removing.has(m.modId)}
                >
                  {removing.has(m.modId) ? "…" : confirming.has(m.modId) ? tr("Confirmer", "Confirm") : tr("Désinstaller", "Uninstall")}
                </button>
              {/if}
            </div>
          </li>
        {/each}
      </ul>
    </section>
  {/each}

  {#each nexusGames as g (g.gameId)}
    {@const list = nexusMods(g)}
    {@const loaderRows = loaders(g)}
    <section class="group">
      <div class="group-head">
        {#if g.icon}<img class="game-icon" src={g.icon} alt="" width="28" height="28" />{/if}
        <h2>{g.gameName}</h2>
        <span class="chip">Nexus Mods</span>
        <span class="muted count">{nexusStore.count(g)}</span>
        <div class="group-actions">
          <PlayButton name={g.gameName} folders={[g.extraFolders?.game, g.folder]} />
          {#if Object.keys(g.mods).length > 0}
            <button class="btn btn-ghost" onclick={() => toggleShare(`nexus:${g.gameId}`)}><Icon name="share" size={15} />{tr("Partager", "Share")}</button>
          {/if}
          <button class="btn btn-ghost" onclick={() => nexusReveal(g.gameId, null)}>{tr("Ouvrir le dossier", "Open folder")}</button>
          <button class="btn btn-ghost" onclick={() => (nexusEditing = nexusEditing === g.gameId ? null : g.gameId)}>
            {tr("Réglages du jeu", "Game settings")}
          </button>
        </div>
      </div>
      <p class="folder muted">
        {g.folder}, {g.rules ? (ruleLabels[g.rules] ?? tr("rangement automatique", "automatic layout")) : layoutLabels[g.layout].title.toLowerCase()}
        {#if g.extraFolders?.game}<br />{tr(`Mods de code : ${g.extraFolders.game}`, `Code mods: ${g.extraFolders.game}`)}{/if}
      </p>

      {#if nexusEditing === g.gameId && g.domain}
        <div class="editor">
          <NexusFolderSetup
            gameId={g.gameId}
            domain={g.domain}
            gameName={g.gameName}
            onsaved={() => (nexusEditing = null)}
            oncancel={() => (nexusEditing = null)}
          />
        </div>
      {/if}

      {#if sharing === `nexus:${g.gameId}`}
        <SharePanel source="nexus" game={g} onclose={() => (sharing = null)} />
      {/if}

      {#if Object.keys(g.mods).length > 0}
        <ProfileBar
          game={g}
          onapply={(name) => nexusStore.applyProfile(g.gameId, name)}
          onedit={(edit) => nexusStore.editProfiles(g.gameId, edit)}
        />
      {/if}

      {#if nxNotes.get(g.gameId)}<p class="note">{nxNotes.get(g.gameId)}</p>{/if}

      {#if nxOrphans.get(g.gameId)}
        {@const o = nxOrphans.get(g.gameId)!}
        <div class="orphans">
          <p class="small">
            {#if english}
              {o.mods.map((x) => `"${x.name}"`).join(", ")}
              {o.mods.length === 1 ? "was" : "were"} installed with "{o.parent}" and {o.mods.length === 1 ? "is" : "are"} no
              longer needed by any other mod.
            {:else}
              {o.mods.map((x) => `« ${x.name} »`).join(", ")}
              {o.mods.length > 1 ? "avaient été installés" : "avait été installé"} avec « {o.parent} » et ne
              {o.mods.length > 1 ? "servent" : "sert"} plus à aucun autre mod.
            {/if}
          </p>
          <button class="btn btn-primary" onclick={() => removeNexusOrphans(g)}>
            {o.mods.length > 1 ? tr("Les désinstaller aussi", "Uninstall them too") : tr("Le désinstaller aussi", "Uninstall it too")}
          </button>
          <button class="btn btn-ghost" onclick={() => nxOrphans.delete(g.gameId)}>{tr("Garder", "Keep")}</button>
        </div>
      {/if}

      {#if list.length === 0 && loaderRows.length === 0}
        <p class="muted small">
          {tr(`Aucun mod installé ne correspond à « ${query.trim()} ».`, `No installed mod matches "${query.trim()}".`)}
        </p>
      {/if}

      <ul class="rows">
        {#each loaderRows as l (l.key)}
          {@const k = `tool:${g.gameId}:${l.tool?.id}`}
          <li class="row">
            <button class="row-main" onclick={() => g.domain && nav.openNexusGame(nexusGameOf(g))}>
              <span class="row-thumb"><Thumb src={null} label={l.name} /></span>
              <span class="row-text">
                <span class="row-name">{l.name}</span>
                <span class="row-meta">
                  <span>{tr("chargeur de mods", "mod loader")}</span>
                  {#if l.tool}
                    <span>{l.tool.version ? `version ${l.tool.version}` : tr("version sans numéro", "no version number")}</span>
                    <span>{tr(`installé ${ago(l.tool.installedAt)}`, `installed ${ago(l.tool.installedAt)}`)}</span>
                  {:else}
                    <span>{tr("installé hors de ModStudio", "installed outside ModStudio")}</span>
                  {/if}
                </span>
              </span>
            </button>
            {#if l.tool}
              {@const tool = l.tool}
              <div class="row-action">
                {#if nexusStore.toolProgress[g.gameId]}
                  <div class="row-progress">
                    <ProgressBar
                      progress={nexusStore.toolProgress[g.gameId]}
                      oncancel={() => nexusStore.cancelTool(g.gameId)}
                    />
                  </div>
                {:else}
                  {#if l.update}
                    <span class="chip chip-accent">{tr("Nouvelle version", "New version")}</span>
                    <button class="btn btn-primary" onclick={() => g.domain && nav.openNexusGame(nexusGameOf(g))}>
                      {tr("Mettre à jour", "Update")}
                    </button>
                  {/if}
                  <button
                    class="btn"
                    class:btn-ghost={!nxConfirming.has(k)}
                    class:danger={nxConfirming.has(k)}
                    onclick={() => removeTool(g, tool.id, tool.name)}
                    disabled={nxRemoving.has(k)}
                  >
                    {nxRemoving.has(k) ? "…" : nxConfirming.has(k) ? tr("Confirmer", "Confirm") : tr("Retirer", "Remove")}
                  </button>
                {/if}
              </div>
            {/if}
          </li>
        {/each}
        {#each list as m (m.modId)}
          {@const k = nxKey(g, m)}
          {@const progress = nexusStore.progress[k]}
          {@const isOutdated = nexusStore.outdated(g.gameId, m.modId)}
          <li class="row" class:off={m.disabled}>
            <button class="row-main" onclick={() => nexusOpen(g, m)}>
              <span class="row-thumb"><Thumb src={m.logo} label={m.name} /></span>
              <span class="row-text">
                <span class="row-name">{m.name}</span>
                <span class="row-meta">
                  {#if m.fileName}<span>{m.fileName}</span>{/if}
                  <span>{m.version ? `version ${m.version}` : tr("version sans numéro", "no version number")}</span>
                  <span>{bytes(m.size)}</span>
                  <span>{tr(`installé ${ago(m.installedAt)}`, `installed ${ago(m.installedAt)}`)}</span>
                  {#if m.parts?.length}<span>{tr("mod de code", "code mod")}</span>{/if}
                  {#if m.disabled}<span class="chip">{tr("Désactivé", "Disabled")}</span>{/if}
                  {#if m.auto && m.requiredBy.length}
                    <span
                      >{tr(
                        `nécessaire à « ${m.requiredBy.map((id) => g.mods[id]?.name ?? `mod ${id}`).join(" », « ")} »`,
                        `required by "${m.requiredBy.map((id) => g.mods[id]?.name ?? `mod ${id}`).join('", "')}"`,
                      )}</span
                    >
                  {/if}
                </span>
                {#if nxErrors.get(k)}<span class="row-error">{nxErrors.get(k)}</span>{/if}
              </span>
            </button>
            <div class="row-action">
              {#if progress}
                <div class="row-progress"><ProgressBar {progress} oncancel={() => nexusStore.cancel(m.modId)} /></div>
              {:else}
                <ModToggle
                  enabled={!m.disabled}
                  name={m.name}
                  busy={toggling.has(`nexus:${g.gameId}:${m.modId}`)}
                  ontoggle={(on) => toggle("nexus", g, m, on)}
                />
                {#if isOutdated}
                  <span class="chip chip-accent">{tr("Nouvelle version", "New version")}</span>
                  <button class="btn btn-primary" onclick={() => nexusOpen(g, m)}>{tr("Choisir", "Choose")}</button>
                {/if}
                <button class="btn btn-ghost" onclick={() => nexusReveal(g.gameId, m.modId)} disabled={m.disabled}>{tr("Ouvrir", "Open")}</button>
                <button
                  class="btn"
                  class:btn-ghost={!nxConfirming.has(k)}
                  class:danger={nxConfirming.has(k)}
                  onclick={() => nexusUninstall(g, m)}
                  disabled={nxRemoving.has(k)}
                >
                  {nxRemoving.has(k) ? "…" : nxConfirming.has(k) ? tr("Confirmer", "Confirm") : tr("Désinstaller", "Uninstall")}
                </button>
              {/if}
            </div>
          </li>
        {/each}
      </ul>
    </section>
  {/each}

  {#each tsGames as g (g.gameId)}
    {@const list = mods(g)}
    {@const outdated = tsOutdatedIn(g)}
    {@const job = tsStore.progress[g.gameId]}
    <section class="group">
      <div class="group-head">
        {#if g.icon}<img class="game-icon" src={g.icon} alt="" width="28" height="28" />{/if}
        <h2>{g.gameName}</h2>
        <span class="chip">Thunderstore</span>
        <span class="muted count">{tsStore.count(g)}</span>
        <div class="group-actions">
          <PlayButton name={g.gameName} folders={[g.extraFolders?.game, g.folder]} />
          {#if outdated.length > 0}
            <button class="btn btn-primary" onclick={() => tsUpdateAll(g)} disabled={tsUpdatingAll.has(g.gameId) || !!job}>
              <Icon name="download" size={15} />{tsUpdatingAll.has(g.gameId)
                ? tr("Mise à jour…", "Updating…")
                : outdated.length > 1
                  ? tr(`Tout mettre à jour (${outdated.length})`, `Update all (${outdated.length})`)
                  : tr("Mettre à jour (1)", "Update (1)")}
            </button>
          {/if}
          <button class="btn btn-ghost" onclick={() => toggleShare(`ts:${g.gameId}`)}><Icon name="share" size={15} />{tr("Partager", "Share")}</button>
          <button class="btn btn-ghost" onclick={() => tsReveal(g.gameId, null)}>{tr("Ouvrir le dossier", "Open folder")}</button>
          <button class="btn btn-ghost" onclick={() => nav.openTsGame(tsStore.communityOf(g))}>{tr("Page du jeu", "Game page")}</button>
        </div>
      </div>
      <p class="folder muted">{tr(`${g.folder}, rangement BepInEx`, `${g.folder}, BepInEx layout`)}</p>

      {#if sharing === `ts:${g.gameId}`}
        <SharePanel source="ts" game={g} onclose={() => (sharing = null)} />
      {/if}

      {#if Object.keys(g.mods).length > 0}
        <ProfileBar
          game={g}
          onapply={(name) => tsStore.applyProfile(g.gameId, name)}
          onedit={(edit) => tsStore.editProfiles(g.gameId, edit)}
        />
      {/if}

      {#if job}
        <div class="row-progress wide"><ProgressBar progress={job} oncancel={() => tsStore.cancel(tsStore.communityOf(g))} /></div>
      {/if}

      {#if tsGameNotes.get(g.gameId)}<p class="note">{tsGameNotes.get(g.gameId)}</p>{/if}

      {#if tsOrphans.get(g.gameId)}
        {@const o = tsOrphans.get(g.gameId)!}
        <div class="orphans">
          <p class="small">
            {#if english}
              {o.mods.map((x) => `"${x.name}"`).join(", ")}
              {o.mods.length === 1 ? "was" : "were"} installed with "{o.parent}" and {o.mods.length === 1 ? "is" : "are"} no
              longer needed by any other mod.
            {:else}
              {o.mods.map((x) => `« ${x.name} »`).join(", ")}
              {o.mods.length > 1 ? "avaient été installés" : "avait été installé"} avec « {o.parent} » et ne
              {o.mods.length > 1 ? "servent" : "sert"} plus à aucun autre mod.
            {/if}
          </p>
          <button class="btn btn-primary" onclick={() => removeTsOrphans(g)}>
            {o.mods.length > 1 ? tr("Les désinstaller aussi", "Uninstall them too") : tr("Le désinstaller aussi", "Uninstall it too")}
          </button>
          <button class="btn btn-ghost" onclick={() => tsOrphans.delete(g.gameId)}>{tr("Garder", "Keep")}</button>
        </div>
      {/if}

      {#if list.length === 0}
        <p class="muted small">
          {tr(`Aucun mod installé ne correspond à « ${query.trim()} ».`, `No installed mod matches "${query.trim()}".`)}
        </p>
      {/if}

      <ul class="rows">
        {#each list as m (m.modId)}
          {@const k = tsKey(g, m)}
          {@const latest = tsStore.latest(g.gameId, m.modId)}
          <li class="row" class:off={m.disabled}>
            <button class="row-main" onclick={() => tsOpen(g, m)}>
              <span class="row-thumb square"><Thumb src={m.logo} label={m.name} /></span>
              <span class="row-text">
                <span class="row-name">{m.name}</span>
                <span class="row-meta">
                  {#if rmlIdOf(m.fileName)}<span>ResoniteModLoader</span>{:else if m.fileName}<span>{m.fileName.split("-")[0]}</span>{/if}
                  <span>{m.version ? `version ${m.version}` : tr("version sans numéro", "no version number")}</span>
                  <span>{bytes(m.size)}</span>
                  <span>{tr(`installé ${ago(m.installedAt)}`, `installed ${ago(m.installedAt)}`)}</span>
                  {#if m.disabled}<span class="chip">{tr("Désactivé", "Disabled")}</span>{/if}
                  {#if m.auto && m.requiredBy.length}
                    <span
                      >{tr(
                        `nécessaire à « ${m.requiredBy.map((id) => g.mods[id]?.name ?? `mod ${id}`).join(" », « ")} »`,
                        `required by "${m.requiredBy.map((id) => g.mods[id]?.name ?? `mod ${id}`).join('", "')}"`,
                      )}</span
                    >
                  {/if}
                </span>
                {#if tsErrors.get(k)}
                  <span class="row-error">{tsErrors.get(k)}</span>
                {:else if tsNotes.get(k)}
                  <span class="row-note">{tsNotes.get(k)}</span>
                {/if}
              </span>
            </button>
            <div class="row-action">
              <ModToggle
                enabled={!m.disabled}
                name={m.name}
                busy={toggling.has(`ts:${g.gameId}:${m.modId}`) || !!job}
                ontoggle={(on) => toggle("ts", g, m, on)}
              />
              {#if latest && !m.disabled}
                <span class="chip chip-accent">Version {latest}</span>
                <button class="btn btn-primary" onclick={() => tsUpdate(g, m)} disabled={!!job}>{tr("Mettre à jour", "Update")}</button>
              {/if}
              <button class="btn btn-ghost" onclick={() => tsReveal(g.gameId, m.modId)} disabled={m.disabled}>{tr("Ouvrir", "Open")}</button>
              <button
                class="btn"
                class:btn-ghost={!tsConfirming.has(k)}
                class:danger={tsConfirming.has(k)}
                onclick={() => tsUninstall(g, m)}
                disabled={tsRemoving.has(k) || !!job}
              >
                {tsRemoving.has(k) ? "…" : tsConfirming.has(k) ? tr("Confirmer", "Confirm") : tr("Désinstaller", "Uninstall")}
              </button>
            </div>
          </li>
        {/each}
      </ul>
    </section>
  {/each}

  {#each clGames as g (g.gameId)}
    {@const list = mods(g)}
    {@const outdated = clOutdatedIn(g)}
    {@const job = listsStore.progress[g.gameId]}
    {@const game = listsStore.gameOf(g)}
    <section class="group">
      <div class="group-head">
        {#if g.icon}<img class="game-icon wide" src={g.icon} alt="" height="28" />{/if}
        <h2>{g.gameName}</h2>
        <span class="chip">{game?.listName ?? tr("Communauté", "Community")}</span>
        <span class="muted count">{listsStore.count(g)}</span>
        <div class="group-actions">
          <PlayButton name={g.gameName} folders={[g.folder]} steamAppId={game?.steamAppId} />
          {#if outdated.length > 0}
            <button class="btn btn-primary" onclick={() => clUpdateAll(g)} disabled={clUpdatingAll.has(g.gameId) || !!job}>
              <Icon name="download" size={15} />{clUpdatingAll.has(g.gameId)
                ? tr("Mise à jour…", "Updating…")
                : outdated.length > 1
                  ? tr(`Tout mettre à jour (${outdated.length})`, `Update all (${outdated.length})`)
                  : tr("Mettre à jour (1)", "Update (1)")}
            </button>
          {/if}
          <button class="btn btn-ghost" onclick={() => toggleShare(`cl:${g.gameId}`)}><Icon name="share" size={15} />{tr("Partager", "Share")}</button>
          <button class="btn btn-ghost" onclick={() => cl.reveal(g.gameId).catch((e) => (error = asAppError(e)))}>{tr("Ouvrir le dossier", "Open folder")}</button>
          {#if game}<button class="btn btn-ghost" onclick={() => nav.openClGame(game)}>{tr("Page du jeu", "Game page")}</button>{/if}
        </div>
      </div>
      <p class="folder muted">{g.folder}{game ? tr(`, avec ${game.loader}`, `, with ${game.loader}`) : ""}</p>
      {#if game}
        {@const pairs = listsStore.incompatiblePairs(game)}
        {#if pairs.length}
          <p class="incompat">
            <Icon name="warning" size={15} />
            <span>
              {tr("Déclarés incompatibles entre eux :", "Declared incompatible with each other:")}
              {pairs.map(([a, b]) => `${a} + ${b}`).join(" · ")}. {tr("Désactive l'un des deux.", "Disable one of them.")}
            </span>
          </p>
        {/if}
      {/if}

      {#if sharing === `cl:${g.gameId}`}
        <SharePanel source="cl" game={g} onclose={() => (sharing = null)} />
      {/if}

      {#if Object.keys(g.mods).length > 0}
        <ProfileBar
          game={g}
          onapply={(name) => listsStore.applyProfile(g.gameId, name)}
          onedit={(edit) => listsStore.editProfiles(g.gameId, edit)}
        />
      {/if}

      {#if job && game}
        <div class="row-progress wide"><ProgressBar progress={job} oncancel={() => listsStore.cancel(game)} /></div>
      {/if}

      {#if clGameNotes.get(g.gameId)}<p class="note">{clGameNotes.get(g.gameId)}</p>{/if}

      {#if clOrphans.get(g.gameId)}
        {@const o = clOrphans.get(g.gameId)!}
        <div class="orphans">
          <p class="small">
            {#if english}
              {o.mods.map((x) => `"${x.name}"`).join(", ")}
              {o.mods.length === 1 ? "was" : "were"} installed with "{o.parent}" and {o.mods.length === 1 ? "is" : "are"} no
              longer needed by any other mod.
            {:else}
              {o.mods.map((x) => `« ${x.name} »`).join(", ")}
              {o.mods.length > 1 ? "avaient été installés" : "avait été installé"} avec « {o.parent} » et ne
              {o.mods.length > 1 ? "servent" : "sert"} plus à aucun autre mod.
            {/if}
          </p>
          <button class="btn btn-primary" onclick={() => removeClOrphans(g)}>
            {o.mods.length > 1 ? tr("Les désinstaller aussi", "Uninstall them too") : tr("Le désinstaller aussi", "Uninstall it too")}
          </button>
          <button class="btn btn-ghost" onclick={() => clOrphans.delete(g.gameId)}>{tr("Garder", "Keep")}</button>
        </div>
      {/if}

      {#if list.length === 0}
        <p class="muted small">
          {tr(`Aucun mod installé ne correspond à « ${query.trim()} ».`, `No installed mod matches "${query.trim()}".`)}
        </p>
      {/if}

      <ul class="rows">
        {#each list as m (m.modId)}
          {@const k = tsKey(g, m)}
          {@const latest = listsStore.latest(g.gameId, m.modId)}
          <li class="row" class:off={m.disabled}>
            <button class="row-main" onclick={() => clOpen(g, m)}>
              <span class="row-thumb square"><Thumb src={m.logo} label={m.name} /></span>
              <span class="row-text">
                <span class="row-name">{m.name}</span>
                <span class="row-meta">
                  <span>{m.version ? `version ${m.version}` : tr("version sans numéro", "no version number")}</span>
                  {#if m.size > 0}<span>{bytes(m.size)}</span>{/if}
                  <span>{tr(`installé ${ago(m.installedAt)}`, `installed ${ago(m.installedAt)}`)}</span>
                  {#if m.disabled}<span class="chip">{tr("Désactivé", "Disabled")}</span>{/if}
                  {#if m.auto && m.requiredBy.length}
                    <span
                      >{tr(
                        `nécessaire à « ${m.requiredBy.map((id) => g.mods[id]?.name ?? `mod ${id}`).join(" », « ")} »`,
                        `required by "${m.requiredBy.map((id) => g.mods[id]?.name ?? `mod ${id}`).join('", "')}"`,
                      )}</span
                    >
                  {/if}
                </span>
                {#if clErrors.get(k)}
                  <span class="row-error">{clErrors.get(k)}</span>
                {:else if clNotes.get(k)}
                  <span class="row-note">{clNotes.get(k)}</span>
                {/if}
              </span>
            </button>
            <div class="row-action">
              <ModToggle
                enabled={!m.disabled}
                name={m.name}
                busy={toggling.has(`cl:${g.gameId}:${m.modId}`) || !!job}
                ontoggle={(on) => toggle("cl", g, m, on)}
              />
              {#if latest && !m.disabled}
                <span class="chip chip-accent">Version {latest}</span>
                <button class="btn btn-primary" onclick={() => clUpdate(g, m)} disabled={!!job}>{tr("Mettre à jour", "Update")}</button>
              {/if}
              <button class="btn btn-ghost" onclick={() => cl.reveal(g.gameId, m.modId).catch((e) => (error = asAppError(e)))} disabled={m.disabled}>{tr("Ouvrir", "Open")}</button>
              <button
                class="btn"
                class:btn-ghost={!clConfirming.has(k)}
                class:danger={clConfirming.has(k)}
                onclick={() => clUninstall(g, m)}
                disabled={clRemoving.has(k) || !!job}
              >
                {clRemoving.has(k) ? "…" : clConfirming.has(k) ? tr("Confirmer", "Confirm") : tr("Désinstaller", "Uninstall")}
              </button>
            </div>
          </li>
        {/each}
      </ul>
    </section>
  {/each}
</div>

<style>
  .game-icon.wide {
    width: auto;
    border-radius: 4px;
  }

  .row.off .row-thumb,
  .row.off .row-name {
    opacity: 0.5;
  }

  .view {
    height: 100%;
    overflow-y: auto;
    padding: 2.25rem 2.5rem 3rem;
    display: grid;
    align-content: start;
    gap: 1.25rem;
  }

  .head {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-end;
    justify-content: space-between;
    gap: 1.25rem 2rem;
  }

  h1 {
    font-size: var(--t-3xl);
    font-weight: 800;
    margin-bottom: 0.3rem;
  }

  .tools {
    display: flex;
    gap: 0.6rem;
  }

  .search {
    position: relative;
    display: block;
    width: min(18rem, 34vw);
    color: var(--muted);
  }

  .search :global(svg) {
    position: absolute;
    left: 0.75rem;
    top: 50%;
    translate: 0 -50%;
    pointer-events: none;
  }

  .search .field {
    padding-left: 2.2rem;
  }

  .group {
    display: grid;
    gap: 0.6rem;
    margin-top: 1rem;
  }

  .group-head {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.65rem;
  }

  .game-icon {
    border-radius: var(--r-sm);
  }

  h2 {
    font-size: var(--t-xl);
    font-weight: 700;
  }

  .count {
    font-size: var(--t-md);
  }

  .group-actions {
    margin-left: auto;
    display: flex;
    flex-wrap: wrap;
    gap: 0.3rem;
  }

  .folder {
    font-size: var(--t-sm);
    margin-top: -0.4rem;
    overflow-wrap: anywhere;
  }

  .editor {
    max-width: 34rem;
    padding: 1.1rem;
    border-radius: var(--r-md);
    background: var(--panel);
  }

  .orphans {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.6rem 0.8rem;
    padding: 0.8rem 1rem;
    border-radius: var(--r-md);
    background: var(--raise);
  }

  .orphans p {
    flex: 1 1 22rem;
  }

  .note,
  .row-note {
    color: var(--ok);
    font-size: var(--t-sm);
  }

  .small {
    font-size: var(--t-sm);
  }

  .rows {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 0.4rem;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 1rem;
    padding: 0.5rem 0.75rem 0.5rem 0.5rem;
    border-radius: var(--r-md);
    background: var(--panel);
  }

  .row-main {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 1rem;
    padding: 0;
    border: 0;
    background: none;
    text-align: left;
  }

  .row-main:hover .row-name {
    color: var(--accent);
  }

  .row-thumb {
    flex: none;
    width: 7.5rem;
  }

  .row-thumb :global(.thumb) {
    aspect-ratio: 16 / 9;
    border-radius: var(--r-sm);
    overflow: hidden;
    background: var(--raise);
    position: relative;
  }

  .row-thumb :global(.thumb img) {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  /* Icônes carrées de Thunderstore : centrées dans la même largeur. */
  .row-thumb.square {
    display: flex;
    justify-content: center;
  }

  .row-thumb.square :global(.thumb) {
    aspect-ratio: 1;
    width: 4.25rem;
  }

  .row-thumb :global(.fallback) {
    font-size: var(--t-xl);
  }

  .row-text {
    display: grid;
    gap: 0.2rem;
    min-width: 0;
  }

  .row-name {
    font-family: var(--font-display);
    font-weight: 650;
    font-size: var(--t-lg);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .row-meta {
    display: flex;
    flex-wrap: wrap;
    gap: 0.1rem 0.9rem;
    font-size: var(--t-sm);
    color: var(--muted);
  }

  .incompat {
    display: flex;
    align-items: flex-start;
    gap: 0.45rem;
    margin: 0.25rem 0 0.75rem;
    padding: 0.55rem 0.8rem;
    border-radius: var(--r-md);
    background: var(--danger-bg);
    font-size: var(--t-sm);
  }

  .incompat :global(svg) {
    flex: none;
    color: var(--danger);
    margin-top: 0.15rem;
  }

  .swap {
    display: grid;
    gap: 0.4rem;
    margin-bottom: 1.5rem;
  }

  .swap-title {
    font-weight: 600;
  }

  .row-error {
    color: var(--danger);
    font-size: var(--t-sm);
  }

  .row-action {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    flex: none;
  }

  .row-progress.wide {
    max-width: 34rem;
    width: auto;
  }

  .row-progress {
    width: 16rem;
  }

  .danger {
    background: var(--danger-bg);
    border-color: var(--danger);
    color: var(--danger);
  }

  .empty {
    display: grid;
    justify-items: start;
    gap: 1rem;
    padding: 1rem 0;
    max-width: 40rem;
  }

  .empty-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
  }
</style>
