<script lang="ts">
  // Petites icônes au trait, héritent de la couleur du texte.
  type Name =
    | "download"
    | "thumb"
    | "users"
    | "back"
    | "search"
    | "external"
    | "gear"
    | "grid"
    | "warning"
    | "close"
    | "check"
    | "plus"
    | "user"
    | "library"
    | "refresh"
    | "bell"
    | "share"
    | "import"
    | "translate"
    | "play";

  let { name, size = 16 }: { name: Name; size?: number } = $props();

  const paths: Record<Name, string> = {
    download: "M12 4v11m0 0-4.5-4.5M12 15l4.5-4.5M5 19h14",
    thumb: "M7 10v10H4V10h3Zm0 0 4-7a2 2 0 0 1 3 2l-1 4h5.5a2 2 0 0 1 2 2.3l-1.2 7A2 2 0 0 1 17.3 20H7",
    users: "M16 19v-1a4 4 0 0 0-4-4H7a4 4 0 0 0-4 4v1M9.5 10a3 3 0 1 0 0-6 3 3 0 0 0 0 6ZM21 19v-1a4 4 0 0 0-3-3.9M16 4.1a3 3 0 0 1 0 5.8",
    back: "M15 18l-6-6 6-6",
    search: "M11 18a7 7 0 1 0 0-14 7 7 0 0 0 0 14Zm9 2-4-4",
    external: "M14 4h6v6M20 4l-9 9M18 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h5",
    gear: "M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6Zm7.4-3a7.4 7.4 0 0 0-.1-1.2l2-1.6-2-3.4-2.4 1a7.6 7.6 0 0 0-2-1.2L14.5 3h-4l-.4 2.6a7.6 7.6 0 0 0-2 1.2l-2.4-1-2 3.4 2 1.6a7.4 7.4 0 0 0 0 2.4l-2 1.6 2 3.4 2.4-1a7.6 7.6 0 0 0 2 1.2l.4 2.6h4l.4-2.6a7.6 7.6 0 0 0 2-1.2l2.4 1 2-3.4-2-1.6c.1-.4.1-.8.1-1.2Z",
    grid: "M4 4h7v7H4zM13 4h7v7h-7zM4 13h7v7H4zM13 13h7v7h-7z",
    warning: "M12 9v4m0 4h.01M10.3 3.9 2.4 18a2 2 0 0 0 1.7 3h15.8a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0Z",
    close: "M6 6l12 12M18 6 6 18",
    check: "M5 12.5 10 17.5 19 7",
    plus: "M12 5v14M5 12h14",
    user: "M19 20v-1a5 5 0 0 0-5-5h-4a5 5 0 0 0-5 5v1M12 11a3.5 3.5 0 1 0 0-7 3.5 3.5 0 0 0 0 7Z",
    library: "M5 4h3v16H5zM10 4h3v16h-3zM15.5 4.8l2.9-.8 3.6 15.3-2.9.7z",
    refresh: "M20 11a8 8 0 0 0-14.7-4.3L4 8m0-4v4h4M4 13a8 8 0 0 0 14.7 4.3L20 16m0 4v-4h-4",
    bell: "M6 16V11a6 6 0 1 1 12 0v5l1.5 2h-15L6 16Zm4 4a2 2 0 0 0 4 0",
    share: "M12 15V4m0 0L8 8m4-4 4 4M6 12H5a1 1 0 0 0-1 1v6a1 1 0 0 0 1 1h14a1 1 0 0 0 1-1v-6a1 1 0 0 0-1-1h-1",
    play: "M7.5 4.8v14.4a.8.8 0 0 0 1.2.7l11.3-7.2a.8.8 0 0 0 0-1.4L8.7 4.1a.8.8 0 0 0-1.2.7Z",
    translate: "M4 5h9M8.5 3v2m3 0c-.9 4-3.6 7.3-7 9M6.5 9c1.1 2.2 3 3.9 5.3 4.8M13 21l4-9 4 9m-6.6-3h5.2",
    import: "M12 4v11m0 0-4-4m4 4 4-4M6 12H5a1 1 0 0 0-1 1v6a1 1 0 0 0 1 1h14a1 1 0 0 0 1-1v-6a1 1 0 0 0-1-1h-1",
  };
</script>

<svg
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width="1.8"
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"
>
  <path d={paths[name]} />
</svg>
