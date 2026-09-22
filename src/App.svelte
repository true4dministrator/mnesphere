<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { getCurrentWebview } from '@tauri-apps/api/webview';

  /** @tauri-apps/api 把 ResizeDirection 声明成模块内局部类型却没导出，
   *  只能在这儿照着抄一份。方向名是 Tauri 定死的，不会变。 */
  type ResizeDirection =
    | 'East'
    | 'North'
    | 'NorthEast'
    | 'NorthWest'
    | 'South'
    | 'SouthEast'
    | 'SouthWest'
    | 'West';

  import * as api from './lib/api';
  import type { TreeNode } from './lib/api';
  import {
    ai,
    bootstrap,
    cfg,
    checkin,
    doc,
    flushDoc,
    flushIfDirty,
    focusSearch,
    notes,
    openCtxMenu,
    refreshAll,
    refreshCheckin,
    saveConfig,
    tasks,
    toast,
    ui,
    welcome
  } from './lib/state.svelte';
  import { TASK_VIEWS } from './lib/tasks';
  import { clampPanel, measurePanel, PANEL_EXTRA_NOTES, PANEL_EXTRA_TASK } from './lib/panelWidth';
  import { blankMenuItems, editableTarget, editMenuItems } from './lib/ctx';

  import TitleBar from './lib/components/TitleBar.svelte';
  import Rail from './lib/components/Rail.svelte';
  import Panel from './lib/components/Panel.svelte';
  import MainView from './lib/components/MainView.svelte';
  import AiDrawer from './lib/components/AiDrawer.svelte';
  import AiHandle from './lib/components/AiHandle.svelte';
  import StatusBar from './lib/components/StatusBar.svelte';
  import ContextMenu from './lib/components/ContextMenu.svelte';
  import Welcome from './lib/components/Welcome.svelte';

  const win = getCurrentWindow();
  let unlisteners: UnlistenFn[] = [];
  let narrow = $state(false);
  let dropping = $state(false);
  const onResize = () => (narrow = window.innerWidth < 1500);

  const EDGES: { dir: ResizeDirection; cls: string }[] = [
    { dir: 'North', cls: 'n' },
    { dir: 'South', cls: 's' },
    { dir: 'West', cls: 'w' },
    { dir: 'East', cls: 'e' },
    { dir: 'NorthWest', cls: 'nw' },
    { dir: 'NorthEast', cls: 'ne' },
    { dir: 'SouthWest', cls: 'sw' },
    { dir: 'SouthEast', cls: 'se' }
  ];

  function startResize(e: MouseEvent, dir: ResizeDirection) {
    if (e.buttons !== 1) return;
    e.preventDefault();
    win.startResizeDragging(dir);
  }

  // ───────── 左栏宽度：拖分界线 / 自适应 ─────────
  //
  // 三个硬约束：
  // 1. 拖动中**不落盘** —— 每帧发一次 IPC 会把配置写爆，等松手再存一次。
  // 2. 监听挂 document 上，不然鼠标甩出那条 4px 就断了。
  // 3. 松手时如果拖到的宽度比内容需要的还小，就吸附到内容宽度 ——
  //    用户要的是「别再截断了」，不是「精确到 1px」。

  let panelDragging = $state(false);

  function applyPanelWidth(w: number) {
    document.documentElement.style.setProperty('--panel-w', `${w}px`);
  }

  function startPanelDrag(e: MouseEvent) {
    if (e.buttons !== 1) return;
    e.preventDefault();
    panelDragging = true;

    const railW = document.querySelector('.rail')?.getBoundingClientRect().width ?? 46;
    const move = (ev: MouseEvent) => {
      applyPanelWidth(clampPanel(ev.clientX - railW, window.innerWidth));
    };
    const up = () => {
      document.removeEventListener('mousemove', move);
      document.removeEventListener('mouseup', up);
      panelDragging = false;
      const now = parseFloat(
        getComputedStyle(document.documentElement).getPropertyValue('--panel-w')
      );
      if (!Number.isFinite(now)) return;
      // 松手 = 从「自适应」切成手动值。用户亲手定的宽度就是答案，
      // 不该再被下一次内容变化顶回去。
      if (cfg.current) {
        cfg.current.theme = { ...cfg.current.theme, panelWidth: Math.round(now) };
      }
      void saveConfig({});
    };
    document.addEventListener('mousemove', move);
    document.addEventListener('mouseup', up);
  }

  /** 自适应：称完内容宽度写进 CSS 变量。只在 task/notes 这类列表页有意义。 */
  function autoFitPanel() {
    if (!cfg.current) return;
    if (cfg.current.theme.panelWidth !== 'auto') return;
    const { samples, extra } = panelSamples();
    if (!samples.length) return;
    const px = measurePanel(samples, cfg.current.theme.panelFont ?? 12.5, extra);
    applyPanelWidth(clampPanel(px, window.innerWidth));
  }

  /** 当前模块左栏里「最长的几条文字」。称宽度用，只取候选、不含排版。 */
  function panelSamples(): { samples: string[]; extra: number } {
    if (ui.activity === 'task') {
      return {
        samples: tasks.items
          .filter((t) => !t.done)
          .map((t) => t.title)
          .concat(TASK_VIEWS.map((v) => v.label)),
        extra: PANEL_EXTRA_TASK
      };
    }
    if (ui.activity === 'notes') {
      const out: string[] = [];
      const walk = (ns: TreeNode[]) => {
        for (const n of ns) {
          out.push(n.name);
          if (n.isDir) walk(n.children);
        }
      };
      walk(notes.roots);
      return { samples: out, extra: PANEL_EXTRA_NOTES };
    }
    return { samples: [], extra: PANEL_EXTRA_TASK };
  }

  onMount(async () => {
    onResize();
    window.addEventListener('resize', onResize);
    await bootstrap();

    unlisteners.push(
      await listen<string[]>('vault-changed', async (ev) => {
        const paths = ev.payload ?? [];
        if (!paths.length) return;
        // 外部编辑器改了自己的笔记 → 重新读一遍，但不打断正在输入的内容
        if (doc.path && paths.includes(doc.path) && !doc.dirty) {
          const d = await api.readDoc(doc.path);
          doc.data = d;
        }
        await refreshAll();
      })
    );

    unlisteners.push(
      await listen('refresh-stats', async () => {
        await refreshCheckin();
      })
    );

    unlisteners.push(
      await listen('tray-sync', async () => {
        await runSync();
      })
    );

    unlisteners.push(
      await getCurrentWebview().onDragDropEvent((ev) => {
        const p = ev.payload;
        if (p.type === 'enter' || p.type === 'over') {
          dropping = canDropNow();
          return;
        }
        if (p.type === 'drop') {
          void onDropPaths(p.paths ?? []);
          return;
        }
        // 拖出窗口 / 取消
        dropping = false;
      })
    );

    window.addEventListener('keydown', onKey);
    window.addEventListener('contextmenu', onContextMenu);
    // 关窗进托盘不会 unload 页面，beforeunload 指望不上 —— 失焦/切走时先落一盘，
    // 免得防抖窗口里那最后一次编辑（比如刚粘完图）跟着窗口一起消失。
    window.addEventListener('blur', flushIfDirty);
    document.addEventListener('visibilitychange', () => {
      if (document.hidden) flushIfDirty();
    });
    window.addEventListener('beforeunload', () => {
      void flushDoc();
    });
  });

  onDestroy(() => {
    unlisteners.forEach((u) => u());
    window.removeEventListener('keydown', onKey);
    window.removeEventListener('contextmenu', onContextMenu);
    window.removeEventListener('resize', onResize);
    window.removeEventListener('blur', flushIfDirty);
  });

  /** 全局接管右键：WebView2 自带那套「返回/刷新/打印」菜单跟这个软件格格不入，
   *  一律换成自绘的。可编辑区域给编辑菜单，其余地方给默认菜单。
   *  组件自己处理过的（例如文件行）会 preventDefault + stopPropagation，这里就不抢。 */
  function onContextMenu(e: MouseEvent) {
    if (e.defaultPrevented) return;
    e.preventDefault();
    e.stopPropagation();
    const el = editableTarget(e.target);
    openCtxMenu(e.clientX, e.clientY, el ? editMenuItems(el) : blankMenuItems());
  }

  async function runSync() {
    if (!cfg.current?.github.repo) {
      toast('还没填 GitHub 仓库地址，去设置里配一下', 'error');
      return;
    }
    toast('正在同步到 GitHub…');
    try {
      const r = await api.githubSync();
      toast(`${r.message}（commit ${r.commit || '无'}）`, 'ok');
    } catch (e) {
      toast(typeof e === 'string' ? e : String(e), 'error');
    }
  }

  // ───────── 拖图片进来 ─────────
  //
  // 必须走 Tauri 的原生事件：`dragDropEnabled` 开着的时候，webview 的 HTML5 drop
  // 事件会被 Tauri 整个吃掉，只有这儿能拿到被拖文件的**路径** —— 而路径正好能直接
  // 喂给早就写好的 import_attachment。
  //
  // 只在编辑态响应，跟「阅读态不响应粘贴」保持一致。

  function canDropNow(): boolean {
    return !!doc.path && doc.mode === 'edit';
  }

  async function onDropPaths(paths: string[]) {
    dropping = false;
    if (!canDropNow() || !paths.length) return;
    try {
      const rels = await api.importAttachment(paths);
      if (!rels.length) {
        toast('没认出来能放进附件的东西', 'error');
        return;
      }
      // 插入这一步交给 CMEditor —— 只有它手里有 CodeMirror 的光标位置
      window.dispatchEvent(
        new CustomEvent('mnesphere:insert', {
          detail: rels.map((r) => `![[${r}]]`).join('\n')
        })
      );
      toast(`已插入 ${rels.length} 个附件`, 'ok');
    } catch (e) {
      toast(typeof e === 'string' ? e : String(e), 'error');
    }
  }

  function isTyping(t: EventTarget | null): boolean {
    const el = t as HTMLElement | null;
    if (!el) return false;
    return (
      el.tagName === 'INPUT' ||
      el.tagName === 'TEXTAREA' ||
      el.isContentEditable ||
      !!el.closest('.cm-editor')
    );
  }

  function onKey(e: KeyboardEvent) {
    const mod = e.ctrlKey || e.metaKey;
    if (!mod) return;
    const k = e.key.toLowerCase();

    if (k === 's') {
      e.preventDefault();
      void flushDoc().then(() => toast('已保存'));
      return;
    }
    if (k === 'e') {
      e.preventDefault();
      doc.mode = doc.mode === 'edit' ? 'read' : 'edit';
      return;
    }
    if (k === 'j') {
      e.preventDefault();
      ui.aiOpen = !ui.aiOpen;
      return;
    }
    if (k === 'b' && !e.shiftKey) {
      e.preventDefault();
      ui.panelOpen = !ui.panelOpen;
      return;
    }
    if (k === 'k' || (k === 'f' && e.shiftKey)) {
      e.preventDefault();
      focusSearch();
      return;
    }
    if (isTyping(e.target)) return;
  }

  // AI 抽屉展开时自动收起左栏，给笔记腾地方（用户可在设置里关掉）
  const collapsePanel = $derived(
    ui.aiOpen && (cfg.current?.behavior.hidePanelWhenAiOpen ?? true) && narrow
  );

  // 自适应宽度：只在「自适应」模式下、且内容真的变了时才重算。
  // 注意是**只增不减** —— 删掉一条长任务就让整栏缩回去的话，
  // 界面会随着你的操作一直抽动，比宽一点难受得多。
  let lastFit = 0;
  $effect(() => {
    // 把这些读进来，任一变化都会重新跑这个 effect
    const sig = `${ui.activity}|${tasks.items.length}|${notes.roots.length}`;
    void sig;
    if (!cfg.current || cfg.current.theme.panelWidth !== 'auto') return;
    const need = (() => {
      const { samples, extra } = panelSamples();
      if (!samples.length) return 0;
      return clampPanel(
        measurePanel(samples, cfg.current.theme.panelFont ?? 12.5, extra),
        window.innerWidth
      );
    })();
    if (need > lastFit) {
      lastFit = need;
      applyPanelWidth(need);
    }
  });
