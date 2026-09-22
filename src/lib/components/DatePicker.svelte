<script lang="ts">
  /**
   * 自绘日期选择器。
   *
   * 为什么不用 `<input type="date">`：那个日历是操作系统画的 —— 白底、方角、
   * 一套跟本应用无关的灰色按钮。`color-scheme: dark` 只能把它压暗，改不了形状和
   * 排版，放在这套深色玻璃界面里就像贴了张别的软件的截图。所以自己画一个，
   * 配色 / 圆角 / 字号全部走 `--` 变量，跟 `.field` 是一家人。
   *
   * 弹层是 `position: fixed` + 用 `use:portal` 挂到 `document.body` 下：
   *  1. 任务页的滚动容器是 `overflow: auto`，留在原地开向下会被裁掉半截；
   *  2. `App.svelte` 的 `.main` 挂着 `backdrop-filter`，那会让它成为 fixed 后代的
   *     包含块，于是 `getBoundingClientRect()` 拿到的视口坐标就全错了。
   * 挂到 body 底下两个问题一起解决，坐标就是实打实的视口坐标。
   */
  import Icon from './Icons.svelte';

  let {
    value = $bindable(''),
    placeholder = '选择日期',
    /** 选完之后的通知。**不只是方便** —— 详情页那种「没有草稿、选完立刻写盘」
     *  的场景必须知道用户什么时候真的选了，否则只能靠 $effect 盯 value，
     *  那会把「外部刷新导致的赋值」也算进来，写回成环。 */
    onpick
  }: { value?: string; placeholder?: string; onpick?: (iso: string) => void } = $props();

  const WEEK = ['一', '二', '三', '四', '五', '六', '日'];
  /** 弹层尺寸写死，翻月 / 换位置都不用等渲染后测量。改了样式记得同步这两个数。 */
  const POP_W = 208;
  const POP_H = 258;

  /** 方向键步长。周视图：左右 ±1 天，上下 ±7 天。 */
  const STEP: Record<string, number> = {
    ArrowLeft: -1,
    ArrowRight: 1,
    ArrowUp: -7,
    ArrowDown: 7
  };

  function pad(n: number): string {
    return String(n).padStart(2, '0');
  }

  /** 用本地时间格式化，不走 toISOString（那是 UTC，晚上 8 点之后会差一天）。 */
  function isoOf(d: Date): string {
    return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
  }

  const t0 = isoOf(new Date());

  type Cell = { iso: string; day: number; out: boolean };

  let open = $state(false);
  /** 日历翻到的月份 'YYYY-MM' */
  let ym = $state('');
  /** 键盘光标落在哪一天。方向键挪它，回车 / 空格选中。 */
  let focusDay = $state('');
  let at = $state({ left: 0, top: 0 });
  let trig = $state<HTMLButtonElement | null>(null);
  let pop = $state<HTMLDivElement | null>(null);

  /** `ym` 拆成 {年, 0 基月}。串被手改坏过就退回今天，不能让日历整个崩掉。 */
  function viewYM(): { y: number; m: number } {
    const s = ym || value || t0;
    const y = Number(s.slice(0, 4));
    const m = Number(s.slice(5, 7));
    if (!Number.isFinite(y) || !Number.isFinite(m) || m < 1 || m > 12) {
      const n = new Date();
      return { y: n.getFullYear(), m: n.getMonth() };
    }
    return { y, m: m - 1 };
  }

  /** 固定 6 行 × 7 列 = 42 格，上下月顺带填满，切换月份时高度不跳。 */
  const cells = $derived.by(() => {
    const { y, m } = viewYM();
    const lead = (new Date(y, m, 1).getDay() + 6) % 7; // 周一 = 0
    const first = new Date(y, m, 1 - lead);
    const out: Cell[] = [];
    for (let i = 0; i < 42; i += 1) {
      const d = new Date(first.getFullYear(), first.getMonth(), first.getDate() + i);
      out.push({ iso: isoOf(d), day: d.getDate(), out: d.getMonth() !== m });
    }
    return out;
  });

  const heading = $derived.by(() => {
    const { y, m } = viewYM();
    return `${y} 年 ${m + 1} 月`;
  });

  /** 触发按钮上显示什么：有值就显示 ISO 原文（跟 md 文件里写的一致），没值给灰提示。 */
  const shown = $derived(value || placeholder);

  /** 弹层挂到 body 下。见文件头注释。 */
  function portal(node: HTMLElement) {
    document.body.appendChild(node);
    return {
      destroy() {
        node.remove();
      }
    };
  }

  function place() {
    const r = trig?.getBoundingClientRect();
    if (!r) return;
    const below = r.bottom + 6;
    const up = below + POP_H > window.innerHeight - 8;
    at.left = Math.max(8, Math.min(r.left, window.innerWidth - POP_W - 8));
    at.top = up ? Math.max(8, r.top - POP_H - 6) : below;
  }

  function show() {
    if (open) {
      open = false;
      return;
    }
    // 每次打开都从当前值（没有就今天）起算，不要记住上次翻到哪个月
    ym = (value || t0).slice(0, 7);
    focusDay = value || t0;
    open = true;
    place();
  }

  function pick(iso: string) {
    value = iso;
    open = false;
    onpick?.(iso);
    trig?.focus();
  }

  function stepMonth(delta: number) {
    const { y, m } = viewYM();
    const d = new Date(y, m + delta, 1);
    ym = `${d.getFullYear()}-${pad(d.getMonth() + 1)}`;
  }

  /** 方向键在格子之间走。跨月时改 `ym`，让下面的 $effect 把焦点接过去。 */
  function onDayKey(e: KeyboardEvent, iso: string) {
    if (e.key === 'Escape') {
      open = false;
      trig?.focus();
      return;
    }
    const step = STEP[e.key];
    if (step === undefined) return;
    e.preventDefault();
    const d = new Date(`${iso}T00:00:00`);
    if (Number.isNaN(d.getTime())) return;
    d.setDate(d.getDate() + step);
    const next = isoOf(d);
    if (next.slice(0, 7) !== ym) ym = next.slice(0, 7);
    focusDay = next;
  }

  // 弹层是 fixed 的，页面一滚触发按钮就跑别处去了 —— 直接收起来，比追着算坐标省事
  $effect(() => {
    if (!open) return;
    const away = (e: MouseEvent) => {
      const n = e.target as Node;
      if (trig?.contains(n) || pop?.contains(n)) return;
      open = false;
    };
    const drop = () => (open = false);
    const esc = (e: KeyboardEvent) => {
      if (e.key === 'Escape') open = false;
    };
    window.addEventListener('mousedown', away, true);
    window.addEventListener('scroll', drop, true);
    window.addEventListener('resize', drop);
    window.addEventListener('keydown', esc);
    return () => {
      window.removeEventListener('mousedown', away, true);
      window.removeEventListener('scroll', drop, true);
      window.removeEventListener('resize', drop);
      window.removeEventListener('keydown', esc);
    };
  });

  /** 键盘光标跟着走 —— 没有这一步方向键按下去只是数字在跳，焦点还在原地。 */
  $effect(() => {
    if (!open || !focusDay) return;
    pop?.querySelector<HTMLButtonElement>(`[data-d="${focusDay}"]`)?.focus();
  });
