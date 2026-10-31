<script lang="ts">
  // Interrupteur « activé / désactivé » d'un mod installé.
  import { tr } from "$lib/i18n.svelte";

  let {
    enabled,
    name,
    busy = false,
    ontoggle,
  }: { enabled: boolean; name: string; busy?: boolean; ontoggle: (enabled: boolean) => void } = $props();
</script>

<button
  class="switch"
  class:on={enabled}
  type="button"
  role="switch"
  aria-checked={enabled}
  aria-label={enabled ? tr(`Désactiver « ${name} »`, `Disable "${name}"`) : tr(`Activer « ${name} »`, `Enable "${name}"`)}
  title={enabled
    ? tr("Activé : clique pour désactiver sans désinstaller", "Enabled: click to disable without uninstalling")
    : tr("Désactivé : clique pour réactiver", "Disabled: click to enable again")}
  disabled={busy}
  onclick={() => ontoggle(!enabled)}
>
  <span class="knob"></span>
</button>

<style>
  .switch {
    position: relative;
    flex: none;
    width: 2.4rem;
    height: 1.35rem;
    padding: 0;
    border: 1px solid var(--line, var(--raise));
    border-radius: 999px;
    background: var(--raise);
    cursor: pointer;
    transition: background 0.15s ease;
  }

  .switch.on {
    background: var(--accent-fill);
    border-color: var(--accent-fill);
  }

  .switch:disabled {
    opacity: 0.5;
    cursor: progress;
  }

  .switch:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }

  .knob {
    position: absolute;
    top: 50%;
    left: 0.15rem;
    width: 1rem;
    height: 1rem;
    border-radius: 50%;
    background: var(--muted);
    translate: 0 -50%;
    transition:
      left 0.15s ease,
      background 0.15s ease;
  }

  .on .knob {
    left: calc(100% - 1.15rem);
    background: var(--ink, #111);
  }

  @media (prefers-reduced-motion: reduce) {
    .switch,
    .knob {
      transition: none;
    }
  }
</style>
