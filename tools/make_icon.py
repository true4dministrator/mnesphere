"""生成 mnesphere 应用图标源图（纯标准库，不依赖 PIL）。

图形：圆角深色底板 + 荧光青圆环 + 环上卫星点 + 中心核点。
输出 1024x1024 RGBA PNG，供 `pnpm tauri icon` 生成全套图标。
"""

import math
import struct
import zlib
from pathlib import Path

W = H = 1024
OUT = Path(__file__).resolve().parent.parent / "icon-src.png"

buf = bytearray(W * H * 4)
_out = buf  # alias for speed


def over(x: int, y: int, r: float, g: float, b: float, a: float) -> None:
    """标准 source-over 合成，a 为 0..1。"""
    if a <= 0.0 or x < 0 or y < 0 or x >= W or y >= H:
        return
    i = (y * W + x) * 4
    da = _out[i + 3] / 255.0
    na = a + da * (1.0 - a)
    if na <= 0.0001:
        _out[i] = _out[i + 1] = _out[i + 2] = _out[i + 3] = 0
        return
    inv = da * (1.0 - a)
    _out[i] = int(round((r * a + _out[i] * inv) / na))
    _out[i + 1] = int(round((g * a + _out[i + 1] * inv) / na))
    _out[i + 2] = int(round((b * a + _out[i + 2] * inv) / na))
    _out[i + 3] = int(round(na * 255))


def cov(d: float) -> float:
    """由有符号距离得到 1px 抗锯齿覆盖率。d<0 表示在形状内部。"""
    if d <= -0.5:
        return 1.0
    if d >= 0.5:
        return 0.0
    return 0.5 - d


def sd_round_rect(px, py, hw, hh, rad):
    qx = abs(px) - (hw - rad)
    qy = abs(py) - (hh - rad)
    outside = math.hypot(max(qx, 0.0), max(qy, 0.0))
    inside = min(max(qx, qy), 0.0)
    return outside + inside - rad


CX = CY = 512.0
PLATE_R = 232.0
RING_R = 308.0
RING_HW = 17.0
CORE_R = 44.0
SAT_R = 62.0
SAT_ANG = math.radians(-52.0)
SAT_X = CX + RING_R * math.cos(SAT_ANG)
SAT_Y = CY + RING_R * math.sin(SAT_ANG)

COL_PLATE = (10.0, 16.0, 22.0)
COL_ACCENT = (34.0, 211.0, 238.0)
COL_ACCENT2 = (94.0, 234.0, 212.0)

# 逐形状限定包围盒，避免 1M 像素 × 4 次全量遍历
def fill(sd_fn, box, color, alpha):
    x0, y0, x1, y1 = box
    x0 = max(0, int(math.floor(x0)))
    y0 = max(0, int(math.floor(y0)))
    x1 = min(W - 1, int(math.ceil(x1)))
    y1 = min(H - 1, int(math.ceil(y1)))
    r, g, b = color
    for y in range(y0, y1 + 1):
        dy = y - CY
        for x in range(x0, x1 + 1):
            a = cov(sd_fn(x - CX, dy)) * alpha
            if a > 0.0:
                over(x, y, r, g, b, a)


# 1) 底板：圆角方块，铺满画布
fill(
    lambda px, py: sd_round_rect(px, py, 512.0, 512.0, PLATE_R),
    (-4, -4, W + 4, H + 4),
    COL_PLATE,
    1.0,
)

# 2) 底板内侧描边（1px 荧光青，低透明度），呼应 Hyprland 窗口边框
fill(
    lambda px, py: abs(sd_round_rect(px, py, 512.0 - 4.0, 512.0 - 4.0, PLATE_R - 4.0)) - 1.5,
    (-4, -4, W + 4, H + 4),
    COL_ACCENT,
    0.30,
)

# 3) 外环
fill(
    lambda px, py: abs(math.hypot(px, py) - RING_R) - RING_HW,
    (CX - RING_R - 30, CY - RING_R - 30, CX + RING_R + 30, CY + RING_R + 30),
    COL_ACCENT,
    1.0,
)

# 4) 中心核
fill(
    lambda px, py: math.hypot(px, py) - CORE_R,
    (CX - CORE_R - 3, CY - CORE_R - 3, CX + CORE_R + 3, CY + CORE_R + 3),
    COL_ACCENT,
    0.55,
)

# 5) 环上卫星点
fill(
    lambda px, py: math.hypot(px - (SAT_X - CX), py - (SAT_Y - CY)) - SAT_R,
    (SAT_X - SAT_R - 3, SAT_Y - SAT_R - 3, SAT_X + SAT_R + 3, SAT_Y + SAT_R + 3),
    COL_ACCENT2,
    1.0,
)


def write_png(path: Path, w: int, h: int, rgba: bytearray) -> None:
    raw = bytearray()
    stride = w * 4
    for y in range(h):
        raw.append(0)
        raw += rgba[y * stride : (y + 1) * stride]

    def chunk(tag: bytes, data: bytes) -> bytes:
        return (
            struct.pack(">I", len(data))
            + tag
            + data
            + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)
        )

    png = b"\x89PNG\r\n\x1a\n"
    png += chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0))
    png += chunk(b"IDAT", zlib.compress(bytes(raw), 9))
    png += chunk(b"IEND", b"")
    path.write_bytes(png)


write_png(OUT, W, H, buf)
print(f"icon written -> {OUT}")
