<script lang="ts">
  import Icon from './Icons.svelte';
  import DatePicker from './DatePicker.svelte';
  import * as api from '../api';
  import type { Task, TaskPriority } from '../api';
  import { errText, selectTask, tasks, toast } from '../state.svelte';
  import {
    PRIORITY_LABEL,
    TASK_VIEWS,
    dueLabel,
    dueTone,
    leftLabel,
    nextUp,
    parseInline,
    pickUrgent,
    resolveSel,
    taskCounts,
    todayStr
  } from '../tasks';

  const PRIORITIES: TaskPriority[] = ['high', 'normal', 'low'];
  const today = todayStr();

  /**
   * 这一页现在只干两件事：**显示选中那条的详情** + **底部一排操作按钮**。
   * 列表整块搬到左栏了（Panel.svelte 的 task 分支），不要在这里再渲染一遍。
   */

  let busy = $state(false);
  /** 删除要二次确认 —— 左栏那个删除入口已经没了，这里是唯一出口 */
  let confirmDel = $state(false);

  /** 新建表单的草稿。只有 tasks.composing 为真时才用得上。 */
  let draft = $state({ title: '', due: '', priority: 'normal' as TaskPriority, note: '' });
  let titleInput = $state<HTMLInputElement | null>(null);

  // 点左栏 + 号进来就是新建态，输入框直接聚焦 —— 想到一句就记下来的场景
  $effect(() => {
    if (tasks.composing) titleInput?.focus();
  });

  /**
   * 当前要显示的那条。
   *
   * 两级：用户手选过就用他选的（resolveSel 负责在清单里重新定位）；
   * 没选过就**自动推荐最该做的那条** —— 这就是「按紧急度提示要完成的任务」。
   * 推荐的那条会挂一个小标记，跟用户自己选的区分开。
   */
  const picked = $derived.by(() => {
    const hit = resolveSel(tasks.items, tasks.selRaw, tasks.selHint, tasks.selTitle);
    if (hit) return { task: hit as Task | null, auto: false };
    return { task: pickUrgent(tasks.items, tasks.view, today), auto: true };
  });

  const shown = $derived(picked.task);
  const counts = $derived(taskCounts(tasks.items, today));
  const headLabel = $derived(TASK_VIEWS.find((v) => v.id === tasks.view)?.label ?? '任务');

  /** 详情页下半段的「接下来」：同组、当前这条之后的若干条。 */
  const upstream = $derived(nextUp(tasks.items, tasks.view, shown, 6, today));

  function rowKey(t: Task): string {
    return `${t.line}:${t.raw}`;
  }

  /** `2026-09-25` → `2026年9月25日 周五`。日期那一行要的是「绝对值」这个锚。 */
  function absDate(iso: string): string {
    const d = new Date(`${iso}T00:00:00`);
    if (Number.isNaN(d.getTime())) return iso;
    const w = '日一二三四五六'[d.getDay()];
    return `${d.getFullYear()}年${d.getMonth() + 1}月${d.getDate()}日 周${w}`;
  }

  const EMPTY: Record<string, string> = {
    short: '一周内没有要做的。',
    long: '没有更远的安排，也没漏排期的任务。',
    done: '还没有完成的任务。'
  };

  /** 统一的写入出口：拿后端回传的整份清单刷新本地，顺手用一个闸防止连点重复提交。 */
  async function guard(fn: () => Promise<Task[]>): Promise<boolean> {
    if (busy) return false;
    busy = true;
    try {
      tasks.items = await fn();
      return true;
    } catch (e) {
      toast(errText(e), 'error');
      return false;
    } finally {
      busy = false;
    }
  }

  /**
   * 写完盘之后，把选中凭据重新对准。
   *
   * - `next`：跳到下一条最紧急的 —— 连按「完成」能一路清下去
   * - `stay`：留在原地（改日期 / 改优先级用这个，视野被挪走会让人找不着）
   *
   * ⚠️ 改**标题**时必须走 `relabel`：`selTitle` 存的还是旧标题，
   * 而 `resolveSel` 的退化匹配正是拿标题去对的 —— 不更新就等于当场失联。
   * 所以改完标题要把新值回填进凭据，不能偷懒复用 `stay`。
   */
  async function afterWrite(mode: 'next' | 'stay' | { relabel: string } = 'next') {
    if (typeof mode === 'object') {
      // 标题变了：直接用新标题 + 原行号重建凭据，再让后端清单确认一次
      tasks.selTitle = mode.relabel;
      tasks.selRaw = '';
      const hit = resolveSel(tasks.items, '', tasks.selHint, mode.relabel);
      if (hit) selectTask(hit);
      return;
    }
    if (mode === 'stay') {
      const hit = resolveSel(tasks.items, tasks.selRaw, tasks.selHint, tasks.selTitle);
      if (hit) selectTask(hit);
      return;
    }
    selectTask(pickUrgent(tasks.items, tasks.view, today));
  }

  // ───────── 详情页的操作 ─────────

  async function toggleDone(t: Task) {
    if (await guard(() => api.updateTask(t.raw, t.line, { done: !t.done }))) {
      await afterWrite('next');
    }
  }

  async function patch(
    t: Task,
    p: { title?: string; due?: string; priority?: TaskPriority; note?: string }
  ) {
    if (await guard(() => api.updateTask(t.raw, t.line, p))) {
      // 改字段不该把视野挪走，否则你还想接着改下一项就找不到了。
      // 改标题要单独处理 —— 凭据里存着旧标题，得回填新的。
      await afterWrite(p.title !== undefined ? { relabel: p.title } : 'stay');
    }
  }

  async function doDelete(t: Task) {
    confirmDel = false;
    if (await guard(() => api.deleteTask(t.raw, t.line))) {
      toast('任务已删除', 'ok');
      await afterWrite('next');
    }
  }

  // ───────── 新建 ─────────

  function cancelCompose() {
    tasks.composing = false;
    draft = { title: '', due: '', priority: 'normal', note: '' };
  }

  /**
   * 提交新建。**建完直接选中刚建的那条** —— 用户刚写的东西就该是当前焦点，
   * 这样底部按钮马上能用，不用再去左栏里找一遍。
   */
  async function submitCompose() {
    const typed = draft.title.trim();
    if (!typed) return;
    const inline = parseInline(typed);
    const title = inline.title || typed;
    const ok = await guard(() =>
      api.addTask(title, {
        due: inline.due || draft.due,
        priority: inline.priority !== 'normal' ? inline.priority : draft.priority,
        note: draft.note
      })
    );
    if (!ok) return;
    // 从新清单里把刚建的那条捞出来。同名可能有多条，取**行号最大**的
    // （add_task 是往文件末尾追加的），基本不会认错。
    const same = tasks.items.filter((x) => x.title === title);
    selectTask(same.length ? same[same.length - 1] : null);
    tasks.composing = false;
    draft = { title: '', due: '', priority: 'normal', note: '' };
  }
