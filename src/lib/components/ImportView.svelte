<script lang="ts">
  // Importer un profil partagé : code ModStudio ou r2modman, ou fichier
  // (.modstudio, .r2z). Les mods s'installent l'un après l'autre.
  import { onMount } from "svelte";
  import { SvelteMap } from "svelte/reactivity";
  import { open } from "@tauri-apps/plugin-dialog";
  import {
    asAppError,
    share,
    type AppError,
    type FomodInstaller,
    type GameInstall,
    type InstalledMod,
    type NexusInstallOutcome,
    type ShareImport,
    type SharedMod,
  } from "$lib/api";
  import { installs } from "$lib/installs.svelte";
  import { listsStore } from "$lib/lists.svelte";
  import { nav } from "$lib/nav.svelte";
  import { nexusStore } from "$lib/nexus.svelte";
  import { profileFilters, sourceLabels, stableId } from "$lib/share.svelte";
  import { isLoaderName, splitFullName, tsStore } from "$lib/thunderstore.svelte";
  import ClSetup from "./ClSetup.svelte";
  import ErrorNotice from "./ErrorNotice.svelte";
  import FolderSetup from "./FolderSetup.svelte";
  import FomodWizard from "./FomodWizard.svelte";
  import Icon from "./Icon.svelte";
  import NexusFolderSetup from "./NexusFolderSetup.svelte";
  import ProgressBar from "./ProgressBar.svelte";
  import TsGameSetup from "./TsGameSetup.svelte";

  let text = $state("");
  let reading = $state(false);
  let readError = $state<AppError | null>(null);
  let imported = $state<ShareImport | null>(null);

  /** Jeu Thunderstore choisi (un profil r2modman ne dit pas pour quel jeu il est). */
  let tsKey = $state("");
  let sameVersions = $state(true);
  let makeProfile = $state(true);
  let applyConfigs = $state(false);

  type Row = { status: "running" | "done" | "kept" | "error" | "stopped"; message?: string };
  const rows = new SvelteMap<string, Row>();
  let running = $state(false);
  let stopping = $state(false);
  let current = $state<SharedMod | null>(null);
  let summary = $state<{ done: number; failed: number; stopped: number; notes: string[] } | null>(null);
  let runError = $state<AppError | null>(null);

  onMount(() => {
    installs.load().catch(() => {});
    nexusStore.load().catch(() => {});
    tsStore.load().catch(() => {});
    if (!listsStore.loaded) listsStore.load().catch(() => {});
    tsStore.loadCommunities();
  });

  // ------------------------------------------------------------ Lecture

  function take(result: ShareImport) {
    imported = result;
    tsKey = result.profile.source === "ts" ? result.profile.game.key : "";
    sameVersions = true;
    makeProfile = true;
    applyConfigs = false;
    rows.clear();
    summary = null;
    runError = null;
  }

  async function read() {
    if (!text.trim()) return;
    reading = true;
    readError = null;
    try {
      take(await share.decode(text));
    } catch (e) {
      readError = asAppError(e);
    } finally {
      reading = false;
    }
  }

  async function openFile() {
    readError = null;
    const path = await open({ title: "Ouvrir un profil", multiple: false, directory: false, filters: profileFilters.open });
    if (typeof path !== "string") return;
    reading = true;
    try {
      take(await share.openFile(path));
    } catch (e) {
      readError = asAppError(e);
    } finally {
      reading = false;
    }
  }

  function reset() {
    if (running) return;
    imported = null;
    text = "";
    rows.clear();
    summary = null;
  }

  // ------------------------------------------------------------ Le jeu

  const profile = $derived(imported?.profile ?? null);
  const source = $derived(profile?.source ?? null);
  const community = $derived(source === "ts" && tsKey ? tsStore.community(tsKey) : null);
  const clGame = $derived(source === "cl" && profile ? (listsStore.games.find((g) => g.id === profile.game.key) ?? null) : null);

  /** Identifiant du jeu dans la bibliothèque de sa source. */
  const gameId = $derived.by((): number | null => {
    if (!profile) return null;
    if (source === "modio" || source === "nexus") return profile.game.id;
    if (source === "ts") return community?.id ?? null;
    if (source === "cl") return clGame?.gameId ?? null;
    return null;
  });

  const config = $derived.by((): GameInstall | null => {
    if (gameId === null) return null;
    if (source === "modio") return installs.game(gameId);
    if (source === "nexus") return nexusStore.game(gameId);
    if (source === "ts") return tsStore.game(gameId);
    if (source === "cl") return listsStore.installs[gameId] ?? null;
    return null;
  });

  const gameName = $derived(community?.name ?? clGame?.name ?? (profile?.game.name || "ce jeu"));

  /** Ce qui manque avant de pouvoir installer. */
  const blocker = $derived.by((): string | null => {
    if (!profile) return null;
    if (source === "modio" && !nav.settings?.hasApiKey) return "modio-key";
    if (source === "nexus" && !nexusStore.ready) return "nexus-key";
    if (source === "ts" && !tsKey) return "ts-game";
    if (source === "ts" && tsStore.communities && !community) return "ts-unknown";
    if (source === "cl" && listsStore.loaded && !clGame) return "cl-unknown";
    if (gameId !== null && !config) return "folder";
    return null;
  });

  const configuredTs = $derived(
    (tsStore.communities ?? []).filter((c) => tsStore.game(c.id)).sort((a, b) => a.name.localeCompare(b.name, "fr")),
  );
  const otherTs = $derived(
    (tsStore.communities ?? []).filter((c) => !tsStore.game(c.id)).sort((a, b) => a.name.localeCompare(b.name, "fr")),
  );

  // ------------------------------------------------------------ Les mods

  function recordOf(m: SharedMod): InstalledMod | null {
    if (!config) return null;
    if (source === "modio" || source === "nexus") return config.mods[Number(m.id)] ?? null;
    const name = source === "cl" ? `${profile!.game.key}:${m.id}`.toLowerCase() : m.id.toLowerCase();
    return Object.values(config.mods).find((r) => r.fileName?.toLowerCase() === name) ?? null;
  }

  /** Même version que dans le profil ? (`null` : on ne sait pas comparer.) */
  function sameAs(m: SharedMod, r: InstalledMod): boolean | null {
    if (source === "modio" || source === "nexus") return m.file ? r.modfileId === m.file : null;
    return m.version ? r.version === m.version : null;
  }

  /** Les versions comptent (mod.io installe toujours la dernière). */
  const pinnable = $derived(source === "nexus" || source === "ts" || source === "cl");
  const withProfiles = $derived(source === "modio" || source === "nexus");

  type Action = "install" | "replace" | "enable" | "keep";

  function actionFor(m: SharedMod): Action {
    const r = recordOf(m);
    if (!r) return "install";
    if (pinnable && sameVersions && sameAs(m, r) === false) return "replace";
    // Le profil créé active ses mods en y passant.
    if (r.disabled) return withProfiles && makeProfile ? "keep" : "enable";
    return "keep";
  }

  const ordered = $derived.by(() => {
    if (!imported) return [];
    const list = [...imported.profile.mods];
    // Profil r2modman : le chargeur d'abord (les autres en ont besoin).
    if (imported.origin === "r2modman") {
      const loader = (m: SharedMod) => {
        const ref = splitFullName(m.id);
        return ref ? isLoaderName(ref.name, ref.namespace) : false;
      };
      list.sort((a, b) => Number(loader(b)) - Number(loader(a)));
    }
    return list;
  });

  const plan = $derived(ordered.map((m) => ({ mod: m, action: config ? actionFor(m) : ("install" as Action), record: recordOf(m) })));
  const toInstall = $derived(plan.filter((p) => p.action === "install" || p.action === "replace").length);
  const toEnable = $derived(plan.filter((p) => p.action === "enable").length);

  function stateLabel(p: (typeof plan)[number]): string {
    const r = p.record;
    switch (p.action) {
      case "install":
        return "À installer";
      case "replace":
        return `Version ${r?.version ?? "différente"} installée : remplacée par ${p.mod.version ?? "celle du profil"}`;
      case "enable":
        return "Installé mais désactivé : sera activé";
      case "keep":
        if (r?.disabled) return "Installé, activé avec le profil";
        if (r && sameAs(p.mod, r) === false) return `Installé (version ${r.version ?? "différente"})`;
        return "Déjà installé";
    }
  }

  // ------------------------------------------------------------ Options FOMOD (Nexus)

  let fomod = $state<{ token: string; installer: FomodInstaller; modId: number; name: string; resolve: (ok: boolean) => void } | null>(null);

  function settle(outcome: NexusInstallOutcome, modId: number, name: string): Promise<boolean> {
    if (outcome.status === "installed") return Promise.resolve(true);
    return new Promise((resolve) => {
      fomod = { token: outcome.token, installer: outcome.installer, modId, name, resolve };
    });
  }

  async function finishFomod(selections: number[][][]): Promise<string | null> {
    const f = fomod;
    if (!f || gameId === null) return null;
    try {
      const r = await nexusStore.finishFomod(gameId, f.modId, f.token, selections);
      if (r.status === "choices") {
        fomod = { ...f, token: r.token, installer: r.installer };
        return null;
      }
      fomod = null;
      f.resolve(true);
      return null;
    } catch (e) {
      const err = asAppError(e);
      if (err.kind === "invalid_argument") return err.message;
      fomod = null;
      f.resolve(false);
      rows.set(String(f.modId), { status: "error", message: err.message });
      return null;
    }
  }

  function cancelFomod() {
    const f = fomod;
    if (!f) return;
    nexusStore.cancelFomod(f.token);
    fomod = null;
    f.resolve(false);
  }

  // ------------------------------------------------------------ Installation

  const fail = (message: string): AppError => ({ kind: "share_invalid", message, retryAfter: null });

  /** Installe un mod ; renvoie une remarque à afficher, s'il y en a une. */
  async function installOne(m: SharedMod): Promise<string | null> {
    const id = gameId!;
    const version = sameVersions ? (m.version ?? null) : null;
    switch (source) {
      case "modio":
        await installs.install(id, Number(m.id));
        return null;
      case "nexus": {
        if (!m.file) throw fail("Fichier inconnu : installe ce mod depuis sa fiche.");
        const out = await nexusStore.request(id, profile!.game.key, Number(m.id), m.file, null);
        const ok = await settle(out, Number(m.id), m.name);
        if (!ok) throw fail(rows.get(m.id)?.message ?? "Options non choisies : mod pas installé.");
        return null;
      }
      case "ts": {
        if (!community) throw fail("Jeu inconnu.");
        if (m.id.startsWith("rml:")) {
          await tsStore.loadRml();
          const mod = tsStore.rmlMod(m.id.slice(4));
          if (!mod) throw fail("Ce mod n'est plus dans la liste de ResoniteModLoader.");
          const v = version && mod.versions.some((x) => x.version === version) ? version : null;
          await tsStore.installRml(community, mod, v);
          return version && !v ? `version ${version} plus proposée : la dernière est installée` : null;
        }
        const ref = splitFullName(m.id);
        if (!ref) throw fail("Mod illisible.");
        await tsStore.install(community, ref.namespace, ref.name, stableId(m.id), version);
        return null;
      }
      case "cl": {
        if (!clGame) throw fail("Jeu inconnu.");
        await listsStore.loadMods(clGame);
        const mod = listsStore.mod(clGame, m.id);
        if (!mod) throw fail("Ce mod n'est plus dans la liste.");
        const v = version && mod.versions.some((x) => x.version === version) ? version : null;
        await listsStore.install(clGame, mod, v);
        return version && !v ? `version ${version} plus proposée : la dernière est installée` : null;
      }
    }
    return null;
  }

  async function enableOne(m: SharedMod) {
    const r = recordOf(m);
    if (!r || gameId === null) return;
    if (source === "modio") await installs.setEnabled(gameId, r.modId, true);
    else if (source === "nexus") await nexusStore.setEnabled(gameId, r.modId, true);
    else if (source === "ts") await tsStore.setEnabled(gameId, r.modId, true);
    else if (source === "cl") await listsStore.setEnabled(gameId, r.modId, true);
  }

  function cancelCurrent() {
    const m = current;
    if (!m || gameId === null) return;
    if (source === "modio") installs.cancel(Number(m.id));
    else if (source === "nexus") {
      if (nexusStore.waiting[nexusStore.key(gameId, Number(m.id))]) nexusStore.stopWaiting(gameId, profile!.game.key, Number(m.id));
      else nexusStore.cancel(Number(m.id));
    } else if (source === "ts" && community) tsStore.cancel(community);
    else if (source === "cl" && clGame) listsStore.cancel(clGame);
  }

  function stop() {
    stopping = true;
    // Options FOMOD ouvertes : l'installation de ce mod s'arrête là.
    if (fomod) cancelFomod();
    cancelCurrent();
  }

  /** Nom du profil créé (mod.io, Nexus) : jamais « Principal », ni un profil à toi du même nom. */
  const profileName = $derived.by(() => {
    if (!profile) return "";
    const clip = (t: string, n: number) => [...t].slice(0, n).join("").trim();
    const taken = new Set((config?.profiles ?? []).map((p) => p.name.toLowerCase()));
    const base = clip(profile.name, 40);
    if (base.toLowerCase() !== "principal" && !taken.has(base.toLowerCase())) return base;
    const suffix = " (importé)";
    return clip(profile.name, 40 - suffix.length) + suffix;
  });

  /** Mods du profil créé : ceux de la liste et ceux dont ils ont besoin (même ajoutés depuis). */
  function profileMods(game: GameInstall, ids: number[]): number[] {
    const set = new Set(ids.filter((id) => game.mods[id]));
    let grew = true;
    while (grew) {
      grew = false;
      for (const r of Object.values(game.mods)) {
        if (!set.has(r.modId) && r.requiredBy.some((id) => set.has(id))) {
          set.add(r.modId);
          grew = true;
        }
      }
    }
    return [...set];
  }

  async function run() {
    if (!imported || gameId === null || running) return;
    running = true;
    stopping = false;
    summary = null;
    runError = null;
    rows.clear();
    const notes: string[] = [];
    let done = 0;
    let failed = 0;
    let stopped = 0;
    const before = new Set(Object.keys(config?.mods ?? {}).map(Number));
    const steps = plan.map((p) => ({ mod: p.mod, action: p.action }));
    for (const { mod, action } of steps) {
      if (stopping) {
        if (action !== "keep") {
          rows.set(mod.id, { status: "stopped" });
          stopped++;
        }
        continue;
      }
      if (action === "keep") {
        rows.set(mod.id, { status: "kept" });
        continue;
      }
      current = mod;
      rows.set(mod.id, { status: "running" });
      try {
        const note = action === "enable" ? (await enableOne(mod), null) : await installOne(mod);
        rows.set(mod.id, { status: "done", message: note ?? undefined });
        if (note) notes.push(`${mod.name} : ${note}.`);
        done++;
      } catch (e) {
        const err = asAppError(e);
        if (err.kind === "cancelled") {
          rows.set(mod.id, { status: "stopped" });
          stopped++;
          stopping = true;
        } else if (err.kind === "tool_present") {
          // Chargeur déjà installé à la main dans le jeu : il est gardé tel quel.
          rows.set(mod.id, { status: "kept", message: "déjà installé dans le jeu (sans ModStudio)" });
        } else {
          rows.set(mod.id, { status: "error", message: err.message });
          failed++;
        }
      }
    }
    current = null;
    try {
      if (!stopping && withProfiles && makeProfile) {
        const name = profileName;
        const store = source === "modio" ? installs : nexusStore;
        const game = store.game(gameId);
        if (game) {
          // Avec les mods ajoutés pendant l'import (mods nécessaires d'une version plus récente).
          const added = Object.keys(game.mods).map(Number).filter((id) => !before.has(id));
          const ids = profileMods(game, [...imported.profile.mods.map((m) => Number(m.id)), ...added]);
          await store.editProfiles(gameId, { action: "createWith", name, mods: ids });
          const kept = store.game(gameId)?.activeProfile ?? null;
          const issues = await store.applyProfile(gameId, name);
          for (const i of issues) notes.push(`${i.name} : ${i.message}`);
          notes.unshift(
            kept && kept !== name
              ? `Profil « ${name} » créé et activé : tes mods d'avant sont dans le profil « ${kept} ».`
              : `Profil « ${name} » mis à jour et activé.`,
          );
        }
      }
      if (!stopping && applyConfigs && imported.token && source === "ts") {
        const r = await share.applyConfigs(imported.token, gameId);
        // Appliqués une fois : pas à chaque nouvel essai.
        applyConfigs = false;
        imported.token = null;
        notes.push(
          `${r.written} ${r.written > 1 ? "fichiers de réglages appliqués" : "fichier de réglages appliqué"}${r.backup ? ` (les anciens sont gardés dans ${r.backup})` : ""}.`,
        );
      }
    } catch (e) {
      runError = asAppError(e);
    }
    summary = { done, failed, stopped, notes };
    running = false;
    stopping = false;
  }

  // ------------------------------------------------------------ Progression

  const currentProgress = $derived.by(() => {
    if (!current || gameId === null) return null;
    if (source === "modio") return installs.progress[Number(current.id)] ?? null;
    if (source === "nexus") return nexusStore.progress[nexusStore.key(gameId, Number(current.id))] ?? null;
    if (source === "ts") return tsStore.progress[gameId] ?? null;
    if (source === "cl") return listsStore.progress[gameId] ?? null;
    return null;
  });
  const waitingNexus = $derived(
    source === "nexus" && current && gameId !== null ? (nexusStore.waiting[nexusStore.key(gameId, Number(current.id))] ?? null) : null,
  );
  const position = $derived(current ? plan.filter((p) => p.action !== "keep").findIndex((p) => p.mod.id === current!.id) + 1 : 0);
  const steps = $derived(plan.filter((p) => p.action !== "keep").length);
