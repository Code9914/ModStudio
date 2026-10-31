<script lang="ts">
  // Installateur FOMOD : une étape après l'autre, on coche les options
  // voulues ; ModStudio installe ensuite les fichiers correspondants.
  import { untrack } from "svelte";
  import type { FomodGroup, FomodInstaller, FomodPlugin } from "$lib/api";
  import { tr } from "$lib/i18n.svelte";
  import { fomodEval, fomodType } from "$lib/nexus.svelte";
  import Icon from "./Icon.svelte";

  let {
    installer,
    modName,
    onfinish,
    oncancel,
  }: {
    installer: FomodInstaller;
    modName: string;
    /** Renvoie un message d'erreur, ou `null` si l'installation a démarré. */
    onfinish: (selections: number[][][]) => Promise<string | null>;
    oncancel: () => void;
  } = $props();

  // selections[étape][groupe] = options cochées (étapes pas encore vues : vide).
  // L'installateur ne change pas pendant la vie du composant.
  let selections = $state<number[][][]>(untrack(() => installer.steps.map((s) => s.groups.map(() => []))));
  let seen = $state<boolean[]>(untrack(() => installer.steps.map(() => false)));
  /** Étapes visitées, dans l'ordre (pour « Précédent »). */
  let history = $state<number[]>([]);
  let current = $state(-1);
  let focused = $state<FomodPlugin | null>(null);
  let problem = $state<string | null>(null);
  let installing = $state(false);

  /** Flags posés par les options cochées des étapes visibles avant `index`. */
  function flagsBefore(index: number): Record<string, string> {
    const flags: Record<string, string> = {};
    for (let si = 0; si < index; si++) {
      const step = installer.steps[si];
      if (step.visible && !fomodEval(step.visible, flags)) continue;
      const set: Record<string, string> = {};
      step.groups.forEach((g, gi) => {
        for (const pi of selections[si]?.[gi] ?? []) for (const f of g.plugins[pi]?.flags ?? []) set[f.name] = f.value;
      });
      Object.assign(flags, set);
    }
    return flags;
  }

  function visible(index: number): boolean {
    const step = installer.steps[index];
    return !step.visible || fomodEval(step.visible, flagsBefore(index));
  }

  /** Choix de départ d'un groupe : options obligatoires et conseillées. */
  function initial(group: FomodGroup, flags: Record<string, string>): number[] {
    const types = group.plugins.map((p) => fomodType(p, flags));
    const usable = (i: number) => types[i] !== "NotUsable";
    const required = types.flatMap((t, i) => (t === "Required" ? [i] : []));
    const recommended = types.flatMap((t, i) => (t === "Recommended" ? [i] : []));
    const firstUsable = types.findIndex((_, i) => usable(i));
    switch (group.type) {
      case "SelectAll":
        return group.plugins.map((_, i) => i);
      case "SelectExactlyOne":
        return [required[0] ?? recommended[0] ?? firstUsable].filter((i) => i >= 0);
      case "SelectAtMostOne":
        return [required[0] ?? recommended[0]].filter((i) => i !== undefined);
      case "SelectAtLeastOne": {
        const set = [...new Set([...required, ...recommended])];
        return set.length ? set : [firstUsable].filter((i) => i >= 0);
      }
      default:
        return [...new Set([...required, ...recommended])];
    }
  }

  function open(index: number) {
    if (!seen[index]) {
      const flags = flagsBefore(index);
      selections[index] = installer.steps[index].groups.map((g) => initial(g, flags));
      seen[index] = true;
    }
    current = index;
    focused = null;
    problem = null;
  }

  function nextVisible(from: number): number {
    for (let i = from + 1; i < installer.steps.length; i++) if (visible(i)) return i;
    return -1;
  }

  // Première étape visible.
  $effect.pre(() => {
    if (current === -1) {
      const first = nextVisible(-1);
      if (first >= 0) open(first);
    }
  });

  const step = $derived(current >= 0 ? installer.steps[current] : null);
  const flags = $derived(current >= 0 ? flagsBefore(current) : {});
  const isLast = $derived(current >= 0 && nextVisible(current) === -1);
  const stepNumber = $derived(history.length + 1);
  const preview = $derived(focused ?? (step ? firstChosen() : null));

  function firstChosen(): FomodPlugin | null {
    if (!step) return null;
    for (const [gi, g] of step.groups.entries()) {
      const pi = selections[current][gi]?.[0];
      if (pi !== undefined) return g.plugins[pi];
    }
    return step.groups[0]?.plugins[0] ?? null;
  }

  /** Boutons radio pour « une option exactement » ; cases à cocher sinon (une au plus : décochable). */
  function single(group: FomodGroup) {
    return group.type === "SelectExactlyOne";
  }

  function toggle(gi: number, pi: number) {
    const group = step!.groups[gi];
    const type = fomodType(group.plugins[pi], flags);
    if (type === "NotUsable" || type === "Required" || group.type === "SelectAll") return;
    const set = selections[current][gi];
    const on = set.includes(pi);
    if (group.type === "SelectExactlyOne") selections[current][gi] = [pi];
    else if (group.type === "SelectAtMostOne") selections[current][gi] = on ? [] : [pi];
    else selections[current][gi] = on ? set.filter((i) => i !== pi) : [...set, pi];
    problem = null;
  }

  /** Règles des groupes de l'étape affichée. */
  function check(): string | null {
    for (const [gi, g] of step!.groups.entries()) {
      const n = selections[current][gi].length;
      const label = g.name || step!.name;
      if (g.type === "SelectExactlyOne" && n !== 1) return tr(`Choisis une option dans « ${label} ».`, `Choose an option in "${label}".`);
      if (g.type === "SelectAtLeastOne" && n < 1) return tr(`Choisis au moins une option dans « ${label} ».`, `Choose at least one option in "${label}".`);
    }
    return null;
  }

  async function next() {
    problem = check();
    if (problem) return;
    if (!isLast) {
      history = [...history, current];
      open(nextVisible(current));
      return;
    }
    installing = true;
    // Étapes cachées par les choix : rien n'y est installé.
    const final = selections.map((groups, si) => (seen[si] && visible(si) ? groups : groups.map(() => [])));
    problem = await onfinish(final);
    installing = false;
  }

  function back() {
    const prev = history.at(-1);
    if (prev === undefined) return;
    history = history.slice(0, -1);
    current = prev;
    focused = null;
    problem = null;
  }

  const typeLabels: Record<string, string> = {
    Required: tr("Obligatoire", "Required"),
    Recommended: tr("Conseillé", "Recommended"),
    NotUsable: tr("Indisponible", "Unavailable"),
    CouldBeUsable: tr("Peut servir", "May be useful"),
  };

  function keydown(e: KeyboardEvent) {
    if (e.key === "Escape" && !installing) oncancel();
  }
