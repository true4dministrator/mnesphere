<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import * as api from '../api';
  import Icon from './Icons.svelte';
  import CMEditor from './CMEditor.svelte';
  import Reader from './Reader.svelte';
  import {
    cfg,
    checkin,
    doc,
    flushDoc,
    openDoc,
    refreshCheckin,
    refreshDiaries,
    refreshNotes,
    toast,
    ui
  } from '../state.svelte';

  let more = $state(false);

  const isDiary = $derived(doc.data?.kind === 'diary');

  function weekdayLabel(date: string): string {
    const d = new Date(`${date}T00:00:00`);
    if (Number.isNaN(d.getTime())) return '';
    return ['周日', '周一', '周二', '周三', '周四', '周五', '周六'][d.getDay()];
  }

  const today = new Date().toLocaleDateString('sv-SE');
  const todayCell = $derived(checkin.view?.days.find((d) => d.date === today) ?? null);

  async function cycle(habit: string) {
    if (!todayCell) return;
    try {
      await api.cycleCheckin(habit, today);
      await refreshCheckin();
    } catch (e) {
      toast(typeof e === 'string' ? e : String(e), 'error');
    }
  }

  async function copyPath() {
    try {
      const abs = await api.revealPath(doc.path);
      await navigator.clipboard.writeText(abs);
      toast('绝对路径已复制', 'ok');
    } catch (e) {
      toast(String(e), 'error');
    }
    more = false;
  }

  async function remove() {
    more = false;
    try {
      await api.deleteDoc(doc.path);
      toast('已移到回收站', 'ok');
      doc.path = '';
      doc.data = null;
      await refreshDiaries();
      await refreshNotes();
    } catch (e) {
      toast(typeof e === 'string' ? e : String(e), 'error');
    }
  }

  function onWiki(e: Event) {
    const target = (e as CustomEvent<string>).detail;
    void api.resolveLink(target).then(async (rel) => {
      if (rel) {
        ui.activity = rel.startsWith(cfg.current?.editor.diaryDir ?? '日记') ? 'diary' : 'notes';
        await openDoc(rel);
      } else {
        toast(`「${target}」还不存在`, 'info');
      }
    });
  }

  onMount(() => window.addEventListener('mnesphere:wikilink', onWiki));
  onDestroy(() => window.removeEventListener('mnesphere:wikilink', onWiki));
</script>

