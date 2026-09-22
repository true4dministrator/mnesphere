/** 右键菜单的公共构造器。
 *
 * 目标只有一个：把 WebView2 自带的浏览器菜单彻底换掉，全用自绘的，风格才统一。
 * 唯一的代价是「粘贴」——网页层面读不到剪贴板，得走 navigator.clipboard。
 * 万一被安全策略拦下，键盘 Ctrl+V 依然可用（那条路由内核原生处理，不经过我们）。
 *
 * 图片也走同一条路：`navigator.clipboard.read()` 能直接给出 image/png 的 Blob。
 * 但它需要 WebView2 的 CLIPBOARD_READ 权限，而 wry 默认不放行 ——
 * 所以 `lib.rs` 里建窗口时特意加了 `enable_clipboard_access()`。
 */

import * as api from './api';
import { type CtxItem, cfg, refreshAll, toast } from './state.svelte';

/** 命中可编辑区域就返回它，否则返回 null。 */
export function editableTarget(t: EventTarget | null): HTMLElement | null {
  let el = t as HTMLElement | null;
  while (el && el !== document.body) {
    const tag = el.tagName;
    if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT') return el;
    if (el.isContentEditable) return el;
    if (el.classList?.contains('cm-content')) return el;
    el = el.parentElement;
  }
  return null;
}

function exec(cmd: string): boolean {
  try {
    return document.execCommand(cmd);
  } catch {
    return false;
  }
}

function hasSelection(el: HTMLElement): boolean {
  if (el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement) {
    return el.selectionStart !== el.selectionEnd;
  }
  return (window.getSelection()?.toString().length ?? 0) > 0;
}

function selectAll(el: HTMLElement) {
  if (el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement) {
    el.select();
    return;
  }
  exec('selectAll');
}

/** 兜底插入：execCommand('insertText') 在 contenteditable 上偶尔失灵，手动补一刀。 */
function insertManually(el: HTMLElement, text: string) {
  if (el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement) {
    const start = el.selectionStart ?? el.value.length;
    const end = el.selectionEnd ?? start;
    el.value = el.value.slice(0, start) + text + el.value.slice(end);
    const caret = start + text.length;
    el.setSelectionRange(caret, caret);
    el.dispatchEvent(new Event('input', { bubbles: true }));
    return;
  }
  const sel = window.getSelection();
  if (!sel || !sel.rangeCount) return;
  const range = sel.getRangeAt(0);
  range.deleteContents();
  range.insertNode(document.createTextNode(text));
  range.collapse(false);
  sel.removeAllRanges();
  sel.addRange(range);
  el.dispatchEvent(new Event('input', { bubbles: true }));
}

/** 从剪贴板里抠出第一张图。没图、或者没拿到读权限，都返回 null。
 *
 *  依赖 `lib.rs` 建窗口时开的 `enable_clipboard_access()` —— 没开的话
 *  WebView2 默认不放行 CLIPBOARD_READ，`read()` 会抛 NotAllowedError，
 *  于是静默落到下面的文本分支，行为跟以前完全一样。 */
async function readClipboardImage(): Promise<Blob | null> {
  try {
    const items = await navigator.clipboard.read();
    for (const it of items) {
      const type = it.types.find((t) => t.startsWith('image/'));
      if (type) return await it.getType(type);
    }
  } catch {
    /* 读不到图，就当作剪贴板里只有文本 */
  }
  return null;
}

async function pasteInto(el: HTMLElement | null) {
  if (!el) return;
  el.focus();

  // 图片优先 —— 跟编辑器里 Ctrl+V 的口径保持一致（那边 pickImage 也是先找图）
  const img = await readClipboardImage();
  if (img) {
    // 只有笔记正文放得下图，输入框（任务标题之类）放不下
    if (!el.classList.contains('cm-content') && !el.closest('.cm-editor')) {
      toast('图片只能粘进笔记正文', 'info');
      return;
    }
    try {
      const bytes = new Uint8Array(await img.arrayBuffer());
      const rel = await api.savePastedImage(bytes);
      // 插入交给 CMEditor —— 只有它手里有 CodeMirror 的光标位置
      window.dispatchEvent(new CustomEvent('mnesphere:insert', { detail: `![[${rel}]]` }));
      toast('图片已存进附件', 'ok');
    } catch (e) {
      toast(typeof e === 'string' ? e : String(e), 'error');
    }
    return;
  }

  let text = '';
  try {
    text = await navigator.clipboard.readText();
  } catch {
    toast('这个环境不允许读剪贴板，用 Ctrl+V 粘贴吧', 'error');
    return;
  }
  if (!text) {
    // 以前这里是静默 return，用户按了没反应只会以为坏了
    toast('剪贴板里没有可粘贴的内容', 'info');
    return;
  }
  if (!document.execCommand('insertText', false, text)) {
    insertManually(el, text);
  }
}

/** 可编辑区域里的标准编辑菜单。 */
export function editMenuItems(el: HTMLElement | null): CtxItem[] {
  const ok = !!el;
  const sel = ok && hasSelection(el!);
  return [
    {
      label: '剪切',
      icon: 'edit',
      disabled: !sel,
      run: () => {
        el?.focus();
        exec('cut');
      }
    },
    {
      label: '复制',
      icon: 'copy',
      disabled: !sel,
      run: () => {
        el?.focus();
        exec('copy');
      }
    },
    { label: '粘贴', icon: 'plus', disabled: !ok, run: () => void pasteInto(el) },
    { sep: true },
    {
      label: '全选',
      icon: 'grid',
      disabled: !ok,
      run: () => {
        el?.focus();
        selectAll(el!);
      }
    }
  ];
}

/** 非编辑区域的默认菜单：不放「刷新/打印」这种浏览器味儿的东西。 */
export function blankMenuItems(): CtxItem[] {
  return [
    {
      label: '重新读取 vault',
      icon: 'refresh',
      run: () => {
        void refreshAll().then(() => toast('已重新读取', 'ok'));
      }
    },
    {
      label: '打开 vault 目录',
      icon: 'folder',
      run: () => {
        const v = cfg.current?.vault;
        if (v) void api.openInExplorer(v);
      }
    }
  ];
}
