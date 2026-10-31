<script lang="ts" generics="T extends { id: number }">
  import { inview, type PagedList } from "$lib/paged.svelte";
  import { exact } from "$lib/format";
  import { tr } from "$lib/i18n.svelte";
  import ErrorNotice from "./ErrorNotice.svelte";

  let { list, noun }: { list: PagedList<T>; noun: string } = $props();
</script>

{#if list.error}
  <div class="load-more">
    <ErrorNotice error={list.error} onretry={() => list.retry()} />
  </div>
{:else if list.loaded && list.items.length > 0}
  <!-- Recréé à chaque page reçue : si la liste ne remplit pas encore l'écran,
       la suite se charge aussitôt, sans attendre un défilement. -->
  {#key list.items.length}
    <div class="load-more" use:inview={() => list.more()}>
      <p class="muted">
        {tr(
          `${exact(list.items.length)} ${noun} affichés sur ${exact(list.total)}`,
          `Showing ${exact(list.items.length)} of ${exact(list.total)} ${noun}`,
        )}
      </p>
      {#if list.hasMore}
        <button class="btn" onclick={() => list.more()} disabled={list.loading}>
          {list.loading ? tr("Chargement…", "Loading…") : tr("Afficher plus", "Show more")}
        </button>
      {/if}
    </div>
  {/key}
{/if}
