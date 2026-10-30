<script lang="ts">
  // Réglages d'apparence : couleurs, thème, tailles. Appliqués tout de suite.
  import { appearance, defaults, palettes, type Corners, type Size, type ThemeMode } from "$lib/appearance.svelte";

  const value = $derived(appearance.value);
  const custom = $derived(value.palette === "custom");
  const isDefault = $derived(JSON.stringify(value) === JSON.stringify(defaults));

  const themes: { value: ThemeMode; label: string }[] = [
    { value: "system", label: "Comme Windows" },
    { value: "dark", label: "Sombre" },
    { value: "light", label: "Clair" },
  ];
  const textSizes: { value: Size; label: string }[] = [
    { value: "small", label: "Petite" },
    { value: "normal", label: "Normale" },
    { value: "large", label: "Grande" },
  ];
  const thumbSizes: { value: Size; label: string }[] = [
    { value: "small", label: "Petites" },
    { value: "normal", label: "Moyennes" },
    { value: "large", label: "Grandes" },
  ];
  const cornerStyles: { value: Corners; label: string }[] = [
    { value: "square", label: "Droits" },
    { value: "soft", label: "Légers" },
    { value: "round", label: "Arrondis" },
  ];
</script>

{#snippet choice<T extends string>(
  id: string,
  label: string,
  options: { value: T; label: string }[],
  current: T,
  pick: (v: T) => void,
)}
  <div class="pref">
    <span class="label" id="ap-{id}">{label}</span>
    <div class="seg" role="radiogroup" aria-labelledby="ap-{id}">
      {#each options as o (o.value)}
        <label class:on={current === o.value}>
          <input
            class="visually-hidden"
            type="radio"
            name="ap-{id}"
            value={o.value}
            checked={current === o.value}
            onchange={() => pick(o.value)}
          />
          {o.label}
        </label>
      {/each}
    </div>
  </div>
{/snippet}

<section class="block">
  <h2>Apparence</h2>

  <div class="colors">
    <span class="label" id="ap-colors">Couleurs</span>
    <div class="swatches" role="radiogroup" aria-labelledby="ap-colors">
      {#each palettes as p (p.id)}
        <label class="swatch" class:on={value.palette === p.id}>
          <input
            class="visually-hidden"
            type="radio"
            name="ap-palette"
            checked={value.palette === p.id}
            onchange={() => appearance.choosePalette(p)}
          />
          <span class="dot" style:background="linear-gradient(135deg, {p.a}, {p.b})"></span>
          <span class="name">{p.name}</span>
        </label>
      {/each}
      <label class="swatch" class:on={custom}>
        <input
          class="visually-hidden"
          type="radio"
          name="ap-palette"
          checked={custom}
          onchange={() => appearance.setCustom(value.colorA, value.colorB)}
        />
        <span
          class="dot"
          class:any={!custom}
          style:background={custom ? `linear-gradient(135deg, ${value.colorA}, ${value.colorB})` : null}
        ></span>
        <span class="name">À ton goût</span>
      </label>
    </div>

    {#if custom}
      <div class="pickers">
        <label class="picker">
          <input
            type="color"
            value={value.colorA}
            oninput={(e) => appearance.setCustom(e.currentTarget.value, value.colorB)}
          />
          Première couleur
        </label>
        <label class="picker">
          <input
            type="color"
            value={value.colorB}
            oninput={(e) => appearance.setCustom(value.colorA, e.currentTarget.value)}
          />
          Deuxième couleur
        </label>
        <span class="help muted">Mets deux fois la même couleur pour une couleur unie.</span>
      </div>
    {/if}
  </div>

  <label class="toggle">
    <input type="checkbox" checked={value.tint} onchange={(e) => appearance.set({ tint: e.currentTarget.checked })} />
    <span>
      Teinter le fond avec la première couleur
      <span class="help muted">Sinon, les fonds restent gris.</span>
    </span>
  </label>

  <div class="prefs">
    {@render choice("theme", "Thème", themes, value.theme, (v) => appearance.set({ theme: v }))}
    {@render choice("text", "Taille du texte", textSizes, value.text, (v) => appearance.set({ text: v }))}
    {@render choice("thumbs", "Vignettes", thumbSizes, value.thumbs, (v) => appearance.set({ thumbs: v }))}
    {@render choice("corners", "Coins", cornerStyles, value.corners, (v) => appearance.set({ corners: v }))}
  </div>

  <label class="toggle">
    <input
      type="checkbox"
      checked={value.railGames}
      onchange={(e) => appearance.set({ railGames: e.currentTarget.checked })}
    />
    <span>
      Raccourcis des jeux dans la barre latérale
      <span class="help muted">Les jeux qui ont au moins un mod installé apparaissent sous « Jeux ».</span>
    </span>
  </label>

  {#if !isDefault}
    <div>
      <button class="btn btn-ghost reset" type="button" onclick={() => appearance.reset()}>
        Rétablir l'apparence par défaut
      </button>
    </div>
  {/if}
</section>

<style>
  .block {
    display: grid;
    gap: 1.4rem;
  }

  h2 {
    font-size: var(--t-xl);
    font-weight: 700;
    margin-bottom: -0.5rem;
  }

  .label {
    font-weight: 600;
    font-size: var(--t-sm);
  }

  .help {
    font-size: var(--t-sm);
  }

  /* ------------------------------------------------------------ Couleurs */

  .colors {
    display: grid;
    gap: 0.75rem;
  }

  .swatches {
    display: flex;
    flex-wrap: wrap;
    gap: 0.9rem 1.1rem;
  }

  .swatch {
    display: grid;
    justify-items: center;
    gap: 0.45rem;
    width: 4.6rem;
    cursor: pointer;
  }

  .dot {
    width: 2.6rem;
    height: 2.6rem;
    border-radius: 50%;
    outline: 2px solid transparent;
    outline-offset: 3px;
    box-shadow: inset 0 0 0 1px rgb(255 255 255 / 0.12);
    transition: outline-color 120ms ease;
  }

  .dot.any {
    background: conic-gradient(#ff9ccb, #f7cf6e, #6fdcb8, #72d2ff, #8db4ff, #c39bff, #ff9ccb);
  }

  .swatch:hover .dot {
    outline-color: var(--line);
  }

  .swatch.on .dot {
    outline-color: var(--text);
  }

  .swatch:has(input:focus-visible) .dot {
    outline-color: var(--accent);
  }

  .name {
    font-size: var(--t-xs);
    color: var(--muted);
    text-align: center;
    line-height: 1.25;
  }

  .swatch.on .name {
    color: var(--text);
    font-weight: 600;
  }

  .pickers {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.6rem 1.25rem;
  }

  .picker {
    display: inline-flex;
    align-items: center;
    gap: 0.5rem;
    font-size: var(--t-sm);
    cursor: pointer;
  }

  .picker input {
    width: 2.25rem;
    height: 2.25rem;
    padding: 0;
    border: 1px solid var(--line);
    border-radius: var(--r-sm);
    background: none;
    cursor: pointer;
  }

  .picker input::-webkit-color-swatch-wrapper {
    padding: 3px;
  }

  .picker input::-webkit-color-swatch {
    border: 0;
    border-radius: calc(var(--r-sm) - 1px);
  }

  .pickers .help {
    flex-basis: 100%;
  }

  /* ------------------------------------------------- Choix à plusieurs valeurs */

  .prefs {
    display: grid;
    gap: 0.8rem;
  }

  .pref {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.4rem 1rem;
  }

  .pref .label {
    width: 8.5rem;
  }

  .seg {
    display: inline-flex;
    flex-wrap: wrap;
    gap: 2px;
    padding: 3px;
    border-radius: calc(var(--r-sm) + 3px);
    background: var(--raise);
  }

  .seg label {
    padding: 0.3rem 0.8rem;
    border-radius: var(--r-sm);
    font-size: var(--t-sm);
    font-weight: 500;
    color: var(--muted);
    cursor: pointer;
  }

  .seg label:hover {
    color: var(--text);
  }

  .seg label.on {
    background: var(--accent-fill);
    color: var(--on-accent);
    font-weight: 600;
  }

  .seg label:has(input:focus-visible) {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }

  /* --------------------------------------------------------- Cases à cocher */

  .toggle {
    display: flex;
    align-items: flex-start;
    gap: 0.75rem;
    cursor: pointer;
  }

  .toggle input {
    width: 1.1rem;
    height: 1.1rem;
    margin-top: 0.2rem;
    accent-color: var(--accent-fill);
  }

  .toggle .help {
    display: block;
  }

  .reset {
    padding-inline: 0;
  }
</style>