</script>

<div class="app">
  <div class="wallpaper" aria-hidden="true"></div>

  {#each EDGES as e (e.cls)}
    <div
      class="resize {e.cls}"
      role="presentation"
      onmousedown={(ev) => startResize(ev, e.dir)}
    ></div>
  {/each}

  <TitleBar />

  {#if !ui.ready}
    <div class="boot">正在唤起记忆球…</div>
  {:else if ui.fatal}
    <div class="boot fatal">
      <p>启动失败</p>
      <pre>{ui.fatal}</pre>
    </div>
  {:else}
    <div class="body">
      <Rail />
      {#if ui.panelOpen && !collapsePanel}
        <Panel />
        <!-- 用 <button> 而不是 <div role=separator>：div 挂 role 后 Svelte 的
             a11y 检查仍然把它当非交互元素，tabindex 和事件监听都会被警告。
             一个能拖、能按方向键、能双击的控件本来就是按钮。 -->
        <button
          class="panelsplit"
          class:on={panelDragging}
          type="button"
          aria-label="调整左栏宽度"
          title="拖动调宽度，双击回到自适应"
          onmousedown={startPanelDrag}
          onkeydown={(e) => {
            // 键盘也要能调 —— 分界线拖得了，方向键就该同样调得了
            if (e.key !== 'ArrowLeft' && e.key !== 'ArrowRight') return;
            if (!cfg.current) return;
            e.preventDefault();
            const step = e.shiftKey ? 24 : 8;
            const cur = parseFloat(
              getComputedStyle(document.documentElement).getPropertyValue('--panel-w')
            );
            const base = Number.isFinite(cur) ? cur : 244;
            const next = clampPanel(
              base + (e.key === 'ArrowRight' ? step : -step),
              window.innerWidth
            );
            applyPanelWidth(next);
            cfg.current.theme = { ...cfg.current.theme, panelWidth: next };
            void saveConfig({});
          }}
          ondblclick={() => {
            // 双击回到自适应 —— 这是「我拖乱了，你给我还原」最顺手的出口
            if (!cfg.current) return;
            cfg.current.theme = { ...cfg.current.theme, panelWidth: 'auto' };
            lastFit = 0;
            void saveConfig({});
            autoFitPanel();
          }}
        ></button>
      {/if}
      <main class="main">
        <MainView />
      </main>
      {#if ui.aiOpen}
        <AiDrawer />
      {/if}
    </div>
    <StatusBar />
  {/if}

  <!-- 拉手必须挂在 .app 下、而不是 .body 里：.body 自己有 z-index，
       会形成层叠上下文，拉手就没法压过窗口缩放热区（.resize.e，z-index 50）。
       抽屉展开后由抽屉自己的收起拉手接手，所以这里只在收起时渲染。 -->
  {#if ui.ready && !ui.fatal && !ui.aiOpen}
    <AiHandle />
  {/if}

  {#if ui.toast}
    <div class="toast {ui.toast.kind}" role="status">{ui.toast.text}</div>
  {/if}

  {#if dropping && canDropNow()}
    <div class="dropzone" role="presentation">
      <div class="dz-inner">松手就存进附件，并在光标处插入链接</div>
    </div>
  {/if}

  <ContextMenu />

  {#if welcome.open}
    <Welcome />
  {/if}
</div>

<style>
  .app {
    position: relative;
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--bg);
    border: 1.5px solid var(--accent-line);
    border-radius: var(--radius);
    overflow: hidden;
  }

  /* 左栏分界线。热区做宽（5px）但视觉只有 1px —— 要的是好抓，
     不是看起来粗。平时整条透明，划过或拖动时才亮出来。 */
  .panelsplit {
    position: relative;
    z-index: 3;
    flex: 0 0 5px;
    margin-right: -5px;
    cursor: col-resize;
    background: transparent;
  }
  .panelsplit::after {
    content: '';
    position: absolute;
    inset: 0 2px;
    background: var(--accent-line);
    opacity: 0;
    transition: opacity 0.12s;
  }
  .panelsplit:hover::after,
  .panelsplit.on::after,
  .panelsplit:focus-visible::after {
    opacity: 1;
  }

  .wallpaper {
    position: absolute;
    inset: 0;
    background-image: var(--wallpaper, none);
    background-size: cover;
    background-position: center;
    opacity: var(--wallpaper-opacity, 1);
    filter: blur(var(--wallpaper-blur, 0px));
    transform: scale(1.04);
    pointer-events: none;
    z-index: 0;
  }

  /* 壁纸不铺时给一层极淡的几何底纹，避免大面积纯色显得空。
     注意别铺太浓：整屏盖一层色彩等于给所有内容蒙了层雾，什么色阶都显灰 —— 
     所以这里只留一点点角落的呼吸感。 */
  .wallpaper::after {
    content: '';
    position: absolute;
    inset: 0;
    background:
      radial-gradient(70% 55% at 82% 8%, var(--accent-soft), transparent 70%),
      radial-gradient(60% 50% at 8% 96%, var(--accent-soft), transparent 70%);
    opacity: 0.18;
  }

  .body {
    position: relative;
    z-index: 1;
    flex: 1;
    display: flex;
    min-height: 0;
  }

  .main {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    background: var(--panel);
    backdrop-filter: blur(var(--glass-blur, 0px));
  }

  .boot {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 12px;
    align-items: center;
    justify-content: center;
    color: var(--fg-mute);
    font-size: 13px;
  }
  .boot.fatal {
    color: var(--danger);
  }
  .boot pre {
    max-width: 70%;
    white-space: pre-wrap;
    font-family: var(--font-mono);
    font-size: 12px;
    color: var(--fg-dim);
  }

  .toast {
    position: absolute;
    bottom: calc(var(--status-h) + 16px);
    left: 50%;
    transform: translateX(-50%);
    z-index: 60;
    max-width: 62%;
    padding: 9px 16px;
    border-radius: 999px;
    background: var(--surface);
    border: 1px solid var(--line-2);
    color: var(--fg);
    font-size: 12.5px;
    backdrop-filter: blur(20px);
    white-space: pre-wrap;
    text-align: center;
  }
  .toast.ok {
    border-color: var(--accent-line);
    color: var(--accent);
  }
  .toast.error {
    border-color: var(--danger-soft);
    color: var(--danger);
  }

  /* 拖入提示。pointer-events 必须关掉：这层盖在正文上，一旦把事件吃掉，
     drop 就传不进 webview，Tauri 那边的 onDragDropEvent 也就收不到了。 */
  .dropzone {
    position: absolute;
    inset: 6px;
    z-index: 57;
    display: grid;
    place-items: center;
    border: 1px dashed var(--accent-line);
    border-radius: calc(var(--radius) - 5px);
    background: var(--accent-soft);
    pointer-events: none;
  }
  .dz-inner {
    padding: 9px 16px;
    border-radius: 999px;
    background: var(--surface);
    border: 1px solid var(--line-2);
    color: var(--accent);
    font-size: 12.5px;
    backdrop-filter: blur(20px);
  }

  /* 无边框窗口自己补缩放热区 */
  .resize {
    position: absolute;
    z-index: 50;
  }
  .resize.n {
    top: 0;
    left: 10px;
    right: 10px;
    height: 5px;
    cursor: ns-resize;
  }
  .resize.s {
    bottom: 0;
    left: 10px;
    right: 10px;
    height: 5px;
    cursor: ns-resize;
  }
  .resize.w {
    left: 0;
    top: 10px;
    bottom: 10px;
    width: 5px;
    cursor: ew-resize;
  }
  .resize.e {
    right: 0;
    top: 10px;
    bottom: 10px;
    width: 5px;
    cursor: ew-resize;
  }
  .resize.nw {
    top: 0;
    left: 0;
    width: 12px;
    height: 12px;
    cursor: nwse-resize;
  }
  .resize.ne {
    top: 0;
    right: 0;
    width: 12px;
    height: 12px;
    cursor: nesw-resize;
  }
  .resize.sw {
    bottom: 0;
    left: 0;
    width: 12px;
    height: 12px;
    cursor: nesw-resize;
  }
  .resize.se {
    bottom: 0;
    right: 0;
    width: 12px;
    height: 12px;
    cursor: nwse-resize;
  }
</style>
