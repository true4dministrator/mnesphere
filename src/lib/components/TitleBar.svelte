<script lang="ts">
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import Icon from './Icons.svelte';
  import { doc, find, openDoc, searchNow } from '../state.svelte';

  const win = getCurrentWindow();
  let maximized = $state(false);

  let box = $state<HTMLDivElement | null>(null);
  let input = $state<HTMLInputElement | null>(null);
  let lastToken = -1;

  const titles: Record<string, string> = {
    diary: '日记',
    notes: '笔记',
    checkin: '打卡',
    task: '任务',
    settings: '设置'
  };

  async function sync() {
    maximized = await win.isMaximized();
  }

  $effect(() => {
    void sync();
  });

  // Ctrl+K 只是把 focusToken 加一，这里负责真的去聚焦
  $effect(() => {
    if (find.focusToken === lastToken) return;
    lastToken = find.focusToken;
    find.open = true;
    input?.focus();
    input?.select();
  });

  function onInput() {
    find.open = true;
    searchNow();
  }

  function pick(path: string) {
    find.open = false;
    input?.blur();
    void openDoc(path);
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      find.open = false;
      input?.blur();
      return;
    }
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      find.open = true;
      if (find.hits.length) find.index = (find.index + 1) % find.hits.length;
      return;
    }
    if (e.key === 'ArrowUp') {
      e.preventDefault();
      if (find.hits.length) find.index = (find.index - 1 + find.hits.length) % find.hits.length;
      return;
    }
    if (e.key === 'Enter') {
      const hit = find.hits[find.index];
      if (hit) pick(hit.meta.path);
    }
  }

  /** 点别处就收起来。用 mousedown 而不是 click —— 否则点结果时先被这个关掉。 */
  function onGlobalDown(e: MouseEvent) {
    if (!find.open) return;
    if (box && !box.contains(e.target as Node)) find.open = false;
  }
</script>

<svelte:window onmousedown={onGlobalDown} />

