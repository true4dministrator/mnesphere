<script lang="ts">
  import Icon from './Icons.svelte';
  import { closeCtxMenu, ctxMenu, type CtxItem } from '../state.svelte';

  let el = $state<HTMLDivElement | null>(null);
  let pos = $state({ x: 0, y: 0 });

  /** 贴着视口边缘右键时，菜单得往回收，否则一半在屏幕外。 */
  $effect(() => {
    if (!ctxMenu.open || !el) return;
    const r = el.getBoundingClientRect();
    const pad = 8;
    let x = ctxMenu.x;
    let y = ctxMenu.y;
    if (x + r.width + pad > window.innerWidth) x = window.innerWidth - r.width - pad;
    if (y + r.height + pad > window.innerHeight) y = window.innerHeight - r.height - pad;
    pos = { x: Math.max(pad, x), y: Math.max(pad, y) };
  });

  function pick(it: CtxItem) {
    if (it.disabled) return;
    closeCtxMenu();
    it.run?.();
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape') closeCtxMenu();
  }
</script>

<svelte:window onkeydown={onKey} onresize={closeCtxMenu} />

{#if ctxMenu.open}
  <div
    class="back"
    role="presentation"
    onclick={closeCtxMenu}
    oncontextmenu={(e) => {
      e.preventDefault();
      closeCtxMenu();
    }}
  ></div>

  <div class="menu" bind:this={el} style="left:{pos.x}px; top:{pos.y}px" role="menu" tabindex="-1">
    {#each ctxMenu.items as it, i (i)}
      {#if it.sep}
        <div class="sep"></div>
      {:else}
        <button
          class="mi"
          class:danger={it.danger}
          disabled={it.disabled}
          role="menuitem"
          onclick={() => pick(it)}
        >
          {#if it.icon}
            <Icon name={it.icon} size={13} />
          {/if}
          <span>{it.label}</span>
        </button>
      {/if}
    {/each}
  </div>
{/if}

<style>
  .back {
    position: fixed;
    inset: 0;
    z-index: 90;
  }

  .menu {
    position: fixed;
    z-index: 91;
    min-width: 186px;
    padding: 4px;
    border-radius: calc(var(--radius) * 0.7);
    background: var(--surface);
    border: 1px solid var(--line-2);
    box-shadow: 0 14px 34px -14px rgba(0, 0, 0, 0.72);
    backdrop-filter: blur(22px);
  }

  .mi {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    height: 28px;
    padding: 0 9px;
    border-radius: 7px;
    color: var(--fg-dim);
    font-size: 12.5px;
    text-align: left;
    transition: background 0.1s, color 0.1s;
  }
  .mi span {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .mi:hover:not(:disabled) {
    background: var(--hover);
    color: var(--fg);
  }
  .mi.danger {
    color: var(--danger);
  }
  .mi.danger:hover:not(:disabled) {
    background: var(--danger-soft);
  }
  .mi:disabled {
    opacity: 0.36;
    cursor: default;
  }

  .sep {
    height: 1px;
    margin: 4px 6px;
    background: var(--line);
  }
</style>
