<script lang="ts">
  import type { Progress } from "$lib/installs.svelte";

  let { progress, oncancel }: { progress: Progress; oncancel?: () => void } = $props();

  const percent = $derived(progress.ratio === null ? null : Math.round(progress.ratio * 100));
</script>

<div class="progress">
  <div
    class="track"
    role="progressbar"
    aria-label={progress.label}
    aria-valuemin="0"
    aria-valuemax="100"
    aria-valuenow={percent ?? undefined}
  >
    <span class="fill" class:indeterminate={percent === null} style:width={percent === null ? "35%" : `${percent}%`}></span>
  </div>
  <div class="row">
    <span class="label">{progress.label}{percent !== null ? ` (${percent} %)` : ""}</span>
    {#if oncancel}<button class="btn btn-ghost cancel" onclick={oncancel}>Annuler</button>{/if}
  </div>
</div>

<style>
  .progress {
    display: grid;
    gap: 0.35rem;
  }

  .track {
    position: relative;
    height: 8px;
    border-radius: 999px;
    background: var(--raise);
    overflow: hidden;
  }

  .fill {
    position: absolute;
    inset: 0 auto 0 0;
    border-radius: 999px;
    background: var(--accent-fill);
    transition: width 150ms linear;
  }

  .fill.indeterminate {
    animation: slide 1.1s ease-in-out infinite;
  }

  @keyframes slide {
    from {
      left: -35%;
    }
    to {
      left: 100%;
    }
  }

  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
  }

  .label {
    font-size: var(--t-sm);
    color: var(--muted);
  }

  .cancel {
    font-size: var(--t-sm);
    padding-block: 0.2rem;
  }

  @media (prefers-reduced-motion: reduce) {
    .fill.indeterminate {
      animation: none;
      left: 0;
      width: 100% !important;
      opacity: 0.4;
    }
  }
</style>
