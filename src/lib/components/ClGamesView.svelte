<script lang="ts">
  // Jeux dont la communauté tient sa propre liste de mods.
  import { onMount } from "svelte";
  import { listsStore } from "$lib/lists.svelte";
  import { nav } from "$lib/nav.svelte";
  import { launcherNames, nexusStore } from "$lib/nexus.svelte";
  import { RML_COMMUNITY, tsStore } from "$lib/thunderstore.svelte";
  import Thumb from "./Thumb.svelte";

  onMount(() => {
    if (!listsStore.loaded) listsStore.load().catch(() => {});
    if (nexusStore.detected === null) nexusStore.detectGames();
    tsStore.loadCommunities();
  });

  const resonite = $derived(tsStore.community(RML_COMMUNITY));

  function openResonite() {
    if (!resonite) return;
    nav.openTsGame(resonite);
    nav.tsTab = "rml";
  }
</script>

<div class="view">
  <header class="head">
    <p class="source">Communautés</p>
    <h1>Jeux</h1>
    <p class="intro muted">
      Pour ces jeux, la plupart des mods ne sont ni sur mod.io, ni sur Nexus Mods, ni sur Thunderstore : leur communauté tient
      sa propre liste, avec son chargeur de mods. ModStudio installe le chargeur s'il manque, puis chaque mod avec ceux dont il
      a besoin, rangé là où le jeu le lit. Les fichiers sont vérifiés quand la liste publie leur empreinte.
    </p>
  </header>

  <ul class="card-grid posters">
    {#each listsStore.games as g (g.id)}
      {@const config = listsStore.config(g)}
      {@const n = config ? listsStore.count(config) : 0}
      {@const detected = listsStore.detectedFor(g)}
      <li>
        <button class="card" onclick={() => nav.openClGame(g)}>
          <Thumb
            src={g.cover}
            label={g.name}
            badges={[
              ...(n > 0 ? [{ text: `${n} ${n > 1 ? "mods" : "mod"}`, tone: "ok" as const }] : []),
              ...(detected && n === 0 ? [{ text: launcherNames[detected.launcher] ?? detected.launcher, tone: "neutral" as const }] : []),
            ]}
          />
          <span class="card-title">{g.name}</span>
          <span class="card-meta"><span>{g.listName}</span></span>
          <span class="card-meta"><span>{g.loader}</span></span>
        </button>
      </li>
    {/each}
    {#if resonite}
      <li>
        <button class="card" onclick={openResonite}>
          <Thumb src={resonite.coverImageUrl} label={resonite.name} badges={[{ text: "Thunderstore", tone: "neutral" as const }]} />
          <span class="card-title">{resonite.name}</span>
          <span class="card-meta"><span>ResoniteModLoader</span></span>
          <span class="card-meta"><span>dans la page Thunderstore</span></span>
        </button>
      </li>
    {/if}
  </ul>
</div>

<style>
  .view {
    height: 100%;
    overflow-y: auto;
    padding: 2.25rem 2.5rem 3rem;
  }

  .head {
    margin-bottom: 1.75rem;
  }

  .source {
    font-size: var(--t-xs);
    font-weight: 700;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--accent);
    margin-bottom: 0.2rem;
  }

  h1 {
    font-size: var(--t-3xl);
    font-weight: 800;
    margin-bottom: 0.6rem;
  }

  .intro {
    max-width: 48rem;
    font-size: var(--t-sm);
  }

  .posters {
    grid-template-columns: repeat(auto-fill, minmax(calc(150px * var(--thumb-scale)), 1fr));
    gap: 1.5rem 1.1rem;
  }

  /* Affiches de Steam : 600 × 900. */
  .posters :global(.thumb) {
    aspect-ratio: 2 / 3;
  }

  .posters :global(.card-title) {
    font-size: var(--t-md);
  }
</style>
