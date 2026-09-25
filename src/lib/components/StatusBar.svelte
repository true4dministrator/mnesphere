<script lang="ts">
  import { onMount } from 'svelte';
  import Icon from './Icons.svelte';
  import { openExternal } from '../api';
  import { ai, checkin, doc, gh, stats, tasks, ui } from '../state.svelte';
  import { relTime } from '../time';

  const LABEL: Record<string, string> = {
    diary: '日记',
    notes: '笔记',
    checkin: '打卡',
    task: '任务',
    settings: '设置'
  };

  // 五个模块在 Icons 里各有一个同名图标，直接拿 activity 当图标名用
  const moduleIcon = $derived(ui.activity);

  /** 待办计数：过期单独拎出来，它是唯一需要「立刻处理」的信号。 */
  const todo = $derived.by(() => {
    const today = new Date().toLocaleDateString('sv-SE');
    let open = 0;
    let overdue = 0;
    for (const t of tasks.items) {
      if (t.done) continue;
      open += 1;
      if (t.due && t.due < today) overdue += 1;
    }
    return { open, overdue };
  });

  /** 当前文档路径的尾巴：日记/2026-09/2026-09-19.md → 2026-09/2026-09-19 */
  const crumbs = $derived.by(() => {
    const p = doc.data?.path ?? doc.path;
    if (!p) return [] as string[];
    return p.replace(/\.md$/i, '').split('/');
  });

  const today = $derived.by(() => {
    const v = checkin.view;
    if (!v) return null;
    const day = v.days.find((d) => d.today);
    if (!day) return null;
    const names = v.habits.filter((h) => !h.archived).map((h) => h.name);
    if (!names.length) return null;
    const done = names.filter((n) => day.states[n] === 'done').length;
    return { done, total: names.length };
  });

  const streak = $derived(
    checkin.focus !== 'all' && checkin.stats ? checkin.stats.currentStreak : 0
  );

  /** 驱动相对时间刷新（每 30 秒动一次），否则「3 分钟前」会一直停在「3 分钟前」 */
  let now = $state(Date.now());
  onMount(() => {
    const id = setInterval(() => (now = Date.now()), 30_000);
    return () => clearInterval(id);
  });

  /**
   * 状态栏那一格 GitHub 的显示内容。null = 没配仓库，整格退化成一个灰字。
   *
   * 三种状态：
   *   · 有未推的改动 → 黄点 + 相对时间（提示该点同步了）
   *   · 已同步       → 常规色 + 相对时间
   *   · 从没同步过   → 「未同步」
   */
  const ghCell = $derived.by(() => {
    const s = gh.stat;
    if (!s || !s.configured) return null;
    return {
      url: s.url,
      dirty: s.dirty,
      label: s.lastSync ? relTime(s.lastSync, now) : '未同步',
      tip: s.dirty
        ? `有改动还没推上去${s.lastCommit ? `（上次提交 ${s.lastCommit.slice(0, 7)}）` : ''} · 点击打开仓库`
        : `${s.lastSync ? `上次同步 ${s.lastSync}` : '还没同步过'} · 点击打开仓库`
    };
  });
  const saveLabel = $derived(
    doc.saving ? '保存中' : doc.dirty ? '未保存' : doc.data ? '已保存' : ''
  );
</script>

