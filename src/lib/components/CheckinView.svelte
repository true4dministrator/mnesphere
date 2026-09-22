<script lang="ts">
  import Icon from './Icons.svelte';
  import * as api from '../api';
  import type { CheckState } from '../api';
  import { checkin, errText, openCtxMenu, refreshCheckin, toast } from '../state.svelte';

  let prompt = $state<null | { label: string; value: string; ok: (v: string) => void }>(null);

  /** 当前选中的日期。默认落在「最近一个可以打卡的日子」上。 */
  let sel = $state('');
  /** 刚写入的那个格子要闪一下，靠 token 重新挂载让它重播动画 */
  let pulseDate = $state('');
  let pulseToken = $state(0);
  /** 涟漪按状态着色。以前写死成强调色，于是「没做到」的红格子上会蹦出一圈青色的光。 */
  let pulseKind = $state<'done' | 'missed' | 'clear'>('done');
  let bumpKey = $state(0);
  let flash = $state('');
  let flashTimer: ReturnType<typeof setTimeout> | null = null;

  const view = $derived(checkin.view);
  const focus = $derived(checkin.focus);
  const single = $derived(focus !== 'all');
  const today = new Date().toLocaleDateString('sv-SE');

  const wd = ['一', '二', '三', '四', '五', '六', '日'];
  const stats = $derived(single ? checkin.stats : null);

  const selCell = $derived(view?.days.find((d) => d.date === sel) ?? null);
  const curState = $derived<CheckState>(single && selCell ? selCell.states[focus] ?? '' : '');

  const selMeta = $derived.by(() => {
    if (!sel) return { week: '', rel: '' };
    const d = new Date(`${sel}T00:00:00`);
    const week = `周${'日一二三四五六'[d.getDay()]}`;
    const t = new Date(`${today}T00:00:00`);
    const diff = Math.round((t.getTime() - d.getTime()) / 86400000);
    const rel = diff === 0 ? '今天' : diff === 1 ? '昨天' : diff > 1 ? `${diff} 天前` : `${-diff} 天后`;
    return { week, rel };
  });

  const canJumpToday = $derived(sel !== today && !!view?.days.some((d) => d.date === today && !d.future));

  const aggregate = $derived.by(() => {
    const v = view;
    if (!v) return { done: 0, missed: 0, none: 0, rate: 0, todayDone: 0, todayTotal: 0 };
    let done = 0;
    let missed = 0;
    let none = 0;
    for (const c of Object.values(v.summary)) {
      done += c.done;
      missed += c.missed;
      none += c.none;
    }
    const cell = v.days.find((d) => d.date === today);
    let td = 0;
    let tt = 0;
    if (cell) {
      for (const s of Object.values(cell.states)) {
        tt += 1;
        if (s === 'done') td += 1;
      }
    }
    return {
      done,
      missed,
      none,
      rate: done + missed > 0 ? done / (done + missed) : 0,
      todayDone: td,
      todayTotal: tt
    };
  });

  // 月历前导空格：周一为一周之始
  const lead = $derived(view?.days.length ? view.days[0].weekday : 0);

  /** 没有「今天」可打的时候（翻到过去的月份），退到该月最后一个非未来日。 */
  function latestCheckable(v = view): string {
    const days = v?.days ?? [];
    const t = days.find((d) => d.date === today && !d.future);
    if (t) return t.date;
    const past = days.filter((d) => !d.future);
    return past.length ? past[past.length - 1].date : '';
  }

  // 月份一换，或者选中的日子落到未来去了，就把选中项挪到该月最新可打卡日
  $effect(() => {
    const v = checkin.view;
    if (!v) return;
    if (!v.days.some((d) => d.date === sel && !d.future)) {
      sel = latestCheckable(v);
    }
  });

  function say(text: string) {
    flash = text;
    if (flashTimer) clearTimeout(flashTimer);
    flashTimer = setTimeout(() => (flash = ''), 2400);
  }

  async function go(delta: number) {
    if (!checkin.month) return;
    const [y, m] = checkin.month.split('-').map(Number);
    const d = new Date(y, m - 1 + delta, 1);
    checkin.month = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}`;
    checkin.view = await api.getMonth(checkin.month);
  }

  function currentMonth() {
    const d = new Date();
    return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}`;
  }

  async function pickHabit(name: string) {
    checkin.focus = name;
    checkin.stats = name === 'all' ? null : await api.habitStats(name);
  }

  async function refreshView() {
    checkin.view = await api.getMonth(checkin.month);
    checkin.overview = await api.allHabitsOverview();
    if (checkin.focus !== 'all') checkin.stats = await api.habitStats(checkin.focus);
  }

  /**
   * 唯一一处写入口。日期靠点选、状态靠按钮，绝不在格子上循环切换 ——
   * 之前那种「点一下就翻一个状态」的操作，误触率高得离谱。
   */
  async function mark(habit: string, value: CheckState) {
    if (!sel) return;
    try {
      await api.setCheckin(habit, sel, value);
      pulseDate = sel;
      pulseKind = value === 'missed' ? 'missed' : value === 'done' ? 'done' : 'clear';
      pulseToken += 1;
      bumpKey += 1;
      await refreshView();
      const label = value === 'done' ? '做到了' : value === 'missed' ? '没做到' : '清成未打卡';
      say(`${sel} · ${habit} · ${label}`);
    } catch (e) {
      toast(errText(e), 'error');
    }
  }

  function selectDate(d: { date: string; future: boolean }) {
    if (d.future) return;
    sel = d.date;
    flash = '';
  }

  function cellClass(date: string) {
    if (!single || !view) return '';
    const cell = view.days.find((d) => d.date === date);
    if (!cell) return '';
    if (cell.future) return 'future';
    return cell.states[focus] || 'none';
  }

  // ───────── 目标管理（右键/更多菜单统一走全局自绘那套） ─────────

  function openHabitMenu(e: MouseEvent) {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    openCtxMenu(r.left - 96, r.bottom + 4, [
      { label: '重命名', icon: 'edit', run: renameCurrent },
      { label: '归档', icon: 'grid', run: archiveCurrent },
      { sep: true },
      { label: '删除目标', icon: 'trash', danger: true, run: deleteCurrent }
    ]);
  }

  function renameCurrent() {
    const name = focus;
    prompt = {
      label: '重命名目标',
      value: name,
      ok: async (v) => {
        try {
          await api.updateHabit(name, { name: v });
          await pickHabit(v);
          await refreshCheckin();
          toast('已重命名，历史表格的列名也一并改好了', 'ok');
        } catch (e) {
          toast(errText(e), 'error');
        }
      }
    };
  }

  async function archiveCurrent() {
    try {
      await api.updateHabit(focus, { archived: true });
      checkin.focus = 'all';
      checkin.stats = null;
      await refreshCheckin();
      toast('已归档（历史数据保留，不再出现在月历里）', 'ok');
    } catch (e) {
      toast(errText(e), 'error');
    }
  }

  async function deleteCurrent() {
    try {
      await api.deleteHabit(focus);
      checkin.focus = 'all';
      checkin.stats = null;
      await refreshCheckin();
      toast('目标已删除', 'ok');
    } catch (e) {
      toast(errText(e), 'error');
    }
  }

  async function newHabit() {
    prompt = {
      label: '新建目标',
      value: '',
      ok: async (v) => {
        try {
          await api.addHabit(v);
          await refreshCheckin();
        } catch (e) {
          toast(errText(e), 'error');
        }
      }
    };
  }
