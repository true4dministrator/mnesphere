<script lang="ts">
  import Icon from './Icons.svelte';
  import { ai, cfg, checkin, doc, stats, tasks, ui } from '../state.svelte';

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

  /** lastSync 后端存的是 ISO 字符串，这里只做展示层的轻度裁剪 */
  function shortTime(v: string): string {
    if (!v) return '';
    const d = new Date(v);
    if (Number.isNaN(d.getTime())) return v;
    const p = (n: number) => String(n).padStart(2, '0');
    return `${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`;
  }

  const gh = $derived(cfg.current?.github);
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

    <span class="item {gh?.repo ? 'ok' : 'muted'}" title={gh?.lastCommit ? `上次提交 ${gh.lastCommit}` : 'GitHub 未配置'}>
      <Icon name="github" size={12} />
      {#if gh?.lastSync}{shortTime(gh.lastSync)}{:else}{gh?.repo ? '未同步' : '未配置'}{/if}
    </span>
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
