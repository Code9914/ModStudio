<script lang="ts">
  // Image 16:9 chargée paresseusement, avec repli sur l'initiale si absente ou cassée.
  export type Badge = { text: string; tone: "warn" | "ok" | "accent" };

  let { src, label, badges = [] }: { src: string | null | undefined; label: string; badges?: Badge[] } =
    $props();

  let failed = $state(false);
  $effect(() => {
    src;
    failed = false;
  });

  const initial = $derived((label.trim()[0] ?? "?").toUpperCase());
</script>

<div class="thumb">
  {#if src && !failed}
    <img {src} alt="" loading="lazy" decoding="async" onerror={() => (failed = true)} />
  {:else}
    <span class="fallback" aria-hidden="true">{initial}</span>
  {/if}
  {#if badges.length}
    <span class="badges">
      {#each badges as b (b.text)}
        <span
          class="chip"
          class:chip-warn={b.tone === "warn"}
          class:chip-ok={b.tone === "ok"}
          class:chip-accent={b.tone === "accent"}>{b.text}</span
        >
      {/each}
    </span>
  {/if}
</div>

<style>
  .fallback {
    display: grid;
    place-items: center;
    width: 100%;
    height: 100%;
    font-family: var(--font-display);
    font-size: var(--t-4xl);
    font-weight: 800;
    color: var(--muted);
  }

  .badges {
    position: absolute;
    top: 0.5rem;
    left: 0.5rem;
    display: flex;
    gap: 0.3rem;
  }
</style>