<header class="bar" data-tauri-drag-region>
  <div class="left" data-tauri-drag-region>
    <span class="glyph" aria-hidden="true"></span>
    <span class="brand" data-tauri-drag-region>mnesphere</span>
    {#if doc.dirty}
      <span class="dot dirty" title="有未保存的改动"></span>
    {:else if doc.saving}
      <span class="dot" title="保存中"></span>
    {/if}
  </div>

  <div class="center">
    <div class="findbox" bind:this={box}>
      <span class="fi" aria-hidden="true"><Icon name="search" size={13} /></span>
      <input
        class="find"
        bind:this={input}
        bind:value={find.query}
        placeholder="搜索日记与笔记…（Ctrl+K）"
        spellcheck="false"
        oninput={onInput}
        onfocus={() => (find.open = true)}
        onkeydown={onKey}
      />
      {#if find.query}
        <button
          class="fclear"
          title="清空"
          aria-label="清空搜索"
          onclick={() => {
            find.query = '';
            searchNow();
            input?.focus();
          }}><Icon name="close" size={11} /></button
        >
      {/if}

      {#if find.open && find.query.trim()}
        <div class="results scroll">
          {#if find.busy && !find.hits.length}
            <p class="rhint">正在找…</p>
          {:else if !find.hits.length}
            <p class="rhint">没有匹配的内容。</p>
          {:else}
            {#each find.hits as h, i (h.meta.path)}
              <button class="hit" class:on={i === find.index} onclick={() => pick(h.meta.path)}>
                <span class="ht">
                  <Icon name={h.meta.kind === 'diary' ? 'diary' : 'notes'} size={12} />
                  {h.meta.title}
                </span>
                <span class="hp mono">{h.meta.path}</span>
                <span class="hs">{h.snippet}</span>
              </button>
            {/each}
          {/if}
        </div>
      {/if}
    </div>
  </div>

  <div class="right">
    <div class="win-ctl">
      <button title="最小化" onclick={() => win.minimize()} aria-label="最小化">
        <Icon name="minus" size={13} />
      </button>
      <button
        title={maximized ? '还原' : '最大化'}
        onclick={async () => {
          await win.toggleMaximize();
          await sync();
        }}
        aria-label="最大化"
      >
        <Icon name="maximize" size={12} />
      </button>
      <button class="close" title="关闭到托盘" onclick={() => win.close()} aria-label="关闭">
        <Icon name="close" size={13} />
      </button>
    </div>
  </div>
</header>

<style>
  .bar {
    position: relative;
    z-index: 20;
    display: flex;
    align-items: center;
    height: var(--titlebar-h);
    flex: 0 0 var(--titlebar-h);
    padding: 0 8px 0 12px;
    background: var(--panel-2);
    border-bottom: 1px solid var(--line);
    backdrop-filter: blur(var(--glass-blur, 0px));
  }

  .left {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 170px;
    flex: 0 0 170px;
  }

  .glyph {
    width: 11px;
    height: 11px;
    border-radius: 3.5px;
    background: var(--accent);
    box-shadow: 0 0 10px -1px var(--accent-line);
  }

  .brand {
    font-size: 12px;
    color: var(--fg-mute);
    letter-spacing: 0.2px;
  }

  .dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--fg-faint);
    flex: 0 0 auto;
  }
  .dot.dirty {
    background: var(--warn);
  }

  .center {
    flex: 1;
    display: flex;
    justify-content: center;
    min-width: 0;
  }

  /* ── 搜索 ── */
  .findbox {
    position: relative;
    width: 100%;
    max-width: 430px;
    height: 23px;
  }

  .fi {
    position: absolute;
    left: 9px;
    top: 50%;
    transform: translateY(-50%);
    display: grid;
    place-items: center;
    color: var(--fg-faint);
    pointer-events: none;
  }

  .find {
    width: 100%;
    height: 23px;
    padding: 0 26px 0 27px;
    border-radius: 999px;
    background: var(--inset);
    border: 1px solid var(--line);
    color: var(--fg);
    font-size: 11.5px;
    transition: border-color 0.13s, background 0.13s;
  }
  .find::placeholder {
    color: var(--fg-faint);
  }
  .find:focus {
    border-color: var(--accent-line);
    background: var(--accent-soft);
  }

  .fclear {
    position: absolute;
    right: 7px;
    top: 50%;
    transform: translateY(-50%);
    display: grid;
    place-items: center;
    width: 15px;
    height: 15px;
    border-radius: 50%;
    color: var(--fg-faint);
  }
  .fclear:hover {
    background: var(--hover);
    color: var(--fg);
  }

  .results {
    position: absolute;
    top: calc(100% + 6px);
    left: 0;
    right: 0;
    z-index: 40;
    max-height: 340px;
    padding: 4px;
    border-radius: calc(var(--radius) * 0.7);
    background: var(--surface);
    border: 1px solid var(--line-2);
    box-shadow: 0 16px 38px -16px rgba(0, 0, 0, 0.72);
    backdrop-filter: blur(22px);
  }

  .rhint {
    padding: 10px 10px;
    color: var(--fg-faint);
    font-size: 12px;
  }

  .hit {
    display: flex;
    flex-direction: column;
    gap: 1px;
    width: 100%;
    padding: 6px 9px;
    border-radius: 7px;
    text-align: left;
  }
  .hit:hover,
  .hit.on {
    background: var(--hover);
  }
  .hit.on {
    box-shadow: inset 2px 0 0 var(--accent);
  }

  .ht {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12.5px;
    color: var(--fg);
  }
  .hp {
    font-size: 10.5px;
    color: var(--fg-faint);
  }
  .hs {
    font-size: 11.5px;
    color: var(--fg-mute);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* ── 右侧 ── */
  .right {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 170px;
    flex: 0 0 170px;
    justify-content: flex-end;
  }

  .win-ctl {
    display: flex;
    gap: 2px;
    margin-left: 4px;
  }
  .win-ctl button {
    display: grid;
    place-items: center;
    width: 28px;
    height: 24px;
    border-radius: 6px;
    color: var(--fg-mute);
    transition: background 0.12s, color 0.12s;
  }
  .win-ctl button:hover {
    background: var(--hover);
    color: var(--fg);
  }
  .win-ctl button.close:hover {
    background: var(--danger-soft);
    color: var(--danger);
  }
</style>
