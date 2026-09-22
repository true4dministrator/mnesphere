/** 任务的展示层逻辑：分视图、分组、排序、日期文案。
 *
 *  左栏（Panel）和任务页（TaskView）都要用同一套口径 —— 左栏那个「逾期 3」的角标
 *  必须和任务页里逾期分组的条数一模一样。所以这块单独拎出来，两边 import 同一份，
 *  绝不在组件里各算各的。 */

import type { Task, TaskPriority } from './api';

export const PRIORITY_LABEL: Record<TaskPriority, string> = {
  high: '高',
  normal: '中',
  low: '低'
};

const PRIORITY_ORDER: Record<TaskPriority, number> = { high: 0, normal: 1, low: 2 };

/**
 * 左栏只有三个视图。口径是「什么时候要动手」，不再按日期细分：
 * - short 短期：一周内到期（含已逾期）—— 现在就该看的
 * - long  长期：一周以后到期 + 没排期的 —— 记下来了，先放着
 * - done  已完成
 *
 * 视图内部还有次级分组（逾期 / 一周内 / 以后 / 未排期），那只是页面里的标题，
 * 不是导航项 —— 别再往左栏加回去了。
 */
export type TaskView = 'short' | 'long' | 'done';

export const TASK_VIEWS: { id: TaskView; label: string; hint: string }[] = [
  { id: 'short', label: '短期', hint: '一周内' },
  { id: 'long', label: '长期', hint: '一周后 · 未排期' },
  { id: 'done', label: '已完成', hint: '' }
];

/** 「一周内」= 从今天起算的滚动 7 天（含今天），不是自然周。
 *  任务是要动手做的事，按「还剩几天」算比按「这周日之前」算更贴题。 */
const SHORT_DAYS = 7;

export type GroupTone = 'danger' | 'accent' | 'mute';

export interface TaskGroup {
  id: string;
  label: string;
  tone: GroupTone;
  items: Task[];
}

export function todayStr(): string {
  return new Date().toLocaleDateString('sv-SE');
}

/** 日期字符串加减天数。`YYYY-MM-DD` 后补本地零点再交给 Date，不会有时区偏移。 */
export function shiftDays(iso: string, days: number): string {
  const d = new Date(`${iso}T00:00:00`);
  d.setDate(d.getDate() + days);
  return d.toLocaleDateString('sv-SE');
}

/** 短期视图的上界（含）。 */
export function shortEnd(today = todayStr()): string {
  return shiftDays(today, SHORT_DAYS - 1);
}

const byPlan = (a: Task, b: Task) =>
  PRIORITY_ORDER[a.priority] - PRIORITY_ORDER[b.priority] ||
  (a.due || '9999-99-99').localeCompare(b.due || '9999-99-99') ||
  a.line - b.line;

/** 已完成按「越靠文件末尾越新」倒序 —— 回顾时关心的是顺序，不是优先级。 */
const byRecent = (a: Task, b: Task) => b.line - a.line;

/**
 * 紧急度权重。**这是「现在该做哪一条」的唯一口径** ——
 * 右半边自动推荐的那条、左栏里排在最上面的那条，都走这里，不许各算各的。
 *
 * 为什么不是单纯的「按日期排」：一条「高优先级 + 没排期」的事，
 * 比一条「低优先级 + 三天后」的事更该先动手。日期只在同一档里比。
 *
 * 0 逾期 > 1 今天 > 2 高优先级 > 3 一周内 > 4 未排期 > 5 更远 / 低优先级
 */
export function urgencyRank(t: Task, today = todayStr(), end = shortEnd(today)): number {
  if (t.due && t.due < today) return 0;
  if (t.due === today) return 1;
  if (t.priority === 'high') return 2;
  if (t.due && t.due <= end) return 3;
  if (!t.due) return 4;
  return 5;
}

/** 同档内怎么比：逾期/一周内按日期近的优先，未排期/更远的按优先级再按行号。 */
export function byUrgency(a: Task, b: Task, today = todayStr(), end = shortEnd(today)): number {
  return (
    urgencyRank(a, today, end) - urgencyRank(b, today, end) ||
    (a.due || '9999-99-99').localeCompare(b.due || '9999-99-99') ||
    PRIORITY_ORDER[a.priority] - PRIORITY_ORDER[b.priority] ||
    a.line - b.line
  );
}

/**
 * 当前视野里最该做的那一条 —— 「追踪」目标。
 *
 * 只在一件事上投降：这个视图里全是已完成的，那就没有「该做」可言，返回 null。
 */
