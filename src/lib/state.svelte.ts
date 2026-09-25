/** 全局状态。用 Svelte 5 的 runes，模块顶层 $state 在 .svelte.ts 里是允许的。 */

import * as api from './api';
import type {
  AppConfig,
  DiaryMonth,
  DocContent,
  GhStatus,
  Habit,
  HabitStats,
  IndexStats,
  MonthView,
  NoteMeta,
  SearchHit,
  Task,
  TreeNode
} from './api';
import type { TaskView } from './tasks';
import { resolveSel } from './tasks';
import { sortNotesTree, type NoteSort } from './notesSort';
import { applyTheme, FALLBACK_THEME } from './theme';
import { splitFrontmatter } from './markdown';

export type Activity = 'diary' | 'notes' | 'checkin' | 'task' | 'settings';

/** 设置页的分区。左栏导航与右侧内容靠这一个字段串起来。 */
export type SettingsTab = 'appearance' | 'storage' | 'ai' | 'github' | 'behavior' | 'about';

export const ui = $state({
  activity: 'diary' as Activity,
  panelOpen: true,
  aiOpen: false,
  settingsTab: 'appearance' as SettingsTab,
  ready: false,
  fatal: '',
  toast: null as { text: string; kind: 'info' | 'ok' | 'error' } | null
});

/** 欢迎引导：首次启动（onboarded=false）时铺一层，也能从设置里手动唤回 */
export const welcome = $state({
  open: false,
  step: 0
});

export function openWelcome() {
  welcome.step = 0;
  welcome.open = true;
}

export function closeWelcome() {
  welcome.open = false;
}

export const cfg = $state({
  current: null as AppConfig | null
});

/**
 * GitHub 同步状态。状态栏和设置页都要读，所以挂在全局而不是某个组件里。
 *
 * `dirty`（有没有没推上去的改动）由后端按 mtime 算，这里只负责缓存和拉取时机。
 */
export const gh = $state({
  stat: null as GhStatus | null
});

export const doc = $state({
  path: '',
  data: null as DocContent | null,
  mode: 'edit' as 'read' | 'edit',
  dirty: false,
  saving: false,
  loading: false,
  error: ''
});

export const diaries = $state({
  months: [] as DiaryMonth[]
});

export const notes = $state({
  roots: [] as TreeNode[],
  expanded: new Set<string>(),
  filter: ''
});

/** 顶栏搜索。结果挂在标题栏下方，不再单占一个左侧模块。 */
export const find = $state({
  query: '',
  hits: [] as SearchHit[],
  busy: false,
  open: false,
  index: 0,
  /** 自增一次 = 请求把焦点交给搜索框（Ctrl+K 用） */
  focusToken: 0
});

export function focusSearch() {
  find.focusToken += 1;
}

let findTimer: ReturnType<typeof setTimeout> | null = null;

export function searchNow(q?: string) {
  if (q !== undefined) find.query = q;
  const query = find.query.trim();
  if (findTimer) clearTimeout(findTimer);
  if (!query) {
    find.hits = [];
    find.busy = false;
    return;
  }
  find.busy = true;
  // 打字快的时候没必要每个键都打一次 IPC
  findTimer = setTimeout(async () => {
    try {
      find.hits = await api.searchNotes(query);
      find.index = 0;
    } catch (e) {
      toast(errText(e), 'error');
    } finally {
      find.busy = false;
    }
  }, 140);
}

// ───────── 右键菜单：全局共用一套，风格才统一 ─────────

export interface CtxItem {
  label?: string;
  icon?: string;
  danger?: boolean;
  sep?: boolean;
  disabled?: boolean;
  run?: () => void;
}

export const ctxMenu = $state({
  open: false,
  x: 0,
  y: 0,
  items: [] as CtxItem[]
});

export function openCtxMenu(x: number, y: number, items: CtxItem[]) {
  ctxMenu.items = items;
  ctxMenu.x = x;
  ctxMenu.y = y;
  ctxMenu.open = true;
}

export function closeCtxMenu() {
  ctxMenu.open = false;
}

export const checkin = $state({
  month: '',
  view: null as MonthView | null,
  habits: [] as Habit[],
  // 'all' 表示总览，否则是某个习惯名
  focus: 'all',
  stats: null as HabitStats | null,
  overview: {} as Record<string, HabitStats>
});

