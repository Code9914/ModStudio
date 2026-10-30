<script lang="ts">
  // Mods suivis sur Nexus Mods, tous jeux confondus.
  import { untrack } from "svelte";
  import { SvelteSet } from "svelte/reactivity";
  import { asAppError, nexus, type AppError, type NexusMod } from "$lib/api";
  import { count } from "$lib/format";
  import { nav } from "$lib/nav.svelte";
  import { nexusStore } from "$lib/nexus.svelte";
  import ErrorNotice from "./ErrorNotice.svelte";
  import Icon from "./Icon.svelte";
  import Thumb from "./Thumb.svelte";

  let mods = $state<NexusMod[] | null>(null);
  let error = $state<AppError | null>(null);
  const removing = new SvelteSet<string>();

  async function load() {
    error = null;
    try {
      mods = await nexus.trackedMods();
      nexusStore.loadSocial(true);
    } catch (e) {
      error = asAppError(e);
    }
  }

  // Relu à chaque visite : un mod a pu être suivi depuis sa fiche.
  $effect(() => {
    if (nav.view.name === "nexusTracked" && nexusStore.ready) untrack(load);
  });

  const key = (m: NexusMod) => `${m.game?.domainName}:${m.modId}`;
  // Un mod qui n'est plus suivi disparaît tout de suite de la liste.
  const shown = $derived(
    (mods ?? []).filter((m) => m.game && (!nexusStore.socialLoaded || nexusStore.isTracked(m.game.domainName, m.modId))),
  );

  function openMod(m: NexusMod) {
    if (!m.game) return;
    const g = m.game;
    nav.openNexusModFrom(
      { id: g.id, name: g.name ?? g.domainName, domainName: g.domainName, modCount: 0, downloadCount: 0, genre: null },
      m.modId,
      "nexusTracked",
    );
  }

  async function untrackMod(m: NexusMod) {
    if (!m.game) return;
    const k = key(m);
    removing.add(k);
    try {
      await nexusStore.setTracked(m.game.domainName, m.modId, false);
    } catch (e) {
      error = asAppError(e);
    } finally {
      removing.delete(k);
    }
  }
</script>

<div class="view">
  <header class="head">
    <h1>Mods suivis</h1>
    {#if mods}
      <p class="muted">
        {shown.length === 0 ? "Aucun mod suivi" : `${shown.length} ${shown.length > 1 ? "mods suivis" : "mod suivi"}`}, sur ton compte Nexus Mods
      </p>
    {/if}
  </header>

  {#if !nexusStore.ready}
    <div class="empty">
      <p>Ajoute ta clé API Nexus Mods pour retrouver ici les mods que tu suis.</p>
      <button class="btn btn-primary" onclick={() => nav.openNexusAccount({ name: "nexusTracked" })}>Ajouter ma clé</button>
    </div>
  {:else if error}
    <ErrorNotice {error} onretry={load} />
  {:else if !mods}
    <ul class="card-grid" aria-busy="true" aria-label="Chargement des mods suivis">
      {#each Array(6) as _, i (i)}
        <li class="skeleton"><div class="thumb"></div><div class="line"></div></li>
      {/each}
    </ul>
  {:else if shown.length === 0}
    <div class="empty">
      <p>
        Tu ne suis aucun mod pour l'instant. Sur la fiche d'un mod, clique sur « Suivre » : il apparaîtra ici, et Nexus
        Mods te préviendra de ses mises à jour.
      </p>
      <button class="btn" onclick={() => nav.openNexusGames()}>Parcourir les jeux</button>
    </div>
  {:else}
    <ul class="card-grid">
      {#each shown as m (key(m))}
        <li class="item">
          <button class="card" onclick={() => openMod(m)}>
            <Thumb
              src={m.thumbnailUrl ?? m.pictureUrl}
              label={m.name}
              badges={m.adultContent ? [{ text: "Adulte", tone: "warn" }] : []}
            />
            <span class="card-title">{m.name}</span>
            <span class="card-meta">
              <span class="chip">{m.game?.name ?? m.game?.domainName}</span>
              {#if m.author}<span>par {m.author}</span>{/if}
            </span>
            <span class="card-meta">
              <span title="Recommandations"><Icon name="thumb" size={14} />{count(m.endorsements)}</span>
              <span title="Téléchargements"><Icon name="download" size={14} />{count(m.downloads)}</span>
            </span>
          </button>
          <button class="btn btn-ghost untrack" onclick={() => untrackMod(m)} disabled={removing.has(key(m))}>
            {removing.has(key(m)) ? "…" : "Ne plus suivre"}
          </button>
        </li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  .view {
    height: 100%;
    overflow-y: auto;
    padding: 2.5rem;
    display: grid;
    align-content: start;
    gap: 1.75rem;
  }

  .head {
    display: grid;
    gap: 0.35rem;
  }

  h1 {
    font-size: var(--t-3xl);
    font-weight: 800;
  }

  .card-grid {
    grid-template-columns: repeat(auto-fill, minmax(calc(210px * var(--thumb-scale)), 1fr));
  }

  .item {
    display: grid;
    align-content: start;
    gap: 0.2rem;
  }

  .untrack {
    justify-self: start;
    padding-inline: 0;
    font-size: var(--t-sm);
  }

  .empty {
    display: grid;
    justify-items: start;
    gap: 1rem;
    max-width: 38rem;
  }

  .skeleton .thumb {
    aspect-ratio: 16 / 9;
    border-radius: var(--r-md);
    background: var(--raise);
  }

  .skeleton .line {
    height: 1rem;
    width: 70%;
    margin-top: 0.6rem;
    border-radius: var(--r-sm);
    background: var(--raise);
  }
</style>