export function pickUrgent(items: Task[], view: TaskView, today = todayStr()): Task | null {
  if (view === 'done') return null;
  const end = shortEnd(today);
  const open = items.filter((t) => !t.done);
  if (!open.length) return null;
  return [...open].sort((a, b) => byUrgency(a, b, today, end))[0];
}

/**
 * 把一个「选中凭据」还原成当前清单里的那一条。
 *
 * 不能只认 raw：`raw` 里含勾选状态（`- [ ] x` / `- [x] x`），
 * 一按「完成」raw 就变了，选中会当场断掉、右边闪一下空。
 * 也不能只认行号：用户手改文件、插一行，行号就漂了。
 *
 * 所以四步退化：raw 精确 → 「标题 + 行号」→「标题 + 唯一的同标题项」→ 放弃。
 * 命中时把最新的 raw / line 一并回吐，让调用方把凭据刷新一遍。
 */
export function resolveSel(
  items: Task[],
  selRaw: string,
  selHint: number,
  selTitle = ''
): Task | null {
  if (!selRaw && !selTitle) return null;

  if (selRaw) {
    const exact = items.find((t) => t.raw === selRaw);
    if (exact) return exact;
  }

  if (!selTitle) return null;

  // 标题 + 原行号：勾选状态变了但行没挪，这一步就能接住
  const near = items.find((t) => t.title === selTitle && t.line === selHint);
  if (near) return near;

  // 只剩一条同名的时候也认（另一种勾选态）。两条同名就宁可不猜，
  // 猜错了用户会以为软件自己跳了 —— 那比显示空态更糟。
  const same = items.filter((t) => t.title === selTitle);
  return same.length === 1 ? same[0] : null;
}

function bucket(
  id: string,
  label: string,
  tone: GroupTone,
  list: Task[],
  cmp: (a: Task, b: Task) => number = byPlan
): TaskGroup {
  return { id, label, tone, items: [...list].sort(cmp) };
}

/**
 * 一个视图里的分组。空分组照样返回 —— 渲染不渲染由调用方定，
 * 但左栏角标要的是「这个视图有几条」这个完整数字，不能提前被过滤掉。
 */
export function tasksInView(items: Task[], view: TaskView, today = todayStr()): TaskGroup[] {
  if (view === 'done') {
    return [bucket('done', '已完成', 'mute', items.filter((t) => t.done), byRecent)];
  }

  const open = items.filter((t) => !t.done);
  const end = shortEnd(today);

  if (view === 'short') {
    return [
      bucket('overdue', '逾期', 'danger', open.filter((t) => t.due && t.due < today)),
      bucket(
        'soon',
        '一周内',
        'accent',
        open.filter((t) => t.due && t.due >= today && t.due <= end)
      )
    ];
  }

  // long：一周以后 + 没排期。没排期的没有「什么时候」可比，只能归到长期 ——
  // 不然它会掉出三个视图之外，那样记下来的任务就再也找不到了。
  return [
    bucket('later', '以后', 'mute', open.filter((t) => t.due && t.due > end)),
    bucket('none', '未排期', 'mute', open.filter((t) => !t.due))
  ];
}

export interface TaskCounts {
  short: number;
  long: number;
  done: number;
  /** 短期里已逾期的那几条 —— 左栏和标题都要单独标红 */
  overdue: number;
}

/** 每个视图的条数，给左栏角标用。跟 tasksInView 同一套判定，别各写一份。 */
export function taskCounts(items: Task[], today = todayStr()): TaskCounts {
  const end = shortEnd(today);
  let short = 0;
  let long = 0;
  let done = 0;
  let overdue = 0;
  for (const t of items) {
    if (t.done) {
      done += 1;
      continue;
    }
    if (t.due && t.due < today) {
      overdue += 1;
      short += 1;
    } else if (t.due && t.due <= end) {
      short += 1;
    } else {
      long += 1;
    }
  }
  return { short, long, done, overdue };
}

/** 截止日的短文案。今天/明天这种相对说法比日期好读，超过一周才退回绝对日期。 */
export function dueLabel(due: string, today = todayStr()): string {
  if (!due) return '';
  const d = new Date(`${due}T00:00:00`);
  const t = new Date(`${today}T00:00:00`);
  if (Number.isNaN(d.getTime())) return due;
  const diff = Math.round((d.getTime() - t.getTime()) / 86400000);
  if (diff === 0) return '今天';
  if (diff === 1) return '明天';
  if (diff === -1) return '昨天';
  if (diff > 1 && diff <= 7) return `${diff} 天后`;
  if (diff < -1 && diff >= -60) return `逾期 ${-diff} 天`;
  return `${d.getMonth() + 1}月${d.getDate()}日`;
}

