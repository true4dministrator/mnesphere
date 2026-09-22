/** 主题引擎：把用户配置翻译成一组 CSS 变量写进 :root。
 *  Rust 侧完全不参与渲染 —— 换肤是纯前端行为，不需要重启、不需要重编译。 */

import { convertFileSrc } from '@tauri-apps/api/core';

export interface Theme {
  preset: string;
  accent: string;
  accent2: string;
  bg: string;
  fg: string;
  wallpaper: string;
  wallpaperOpacity: number;
  wallpaperBlur: number;
  panelOpacity: number;
  radius: number;
  fontUi: string;
  fontMono: string;
  /** 正文字号（编辑器 / 阅读区 / 全局基准） */
  fontSize: number;
  /** 左侧文件栏（Panel）的字号。跟正文分开，左栏文字密度大，常要单独调小或调大。 */
  panelFont: number;
  /**
   * 左侧文件栏的宽度。
   * - 数字 = 用户拖出来的固定值
   * - `'auto'` = 按内容称出来（只增不减，见 panelWidth.ts）
   *
   * 注意：`--panel-w` 原本写死在 app.css 的 :root 里，这个字段是后加的，
   * 所以 applyTheme 里**必须显式 set 一遍**，光加字段对老用户不生效。
   */
  panelWidth: number | 'auto';
}

export interface PresetInfo {
  id: string;
  accent: string;
  accent2: string;
  bg: string;
  fg: string;
}

type RGB = [number, number, number];

function parseHex(hex: string): RGB {
  let h = (hex || '').trim().replace('#', '');
  if (h.length === 3) h = h.split('').map((c) => c + c).join('');
  if (h.length !== 6 || /[^0-9a-fA-F]/.test(h)) return [34, 211, 238];
  return [
    parseInt(h.slice(0, 2), 16),
    parseInt(h.slice(2, 4), 16),
    parseInt(h.slice(4, 6), 16)
  ];
}

const clamp = (n: number, a = 0, b = 1) => Math.min(b, Math.max(a, n));

function rgba(c: RGB, a: number): string {
  return `rgba(${c[0]}, ${c[1]}, ${c[2]}, ${clamp(a).toFixed(3)})`;
}

/** 朝目标色混合，t ∈ [0,1] */
function mix(a: RGB, b: RGB, t: number): RGB {
  const k = clamp(t);
  return [
    Math.round(a[0] + (b[0] - a[0]) * k),
    Math.round(a[1] + (b[1] - a[1]) * k),
    Math.round(a[2] + (b[2] - a[2]) * k)
  ];
}

/** sRGB(0-255) → HSL。h 是 0..360 的度，s/l 是 0..1 */
function toHsl(c: RGB): [number, number, number] {
  const r = c[0] / 255;
  const g = c[1] / 255;
  const b = c[2] / 255;
  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  const l = (max + min) / 2;
  const d = max - min;
  if (d === 0) return [0, 0, l];
  const s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
  let h: number;
  if (max === r) h = ((g - b) / d) % 6;
  else if (max === g) h = (b - r) / d + 2;
  else h = (r - g) / d + 4;
  h *= 60;
  if (h < 0) h += 360;
  return [h, s, l];
}

/** HSL → sRGB(0-255) */
function fromHsl(h: number, s: number, l: number): RGB {
  const hh = ((h % 360) + 360) % 360;
  const c = (1 - Math.abs(2 * l - 1)) * clamp(s);
  const x = c * (1 - Math.abs(((hh / 60) % 2) - 1));
  const m = l - c / 2;
  let r = 0;
  let g = 0;
  let b = 0;
  if (hh < 60) [r, g, b] = [c, x, 0];
  else if (hh < 120) [r, g, b] = [x, c, 0];
  else if (hh < 180) [r, g, b] = [0, c, x];
  else if (hh < 240) [r, g, b] = [0, x, c];
  else if (hh < 300) [r, g, b] = [x, 0, c];
  else [r, g, b] = [c, 0, x];
  return [Math.round((r + m) * 255), Math.round((g + m) * 255), Math.round((b + m) * 255)];
}

