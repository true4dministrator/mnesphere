/** 笔记树的排序口径 —— **整个项目里唯一的一份**。
 *
 *  ⚠️ 为什么排序在前端而不是 Rust（`note.rs` 里那个 `sort_tree` 已经删了）：
 *  「中文按拼音排」在前端只要一个 `Intl.Collator`，浏览器原生、零依赖；
 *  而 Rust 要复刻就得拖进一整个拼音库（音表几百 KB），
 *  而且顺带拿不到 `numeric` 这种「第 2 章排在 第 10 章 前面」的能力。
 *  所以口径整体搬到前端，**后端只管把 `mtime` 送出来**，别再出现两套实现。
 *
 *  ⚠️ 前端其它几处「消费这棵树」的地方（点笔记图标打开哪篇、双链补全候选、
 *  AI 抽屉的引用选择器）全都吃 `notes.roots`，所以只要在这一处排好，
 *  它们自动同序 —— 这正是「口径只有一份」的好处。 */

import type { TreeNode } from './api';

export type NoteSort = 'name' | 'mtime';

/** 排序方式的中文名，供设置页与提示复用。 */
export const NOTE_SORT_LABEL: Record<NoteSort, string> = {
  name: '名称',
  mtime: '修改时间'
};

/**
 * 排序器建**一次**就够 —— `new Intl.Collator` 不便宜，
 * 而排序每次刷新笔记都要跑一遍，别在循环/派生里现场 new。
 *
 * locale 里那个 `-u-co-pinyin` 是**显式指定拼音 collation**（Unicode 扩展语法）。
 * 只写 `'zh-Hans-CN'` 是碰运气 —— 默认 collation 由 ICU 版本决定，
 * 想要的一定是拼音就写死它。
 */
function makeCollator(): Intl.Collator {
  const opts: Intl.CollatorOptions = { numeric: true };
  try {
    return new Intl.Collator('zh-Hans-CN-u-co-pinyin', opts);
  } catch {
    // 极老的引擎不认扩展语法就退回普通中文排序，至少不崩
    return new Intl.Collator('zh-Hans-CN', opts);
  }
}

const collator = makeCollator();

/** 比名字用。导出是为了让别处（比如新建后排序的自检）能复用同一套比较规则。 */
export function compareName(a: string, b: string): number {
  return collator.compare(a, b);
}

/**
 * 按当前方式排一棵笔记树，返回**新树**（不改入参，避免和 Svelte 的响应式打架）。
 *
 * 规则（每一层都一样，递归套用）：
 *   1. **文件夹永远在前，且永远按名字排**
 *      目录的 mtime 会因子项增删而变，拿它排序会让树「抖」；而且「文件夹在前」
 *      是树能一眼扫完的地基，不该随排序方式翻面。
 *   2. 文件按 `mode`：`name` → 名字升序；`mtime` → 修改时间**降序**（最新在最上）。
 *
 * `mtime` 相同时退回按名字比，保证顺序稳定 —— 否则每次刷出来的顺序可能不一样，
 * 树会自己「跳」。
 */
export function sortNotesTree(nodes: TreeNode[], mode: NoteSort): TreeNode[] {
  const out = nodes.map((n) =>
    n.isDir ? { ...n, children: sortNotesTree(n.children, mode) } : n
  );
  out.sort((a, b) => {
    if (a.isDir !== b.isDir) return a.isDir ? -1 : 1;
    if (a.isDir) return compareName(a.name, b.name);
    if (mode === 'mtime') {
      const d = b.mtime - a.mtime;
      if (d !== 0) return d;
    }
    return compareName(a.name, b.name);
  });
  return out;
}

/**
 * 全树里**最近修改的那个文件**的路径，没有文件就返回 null。
 *
 * 用来回答「点『笔记』图标该打开哪篇」。不能拿树里的第一个文件顶数 ——
 * 因为文件夹永远在前，第一个文件其实是「名字最靠前的那个文件夹里的文件」，
 * 跟你刚才在写的东西八竿子打不着。
 */
export function newestNotePath(nodes: TreeNode[]): string | null {
  let bestPath: string | null = null;
  let bestTime = -1;
  const walk = (ns: TreeNode[]) => {
    for (const n of ns) {
      if (n.isDir) {
        walk(n.children);
      } else if (n.mtime > bestTime) {
        bestTime = n.mtime;
        bestPath = n.path;
      }
    }
  };
  walk(nodes);
  return bestPath;
}
