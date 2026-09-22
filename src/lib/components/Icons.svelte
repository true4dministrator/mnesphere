<script lang="ts">
  let { name, size = 18, stroke = 1.6 }: { name: string; size?: number; stroke?: number } = $props();
</script>

<svg
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width={stroke}
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"
>
  {#if name === 'diary'}
    <path d="M5 3.5h10.5a2.5 2.5 0 0 1 2.5 2.5v14.5H7.5A2.5 2.5 0 0 1 5 18V3.5Z" />
    <path d="M9 3.5v17" />
    <path d="M12.5 8.5h3M12.5 12h3" />
  {:else if name === 'notes'}
    <path d="M6 3.5h8l4.5 4.5v12.5H6z" />
    <path d="M14 3.5V8h4.5" />
    <path d="M9 13h6M9 16.5h4" />
  {:else if name === 'checkin'}
    <circle cx="12" cy="12" r="8.5" />
    <path d="M8.4 12.2l2.5 2.6 4.7-5.4" />
  {:else if name === 'check'}
    <path d="M5.5 12.6l4.2 4.2 8.8-9.6" />
  {:else if name === 'task'}
    <path d="M4.4 6.4L5.7 7.7l2.3-2.6" />
    <path d="M11 6.5h8.6" />
    <path d="M4.4 12.4L5.7 13.7l2.3-2.6" />
    <path d="M11 12.5h8.6" />
    <path d="M4.4 18.4L5.7 19.7l2.3-2.6" />
    <path d="M11 18.5h8.6" />
  {:else if name === 'search'}
    <circle cx="10.5" cy="10.5" r="6" />
    <path d="M15 15l4.5 4.5" />
  {:else if name === 'settings'}
    <circle cx="12" cy="12" r="3" />
    <path
      d="M12 3.5v2.2M12 18.3v2.2M4.9 7.8l1.9 1.1M17.2 15.1l1.9 1.1M4.9 16.2l1.9-1.1M17.2 8.9l1.9-1.1"
    />
  {:else if name === 'calendar'}
    <rect x="3.5" y="5.5" width="17" height="15" rx="2.5" />
    <path d="M3.5 10h17M8 3.5v4M16 3.5v4" />
  {:else if name === 'palette'}
    <path d="M12 3.5a8.5 8.5 0 1 0 0 17c1.3 0 2-.9 2-1.9 0-.6-.3-1-.3-1.6 0-.9.7-1.6 1.7-1.6h1.4a3.6 3.6 0 0 0 3.7-3.6c0-4.5-3.8-8.3-8.5-8.3Z" />
    <circle cx="8" cy="10" r="1.2" />
    <circle cx="12" cy="7.8" r="1.2" />
    <circle cx="16" cy="10" r="1.2" />
  {:else if name === 'database'}
    <ellipse cx="12" cy="6.5" rx="7" ry="3" />
    <path d="M5 6.5v11c0 1.66 3.13 3 7 3s7-1.34 7-3v-11" />
    <path d="M5 12c0 1.66 3.13 3 7 3s7-1.34 7-3" />
  {:else if name === 'sliders'}
    <path d="M4.5 8h4M12.5 8h7M4.5 16h9M17.5 16h2" />
    <circle cx="10.5" cy="8" r="2" />
    <circle cx="15.5" cy="16" r="2" />
  {:else if name === 'info'}
    <circle cx="12" cy="12" r="8.5" />
    <path d="M12 11v5.5" />
    <path d="M12 7.6v.1" />
  {:else if name === 'tag'}
    <path d="M11.5 3.5H20v8.5l-8.6 8.6a1.5 1.5 0 0 1-2.1 0l-6.4-6.4a1.5 1.5 0 0 1 0-2.1z" />
    <circle cx="16" cy="8" r="1.4" />
  {:else if name === 'ai'}
    <path d="M12 3.5l1.9 5.1 5.1 1.9-5.1 1.9L12 17.5l-1.9-5.1L5 10.5l5.1-1.9z" />
    <path d="M18.5 16l.8 2.2 2.2.8-2.2.8-.8 2.2-.8-2.2-2.2-.8 2.2-.8z" />
  {:else if name === 'send'}
    <path d="M4 12h14M13 6.5l5.5 5.5L13 17.5" />
  {:else if name === 'plus'}
    <path d="M12 5.5v13M5.5 12h13" />
  {:else if name === 'minus'}
    <path d="M6 12h12" />
  {:else if name === 'close'}
    <path d="M6.5 6.5l11 11M17.5 6.5l-11 11" />
  {:else if name === 'maximize'}
    <rect x="6" y="6" width="12" height="12" rx="2" />
  {:else if name === 'chev-r'}
    <path d="M9.5 6l6 6-6 6" />
  {:else if name === 'chev-d'}
    <path d="M6 9.5l6 6 6-6" />
  {:else if name === 'chev-l'}
    <path d="M14.5 6l-6 6 6 6" />
  {:else if name === 'arrow-l'}
    <path d="M19 12H5M10.5 6.5L5 12l5.5 5.5" />
  {:else if name === 'folder'}
    <path d="M3.5 6.5A2 2 0 0 1 5.5 4.5h3.2l2 2.4h7.8a2 2 0 0 1 2 2v8.6a2 2 0 0 1-2 2H5.5a2 2 0 0 1-2-2z" />
  {:else if name === 'file'}
    <path d="M7 3.5h7l4 4v13H7z" />
    <path d="M14 3.5v4h4" />
  {:else if name === 'edit'}
    <path d="M4.5 19.5l.9-4.2L16.1 4.6a1.8 1.8 0 0 1 2.6 0l.7.7a1.8 1.8 0 0 1 0 2.6L8.7 18.6z" />
  {:else if name === 'book'}
    <path d="M12 6.5C10.5 5 8.5 4.5 4.5 4.5v13c4 0 6 .5 7.5 2 1.5-1.5 3.5-2 7.5-2v-13c-4 0-6 .5-7.5 2z" />
    <path d="M12 6.5v13" />
  {:else if name === 'trash'}
    <path d="M4.5 7h15M9 7V4.8h6V7M6.5 7l1 13h9l1-13" />
  {:else if name === 'sync'}
    <path d="M20 12a8 8 0 1 1-2.6-5.9" />
    <path d="M20 4.5V10h-5.5" />
  {:else if name === 'link'}
    <path d="M10.5 13.5a3.5 3.5 0 0 0 5 0l3-3a3.5 3.5 0 0 0-5-5l-1.2 1.2" />
    <path d="M13.5 10.5a3.5 3.5 0 0 0-5 0l-3 3a3.5 3.5 0 0 0 5 5l1.2-1.2" />
  {:else if name === 'image'}
    <rect x="3.5" y="5" width="17" height="14" rx="2.5" />
    <circle cx="9" cy="10" r="1.6" />
    <path d="M4 17l4.5-4.5 3 3 3.5-3.5 4 4" />
  {:else if name === 'refresh'}
    <path d="M20 12a8 8 0 1 1-2.6-5.9" />
    <path d="M20 4.5V10h-5.5" />
  {:else if name === 'github'}
    <path
      d="M12 3.2a9 9 0 0 0-2.85 17.54c.45.08.62-.2.62-.44v-1.7c-2.5.55-3.03-1.06-3.03-1.06-.41-1.04-1-1.32-1-1.32-.82-.56.06-.55.06-.55.9.06 1.38.93 1.38.93.8 1.38 2.1.98 2.62.75.08-.58.31-.98.57-1.2-2-.23-4.1-1-4.1-4.45 0-.98.35-1.79.93-2.42-.1-.23-.4-1.15.09-2.4 0 0 .75-.24 2.47.92a8.5 8.5 0 0 1 4.5 0c1.72-1.16 2.47-.92 2.47-.92.49 1.25.18 2.17.09 2.4.58.63.93 1.44.93 2.42 0 3.46-2.11 4.22-4.12 4.44.32.28.61.83.61 1.68v2.49c0 .24.16.53.62.44A9 9 0 0 0 12 3.2Z"
    />
  {:else if name === 'search-file'}
    <circle cx="10.5" cy="10.5" r="6" />
    <path d="M15 15l4.5 4.5" />
  {:else if name === 'target'}
    <circle cx="12" cy="12" r="8" />
    <circle cx="12" cy="12" r="3" />
  {:else if name === 'flame'}
    <path d="M12 3.5c3 3.2 4.5 5.6 4.5 8.2a4.5 4.5 0 0 1-9 0c0-1.2.4-2.3 1.2-3.4" />
  {:else if name === 'chart'}
    <path d="M4.5 19.5h15" />
    <path d="M7.5 19.5v-6M12 19.5V7M16.5 19.5v-9" />
  {:else if name === 'grid'}
    <rect x="4" y="4" width="7" height="7" rx="1.6" />
    <rect x="13" y="4" width="7" height="7" rx="1.6" />
    <rect x="4" y="13" width="7" height="7" rx="1.6" />
    <rect x="13" y="13" width="7" height="7" rx="1.6" />
  {:else if name === 'copy'}
    <rect x="8.5" y="8.5" width="11" height="11" rx="2" />
    <path d="M15.5 5.5h-9a2 2 0 0 0-2 2v9" />
  {:else if name === 'terminal'}
    <rect x="3.5" y="5" width="17" height="14" rx="2.5" />
    <path d="M7.5 10l2.5 2-2.5 2M12.5 14.5h4" />
  {/if}
</svg>

<style>
  svg {
    display: block;
    flex: 0 0 auto;
  }
</style>