const BLACK: RGB = [0, 0, 0];

/**
 * 抬升表面时用的「冷调浅色」。
 *
 * 关键：**不能用纯白**。朝白混等于往颜色里加灰 —— 色相被冲淡，
 * 五层表面叠出来就是一条没有性格的中性灰阶，整屏看着就是「灰蒙蒙」。
 * 混一个带色相的冷色，面板才是「深墨蓝」而不是「中灰」。
 */
const COOL: RGB = [150, 196, 222];

/**
 * 表面色饱和度的下限。深色在 sRGB 里直接混出来的饱和度只有二十几个百分点，
 * 摆满一屏就是「石板灰」。抬到这个下限，才真的是「墨青 / 墨蓝」。
 */
const MIN_SURFACE_S = 0.42;
/** 背景本身有色相时，只朝强调色偏这么多（0 = 不动，1 = 完全用强调色的色相） */
const HUE_PULL = 0.45;

/**
 * 保证一个颜色「有色相」：饱和度不低于下限，色相朝强调色偏一段。
 *
 * 两个作用：
 * 1. 去灰 —— 表面不再是一条饱和度 29% 的中性阶；
 * 2. 顺带修掉一个老毛病：抬升用的是冷色 COOL，所以在暖色预设（gruvbox 那种
 *    bg 饱和度接近 0 的）上会往暖底盖冷面板，很脏。
 *
 * 色相的可信度看 bg 的饱和度：bg 越接近中性，它的色相越没意义
 * （gruvbox 的 #282828 算出来的色相纯属噪声），这时候就直接采用强调色的色相。
 */
function chroma(c: RGB, acHue: number, pull: number): RGB {
  const [h, s, l] = toHsl(c);
  // 走最短弧，避免 350° → 10° 时反向绕一大圈
  const d = ((acHue - h + 540) % 360) - 180;
  return fromHsl(h + d * pull, Math.max(s, MIN_SURFACE_S), l);
}

/**
 * bg 的色相有多可信：饱和度 0（纯中性）→ pull = 1，直接用强调色的色相；
 * 饱和度 ≥ 20% → pull = HUE_PULL，只在 bg 自己的色相上偏一点。
 */
function hueTrust(bg: RGB): number {
  const s = clamp(toHsl(bg)[1] / 0.2);
  return 1 - s * (1 - HUE_PULL);
}

