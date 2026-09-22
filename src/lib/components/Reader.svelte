<script lang="ts">
  import * as api from '../api';
  import { render } from '../markdown';
  import { ai, cfg, doc, openDoc, toast, ui } from '../state.svelte';

  const out = $derived.by(() =>
    render(doc.data?.body ?? '', {
      vault: cfg.current?.vault ?? '',
      currentPath: doc.path
    })
  );

  async function follow(target: string) {
    const rel = await api.resolveLink(target);
    if (rel) {
      const fromDiary = doc.data?.kind === 'diary';
      const isDiary = rel.includes('日记');
      if (fromDiary !== isDiary) {
        // 跨模块跳转：切 activity 并保留来源，Alt+← 可退回
        ui.activity = isDiary ? 'diary' : 'notes';
      }
      await openDoc(rel);
    } else {
      toast(`「${target}」还不存在。在笔记里新建一篇同名笔记，双链就接上了。`, 'info');
    }
  }

  function onClick(e: MouseEvent) {
    const el = e.target as HTMLElement;
    const wiki = el.closest('.wiki-link') as HTMLElement | null;
    if (wiki) {
      e.preventDefault();
      void follow(wiki.dataset.target ?? '');
      return;
    }
    const ext = el.closest('.ext-link') as HTMLElement | null;
    if (ext) {
      e.preventDefault();
      void api.openExternal(ext.dataset.href ?? '').catch((err) => toast(String(err), 'error'));
    }
  }

  function onDblClick(e: MouseEvent) {
    if ((e.target as HTMLElement).closest('a')) return;
    doc.mode = 'edit';
  }

  /** 选中正文 → 一键问 AI */
  function askSelection() {
    const sel = window.getSelection()?.toString().trim();
    if (!sel) {
      toast('先选中一段文字');
      return;
    }
    ai.input = `请解释/检查这段内容：\n\n${sel}`;
    ui.aiOpen = true;
  }
</script>

<div class="reader">
  <div class="page">
    <!-- 渲染出来的正文里，[[双链]] / 外链是真正的 <a>（自带键盘可达性），
         这里的 click/dblclick 只是外层委托：点链接跳转、双击正文切到编辑。 -->
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <article class="md-body" onclick={onClick} ondblclick={onDblClick}>
      {@html out.html}
    </article>
  </div>

  <button class="ask" onclick={askSelection} title="把选中的内容发给 AI">
    问 AI
  </button>
</div>

<style>
  .reader {
    position: relative;
    flex: 1;
    min-height: 0;
    overflow: auto;
    overscroll-behavior: contain;
  }

  .page {
    max-width: 780px;
    margin: 0 auto;
    padding: 26px 32px 40vh;
  }

  .md-body {
    font-size: var(--fs);
    line-height: 1.85;
    color: var(--fg-dim);
    user-select: text;
    cursor: text;
    word-break: break-word;
  }

  .ask {
    position: absolute;
    top: 14px;
    right: 18px;
    height: 24px;
    padding: 0 10px;
    border-radius: 999px;
    border: 1px solid var(--accent-line);
    background: var(--panel-2);
    color: var(--accent);
    font-size: 11.5px;
    backdrop-filter: blur(14px);
    opacity: 0;
    transition: opacity 0.15s;
  }
  .reader:hover .ask {
    opacity: 1;
  }

  :global(.md-body > *:first-child) {
    margin-top: 0;
  }
  :global(.md-body h1) {
    font-size: 1.65em;
    font-weight: 600;
    color: var(--fg);
    margin: 30px 0 14px;
    line-height: 1.35;
  }
  :global(.md-body h2) {
    font-size: 1.28em;
    font-weight: 500;
    color: var(--fg);
    margin: 26px 0 10px;
    padding-bottom: 6px;
    border-bottom: 1px solid var(--line);
  }
  :global(.md-body h3) {
    font-size: 1.1em;
    font-weight: 500;
    color: var(--accent);
    margin: 22px 0 8px;
  }
  :global(.md-body h4),
  :global(.md-body h5),
  :global(.md-body h6) {
    font-size: 1em;
    font-weight: 500;
    color: var(--accent-2);
    margin: 18px 0 6px;
  }
  :global(.md-body p) {
    margin: 12px 0;
  }
  :global(.md-body ul),
  :global(.md-body ol) {
    margin: 12px 0;
    padding-left: 24px;
  }
  :global(.md-body li) {
    margin: 5px 0;
  }
  :global(.md-body li::marker) {
    color: var(--accent);
  }
  :global(.md-body blockquote) {
    margin: 16px 0;
    padding: 4px 0 4px 16px;
    border-left: 2px solid var(--accent-line);
    color: var(--fg-mute);
  }
  :global(.md-body a) {
    color: var(--accent);
    text-decoration: none;
    border-bottom: 1px solid transparent;
    cursor: pointer;
  }
  :global(.md-body a:hover) {
    border-bottom-color: var(--accent-line);
  }
  :global(.md-body a.wiki-link) {
    color: var(--accent-2);
    background: var(--accent-soft);
    border-radius: 4px;
    padding: 0 5px;
  }
  :global(.md-body hr) {
    margin: 26px 0;
    border: 0;
    height: 1px;
    background: var(--line);
  }
  :global(.md-body table) {
    width: 100%;
    margin: 16px 0;
    border-collapse: collapse;
    font-size: 0.92em;
  }
  :global(.md-body th),
  :global(.md-body td) {
    padding: 7px 12px;
    border: 1px solid var(--line);
    text-align: left;
  }
  :global(.md-body th) {
    background: var(--card);
    color: var(--fg);
    font-weight: 500;
  }
  :global(.md-body img.md-img) {
    max-width: 100%;
    border-radius: calc(var(--radius) * 0.7);
    border: 1px solid var(--line);
    margin: 8px 0;
    display: block;
  }
  :global(.md-body .task-item) {
    list-style: none;
    margin-left: -20px;
  }
  :global(.md-body .task-box) {
    display: inline-block;
    width: 13px;
    height: 13px;
    margin-right: 8px;
    vertical-align: -2px;
    border-radius: 4px;
    border: 1px solid var(--line-2);
  }
  :global(.md-body .task-box.done) {
    background: var(--accent-soft);
    border-color: var(--accent-line);
  }
  :global(.md-body .task-box.miss) {
    background: var(--danger-soft);
    border-color: var(--danger);
  }
</style>
