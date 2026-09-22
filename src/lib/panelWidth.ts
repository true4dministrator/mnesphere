/** 左栏宽度的口径。**只有这一份** —— 拖动分界线、设置里的输入框、
 *  「自适应」的测算，全都走这里，不许各自算各自的边界。 */

/** 再窄下去分组名和任务标题就要开始截断了，这是硬下限。 */
export const PANEL_MIN = 180;
/** 右半边（详情 / 编辑器）永远要留够站的地方，所以上限跟窗口宽度挂钩。 */
export const PANEL_RESERVE = 320;

export function clampPanel(w: number, winW: number): number {
  const hi = Math.max(PANEL_MIN, winW - PANEL_RESERVE);
  return Math.round(Math.min(hi, Math.max(PANEL_MIN, w)));
}

/**
 * 称一称左栏「刚好装下」要多少像素。
 *
 * 做法不是去量 DOM（那要在渲染后、还得等字体加载完，很脆），
 * 而是按当前字号把最长的那条文字估出来：
 *   CJK 一个字 ≈ 1em，其余 ASCII ≈ 0.55em
 * 这个系数在等宽/无衬线上都够用，因为最终还会被 clampPanel 和一点点余量兜住。
 *
 * `extra` 是「除了文字之外那一行的固定开销」：行内 padding（左右各 8）、
 * 右边界那个日期角标占的一列、以及栏本身的左右留白。
 * 任务行**原来还有个勾选框（≈20px + 6px gap），后来去掉了** ——
 * 所以任务态的 overhead 比笔记态小一档。
 */
export const PANEL_EXTRA_TASK = 46;
export const PANEL_EXTRA_NOTES = 66;

export function measurePanel(samples: string[], fontPx: number, extra = PANEL_EXTRA_TASK): number {
  let widest = 0;
  for (const s of samples) {
    let em = 0;
    for (const ch of s) em += /[\u2E80-\u9FFF\uFF00-\uFFEF]/.test(ch) ? 1 : 0.55;
    widest = Math.max(widest, em * fontPx);
  }
  return widest + extra;
}