export function applyTheme(t: Theme) {
  const root = document.documentElement;
  const set = (k: string, v: string) => root.style.setProperty(k, v);

  const bg = parseHex(t.bg);
  const fg = parseHex(t.fg);
  const ac = parseHex(t.accent);
  const ac2 = parseHex(t.accent2);

  const hasWall = !!(t.wallpaper && t.wallpaper.trim());
  // 没铺壁纸的时候，半透明是纯粹的有害无益：它会把 panel → panel-2 → surface → card
  // 一层层叠成「平均色」，而且叠得越多次越灰。所以只有在真的有壁纸可透的时候，
  // 才去尊重用户设的那个不透明度。
  const pa = hasWall ? clamp(t.panelOpacity ?? 0.86, 0.2, 1) : 1;

  set('--bg', t.bg);
  set('--fg', t.fg);
  set('--accent', t.accent);
  set('--accent-2', t.accent2);
  set('--radius', `${t.radius ?? 12}px`);
  set('--fs', `${t.fontSize ?? 15}px`);
  set('--panel-font', `${t.panelFont ?? 12.5}px`);
  // 'auto' 时不在这里拍死 —— 交给 panelWidth.ts 称完内容再写。
  // 这里先兜一个 244（跟 app.css 的出厂值一致），避免首帧没有宽度。
  if (typeof t.panelWidth === 'number' && t.panelWidth > 0) {
    set('--panel-w', `${Math.round(t.panelWidth)}px`);
  } else {
    set('--panel-w', '244px');
  }
  set('--accent-soft', rgba(ac, 0.14));
  set('--accent-line', rgba(ac, 0.45));
  set('--on-accent', rgba(mix(bg, BLACK, 0.3), 1));

  // 表面层次：朝冷色抬升 + 掺一点点强调色，默认实心。
  // 层次感交给下面的描边和 --inset，而不是靠整体提亮。
  // 最后统一过一遍 chroma()，把饱和度和色相兜住 —— 不然叠出来是一屏灰。
  const acHue = toHsl(ac)[0];
  const pull = hueTrust(bg);
  const tone = (k: number, extra: number, a = pa) =>
    rgba(chroma(mix(mix(bg, COOL, k), ac, extra), acHue, pull), a);
  set('--panel', tone(0.075, 0.02, pa));
  set('--panel-2', tone(0.12, 0.03, Math.min(1, pa + 0.06)));
  set('--surface', tone(0.17, 0.04, Math.min(1, pa + 0.12)));
  set('--card', tone(0.1, 0.028, Math.min(1, pa + 0.08)));
  // 凹陷面（输入框、代码块）：靠「变暗」而不是「提亮」，深色才保得住
  set('--inset', 'rgba(0, 0, 0, 0.28)');

  // 描边和弱化文字都往强调色里掺一点。
  // 以前它们都是「近白色 + 低透明度」，那本质就是灰 —— 一屏几十条灰线、
  // 一层层灰小字，再怎么调表面色也还是灰蒙蒙。掺色之后连结构线都是青的。
  const edge = mix(fg, ac, 0.45);
  const dim = mix(fg, ac, 0.08);
  const mute = mix(fg, ac, 0.2);
  const faint = mix(fg, ac, 0.14);
  set('--line', rgba(edge, 0.13));
  set('--line-2', rgba(edge, 0.22));
  set('--hover', rgba(mute, 0.062));
  set('--active', rgba(mute, 0.1));

  // 文字层级：直接给「相对前景色的透明度」，落在任何表面上都保持同一套梯度。
  // 以前是「先往背景色混、再降透明度」，两次降对比叠在一起，弱化文字就糊成灰了。
  set('--fg-dim', rgba(dim, 0.8));
  set('--fg-mute', rgba(mute, 0.58));
  set('--fg-faint', rgba(faint, 0.4));

  set('--danger', `rgb(${mix([226, 135, 122], ac, 0).join(', ')})`);
  set('--danger-soft', rgba([226, 135, 122], 0.15));
  set('--warn', '#e8c06a');

  if (t.fontUi) set('--font-ui', `"${t.fontUi}", var(--font-ui)`);
  if (t.fontMono) set('--font-mono', `"${t.fontMono}", var(--font-mono)`);

  // 毛玻璃只在有壁纸时才有意义。没壁纸还挂着 backdrop-filter，
  // 一来白建一层合成层，二来会让这层上的文字失去次像素抗锯齿、看起来发虚。
  set('--glass-blur', hasWall ? '20px' : '0px');

  // 壁纸
  const wp = t.wallpaper && t.wallpaper.trim();
  if (wp) {
    set('--wallpaper', `url("${convertFileSrc(wp).replace(/"/g, '%22')}")`);
  } else {
    root.style.removeProperty('--wallpaper');
  }
  set('--wallpaper-opacity', String(clamp(t.wallpaperOpacity ?? 1)));
  set('--wallpaper-blur', `${t.wallpaperBlur ?? 0}px`);
}

export const FALLBACK_THEME: Theme = {
  preset: 'cyan',
  accent: '#22d3ee',
  accent2: '#5eead4',
  bg: '#0a0e13',
  fg: '#dbe6f0',
  wallpaper: '',
  wallpaperOpacity: 1,
  wallpaperBlur: 0,
  panelOpacity: 0.86,
  radius: 12,
  fontUi: '',
  fontMono: 'JetBrains Mono',
  fontSize: 15,
  panelFont: 12.5,
  panelWidth: 244
};
