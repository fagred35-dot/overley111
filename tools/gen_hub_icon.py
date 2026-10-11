#!/usr/bin/env python3
"""Генератор иконки хаба Overley: spike/hub/icons/icon.ico (PNG-сжатие внутри ICO).
Только стандартная библиотека. Дизайн: тёмный скруглённый квадрат + зелёный прицел.
Запуск: python3 tools/gen_hub_icon.py
"""
import math, struct, zlib, pathlib

W = H = 256
CX = CY = 128.0

def clamp(x, lo, hi):
    return lo if x < lo else hi if x > hi else x

def sd_round_box(px, py, cx, cy, hx, hy, r):
    qx = abs(px - cx) - hx + r
    qy = abs(py - cy) - hy + r
    ax, ay = max(qx, 0.0), max(qy, 0.0)
    return math.hypot(ax, ay) + min(max(qx, qy), 0.0) - r

def sd_circle(px, py, cx, cy, r):
    return math.hypot(px - cx, py - cy) - r

def aa(d):  # покрытие кромки в 1 px
    return clamp(0.5 - d, 0.0, 1.0)

BG = (0x12, 0x18, 0x26)
GREEN = (0x00, 0xFF, 0x88)

px = bytearray()
for y in range(H):
    px.append(0)  # filter none
    for x in range(W):
        r = g = b = a = 0.0
        # фон: скруглённый квадрат 232x232, r=56
        k = aa(sd_round_box(x + 0.5, y + 0.5, CX, CY, 116, 116, 56))
        r, g, b, a = BG[0], BG[1], BG[2], k
        # кольцо r=100, толщина 7, полупрозрачное
        k = aa(abs(sd_circle(x + 0.5, y + 0.5, CX, CY, 100)) - 3.5) * 0.55
        if k > 0:
            r = r * (1 - k) + GREEN[0] * k
            g = g * (1 - k) + GREEN[1] * k
            b = b * (1 - k) + GREEN[2] * k
            a = max(a, k)
        # лучи креста: 4 скруглённых бара 20x64, r=8
        for (bx, by, hx, hy) in [(CX, CY - 62, 10, 32), (CX, CY + 62, 10, 32),
                                 (CX - 62, CY, 32, 10), (CX + 62, CY, 32, 10)]:
            k = aa(sd_round_box(x + 0.5, y + 0.5, bx, by, hx, hy, 8))
            if k > 0:
                r = r * (1 - k) + GREEN[0] * k
                g = g * (1 - k) + GREEN[1] * k
                b = b * (1 - k) + GREEN[2] * k
                a = max(a, k)
        # точка в центре
        k = aa(sd_circle(x + 0.5, y + 0.5, CX, CY, 13))
        if k > 0:
            r = r * (1 - k) + GREEN[0] * k
            g = g * (1 - k) + GREEN[1] * k
            b = b * (1 - k) + GREEN[2] * k
            a = max(a, k)
        px += bytes((int(r), int(g), int(b), int(round(a * 255))))

def chunk(tag, data):
    return struct.pack(">I", len(data)) + tag + data + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)

png = (b"\x89PNG\r\n\x1a\n"
       + chunk(b"IHDR", struct.pack(">IIBBBBB", W, H, 8, 6, 0, 0, 0))
       + chunk(b"IDAT", zlib.compress(bytes(px), 9))
       + chunk(b"IEND", b""))

ico = struct.pack("<HHH", 0, 1, 1) + struct.pack("<BBBBHHII", 0, 0, 0, 0, 1, 32, len(png), 22) + png
out = pathlib.Path("spike/hub/icons/icon.ico")
out.parent.mkdir(parents=True, exist_ok=True)
out.write_bytes(ico)
print(f"{out}: {len(ico)} bytes (PNG-in-ICO {W}x{H})")