</script>

<div class="dp">
  <button
    class="trig"
    class:on={open}
    class:set={!!value}
    class:hasclr={!!value}
    type="button"
    bind:this={trig}
    title={value || placeholder}
    onclick={show}
  >
    <Icon name="calendar" size={13} />
    <span class="lb">{shown}</span>
  </button>
  {#if value}
    <button
      class="clr"
      type="button"
      title="清除日期"
      aria-label="清除日期"
      onclick={() => {
        value = '';
        open = false;
        onpick?.('');
      }}
    >
      <Icon name="close" size={10} />
    </button>
  {/if}

  {#if open}
    <div
      class="pop"
      role="dialog"
      aria-label="选择日期"
      bind:this={pop}
      use:portal
      style="left: {at.left}px; top: {at.top}px; width: {POP_W}px;"
    >
      <div class="ph">
        <button class="nav" type="button" title="上个月" onclick={() => stepMonth(-1)}>
          <Icon name="chev-l" size={13} />
        </button>
        <span class="ym">{heading}</span>
        <button class="nav" type="button" title="下个月" onclick={() => stepMonth(1)}>
          <Icon name="chev-r" size={13} />
        </button>
      </div>

      <div class="wk">
        {#each WEEK as w (w)}<span>{w}</span>{/each}
      </div>

      <div class="grid">
        {#each cells as c (c.iso)}
          <button
            class="d"
            type="button"
            data-d={c.iso}
            class:out={c.out}
            class:today={c.iso === t0}
            class:on={c.iso === value}
            onclick={() => pick(c.iso)}
            onkeydown={(e) => onDayKey(e, c.iso)}
          >
            {c.day}
          </button>
        {/each}
      </div>

      <div class="pf">
        <button class="lnk" type="button" onclick={() => pick(t0)}>今天</button>
        <button
          class="lnk"
          type="button"
          onclick={() => {
            value = '';
            open = false;
            onpick?.('');
          }}>清除</button
        >
      </div>
    </div>
  {/if}
</div>

<style>
  .dp {
    position: relative;
    display: inline-flex;
    flex: 0 0 auto;
  }

  /* ── 触发按钮：跟 .field 同一套凹陷色，高度由外面用 --dp-h 调 ── */
  .trig {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: var(--dp-h, 26px);
    padding: 0 9px;
    border-radius: calc(var(--radius) * 0.6);
    background: var(--inset);
    border: 1px solid transparent;
    color: var(--fg-faint);
    font-size: 12px;
    white-space: nowrap;
    transition: background 0.12s, border-color 0.12s, color 0.12s;
  }
  .trig:hover {
    color: var(--fg-dim);
  }
  .trig.on {
    border-color: var(--accent-line);
    background: var(--accent-soft);
    color: var(--accent);
  }
  /* 选好了就让它站稳 —— 日期是这一行的关键信息，不该一直是灰的 */
  .trig.set {
    color: var(--fg);
    font-family: var(--font-mono);
  }
  .trig.set .lb {
    letter-spacing: -0.2px;
  }
  /* 有清除按钮时右边留出它的位置。用类而不是 :has() —— 样式裁剪认不出 :has()，
     留着可能被当成没用到而删掉。 */
  .trig.hasclr {
    padding-right: 22px;
  }

  .clr {
    position: absolute;
    right: 3px;
    top: 50%;
    transform: translateY(-50%);
    display: grid;
    place-items: center;
    width: 15px;
    height: 15px;
    border-radius: 5px;
    color: var(--fg-faint);
    transition: background 0.12s, color 0.12s;
  }
  .clr:hover {
    background: var(--hover);
    color: var(--fg);
  }

  /* ── 弹层 ── */
  .pop {
    position: fixed;
    z-index: 55;
    padding: 8px;
    border-radius: calc(var(--radius) * 0.9);
    background: var(--surface);
    border: 1px solid var(--line-2);
    backdrop-filter: blur(24px);
    box-shadow: 0 16px 40px rgba(0, 0, 0, 0.45);
  }

  .ph {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 4px;
    margin-bottom: 6px;
  }
  .ym {
    font-size: 12px;
    color: var(--fg);
  }
  .nav {
    display: grid;
    place-items: center;
    width: 22px;
    height: 22px;
    border-radius: 6px;
    color: var(--fg-mute);
  }
  .nav:hover {
    background: var(--hover);
    color: var(--accent);
  }

  .wk {
    display: grid;
    grid-template-columns: repeat(7, 1fr);
    margin-bottom: 2px;
  }
  .wk span {
    text-align: center;
    font-size: 10.5px;
    line-height: 18px;
    color: var(--fg-faint);
  }

  .grid {
    display: grid;
    grid-template-columns: repeat(7, 1fr);
    gap: 1px;
  }
  .d {
    height: 26px;
    border-radius: 7px;
    font-family: var(--font-mono);
    font-size: 11.5px;
    color: var(--fg-dim);
    transition: background 0.1s, color 0.1s;
  }
  .d:hover {
    background: var(--hover);
    color: var(--fg);
  }
  .d.out {
    color: var(--fg-faint);
    opacity: 0.45;
  }
  /* 今天只描一圈，不填色 —— 填色留给「已选中」，两者不能长得一样 */
  .d.today {
    color: var(--accent);
    box-shadow: inset 0 0 0 1px var(--accent-line);
  }
  .d.on {
    background: var(--accent);
    color: var(--on-accent);
    font-weight: 500;
  }
  .d.on:hover {
    background: var(--accent);
    color: var(--on-accent);
  }

  .pf {
    display: flex;
    justify-content: space-between;
    margin-top: 6px;
    padding-top: 6px;
    border-top: 1px solid var(--line);
  }
  .lnk {
    height: 20px;
    padding: 0 6px;
    border-radius: 6px;
    font-size: 11.5px;
    color: var(--fg-mute);
  }
  .lnk:hover {
    background: var(--accent-soft);
    color: var(--accent);
  }

  @media (prefers-reduced-motion: reduce) {
    .trig,
    .clr,
    .d {
      transition: none;
    }
  }
</style>