</script>

<section class="cv">
  <header class="ch">
    <div class="left">
      <h2>{single ? focus : '全部目标'}</h2>
      {#if single}
        <button class="chip" onclick={() => pickHabit('all')}>← 回到总览</button>
        <button class="ico" title="更多" onclick={openHabitMenu}><Icon name="grid" size={14} /></button>
      {:else}
        <button class="chip" onclick={newHabit}><Icon name="plus" size={12} /> 新建目标</button>
      {/if}
    </div>

    <div class="nav">
      <button class="ico" title="上个月" onclick={() => go(-1)}><Icon name="chev-l" size={14} /></button>
      <span class="mono">{view?.label ?? ''}</span>
      <button class="ico" title="下个月" onclick={() => go(1)}><Icon name="chev-r" size={14} /></button>
      {#if checkin.month !== currentMonth()}
        <button class="chip" onclick={() => { checkin.month = currentMonth(); go(0); }}>回到本月</button>
      {/if}
    </div>
  </header>

  <div class="scroll body">
    <div class="kpis">
      {#if single}
        <div class="kpi">
          <span class="kl">当前连续</span>
          <b>{stats?.currentStreak ?? 0} <small>天</small></b>
          <span class="kn">未打卡不打断，连续 3 天无记录才算断</span>
        </div>
        <div class="kpi">
          <span class="kl">本月完成</span>
          {#key bumpKey}
            <b class:bump={bumpKey > 0}
              >{view?.summary[focus]?.done ?? 0}<small
                > / {(view?.summary[focus]?.done ?? 0) + (view?.summary[focus]?.missed ?? 0)}</small
              ></b
            >
          {/key}
          <span class="kn">未打卡不计入分母</span>
        </div>
        <div class="kpi">
          <span class="kl">完成率</span>
          <b>{Math.round((view?.summary[focus]?.rate ?? 0) * 100)}<small>%</small></b>
          <span class="kn">最长连续 {stats?.longestStreak ?? 0} 天</span>
        </div>
      {:else}
        <div class="kpi">
          <span class="kl">今日完成</span>
          {#key bumpKey}
            <b class:bump={bumpKey > 0}>{aggregate.todayDone}<small> / {aggregate.todayTotal}</small></b>
          {/key}
          <span class="kn">{today}</span>
        </div>
        <div class="kpi">
          <span class="kl">本月完成</span>
          <b>{aggregate.done}<small> 次</small></b>
          <span class="kn">漏打 {aggregate.missed} 次 · 未记录 {aggregate.none} 次</span>
        </div>
        <div class="kpi">
          <span class="kl">本月完成率</span>
          <b>{Math.round(aggregate.rate * 100)}<small>%</small></b>
          <span class="kn">口径：做到 /（做到 + 没做到）</span>
        </div>
      {/if}
    </div>

    <div class="workzone">
      <div class="gridwrap">
        <div class="weekhead">
          {#each wd as w (w)}<span>{w}</span>{/each}
        </div>
        <div class="grid">
          {#each Array(lead) as _, i (i)}<div class="pad"></div>{/each}
          {#each view?.days ?? [] as d (d.date)}
            <button
              class="cell {cellClass(d.date)}"
              class:today={d.today}
              class:sel={sel === d.date}
              disabled={d.future}
              onclick={() => selectDate(d)}
              title={d.future ? `${d.date}（还没到）` : `选中 ${d.date}`}
            >
              <span class="num">{d.day}</span>
              {#if single && view && !d.future && d.states[focus]}
                <!-- 勾/叉是「一眼可辨」的主力：底色和描边只是辅助，
                     隔着一屏扫过去，符号才是能立刻读懂的那一层。
                     用 key 包住是为了让状态一变就重播一次弹入动画。 -->
                {#key d.states[focus]}
                  <span class="mark {d.states[focus]}"
                    >{d.states[focus] === 'done' ? '✓' : '✗'}</span
                  >
                {/key}
              {/if}
              {#if !single && view}
                <span class="dots">
                  {#each view.habits as h (h.name)}
                    <i class={d.future ? 'f' : d.states[h.name] || 'n'}></i>
                  {/each}
                </span>
              {/if}
              {#if pulseDate === d.date}
                {#key pulseToken}<span class="ripple {pulseKind}"></span>{/key}
              {/if}
            </button>
          {/each}
        </div>

        <div class="legend">
          <span><i class="sw done">✓</i>做到了</span>
          <span><i class="sw missed">✗</i>没做到</span>
          <span><i class="sw none"></i>未打卡</span>
          <span class="tip">{single ? '点日期选中，右侧按钮落状态' : '点日期选中后，逐个目标落状态'}</span>
        </div>
      </div>

      <aside class="sbar">
        <div class="shead">
          <div>
            <span class="sdate mono">{sel || '—'}</span>
            <span class="sweek">{selMeta.week}{selMeta.rel ? ` · ${selMeta.rel}` : ''}</span>
          </div>
          {#if canJumpToday}
            <button class="chip" onclick={() => (sel = today)}>跳到今天</button>
          {/if}
        </div>

        {#if !sel}
          <p class="empty2">这个月还没有可以打卡的日子。</p>
        {:else if single}
          <div class="acts">
            <button
              class="ab done"
              class:on={curState === 'done'}
              onclick={() => mark(focus, 'done')}
            >
              <Icon name="check" size={16} stroke={2.2} /> 做到了
            </button>
            <button
              class="ab missed"
              class:on={curState === 'missed'}
              onclick={() => mark(focus, 'missed')}
            >
              <Icon name="close" size={15} stroke={2.2} /> 没做到
            </button>
            <button class="ab clr" disabled={!curState} onclick={() => mark(focus, '')}>
              <Icon name="minus" size={15} /> 清除
            </button>
          </div>
          <p class="note2">「没做到」是明确表态，会打断连续；什么都不点则是「未打卡」，不计入分母。</p>
        {:else}
          <div class="hablist">
            {#each view?.habits ?? [] as h (h.name)}
              {@const st = selCell?.states[h.name] ?? ''}
              <div class="hrow2">
                <span class="hn2">{h.name}</span>
                <div class="tri">
                  <button
                    class="t done"
                    class:on={st === 'done'}
                    title="做到了"
                    onclick={() => mark(h.name, 'done')}
                  >
                    <Icon name="check" size={14} stroke={2.2} />
                  </button>
                  <button
                    class="t missed"
                    class:on={st === 'missed'}
                    title="没做到"
                    onclick={() => mark(h.name, 'missed')}
                  >
                    <Icon name="close" size={13} stroke={2.2} />
                  </button>
                  <button
                    class="t clr"
                    class:on={st === ''}
                    title="未打卡"
                    onclick={() => mark(h.name, '')}
                  >
                    <Icon name="minus" size={13} />
                  </button>
                </div>
              </div>
            {/each}
            {#if !(view?.habits ?? []).length}
              <p class="empty2">还没有目标。左上角「新建目标」开始。</p>
            {/if}
          </div>
        {/if}

        {#if flash}
          <div class="flash">{flash}</div>
        {/if}
      </aside>
    </div>

    {#if single && stats}
      <div class="panel2">
        <div class="p2l">
          <h3>近 12 周完成率</h3>
          <div class="bars">
            {#each stats.weeks as w, i (w.label)}
              <div class="barcol" title={`${w.label} 起那一周：${w.done}/${w.total}`}>
                <div class="bartrack">
                  <div
                    class="barfill"
                    class:last={i === stats.weeks.length - 1}
                    style="height:{Math.round(w.rate * 100)}%"
                  ></div>
                </div>
                <span class="blab">{w.done}</span>
              </div>
            {/each}
          </div>
        </div>
        <div class="p2r">
          <h3>累计</h3>
          <ul>
            <li><span>做到</span><b>{stats.done} 天</b></li>
            <li><span>没做到</span><b>{stats.missed} 天</b></li>
            <li><span>未打卡</span><b>{stats.none} 天</b></li>
            <li><span>最长连续</span><b>{stats.longestStreak} 天</b></li>
            <li><span>起始</span><b class="mono">{stats.firstDate || '—'}</b></li>
          </ul>
        </div>
      </div>
    {/if}

    {#if !single}
      <div class="habits">
        <h3>各目标状态</h3>
        {#each view?.habits ?? [] as h (h.name)}
          {@const st = checkin.overview[h.name]}
          <button class="hrow" onclick={() => pickHabit(h.name)}>
            <span class="hn">{h.name}</span>
            <span class="hs">
              <b>{st ? st.currentStreak : 0}</b><small>天连续</small>
            </span>
            <span class="hr">
              <span class="track">
                <span class="fill" style="width:{Math.round((st?.rate ?? 0) * 100)}%"></span>
              </span>
              <small>{Math.round((st?.rate ?? 0) * 100)}%</small>
            </span>
          </button>
        {/each}
        {#if !(view?.habits ?? []).length}
          <p class="empty">还没有目标。点右上角「新建目标」开始。</p>
        {/if}
      </div>
    {/if}
  </div>
</section>

{#if prompt}
  <div class="pb" role="presentation" onclick={() => (prompt = null)}></div>
  <div class="modal">
    <p>{prompt.label}</p>
    <!-- 模态框只有这一个输入项，autofocus 是刻意的 -->
    <!-- svelte-ignore a11y_autofocus -->
    <input
      class="field"
      autofocus
      bind:value={prompt.value}
      onkeydown={(e) => {
        if (e.key === 'Enter') {
          const v = prompt!.value.trim();
          if (v) prompt!.ok(v);
          prompt = null;
        }
        if (e.key === 'Escape') prompt = null;
      }}
    />
    <div class="mbtns">
      <button class="btn sm" onclick={() => (prompt = null)}>取消</button>
      <button
        class="btn sm primary"
        onclick={() => {
          const v = prompt!.value.trim();
          if (v) prompt!.ok(v);
          prompt = null;
        }}>确定</button
      >
    </div>
  </div>
{/if}

<style>
  .cv {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
    min-width: 0;
  }

  .ch {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    height: 44px;
    flex: 0 0 44px;
    padding: 0 18px;
    border-bottom: 1px solid var(--line);
  }
  .ch .left {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
  }
  .ch h2 {
    font-size: 14px;
    font-weight: 500;
    color: var(--fg);
    white-space: nowrap;
  }
  .ch .chip {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    height: 22px;
    padding: 0 10px;
    border-radius: 999px;
    background: var(--card);
    color: var(--fg-mute);
    font-size: 11.5px;
    white-space: nowrap;
  }
  .ch .chip:hover {
    background: var(--accent-soft);
    color: var(--accent);
  }

  .nav {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12.5px;
    color: var(--fg-dim);
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

  .body {
    flex: 1;
    min-height: 0;
    padding: 16px 18px 30px;
  }

  .kpis {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: 10px;
    margin-bottom: 18px;
  }
  .kpi {
    display: flex;
    flex-direction: column;
    gap: 3px;
    padding: 12px 14px;
    border-radius: calc(var(--radius) * 0.8);
    background: var(--card);
    border: 1px solid var(--line);
  }
  .kl {
    font-size: 11px;
    color: var(--fg-mute);
  }
  .kpi b {
    font-size: 22px;
    font-weight: 500;
    color: var(--fg);
    line-height: 1.25;
  }
  .kpi b small {
    font-size: 12px;
    color: var(--fg-mute);
    font-weight: 400;
  }
  .kn {
    font-size: 10.5px;
    color: var(--fg-faint);
    line-height: 1.5;
  }
  .kpi b.bump {
    animation: bump 0.5s cubic-bezier(0.34, 1.56, 0.64, 1);
  }
  @keyframes bump {
    0% {
      transform: scale(1);
    }
    38% {
      transform: scale(1.24);
      color: var(--accent);
    }
    100% {
      transform: scale(1);
    }
  }

  /* 月历在左、动作区在右。格子因此不必再撑满整个宽度，尺寸自然收下来了。 */
  .workzone {
    display: grid;
    grid-template-columns: minmax(0, 1.1fr) minmax(0, 1fr);
    gap: 20px;
    align-items: start;
    margin-bottom: 20px;
  }
  @media (max-width: 1180px) {
    .workzone {
      grid-template-columns: minmax(0, 1fr);
      gap: 14px;
    }
  }

  .gridwrap {
    max-width: 540px;
  }
  .weekhead {
    display: grid;
    grid-template-columns: repeat(7, minmax(0, 1fr));
    gap: 5px;
    margin-bottom: 5px;
  }
  .weekhead span {
    text-align: center;
    font-size: 10.5px;
    color: var(--fg-faint);
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(7, minmax(0, 1fr));
    gap: 5px;
  }
  .pad {
    aspect-ratio: 1.5;
  }

  .cell {
    position: relative;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    justify-content: space-between;
    aspect-ratio: 1.5;
    padding: 4px 5px;
    border-radius: 7px;
    border: 1px solid var(--line);
    background: var(--card);
    color: var(--fg-dim);
    /* 这里绝不能加 overflow: hidden —— 涟漪是靠 box-shadow 向外扩散的，
       裁掉就等于把「打卡成功」的视觉反馈整个抹掉。 */
    transition: transform 0.1s, border-color 0.12s, background 0.12s;
  }
  .cell:hover:not(:disabled) {
    border-color: var(--accent-line);
  }
  .cell .num {
    font-size: 11px;
    font-family: var(--font-mono);
    color: var(--fg-mute);
    line-height: 1.2;
  }
  /* 单目标视图里的勾 / 叉。压在右下角，不跟日期抢左上角的位置。 */
  .cell .mark {
    position: absolute;
    right: 5px;
    bottom: 3px;
    font-size: 15px;
    font-weight: 700;
    line-height: 1;
    animation: markin 0.36s cubic-bezier(0.34, 1.56, 0.64, 1);
  }
  .cell .mark.done {
    color: var(--accent);
    text-shadow: 0 0 10px var(--accent-line);
  }
  .cell .mark.missed {
    color: var(--danger);
    text-shadow: 0 0 10px var(--danger-soft);
  }
  @keyframes markin {
    0% {
      transform: scale(0.3) rotate(-16deg);
      opacity: 0;
    }
    62% {
      transform: scale(1.22) rotate(4deg);
      opacity: 1;
    }
    100% {
      transform: scale(1) rotate(0);
    }
  }

  /* 成败态：底色 + 描边 + 右侧一根竖条，三层叠加。
     只靠一层极淡的底色，在深色主题里离远看几乎分不出来。
     竖条必须用 ::before 而不是 box-shadow —— .cell.sel 整条 box-shadow 会把它盖掉，
     而「刚打完卡那一格」恰恰十有八九正处于选中态。 */
  .cell.done {
    background: var(--accent-soft);
    border-color: var(--accent-line);
  }
  .cell.done .num {
    color: var(--accent);
  }
  .cell.missed {
    background: var(--danger-soft);
    border-color: var(--danger);
  }
  .cell.missed .num {
    color: var(--danger);
  }
  .cell.done::before,
  .cell.missed::before {
    content: '';
    position: absolute;
    right: 0;
    top: 0;
    bottom: 0;
    width: 3px;
    border-radius: 0 6px 6px 0;
    background: var(--accent);
  }
  .cell.missed::before {
    background: var(--danger);
  }
  .cell.none {
    background: var(--card);
  }
  .cell.future {
    background: transparent;
    border-style: dashed;
    border-color: var(--line);
    opacity: 0.45;
    cursor: default;
  }
  .cell.today {
    border-color: var(--accent-2);
  }
  .cell.today .num {
    color: var(--accent-2);
    font-weight: 600;
  }
  /* 选中态压过其他一切描边，否则点在 done 格子上根本看不出选没选中 */
  .cell.sel {
    border-color: var(--accent);
    box-shadow: 0 0 0 2px var(--accent-soft);
  }
  .cell.sel:hover:not(:disabled) {
    border-color: var(--accent);
  }

  .ripple {
    position: absolute;
    inset: 0;
    border-radius: inherit;
    pointer-events: none;
    animation: pulse 0.62s ease-out;
  }
  .ripple.done {
    --rc: var(--accent);
  }
  .ripple.missed {
    --rc: var(--danger);
  }
  .ripple.clear {
    --rc: var(--fg-mute);
  }
  /* 颜色走 --rc 这个自定义属性，而不是写死强调色 ——
     否则在「没做到」的红格子上会炸出一圈青色的涟漪，状态和反馈互相打架。 */
  @keyframes pulse {
    0% {
      box-shadow: 0 0 0 0 var(--rc, var(--accent));
      opacity: 0.9;
    }
    100% {
      box-shadow: 0 0 0 14px transparent;
      opacity: 0;
    }
  }

  .dots {
    display: flex;
    flex-wrap: wrap;
    gap: 2px;
  }
  .dots i {
    width: 4px;
    height: 4px;
    border-radius: 1.5px;
    background: var(--line-2);
  }
  .dots i.done {
    background: var(--accent);
  }
  .dots i.missed {
    background: var(--danger);
  }
  .dots i.n {
    background: var(--fg-faint);
    opacity: 0.45;
  }
  .dots i.f {
    background: transparent;
    border: 1px solid var(--line-2);
  }

  .legend {
    display: flex;
    align-items: center;
    gap: 12px;
    flex-wrap: wrap;
    padding: 8px 2px 0;
    font-size: 11px;
    color: var(--fg-mute);
  }
  .legend span {
    display: inline-flex;
    align-items: center;
    gap: 5px;
  }
  .sw {
    display: grid;
    place-items: center;
    width: 14px;
    height: 14px;
    border-radius: 4px;
    border: 1px solid var(--line);
    /* 图例里直接印勾 / 叉，跟格子上的符号对上号 —— 光靠颜色分类，
       对这个主题里那两层很淡的底色不够用。 */
    font-size: 10px;
    font-style: normal;
    font-weight: 700;
    line-height: 1;
  }
  .sw.done {
    background: var(--accent-soft);
    border-color: var(--accent-line);
    color: var(--accent);
  }
  .sw.missed {
    background: var(--danger-soft);
    border-color: var(--danger);
    color: var(--danger);
  }
  .sw.none {
    background: var(--card);
  }
  .legend .tip {
    color: var(--fg-faint);
  }

  /* ── 选中日期的动作区 ── */
  .sbar {
    padding: 12px 14px 14px;
    border-radius: calc(var(--radius) * 0.9);
    background: var(--card);
    border: 1px solid var(--line);
    position: sticky;
    top: 0;
  }
  .shead {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    padding-bottom: 10px;
    margin-bottom: 12px;
    border-bottom: 1px solid var(--line);
  }
  .sdate {
    display: block;
    font-size: 13.5px;
    color: var(--fg);
    letter-spacing: 0.2px;
  }
  .sweek {
    display: block;
    margin-top: 2px;
    font-size: 11px;
    color: var(--fg-mute);
  }

  .acts {
    display: flex;
    flex-direction: column;
    gap: 7px;
  }
  .ab {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 9px 12px;
    border-radius: 8px;
    border: 1px solid var(--line);
    background: var(--panel-2);
    color: var(--fg-dim);
    font-size: 12.5px;
    text-align: left;
    transition: background 0.12s, border-color 0.12s, color 0.12s, transform 0.1s;
  }
  .ab:hover:not(:disabled) {
    border-color: var(--accent-line);
    color: var(--fg);
  }
  .ab:active:not(:disabled) {
    transform: scale(0.985);
  }
  .ab.done.on {
    background: var(--accent-soft);
    border-color: var(--accent);
    color: var(--accent);
  }
  .ab.missed.on {
    background: var(--danger-soft);
    border-color: var(--danger);
    color: var(--danger);
  }
  .ab:disabled {
    opacity: 0.4;
    cursor: default;
  }

  .hablist {
    display: flex;
    flex-direction: column;
    gap: 4px;
    max-height: 340px;
    overflow: auto;
  }
  .hrow2 {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 4px 0 4px 2px;
  }
  .hn2 {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 12.5px;
    color: var(--fg-dim);
  }
  .tri {
    display: flex;
    gap: 3px;
    flex: 0 0 auto;
  }
  .t {
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
    border-radius: 7px;
    border: 1px solid var(--line);
    background: var(--panel-2);
    color: var(--fg-faint);
    transition: background 0.12s, border-color 0.12s, color 0.12s;
  }
  .t:hover {
    color: var(--fg);
    border-color: var(--line-2);
  }
  .t.done.on {
    background: var(--accent-soft);
    border-color: var(--accent);
    color: var(--accent);
  }
  .t.missed.on {
    background: var(--danger-soft);
    border-color: var(--danger);
    color: var(--danger);
  }
  .t.clr.on {
    background: var(--hover);
    color: var(--fg-mute);
  }

  .flash {
    margin-top: 12px;
    padding: 8px 10px;
    border-radius: 8px;
    background: var(--accent-soft);
    border-left: 2px solid var(--accent);
    color: var(--accent);
    font-size: 11.5px;
    font-family: var(--font-mono);
    animation: fadein 0.2s ease-out;
  }
  @keyframes fadein {
    from {
      opacity: 0;
      transform: translateY(-3px);
    }
    to {
      opacity: 1;
      transform: none;
    }
  }

  .note2 {
    margin-top: 12px;
    font-size: 11px;
    color: var(--fg-faint);
    line-height: 1.7;
  }
  .empty2 {
    padding: 14px 4px;
    font-size: 11.5px;
    color: var(--fg-faint);
    line-height: 1.7;
  }

  .panel2 {
    display: grid;
    grid-template-columns: minmax(0, 1.6fr) minmax(0, 1fr);
    gap: 18px;
    padding: 16px;
    border-radius: calc(var(--radius) * 0.9);
    background: var(--card);
    border: 1px solid var(--line);
    margin-bottom: 20px;
  }
  h3 {
    font-size: 12px;
    font-weight: 500;
    color: var(--fg-dim);
    margin-bottom: 12px;
  }

  .bars {
    display: flex;
    align-items: flex-end;
    gap: 6px;
    height: 108px;
  }
  .barcol {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 5px;
    height: 100%;
  }
  .bartrack {
    position: relative;
    flex: 1;
    width: 100%;
    border-radius: 4px;
    background: var(--line);
    overflow: hidden;
  }
  .barfill {
    position: absolute;
    bottom: 0;
    left: 0;
    right: 0;
    border-radius: 4px;
    background: var(--accent-line);
    min-height: 2px;
  }
  .barfill.last {
    background: var(--accent);
  }
  .blab {
    font-size: 10px;
    color: var(--fg-faint);
    font-family: var(--font-mono);
  }

  .p2r ul {
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 7px;
  }
  .p2r li {
    display: flex;
    justify-content: space-between;
    font-size: 12px;
    color: var(--fg-mute);
  }
  .p2r li b {
    color: var(--fg);
    font-weight: 500;
  }

  .habits h3 {
    margin-bottom: 8px;
  }
  .hrow {
    display: flex;
    align-items: center;
    gap: 14px;
    width: 100%;
    padding: 9px 12px;
    border-radius: 8px;
    border: 1px solid transparent;
    color: var(--fg-dim);
    font-size: 12.5px;
    text-align: left;
  }
  .hrow:hover {
    background: var(--hover);
    border-color: var(--line);
  }
  .hn {
    flex: 1;
    color: var(--fg);
  }
  .hs b {
    font-size: 14px;
    color: var(--accent);
    font-weight: 500;
    margin-right: 3px;
  }
  .hs small,
  .hr small {
    font-size: 10.5px;
    color: var(--fg-faint);
  }
  .hr {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 190px;
  }
  .track {
    flex: 1;
    height: 5px;
    border-radius: 3px;
    background: var(--line);
    overflow: hidden;
  }
  .fill {
    display: block;
    height: 100%;
    border-radius: 3px;
    background: var(--accent);
  }

  .empty {
    padding: 20px;
    text-align: center;
    font-size: 12px;
    color: var(--fg-faint);
  }

  .pb {
    position: fixed;
    inset: 0;
    z-index: 90;
  }
  .modal {
    position: fixed;
    z-index: 91;
    top: 24%;
    left: 50%;
    transform: translateX(-50%);
    width: 340px;
    padding: 16px;
    border-radius: var(--radius);
    background: var(--surface);
    border: 1px solid var(--line-2);
    backdrop-filter: blur(24px);
  }
  .modal p {
    margin-bottom: 10px;
    font-size: 12.5px;
    color: var(--fg-dim);
  }
  .mbtns {
    display: flex;
    gap: 8px;
    justify-content: flex-end;
    margin-top: 12px;
  }

  /* 动画是这个模块的反馈主体，系统层面关了动效就得整套让位 */
  @media (prefers-reduced-motion: reduce) {
    .cell,
    .ab,
    .t,
    .sw {
      transition: none;
    }
    .cell .mark,
    .ripple,
    .kpi b.bump,
    .flash {
      animation: none;
    }
    /* 涟漪没了，选中格子上的勾/叉就得顶上来，不能让反馈整个消失 */
    .cell .mark {
      opacity: 1;
    }
  }
</style>
