<script lang="ts">
  import Icon from './Icons.svelte';
  import * as api from '../api';
  import type { ChatMessage } from '../api';
  import { render as renderMd } from '../markdown';
  import {
    ai,
    aiContextAll,
    aiContextRefs,
    cfg,
    diaries,
    doc,
    flushDoc,
    markDirty,
    maxRefs,
    notes,
    openDoc,
    toast,
    toggleCurrentRef,
    ui
  } from '../state.svelte';

  let scroller: HTMLDivElement;
  let picking = $state(false);
  let pick = $state('');

  const allDocs = $derived.by(() => {
    const out: { path: string; title: string }[] = [];
    for (const m of diaries.months) for (const i of m.items) out.push({ path: i.path, title: `${i.date} ${i.title}` });
    const walk = (ns: typeof notes.roots) => {
      for (const n of ns) {
        if (n.isDir) walk(n.children);
        else out.push({ path: n.path, title: n.name });
      }
    };
    walk(notes.roots);
    return out;
  });

  const filtered = $derived(
    pick.trim()
      ? allDocs.filter((d) => d.title.toLowerCase().includes(pick.trim().toLowerCase())).slice(0, 30)
      : allDocs.slice(0, 30)
  );

  /** 真正进上下文的那几篇（当前页 + 手选，截到上限）。展示、计数、发送都用它。 */
  const ctxRefs = $derived(aiContextRefs());
  /** 截断前的完整清单 —— 只用来判断「是不是有引用被上限挤掉了」。 */
  const ctxAll = $derived(aiContextAll());
  const overflow = $derived(ctxAll.length > maxRefs());
  /** 当前页这枚隐式引用此刻在不在上下文里。 */
  const curOn = $derived(!!doc.path && !ai.currentOff);

  const tokens = $derived.by(() => {
    let chars = 0;
    for (const r of ctxRefs) {
      const d = allDocs.find((x) => x.path === r);
      chars += d ? 500 : 0;
    }
    const msgChars = ai.messages.reduce((n, m) => n + m.content.length, 0);
    return Math.round((chars + msgChars) / 1.6);
  });

  function scrollDown() {
    requestAnimationFrame(() => {
      if (scroller) scroller.scrollTop = scroller.scrollHeight;
    });
  }

  async function send() {
    const text = ai.input.trim();
    if (!text || ai.streaming) return;
    if (!ai.hasKey) {
      toast('还没配置 API Key，去「设置 → AI」填一下', 'error');
      return;
    }
    ai.input = '';
    ai.messages.push({ role: 'user', content: text, thinking: '' });
    const idx = ai.messages.length;
    ai.messages.push({ role: 'assistant', content: '', thinking: '' });
    ai.streaming = true;
    scrollDown();

    const payload: ChatMessage[] = ai.messages
      .slice(0, -1)
      .map((m) => ({ role: m.role, content: m.content }));

    try {
      await api.aiChat(payload, aiContextRefs(), (ev) => {
        const target = ai.messages[idx];
        if (!target) return;
        if (ev.type === 'delta') {
          target.content += ev.text;
          scrollDown();
        } else if (ev.type === 'reasoning') {
          target.thinking += ev.text;
          scrollDown();
        } else if (ev.type === 'error') {
          target.content = target.content || ev.message;
          target.error = true;
          toast(ev.message, 'error');
        }
      });
    } catch (e) {
      const target = ai.messages[idx];
      if (target) {
        target.content = typeof e === 'string' ? e : String(e);
        target.error = true;
      }
    } finally {
      ai.streaming = false;
      // 不再挂载但引用仍可能被删掉，清理一下
      ai.messages = ai.messages.filter((m, i) => !(i === idx && !m.content && !m.thinking));
      scrollDown();
    }
  }

  /** 新对话只清消息和输入框 —— **不动** `ai.refs`。
   *  用户挂上去的引用是有意为之的，换一轮问答不该把他的上下文清空。 */
  function reset() {
    ai.messages = [];
    ai.input = '';
  }

  async function insertIntoNote() {
    const last = [...ai.messages].reverse().find((m) => m.role === 'assistant' && m.content);
    if (!last) return;
    if (!doc.data) {
      toast('先打开一篇笔记，我才有地方插', 'error');
      return;
    }
    const body = doc.data.content;
    const sep = body.endsWith('\n') ? '\n' : '\n\n';
    markDirty(`${body}${sep}> [!] 来自 AI 助手\n\n${last.content}\n`);
    await flushDoc();
    toast('已插入到当前笔记末尾', 'ok');
  }

  async function saveAsNote() {
    const last = [...ai.messages].reverse().find((m) => m.role === 'assistant' && m.content);
    if (!last) return;
    const title = last.content.split('\n')[0].replace(/[#*`>\s]/g, '').slice(0, 24) || 'AI 对话';
    try {
      const rel = await api.createNote(`${title}-${Date.now().toString(36).slice(-4)}`);
      await api.writeDoc(rel, `# ${title}\n\n${last.content}\n`);
      await openDoc(rel);
      ui.activity = 'notes';
      toast('已存成新笔记', 'ok');
    } catch (e) {
      toast(String(e), 'error');
    }
  }

  function copy(text: string) {
    navigator.clipboard.writeText(text).then(
      () => toast('已复制', 'ok'),
      () => toast('复制失败', 'error')
    );
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) {
      e.preventDefault();
      void send();
    }
  }

  /* ── 回答渲染 ──────────────────────────────────────────────
     以前这里是手搓的：转义 HTML + 认 [[双链]] + 换行变 <br>。
     结果就是抽屉里只认这三种东西 —— 公式原样显示、`**粗体**` 裸露星号、
     列表和代码块全不渲染，跟笔记里看到的两套样子。

     现在直接复用 `markdown.ts::render()`：**渲染口径只有一份**，
     以后笔记渲染改了，抽屉自动跟着改。公式、粗体、列表、代码块、双链一次到位。

     流式输出每帧都在改，整篇重渲染（markdown-it + DOMParser + KaTeX）会拖慢打字感，
     所以：**流式中那条做节流**，其余消息走缓存只渲染一次。 */

  let pump = $state(0); // 节流泵：自增一下，强制模板重算那一条
  const memo = new Map<string, string>(); // 非流式消息：缓存键 → HTML
  let streamText = '';
  let streamHtml = '';
  let streamTimer: ReturnType<typeof setTimeout> | null = null;

  function mdCtx() {
    return { vault: cfg.current?.vault ?? '', currentPath: doc.path };
  }

  /** 缓存键要带上 vault 与当前笔记：回答里若有**相对**图片引用，
   *  渲染结果依赖当前笔记的位置。只按正文文本做键的话，
   *  切一篇笔记再看同一条消息就会命中到旧路径的渲染结果。 */
  function keyOf(text: string): string {
    const c = mdCtx();
    return `${c.vault}\u0000${c.currentPath}\u0000${text}`;
  }

  function renderCached(text: string): string {
    const key = keyOf(text);
    const hit = memo.get(key);
    if (hit !== undefined) return hit;
    const html = renderMd(text, mdCtx()).html;
    // 简单封顶：对话很长时别让缓存无限堆
    if (memo.size > 60) memo.clear();
    memo.set(key, html);
    return html;
  }

  function renderStreaming(text: string): string {
    void pump; // 建立依赖：timer 里 pump++ 后模板会重算
    if (text === streamText) return streamHtml;
    streamText = text;
    if (streamTimer) return streamHtml; // 节流窗口内先顶着上一版
    streamTimer = setTimeout(() => {
      streamTimer = null;
      streamHtml = renderCached(streamText);
      pump++;
    }, 90);
    return streamHtml;
  }

  /** 回答正文 → HTML。`streaming` 为真时走节流。 */
  function renderMessage(text: string, streaming: boolean): string {
    return streaming ? renderStreaming(text) : renderCached(text);
  }

  async function follow(e: MouseEvent) {
    const el = e.target as HTMLElement;
    const wiki = el.closest('.wiki-link') as HTMLElement | null;
    if (wiki) {
      e.preventDefault();
      const target = wiki.dataset.target ?? '';
      const rel = await api.resolveLink(target);
      if (rel) {
        ui.activity = rel.includes('日记') ? 'diary' : 'notes';
        await openDoc(rel);
      } else {
        toast(`「${target}」还不存在`, 'info');
      }
      return;
    }
    const ext = el.closest('.ext-link') as HTMLElement | null;
    if (ext) {
      e.preventDefault();
      void api.openExternal(ext.dataset.href ?? '').catch((err) => toast(String(err), 'error'));
    }
  }
</script>

<aside class="drawer">
  <!-- 抽屉展开后，「拉手」的角色由这条接手：贴在抽屉外侧左缘，点它收起。
       收着的时候是 AiHandle 贴窗口右缘那一竖条，展开时两者视觉上是同一条。 -->
  <button
    class="pull"
    title="收起 AI 助手（Ctrl+J）"
    aria-label="收起 AI 助手"
    onclick={() => (ui.aiOpen = false)}
  >
    <Icon name="chev-r" size={13} />
  </button>

  <header class="dh">
    <span class="ttl">AI 助手</span>
    <span class="model mono">{cfg.current?.ai.model ?? ''}</span>
    <button class="ico" title="新对话" onclick={reset}><Icon name="plus" size={14} /></button>
    <button class="ico" title="收起（Ctrl+J）" onclick={() => (ui.aiOpen = false)}>
      <Icon name="chev-r" size={14} />
    </button>
  </header>

  <div class="ctx">
    <span class="clabel">上下文</span>

    <!-- 当前页：**隐式跟随**。不占用户的手选位，× 只是「这一轮先别带上」，
         换个页面就自己回来（见 openDoc 里的 currentOff 复位）。 -->
    {#if curOn}
      {@const cd = allDocs.find((x) => x.path === doc.path)}
      <span class="cchip cur" title={`当前页面（自动跟随）：${doc.path}`}>
        <button type="button" class="clink" onclick={() => openDoc(doc.path)}>
          <i class="dot"></i>
          <span>{cd?.title ?? doc.path.split('/').pop()}</span>
        </button>
        <button
          type="button"
          class="cx"
          title="这一轮先不带上（换页后自动回来）"
          aria-label="这一轮不引用当前页"
          onclick={toggleCurrentRef}>×</button
        >
      </span>
    {/if}

    <!-- 手选的那些。当前页已经由上面那枚「跟随」胶囊代表了，这里不再重复显示
         （用户把当前页也手动挂过的话，摘掉上面那枚它就以手选身份出现）。 -->
    {#each ai.refs.filter((r) => !(curOn && r === doc.path)) as r (r)}
      {@const d = allDocs.find((x) => x.path === r)}
      <!-- 胶囊本身不是按钮，里面放两个真按钮：点标题跳转、点 × 移出。
           不要在 button 里套 role=button 的元素再 stopPropagation —— 那是嵌套交互，键盘和读屏都不认。 -->
      <span class="cchip" title={r}>
        <button type="button" class="clink" onclick={() => openDoc(r)}>
          <Icon name="file" size={11} />
          <span>{d?.title ?? r.split('/').pop()}</span>
        </button>
        <button
          type="button"
          class="cx"
          title="移出上下文"
          aria-label="移出上下文"
          onclick={() => (ai.refs = ai.refs.filter((x) => x !== r))}>×</button
        >
      </span>
    {/each}

    <button class="cchip add" onclick={() => (picking = !picking)}>@ 引用笔记</button>

    {#if overflow}
      <span class="cover">超出上限，本次只发前 {maxRefs()} 篇</span>
    {/if}
  </div>

  {#if picking}
    <div class="picker">
      <!-- 打开引用面板就是要立刻打字，autofocus 是刻意的 -->
      <!-- svelte-ignore a11y_autofocus -->
      <input class="field" bind:value={pick} placeholder="搜笔记或日记…" autofocus />
      <div class="plist scroll">
        {#each filtered as d (d.path)}
          <button
            class="pitem"
            class:on={ctxRefs.includes(d.path)}
            onclick={() => {
              if (!ai.refs.includes(d.path)) ai.refs = [...ai.refs, d.path];
              picking = false;
              pick = '';
            }}
          >
            <span>{d.title}</span>
            <small>{d.path}</small>
          </button>
        {/each}
      </div>
    </div>
  {/if}

  <div class="msgs scroll" bind:this={scroller}>
    {#if !ai.messages.length}
      <div class="welcome">
        <p>问点什么，或者直接把右边的笔记挂进上下文。</p>
        <div class="qs">
          <button onclick={() => (ai.input = '把挂载的笔记总结成三条要点')}>总结要点</button>
          <button onclick={() => (ai.input = '这篇笔记里有没有前后矛盾的地方？')}>找矛盾</button>
          <button onclick={() => (ai.input = '基于这些内容，我接下来该做什么？')}>下一步</button>
        </div>
      </div>
    {/if}

    {#each ai.messages as m, i (i)}
      {#if m.role === 'user'}
        <div class="msg user">{m.content}</div>
      {:else}
        <div class="msg bot" class:err={m.error}>
          {#if m.thinking}
            <details class="think">
              <summary>思考过程</summary>
              <div>{m.thinking}</div>
            </details>
          {/if}
          {#if m.content}
            <!-- 回答里的 [[双链]] 和外部链接用事件委托统一处理，
                 键盘可达性由内层真正的 <a> 承担，这里只是外层容器。 -->
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <!-- svelte-ignore a11y_click_events_have_key_events -->
            <!-- 挂 `md-body` 是为了直接吃 Reader 那套全局排版（标题/列表/代码/表格/双链）。
                 这就是「渲染口径只有一份」在 CSS 上的落法：不重写一套，而是复用同一批。 -->
            <div class="rich md-body" onclick={follow}>{@html renderMessage(m.content, ai.streaming && i === ai.messages.length - 1)}</div>
          {:else if ai.streaming && i === ai.messages.length - 1}
            <div class="dots"><i></i><i></i><i></i></div>
          {/if}
        </div>

        {#if m.content && i === ai.messages.length - 1}
          <div class="acts">
            <button onclick={insertIntoNote}>插入到当前笔记</button>
            <button onclick={saveAsNote}>存为新笔记</button>
            <button onclick={() => copy(m.content)}>复制</button>
          </div>
          <span class="tok">上下文 ≈ {tokens.toLocaleString()} tokens</span>
        {/if}
      {/if}
    {/each}
  </div>

  <div class="input">
    <textarea
      class="ta"
      rows="1"
      bind:value={ai.input}
      onkeydown={onKey}
      placeholder={ai.hasKey ? '问点什么…（Enter 发送，Shift+Enter 换行）' : '先去设置里填 API Key'}
    ></textarea>
    <button class="send" disabled={ai.streaming || !ai.input.trim()} onclick={send} title="发送">
      <Icon name="send" size={15} />
    </button>
  </div>
</aside>

<style>
  .drawer {
    position: relative;
    z-index: 3;
    display: flex;
    flex-direction: column;
    width: var(--ai-w);
    flex: 0 0 var(--ai-w);
    max-width: 60vw;
    background: var(--panel-2);
    backdrop-filter: blur(var(--glass-blur, 0px));
    border-left: 1px solid var(--line);
    min-height: 0;
  }

  /* 贴在抽屉外侧左缘的收起拉手。抽屉是 position:relative，所以 left 取负值
     就能露在抽屉外面 —— 不占抽屉自己的内容宽度。 */
  .pull {
    position: absolute;
    left: -14px;
    top: 50%;
    transform: translateY(-50%);
    z-index: 6;
    display: grid;
    place-items: center;
    width: 14px;
    height: 54px;
    border-radius: 9px 0 0 9px;
    background: var(--card);
    border: 1px solid var(--line);
    border-right: 0;
    color: var(--fg-mute);
    opacity: 0.7;
    transition: opacity 0.14s, color 0.14s, background 0.14s, border-color 0.14s;
  }
  .pull:hover {
    opacity: 1;
    color: var(--accent);
    background: var(--accent-soft);
    border-color: var(--accent-line);
  }

  .dh {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 44px;
    flex: 0 0 44px;
    padding: 0 10px 0 16px;
    border-bottom: 1px solid var(--line);
  }
  .ttl {
    font-size: 13px;
    font-weight: 500;
  }
  .model {
    flex: 1;
    font-size: 10.5px;
    color: var(--fg-faint);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
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

  .ctx {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-wrap: wrap;
    padding: 9px 14px;
    border-bottom: 1px solid var(--line);
  }
  .clabel {
    font-size: 10.5px;
    color: var(--fg-faint);
  }
  .cchip {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    max-width: 200px;
    height: 22px;
    padding: 0 5px 0 9px;
    border-radius: 999px;
    background: var(--accent-soft);
    border: 1px solid var(--accent-line);
    color: var(--accent);
    font-size: 11px;
  }
  .cchip .clink {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    min-width: 0;
    color: inherit;
  }
  .cchip span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .cchip .cx {
    font-style: normal;
    line-height: 1;
    padding: 0 3px;
    opacity: 0.6;
    color: inherit;
  }
  .cchip .cx:hover {
    opacity: 1;
  }
  /* 「当前页」那枚隐式引用：同一个胶囊形状，用一个跟随小圆点跟手选区分开 */
  .cchip.cur .dot {
    flex: 0 0 auto;
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: currentColor;
    opacity: 0.6;
  }
  .cchip.add {
    padding: 0 8px;
    background: transparent;
    border: 1px dashed var(--line-2);
    color: var(--fg-mute);
  }
  .cchip.add:hover {
    color: var(--accent);
    border-color: var(--accent-line);
  }
  /* 引用比上限多时的一句实话：胶囊还挂在上面，但发出去的不含被挤掉的那些 */
  .cover {
    font-size: 10.5px;
    color: var(--warn, var(--fg-faint));
  }

  .picker {
    border-bottom: 1px solid var(--line);
    padding: 9px 14px;
  }
  .plist {
    margin-top: 7px;
    max-height: 200px;
  }
  .pitem {
    display: flex;
    flex-direction: column;
    gap: 1px;
    width: 100%;
    padding: 6px 8px;
    border-radius: 7px;
    text-align: left;
  }
  .pitem:hover,
  .pitem.on {
    background: var(--hover);
  }
  .pitem span {
    font-size: 12px;
    color: var(--fg-dim);
  }
  .pitem small {
    font-size: 10px;
    color: var(--fg-faint);
    font-family: var(--font-mono);
  }

  .msgs {
    flex: 1;
    min-height: 0;
    padding: 14px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .welcome {
    padding: 18px 4px;
    font-size: 12px;
    color: var(--fg-mute);
    line-height: 1.8;
  }
  .qs {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin-top: 12px;
  }
  .qs button {
    text-align: left;
    padding: 8px 11px;
    border-radius: 8px;
    border: 1px solid var(--line);
    font-size: 12px;
    color: var(--fg-dim);
  }
  .qs button:hover {
    border-color: var(--accent-line);
    background: var(--accent-soft);
    color: var(--accent);
  }

  .msg {
    max-width: 92%;
    padding: 9px 12px;
    border-radius: 12px;
    font-size: 12.5px;
    line-height: 1.75;
    user-select: text;
    word-break: break-word;
  }
  .msg.user {
    align-self: flex-end;
    background: var(--accent-soft);
    color: var(--accent);
    border: 1px solid var(--accent-line);
    border-bottom-right-radius: 4px;
  }
  .msg.bot {
    align-self: flex-start;
    background: var(--hover);
    border: 1px solid var(--line);
    color: var(--fg-dim);
    border-bottom-left-radius: 4px;
  }
  .msg.bot.err {
    border-color: var(--danger);
    color: var(--danger);
    background: var(--danger-soft);
  }

  /* 抽屉里的正文比阅读区密一点（宽度小），其余排版全部复用 .md-body 那套。
     注意：{@html} 注入的元素拿不到 Svelte 的 scope 类，所以子元素样式只能靠
     Reader 里的 :global(...)，这里不重复定义。 */
  .rich.md-body {
    font-size: 13.2px;
    line-height: 1.8;
    cursor: auto;
  }
  .rich.md-body :global(> *:first-child) {
    margin-top: 0;
  }
  /* 公式/代码块在窄抽屉里要能横向滚，不然会撑破气泡 */
  .rich.md-body :global(.katex-display) {
    overflow-x: auto;
    overflow-y: hidden;
    padding: 2px 0;
  }
  .rich.md-body :global(pre) {
    overflow-x: auto;
  }

  .think {
    margin-bottom: 7px;
    font-size: 11px;
    color: var(--fg-faint);
  }
  .think summary {
    cursor: pointer;
  }
  .think div {
    margin-top: 5px;
    padding-left: 9px;
    border-left: 2px solid var(--line);
    white-space: pre-wrap;
    line-height: 1.7;
  }

  .dots {
    display: flex;
    gap: 4px;
    padding: 3px 0;
  }
  .dots i {
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: var(--accent);
    animation: blink 1.2s infinite;
  }
  .dots i:nth-child(2) {
    animation-delay: 0.2s;
  }
  .dots i:nth-child(3) {
    animation-delay: 0.4s;
  }
  @keyframes blink {
    0%,
    60%,
    100% {
      opacity: 0.25;
    }
    30% {
      opacity: 1;
    }
  }

  .acts {
    display: flex;
    gap: 6px;
    flex-wrap: wrap;
    margin-top: -4px;
  }
  .acts button {
    height: 23px;
    padding: 0 10px;
    border-radius: 7px;
    border: 1px solid var(--line);
    font-size: 11.5px;
    color: var(--fg-mute);
  }
  .acts button:hover {
    border-color: var(--accent-line);
    color: var(--accent);
    background: var(--accent-soft);
  }
  .tok {
    font-size: 10.5px;
    color: var(--fg-faint);
    font-family: var(--font-mono);
    margin-top: -6px;
  }

  .input {
    display: flex;
    align-items: flex-end;
    gap: 8px;
    padding: 10px 14px 12px;
    border-top: 1px solid var(--line);
  }
  .ta {
    flex: 1;
    min-height: 38px;
    max-height: 160px;
    padding: 9px 11px;
    border-radius: 10px;
    background: var(--inset);
    border: 1px solid var(--line);
    font-size: 12.5px;
    line-height: 1.6;
    resize: none;
    user-select: text;
  }
  .ta:focus {
    border-color: var(--accent-line);
  }
  .send {
    display: grid;
    place-items: center;
    width: 38px;
    height: 38px;
    border-radius: 10px;
    background: var(--accent);
    color: var(--on-accent);
  }
  .send:disabled {
    opacity: 0.3;
    cursor: not-allowed;
  }
</style>
