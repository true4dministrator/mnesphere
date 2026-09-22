/** Markdown 渲染：wiki 双链、Obsidian 图片语法、任务列表、代码高亮。
 *
 *  为什么渲染放前端而索引放 Rust：
 *  索引（双链 / 反链 / 搜索）必须全局可见，所以必须在 Rust；而渲染只服务当前这一篇，
 *  放前端能直接用成熟库 + highlight.js，省掉一大堆重复实现。 */

import MarkdownIt from 'markdown-it';
import hljs from 'highlight.js';
import { convertFileSrc } from '@tauri-apps/api/core';
import { renderMath, scanMath, type MathItem } from './math';

/** 用不可能出现在正常路径里的前缀做标记，渲染完再替换成真实地址 */
const ASSET_PREFIX = '__mnesphere_asset__/';
const WIKI_PREFIX = '__mnesphere_wiki__/';

const md: MarkdownIt = new MarkdownIt({
  html: false,
  linkify: true,
  breaks: true,
  typographer: false,
  highlight(code: string, lang: string): string {
    if (lang && hljs.getLanguage(lang)) {
      try {
        const out = hljs.highlight(code, { language: lang, ignoreIllegals: true }).value;
        return `<pre class="hljs"><code class="language-${escapeAttr(lang)}">${out}</code></pre>`;
      } catch {
        /* 落到下面兜底 */
      }
    }
    return `<pre class="hljs"><code>${escapeHtml(code)}</code></pre>`;
  }
});

function escapeHtml(s: string): string {
  return s.replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[c]!);
}
function escapeAttr(s: string): string {
  return s.replace(/[^a-zA-Z0-9_-]/g, '');
}

/** 先把代码（围栏 + 行内）抠出来存好。
 *
 *  ⚠️ **顺序是硬要求：代码必须排在数学之前扫描。**
 *  代码里出现 `$` 是常态（`$$` 本身、shell 的 `$PATH`、正则的 `$`），
 *  先屏蔽代码，数学扫描器就看不到它们，不会把代码当公式。
 *
 *  行内代码一起屏蔽还顺手修掉一个老毛病：`` `[[x]]` `` 写在反引号里本来
 *  也会被 wiki 链接替换吃掉，然后变成一个指向「x」的链接 —— 而它明明是代码。 */