</script>

<svelte:window onkeydown={keydown} />

<div class="backdrop" role="presentation">
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="fomod-title">
    <header class="head">
      <div>
        <p class="eyebrow">{tr(`Installation de « ${modName} »`, `Installing "${modName}"`)}</p>
        <h2 id="fomod-title">{step?.name || installer.moduleName || "Options"}</h2>
      </div>
      <p class="muted small">{tr(`Étape ${stepNumber}`, `Step ${stepNumber}`)}</p>
    </header>

    {#if step}
      <div class="body">
        <!-- Recréés à chaque étape : des boutons radio réutilisés d'une étape à l'autre se décocheraient. -->
        {#key current}
        <div class="groups">
          {#each step.groups as group, gi (gi)}
            <fieldset class="group">
              <legend>
                {group.name}
                <span class="muted small">
                  {#if group.type === "SelectExactlyOne"}{tr("une option", "one option")}{:else if group.type === "SelectAtMostOne"}{tr(
                      "une option au plus",
                      "at most one option",
                    )}{:else if group.type === "SelectAtLeastOne"}{tr("au moins une", "at least one")}{:else if group.type === "SelectAll"}{tr(
                      "tout est installé",
                      "everything is installed",
                    )}{:else}{tr("au choix", "any")}{/if}
                </span>
              </legend>
              {#each group.plugins as plugin, pi (pi)}
                {@const type = fomodType(plugin, flags)}
                {@const checked = selections[current][gi].includes(pi)}
                <label
                  class="option"
                  class:checked
                  class:unusable={type === "NotUsable"}
                  onmouseenter={() => (focused = plugin)}
                >
                  <input
                    type={single(group) ? "radio" : "checkbox"}
                    name="fomod-{current}-{gi}"
                    {checked}
                    disabled={type === "NotUsable" || type === "Required" || group.type === "SelectAll" || installing}
                    onchange={() => toggle(gi, pi)}
                    onfocus={() => (focused = plugin)}
                  />
                  <span class="option-name">{plugin.name}</span>
                  {#if typeLabels[type]}
                    <span class="chip" class:chip-ok={type === "Recommended"} class:chip-accent={type === "Required"}>{typeLabels[type]}</span>
                  {/if}
                </label>
              {/each}
            </fieldset>
          {/each}
        </div>
        {/key}

        <aside class="preview" aria-live="polite">
          {#if preview?.image ?? installer.image}
            <img src={preview?.image ?? installer.image} alt="" />
          {/if}
          {#if preview}
            <p class="strong">{preview.name}</p>
            {#if preview.description}<p class="desc small">{preview.description}</p>{/if}
          {/if}
        </aside>
      </div>
    {/if}

    <footer class="foot">
      {#if problem}
        <p class="problem small" role="alert"><Icon name="warning" size={15} /><span>{problem}</span></p>
      {/if}
      <div class="actions">
        <button class="btn btn-ghost" onclick={oncancel} disabled={installing}>{tr("Annuler", "Cancel")}</button>
        <span class="spacer"></span>
        <button class="btn" onclick={back} disabled={history.length === 0 || installing}>{tr("Précédent", "Back")}</button>
        <button class="btn btn-primary" onclick={next} disabled={installing}>
          {installing ? tr("Installation…", "Installing…") : isLast ? tr("Installer", "Install") : tr("Suivant", "Next")}
        </button>
      </div>
    </footer>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 60;
    display: grid;
    place-items: center;
    padding: 1.5rem;
    background: rgb(0 0 0 / 0.55);
  }

  .dialog {
    width: min(60rem, 100%);
    max-height: min(46rem, 100%);
    display: grid;
    grid-template-rows: auto 1fr auto;
    border-radius: var(--r-md);
    background: var(--panel);
    box-shadow: 0 24px 64px rgb(0 0 0 / 0.45);
    overflow: hidden;
  }

  .head {
    display: flex;
    align-items: flex-end;
    justify-content: space-between;
    gap: 1rem;
    padding: 1.1rem 1.4rem 0.9rem;
    border-bottom: 1px solid var(--raise);
  }

  .eyebrow {
    font-size: var(--t-xs);
    font-weight: 700;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--accent);
  }

  h2 {
    font-size: var(--t-xl);
    font-weight: 800;
  }

  .body {
    display: grid;
    grid-template-columns: minmax(0, 1.2fr) minmax(0, 1fr);
    min-height: 0;
  }

  .groups {
    overflow-y: auto;
    padding: 1rem 1.4rem;
    display: grid;
    align-content: start;
    gap: 1.1rem;
  }

  .group {
    border: 0;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 0.3rem;
  }

  legend {
    font-weight: 700;
    margin-bottom: 0.4rem;
    display: flex;
    gap: 0.5rem;
    align-items: baseline;
  }

  .option {
    display: flex;
    align-items: center;
    gap: 0.6rem;
    padding: 0.5rem 0.65rem;
    border-radius: var(--r-sm);
    cursor: pointer;
  }

  .option:hover,
  .option.checked {
    background: var(--raise);
  }

  .option input {
    accent-color: var(--accent-fill);
    flex: none;
  }

  .option.unusable {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .option-name {
    flex: 1;
    min-width: 0;
  }

  .preview {
    overflow-y: auto;
    padding: 1rem 1.4rem;
    border-left: 1px solid var(--raise);
    display: grid;
    align-content: start;
    gap: 0.6rem;
  }

  .preview img {
    width: 100%;
    max-height: 18rem;
    object-fit: contain;
    border-radius: var(--r-sm);
    background: var(--raise);
  }

  .desc {
    white-space: pre-line;
    color: var(--muted);
  }

  .foot {
    padding: 0.9rem 1.4rem 1.1rem;
    border-top: 1px solid var(--raise);
    display: grid;
    gap: 0.6rem;
  }

  .actions {
    display: flex;
    gap: 0.4rem;
  }

  .spacer {
    flex: 1;
  }

  .problem {
    display: flex;
    align-items: flex-start;
    gap: 0.45rem;
    color: var(--danger);
  }

  .small {
    font-size: var(--t-sm);
  }

  .strong {
    font-weight: 700;
  }

  @media (max-width: 760px) {
    .body {
      grid-template-columns: 1fr;
    }

    .preview {
      border-left: 0;
      border-top: 1px solid var(--raise);
    }
  }
</style>
