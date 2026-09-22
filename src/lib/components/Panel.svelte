<script lang="ts">
  import Icon from './Icons.svelte';
  import * as api from '../api';
  import type { NoteMeta, Task, TreeNode } from '../api';
  import {
    checkin,
    cfg,
    diaries,
    doc,
    errText,
    focusTaskAdd,
    notes,
    openCtxMenu,
    openDoc,
    refreshCheckin,
    refreshDiaries,
    refreshNotes,
    selectTask,
    tasks,
    toast,
    toggleTaskGroup,
    ui
  } from '../state.svelte';
  import type { SettingsTab } from '../state.svelte';
  import {
    TASK_VIEWS,
    dueLabel,
    dueTone,
    resolveSel,
    taskCounts,
    tasksInView,
    todayStr
  } from '../tasks';
  import type { TaskView } from '../tasks';

  /** 设置页左栏的内容 = 分区导航。顺序就是用户从上往下读的顺序。 */
  const SETTINGS_SECTIONS: { id: SettingsTab; icon: string; label: string }[] = [
    { id: 'appearance', icon: 'palette', label: '外观' },
    { id: 'storage', icon: 'database', label: '存储' },
    { id: 'ai', icon: 'ai', label: 'AI 助手' },
    { id: 'github', icon: 'github', label: 'GitHub 同步' },
    { id: 'behavior', icon: 'sliders', label: '行为' },
    { id: 'about', icon: 'info', label: '关于' }
  ];

  let filter = $state('');
  /** null = 没在新建；否则记着「这一条要建到哪儿去」 */
  let newIn = $state<null | { kind: 'note' | 'folder'; parent: string | null }>(null);
  let newName = $state('');
  let promptCtx = $state<null | { label: string; value: string; ok: (v: string) => void }>(null);

  const today = () => new Date().toLocaleDateString('sv-SE');

  const filteredDiaries = $derived.by(() => {
    const f = filter.trim().toLowerCase();
    if (!f) return diaries.months;
    return diaries.months
      .map((m) => ({
        ...m,
        items: m.items.filter(
          (i) =>
            i.title.toLowerCase().includes(f) ||
            i.date.includes(f) ||
            i.tags.some((t) => t.toLowerCase().includes(f))
        )
      }))
      .filter((m) => m.items.length > 0);
  });

  function matchTree(nodes: TreeNode[], f: string): TreeNode[] {
    if (!f) return nodes;
    const out: TreeNode[] = [];
    for (const n of nodes) {
      if (n.isDir) {
        const kids = matchTree(n.children, f);
        if (kids.length) out.push({ ...n, children: kids });
      } else if (n.name.toLowerCase().includes(f) || n.path.toLowerCase().includes(f)) {
        out.push(n);
      }
    }
    return out;
  }

  const treeRoots = $derived(matchTree(notes.roots, filter.trim().toLowerCase()));

  /** 左栏角标和任务页分组必须同源 —— 都走 tasks.ts 里那一份口径。 */
  const taskCount = $derived(taskCounts(tasks.items));

  // ───────── 任务：左栏里的列表 ─────────
  //
  // 三组下面各自的任务行，用的是 tasksInView 拍平后的结果（不带次级分组标题，
  // 那些留在右半边）。左栏只平铺，保持能一眼扫完。

  /** 当前选中的那条。凭据存的是 raw + 标题，解析走 tasks.ts 的 resolveSel。 */
  const selKey = $derived.by(() => {
    const hit = resolveSel(tasks.items, tasks.selRaw, tasks.selHint, tasks.selTitle);
    return hit ? rowKey(hit) : '';
  });

  function rowKey(t: Task): string {
    return `${t.line}:${t.raw}`;
  }

  function groupItems(v: TaskView): Task[] {
    return tasksInView(tasks.items, v).flatMap((g) => g.items);
  }

  const EMPTY_VIEW: Record<TaskView, string> = {
    short: '一周内没有要做的',
    long: '没有更远的安排',
    done: '还没有完成的'
  };

  /**
   * 树里的 path 是「含 notes_dir 前缀的 vault 相对路径」，而 create_note 的 folder
   * 参数要的是「相对 notes_dir 的路径」。两套口径不一样，这里剥一次前缀。
   * （文件夹类命令要的恰好是前者，所以不用转换。）
   */
  function notesRel(parent: string): string {
    const dir = cfg.current?.editor.notesDir ?? '';
    const pre = dir ? `${dir}/` : '';
    return pre && parent.startsWith(pre) ? parent.slice(pre.length) : parent;
  }

  function expand(path: string) {
    notes.expanded.add(path);
    notes.expanded = new Set(notes.expanded);
  }

  function toggleDir(path: string) {
    if (notes.expanded.has(path)) notes.expanded.delete(path);
    else notes.expanded.add(path);
    notes.expanded = new Set(notes.expanded);
  }

  // ───────── 选中的那一行滚进可视区 ─────────
  //
  // 左栏只靠 `class:on` 标出当前行，从不滚动：新建的笔记按名字插到树中间、
  // 搜索结果/双链跳过来的可能在很下面 —— 高亮了但看不见，用户以为没打开。
  // 这里在文档切换后把当前行滚进来。

  /** 左栏的滚动容器（日记 / 笔记两个分支同一时刻只挂一个，共用一个引用）。 */
  let listEl = $state<HTMLDivElement | undefined>(undefined);
  /** 已经为哪篇文档展开过祖先目录 —— 保证「展开」只在换文档时发生一次。 */
  let revealedFor = '';

  /** 目标躺在折叠的文件夹里时，把一路上每一层文件夹都展开。
   *  不展开它根本不在 DOM 里，也就谈不上滚动。 */
  function revealInTree(rel: string) {
    const dir = cfg.current?.editor.notesDir ?? '';
    const pre = dir ? `${dir}/` : '';
    if (!rel.startsWith(pre)) return;
    const segs = rel.slice(pre.length).split('/');
    segs.pop(); // 去掉文件名，只留祖先目录
    let acc = dir;
    let changed = false;
    for (const seg of segs) {
      acc = acc ? `${acc}/${seg}` : seg;
      if (!notes.expanded.has(acc)) {
        notes.expanded.add(acc);
        changed = true;
      }
    }
    if (changed) notes.expanded = new Set(notes.expanded);
  }

  $effect(() => {
    const p = doc.path;
    const act = ui.activity;
    // 依赖：目录展开态 / 树本身变了，行的位置就变了，要重滚一次
    void notes.expanded;
    void notes.roots;
    void listEl;
    if (!p || (act !== 'notes' && act !== 'diary')) return;
    // ⚠️ 只在**换文档**时展开祖先。若每次都展开，用户手动折叠当前笔记所在的
    // 文件夹会被立刻顶回去 —— 那是跟用户较劲。
    if (act === 'notes' && revealedFor !== p) {
      revealedFor = p;
      revealInTree(p);
    }
    requestAnimationFrame(() => {
      const el = listEl?.querySelector('.row.on');
      if (el instanceof HTMLElement) el.scrollIntoView({ block: 'nearest' });
    });
  });

  // ───────── 新建笔记 / 文件夹 ─────────

  function openNewMenu(e: MouseEvent, parent: string | null) {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    openCtxMenu(r.left, r.bottom + 4, [
      { label: '新建笔记', icon: 'file', run: () => startCreate('note', parent) },
      { label: '新建文件夹', icon: 'folder', run: () => startCreate('folder', parent) }
    ]);
  }

  function startCreate(kind: 'note' | 'folder', parent: string | null) {
    if (parent) expand(parent);
    newName = '';
    newIn = { kind, parent };
  }

  function cancelNew() {
    newIn = null;
    newName = '';
  }

  async function submitNew() {
    const cur = newIn;
    if (!cur) return;
    const name = newName.trim();
    if (!name) {
      cancelNew();
      return;
    }
    try {
      if (cur.kind === 'folder') {
        const rel = await api.createFolder(cur.parent, name);
        await refreshNotes();
        expand(rel);
        toast(`文件夹「${name}」建好了`, 'ok');
      } else {
        const rel = await api.createNote(name, cur.parent ? notesRel(cur.parent) : undefined);
        await refreshNotes();
        // 新建完就是要接着写，直接给编辑态
        await openDoc(rel, 'edit');
      }
      cancelNew();
    } catch (e) {
      toast(errText(e), 'error');
    }
  }

  async function newDiary() {
    const rel = await api.createDiary(today());
    await refreshDiaries();
    await openDoc(rel, 'edit');
  }

  // ───────── 右键菜单（统一走全局自绘那套） ─────────

  function openFileMenu(e: MouseEvent, n: TreeNode) {
    e.preventDefault();
    openCtxMenu(e.clientX, e.clientY, [
      { label: '打开', icon: 'file', run: () => void openDoc(n.path) },
      { label: '重命名', icon: 'edit', run: () => promptRenameDoc(n.path, n.name) },
      { label: '在资源管理器中显示', icon: 'folder', run: () => void revealPath(n.path) },
      { sep: true },
      { label: '移到回收站', icon: 'trash', danger: true, run: () => void removeDoc(n.path, n.name) }
    ]);
  }

  function openFolderMenu(e: MouseEvent, n: TreeNode) {
    e.preventDefault();
    openCtxMenu(e.clientX, e.clientY, [
      { label: '在此新建笔记', icon: 'file', run: () => startCreate('note', n.path) },
      { label: '在此新建文件夹', icon: 'folder', run: () => startCreate('folder', n.path) },
      { sep: true },
      { label: '重命名', icon: 'edit', run: () => promptRenameFolder(n) },
      { label: '在资源管理器中显示', icon: 'folder', run: () => void revealPath(n.path) },
      { sep: true },
      { label: '删除文件夹', icon: 'trash', danger: true, run: () => void removeFolder(n) }
    ]);
  }

  function openDiaryMenu(e: MouseEvent, m: NoteMeta) {
    e.preventDefault();
    openCtxMenu(e.clientX, e.clientY, [
      { label: '打开', icon: 'diary', run: () => void openDoc(m.path) },
      { label: '重命名', icon: 'edit', run: () => promptRenameDoc(m.path, m.title) },
      { label: '在资源管理器中显示', icon: 'folder', run: () => void revealPath(m.path) },
      { sep: true },
      { label: '移到回收站', icon: 'trash', danger: true, run: () => void removeDoc(m.path, m.title) }
    ]);
  }

  async function revealPath(rel: string) {
    try {
      const abs = await api.revealPath(rel);
      await api.openInExplorer(abs, true);
    } catch (e) {
      toast(errText(e), 'error');
    }
  }

  function promptRenameDoc(rel: string, title: string) {
    promptCtx = {
      label: `重命名「${title}」`,
      value: title,
      ok: async (v) => {
        try {
          const next = await api.renameDoc(rel, v);
          await refreshNotes();
          await refreshDiaries();
          if (doc.path === rel) await openDoc(next);
          toast('已重命名', 'ok');
        } catch (e) {
          toast(errText(e), 'error');
        }
      }
    };
  }

  function promptRenameFolder(n: TreeNode) {
    promptCtx = {
      label: `重命名文件夹「${n.name}」`,
      value: n.name,
      ok: async (v) => {
        try {
          const next = await api.renameFolder(n.path, v);
          // 里面的笔记路径全变了，展开态也跟着挪一下
          if (notes.expanded.delete(n.path)) expand(next);
          await refreshNotes();
          await refreshDiaries();
          toast('已重命名，里面的笔记路径也一起改了', 'ok');
        } catch (e) {
          toast(errText(e), 'error');
        }
      }
    };
  }

  async function removeDoc(rel: string, title: string) {
    try {
      await api.deleteDoc(rel);
      if (doc.path === rel) {
        doc.path = '';
        doc.data = null;
      }
      await refreshNotes();
      await refreshDiaries();
      // 软删除，进 vault/.mnesphere/trash，可以捞回来
      toast(`「${title}」已移到回收站（vault/.mnesphere/trash）`, 'ok');
    } catch (e) {
      toast(errText(e), 'error');
    }
  }

  async function removeFolder(n: TreeNode) {
    try {
      await api.deleteFolder(n.path);
      // 正开着的笔记要是在这个文件夹里，得先关掉，不然后面读的就是幽灵路径
      if (doc.path.startsWith(`${n.path}/`)) {
        doc.path = '';
        doc.data = null;
      }
      await refreshNotes();
      await refreshDiaries();
      toast(`文件夹「${n.name}」连同里面的笔记一起进了回收站`, 'ok');
    } catch (e) {
      toast(errText(e), 'error');
    }
  }

  // ───────── 打卡侧栏 ─────────

  async function newHabit() {
    promptCtx = {
      label: '新建目标',
      value: '',
      ok: async (v) => {
        try {
          await api.addHabit(v);
          await refreshCheckin();
          toast(`已建立目标「${v}」`, 'ok');
        } catch (e) {
          toast(errText(e), 'error');
        }
      }
    };
  }

  async function focusHabit(name: string) {
    checkin.focus = name;
    checkin.stats = name === 'all' ? null : await api.habitStats(name);
  }