export const tasks = $state({
  items: [] as Task[],
  /** 左栏选中的视图：短期 / 长期 / 已完成 */
  view: 'short' as TaskView,

  // ── 下面这些是「任务界面像原神那样」所需的 UI 态，跟文件内容无关 ──

  /**
   * 选中的那条任务。**存原文和标题，不存行号** —— 行号会随手改文件漂掉。
   * 只有 `raw` / `title` 都空才算「没选」，这时右半边走「自动推荐」。
   */
  selRaw: '',
  selHint: -1,
  selTitle: '',
  /**
   * 右半边是不是在「新建」态。点左栏 + 号 = true，
   * 提交或取消都回到 false。
   */
  composing: false,
  /** 折叠起来的视图 id（默认三个全展开，跟原神一样一眼扫完） */
  collapsed: [] as TaskView[]
});

/**
 * 选中一条任务。传空字符串 = 取消选中（右半边回落到自动推荐）。
 * 这是**唯一**改选中的入口，别在组件里直接写 tasks.selRaw。
 */
export function selectTask(t: Task | null) {
  tasks.composing = false;
  if (!t) {
    tasks.selRaw = '';
    tasks.selHint = -1;
    tasks.selTitle = '';
    return;
  }
  tasks.selRaw = t.raw;
  tasks.selHint = t.line;
  tasks.selTitle = t.title;
}

export function toggleTaskGroup(v: TaskView) {
  const i = tasks.collapsed.indexOf(v);
  if (i >= 0) tasks.collapsed.splice(i, 1);
  else tasks.collapsed.push(v);
}

export function focusTaskAdd() {
  ui.activity = 'task';
  ui.panelOpen = true;
  // 现在是「进入新建界面」，不再是「聚焦输入框」——
  // 右半边整块换成表单，所以还要把当前选中让开，免得表单和详情打架。
  selectTask(null);
  tasks.composing = true;
}

export const ai = $state({
  messages: [] as { role: 'user' | 'assistant'; content: string; thinking: string; error?: boolean }[],
  streaming: false,
  /** **只手选**引用。当前页不再往这里塞 —— 它是隐式上下文，见 `aiContextRefs()`。
   *  切页不会冲掉这个列表，新对话也不会（用户勾的东西是有意挂上去的）。 */
  refs: [] as string[],
  /** 这一轮把「当前页」那枚隐式引用手动摘掉了；切页自动复位（见 openDoc）。 */
  currentOff: false,
  hasKey: false,
  input: ''
});

/** 单次对话最多注入多少篇。与 Rust `ai.rs` 同源（`cfg.ai.maxRefs`，默认 5）。 */
export function maxRefs(): number {
  const n = cfg.current?.ai.maxRefs ?? 5;
  return n > 0 ? n : 0;
}

/**
 * 上下文清单（**截断前**）= 当前页（隐式，排第一）+ 手选引用，去重。
 * 只给「是不是超上限了」这类判断用；真正发送/展示走 `aiContextRefs()`。
 */
export function aiContextAll(): string[] {
  const out: string[] = [];
  if (!ai.currentOff && doc.path) out.push(doc.path);
  for (const r of ai.refs) if (r && !out.includes(r)) out.push(r);
  return out;
}

/**
 * 真正发给 AI 的引用清单 = `aiContextAll()` 截到 `maxRefs`。
 *
 * 展示（上下文胶囊）与请求（`aiChat`）**都**走这一个函数，免得「看到的」和
 * 「发出去的」对不上 —— 之前三处各写死一个上限（6 / 8 / 5），就是这个毛病。
 */
export function aiContextRefs(): string[] {
  const cap = maxRefs();
  return cap > 0 ? aiContextAll().slice(0, cap) : [];
}

/** 摘掉 / 唤回「当前页」那枚隐式引用。只影响这一轮，切页自动复位。 */
export function toggleCurrentRef() {
  ai.currentOff = !ai.currentOff;
}

export const stats = $state({
  index: null as IndexStats | null
});

// ───────── 提示 ─────────

let toastTimer: ReturnType<typeof setTimeout> | null = null;
export function toast(text: string, kind: 'info' | 'ok' | 'error' = 'info') {
  ui.toast = { text, kind };
  if (toastTimer) clearTimeout(toastTimer);
  toastTimer = setTimeout(() => (ui.toast = null), kind === 'error' ? 7000 : 3200);
}

export function errText(e: unknown): string {
  if (typeof e === 'string') return e;
  if (e instanceof Error) return e.message;
  return String(e);
}

// ───────── 启动 ─────────