<footer class="bar">
  <div class="side left">
    <span class="mod" title="当前模块">
      <Icon name={moduleIcon} size={12} />
      <b>{LABEL[ui.activity] ?? ui.activity}</b>
    </span>

    {#if crumbs.length}
      <span class="sep">›</span>
      <span class="path mono" title={doc.data?.path ?? doc.path}>
        {#each crumbs as c, i (i)}<i class={i === crumbs.length - 1 ? 'tail' : ''}>{c}</i>{#if i < crumbs.length - 1}<s>/</s>{/if}{/each}
      </span>
      {#if doc.mode === 'read'}
        <span class="pill" title="当前为阅读模式">阅读</span>
      {:else}
        <span class="pill accent" title="当前为编辑模式">编辑</span>
      {/if}
      {#if saveLabel}
        <span class="dotstate" class:dirty={doc.dirty} class:saving={doc.saving}>
          <s></s>{saveLabel}
        </span>
      {/if}
    {/if}
  </div>

  <div class="side right">
    {#if doc.data}
      <span class="item" title="当前文档字数">
        <Icon name="chart" size={12} />
        <em class="mono">{doc.data.words.toLocaleString()}</em>
      </span>
    {/if}

    {#if streak > 0}
      <span class="item accent" title="当前习惯连续达标天数">
        <Icon name="flame" size={12} />
        <em class="mono">{streak}</em>天
      </span>
    {/if}

    {#if tasks.items.length}
      <button
        class="item taskitem"
        class:danger={todo.overdue > 0}
        title={todo.overdue > 0 ? `${todo.overdue} 项已逾期` : '未完成任务数'}
        onclick={() => {
          ui.activity = 'task';
          ui.panelOpen = true;
        }}
      >
        <Icon name="task" size={12} />
        <em class="mono">{todo.open}</em>待办{#if todo.overdue > 0}<em class="mono">{todo.overdue}</em
          >逾期{/if}
      </button>
    {/if}

    {#if today}
      <span class="item" title="今日打卡进度">
        <Icon name="checkin" size={12} />
        <em class="mono">{today.done}/{today.total}</em>
      </span>
    {/if}

    {#if stats.index}
      <span class="item muted" title="索引：笔记 / 日记 / 双链">
        <Icon name="grid" size={12} />
        <em class="mono">{stats.index.notes}</em>/<em class="mono">{stats.index.diaries}</em>/<em class="mono">{stats.index.links}</em>
      </span>
    {/if}

    <span class="item {ai.hasKey ? 'ok' : 'muted'}" title={ai.hasKey ? 'AI 已配置密钥' : 'AI 未配置密钥'}>
      <Icon name="ai" size={12} />
      AI
    </span>

    {#if ghCell}
      <button
        class="item ghitem"
        class:ok={!ghCell.dirty}
        class:dirty={ghCell.dirty}
        title={ghCell.tip}
        onclick={() => void openExternal(ghCell.url)}
      >
        <Icon name="github" size={12} />
        {#if ghCell.dirty}<s class="gdot"></s>{/if}
        {ghCell.label}
      </button>
    {:else}
      <span class="item muted" title="GitHub 未配置">
        <Icon name="github" size={12} />
        未配置
      </span>
    {/if}
  </div>
</footer>

<style>
  .bar {
    position: relative;
    z-index: 3;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    height: var(--status-h);
    padding: 0 10px;
    background: var(--panel-2);
    border-top: 1px solid var(--line);
    font-size: 11.5px;
    color: var(--fg-mute);
    backdrop-filter: blur(var(--glass-blur, 0px));
    white-space: nowrap;
    overflow: hidden;
  }

  .side {
    display: flex;
    align-items: center;
    gap: 9px;
    min-width: 0;
  }
  .right {
    flex: 0 0 auto;
    gap: 13px;
  }
  .left {
    flex: 1 1 auto;
    overflow: hidden;
  }

  .mod {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    color: var(--accent);
  }
  .mod b {
    font-weight: 500;
  }

  .sep {
    color: var(--fg-faint);
  }

  .path {
    display: inline-flex;
    align-items: center;
    gap: 0;
    min-width: 0;
    overflow: hidden;
    font-size: 11px;
    color: var(--fg-dim);
    text-overflow: ellipsis;
    user-select: text;
  }
  .path i {
    font-style: normal;
  }
  .path i.tail {
    color: var(--fg);
  }
  .path s {
    text-decoration: none;
    margin: 0 1px;
    color: var(--fg-faint);
  }

  .pill {
    flex: 0 0 auto;
    padding: 0 7px;
    height: 15px;
    display: inline-flex;
    align-items: center;
    border-radius: 8px;
    background: var(--card);
    color: var(--fg-mute);
    font-size: 10.5px;
  }
  .pill.accent {
    background: var(--accent-soft);
    color: var(--accent);
  }

  .dotstate {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    flex: 0 0 auto;
    color: var(--fg-faint);
    font-size: 10.5px;
  }
  .dotstate s {
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: var(--accent);
    opacity: 0.75;
    text-decoration: none;
  }
  .dotstate.dirty {
    color: var(--warn);
  }
  .dotstate.dirty s {
    background: var(--warn);
    animation: breath 1.5s ease-in-out infinite;
  }
  .dotstate.saving {
    color: var(--accent);
  }
  .dotstate.saving s {
    background: var(--accent);
    animation: breath 0.7s ease-in-out infinite;
  }

  .item {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    color: var(--fg-dim);
  }
  .item em {
    font-style: normal;
    color: var(--fg);
  }
  .item.muted {
    color: var(--fg-faint);
  }
  .item.accent {
    color: var(--accent);
  }
  .item.ok {
    color: var(--accent-2);
  }

  /* 状态栏里的待办是个可点的快捷入口，别看着像纯文本 */
  .taskitem {
    height: 18px;
    padding: 0 7px;
    border-radius: 9px;
    gap: 3px;
  }
  .taskitem:hover {
    background: var(--hover);
    color: var(--fg);
  }
  .taskitem.danger {
    background: var(--danger-soft);
    color: var(--danger);
  }
  .taskitem.danger em {
    color: var(--danger);
  }

  /* 状态栏里的 GitHub 格子是个可点入口（打开仓库），别看着像纯文本 */
  .ghitem {
    height: 18px;
    padding: 0 7px;
    border-radius: 9px;
    gap: 4px;
  }
  .ghitem:hover {
    background: var(--hover);
    color: var(--fg);
  }
  .ghitem.dirty {
    color: var(--warn);
  }
  /* 「有改动还没推」的小黄点，跟文档「未保存」那个点同一个呼吸节奏 */
  .ghitem .gdot {
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: var(--warn);
    text-decoration: none;
    animation: breath 1.5s ease-in-out infinite;
  }

  @keyframes breath {
    0%,
    100% {
      opacity: 0.35;
    }
    50% {
      opacity: 1;
    }
  }
</style>
