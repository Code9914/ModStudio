<script lang="ts">
  // Image 16:9 chargée paresseusement, avec repli sur l'initiale si absente ou cassée.
  let { src, label, badge }: { src: string | null | undefined; label: string; badge?: string } = $props();

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
  {#if badge}
    <span class="badge chip chip-warn">{badge}</span>
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

  .badge {
    position: absolute;
    top: 0.5rem;
    left: 0.5rem;
  }
</style>