export async function bootstrap() {
  try {
    const c = await api.getConfig();
    cfg.current = c;
    applyTheme(c.theme);
    doc.mode = c.editor.defaultMode === 'read' ? 'read' : 'edit';
    ai.hasKey = await api.secretHas('ai_api_key');
    ui.ready = true;
    // 首次启动（或用户还没走完引导）就铺欢迎页
    if (!c.onboarded) openWelcome();
    await Promise.all([
      refreshDiaries(),
      refreshNotes(),
      refreshCheckin(),
      refreshTasks(),
      refreshStats()
    ]);
    // 启动落点：默认界面（用户可配）+ 「要不要自动建今天的日记」。
    //
    // 0.2.0 之前这里是硬编码的 `openTodayDiary()` —— 打开日记模块**兼**建今天的文件，
    // 两件事绑死。0.2.1 拆成两个开关，但「自动建日记」仍然只在落点是日记时才生效
    // （落点是笔记就不建），跟开关字面意思对不上。
    //
    // 0.2.2 起彻底解耦：**开关为真就无条件建出今天那篇**；落点是不是日记只决定
    // 「要不要把它打开并跳进编辑态」。
    //   · 落点 = 日记 → 建 + 打开 + 进编辑态（打开日记就是要写东西）
    //   · 落点 ≠ 日记 → 只建文件，**不跳过去**，视线留在用户选的模块
    //
    // 白名单兜一道：config.json 是允许手改的，写错一个字母不该让启动停在一个
    // 空白界面上（Rail 会一个都不亮）。不认就回落到「日记」。
    const LANDINGS: Activity[] = ['diary', 'notes', 'checkin', 'task'];
    const landing = LANDINGS.includes(c.behavior.defaultActivity as Activity)
      ? (c.behavior.defaultActivity as Activity)
      : 'diary';
    if (landing === 'diary') {
      ui.activity = 'diary';
      // 开关没开就只是停在日记模块，不替他造文件
      if (c.behavior.autoCreateDiary) await goToTodayDiary();
    } else {
      if (c.behavior.autoCreateDiary) await ensureTodayDiary();
      ui.activity = landing;
    }
  } catch (e) {
    ui.fatal = errText(e);
    ui.ready = true;
  }
}

/** 今天的日期戳（本地时区）。`sv-SE` 的短格式恰好就是 `YYYY-MM-DD`。 */
function todayStamp(): string {
  return new Date().toLocaleDateString('sv-SE');
}

/** 今天那篇日记的 vault 相对路径。日记固定落 `日记/YYYY-MM/YYYY-MM-DD.md`。 */
export function todayDiaryRel(): string {
  const today = todayStamp();
  const dir = cfg.current?.editor.diaryDir ?? '日记';
  return `${dir}/${today.slice(0, 7)}/${today}.md`;
}

/**
 * 建今天那篇日记（幂等）并返回路径 —— **不打开**。
 *
 * `create_diary` 已经存在就原样返回路径，不会覆盖正在写的内容。
 * 默认界面落在别的模块时，「自动建日记」只建文件、不抢视线。
 * 日记列表也顺手刷新一下，左栏当天的格子立刻有。
 */
export async function ensureTodayDiary(): Promise<string | null> {
  try {
    const rel = await api.createDiary(todayStamp());
    await refreshDiaries();
    return rel;
  } catch (e) {
    // 建日记失败不该拖住启动流程，说一声就过
    toast(`今天的日记没建出来：${errText(e)}`, 'error');
    return null;
  }
}

/**
 * 「进今天这篇日记」—— 点 Rail 的日记图标走这里，**从任何状态**都能落到
 * 「今天 + 编辑态」，而不是「已经在日记里就什么也不做」：
 * - 已经在今天的日记里、且是编辑态 → 什么都不做（再点一下不该把光标拽走）
 * - 在今天的日记里但是阅读态 → 只切编辑态，**不重新读盘**（保住内存里未保存的编辑）
 * - 别的笔记 / 别的模块 → 建（若缺）后打开，**强制 edit**
 *
 * 为什么强制 edit：日记正文基本是空的，阅读态在那一刻等于一片白；
 * 打开日记的人十有八九是要写东西。
 */
export async function goToTodayDiary() {
  const today = todayDiaryRel();
  if (doc.path === today) {
    if (doc.mode !== 'edit') doc.mode = 'edit';
    return;
  }
  const rel = await ensureTodayDiary();
  if (rel) await openDoc(rel, 'edit');
}