<section class="editor">
  <header class="eh">
    <div class="left">
      {#if !doc.data}
        <span class="t muted">没有打开任何文档</span>
      {:else if isDiary}
        <span class="t">{doc.data.date}</span>
        <span class="pill-date">{weekdayLabel(doc.data.date)}</span>
      {:else}
        <span class="t">{doc.data.title}</span>
      {/if}

      {#if doc.data && doc.data.tags.length}
        <div class="tags">
          {#each doc.data.tags.slice(0, 6) as t (t)}
            <span class="chip accent">#{t}</span>
          {/each}
        </div>
      {/if}
    </div>

    <div class="right">
      <span class="wcount">{doc.data?.words ?? 0} 字</span>

      <div class="seg">
        <button class:on={doc.mode === 'read'} onclick={() => (doc.mode = 'read')}>阅读</button>
        <button class:on={doc.mode === 'edit'} onclick={() => (doc.mode = 'edit')}>编辑</button>
      </div>

      <button class="ico" title="保存（Ctrl+S）" onclick={() => flushDoc().then(() => toast('已保存'))}>
        <Icon name="refresh" size={14} />
      </button>

      <div class="morewrap">
        <button class="ico" title="更多" onclick={() => (more = !more)}>
          <Icon name="grid" size={14} />
        </button>
        {#if more}
          <div class="backdrop" role="presentation" onclick={() => (more = false)}></div>
          <div class="menu">
            <button
              onclick={async () => {
                more = false;
                const abs = await api.revealPath(doc.path);
                await api.openInExplorer(abs, true);
              }}><Icon name="folder" size={13} /> 在资源管理器中显示</button
            >
            <button onclick={copyPath}><Icon name="copy" size={13} /> 复制绝对路径</button>
            <button
              onclick={async () => {
                more = false;
                await flushDoc();
                toast('已保存到磁盘', 'ok');
              }}><Icon name="refresh" size={13} /> 立即保存</button
            >
            <div class="hair"></div>
            <button class="danger" onclick={remove}><Icon name="trash" size={13} /> 移到回收站</button>
          </div>
        {/if}
      </div>
    </div>
  </header>

  {#if !doc.data}
    <div class="placeholder">
      <div class="orb" aria-hidden="true"></div>
      <p>左边选一篇日记或笔记开始。</p>
      <p class="sub">
        所有内容都是普通的 Markdown 文件，躺在 <span class="mono">{cfg.current?.vault}</span> 里，
        你随时可以拿 Obsidian 或 VS Code 直接打开。
      </p>
      <div class="hints">
        <span class="k">Ctrl+K</span> 命令面板
        <span class="k">Ctrl+J</span> AI 助手
        <span class="k">Ctrl+E</span> 切换阅读/编辑
        <span class="k">Ctrl+B</span> 收起左栏
      </div>
    </div>
  {:else if doc.mode === 'edit'}
    <CMEditor />
  {:else}
    <Reader />
  {/if}

  {#if doc.data && isDiary && todayCell && todayCell.states && Object.keys(todayCell.states).length}
    <footer class="qbar">
      <span class="qlabel">今日打卡</span>
      {#each Object.entries(todayCell.states) as [name, st] (name)}
        <button
          class="qchip {st}"
          onclick={() => cycle(name)}
          title="点一下循环切换：未打卡 → 做到了 → 没做到"
        >
          <i></i>{name}
        </button>
      {/each}
    </footer>
  {/if}

  {#if doc.data && doc.data.backlinks.length}
    <footer class="backlinks">
      <span class="blabel"><Icon name="link" size={12} /> 反向链接</span>
      {#each doc.data.backlinks.slice(0, 12) as b (b.path)}
        <button class="bl" onclick={() => openDoc(b.path)}>{b.title}</button>
      {/each}
    </footer>
  {/if}
</section>

<style>
  .editor {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
    min-width: 0;
  }

  .eh {
    display: flex;
    align-items: center;
    gap: 10px;
    height: 40px;
    flex: 0 0 40px;
    padding: 0 12px 0 18px;
    border-bottom: 1px solid var(--line);
  }

  .left {
    display: flex;
    align-items: center;
    gap: 7px;
    flex: 1;
    min-width: 0;
    overflow: hidden;
  }

  .t {
    font-size: 13px;
    color: var(--fg);
    font-weight: 500;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .t.muted {
    color: var(--fg-mute);
    font-weight: 400;
  }

  .pill-date {
    font-size: 11px;
    color: var(--fg-mute);
    font-family: var(--font-mono);
  }

  .chip {
    display: inline-flex;
    align-items: center;
    height: 20px;
    padding: 0 9px;
    border-radius: 999px;
    background: var(--card);
    color: var(--fg-mute);
    font-size: 11px;
    white-space: nowrap;
  }
  .chip:hover {
    color: var(--accent);
    background: var(--accent-soft);
  }
  .chip.accent {
    background: var(--accent-soft);
    color: var(--accent);
  }

  .tags {
    display: flex;
    gap: 5px;
    overflow: hidden;
  }

  .right {
    display: flex;
    align-items: center;
    gap: 8px;
    flex: 0 0 auto;
  }

  .wcount {
    font-size: 11px;
    color: var(--fg-faint);
    font-family: var(--font-mono);
  }

  .seg {
    display: flex;
    padding: 2px;
    border-radius: 8px;
    background: var(--card);
  }
  .seg button {
    height: 20px;
    padding: 0 10px;
    border-radius: 6px;
    font-size: 11.5px;
    color: var(--fg-mute);
    transition: all 0.12s;
  }
  .seg button.on {
    background: var(--accent-soft);
    color: var(--accent);
  }

  .ico {
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
    border-radius: 7px;
    color: var(--fg-mute);
  }
  .ico:hover {
    background: var(--hover);
    color: var(--fg);
  }

  .morewrap {
    position: relative;
  }
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 55;
  }
  .menu {
    position: absolute;
    right: 0;
    top: 30px;
    z-index: 56;
    min-width: 200px;
    padding: 5px;
    border-radius: 10px;
    background: var(--surface);
    border: 1px solid var(--line-2);
    backdrop-filter: blur(22px);
    box-shadow: 0 12px 32px rgba(0, 0, 0, 0.4);
  }
  .menu button {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 7px 9px;
    border-radius: 7px;
    color: var(--fg-dim);
    font-size: 12.5px;
    text-align: left;
  }
  .menu button:hover {
    background: var(--hover);
    color: var(--fg);
  }
  .menu button.danger {
    color: var(--danger);
  }
  .menu button.danger:hover {
    background: var(--danger-soft);
  }
  .menu .hair {
    margin: 4px 6px;
  }

  .placeholder {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 10px;
    padding: 30px;
    text-align: center;
    color: var(--fg-mute);
  }
  .orb {
    width: 46px;
    height: 46px;
    border-radius: 50%;
    border: 2px solid var(--accent-line);
    box-shadow: 0 0 26px -6px var(--accent);
    margin-bottom: 8px;
  }
  .placeholder p {
    font-size: 13px;
  }
  .placeholder .sub {
    font-size: 12px;
    color: var(--fg-faint);
    max-width: 460px;
    line-height: 1.75;
  }
  .hints {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-top: 14px;
    font-size: 11px;
    color: var(--fg-faint);
  }
  .k {
    display: inline-block;
    padding: 2px 7px;
    border-radius: 5px;
    background: var(--card);
    border: 1px solid var(--line);
    font-family: var(--font-mono);
    color: var(--fg-mute);
  }

  .qbar {
    display: flex;
    align-items: center;
    gap: 7px;
    padding: 8px 18px;
    border-top: 1px solid var(--line);
    background: var(--panel-2);
    flex-wrap: wrap;
  }
  .qlabel {
    font-size: 11px;
    color: var(--fg-faint);
    margin-right: 4px;
  }
  .qchip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 23px;
    padding: 0 10px;
    border-radius: 999px;
    background: var(--card);
    border: 1px solid transparent;
    color: var(--fg-mute);
    font-size: 11.5px;
  }
  .qchip i {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--fg-faint);
  }
  .qchip.done {
    background: var(--accent-soft);
    border-color: var(--accent-line);
    color: var(--accent);
  }
  .qchip.done i {
    background: var(--accent);
  }
  .qchip.missed {
    background: var(--danger-soft);
    border-color: var(--danger);
    color: var(--danger);
  }
  .qchip.missed i {
    background: var(--danger);
  }

  .backlinks {
    display: flex;
    align-items: center;
    gap: 7px;
    padding: 8px 18px;
    border-top: 1px solid var(--line);
    flex-wrap: wrap;
    max-height: 82px;
    overflow: auto;
  }
  .blabel {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    font-size: 11px;
    color: var(--fg-faint);
  }
  .bl {
    font-size: 11.5px;
    color: var(--accent-2);
    padding: 2px 8px;
    border-radius: 6px;
    background: var(--accent-soft);
  }
  .bl:hover {
    background: var(--accent);
    color: var(--on-accent);
  }
</style>
