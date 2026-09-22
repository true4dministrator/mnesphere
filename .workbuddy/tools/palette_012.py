"""复刻 theme.ts 的算法，算出默认荧光青主题下各 CSS 变量的最终值，
用来同步 app.css 的 :root 兜底值（两边必须一致，否则主题注入前那一帧会闪灰）。
"""


def clamp(n, a=0.0, b=1.0):
    return min(b, max(a, n))


def mix(a, b, t):
    k = clamp(t)
    return tuple(round(a[i] + (b[i] - a[i]) * k) for i in range(3))


def to_hsl(c):
    r, g, b = [v / 255 for v in c]
    mx, mn = max(r, g, b), min(r, g, b)
    l = (mx + mn) / 2
    d = mx - mn
    if d == 0:
        return (0.0, 0.0, l)
    s = d / (2 - mx - mn) if l > 0.5 else d / (mx + mn)
    if mx == r:
        h = ((g - b) / d) % 6
    elif mx == g:
        h = (b - r) / d + 2
    else:
        h = (r - g) / d + 4
    h *= 60
    if h < 0:
        h += 360
    return (h, s, l)


def from_hsl(h, s, l):
    hh = ((h % 360) + 360) % 360
    c = (1 - abs(2 * l - 1)) * clamp(s)
    x = c * (1 - abs(((hh / 60) % 2) - 1))
    m = l - c / 2
    if hh < 60:
        r, g, b = c, x, 0
    elif hh < 120:
        r, g, b = x, c, 0
    elif hh < 180:
        r, g, b = 0, c, x
    elif hh < 240:
        r, g, b = 0, x, c
    elif hh < 300:
        r, g, b = x, 0, c
    else:
        r, g, b = c, 0, x
    return tuple(round((v + m) * 255) for v in (r, g, b))


COOL = (150, 196, 222)
MIN_SURFACE_S = 0.42
HUE_PULL = 0.45
BLACK = (0, 0, 0)


def chroma(c, ac_hue, pull):
    h, s, l = to_hsl(c)
    d = ((ac_hue - h + 540) % 360) - 180
    return from_hsl(h + d * pull, max(s, MIN_SURFACE_S), l)


def hue_trust(bg):
    s = clamp(to_hsl(bg)[1] / 0.2)
    return 1 - s * (1 - HUE_PULL)


def rgba(c, a):
    return "rgba(%d, %d, %d, %.3f)" % (c[0], c[1], c[2], clamp(a))


def hexs(c):
    return "#%02x%02x%02x" % c


def report(name, preset):
    ac, ac2, bg_h, fg_h = preset
    ac = tuple(int(ac[i:i + 2], 16) for i in (1, 3, 5))
    ac2 = tuple(int(ac2[i:i + 2], 16) for i in (1, 3, 5))
    bg = tuple(int(bg_h[i:i + 2], 16) for i in (1, 3, 5))
    fg = tuple(int(fg_h[i:i + 2], 16) for i in (1, 3, 5))
    pa = 1.0
    ac_hue = to_hsl(ac)[0]
    pull = hue_trust(bg)

    def tone(k, extra, a=pa):
        return chroma(mix(mix(bg, COOL, k), ac, extra), ac_hue, pull)

    panel = tone(0.075, 0.02)
    panel2 = tone(0.12, 0.03)
    surface = tone(0.17, 0.04)
    card = tone(0.1, 0.028)
    edge = mix(fg, ac, 0.45)
    dim = mix(fg, ac, 0.08)
    mute = mix(fg, ac, 0.2)
    faint = mix(fg, ac, 0.14)

    print("=" * 62)
    print(f"{name}   accent={hexs(ac)}  accentHue={ac_hue:.1f}deg")
    print(f"  bg={hexs(bg)}  bgHue={to_hsl(bg)[0]:.1f}  bgS={to_hsl(bg)[1]*100:.0f}%  pull={pull:.2f}")
    print("-" * 62)
    for label, c in (("--panel", panel), ("--panel-2", panel2),
                     ("--surface", surface), ("--card", card)):
        h, s, l = to_hsl(c)
        print(f"  {label:<11} {hexs(c):<9} H={h:5.1f}  S={s*100:4.1f}%  L={l*100:4.1f}%")
    print("-" * 62)
    print(f"  --line       {rgba(edge, 0.13)}     edge={hexs(edge)}")
    print(f"  --line-2     {rgba(edge, 0.22)}")
    print(f"  --hover      {rgba(mute, 0.062)}")
    print(f"  --active     {rgba(mute, 0.1)}")
    print(f"  --fg-dim     {rgba(dim, 0.8)}     dim={hexs(dim)}")
    print(f"  --fg-mute    {rgba(mute, 0.58)}    mute={hexs(mute)}")
    print(f"  --fg-faint   {rgba(faint, 0.4)}    faint={hexs(faint)}")
    # 对照：0.1.1 的旧算法（朝 COOL 抬升后直接输出，不做饱和度/色相兜底）
    old_panel = mix(mix(bg, COOL, 0.075), ac, 0.02)
    oh, os_, ol = to_hsl(old_panel)
    print(f"  >>> 0.1.1 旧 --panel = {hexs(old_panel)}  H={oh:5.1f}  S={os_*100:4.1f}%  "
          f"L={ol*100:4.1f}%   （饱和度 {os_*100:.0f}% → {to_hsl(panel)[1]*100:.0f}%）")


report("cyan (默认)", ("#22d3ee", "#5eead4", "#0a0e13", "#dbe6f0"))
report("zch 正在用的自定义(Night 系)", ("#7aa2f7", "#2a1650", "#1a1b26", "#7a93ff"))
report("gruvbox", ("#fabd2f", "#b8bb26", "#282828", "#ebdbb2"))
report("nord", ("#88c0d0", "#a3be8c", "#2e3440", "#eceff4"))
report("catppuccin", ("#cba6f7", "#94e2d5", "#1e1e2e", "#cdd6f4"))