export async function saveConfig(patch: Partial<AppConfig>) {
  if (!cfg.current) return;
  const next = { ...cfg.current, ...patch } as AppConfig;
  try {
    cfg.current = await api.setConfig(next);
    applyTheme(cfg.current.theme);
  } catch (e) {
    toast(errText(e), 'error');
  }
}

/**
 * 只把后端配置**回读**一遍，不做任何写回。
 *
 * 同步 / 建仓库这类「后端自己会改配置」的操作之后必须调它 —— 否则界面读的还是
 * 内存里那份过期快照（典型症状：「上次同步」永远显示「从未」）。
 *
 * ⚠️ 别图省事拿 `saveConfig({})` 代替：那是**全量提交**，会把后端刚写好的
 * lastSync / lastCommit 用前端的过期值覆盖掉。后端 `set_config` 现在会挡这两
 * 个字段，但仍不该把回读这件事交给一条写路径去做。
 */
export async function reloadConfig() {
  try {
    cfg.current = await api.getConfig();
  } catch (e) {
    toast(errText(e), 'error');
  }
}

/**
 * 拉一次 GitHub 同步状态（配没配 / 有没有未同步的改动 / 上次同步时间）。
 *
 * 刻意**不弹错**：状态栏是个被动显示区，Token 被清掉之类的异常情况让它安静地
 * 空着就好 —— 每次轮询都糊用户一脸红字，比不显示更烦人。
 */
export async function pullGhStatus() {
  try {
    gh.stat = await api.githubStatus();
  } catch {
    gh.stat = null;
  }
}

// ───────── 刷新 ─────────

export async function refreshDiaries() {
  try {
    diaries.months = await api.listDiaries();
    const set = new Set(diaries.months.map((m) => m.month));
    void set;
  } catch (e) {
    toast(errText(e), 'error');
  }
}

/** 当前的笔记排序方式。`config.json` 允许手改，白名单兜一道，不认就回落到「名称」。 */
export function noteSortMode(): NoteSort {
  return cfg.current?.behavior.noteSort === 'mtime' ? 'mtime' : 'name';
}

export async function refreshNotes() {
  try {
    const tree = await api.listNotesTree();
    // 排序在**这里**做一次就够：`notes.roots` 是前端所有「这棵树」视图的唯一入口，
    // 排好它，左栏 / 双链补全 / AI 引用选择器自动同序。
    notes.roots = sortNotesTree(tree, noteSortMode());
  } catch (e) {
    toast(errText(e), 'error');
  }
}

/** 排序方式改了就地重排一次。排序是幂等的（同值时按名字兜底），不用重新扫盘。 */
export function applyNoteSort() {
  notes.roots = sortNotesTree(notes.roots, noteSortMode());
}

export async function refreshCheckin() {
  try {
    checkin.habits = await api.listHabits();
    const m = checkin.month || new Date().toISOString().slice(0, 7);
    checkin.month = m;
    checkin.view = await api.getMonth(m);
    checkin.overview = await api.allHabitsOverview();
    if (checkin.focus !== 'all') {
      checkin.stats = await api.habitStats(checkin.focus);
    }
  } catch (e) {
    toast(errText(e), 'error');
  }
}

export async function refreshStats() {
  try {
    stats.index = await api.indexStats();
  } catch {
    /* 索引统计失败不影响主流程 */
  }
}

export async function refreshTasks() {
  try {
    tasks.items = await api.listTasks();
    // 选中的那条要是被别处删了，凭据就悬空了，右半边会显示幽灵内容。
    // 在这里收口：解析不出来就直接清掉，回落到自动推荐。
    if (tasks.selRaw || tasks.selTitle) {
      const hit = resolveSel(tasks.items, tasks.selRaw, tasks.selHint, tasks.selTitle);
      if (hit) {
        // 勾选态变了之后 raw 会变，顺手把凭据刷新成最新的，
        // 不然下一次刷新又要走一遍退化匹配。
        tasks.selRaw = hit.raw;
        tasks.selHint = hit.line;
        tasks.selTitle = hit.title;
      } else {
        selectTask(null);
      }
    }
  } catch (e) {
    toast(errText(e), 'error');
  }
}

export async function refreshAll() {
  await Promise.all([
    refreshDiaries(),
    refreshNotes(),
    refreshCheckin(),
    refreshTasks(),
    refreshStats()
  ]);
}

// ───────── 文档 ─────────

