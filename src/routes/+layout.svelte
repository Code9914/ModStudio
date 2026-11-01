<script lang="ts">
  import { onMount } from "svelte";
  import "@fontsource-variable/bricolage-grotesque";
  import "@fontsource-variable/figtree";
  import "../app.css";
  // Couleurs et tailles choisies dans les réglages.
  import "$lib/appearance.svelte";

  let { children } = $props();

  // Les styles de l'appli sont là : le fond provisoire posé par app.html laisse
  // la place à --ink (qui suit les changements de thème).
  onMount(() => document.documentElement.style.removeProperty("background"));
</script>

{@render children()}
