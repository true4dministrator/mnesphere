<script lang="ts">
  import { onMount } from 'svelte';
  import { EditorState, StateField, Compartment } from '@codemirror/state';
  import type { Range } from '@codemirror/state';
  import {
    EditorView,
    Decoration,
    WidgetType,
    keymap,
    drawSelection,
    highlightActiveLine,
    rectangularSelection,
    crosshairCursor,
    placeholder as cmPlaceholder
  } from '@codemirror/view';
  import type { DecorationSet } from '@codemirror/view';
  import { defaultKeymap, history, historyKeymap, indentWithTab } from '@codemirror/commands';
  import { markdown } from '@codemirror/lang-markdown';
  import { syntaxHighlighting, HighlightStyle, bracketMatching } from '@codemirror/language';
  import { tags as tg } from '@lezer/highlight';

  import * as api from '../api';
  import { assetUrl, eachImageRef } from '../markdown';
  import { cfg, doc, markDirty, notes, toast } from '../state.svelte';
  import type { TreeNode } from '../api';

  let host: HTMLDivElement;
  let view: EditorView | null = null;

  // wiki 链接补全
  let wiki = $state<null | { x: number; y: number; from: number; query: string }>(null);
  let wikiList: { title: string; path: string }[] = $state([]);
  let wikiSel = $state(0);

  const highlight = HighlightStyle.define([
    { tag: tg.heading1, color: 'var(--fg)', fontWeight: '600', fontSize: '1.45em' },
    { tag: tg.heading2, color: 'var(--fg)', fontWeight: '500', fontSize: '1.25em' },
    { tag: tg.heading3, color: 'var(--accent)', fontWeight: '500', fontSize: '1.1em' },
    { tag: [tg.heading4, tg.heading5, tg.heading6], color: 'var(--accent-2)', fontWeight: '500' },
    { tag: tg.strong, color: 'var(--fg)', fontWeight: '600' },
    { tag: tg.emphasis, fontStyle: 'italic', color: 'var(--fg-dim)' },
    { tag: tg.link, color: 'var(--accent)' },
    { tag: tg.url, color: 'var(--fg-faint)', textDecoration: 'underline' },
    { tag: tg.monospace, color: 'var(--accent-2)' },
    { tag: tg.quote, color: 'var(--fg-mute)', fontStyle: 'italic' },
    { tag: tg.list, color: 'var(--accent)' },
    { tag: tg.contentSeparator, color: 'var(--fg-faint)' },
    { tag: tg.processingInstruction, color: 'var(--fg-faint)' },
    { tag: tg.comment, color: 'var(--fg-faint)', fontStyle: 'italic' },
    { tag: tg.string, color: 'var(--accent-2)' },
    { tag: tg.keyword, color: 'var(--accent)' },
    { tag: tg.meta, color: 'var(--warn)' }
  ]);

  const cmTheme = EditorView.theme({
    '&': {
      color: 'var(--fg)',
      backgroundColor: 'transparent',
      height: '100%',
      fontSize: 'var(--fs)'
    },
    '.cm-scroller': {
      fontFamily: 'var(--font-mono)',
      lineHeight: '1.78',
      overflow: 'auto',
      cursor: 'text'
    },
    '.cm-content': {
      padding: '20px 0 45vh',
      caretColor: 'var(--accent)',
      maxWidth: '820px',
      margin: '0 auto',
      // ⚠️ 全局 `body { user-select: none }`（为了界面好点）会**继承进编辑器**，
      // 于是拖选文字变得又黏又飘、选不干净。CodeMirror 自己的基础样式只给
      // `.cm-layer` / `.cm-placeholder` 设了 none，从没把正文恢复成 text，
      // 这一步只能我们自己补 —— 别删。
      userSelect: 'text'
    },
    '.cm-line': { padding: '0 28px' },
    '.cm-cursor, .cm-dropCursor': { borderLeftColor: 'var(--accent)', borderLeftWidth: '2px' },
    '&.cm-focused .cm-selectionBackground, .cm-selectionBackground, .cm-content ::selection': {
      backgroundColor: 'var(--accent-soft)'
    },
    '.cm-activeLine': { backgroundColor: 'transparent' },
    '.cm-gutters': { backgroundColor: 'transparent', border: 'none', color: 'var(--fg-faint)' },
    '.cm-placeholder': { color: 'var(--fg-faint)' },
    '&.cm-focused': { outline: 'none' },
    '.cm-matchingBracket': {
      backgroundColor: 'var(--accent-soft)',
      outline: '1px solid var(--accent-line)'
    }
  });

  const editable = new Compartment();

  // ───────── frontmatter 折叠 ─────────
  //
  // 日记开头那三行 `--- date: ... ---` 在编辑态里纯属噪音：它既不是要写的内容，
  // 又占着第一屏最好的位置。这里把整块折成一枚「元数据 · 日期」的标签，
  // 想看原样点一下就展开。
  //
  // 顺带解决一个真实的坑：折叠之后光标进不去那一段 —— 以前光标默认停在 0，
  // 顺手打一个字就把元数据写坏了。
  //
  // 用 StateField 而不是 ViewPlugin：块级（block）替换装饰只有字段提供时才被允许。

  const FM_RE = /^(?:\uFEFF)?---\r?\n([\s\S]*?)\r?\n---[ \t]*(?:\r?\n|$)/;

  class FmChip extends WidgetType {
    // 不用「构造函数参数属性」那种简写 —— Svelte 的编译器不认这个 TS 特性，
    // 非得走预处理器才行。老老实实声明字段最省事。
    label: string;
    onOpen: () => void;
    constructor(label: string, onOpen: () => void) {
      super();
      this.label = label;
      this.onOpen = onOpen;
    }
    eq(o: FmChip) {
      return o.label === this.label;
    }
    toDOM() {
      const el = document.createElement('button');
      el.type = 'button';
      el.className = 'cn-fm-chip';
      el.textContent = this.label;
      el.title = '点开看这行的元数据（date）';
      el.onclick = (e) => {
        e.preventDefault();
        this.onOpen();
      };
      return el;
    }
    // 这里**不能**重写 ignoreEvent 返回 false。它的语义是「CM 要不要无视这个事件」，
    // 默认 true = 无视（正是我们要的：点标签不会顺手把光标塞到文档开头）。
    // 返回 false 反而是「别无视」，CM 就会去处理这次点击、挪走光标。
  }

  function buildFm(state: EditorState, onOpen: () => void): DecorationSet {
    const text = state.doc.toString();
    const m = FM_RE.exec(text);
    if (!m) return Decoration.none;
    // 整篇文档就是一段 frontmatter、且末尾没换行时干脆不折：块级替换把文档吃干净
    // 这种边角情况没必要去试探 CM 的脾气，原样显示就行。
    if (m[0].length >= text.length && !text.endsWith('\n')) return Decoration.none;
    const date =
      /(?:^|\n)[ \t]*date[ \t]*:[ \t]*(.+)/.exec(m[1])?.[1]?.trim().replace(/^["']|["']$/g, '') ??
      '';
    const label = date ? `元数据 · ${date}` : '元数据';
    return Decoration.set([
      Decoration.replace({ block: true, widget: new FmChip(label, onOpen) }).range(0, m[0].length)
    ]);
  }

  function fmField(onOpen: () => void) {
    return StateField.define<DecorationSet>({
      create: (state) => buildFm(state, onOpen),
      update: (deco, tr) => (tr.docChanged ? buildFm(tr.state, onOpen) : deco),
      provide: (f) => EditorView.decorations.from(f)
    });
  }

  const fmFold = new Compartment();
  /** 被手动展开元数据的是**哪个**文档。存路径而不是布尔值：换文档时它自然就对不上了，
   *  折叠状态自动收回，不需要再写一个「路径变了就重置」的副作用（那样还得防自读自写）。 */
  let fmOpenFor = $state('');
  const openFm = () => (fmOpenFor = doc.path);

  // ───────── 图片：把 `![[附件/x.png]]` 就地画出来 ─────────
  //
  // 以前编辑态只有一串原文，粘完图完全看不出成没成功 —— 用户以为没粘上，
  // 其实就是没渲染。这里把图片引用替换成一个真的 <img>。
  //
  // 两条硬约束：
  //  1. 用 StateField，不用 ViewPlugin —— 装饰要跟着**光标位置**走（光标压在上面
  //     得把原文露出来，否则改不动、删不掉），而 ViewPlugin 在选区变化时的重算
  //     时序不可靠。
  //  2. 只对**图片扩展名**生效。光看 `![[...]]` 分不出「嵌一张图」和「嵌一篇笔记」，
  //     不判扩展名就会把笔记嵌入渲染成一个坏掉的 <img>。

  class ImgWidget extends WidgetType {
    rel: string;
    url: string;
    from: number;
    to: number;
    onReveal: (from: number, to: number) => void;
    constructor(
      rel: string,
      url: string,
      from: number,
      to: number,
      onReveal: (from: number, to: number) => void
    ) {
      super();
      this.rel = rel;
      this.url = url;
      this.from = from;
      this.to = to;
      this.onReveal = onReveal;
    }
    // 必须把 from/to 也比进去：图片上面插了几行之后，位置变了但 rel 没变，
    // 只比 rel 的话 CM 会**复用旧 widget 实例**（连带旧的 from/to 闭包），
    // 点一下就会跳到错误的位置上。
    eq(o: ImgWidget) {
      return o.rel === this.rel && o.url === this.url && o.from === this.from && o.to === this.to;
    }
    toDOM() {
      const box = document.createElement('span');
      box.className = 'cn-img';
      const el = document.createElement('img');
      el.src = this.url;
      el.alt = this.rel;
      el.loading = 'lazy';
      el.draggable = false;
      el.title = `${this.rel}\n（点一下露出源码，就能改或删）`;
      el.onclick = (e) => {
        e.preventDefault();
        this.onReveal(this.from, this.to);
      };
      box.appendChild(el);
      return box;
    }
    // 同 FmChip：**不重写 ignoreEvent**。默认 true = CM 无视这个事件，
    // 我们自己的 onclick 才收得到；返回 false 反而会被 CM 抢走、顺手挪光标。
  }

  function buildImages(
    state: EditorState,
    onReveal: (from: number, to: number) => void
  ): DecorationSet {
    const text = state.doc.toString();
    // 快路径：图片引用是 `![` 开头，绝大多数文档里一个都没有。
    // 这层判断是为了让「每次选区变化都重算」不至于变成拖选时候的性能负担。
    if (!text.includes('![')) return Decoration.none;

    const vault = cfg.current?.vault ?? '';
    const sel = state.selection;
    const decos: Range<Decoration>[] = [];
    eachImageRef(text, ({ from, to, rel }) => {
      // 光标/选区**真正**压在这一处上 → 露出原文。少了这条就没法把图删掉。
      //
      // 判定必须用严格不等号（`from < to && to > from`，注意右边拿的是选区端点）：
      // 粘贴完之后光标恰恰停在引用的**末尾**（`insertAtCursor` 把锚点放在 to），
      // 用 `<=` / `>=` 的话那一刻就会被判成「压在上面」，于是刚粘完的图反而
      // 立刻缩回一串源码 —— 用户看到的还是「没显示」。
      // 严格版：光标落在 from 或 to 这两个边界上算「在旁边」，落在中间才算「在里面」。
      for (const r of sel.ranges) {
        if (r.from < to && r.to > from) return;
      }
      const url = assetUrl(vault, rel);
      if (!url) return;
      decos.push(
        Decoration.replace({ widget: new ImgWidget(rel, url, from, to, onReveal) }).range(from, to)
      );
    });
    return decos.length ? Decoration.set(decos, true) : Decoration.none;
  }

  function imgField(onReveal: (from: number, to: number) => void) {
    return StateField.define<DecorationSet>({
      create: (state) => buildImages(state, onReveal),
      update: (deco, tr) =>
        tr.docChanged || tr.selection ? buildImages(tr.state, onReveal) : deco.map(tr.changes),
      provide: (f) => EditorView.decorations.from(f)
    });
  }

  /** 点图 → 把整条引用选中。选区一进去，上面的 buildImages 就改为露原文，
   *  于是「点一下 → 源码选中 → 直接改或删」一气呵成；再点别处又变回图片。 */
  function revealImage(from: number, to: number) {
    if (!view) return;
    view.dispatch({ selection: { anchor: from, head: to } });
    view.focus();
  }

  function flatten(nodes: TreeNode[], out: { title: string; path: string }[] = []) {
    for (const n of nodes) {
      if (n.isDir) flatten(n.children, out);
      else out.push({ title: n.name, path: n.path });
    }
    return out;
  }

  let allNotes: { title: string; path: string }[] = [];
  $effect(() => {
    allNotes = flatten(notes.roots);
  });

  /** 光标前是不是有一个没闭合的 `[[` */
  function detectWiki(state: EditorState) {
    const pos = state.selection.main.head;
    const lineStart = state.doc.lineAt(pos).from;
    const before = state.sliceDoc(lineStart, pos);
    const m = /\[\[([^\[\]\n]{0,40})$/.exec(before);
    if (!m) return null;
    return { from: pos - m[1].length, query: m[1] };
  }

  function refreshWiki(v: EditorView) {
    const hit = detectWiki(v.state);
    if (!hit) {
      wiki = null;
      return;
    }
    // 已经闭合就不要弹
    const after = v.state.sliceDoc(v.state.selection.main.head, v.state.selection.main.head + 2);
    if (after.startsWith(']]')) {
      wiki = null;
      return;
    }
    const q = hit.query.toLowerCase();
    wikiList = allNotes
      .filter((n) => !q || n.title.toLowerCase().includes(q))
      .slice(0, 8);
    wikiSel = 0;
    const coords = v.coordsAtPos(v.state.selection.main.head);
    wiki = {
      x: coords?.left ?? 200,
      y: (coords?.bottom ?? 200) + 6,
      from: hit.from,
      query: hit.query
    };
  }

  function acceptWiki(pick: { title: string; path: string }) {
    if (!view || !wiki) return;
    const pos = view.state.selection.main.head;
    const insert = `${pick.title}]]`;
    view.dispatch({
      changes: { from: wiki.from, to: pos, insert },
      selection: { anchor: wiki.from + insert.length }
    });
    wiki = null;
    view.focus();
  }

  /** 编辑态里 Ctrl/Cmd + 点击可以直接跳转双链 */
  function onHostClick(e: MouseEvent) {
    if (!view || !(e.ctrlKey || e.metaKey)) return;
    const pos = view.posAtCoords({ x: e.clientX, y: e.clientY });
    if (pos == null) return;
    const line = view.state.doc.lineAt(pos);
    const text = line.text;
    const col = pos - line.from;
    const re = /\[\[([^\[\]|]+)(?:\|[^\]]*)?\]\]/g;
    let m: RegExpExecArray | null;
    while ((m = re.exec(text))) {
      if (col >= m.index && col <= m.index + m[0].length) {
        e.preventDefault();
        window.dispatchEvent(new CustomEvent('mnesphere:wikilink', { detail: m[1].trim() }));
        return;
      }
    }
  }

  // ───────── 图片：粘贴进来的图 ─────────
  //
  // 图只能从 `paste` 事件里拿：WebView2 里 `navigator.clipboard.read()` 给不到图片，
  // 而 `clipboardData` 里直接就是系统剪贴板里那份 PNG 的**原始字节**，不用自己编码
  // （对比：官方 clipboard-manager 插件的 readImage() 吐的是裸 RGBA，还得再编码一遍）。
  //
  // 监听挂在宿主的**捕获**相位：必须先于 CodeMirror 自己的 paste 处理跑，否则它会先
  // 往文档里插一道没用的东西。

  /** 剪贴板里挑出第一张图；没图就返回 null，让文本照旧交给 CodeMirror。 */
  function pickImage(dt: DataTransfer | null): File | null {
    if (!dt) return null;
    for (const it of Array.from(dt.items)) {
      if (it.kind === 'file' && it.type.startsWith('image/')) {
        const f = it.getAsFile();
        if (f) return f;
      }
    }
    // 有些来源不填 items，只在 files 里放
    for (const f of Array.from(dt.files)) {
      if (f.type.startsWith('image/')) return f;
    }
    return null;
  }

  /** 在光标处插入一段文本，并把光标挪到它后面。 */
  function insertAtCursor(text: string) {
    if (!view) return;
    const sel = view.state.selection.main;
    view.dispatch({
      changes: { from: sel.from, to: sel.to, insert: text },
      selection: { anchor: sel.from + text.length }
    });
    view.focus();
  }

  async function onPaste(e: ClipboardEvent) {
    const file = pickImage(e.clipboardData);
    if (!file) {
      // 探针：只在「剪贴板里像是有图、但没抠出来」时才吭声 —— 普通的文本粘贴
      // 不该往控制台刷噪声。真机上这里要是冒了输出，说明得换官方剪贴板插件那条路。
      const types = e.clipboardData ? Array.from(e.clipboardData.types) : [];
      if (types.some((t) => t.startsWith('image/') || t === 'Files')) {
        console.warn('[mnesphere] 剪贴板里像是有图，但没抠出来。types =', types.join(','));
      }
      return;
    }
    e.preventDefault();
    // 捕获相位一起掐掉，免得 CodeMirror 再补一遍
    e.stopPropagation();
    try {
      const bytes = new Uint8Array(await file.arrayBuffer());
      const rel = await api.savePastedImage(bytes);
      insertAtCursor(`![[${rel}]]`);
      toast('图片已存进附件', 'ok');
    } catch (err) {
      toast(typeof err === 'string' ? err : String(err), 'error');
    }
  }

  /** 拖进来的文件由 App 那边落盘，这里只负责把链接写进光标处。 */
  function onInsertRequest(e: Event) {
    const text = (e as CustomEvent<string>).detail;
    if (text) insertAtCursor(text);
  }

  onMount(() => {
    view = new EditorView({
      parent: host,
      state: EditorState.create({
        doc: doc.data?.content ?? '',
        extensions: [
          history(),
          drawSelection(),
          rectangularSelection(),
          crosshairCursor(),
          highlightActiveLine(),
          bracketMatching(),
          markdown(),
          syntaxHighlighting(highlight),
          cmTheme,
          cmPlaceholder('开始写点什么…  支持 [[双链]]、#标签、粘贴或拖入图片'),
          fmFold.of(fmField(openFm)),
          imgField(revealImage),
          keymap.of([
            {
              key: 'ArrowDown',
              run: () => {
                if (!wiki) return false;
                wikiSel = Math.min(wikiSel + 1, wikiList.length - 1);
                return true;
              }
            },
            {
              key: 'ArrowUp',
              run: () => {
                if (!wiki) return false;
                wikiSel = Math.max(wikiSel - 1, 0);
                return true;
              }
            },
            {
              key: 'Enter',
              run: () => {
                if (!wiki || !wikiList.length) return false;
                acceptWiki(wikiList[wikiSel]);
                return true;
              }
            },
            {
              key: 'Escape',
              run: () => {
                if (!wiki) return false;
                wiki = null;
                return true;
              }
            },
            indentWithTab
          ]),
          keymap.of([...defaultKeymap, ...historyKeymap]),
          EditorView.updateListener.of((u) => {
            if (u.docChanged) {
              markDirty(u.state.doc.toString());
            }
            if (u.docChanged || u.selectionSet) {
              refreshWiki(u.view);
            }
          }),
          editable.of(EditorView.editable.of(true))
        ]
      })
    });

    host.addEventListener('paste', onPaste, true);
    window.addEventListener('mnesphere:insert', onInsertRequest);

    return () => {
      host.removeEventListener('paste', onPaste, true);
      window.removeEventListener('mnesphere:insert', onInsertRequest);
      view?.destroy();
      view = null;
    };
  });

  // 切文档 / 外部改动 → 同步进编辑器
  $effect(() => {
    const path = doc.path;
    const content = doc.data?.content ?? '';
    const isDiary = doc.data?.kind === 'diary';
    if (!view) return;
    const cur = view.state.doc.toString();
    void path;
    if (cur === content) return;
    view.dispatch({ changes: { from: 0, to: cur.length, insert: content } });
    // 日记是流水账：打开就是接着写，所以光标放到文末。
    // 而且日记开头是 frontmatter，光标停在 0 会落在 `---` 之前 ——
    // 顺手打一个字就把元数据写坏了，这个坑不得不防。
    view.dispatch({
      selection: { anchor: isDiary ? content.length : 0 },
      scrollIntoView: false
    });
    view.scrollDOM.scrollTop = 0;
  });

  /**
   * 折叠开关。只依赖 doc.path 和 fmOpenFor，刻意不读 doc 内容 ——
   * 读了的话每敲一个字都要重配一次扩展，纯属白烧。
   */
  $effect(() => {
    const path = doc.path;
    const open = fmOpenFor === path && path !== '';
    if (!view) return;
    view.dispatch({ effects: fmFold.reconfigure(open ? [] : fmField(openFm)) });
  });

  export function focusEditor() {
    view?.focus();
  }
</script>

<div class="wrap">
  <!-- 这个 div 只是 CodeMirror 的宿主：可编辑性、光标、键盘可达性全部由 CM 自己
       创建的 contenteditable 承担；外层 click 仅用于拦截 Ctrl/Cmd+点击 [[双链]] 跳转。 -->
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="cm-host" bind:this={host} onclick={onHostClick}></div>

  {#if wiki && wikiList.length}
    <div class="wiki-pop" style="left:{wiki.x}px; top:{wiki.y}px">
      {#each wikiList as n, i (n.path)}
        <button class:on={i === wikiSel} onclick={() => acceptWiki(n)} onmouseenter={() => (wikiSel = i)}>
          <span>{n.title}</span>
          <small>{n.path}</small>
        </button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .wrap {
    position: relative;
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .cm-host {
    flex: 1;
    min-height: 0;
    overflow: hidden;
  }

  .wiki-pop {
    position: fixed;
    z-index: 70;
    min-width: 240px;
    max-width: 400px;
    padding: 4px;
    border-radius: 10px;
    background: var(--surface);
    border: 1px solid var(--line-2);
    backdrop-filter: blur(24px);
    box-shadow: 0 14px 36px rgba(0, 0, 0, 0.45);
  }
  .wiki-pop button {
    display: flex;
    flex-direction: column;
    gap: 1px;
    width: 100%;
    padding: 6px 9px;
    border-radius: 7px;
    text-align: left;
  }
  .wiki-pop button.on {
    background: var(--accent-soft);
  }
  .wiki-pop span {
    font-size: 12.5px;
    color: var(--fg);
  }
  .wiki-pop small {
    font-size: 10.5px;
    color: var(--fg-faint);
    font-family: var(--font-mono);
  }

  /* frontmatter 折叠后那枚标签。它是 WidgetType 里 document.createElement 出来的，
     拿不到 Svelte 的 scope 类，所以只能用 :global 命中。 */
  :global(.cn-fm-chip) {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 22px;
    margin: 0 28px 6px;
    padding: 0 10px;
    border-radius: 11px;
    background: var(--card);
    border: 1px solid var(--line);
    color: var(--fg-mute);
    font-family: var(--font-mono);
    font-size: 10.5px;
    cursor: pointer;
    user-select: none;
    transition: background 0.12s, border-color 0.12s, color 0.12s;
  }
  :global(.cn-fm-chip::before) {
    content: '';
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: var(--accent);
    opacity: 0.7;
  }
  :global(.cn-fm-chip:hover) {
    background: var(--accent-soft);
    border-color: var(--accent-line);
    color: var(--accent);
  }

  /* 编辑态里的图片预览。它同样是 WidgetType 里 document.createElement 出来的，
     拿不到 Svelte 的 scope 类，只能用 :global 命中。 */
  :global(.cm-content .cn-img) {
    display: inline-block;
    max-width: 100%;
    padding: 5px 0;
    vertical-align: bottom;
  }
  :global(.cm-content .cn-img img) {
    display: block;
    max-width: 100%;
    /* 竖屏截图能有一屏高，不限一下会把整个编辑区顶飞 */
    max-height: 46vh;
    width: auto;
    border-radius: calc(var(--radius) * 0.7);
    border: 1px solid var(--line);
    background: var(--inset);
    cursor: zoom-in;
  }
</style>