export async function openDoc(rel: string, forceMode?: 'read' | 'edit') {
  if (!rel) return;
  if (doc.dirty) await flushDoc();
  const prev = doc.path;
  doc.loading = true;
  doc.error = '';
  try {
    const d = await api.readDoc(rel);
    doc.data = d;
    doc.path = rel;
    doc.dirty = false;
    if (forceMode) doc.mode = forceMode;
    else if (cfg.current) doc.mode = cfg.current.editor.defaultMode === 'read' ? 'read' : 'edit';
    // 换了一页 → 允许「当前页」这枚隐式引用重新出现（用户上一页摘掉过是上一页的事）。
    if (prev !== rel) ai.currentOff = false;
    // ⚠️ 这里**不再**把刚打开的笔记塞进 `ai.refs`。以前每开一篇就往前插一篇、
    // 截断到 6 篇，结果手选引用被悄悄冲掉，上下文里全是没要过的笔记。
    // 现在当前页由 `aiContextRefs()` 隐式带上，`ai.refs` 只装用户亲手挂的。
  } catch (e) {
    doc.error = errText(e);
    doc.data = null;
  } finally {
    doc.loading = false;
  }
}

export async function flushDoc() {
  if (!doc.data || !doc.dirty) return;
  cancelAutosave();
  const path = doc.path;
  const content = doc.data.content;
  doc.saving = true;
  try {
    const meta: NoteMeta = await api.writeDoc(path, content);
    if (doc.data && doc.path === path) {
      doc.data.words = meta.words;
      doc.data.title = meta.title;
      doc.data.tags = meta.tags;
      const [stem, ...rest] = path.split('/').reverse();
      void stem;
      void rest;
    }
    doc.dirty = false;
  } catch (e) {
    toast(`保存失败：${errText(e)}`, 'error');
  } finally {
    doc.saving = false;
  }
}

// ───────── 自动保存 ─────────
//
// 之前**根本没有自动保存**：`cfg.editor.autosaveMs` 定义了、配置里也存着 900，
// 但全项目没有任何地方读它 —— flushDoc 只在切文档 / Ctrl+S / beforeunload 时调。
// 而本应用是「关窗进托盘」，页面压根不会 unload，beforeunload 永不触发 →
// **编辑内容静默丢失**（图片粘贴那次就是这样：图存进了附件，链接却没了）。
//
// 这里把它补上：改一个字就重置一个防抖计时器，停手 autosaveMs 之后落盘。
// 配 0 或负数 = 关掉自动保存，退回手动 Ctrl+S（用户控制软件，留个开关）。

let saveTimer: ReturnType<typeof setTimeout> | null = null;

function cancelAutosave() {
  if (saveTimer !== null) {
    clearTimeout(saveTimer);
    saveTimer = null;
  }
}

function scheduleAutosave() {
  cancelAutosave();
  const ms = cfg.current?.editor.autosaveMs ?? 900;
  if (!(ms > 0)) return;
  saveTimer = setTimeout(() => {
    saveTimer = null;
    void flushDoc();
  }, ms);
}

/** 窗口失焦/被隐藏时立刻落盘 —— 防抖窗口内直接点「关闭到托盘」的那次编辑。
 *  没脏内容时 flushDoc 自己会直接返回，所以随便调。 */
export function flushIfDirty() {
  if (doc.dirty) void flushDoc();
}

export function markDirty(content: string) {
  if (!doc.data) return;
  doc.data.content = content;
  // ⚠️ `body` 必须跟着一起更新。阅读态渲染的是 `body` 而不是 `content`
  // （Rust 侧 `parse_doc` 就把 frontmatter 剥掉存进 body）。只改 content 的话，
  // 编辑完切到阅读态看到的还是打开时那份旧内容 —— 图片粘贴「阅读模式也不显示」
  // 的根因就在这里，而且它影响的是**所有**编辑，不只是图片。
  doc.data.body = splitFrontmatter(content).body;
  doc.dirty = true;
  scheduleAutosave();
}

// ───────── AI 上下文 ─────────

/** 手选引用的增删。上限不在这里截 —— 交给 `aiContextRefs()` 统一收口，
 *  否则用户挂到第 6 篇时前一篇会被悄悄挤掉，而他并没有删过它。 */
export function toggleRef(rel: string) {
  if (ai.refs.includes(rel)) ai.refs = ai.refs.filter((r) => r !== rel);
  else ai.refs = [...ai.refs, rel];
}