</script>

<aside class="panel">
  {#if ui.activity === 'diary'}
    <div class="head">
      <span class="title">日记</span>
      <button class="mini" title="写今天这一篇" onclick={newDiary}>
        <Icon name="plus" size={14} />
      </button>
    </div>
    <div class="filter">
      <input class="field" bind:value={filter} placeholder="过滤日期 / 标题 / 标签…" />
    </div>
    <div class="scroll list" bind:this={listEl}>
      {#each filteredDiaries as m (m.month)}
        <div class="group">
          <div class="gh">
            <Icon name="chev-d" size={12} />
            <span>{m.label}</span>
            <b>{m.items.length}</b>
          </div>
          {#each m.items as it (it.path)}
            <button
              class="row"
              class:on={doc.path === it.path}
              onclick={() => openDoc(it.path)}
              oncontextmenu={(e) => openDiaryMenu(e, it)}
            >
              <span class="d">{it.date.slice(8)}</span>
              <span class="nm">{it.title}</span>
              <span class="w">{it.words}</span>
            </button>
          {/each}
        </div>
      {/each}
      {#if !filteredDiaries.length}
        <p class="empty">还没有日记。点右上角 + 写今天这一篇。</p>
      {/if}
    </div>
  {:else if ui.activity === 'notes'}
    <div class="head">
      <span class="title">笔记</span>
      <button class="mini" title="新建笔记 / 文件夹" onclick={(e) => openNewMenu(e, null)}>
        <Icon name="plus" size={14} />
      </button>
    </div>
    <div class="filter">
      <input class="field" bind:value={filter} placeholder="过滤笔记…" />
    </div>
    <div class="scroll list" bind:this={listEl}>
      {#if newIn && newIn.parent === null}
        {@render createForm()}
      {/if}
      {#if treeRoots.length || newIn}
        {@render branch(treeRoots)}
      {:else}
        <p class="empty">
          笔记目录还是空的。点右上角 + 新建一篇，或者在右键菜单里先建个文件夹。
        </p>
      {/if}
    </div>
  {:else if ui.activity === 'checkin'}
    <div class="head">
      <span class="title">打卡</span>
      <button class="mini" title="新建目标" onclick={newHabit}>
        <Icon name="plus" size={14} />
      </button>
    </div>
    <div class="scroll list">
      <button class="row habit" class:on={checkin.focus === 'all'} onclick={() => focusHabit('all')}>
        <span class="dotview" style="background: var(--accent)"></span>
        <span class="nm">总览</span>
        <span class="w">{checkin.habits.filter((h) => !h.archived).length}</span>
      </button>
      {#each checkin.habits.filter((h) => !h.archived) as h (h.name)}
        <button
          class="row habit"
          class:on={checkin.focus === h.name}
          onclick={() => focusHabit(h.name)}
        >
          <span class="dotview" style="background: var(--accent-2)"></span>
          <span class="nm">{h.name}</span>
          {#if checkin.overview[h.name]}
            <span class="w">{checkin.overview[h.name].currentStreak}</span>
          {/if}
        </button>
      {/each}
      {#if checkin.habits.some((h) => h.archived)}
        <div class="gh sub">已归档</div>
        {#each checkin.habits.filter((h) => h.archived) as h (h.name)}
          <button class="row habit arch" onclick={() => focusHabit(h.name)}>
            <span class="dotview" style="background: var(--fg-faint)"></span>
            <span class="nm">{h.name}</span>
          </button>
        {/each}
      {/if}
      {#if !checkin.habits.length}
        <p class="empty">还没有目标。点 + 建一个，比如「晨跑 5km」。</p>
      {/if}
    </div>
    <div class="foot">
      <span>{checkin.habits.filter((h) => !h.archived).length} 个进行中</span>
    </div>
  {:else if ui.activity === 'task'}
    <div class="head">
      <span class="title">任务</span>
      <button class="mini" title="新建任务" onclick={focusTaskAdd}>
        <Icon name="plus" size={14} />
      </button>
    </div>
    <!-- 任务列表也进左栏。三组（短期/长期/已完成）保留分组头，
         像主线/支线/传说那样一眼扫完，点标题折叠、点任务行选中。 -->
    <div class="scroll list">
      {#each TASK_VIEWS as v (v.id)}
        <div class="group">
          <button
            class="gh click tgh"
            class:alert={v.id === 'short' && taskCount.overdue > 0}
            title={v.hint ? `${v.label} · ${v.hint}` : v.label}
            onclick={() => toggleTaskGroup(v.id)}
          >
            <Icon name={tasks.collapsed.includes(v.id) ? 'chev-r' : 'chev-d'} size={12} />
            <span>{v.label}</span>
            {#if taskCount[v.id]}<b>{taskCount[v.id]}</b>{/if}
          </button>
          {#if !tasks.collapsed.includes(v.id)}
            <div class="titems">
              {#each groupItems(v.id) as t (rowKey(t))}
                <button
                  class="row trow"
                  class:on={selKey === rowKey(t)}
                  class:done={t.done}
                  title={t.title}
                  onclick={() => selectTask(t)}
                >
                  <span class="nm">{t.title}</span>
                  {#if t.due && !t.done}
                    <span class="w due {dueTone(t, todayStr())}">{dueLabel(t.due, todayStr())}</span>
                  {/if}
                </button>
              {/each}
              {#if !groupItems(v.id).length}
                <p class="tnone">{EMPTY_VIEW[v.id]}</p>
              {/if}
            </div>
          {/if}
        </div>
      {/each}
    </div>
    <div class="foot">
      <span>
        {taskCount.short + taskCount.long} 项待办{#if taskCount.overdue > 0}<b class="over">
            · 逾期 {taskCount.overdue}</b
          >{/if}
      </span>
    </div>
  {:else if ui.activity === 'settings'}
    <div class="head"><span class="title">设置</span></div>
    <div class="scroll list">
      {#each SETTINGS_SECTIONS as s (s.id)}
        <button
          class="row sect"
          class:on={ui.settingsTab === s.id}
          onclick={() => (ui.settingsTab = s.id)}
        >
          <Icon name={s.icon} size={14} />
          <span class="nm">{s.label}</span>
        </button>
      {/each}
    </div>
  {/if}
</aside>

{#snippet branch(nodes: TreeNode[])}
  {#each nodes as n (n.path)}
    {#if n.isDir}
      <div class="group">
        <button
          class="gh click"
          onclick={() => toggleDir(n.path)}
          oncontextmenu={(e) => openFolderMenu(e, n)}
          title={n.path}
        >
          <Icon name={notes.expanded.has(n.path) ? 'chev-d' : 'chev-r'} size={12} />
          <Icon name="folder" size={13} />
          <span>{n.name}</span>
          {#if n.words}<b>{n.words}</b>{/if}
        </button>
        {#if notes.expanded.has(n.path)}
          <div class="indent">
            {#if newIn && newIn.parent === n.path}
              {@render createForm()}
            {/if}
            {@render branch(n.children)}
          </div>
        {/if}
      </div>
    {:else}
      <button
        class="row file"
        class:on={doc.path === n.path}
        onclick={() => openDoc(n.path)}
        oncontextmenu={(e) => openFileMenu(e, n)}
        title={n.path}
      >
        <Icon name="file" size={13} />
        <span class="nm">{n.name}</span>
        <span class="w">{n.words}</span>
      </button>
    {/if}
  {/each}
{/snippet}

{#snippet createForm()}
  <div class="create">
    <!-- 新建时输入框立刻要能打字，autofocus 是刻意的 -->
    <!-- svelte-ignore a11y_autofocus -->
    <input
      class="field"
      bind:value={newName}
      autofocus
      placeholder={newIn?.kind === 'folder' ? '文件夹名' : '笔记标题'}
      onkeydown={(e) => {
        if (e.key === 'Enter') submitNew();
        if (e.key === 'Escape') cancelNew();
      }}
    />
    <div class="cbtns">
      <button class="btn sm" onclick={cancelNew}>取消</button>
      <button class="btn sm primary" onclick={submitNew}>
        {newIn?.kind === 'folder' ? '建文件夹' : '建笔记'}
      </button>
    </div>
  </div>
{/snippet}

{#if promptCtx}
  <div class="modal-back" role="presentation" onclick={() => (promptCtx = null)}></div>
  <div class="modal">
    <p>{promptCtx.label}</p>
    <!-- 模态框只有这一个输入项，autofocus 是刻意的 -->
    <!-- svelte-ignore a11y_autofocus -->
    <input
      class="field"
      autofocus
      bind:value={promptCtx.value}
      onkeydown={(e) => {
        if (e.key === 'Enter') {
          const v = promptCtx!.value.trim();
          if (v) promptCtx!.ok(v);
          promptCtx = null;
        }
        if (e.key === 'Escape') promptCtx = null;
      }}
    />
    <div class="mbtns">
      <button class="btn sm" onclick={() => (promptCtx = null)}>取消</button>
      <button
        class="btn sm primary"
        onclick={() => {
          const v = promptCtx!.value.trim();
          if (v) promptCtx!.ok(v);
          promptCtx = null;
        }}>确定</button
      >
    </div>
  </div>
{/if}

<style>
  .panel {
    position: relative;
    z-index: 2;
    display: flex;
    flex-direction: column;
    width: var(--panel-w);
    flex: 0 0 var(--panel-w);
    background: var(--panel);
    backdrop-filter: blur(var(--glass-blur, 0px));
    border-right: 1px solid var(--line);
    min-height: 0;
    /* 左栏的字号基准。下面各处一律写成 em，所以只要动这一个变量，
       标题 / 分组 / 文件行 / 角标会按同一个比例一起缩放 ——
       不会出现「只把文件行调大了、分组标题还是原样」这种散架感。 */
    font-size: var(--panel-font, 12.5px);
  }

  /* 过滤框和新建表单也跟着走，不要用 app.css 里 .field 的 12.5px */
  .panel .field {
    font-size: 1em;
  }

  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px 12px 8px;
  }
  .title {
    font-size: 1.04em;
    font-weight: 500;
    color: var(--fg);
  }
  .mini {
    display: grid;
    place-items: center;
    width: 24px;
    height: 24px;
    border-radius: 7px;
    color: var(--fg-mute);
  }
  .mini:hover {
    background: var(--hover);
    color: var(--accent);
  }

  .filter {
    padding: 0 10px 8px;
  }

  .create {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 2px 4px 8px;
  }
  .cbtns {
    display: flex;
    gap: 6px;
    justify-content: flex-end;
  }

  .list {
    flex: 1;
    padding: 0 6px 12px;
    min-height: 0;
  }

  .group {
    margin-bottom: 4px;
  }

  .gh {
    display: flex;
    align-items: center;
    gap: 5px;
    width: 100%;
    padding: 5px 8px;
    color: var(--fg-mute);
    font-size: 0.92em;
    border-radius: 6px;
  }
  .gh.click:hover {
    background: var(--hover);
  }
  .gh.sub {
    color: var(--fg-faint);
    padding-left: 10px;
  }
  /* 任务分组头（短期/长期/已完成）：可折叠，所以整行可点 */
  .gh.tgh {
    padding: 6px 8px;
    margin-top: 2px;
  }
  .gh.tgh.alert {
    color: var(--danger);
  }
  .gh.tgh.alert b {
    color: var(--danger);
  }
  .gh span {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    text-align: left;
  }
  .gh b {
    font-weight: 400;
    color: var(--fg-faint);
    font-family: var(--font-mono);
  }

  /* 任务行缩进在分组头下面。缩进量要小 —— 上面还有 chev，再往里推
     就没多少地方留给标题了。 */
  .titems {
    padding-left: 4px;
    margin-bottom: 2px;
  }
  /* 分组下一件任务都没有的时候给一行灰字，不要留一段空白让人以为坏了 */
  .tnone {
    padding: 3px 10px 5px;
    font-size: 0.9em;
    color: var(--fg-faint);
  }

  .row.trow {
    gap: 6px;
    padding: 4px 8px;
  }
  .row.trow.done .nm {
    text-decoration: line-through;
    color: var(--fg-faint);
  }
  /* 紧急度那一列：文案短（今天 / 逾期 3 天 / 9月28日），
     给个最小宽度避免每行右边界参差。 */
  .row.trow .due {
    min-width: 3.2em;
    text-align: right;
  }
  .row.trow .due.soon {
    color: var(--accent);
  }
  .row.trow .due.over {
    color: var(--danger);
  }

  .indent {
    padding-left: 10px;
    border-left: 1px solid var(--line);
    margin-left: 10px;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 7px;
    width: 100%;
    padding: 5px 8px;
    border-radius: 7px;
    color: var(--fg-dim);
    /* 字号继承 .panel 的 --panel-font，不再写死 */
    text-align: left;
    transition: background 0.1s, color 0.1s;
  }
  .row:hover {
    background: var(--hover);
    color: var(--fg);
  }
  .row.on {
    background: var(--accent-soft);
    color: var(--accent);
  }
  .row .nm {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .row .d,
  .row .w {
    font-size: 0.84em;
    color: var(--fg-faint);
    font-family: var(--font-mono);
    flex: 0 0 auto;
  }
  .row.on .d,
  .row.on .w {
    color: var(--accent);
    opacity: 0.65;
  }
  .row.file {
    padding-left: 10px;
  }
  /* 设置页左栏的分区导航：比普通行稍高一点，扫读更舒服 */
  .row.sect {
    padding: 7px 9px;
    margin-bottom: 2px;
  }
  .row.habit .dotview {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    flex: 0 0 auto;
  }
  .row.arch .nm {
    color: var(--fg-faint);
    text-decoration: line-through;
  }
  /* 逾期是唯一「必须现在处理」的信号，值得在左栏就红出来。
     任务分组头的 .alert 在上面；这里这个 .row.alert 已经没有调用方了 ——
     任务行的紧急度是靠 .due.over 标红的，比整行染红更准。 */
  .gh.tgh.alert .nm {
    color: var(--danger);
  }
  .foot .over {
    color: var(--danger);
    font-weight: 400;
  }

  .empty {
    padding: 22px 14px;
    color: var(--fg-faint);
    font-size: 0.96em;
    line-height: 1.7;
    text-align: center;
  }

  .foot {
    padding: 8px 14px;
    border-top: 1px solid var(--line);
    color: var(--fg-faint);
    font-size: 0.88em;
  }

  .modal-back {
    position: fixed;
    inset: 0;
    z-index: 90;
    background: rgba(0, 0, 0, 0.32);
  }
  .modal {
    position: fixed;
    z-index: 91;
    top: 22%;
    left: 50%;
    transform: translateX(-50%);
    width: 340px;
    padding: 16px;
    border-radius: var(--radius);
    background: var(--surface);
    border: 1px solid var(--line-2);
    backdrop-filter: blur(24px);
    box-shadow: 0 18px 48px rgba(0, 0, 0, 0.45);
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
</style>