</script>

<div class="view">
  <header class="head">
    <button class="btn btn-ghost crumb" onclick={() => nav.openInstalled()}><Icon name="back" />Installés</button>
    <h1>Importer un profil</h1>
    <p class="intro muted">
      Colle le code qu'un ami t'a envoyé (code ModStudio « MS1-… », ou code r2modman de Thunderstore), ou ouvre un fichier
      de profil (.modstudio, .r2z). ModStudio te montre les mods avant d'installer ceux qui te manquent.
    </p>
  </header>

  {#if !imported}
    <section class="panel">
      <label class="label" for="import-code">Code du profil</label>
      <textarea
        id="import-code"
        class="field code"
        rows="4"
        placeholder="MS1-… ou 0192f0a1-…"
        bind:value={text}
        onkeydown={(e) => {
          if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) read();
        }}
      ></textarea>
      <div class="row">
        <button class="btn btn-primary" onclick={read} disabled={reading || !text.trim()}>{reading ? "Lecture…" : "Lire le code"}</button>
        <button class="btn" onclick={openFile} disabled={reading}>Ouvrir un fichier…</button>
      </div>
      {#if readError}<ErrorNotice error={readError} />{/if}
    </section>
  {:else if profile}
    <section class="panel">
      <div class="summary">
        <div>
          <p class="label">Profil</p>
          <h2>{profile.name}</h2>
          <div class="chips">
            <span class="chip">{source === "cl" ? (clGame?.listName ?? sourceLabels.cl) : sourceLabels[profile.source]}</span>
            {#if community || clGame || profile.game.name}<span class="chip">{gameName}</span>{/if}
            {#if imported.origin === "r2modman"}<span class="chip">r2modman</span>{/if}
            <span class="muted small">{profile.mods.length} {profile.mods.length > 1 ? "mods" : "mod"}</span>
          </div>
        </div>
        <button class="btn btn-ghost" onclick={reset} disabled={running}>Autre profil</button>
      </div>

      {#if source === "ts" && (imported.origin === "r2modman" || !profile.game.key || (tsStore.communities && !tsStore.community(profile.game.key)))}
        <label class="game-pick">
          <span class="small">{imported.origin === "r2modman" ? "Jeu du profil (r2modman ne le précise pas)" : "Jeu"}</span>
          <select class="field" bind:value={tsKey} disabled={running}>
            <option value="" disabled>Choisis le jeu</option>
            {#if configuredTs.length}
              <optgroup label="Jeux déjà configurés">
                {#each configuredTs as c (c.identifier)}<option value={c.identifier}>{c.name}</option>{/each}
              </optgroup>
            {/if}
            <optgroup label="Tous les jeux de Thunderstore">
              {#each otherTs as c (c.identifier)}<option value={c.identifier}>{c.name}</option>{/each}
            </optgroup>
          </select>
        </label>
      {/if}

      {#if blocker === "modio-key"}
        <p class="small warn"><Icon name="warning" size={15} />Ces mods viennent de mod.io : ajoute d'abord ta clé API mod.io.</p>
        <div><button class="btn btn-primary" onclick={() => nav.openSettings()}>Ouvrir les réglages</button></div>
      {:else if blocker === "nexus-key"}
        <p class="small warn"><Icon name="warning" size={15} />Ces mods viennent de Nexus Mods : ajoute d'abord ta clé API Nexus Mods.</p>
        <div><button class="btn btn-primary" onclick={() => nav.openNexusAccount({ name: "import" })}>Ajouter ma clé Nexus</button></div>
      {:else if blocker === "ts-unknown"}
        <p class="small warn"><Icon name="warning" size={15} />Ce jeu n'est pas sur Thunderstore : choisis-en un autre.</p>
      {:else if blocker === "cl-unknown"}
        <p class="small warn"><Icon name="warning" size={15} />Ce profil est pour un jeu que cette version de ModStudio ne connaît pas : mets ModStudio à jour.</p>
      {:else if blocker === "folder" && gameId !== null}
        <div class="setup">
          <p class="small">Indique d'abord où {gameName} est installé : les mods y seront rangés.</p>
          {#if source === "modio"}
            <FolderSetup gameId={gameId} gameName={profile.game.name} onsaved={() => {}} oncancel={() => {}} />
          {:else if source === "nexus"}
            <NexusFolderSetup gameId={gameId} domain={profile.game.key} gameName={profile.game.name} onsaved={() => {}} oncancel={() => {}} />
          {:else if source === "ts" && community}
            <TsGameSetup game={community} />
          {:else if source === "cl" && clGame}
            <ClSetup game={clGame} />
          {/if}
        </div>
      {/if}

      {#if community && !community.supported}
        <p class="small warn"><Icon name="warning" size={15} />ModStudio ne sait pas encore installer les mods de ce jeu.</p>
      {/if}

      {#if !blocker}
        <div class="options">
          {#if pinnable}
            <label class="check small">
              <input type="checkbox" bind:checked={sameVersions} disabled={running} />
              Installer les mêmes versions que dans le profil (conseillé pour jouer ensemble)
            </label>
          {/if}
          {#if withProfiles}
            <label class="check small">
              <input type="checkbox" bind:checked={makeProfile} disabled={running} />
              <span>
                Créer le profil « {profileName} » avec ces mods : tes autres mods y sont désactivés, et tu retrouves tes
                mods actuels dans le profil « {config?.activeProfile ??
                  (config?.profiles?.some((p) => p.name.toLowerCase() === "principal") ? "Avant l'import" : "Principal")} ».
              </span>
            </label>
          {/if}
          {#if imported.configs > 0 && imported.token}
            <label class="check small">
              <input type="checkbox" bind:checked={applyConfigs} disabled={running} />
              <span>
                Remplacer les réglages des mods par ceux du profil ({imported.configs}
                {imported.configs > 1 ? "fichiers" : "fichier"}). Ceux que tu avais sont gardés de côté.
              </span>
            </label>
          {/if}
        </div>
        {#if source === "nexus" && !nexusStore.premium && toInstall > 0}
          <p class="small muted">
            Sans compte Premium, la page de chaque fichier va s'ouvrir : clique sur « Slow download » à chaque fois.
          </p>
        {/if}
        {#if imported.skipped.length}
          <p class="small muted">
            {imported.skipped.length > 1 ? `${imported.skipped.length} mods désactivés dans le profil ne seront pas installés` : "Un mod désactivé dans le profil ne sera pas installé"} :
            {imported.skipped.join(", ")}.
          </p>
        {/if}

        {#if running}
          <div class="progress">
            <p class="small strong">
              {stopping ? "Arrêt après ce mod…" : current ? `Installation ${position} sur ${steps} : ${current.name}` : "Création du profil…"}
            </p>
            {#if waitingNexus && current}
              <p class="small muted">En attente du clic sur « Slow download » dans la page de Nexus Mods.</p>
              <div>
                <button class="btn" onclick={() => nexusStore.reopen(gameId!, profile.game.key, Number(current!.id))}>Rouvrir la page</button>
              </div>
            {:else if currentProgress}
              <ProgressBar progress={currentProgress} />
            {/if}
            <div><button class="btn btn-ghost" onclick={stop} disabled={stopping}>Arrêter</button></div>
          </div>
        {:else if summary}
          <div class="done" role="status">
            <p class="strong">
              {#if summary.done === 0 && summary.failed === 0 && summary.stopped === 0}
                Tout était déjà installé.
              {:else}
                {summary.done} {summary.done > 1 ? "mods installés ou activés" : "mod installé ou activé"}{summary.failed
                  ? `, ${summary.failed} en échec`
                  : ""}{summary.stopped ? `, ${summary.stopped} pas encore installé${summary.stopped > 1 ? "s" : ""} (arrêté)` : ""}.
              {/if}
            </p>
            {#each summary.notes as n (n)}<p class="small">{n}</p>{/each}
            <div class="row">
              <button class="btn btn-primary" onclick={() => nav.openInstalled()}>Voir les mods installés</button>
              {#if summary.failed || summary.stopped}
                <button class="btn" onclick={run}>{summary.failed ? "Réessayer ceux qui ont échoué" : "Reprendre"}</button>
              {/if}
            </div>
          </div>
        {:else}
          {@const nothing = toInstall === 0 && toEnable === 0 && !(withProfiles && makeProfile) && !applyConfigs}
          <div class="row">
            {#if nothing}
              <p class="small ok"><Icon name="check" size={15} />Tu as déjà tous les mods de ce profil, dans les mêmes versions.</p>
            {:else}
              <button class="btn btn-primary big" onclick={run} disabled={!!community && !community.supported}>
                <Icon name="download" size={16} />
                {toInstall > 0 ? `Installer ${toInstall} ${toInstall > 1 ? "mods" : "mod"}` : "Appliquer le profil"}
              </button>
            {/if}
          </div>
        {/if}
        {#if runError}<ErrorNotice error={runError} />{/if}
      {/if}

      {#if fomod}
        <FomodWizard installer={fomod.installer} modName={fomod.name} onfinish={finishFomod} oncancel={cancelFomod} />
      {/if}
    </section>

    <section class="list" aria-label="Mods du profil">
      <ul>
        {#each plan as p (p.mod.id)}
          {@const row = rows.get(p.mod.id)}
          <li class="mod" class:dim={!row && p.action === "keep"}>
            <span class="name">
              <span class="strong">{p.mod.name}</span>
              {#if p.mod.version}<span class="muted small">{p.mod.version}</span>{/if}
              {#if p.mod.dep}<span class="muted small">mod nécessaire</span>{/if}
            </span>
            <span class="state small">
              {#if row?.status === "running"}
                <span class="accent">Installation…</span>
              {:else if row?.status === "done"}
                <span class="ok"><Icon name="check" size={14} />{p.action === "enable" ? "Activé" : "Installé"}</span>
                {#if row.message}<span class="muted">({row.message})</span>{/if}
              {:else if row?.status === "error"}
                <span class="error">{row.message}</span>
              {:else if row?.status === "stopped"}
                <span class="muted">Arrêté</span>
              {:else if row?.status === "kept"}
                <span class="muted">{row.message ?? (config ? stateLabel(p) : "Déjà installé")}</span>
              {:else}
                <span class="muted">{config ? stateLabel(p) : "À installer"}</span>
              {/if}
            </span>
          </li>
        {/each}
      </ul>
    </section>
  {/if}
</div>

<style>
  .view {
    height: 100%;
    overflow-y: auto;
    padding: 1.5rem 2.5rem 3rem;
  }

  .head {
    display: grid;
    justify-items: start;
    gap: 0.5rem;
    margin-bottom: 1.25rem;
  }

  .crumb {
    margin-left: -0.5rem;
  }

  h1 {
    font-size: var(--t-3xl);
    font-weight: 800;
  }

  h2 {
    font-size: var(--t-xl);
    font-weight: 700;
    margin: 0.1rem 0 0.4rem;
  }

  .intro {
    max-width: 46rem;
    font-size: var(--t-sm);
  }

  .panel {
    display: grid;
    gap: 0.8rem;
    max-width: 48rem;
    padding: 1.1rem 1.2rem;
    margin-bottom: 1.25rem;
    border-radius: var(--r-md);
    background: var(--panel);
  }

  .panel :global(.setup),
  .setup :global(.setup) {
    margin: 0;
  }

  .label {
    font-size: var(--t-xs);
    font-weight: 700;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--muted);
  }

  .code {
    width: 100%;
    font-family: var(--font-mono, ui-monospace, monospace);
    font-size: var(--t-sm);
    word-break: break-all;
    resize: vertical;
  }

  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.4rem;
  }

  .summary {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-start;
    justify-content: space-between;
    gap: 0.75rem;
  }

  .chips {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.4rem;
  }

  .game-pick {
    display: grid;
    gap: 0.3rem;
    max-width: 24rem;
  }

  .setup {
    display: grid;
    gap: 0.6rem;
  }

  .options {
    display: grid;
    gap: 0.5rem;
  }

  .check {
    display: flex;
    align-items: flex-start;
    gap: 0.5rem;
  }

  .check input {
    margin-top: 0.2rem;
  }

  .warn {
    display: flex;
    align-items: flex-start;
    gap: 0.45rem;
  }

  .warn :global(svg) {
    flex: none;
    margin-top: 0.15rem;
    color: var(--warn, var(--accent));
  }

  .progress,
  .done {
    display: grid;
    gap: 0.5rem;
    padding: 0.8rem 0.9rem;
    border-radius: var(--r-md);
    background: var(--raise);
    border-left: 3px solid var(--accent-fill);
  }

  .done {
    border-left-color: var(--ok);
  }

  .big {
    padding-block: 0.65rem;
    font-size: var(--t-md);
  }

  .list {
    max-width: 48rem;
  }

  .list ul {
    display: grid;
    gap: 0.25rem;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .mod {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    justify-content: space-between;
    gap: 0.3rem 1rem;
    padding: 0.55rem 0.8rem;
    border-radius: var(--r-sm);
    background: var(--panel);
  }

  .mod.dim {
    opacity: 0.75;
  }

  .name {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 0.5rem;
    min-width: 0;
    overflow-wrap: anywhere;
  }

  .state {
    display: flex;
    align-items: center;
    gap: 0.35rem;
    text-align: right;
  }

  .small {
    font-size: var(--t-sm);
  }

  .strong {
    font-weight: 600;
  }

  .ok {
    display: inline-flex;
    align-items: center;
    gap: 0.25rem;
    color: var(--ok);
  }

  .accent {
    color: var(--accent);
  }

  .error {
    color: var(--danger);
  }

  @media (max-width: 1150px) {
    .view {
      padding-inline: 1.75rem;
    }
  }
</style>
