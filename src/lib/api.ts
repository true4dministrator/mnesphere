/** Rust 侧命令的类型化封装。所有跨语言调用都从这里走，前端别的地方不直接 invoke。 */

import { invoke, Channel } from '@tauri-apps/api/core';
import type { Theme } from './theme';

// ───────── 数据类型 ─────────

export interface NoteMeta {
  path: string;
  stem: string;
  title: string;
  kind: 'diary' | 'note';
  date: string;
  tags: string[];
  links: string[];
  words: number;
  mtime: number;
}

export interface TreeNode {
  name: string;
  path: string;
  isDir: boolean;
  children: TreeNode[];
  date: string;
  words: number;
  /** 文件最后修改时间（Unix 秒）；**目录恒为 0**（目录永远按名字排） */
  mtime: number;
}

export interface DiaryMonth {
  month: string;
  label: string;
  items: NoteMeta[];
}

export interface DocContent {
  path: string;
  stem: string;
  title: string;
  kind: 'diary' | 'note';
  date: string;
  frontmatter: Record<string, unknown> | null;
  body: string;
  content: string;
  tags: string[];
  links: string[];
  backlinks: NoteMeta[];
  words: number;
  mtime: number;
}

export interface SearchHit {
  meta: NoteMeta;
  snippet: string;
  score: number;
}

export interface IndexStats {
  notes: number;
  diaries: number;
  words: number;
  links: number;
  orphans: number;
}

export interface Habit {
  name: string;
  color: string;
  created: string;
  note: string;
  archived: boolean;
}

// ───────── 任务 ─────────

export type TaskPriority = 'high' | 'normal' | 'low';

/** 一条任务。`raw` + `line` 是写回时的定位凭据：`raw` 是原文，`line` 只是提示，
 *  后端不认死行号 —— 详情见 task.rs 的 locate()。 */
export interface Task {
  line: number;
  raw: string;
  span: number;
  title: string;
  due: string;
  priority: TaskPriority;
  note: string;
  done: boolean;
}

/* 左栏的任务视图（短期 / 长期 / 已完成）不走 IPC，定义在 tasks.ts 里 —— 它是纯前端口径。 */

export type CheckState = '' | 'done' | 'missed';

export interface DayCell {
  date: string;
  day: number;
  weekday: number;
  states: Record<string, CheckState>;
  future: boolean;
  today: boolean;
}

export interface Counts {
  done: number;
  missed: number;
  none: number;
  rate: number;
}

export interface MonthView {
  month: string;
  label: string;
  habits: Habit[];
  days: DayCell[];
  summary: Record<string, Counts>;
}

export interface WeekBar {
  label: string;
  done: number;
  total: number;
  rate: number;
}

export interface HabitStats {
  habit: string;
  currentStreak: number;
  longestStreak: number;
  done: number;
  missed: number;
  none: number;
  rate: number;
  weeks: WeekBar[];
  firstDate: string;
  totalDays: number;
}

export interface AiConfig {
  baseUrl: string;
  model: string;
  temperature: number;
  maxRefs: number;
  systemPrompt: string;
}

export interface GithubConfig {
  repo: string;
  branch: string;
  lastSync: string;
  lastCommit: string;
}

export interface Behavior {
  closeToTray: boolean;
  autostart: boolean;
  startHidden: boolean;
  hidePanelWhenAiOpen: boolean;
  /** 启动后默认停在哪个模块 */
  defaultActivity: 'notes' | 'diary' | 'checkin' | 'task';
  /** 启动时自动创建并打开「今天」那篇日记 */
  autoCreateDiary: boolean;
  /** 左侧笔记树的排序方式 */
  noteSort: 'name' | 'mtime';
}

export interface EditorPrefs {
  defaultMode: 'read' | 'edit';
  autosaveMs: number;
  diaryDir: string;
  notesDir: string;
  checkinDir: string;
  taskDir: string;
  attachmentsDir: string;
  diaryTemplate: string;
}

export interface AppConfig {
  vault: string;
  theme: Theme;
  ai: AiConfig;
  github: GithubConfig;
  behavior: Behavior;
  editor: EditorPrefs;
  onboarded: boolean;
}

export interface ConflictItem {
  /** 冲突的文件 —— 本地这份原封不动 */
  path: string;
  /** 远端那份被另存到了哪儿 */
  savedAs: string;
}