function shieldCode(src: string): { text: string; blocks: string[] } {
  const blocks: string[] = [];
  const keep = (m: string) => {
    blocks.push(m);
    return `\u0000F${blocks.length - 1}\u0000`;
  };
  let s = src.replace(/```[\s\S]*?```|~~~[\s\S]*?~~~/g, keep);
  // 行内代码：单行、1~3 个反引号。围栏已经被上面抠走了，不会误伤。
  s = s.replace(/(`{1,3})([^\n]*?)\1/g, keep);
  return { text: s, blocks };
}

function unshield(src: string, blocks: string[]): string {
  return src.replace(/\u0000F(\d+)\u0000/g, (_m, i) => blocks[Number(i)] ?? '');
}

/**
 * 预处理：把「不该被 markdown-it 碰」的东西先抠走。
 *
 * 目前两类：**代码**和**公式**。
 * 公式必须在这里抠走 —— 实测 markdown-it 会把 `&` 转义成 `&amp;`、
 * 把矩阵换行 `\\` 吃成一个 `\`，等它处理完再补救已经来不及。
 * 详见 `math.ts` 顶部的说明。
 */
export function prepare(src: string): { text: string; math: MathItem[] } {
  const { text, blocks } = shieldCode(src);
  const { text: mathShielded, items } = scanMath(text);
  let s = mathShielded;

  // ![[xxx.png]] —— Obsidian 嵌入语法
  //
  // ⚠️ 必须先判扩展名。`![[...]]` 有两种完全不同的意思：嵌**图片**和嵌**笔记**。
  // 以前一律当图片处理，于是 `![[某篇笔记]]` 会变成一个 src 指向笔记路径的
  // <img>，在阅读态就是一张永远加载不出来的破图；更糟的是它连双链也不算，
  // 点都点不动。不是图片就退回普通双链，至少还是个能点的链接。
  s = s.replace(/!\[\[([^\[\]]{1,300}?)\]\]/g, (_m, inner: string) => {
    const target = String(inner).split('|')[0].trim();
    if (!isImageRef(target)) return `[[${inner}]]`;
    return `![${target}](<${ASSET_PREFIX}${target}>)`;
  });

  // [[目标|别名]] 或 [[目标]]
  s = s.replace(/\[\[([^\[\]]{1,300}?)\]\]/g, (_m, inner: string) => {
    const parts = String(inner).split('|');
    const target = (parts[0] ?? '').trim();
    const alias = (parts[1] ?? parts[0] ?? '').trim();
    if (!target) return _m as string;
    // markdown-it 会把 URL 里的空格编码掉，所以用尖括号包起来
    return `[${alias}](<${WIKI_PREFIX}${target}>)`;
  });

  // 只还原代码。**公式的占位符要留着** —— 它得穿过 markdown-it 才能活到
  // render() 里被换成 KaTeX 的 HTML。
  return { text: unshield(s, blocks), math: items };
}

/** 只要文本那一半的旧接口。渲染路径请用 `prepare()`，否则公式会被当成普通文字。 */
export function preprocess(src: string): string {
  return prepare(src).text;
}

/** frontmatter 正则 —— **必须与 Rust 侧 `note.rs::re_fm()` 完全一致**
 *  （`(?s)\A---\r?\n(.*?)\r?\n---[ \t]*\r?\n?`），否则前端算出来的 body
 *  会和 Rust 读盘时算出来的对不上，阅读态就会一闪一变。
 *  调用前先剥 BOM，跟 Rust 的 `strip_prefix('\u{feff}')` 对齐。 */
const FM_RE = /^---\r?\n([\s\S]*?)\r?\n---[ \t]*\r?\n?/;

/** 拆 frontmatter。语义对齐 `note.rs::split_frontmatter`。 */
export function splitFrontmatter(src: string): { fm: string | null; body: string } {
  const t = src.startsWith('\uFEFF') ? src.slice(1) : src;
  const m = FM_RE.exec(t);
  if (!m) return { fm: null, body: t };
  return { fm: m[1] ?? '', body: t.slice(m[0].length) };
}

/** 是图片文件吗。**判定只看扩展名** —— 光看 `![[...]]` 分不出「嵌一张图」和
 *  「嵌一篇笔记」，不判扩展名就会把双链嵌稿渲染成一个坏掉的 <img>。 */
const IMG_EXT_RE = /\.(png|jpe?g|gif|bmp|webp|svg|avif)$/i;

export function isImageRef(rel: string): boolean {
  return IMG_EXT_RE.test(rel.split('#')[0]!.split('?')[0]!.trim());
}

/** 图片引用的两种写法：`![[附件/x.png]]`（可带 `|宽`）和 `![alt](附件/x.png)`。
 *  刻意不导出成常量 —— 带 `g` 的正则是有状态的，谁把它当常量用谁就会踩 lastIndex。 */
const IMG_REF_SRC =
  /!\[\[([^\[\]|]{1,300}?)(?:\|[^\[\]]{0,80})?\]\]|!\[[^\]\n]{0,200}\]\(\s*<?([^)\n>]{1,300})>?\s*\)/g.source;

/** 扫一遍文本里所有的图片引用。`from`/`to` 是**文档坐标**，专门给 CMEditor 做装饰用。
 *  上限 300 张：纯防御，正常笔记到不了，但真到了也不能让一次重算把界面卡住。 */
export function eachImageRef(
  text: string,
  fn: (hit: { from: number; to: number; rel: string }) => void
): void {
  const re = new RegExp(IMG_REF_SRC, 'g');
  let m: RegExpExecArray | null;
  let n = 0;
  while ((m = re.exec(text))) {
    const rel = (m[1] ?? m[2] ?? '').trim();
    if (!rel || !isImageRef(rel)) continue;
    fn({ from: m.index, to: m.index + m[0].length, rel });
    if (++n >= 300) break;
  }
}

/** vault 相对路径 → 能给 `<img src>` 用的 asset 协议地址。
 *  编辑态那套图片预览也走这一份，别在别处自己拼 `convertFileSrc`。 */
export function assetUrl(vault: string, rel: string): string {
  return convertFileSrc(joinVault(vault, rel));
}

export interface RenderResult {
  html: string;
  links: string[];
}

export interface RenderCtx {
  vault: string;
  /** 当前笔记的相对路径，用于解析相对图片地址 */
  currentPath: string;
}

export function render(src: string, ctx: RenderCtx): RenderResult {
  const { text, math } = prepare(src);
  // 公式在这里换成 KaTeX 的 HTML。必须在 DOMParser 之前完成 ——
  // 那之后的操作（链接、图片、任务框）都建立在「已经是最终 HTML」的前提上。
  const raw = renderMath(md.render(text), math);
  const doc = new DOMParser().parseFromString(`<div id="root">${raw}</div>`, 'text/html');
  const root = doc.getElementById('root')!;

  // wiki 链接
  root.querySelectorAll('a[href]').forEach((a) => {
    const href = a.getAttribute('href') ?? '';
    if (href.includes(WIKI_PREFIX)) {
      const target = decodeURIComponent(href.slice(href.indexOf(WIKI_PREFIX) + WIKI_PREFIX.length));
      a.removeAttribute('href');
      a.classList.add('wiki-link');
      a.setAttribute('data-target', target);
      // 目标不存在时标灰，提示这是"还没写的笔记"
      return;
    }
    if (/^https?:/i.test(href)) {
      a.classList.add('ext-link');
      a.setAttribute('data-href', href);
      a.removeAttribute('href');
    }
  });

  // 图片：统一经 asset 协议加载本地文件
  root.querySelectorAll('img').forEach((img) => {
    let src = img.getAttribute('src') ?? '';
    if (src.startsWith(ASSET_PREFIX)) src = src.slice(ASSET_PREFIX.length);
    src = src.trim().replace(/^<|>$/g, '');
    try {
      src = decodeURIComponent(src);
    } catch {
      /* 保持原样 */
    }
    if (/^(https?:|data:|asset:|blob:)/i.test(src)) {
      if (!/^https?:/i.test(src)) img.setAttribute('src', src);
      return;
    }
    img.setAttribute('src', assetUrl(ctx.vault, src));
    img.setAttribute('loading', 'lazy');
    img.classList.add('md-img');
  });

  // 任务列表：markdown-it 不管，自己补 checkbox
  root.querySelectorAll('li').forEach((li) => {
    const first = li.firstChild;
    if (!first || first.nodeType !== 3) return;
    const text = first.textContent ?? '';
    const m = /^\s*\[([ xX\-])\]\s+/.exec(text);
    if (!m) return;
    const mark = m[1].toLowerCase();
    const rest = text.slice(m[0].length);
    first.textContent = rest;
    const box = doc.createElement('span');
    box.className = `task-box ${mark === 'x' ? 'done' : mark === '-' ? 'miss' : ''}`;
    li.insertBefore(box, li.firstChild);
    li.classList.add('task-item');
  });

  return { html: root.innerHTML, links: [] };
}

function joinVault(vault: string, rel: string): string {
  const base = vault.replace(/\\/g, '/').replace(/\/+$/, '');
  if (/^[A-Za-z]:[\\/]/.test(rel) || rel.startsWith('/')) return rel;
  let r = rel.replace(/\\/g, '/').replace(/^\.\//, '');
  // 支持 ../ 回溯，但夹在 vault 内
  const parts: string[] = [];
  for (const seg of r.split('/')) {
    if (seg === '..') parts.pop();
    else if (seg !== '.' && seg !== '') parts.push(seg);
  }
  return `${base}/${parts.join('/')}`;
}

export { md };