</script>

<section class="tv">
  <header class="th">
    <div class="left">
      <h2>{tasks.composing ? '新建任务' : headLabel}</h2>
      {#if !tasks.composing}
        <span class="sub">
          {counts[tasks.view] ?? 0} 项{#if counts.overdue > 0 && tasks.view === 'short'}<b
              class="over">· 逾期 {counts.overdue}</b
            >{/if}
        </span>
      {/if}
    </div>
  </header>

  {#if tasks.composing}
    <!-- ───────── 新建态：整块就是一张表单 ───────── -->
    <div class="scroll page">
      <div class="form">
        <label class="fl">
          <span class="flab">任务</span>
          <!-- 新建时立刻要能打字，autofocus 是刻意的 -->
          <!-- svelte-ignore a11y_autofocus -->
          <input
            class="field big"
            bind:this={titleInput}
            bind:value={draft.title}
            autofocus
            spellcheck="false"
            placeholder="要做的事，比如：交材料 @2026-09-22 !高"
            onkeydown={(e) => {
              if (e.key === 'Enter') submitCompose();
              if (e.key === 'Escape') cancelCompose();
            }}
          />
        </label>

        <div class="frow">
          <div class="fl">
            <span class="flab">截止日</span>
            <DatePicker bind:value={draft.due} />
          </div>
          <div class="fl">
            <span class="flab">优先级</span>
            <div class="seg">
              {#each PRIORITIES as p (p)}
                <button
                  class:on={draft.priority === p}
                  title="{PRIORITY_LABEL[p]}优先级"
                  onclick={() => (draft.priority = p)}>{PRIORITY_LABEL[p]}</button
                >
              {/each}
            </div>
          </div>
        </div>

        <label class="fl">
          <span class="flab">备注</span>
          <input class="field" bind:value={draft.note} placeholder="可选，写在任务下面一行" />
        </label>
      </div>
    </div>

    <!-- 钉在底，不跟着表单滚 —— 跟详情态的按键条同一个位置关系 -->
    <div class="bar">
      <button class="btn" onclick={cancelCompose}>取消</button>
      <button class="btn primary" disabled={!draft.title.trim()} onclick={submitCompose}>
        创建任务
      </button>
    </div>
  {:else if !tasks.items.length}
    <div class="scroll page"><p class="empty">还没有任务。点左栏的 + 加一条。</p></div>
  {:else if !shown}
    <div class="scroll page"><p class="empty">{EMPTY[tasks.view] ?? '这个视图下是空的。'}</p></div>
  {:else}
    <!-- ───────── 详情态 ───────── -->
    <div class="scroll page">
      <div class="detail" class:done={shown.done}>
        {#if picked.auto}
          <!-- 自动推荐的必须标出来，不然用户会困惑「我什么时候选的这条」。
               一点就变成他自己的选择。 -->
          <button class="tag" title="按紧急度挑的。点一下固定下来" onclick={() => selectTask(shown)}>
            先做这条
          </button>
        {/if}

        <!-- 标题就地可改。用 div + role=textbox 而不是 contenteditable 的 h1：
             h1 是「非交互元素」，挂 textbox 角色会被 a11y 检查拦下，
             而视觉上仍按标题排（.tt 那套）。 -->
        <div
          class="tt"
          class:doney={shown.done}
          contenteditable="plaintext-only"
          spellcheck="false"
          role="textbox"
          tabindex="0"
          aria-label="任务标题"
          onblur={(e) => {
            const v = (e.currentTarget as HTMLElement).innerText.trim();
            if (v && v !== shown.title) void patch(shown, { title: v });
            else (e.currentTarget as HTMLElement).innerText = shown.title;
          }}
          onkeydown={(e) => {
            if (e.key === 'Enter') {
              e.preventDefault();
              (e.currentTarget as HTMLElement).blur();
            }
            if (e.key === 'Escape') {
              (e.currentTarget as HTMLElement).innerText = shown.title;
              (e.currentTarget as HTMLElement).blur();
            }
          }}
        >{shown.title}</div>

        <!--
          这里以前是「一排只读 pill（日期/优先级/进行中）+ 下面一套可编辑控件」，
          同一件事说两遍。现已合并成**一份可改的**：

          - 「进行中」pill 删了：一条没完成的任务必然是进行中；完成了它就进「已完成」组，
            分组头已经把这个信息说完了，那个 pill 恒真、也恒无用。
          - 日期放大成「还剩 N 天」：人读相对时间比读绝对值有用，而且这行大字
            正好把原本空着的横向空间用上了。

          ⚠️ 已完成的那条**不能说「还剩 N 天」** —— 对它来说「还剩」已经没意义了。
          磁盘上的行语法里**没有完成时间**这个字段，所以这里不编一个出来，
          只把原定截止日当历史交代，并明确标出已完成。
        -->
        {#if shown.done}
          <div class="when done">
            <span class="big donebig">已完成</span>
            {#if shown.due}
              <span class="abs">原定 {absDate(shown.due)}</span>
            {/if}
          </div>
        {:else}
          <div class="when" class:over={shown.due && shown.due < today}>
            {#if shown.due}
              <span class="big">{leftLabel(shown.due, today)}</span>
              <span class="abs">{absDate(shown.due)}</span>
            {:else}
              <span class="big mute">未排期</span>
            {/if}
          </div>
        {/if}

        <div class="edit">
          <div class="eitem">
            <DatePicker
              value={shown.due}
              onpick={(v: string) => {
                if (v !== shown.due) void patch(shown, { due: v });
              }}
            />
          </div>
          <div class="eitem">
            <div class="seg">
              {#each PRIORITIES as p (p)}
                <button
                  class:on={shown.priority === p}
                  title="{PRIORITY_LABEL[p]}优先级"
                  onclick={() => {
                    if (p !== shown.priority) void patch(shown, { priority: p });
                  }}>{PRIORITY_LABEL[p]}</button
                >
              {/each}
            </div>
          </div>
        </div>

        <!-- 备注只留这一处（可改）。以前上面还有一块只读展示，同样是说两遍。 -->
        <input
          class="field note"
          value={shown.note}
          placeholder="备注（可选）"
          onkeydown={(e) => {
            if (e.key === 'Enter') {
              const v = (e.currentTarget as HTMLInputElement).value.trim();
              if (v !== shown.note) void patch(shown, { note: v });
              (e.currentTarget as HTMLInputElement).blur();
            }
          }}
          onblur={(e) => {
            const v = (e.currentTarget as HTMLInputElement).value.trim();
            if (v !== shown.note) void patch(shown, { note: v });
          }}
        />

        <!-- 下半段的「厚度」：同组接下来还有什么。数据就是 tasksInView 的输出，
             没有第二套口径。已完成视图里没有「接下来」，nextUp 会返回空。 -->
        {#if upstream.length}
          <div class="up">
            <div class="uphead">接下来</div>
            {#each upstream as t (rowKey(t))}
              <button class="uprow" onclick={() => selectTask(t)} title={t.title}>
                <span class="upname">{t.title}</span>
                {#if t.due}
                  <span class="update {dueTone(t, today)}">{dueLabel(t.due, today)}</span>
                {/if}
              </button>
            {/each}
          </div>
        {/if}
      </div>
    </div>

    <!-- 底部只有两个按钮，并排贴右下。原来是「完成/今天/明天/删除」四个，
         今天·明天属于重复（上面日期控件就在手边），已删。 -->
    <div class="bar">
      {#if confirmDel}
        <span class="cwarn">删掉「{shown.title}」？</span>
        <button class="btn danger" onclick={() => doDelete(shown)}>删除</button>
        <button class="btn" onclick={() => (confirmDel = false)}>取消</button>
      {:else}
        <button class="btn primary" onclick={() => toggleDone(shown)}>
          {#if shown.done}取消完成{:else}完成{/if}
        </button>
        <button class="btn danger" onclick={() => (confirmDel = true)}>
          <Icon name="trash" size={13} />
          删除
        </button>
      {/if}
    </div>
  {/if}
</section>

<style>
  .tv {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
    min-width: 0;
  }

  .th {
    display: flex;
    align-items: center;
    gap: 12px;
    height: 44px;
    flex: 0 0 44px;
    padding: 0 18px;
    border-bottom: 1px solid var(--line);
  }
  .th .left {
    display: flex;
    align-items: baseline;
    gap: 10px;
    min-width: 0;
  }
  .th h2 {
    font-size: 14px;
    font-weight: 500;
    color: var(--fg);
    white-space: nowrap;
  }
  .th .sub {
    font-size: 11.5px;
    color: var(--fg-faint);
    white-space: nowrap;
  }
  .th .sub .over {
    color: var(--danger);
    font-weight: 500;
  }

  /* 滚动容器叫 .page，正文区叫 .body —— 两条同名规则会互相串味 */
  .page {
    flex: 1;
    min-height: 0;
    padding: 22px 22px 30px;
  }

  /* ── 详情 ── */
  .detail {
    max-width: 640px;
  }

  /* 自动推荐的小标记。要显眼但不要抢戏 —— 它只是个提示，不是状态。 */
  .tag {
    display: inline-flex;
    align-items: center;
    height: 20px;
    padding: 0 8px;
    margin-bottom: 10px;
    border-radius: 6px;
    background: var(--accent-soft);
    border: 1px solid var(--accent-line);
    color: var(--accent);
    font-size: 11px;
  }
  .tag:hover {
    background: var(--accent);
    color: var(--on-accent);
  }

  /* 标题直接可改（contenteditable）。所以它得看起来像标题、不像输入框。 */
  .tt {
    display: block;
    font-size: 21px;
    font-weight: 500;
    line-height: 1.4;
    color: var(--fg);
    outline: none;
    border-radius: 6px;
    padding: 2px 4px;
    margin: 0 -4px 10px;
    word-break: break-word;
  }
  .tt:hover {
    background: var(--hover);
  }
  .tt:focus {
    background: var(--inset);
    box-shadow: inset 0 0 0 1px var(--accent-line);
  }
  .tt.doney {
    text-decoration: line-through;
    color: var(--fg-mute);
  }

  /* 时间感那一行。以前这里是一排只读 pill（日期/优先级/进行中），
     现在只留「还剩几天」这条大字 + 绝对日期这行小字。 */
  .when {
    display: flex;
    align-items: baseline;
    gap: 12px;
    flex-wrap: wrap;
    margin-bottom: 18px;
    padding-bottom: 16px;
    border-bottom: 1px solid var(--line);
  }
  .when .big {
    font-size: 26px;
    font-weight: 500;
    line-height: 1.15;
    color: var(--accent);
    letter-spacing: -0.4px;
  }
  .when .big.mute {
    color: var(--fg-faint);
    font-size: 20px;
  }
  /* 逾期是唯一要喊出来的状态 */
  .when.over .big {
    color: var(--danger);
  }
  /* 已完成：整行安静下来。「已完成」不是强调，是收尾。 */
  .when.done .big.donebig {
    color: var(--fg-mute);
    font-size: 20px;
  }
  .when .abs {
    font-size: 12.5px;
    color: var(--fg-faint);
    font-family: var(--font-mono);
  }

  /* ── 编辑区：日期 + 优先级一行，备注下一行 ── */
  .edit {
    display: flex;
    flex-wrap: wrap;
    gap: 12px;
    --dp-h: 30px;
  }
  .eitem {
    display: flex;
    flex-direction: column;
    gap: 6px;
    flex: 0 0 auto;
  }

  .seg {
    display: flex;
    padding: 2px;
    border-radius: 8px;
    background: var(--inset);
  }
  .seg button {
    height: 26px;
    padding: 0 14px;
    border-radius: 6px;
    font-size: 12px;
    color: var(--fg-mute);
  }
  .seg button:hover {
    color: var(--fg);
  }
  .seg button.on {
    background: var(--accent-soft);
    color: var(--accent);
  }

  .field.note {
    display: block;
    width: 100%;
    height: 32px;
    margin-top: 14px;
    font-size: 12.5px;
  }

  /* ── 「接下来」那段 ──
     它的作用不是清单（清单在左栏），是给详情页下半部分一点厚度，
     所以行要矮、字要小，不能抢当前这条的戏。 */
  .up {
    margin-top: 26px;
    padding-top: 16px;
    border-top: 1px solid var(--line);
  }
  .uphead {
    font-size: 11px;
    color: var(--fg-faint);
    margin-bottom: 6px;
    letter-spacing: 0.4px;
  }
  .uprow {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    padding: 6px 8px;
    margin-left: -8px;
    border-radius: 7px;
    text-align: left;
    transition: background 0.1s;
  }
  .uprow:hover {
    background: var(--hover);
  }
  .upname {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 12.5px;
    color: var(--fg-dim);
  }
  .update {
    flex: 0 0 auto;
    font-size: 11px;
    font-family: var(--font-mono);
    color: var(--fg-faint);
  }
  .update.soon {
    color: var(--accent);
  }
  .update.over {
    color: var(--danger);
  }

  /* ── 新建表单 ──
     详情页的编辑项不挂标签（控件自解释），但新建表单里字段是空白的，
     需要标签说清这一格填什么。 */
  .form {
    display: flex;
    flex-direction: column;
    gap: 16px;
    max-width: 560px;
  }
  .fl {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .flab {
    font-size: 11px;
    color: var(--fg-faint);
  }
  .field.big {
    height: 38px;
    font-size: 14.5px;
  }
  .frow {
    display: flex;
    gap: 20px;
    flex-wrap: wrap;
    --dp-h: 30px;
  }

  /* ── 底部按键条 ──
     只有两个按钮，一起贴右下。上面用 border 跟内容分开，钉在底不跟着滚。 */
  .bar {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 8px;
    flex: 0 0 auto;
    padding: 12px 18px;
    border-top: 1px solid var(--line);
    background: var(--panel);
  }
  .bar .cwarn {
    flex: 1;
    font-size: 12.5px;
    color: var(--fg-dim);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    text-align: left;
  }
  /* 新建态那两个按钮（取消 / 创建）也跟着贴右，保持一致 */
  .bar.keep {
    justify-content: flex-end;
  }

  .empty {
    padding: 40px 10px;
    text-align: center;
    font-size: 13px;
    color: var(--fg-faint);
    line-height: 1.8;
  }
</style>
