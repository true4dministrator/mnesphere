/** 时间显示：把后端存的那串时间变成人话。 */

/**
 * 相对时间：刚刚 / N 分钟前 / N 小时前 / N 天前，超过一个月直接给日期。
 *
 * 后端 `lastSync` 存的是 `"YYYY-MM-DD HH:MM:SS"`（本地时间）。这个格式
 * **不是标准 ISO**（ES 规范只认 `T` 分隔），虽然 V8 能解析，但换个引擎就未必 ——
 * 所以先把空格换成 `T` 再交给 `Date`。解析失败时原样返回，别显示 `NaN`。
 *
 * @param now 传入当前时间戳，让调用方可以用一个定时器统一驱动刷新。
 */
export function relTime(v: string, now = Date.now()): string {
  if (!v) return '';
  const t = new Date(v.replace(' ', 'T')).getTime();
  if (Number.isNaN(t)) return v;

  const diff = now - t;
  // 时钟回拨 / 未来时间：说「刚刚」比说「-3 分钟前」体面
  if (diff < 60_000) return '刚刚';

  const m = Math.floor(diff / 60_000);
  if (m < 60) return `${m} 分钟前`;

  const h = Math.floor(m / 60);
  if (h < 24) return `${h} 小时前`;

  const d = Math.floor(h / 24);
  if (d < 30) return `${d} 天前`;

  // 超过一个月，"62 天前"已经没信息量了，直接报日期
  const dt = new Date(t);
  const p = (n: number) => String(n).padStart(2, '0');
  return `${dt.getFullYear()}-${p(dt.getMonth() + 1)}-${p(dt.getDate())}`;
}
