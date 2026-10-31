<script lang="ts">
  // Page Nexus Mods affichée dans ModStudio, par-dessus la zone de contenu :
  // la page elle-même est une vue web posée exactement sur le cadre ci-dessous.
  import { onMount } from "svelte";
  import { asAppError, nexus, type SiteBounds } from "$lib/api";
  import { english, tr } from "$lib/i18n.svelte";
  import { nexusStore, type SiteRequest } from "$lib/nexus.svelte";
  import Icon from "./Icon.svelte";

  let { request }: { request: SiteRequest } = $props();

  let frame = $state<HTMLDivElement | null>(null);
  let embedded = $state(false);

  function bounds(): SiteBounds | null {
    if (!frame) return null;
    const r = frame.getBoundingClientRect();
    return { x: Math.round(r.left), y: Math.round(r.top), width: Math.round(r.width), height: Math.round(r.height) };
  }

  onMount(() => {
    let raf = 0;
    let gone = false;
    // La page suit le cadre quand la fenêtre change de taille.
    const follow = () => {
      cancelAnimationFrame(raf);
      raf = requestAnimationFrame(() => {
        const b = bounds();
        if (b && embedded) nexus.siteBounds(b).catch(() => {});
      });
    };
    const observer = new ResizeObserver(follow);
    if (frame) observer.observe(frame);
    window.addEventListener("resize", follow);

    (async () => {
      try {
        const b = bounds();
        const placement =
          request.kind === "download"
            ? await nexus.openDownload(request.domain, request.modId, request.fileId, b)
            : await nexus.openLogin(b);
        if (gone) return;
        if (placement === "embedded") {
          embedded = true;
          follow();
        } else if (nexusStore.site === request) {
          // Repli sur une fenêtre séparée : pas besoin du cadre.
          nexusStore.site = null;
        }
      } catch (e) {
        nexusStore.siteFailed(request, asAppError(e));
      }
    })();

    return () => {
      gone = true;
      observer.disconnect();
      window.removeEventListener("resize", follow);
      cancelAnimationFrame(raf);
    };
  });
</script>

<section class="site" aria-label={tr("Page Nexus Mods", "Nexus Mods page")}>
  <header class="bar">
    <div class="text">
      {#if request.kind === "download"}
        <p class="title">
          {request.fileName
            ? tr(`Télécharger « ${request.fileName} »`, `Download "${request.fileName}"`)
            : tr("Télécharger le fichier", "Download the file")}
        </p>
        <p class="hint">
          {#if english}
            Click <strong>"Slow download"</strong>: ModStudio will install the file on its own. If Nexus Mods asks you to,
            sign in first.
          {:else}
            Clique sur <strong>« Slow download »</strong> : ModStudio installera le fichier tout seul. Si Nexus Mods te le
            demande, connecte-toi d'abord.
          {/if}
        </p>
      {:else}
        <p class="title">{tr("Connexion à Nexus Mods", "Sign in to Nexus Mods")}</p>
        <p class="hint">
          {#if english}
            Sign in, then copy your <strong>"Personal API Key"</strong> at the bottom of the page and come back to paste it
            into ModStudio.
          {:else}
            Connecte-toi, puis copie ta <strong>« Personal API Key »</strong> en bas de la page et reviens la coller dans
            ModStudio.
          {/if}
        </p>
      {/if}
    </div>
    <div class="actions">
      {#if request.kind === "login"}
        <button class="btn btn-primary" onclick={() => nexusStore.closeSite()}>
          <Icon name="check" size={15} />{tr("J'ai copié ma clé", "I've copied my key")}
        </button>
      {:else}
        <button class="btn" onclick={() => nexusStore.closeSite()}>
          <Icon name="close" size={15} />{tr("Annuler", "Cancel")}
        </button>
      {/if}
    </div>
  </header>
  <div class="frame" bind:this={frame}>
    <p class="loading muted">{tr("Chargement de la page Nexus Mods…", "Loading the Nexus Mods page…")}</p>
  </div>
</section>

<style>
  .site {
    position: absolute;
    inset: 0;
    z-index: 20;
    display: grid;
    grid-template-rows: auto minmax(0, 1fr);
    background: var(--ink);
  }

  .bar {
    display: flex;
    align-items: center;
    gap: 1rem 1.5rem;
    padding: 0.9rem 1.25rem;
    border-bottom: 1px solid var(--line);
    background: var(--panel);
  }

  .text {
    flex: 1;
    min-width: 0;
    display: grid;
    gap: 0.15rem;
  }

  .title {
    font-family: var(--font-display);
    font-weight: 700;
    font-size: var(--t-lg);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .hint {
    font-size: var(--t-sm);
    color: var(--muted);
  }

  .hint strong {
    color: var(--accent);
  }

  .actions {
    display: flex;
    gap: 0.5rem;
    flex: none;
  }

  .frame {
    position: relative;
    display: grid;
    place-items: center;
    background: var(--panel);
  }

  .loading {
    font-size: var(--t-sm);
  }
</style>