export type DueTone = '' | 'soon' | 'over';

/** 截止日的紧迫度：只给「今天」和「逾期」上色，其余保持安静。 */
export function dueTone(t: Task, today = todayStr()): DueTone {
  if (t.done || !t.due) return '';
  if (t.due < today) return 'over';
  if (t.due === today) return 'soon';
  return '';
}

/**
 * 距离截止日还有几天。**负数 = 已逾期几天**，`null` = 没排期。
 *
 * 用本地零点算，别用 `Date.now()` 相减再除 —— 那会把「今天下午 3 点」
 * 和「明天零点」只差 9 小时算成 0 天，而用户心里那是「明天」。
 */
export function daysLeft(due: string, today = todayStr()): number | null {
  if (!due) return null;
  const d = new Date(`${due}T00:00:00`);
  const t = new Date(`${today}T00:00:00`);
  if (Number.isNaN(d.getTime()) || Number.isNaN(t.getTime())) return null;
  return Math.round((d.getTime() - t.getTime()) / 86400000);
}

/**
 * 详情页那句做大号的「时间感」文案。
 *
 * 人读「还剩 5 天」比读「9月25日」有用得多 —— 日期本身另起一行给绝对值，
 * 这里只负责「还有多久」。所以它比 dueLabel 更直白，也更适合放大。
 */
export function leftLabel(due: string, today = todayStr()): string {
  const n = daysLeft(due, today);
  if (n === null) return '未排期';
  if (n === 0) return '今天到期';
  if (n === 1) return '明天到期';
  if (n === 2) return '后天到期';
  if (n < 0) return `已逾期 ${-n} 天`;
  return `还剩 ${n} 天`;
}

/**
 * 详情页下半段「接下来」用的：同一视图里、**当前这条之后**的若干条。
 *
 * 为什么只取后面的：这一段的作用是给你「往下走」的方向感，
 * 已经翻过去的那几条只会在下面制造噪音。当前这条自身也排除掉。
 *
 * 传入的 current 按 raw + line 比对 —— 跟 resolveSel 同一套凭据口径。
 */
export function nextUp(
  items: Task[],
  view: TaskView,
  current: Task | null,
  limit = 6,
  today = todayStr()
): Task[] {
  if (view === 'done') {
    // 已完成视图没有「接下来」，往下都是更早完成的
    return [];
  }
  const flat = tasksInView(items, view, today).flatMap((g) => g.items);
  let start = 0;
  if (current) {
    const i = flat.findIndex((t) => t.raw === current.raw && t.line === current.line);
    // 找不到当前这条（刚被改过）就从头上给，总比空着强
    start = i >= 0 ? i + 1 : 0;
  }
  return flat.slice(start, start + limit);
}


const PRI_WORDS: Record<string, TaskPriority> = {
  高: 'high',
  中: 'normal',
  低: 'low',
  high: 'high',
  normal: 'normal',
  low: 'low'
};

/** 输入框里能打的相对日期。界面上不再摆这几个快捷按钮了（直接选日期更省事），
 *  但键盘上顺手打 `@明天` 依旧认 —— 这是藏起来的语法糖，不占任何位置。 */
const REL_DAYS: Record<string, number> = { 今天: 0, 明天: 1, 后天: 2 };

/**
 * 从输入框里就地抽 `@日期` 和 `!优先级`，跟文件里那一行的语法是同一套。
 *
 * 这样录任务时手不用离开键盘：`交材料 @2026-09-22 !高` 回车就完事。
 */
export function parseInline(s: string): { title: string; due: string; priority: TaskPriority } {
  let title = s;
  let due = '';

  const abs = /@(\d{4}-\d{2}-\d{2})/.exec(title);
  if (abs) {
    due = abs[1];
    title = title.replace(/@\d{4}-\d{2}-\d{2}/g, '');
  } else {
    const rel = /@(今天|明天|后天)/.exec(title);
    if (rel) {
      due = shiftDays(todayStr(), REL_DAYS[rel[1]] ?? 0);
      title = title.replace(/@(今天|明天|后天)/g, '');
    }
  }

  let priority: TaskPriority = 'normal';
  const pm = /!\s*(高|中|低|high|normal|low)/.exec(title);
  if (pm) {
    priority = PRI_WORDS[pm[1]];
    title = title.replace(/!\s*(高|中|低|high|normal|low)/g, '');
  }

  return { title: title.replace(/\s+/g, ' ').trim(), due, priority };
}
