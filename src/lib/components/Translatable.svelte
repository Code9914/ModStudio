<script lang="ts">
  // Description d'un mod avec un bouton « Traduire » quand elle est écrite dans
  // une autre langue que celle de l'interface (ou traduite d'office si le
  // réglage est activé). Le contenu s'affiche par `children`, original ou traduit.
  import { untrack, type Snippet } from "svelte";
  import type { AppError } from "$lib/api";
  import { bytes } from "$lib/format";
  import { tr } from "$lib/i18n.svelte";
  import { languageName, translator, type Progress, type Source } from "$lib/translate.svelte";
  import Icon from "./Icon.svelte";

  let { sources, children }: { sources: Source[]; children: Snippet<[string[]]> } = $props();

  // Seul le contenu compte (le tableau est recréé à chaque affichage).
  const key = $derived(sources.map((s) => `${s.html ? "h" : "t"}${s.text}`).join("\u0000"));

  let lang = $state<string | null>(null);
  let translated = $state<string[] | null>(null);
  let showing = $state(false);
  let busy = $state(false);
  let progress = $state<Progress | null>(null);
  let error = $state<AppError | null>(null);
  let generation = 0;

  $effect(() => {
    key;
    const gen = ++generation;
    untrack(() => {
      lang = null;
      translated = null;
      showing = false;
      busy = false;
      progress = null;
      error = null;
    });
    const current = untrack(() => sources);
    translator.detect(current).then((l) => {
      if (gen !== generation) return;
      lang = l;
      if (l && translator.auto) run();
    });
  });

  async function run() {
    if (translated) {
      showing = true;
      return;
    }
    if (!lang || busy) return;
    const gen = generation;
    const current = untrack(() => sources);
    busy = true;
    error = null;
    progress = null;
    try {
      const out = await translator.translate(current, lang, (p) => {
        if (gen === generation) progress = p;
      });
      if (gen !== generation) return;
      translated = out;
      showing = true;
    } catch (e) {
      if (gen === generation) error = e as AppError;
    } finally {
      if (gen === generation) {
        busy = false;
        progress = null;
      }
    }
  }

  const name = $derived(lang ? languageName(lang) : "");
  const ratio = $derived.by(() => {
    if (!progress) return null;
    const [n, total] = progress.phase === "download" ? [progress.received, progress.total] : [progress.done, progress.total];
    return total > 0 ? Math.min(1, n / total) : null;
  });
  const shown = $derived(showing && translated ? translated : sources.map((s) => s.text));
</script>

{#if lang}
  <div class="bar" aria-live="polite">
    {#if busy}
      <div class="busy">
        <span class="status">
          <Icon name="translate" size={15} />
          {#if progress?.phase === "download"}
            {tr(`Téléchargement du modèle de traduction (${name})`, `Downloading the translation model (${name})`)}
            <span class="muted">{bytes(progress.received)} / {bytes(progress.total)}</span>
          {:else}
            {tr("Traduction…", "Translating…")}
            {#if ratio !== null && ratio > 0}<span class="muted">{Math.round(ratio * 100)} %</span>{/if}
          {/if}
        </span>
        <span class="track"><span class="fill" class:indeterminate={ratio === null} style:width={ratio === null ? "35%" : `${ratio * 100}%`}></span></span>
      </div>
    {:else if showing}
      <span class="status muted">
        <Icon name="translate" size={15} />
        {tr(`Traduit automatiquement (${name} → français)`, `Machine translated (${name} → English)`)}
      </span>
      <button class="link" onclick={() => (showing = false)}>{tr("Voir l'original", "Show original")}</button>
    {:else}
      <button class="btn small" onclick={run}>
        <Icon name="translate" size={15} />
        {translated ? tr("Voir la traduction", "Show translation") : tr("Traduire en français", "Translate to English")}
      </button>
      {#if !translated}<span class="muted hint">{tr(`Écrit en ${name}`, `Written in ${name}`)}</span>{/if}
    {/if}
  </div>
  {#if error}
    <p class="error">
      {error.message}
      <button class="link" onclick={run}>{tr("Réessayer", "Try again")}</button>
    </p>
  {/if}
{/if}

{@render children(shown)}

<style>
  .bar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.4rem 0.9rem;
    font-size: var(--t-sm);
  }

  .btn.small {
    padding: 0.35rem 0.75rem;
    gap: 0.4rem;
  }

  .status {
    display: inline-flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.2rem 0.45rem;
  }

  .status .muted {
    white-space: nowrap;
  }

  .hint {
    font-size: var(--t-xs);
  }

  .busy {
    display: grid;
    gap: 0.4rem;
    width: min(36rem, 100%);
  }

  .track {
    position: relative;
    display: block;
    height: 4px;
    border-radius: 999px;
    background: var(--raise);
    overflow: hidden;
  }

  .fill {
    position: absolute;
    inset: 0 auto 0 0;
    border-radius: inherit;
    background: var(--accent-fill);
    transition: width 0.2s ease;
  }

  .fill.indeterminate {
    animation: slide 1.2s ease-in-out infinite;
  }

  @keyframes slide {
    from {
      left: -35%;
    }
    to {
      left: 100%;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .fill.indeterminate {
      animation: none;
    }
  }

  .link {
    padding: 0;
    border: 0;
    background: none;
    color: var(--accent);
    font-size: var(--t-sm);
    text-decoration: underline;
    text-underline-offset: 3px;
  }

  .error {
    color: var(--danger);
    font-size: var(--t-sm);
  }
</style>
