<script lang="ts">
  import "@fontsource-variable/bricolage-grotesque";
  import "@fontsource-variable/figtree";
  import "../app.css";
  // Couleurs et tailles choisies dans les réglages.
  import "$lib/appearance.svelte";

  let { children } = $props();
</script>

{@render children()}