export interface SyncReport {
  /** 从远端写进本地的 */
  pulled: string[];
  /** 远端删过、本地没动过 → 本地跟着删掉的 */
  localDeleted: string[];
  /** 本地 → 远端 */
  pushed: string[];
  /** 本地删过、远端没动过 → 远端跟着删掉的 */
  deleted: string[];
  /** 两边都改了。本地原件未动，远端那份在 savedAs */
  conflicts: ConflictItem[];
  unchanged: number;
  skipped: string[];
  commit: string;
  url: string;
  branch: string;
  message: string;
}

export interface RepoInfo {
  fullName: string;
  defaultBranch: string;
  private: boolean;
  htmlUrl: string;
  /** 仓库存在但一个提交都没有 —— 首次同步会自动建首个提交，不是错误 */
  empty: boolean;
}

export interface AccountInfo {
  login: string;
  name: string;
  /** Token 带的 scope；fine-grained token 拿不到这个头，会是空数组 */
  scopes: string[];
}

export interface GhStatus {
  configured: boolean;
  /** 自上次同步以来 vault 里有东西动过 */
  dirty: boolean;
  repo: string;
  url: string;
  branch: string;
  lastSync: string;
  lastCommit: string;
}

export interface AppInfo {
  version: string;
  platform: string;
  configPath: string;
  vault: string;
}

export type AiEvent =
  | { type: 'delta'; text: string }
  | { type: 'reasoning'; text: string }
  | { type: 'done'; chars: number }
  | { type: 'error'; message: string };

export interface ChatMessage {
  role: 'user' | 'assistant' | 'system';
  content: string;
}

// ───────── 系统 ─────────

export const appInfo = () => invoke<AppInfo>('app_info');
export const rebuildIndex = () => invoke<number>('rebuild_index');
export const hideWindow = () => invoke<void>('hide_window');
export const quitApp = () => invoke<void>('quit_app');
export const setAutostart = (enabled: boolean) => invoke<void>('set_autostart', { enabled });
export const autostartEnabled = () => invoke<boolean>('autostart_enabled');

// ───────── 配置 ─────────

export const getConfig = () => invoke<AppConfig>('get_config');
export const setConfig = (config: AppConfig) => invoke<AppConfig>('set_config', { config });
export const themePresets = () => invoke<PresetInfoLike[]>('theme_presets');
export interface PresetInfoLike {
  id: string;
  accent: string;
  accent2: string;
  bg: string;
  fg: string;
}
export const pickWallpaper = () => invoke<string | null>('pick_wallpaper');
export const pickDirectory = () => invoke<string | null>('pick_directory');
export const pathExists = (p: string) => invoke<boolean>('path_exists', { p });

// ───────── 密钥（存在系统凭据管理器，永不落盘进 vault） ─────────

export const secretSet = (name: string, value: string) => invoke<void>('secret_set', { name, value });
export const secretHas = (name: string) => invoke<boolean>('secret_has', { name });
export const secretClear = (name: string) => invoke<void>('secret_clear', { name });

// ───────── 文件 ─────────

export const openInExplorer = (path: string, reveal = false) =>
  invoke<void>('open_in_explorer', { path, reveal });
export const revealPath = (rel: string) => invoke<string>('reveal_path', { rel });
export const importAttachment = (sources: string[]) =>
  invoke<string[]>('import_attachment', { sources });

/** 粘贴图片：把原始字节**直接**当 invoke 的第二个参数发过去。
 *  不要包成 `{ bytes }` —— 一旦套上对象，Tauri 就会按 JSON 序列化（字节会膨胀成
 *  数字数组），到了 Rust 侧 `InvokeBody` 是 Json 而不是 Raw，命令直接报错。
 *  裸的 `Uint8Array` 才会让 Tauri 把 content-type 设成 application/octet-stream。 */
export const savePastedImage = (bytes: Uint8Array) =>
  invoke<string>('save_pasted_image', bytes);
export const openExternal = (url: string) => invoke<void>('open_external', { url });

// ───────── 笔记 ─────────

export const listNotesTree = () => invoke<TreeNode[]>('list_notes_tree');
export const listDiaries = () => invoke<DiaryMonth[]>('list_diaries');
export const readDoc = (rel: string) => invoke<DocContent>('read_doc', { rel });
export const writeDoc = (rel: string, content: string) =>
  invoke<NoteMeta>('write_doc', { rel, content });
export const updateDiaryMeta = (rel: string, key: string, value: string) =>
  invoke<DocContent>('update_diary_meta', { rel, key, value });
export const createNote = (title: string, folder?: string) =>
  invoke<string>('create_note', { title, folder: folder ?? null });
