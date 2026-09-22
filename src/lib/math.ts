/** 数学公式：**先屏蔽成占位符，等 markdown-it 跑完再换成 KaTeX**。
 *
 *  为什么不能让 markdown-it 或它的插件去处理公式 —— 实测过，markdown-it 会
 *  在任何人拿到公式之前就把内容弄坏：
 *
 *    `\begin{bmatrix} a & b \\ c & d \end{bmatrix}`
 *      → `\begin{bmatrix} a &amp; b \ c &amp; d \end{bmatrix}`
 *
 *  `&` 被转义成 HTML 实体、`\\` 被吃成一个 `\`。矩阵 / cases / align 全靠这两个
 *  符号活着，坏一个整块就报错。所以必须**在 markdown-it 之前把公式整段抠走**，
 *  换成占位符，跑完再放回去 —— 跟保护代码块是同一个套路。 */

import katex from 'katex';

export interface MathItem {
  tex: string;
  display: boolean;
}

/**
 * 占位符。
 *
 * ⚠️ **不能用 NUL（`\u0000`）那一套**，虽然代码围栏是那么干的 ——
 * 代码围栏在进 markdown-it 之前就被还原了，而这些占位符要**穿过**
 * markdown-it **和 DOMParser**：DOMParser 会把 NUL 换成 U+FFFD，
 * 占位符就再也认不出来，公式会以乱码形态留在页面上。
 *
 * 所以用纯 ASCII 字母数字：markdown-it 不碰它，linkify 也不会把它当链接
 * （没有点、没有 scheme）。
 */
const sentinel = (i: number) => `mnespheremathZ${i}Z`;

/**
 * 扫出所有数学段落，返回「公式被换成占位符的文本」+「公式清单」。
 *
 * 支持四种写法（前两种行间，后两种行内）：
 *   `$$...$$`、`\[...\]`、`$...$`、`\(...\)`
 * 后者两种是 Obsidian 也认的写法，顺手支持，成本几乎为零。
 *
 * 三道防误判：
 *   1. 前面是反斜杠（`\$`）→ 不是公式
 *   2. 开 `$` 后面紧跟空白、或闭 `$` 前面是空白 → 当成「$5 到 $8」这种普通文字
 *   3. 行内公式不跨行 —— 跨行的两个 `$` 几乎总是两处不相干的美元
 */
export function scanMath(src: string): { text: string; items: MathItem[] } {
  const items: MathItem[] = [];
  let out = '';
  let last = 0;
  let i = 0;

  while (i < src.length) {
    const c = src[i]!;
    let hit: { to: number; texFrom: number; texTo: number; display: boolean } | null = null;

    if (c === '$' && src[i + 1] === '$') {
      const end = src.indexOf('$$', i + 2);
      if (end > i + 2 && src.slice(i + 2, end).trim()) {
        hit = { to: end + 2, texFrom: i + 2, texTo: end, display: true };
      }
    } else if (c === '\\' && src[i + 1] === '[') {
      const end = src.indexOf('\\]', i + 2);
      if (end > i + 2 && src.slice(i + 2, end).trim()) {
        hit = { to: end + 2, texFrom: i + 2, texTo: end, display: true };
      }
    } else if (c === '\\' && src[i + 1] === '(') {
      const end = src.indexOf('\\)', i + 2);
      if (end > i + 2 && src.slice(i + 2, end).trim()) {
        hit = { to: end + 2, texFrom: i + 2, texTo: end, display: false };
      }
    } else if (c === '$' && src[i - 1] !== '\\') {
      const nxt = src[i + 1];
      if (nxt !== undefined && !/\s/.test(nxt)) {
        let j = i + 1;
        while (j < src.length) {
          const d = src[j]!;
          // `\$` 是转义的美元，跳过两个字符
          if (d === '\\') {
            j += 2;
            continue;
          }
          if (d === '\n') break;
          if (d === '$') {
            if (j > i + 1 && !/\s/.test(src[j - 1]!)) {
              hit = { to: j + 1, texFrom: i + 1, texTo: j, display: false };
            }
            break;
          }
          j += 1;
        }
      }
    }

    if (hit) {
      out += src.slice(last, i) + sentinel(items.length);
      items.push({ tex: src.slice(hit.texFrom, hit.texTo), display: hit.display });
      last = hit.to;
      i = hit.to;
      continue;
    }
    i += 1;
  }

  out += src.slice(last);
  return { text: out, items };
}

function escapeHtml(s: string): string {
  return s.replace(/[&<>"]/g, (ch) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[ch]!);
}

function katexHtml(it: MathItem): string {
  try {
    return katex.renderToString(it.tex, {
      displayMode: it.display,
      // 打错一个命令不该把整篇笔记炸掉 —— 显示成红色的原文，用户自己看得见
      throwOnError: false,
      errorColor: '#e2877a',
      // strict:false 是为了容忍 `\le` 这类「KaTeX 觉得不严谨但完全合法」的写法
      strict: false,
      // ⚠️ 保持 false。笔记内容和 AI 回答都是不可信输入，
      // 开了 trust 就等于允许 `\href` / `\includegraphics` 从文档里发请求
      trust: false
    });
  } catch {
    // throwOnError:false 已经兜住绝大多数情况，这里防的是极端输入
    return `<code class="math-err">${escapeHtml(it.tex)}</code>`;
  }
}

/**
 * 把 HTML 里的数学占位符换成 KaTeX 渲染结果。
 *
 * 行间公式额外做一件事：**把包着它的那个 `<p>` 一起去掉**。
 * 因为 markdown-it 会把独占一行的公式包成 `<p>占位符</p>`，
 * 而 KaTeX 的行间输出（`.katex-display`）自己就是块级元素，
 * 留着那个空 `<p>` 会在上下各多一份段落间距。
 */
export function renderMath(html: string, items: MathItem[]): string {
  let out = html;
  for (let i = 0; i < items.length; i++) {
    const it = items[i]!;
    const s = sentinel(i);
    const tex = katexHtml(it);
    if (it.display && out.includes(`<p>${s}</p>`)) {
      out = out.replace(`<p>${s}</p>`, tex);
    } else {
      out = out.replace(s, tex);
    }
  }
  return out;
}
