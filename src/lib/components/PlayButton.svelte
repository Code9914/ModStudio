<script lang="ts">
  // Lance le jeu depuis ModStudio, quand il est trouvé sur ce PC.
  import { onMount } from "svelte";
  import { asAppError, launchGame } from "$lib/api";
  import { tr } from "$lib/i18n.svelte";
  import { launcherNames, nexusStore } from "$lib/nexus.svelte";
  import { detectedGame, type PlayTarget } from "$lib/play.svelte";
  import Icon from "./Icon.svelte";

  let { name = null, folders = [], steamAppId = null, primary = false }: PlayTarget & { primary?: boolean } = $props();

  onMount(() => {
    if (nexusStore.detected === null) nexusStore.detectGames();
  });

  const game = $derived(detectedGame({ name, folders, steamAppId }));
  let stage = $state<"idle" | "starting" | "started">("idle");
  let note = $state<string | null>(null);
  let error = $state<string | null>(null);
  let timer: ReturnType<typeof setTimeout> | undefined;

  async function play() {
    if (!game || stage === "starting") return;
    clearTimeout(timer);
    stage = "starting";
    error = null;
    note = null;
    try {
      const launched = await launchGame(game.launcher, game.appId, game.path);
      note = launched.extender
        ? tr(`Lancé avec ${launched.extender}`, `Started with ${launched.extender}`)
        : tr(`Lancé par ${launcherNames[launched.via] ?? launched.via}`, `Started through ${launcherNames[launched.via] ?? launched.via}`);
      stage = "started";
      timer = setTimeout(() => {
        stage = "idle";
        note = null;
      }, 6000);
    } catch (e) {
      error = asAppError(e).message;
      stage = "idle";
    }
  }

  const label = $derived(game ? tr(`Lancer ${game.name} (${launcherNames[game.launcher] ?? game.launcher})`, `Start ${game.name} (${launcherNames[game.launcher] ?? game.launcher})`) : "");
</script>

{#if game}
  <span class="play">
    <button class="btn" class:btn-primary={primary} onclick={play} disabled={stage === "starting"} title={label}>
      <Icon name="play" size={15} />{stage === "starting" ? tr("Lancement…", "Starting…") : tr("Jouer", "Play")}
    </button>
    {#if note}<span class="note muted" role="status">{note}</span>{/if}
    {#if error}<span class="error" role="alert">{error}</span>{/if}
  </span>
{/if}

<style>
  .play {
    display: inline-flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.3rem 0.6rem;
  }

  .note,
  .error {
    font-size: var(--t-xs);
  }

  .error {
    color: var(--danger);
    max-width: 22rem;
  }
</style>
