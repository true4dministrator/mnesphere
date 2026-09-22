<script lang="ts">
  import Icon from './Icons.svelte';
  import { ai, ui } from '../state.svelte';
</script>

<!--
  AI 助手的入口。原来挂在标题栏右上角，和窗口控制按钮挤在一起，也占标题栏。
  改成贴在窗口右缘的一枚竖直拉手：收着的时候是这一条，展开之后它就让位给抽屉、
  由抽屉自己的左边界接手（见 AiDrawer 的 .pull）。

  三个必须注意的地方：
  1. 层级要压过 App.svelte 里的窗口缩放热区 .resize.e（z-index 50），
     否则最右边那条的点击会被缩放热区吃掉，表现就是「点了没反应」。
  2. 因此往左让开 5px（热区宽 5px），这样贴边拖拽缩放依然可用。
  3. 高度必须由内容撑开、且竖直居中。早先用 top+bottom 双约束，等于把它拉成
     贯穿整窗的一条长条，上端还正好顶在标题栏右下角那几个窗口按钮下面，
     看着就像把按钮盖掉了一截。现在浮在右缘正中，两头都碰不到。
-->
<button
  class="ah"
  class:live={ai.hasKey}
  title="AI 助手（Ctrl+J）"
  aria-label="AI 助手"
  onclick={() => (ui.aiOpen = true)}
>
  <Icon name="ai" size={15} />
  <span class="vlbl">AI 助手</span>
  <i class="dot"></i>
</button>

<style>
  .ah {
    position: absolute;
    right: 5px;
    /* 竖直居中 + 高度由内容撑开：不再 top/bottom 双约束拉成长条 */
    top: 50%;
    transform: translateY(-50%);
    z-index: 55;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 8px;
    width: 17px;
    padding: 11px 0;
    border-radius: 9px;
    background: var(--card);
    border: 1px solid var(--line);
    color: var(--fg-mute);
    transition: width 0.15s, background 0.15s, border-color 0.15s, color 0.15s;
  }
  .ah:hover {
    width: 23px;
    background: var(--accent-soft);
    border-color: var(--accent-line);
    color: var(--accent);
  }

  /* 竖排文字：这是「拉手」的气质的来源，横过来就变回一个普通按钮了 */
  .vlbl {
    writing-mode: vertical-rl;
    font-size: 10.5px;
    letter-spacing: 0.16em;
    user-select: none;
  }

  .dot {
    width: 4px;
    height: 4px;
    border-radius: 50%;
    background: var(--fg-faint);
    transition: background 0.15s;
  }
  /* 配了 Key 才亮，让人一眼知道这个助手是「通的」 */
  .ah.live .dot {
    background: var(--accent-2);
  }

  /* 窄窗口下 17px 也是钱，直接让位，功能留给 Ctrl+J */
  @media (max-width: 1080px) {
    .ah {
      display: none;
    }
  }
</style>
