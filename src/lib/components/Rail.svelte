<script lang="ts">
  import Icon from './Icons.svelte';
  import {
    doc,
    goToTodayDiary,
    noteSortMode,
    notes,
    openDoc,
    refreshTasks,
    ui
  } from '../state.svelte';
  import type { Activity } from '../state.svelte';
  import { newestNotePath } from '../notesSort';

  type Item = { id: Activity; icon: string; label: string };

  // 顺序 = 用户从早到晚的使用节奏：先写笔记、再记日记、顺手打卡、最后盘任务
  const main: Item[] = [
    { id: 'notes', icon: 'notes', label: '笔记' },
    { id: 'diary', icon: 'diary', label: '日记' },
    { id: 'checkin', icon: 'checkin', label: '打卡' },
    { id: 'task', icon: 'task', label: '任务' }
  ];

  /** 模块隔离的关键：切到哪个模块，就只保证那个模块的内容在前台，
   *  绝不把日记、笔记、打卡、任务混在一个列表里。 */
  async function select(id: Activity) {
    ui.activity = id;
    ui.panelOpen = true;

    // 日记：**任何**状态点这个图标都落到「今天 + 编辑态」——
    // 已经在今天这篇的编辑态里就什么也不做，在阅读态只切编辑态，别处则打开今天。
    if (id === 'diary') {
      await goToTodayDiary();
      return;
    }

    if (id === 'notes') {
      // 已经在看某一篇笔记就不动它（再点一下不该把人从原来的位置拽走）
      if (doc.data?.kind === 'note' && doc.path) return;
      // 落点口径跟排序口径对齐，所见即所得：
      //   · 按修改时间排 → 打开**全局最新**那篇（要的就是「最近在弄的那篇」，
      //     而不是第一个文件夹里的第一个文件）
      //   · 按名称排 → 打开树里第一篇
      const target =
        noteSortMode() === 'mtime' ? newestNotePath(notes.roots) : firstNote(notes.roots);
      if (target) await openDoc(target);
      return;
    }

    if (id === 'task') {
      await refreshTasks();
      return;
    }
  }

  function firstNote(tree: typeof notes.roots): string | null {
    for (const n of tree) {
      if (!n.isDir) return n.path;
      const deeper = firstNote(n.children);
      if (deeper) return deeper;
    }
    return null;
  }
</script>

<nav class="rail">
  <div class="top">
    {#each main as it (it.id)}
      <button
        class="ico"
        class:on={ui.activity === it.id}
        title={it.label}
        aria-label={it.label}
        onclick={() => select(it.id)}
      >
        <Icon name={it.icon} size={19} />
        {#if ui.activity === it.id}
          <i class="mark"></i>
        {/if}
      </button>
    {/each}
  </div>

  <div class="bottom">
    <button
      class="ico"
      class:on={ui.activity === 'settings'}
      title="设置"
      aria-label="设置"
      onclick={() => select('settings')}
    >
      <Icon name="settings" size={19} />
    </button>
  </div>
</nav>

<style>
  .rail {
    position: relative;
    z-index: 2;
    display: flex;
    flex-direction: column;
    justify-content: space-between;
    width: var(--rail-w);
    flex: 0 0 var(--rail-w);
    padding: 8px 0;
    background: var(--panel-2);
    backdrop-filter: blur(var(--glass-blur, 0px));
    border-right: 1px solid var(--line);
  }

  .top,
  .bottom {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 4px;
  }

  .ico {
    position: relative;
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    border-radius: 9px;
    color: var(--fg-mute);
    transition: background 0.13s, color 0.13s;
  }
  .ico:hover {
    background: var(--hover);
    color: var(--fg);
  }
  .ico.on {
    background: var(--accent-soft);
    color: var(--accent);
  }

  .ico .mark {
    position: absolute;
    left: -8px;
    top: 50%;
    transform: translateY(-50%);
    width: 2.5px;
    height: 16px;
    border-radius: 2px;
    background: var(--accent);
  }
</style>