export const createDiary = (date: string) => invoke<string>('create_diary', { date });
export const deleteDoc = (rel: string) => invoke<void>('delete_doc', { rel });
export const renameDoc = (rel: string, newTitle: string) =>
  invoke<string>('rename_doc', { rel, newTitle });
export const createFolder = (parent: string | null, name: string) =>
  invoke<string>('create_folder', { parent, name });
export const renameFolder = (rel: string, newName: string) =>
  invoke<string>('rename_folder', { rel, newName });
export const deleteFolder = (rel: string) => invoke<void>('delete_folder', { rel });
export const searchNotes = (query: string) => invoke<SearchHit[]>('search_notes', { query });
export const resolveLink = (target: string) => invoke<string | null>('resolve_link', { target });
export const allTags = () => invoke<[string, number][]>('all_tags');
export const indexStats = () => invoke<IndexStats>('index_stats');

// ───────── 打卡 ─────────

export const listHabits = () => invoke<Habit[]>('list_habits');
export const addHabit = (name: string, color?: string, note?: string) =>
  invoke<Habit[]>('add_habit', { name, color: color ?? null, note: note ?? null });
export const updateHabit = (
  oldName: string,
  patch: { name?: string; color?: string; note?: string; archived?: boolean }
) =>
  invoke<Habit[]>('update_habit', {
    oldName,
    name: patch.name ?? null,
    color: patch.color ?? null,
    note: patch.note ?? null,
    archived: patch.archived ?? null
  });
export const deleteHabit = (name: string) => invoke<Habit[]>('delete_habit', { name });
export const getMonth = (month: string) => invoke<MonthView>('get_month', { month });
export const setCheckin = (habit: string, date: string, value: CheckState) =>
  invoke<void>('set_checkin', { habit, date, value });
export const cycleCheckin = (habit: string, date: string) =>
  invoke<CheckState>('cycle_checkin', { habit, date });
export const habitStats = (habit: string) => invoke<HabitStats>('habit_stats', { habit });
export const todayBoard = () => invoke<MonthView>('today_board');
export const allHabitsOverview = () =>
  invoke<Record<string, HabitStats>>('all_habits_overview');

// ───────── 任务 ─────────

export const listTasks = () => invoke<Task[]>('list_tasks');
export const addTask = (
  title: string,
  opts: { due?: string; priority?: TaskPriority; note?: string } = {}
) =>
  invoke<Task[]>('add_task', {
    title,
    due: opts.due || null,
    priority: opts.priority ?? null,
    note: opts.note || null
  });
/** 只传要改的字段，其余保持原样。`raw` / `hint` 用来在磁盘上重新找到这一条。 */
export const updateTask = (
  raw: string,
  hint: number,
  patch: { title?: string; due?: string; priority?: TaskPriority; note?: string; done?: boolean }
) =>
  invoke<Task[]>('update_task', {
    raw,
    hint,
    title: patch.title ?? null,
    due: patch.due ?? null,
    priority: patch.priority ?? null,
    note: patch.note ?? null,
    done: patch.done ?? null
  });
export const deleteTask = (raw: string, hint: number) =>
  invoke<Task[]>('delete_task', { raw, hint });

// ───────── AI ─────────

export function aiChat(
  messages: ChatMessage[],
  refs: string[],
  onEvent: (e: AiEvent) => void
): Promise<void> {
  const channel = new Channel<AiEvent>();
  channel.onmessage = onEvent;
  return invoke<void>('ai_chat', { messages, refs, onEvent: channel });
}

export const aiTest = (baseUrl?: string, model?: string) =>
  invoke<string>('ai_test', { baseUrl: baseUrl ?? null, model: model ?? null });

// ───────── GitHub ─────────

export const githubAccount = () => invoke<AccountInfo>('github_account');
export const githubCreateRepo = (name: string, description?: string) =>
  invoke<RepoInfo>('github_create_repo', { name, description: description ?? null });
export const githubStatus = () => invoke<GhStatus>('github_status');
export const githubTest = (repo: string, branch?: string) =>
  invoke<RepoInfo>('github_test', { repo, branch: branch ?? null });
/**
 * 双向同步：先拉后推。
 *
 * `protect` 是**正在编辑、还没保存**的文档路径 —— 后端对它们只读不写，
 * 免得把内存里刚敲的字用磁盘内容盖掉。
 */
export const githubSync = (protect?: string[], message?: string) =>
  invoke<SyncReport>('github_sync', { protect: protect ?? null, message: message ?? null });

/** 只下载：把远端更新并进本地，不往远端写一个字节 */
export const githubPull = (protect?: string[]) =>
  invoke<SyncReport>('github_pull', { protect: protect ?? null });
