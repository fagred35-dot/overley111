#!/usr/bin/env python3
"""Референсный рендерер Overley Crosshair DSL v1 → PNG.

Зачем нужен
-----------
1. Фиксирует семантику формата исполняемым кодом: порядок проходов (glow → outline →
   слои снизу вверх, тень каждого слоя под ним), система координат (относительно
   canvas.origin, ось Y вниз), порядок преобразований слоя (scale → rotate → pos),
   формулы SDF для каждого примитива.
2. Даёт «золотые» PNG для сверки с будущим Rust-рендером (задачи M1-4 и M1-9 в
   docs/ROADMAP.md): один и тот же DSL обязан давать тот же результат в оверлее.
3. Позволяет смотреть пресеты без приложения — контент галереи можно готовить уже сейчас.

Только стандартная библиотека (math, zlib, struct, json) — никаких зависимостей, работает
на любой ОС. Производительность не оптимизирована: это эталон семантики, а не прод-рендер.

Осознанно не реализовано (пишется предупреждение):
  * "text"  — рисуется заглушкой-прямоугольником (в stdlib нет шрифтов)
  * "image" — пропускается (декодирование base64-PNG появится в Rust-версии)
  * "animate"/"reactive" — рендерится кадр t=0, каналы мыши = 0
    (для анимаций/реактивности в Rust-рендере будет общий Expression VM, см. ARCHITECTURE §8.2)

Использование
-------------
    python tools/render_preview.py presets/examples/*.json --out assets/previews --scale 2
    python tools/render_preview.py p.ovcross.json --bg light --scale 4 -o out.png
"""
from __future__ import annotations

import argparse
import glob
import json
import math
import os
import struct
import sys
import zlib

TWO_PI = math.pi * 2.0


# --------------------------------------------------------------------------- утилиты

def clamp(x: float, lo: float, hi: float) -> float:
    return lo if x < lo else (hi if x > hi else x)


_WARNINGS: set[str] = set()


def warn(msg: str) -> None:
    if msg not in _WARNINGS:
        _WARNINGS.add(msg)
        print(f"  ! {msg}", file=sys.stderr)


def parse_color(value, default=(1.0, 1.0, 1.0, 1.0)):
    """#RGB / #RRGGBB / #RRGGBBAA → (r,g,b,a) в 0..1. Градиент → объект Gradient."""
    if value is None:
        return default
    if isinstance(value, dict):
        return Gradient(value)
    s = str(value).lstrip("#")
    try:
        if len(s) == 3:
            r, g, b = (int(c * 2, 16) for c in s)
            a = 255
        elif len(s) == 6:
            r, g, b = int(s[0:2], 16), int(s[2:4], 16), int(s[4:6], 16)
            a = 255
        elif len(s) == 8:
            r, g, b, a = int(s[0:2], 16), int(s[2:4], 16), int(s[4:6], 16), int(s[6:8], 16)
        else:
            raise ValueError(s)
    except ValueError:
        warn(f"не разобрал цвет {value!r} — беру белый")
        return default
    return (r / 255.0, g / 255.0, b / 255.0, a / 255.0)


class Gradient:
    def __init__(self, spec: dict):
        self.kind = spec.get("type", "linear")
        self.stops = sorted(
            ((float(st.get("at", 0.0)), parse_color(st["color"])) for st in spec.get("stops", [])),
            key=lambda s: s[0],
        ) or [(0.0, (1, 1, 1, 1)), (1.0, (1, 1, 1, 1))]
        self.frm = tuple(spec.get("from", [0.0, 0.0]))
        self.to = tuple(spec.get("to", [0.0, 1.0]))

    def color_at(self, x: float, y: float):
        if self.kind == "radial":
            cx, cy = self.frm
            radius = math.hypot(self.to[0] - cx, self.to[1] - cy) or 1e-6
            t = math.hypot(x - cx, y - cy) / radius
        else:
            ax, ay = self.frm
            dx, dy = self.to[0] - ax, self.to[1] - ay
            t = ((x - ax) * dx + (y - ay) * dy) / ((dx * dx + dy * dy) or 1e-6)
        t = clamp(t, 0.0, 1.0)
        for i in range(len(self.stops) - 1):
            t0, c0 = self.stops[i]
            t1, c1 = self.stops[i + 1]
            if t <= t1 or i == len(self.stops) - 2:
                k = clamp((t - t0) / ((t1 - t0) or 1e-6), 0.0, 1.0)
                return tuple(c0[j] + (c1[j] - c0[j]) * k for j in range(4))
        return self.stops[-1][1]


# --------------------------------------------------------------------------- буфер

class Buf:
    """Прямоугольный буфер, premultiplied RGBA, float 0..1."""

    __slots__ = ("w", "h", "px")

    def __init__(self, w: int, h: int):
        self.w, self.h = w, h
        self.px = [0.0] * (4 * w * h)

    def composite(self, other: "Buf", ox: int, oy: int, opacity: float = 1.0) -> None:
        """other (premultiplied) поверх self со смещением ox, oy."""
        if opacity <= 0.0:
            return
        sw, sh, spx = self.w, self.h, self.px
        ow, oh, opx = other.w, other.h, other.px
        y0, y1 = max(0, oy), min(sh, oy + oh)
        x0, x1 = max(0, ox), min(sw, ox + ow)
        for y in range(y0, y1):
            srow = (y - oy) * ow
            drow = y * sw
            for x in range(x0, x1):
                si = (srow + (x - ox)) << 2
                sa = opx[si + 3] * opacity
                if sa <= 0.0009:
                    continue
                di = (drow + x) << 2
                inv = 1.0 - sa
                spx[di] = opx[si] * opacity + spx[di] * inv
                spx[di + 1] = opx[si + 1] * opacity + spx[di + 1] * inv
                spx[di + 2] = opx[si + 2] * opacity + spx[di + 2] * inv
                spx[di + 3] = sa + spx[di + 3] * inv

    def blurred_alpha(self, radius: float) -> "Buf":
        """Новый буфер: альфа = дважды.box-blur нашей альфы, rgb = 0."""
        r = max(1, int(round(radius)))
        w, h, span = self.w, self.h, 2 * max(1, int(round(radius))) + 1
        src = [self.px[i + 3] for i in range(0, w * h * 4, 4)]
        tmp = [0.0] * (w * h)
        for y in range(h):
            row = y * w
            acc = sum(src[row + clamp(x, 0, w - 1)] for x in range(-r, r + 1))
            for x in range(w):
                tmp[row + x] = acc / span
                acc -= src[row + clamp(x - r, 0, w - 1)]
                acc += src[row + clamp(x + r + 1, 0, w - 1)]
        out = Buf(w, h)
        opx = out.px
        for x in range(w):
            acc = sum(tmp[clamp(y, 0, h - 1) * w + x] for y in range(-r, r + 1))
            for y in range(h):
                opx[(y * w + x) * 4 + 3] = acc / span
                acc -= tmp[clamp(y - r, 0, h - 1) * w + x]
                acc += tmp[clamp(y + r + 1, 0, h - 1) * w + x]
        return out

    def tint_alpha(self, rgba) -> None:
        """Альфа-маска → premultiplied цвет (для теней и свечения)."""
        r, g, b, a = rgba
        px = self.px
        for i in range(3, len(px), 4):
            al = px[i] * a
            px[i - 3], px[i - 2], px[i - 1], px[i] = r * al, g * al, b * al, al


# --------------------------------------------------------------------------- PNG

def write_png(path: str, buf: Buf) -> None:
    raw = bytearray()
    px = buf.px
    for y in range(buf.h):
        raw.append(0)
        row = y * buf.w
        for x in range(buf.w):
            i = (row + x) << 2
            a = clamp(px[i + 3], 0.0, 1.0)
            if a <= 0.0:
                raw += b"\x00\x00\x00\x00"
            else:
                raw.append(int(round(clamp(px[i] / a, 0.0, 1.0) * 255)))
                raw.append(int(round(clamp(px[i + 1] / a, 0.0, 1.0) * 255)))
                raw.append(int(round(clamp(px[i + 2] / a, 0.0, 1.0) * 255)))
                raw.append(int(round(a * 255)))

    def chunk(tag: bytes, data: bytes) -> bytes:
        return (struct.pack(">I", len(data)) + tag + data
                + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF))

    png = (b"\x89PNG\r\n\x1a\n"
           + chunk(b"IHDR", struct.pack(">IIBBBBB", buf.w, buf.h, 8, 6, 0, 0, 0))
           + chunk(b"IDAT", zlib.compress(bytes(raw), 9))
           + chunk(b"IEND", b""))
    parent = os.path.dirname(os.path.abspath(path))
    if parent:
        os.makedirs(parent, exist_ok=True)
    with open(path, "wb") as fh:
        fh.write(png)


# --------------------------------------------------------------------------- трансформ

class Transform:
    """Матрица 2×3 local → output px (m11 m12 m21 m22 tx ty) + norm для антиалиасинга."""

    __slots__ = ("m", "norm")

    def __init__(self, m, norm):
        self.m = m
        self.norm = norm

    def child(self, layer) -> "Transform":
        """Матрица слоя: scale → rotate → перенос на pos (в координатах родителя)."""
        px, py = (float(v) for v in layer.get("pos", [0.0, 0.0]))
        sx, sy = (float(v) for v in layer.get("scale", [1.0, 1.0]))
        c, s = _rot(float(layer.get("rotate", 0.0)))
        local = (c * sx, -s * sy, s * sx, c * sy, px, py)  # (m11,m12,m21,m22,tx,ty)
        return Transform(_mul(self.m, local), self.norm * (math.sqrt(abs(sx * sy)) or 1.0))

    def apply(self, x, y):
        m = self.m
        return (m[0] * x + m[1] * y + m[4], m[2] * x + m[3] * y + m[5])

    def unapply(self, X, Y):
        m = self.m
        det = (m[0] * m[3] - m[1] * m[2]) or 1e-9
        dx, dy = X - m[4], Y - m[5]
        return ((m[3] * dx - m[1] * dy) / det, (-m[2] * dx + m[0] * dy) / det)


def _rot(deg):
    a = math.radians(deg)
    return math.cos(a), math.sin(a)


def _mul(a, b):
    """(m11,m12,m21,m22,tx,ty): произведение a·b в той же конвенции."""
    return (a[0] * b[0] + a[1] * b[2], a[0] * b[1] + a[1] * b[3],
            a[2] * b[0] + a[3] * b[2], a[2] * b[1] + a[3] * b[3],
            a[0] * b[4] + a[1] * b[5] + a[4], a[2] * b[4] + a[3] * b[5] + a[5])


def flatten(layers, tf: Transform, opacity: float, out=None):
    """Раскрывает group в плоский список (layer, transform, накопленная opacity)."""
    out = [] if out is None else out
    for layer in layers:
        if layer.get("visible") is False:
            continue
        op = opacity * float(layer.get("opacity", 1.0))
        kind = layer.get("type")
        if kind == "group":
            flatten(layer.get("layers", []), tf.child(layer), op, out)
            continue
        if kind == "text":
            warn("слой 'text' рисуется заглушкой (в эталонном рендерере нет шрифтов)")
            size = float(layer.get("font", {}).get("size", 16))
            layer = dict(layer, type="rect", size=[size * 0.6 * max(1, len(layer.get("text", ""))), size])
        if kind == "image":
            warn("слой 'image' пропускается эталонным рендерером")
            continue
        out.append((layer, tf.child(layer), op))
    return out


# --------------------------------------------------------------------------- SDF

def sdf_rect(layer):
    w, h = float(layer["size"][0]), float(layer["size"][1])
    r = float(layer.get("radius", 0.0) or 0.0)
    anchor = layer.get("anchor", "center")
    ox = 0.0 if anchor == "center" else (-w / 2.0 if anchor == "top-left" else 0.0)
    oy = 0.0 if anchor == "center" else (-h / 2.0 if anchor == "top-left" else h / 2.0)
    hw, hh = w / 2.0, h / 2.0

    def f(x, y):
        dx = abs(x - ox) - hw + r
        dy = abs(y - oy) - hh + r
        return math.hypot(max(dx, 0.0), max(dy, 0.0)) + min(max(dx, dy), 0.0) - r

    return f, (-hw - 2, -hh - 2, hw + 2, hh + 2)


def sdf_circle(layer):
    r = abs(float(layer["r"]))
    return (lambda x, y: math.hypot(x, y) - r), (-r - 2, -r - 2, r + 2, r + 2)


def sdf_ring(layer):
    r, th = abs(float(layer["r"])), abs(float(layer["thickness"]))
    dash = layer.get("dash") or []
    on = float(dash[0]) if len(dash) >= 2 else 0.0
    off = float(dash[1]) if len(dash) >= 2 else 0.0
    period = (on + off) or 1e-6

    def f(x, y):
        rho = math.hypot(x, y)
        d = abs(rho - r) - th / 2.0
        if on > 0:
            arc = (math.atan2(y, x) % TWO_PI) * r
            if (arc % period) > on:
                return 1e3
        return d

    m = r + th / 2.0 + 2
    return f, (-m, -m, m, m)


def sdf_line(layer):
    ax, ay = float(layer["from"][0]), float(layer["from"][1])
    bx, by = float(layer["to"][0]), float(layer["to"][1])
    th = abs(float(layer["thickness"]))
    cap = layer.get("cap", "butt")
    dx, dy = bx - ax, by - ay
    L = math.hypot(dx, dy) or 1e-9
    ux, uy = dx / L, dy / L
    half = th / 2.0

    def f(x, y):
        rx, ry = x - ax, y - ay
        along = rx * ux + ry * uy
        perp = abs(rx * uy - ry * ux) - half
        if cap == "round":
            t = clamp(along, 0.0, L)
            return math.hypot(rx - ux * t, ry - uy * t) - half
        if cap == "square":
            return max(perp, -along - half, along - L - half)
        return max(perp, -along, along - L)

    m = half + th + 3
    return f, (min(ax, bx) - m, min(ay, by) - m, max(ax, bx) + m, max(ay, by) + m)


def sdf_arc(layer):
    r, th = abs(float(layer["r"])), abs(float(layer["thickness"]))
    a0 = math.radians(float(layer["startAngle"])) % TWO_PI
    a1 = math.radians(float(layer["endAngle"])) % TWO_PI

    def f(x, y):
        rho = math.hypot(x, y)
        radial = abs(rho - r) - th / 2.0
        ang = math.atan2(y, x) % TWO_PI
        inside = (a0 <= ang <= a1) if a0 <= a1 else (ang >= a0 or ang <= a1)
        if inside:
            return radial
        gap = min(abs(ang - a0), abs(ang - a1), TWO_PI - abs(ang - a0), TWO_PI - abs(ang - a1))
        return math.hypot(max(radial, 0.0), gap * r)

    m = r + th / 2.0 + 2
    return f, (-m, -m, m, m)


def _poly_sdf(pts, closed):
    if closed and len(pts) > 1 and pts[0] != pts[-1]:
        pts = pts + [pts[0]]

    def inside(px, py):
        c = False
        for i in range(len(pts) - 1):
            x1, y1 = pts[i]
            x2, y2 = pts[i + 1]
            if (y1 > py) != (y2 > py):
                xin = x1 + (py - y1) * (x2 - x1) / ((y2 - y1) or 1e-9)
                if px < xin:
                    c = not c
        return c

    def dist(px, py):
        best = 1e9
        for i in range(len(pts) - 1):
            x1, y1 = pts[i]
            x2, y2 = pts[i + 1]
            ex, ey = x2 - x1, y2 - y1
            t = clamp(((px - x1) * ex + (py - y1) * ey) / ((ex * ex + ey * ey) or 1e-9), 0.0, 1.0)
            best = min(best, math.hypot(px - (x1 + ex * t), py - (y1 + ey * t)))
        return best

    def f(x, y):
        d = dist(x, y)
        return -d if inside(x, y) else d

    return f


def sdf_poly(layer):
    pts = [(float(p[0]), float(p[1])) for p in layer["points"]]
    f = _poly_sdf(pts, layer.get("closed", True))
    xs = [p[0] for p in pts]
    ys = [p[1] for p in pts]
    return f, (min(xs) - 2, min(ys) - 2, max(xs) + 2, max(ys) + 2)


def path_to_polys(d: str):
    """Подмножество SVG path (M L H V C Q Z, абсолютные и относительные) → полилинии."""
    toks = [t for t in d.replace(",", " ").replace("-", " -").split() if t]
    polys, cur, x, y, sx, sy = [], [], 0.0, 0.0, 0.0, 0.0
    i, cmd = 0, "M"

    def nxt():
        nonlocal i
        v = float(toks[i])
        i += 1
        return v

    while i < len(toks):
        if toks[i][:1].isalpha():
            cmd = toks[i]
            i += 1
        rel, C = cmd.islower(), cmd.upper()
        if C == "M":
            if len(cur) > 1:
                polys.append(cur)
            nx, ny = nxt(), nxt()
            x, y = (x + nx, y + ny) if rel else (nx, ny)
            sx, sy, cur = x, y, [(x, y)]
        elif C == "L":
            nx, ny = nxt(), nxt()
            x, y = (x + nx, y + ny) if rel else (nx, ny)
            cur.append((x, y))
        elif C == "H":
            nx = nxt()
            x = x + nx if rel else nx
            cur.append((x, y))
        elif C == "V":
            ny = nxt()
            y = y + ny if rel else ny
            cur.append((x, y))
        elif C == "C":
            x1, y1, x2, y2, nx, ny = (nxt() for _ in range(6))
            if rel:
                x1, y1, x2, y2, nx, ny = x + x1, y + y1, x + x2, y + y2, x + nx, y + ny
            for k in range(1, 13):
                t, mt = k / 12.0, 1 - k / 12.0
                cur.append((mt ** 3 * x + 3 * mt ** 2 * t * x1 + 3 * mt * t ** 2 * x2 + t ** 3 * nx,
                            mt ** 3 * y + 3 * mt ** 2 * t * y1 + 3 * mt * t ** 2 * y2 + t ** 3 * ny))
            x, y = nx, ny
        elif C == "Q":
            x1, y1, nx, ny = (nxt() for _ in range(4))
            if rel:
                x1, y1, nx, ny = x + x1, y + y1, x + nx, y + ny
            for k in range(1, 11):
                t, mt = k / 10.0, 1 - k / 10.0
                cur.append((mt ** 2 * x + 2 * mt * t * x1 + t ** 2 * nx,
                            mt ** 2 * y + 2 * mt * t * y1 + t ** 2 * ny))
            x, y = nx, ny
        elif C == "Z":
            cur.append((sx, sy))
            x, y = sx, sy
        else:
            i += 1
    if len(cur) > 1:
        polys.append(cur)
    return polys


def sdf_path(layer):
    polys = path_to_polys(layer["d"])
    if not polys:
        raise ValueError("path без команд")
    sdfs = [_poly_sdf(p, True) for p in polys]

    def f(x, y):
        dmin, crossings = 1e9, 0
        for s in sdfs:
            d = s(x, y)
            dmin = min(dmin, abs(d))
            if d < 0:
                crossings += 1
        return -dmin if (crossings % 2 == 1) else dmin

    pts = [p for poly in polys for p in poly]
    xs = [p[0] for p in pts]
    ys = [p[1] for p in pts]
    return f, (min(xs) - 2, min(ys) - 2, max(xs) + 2, max(ys) + 2)


SDF_BUILDERS = {"rect": sdf_rect, "circle": sdf_circle, "ring": sdf_ring, "line": sdf_line,
                "arc": sdf_arc, "poly": sdf_poly, "path": sdf_path}


# --------------------------------------------------------------------------- растеризация

def shape_mask(layer, tf: Transform, expand: float, band: float | None):
    """Маска покрытия + её позиция в output px.

    expand — насколько расширить геометрию наружу (outline-проход, подложка свечения).
    band   — None: заполнение (d ≤ 0); число: полоса |d| ≤ band (обводка самого слоя).
    """
    builder = SDF_BUILDERS.get(layer.get("type"))
    if builder is None:
        warn(f"примитив {layer.get('type')!r} не поддерживается эталонным рендерером")
        return None
    sdf, (l0, t0, l1, b1) = builder(layer)
    pad = expand + (band or 0.0) + 2.0
    corners = [tf.apply(v, h) for v in (l0 - pad, l1 + pad) for h in (t0 - pad, b1 + pad)]
    x0 = int(math.floor(min(c[0] for c in corners)))
    y0 = int(math.floor(min(c[1] for c in corners)))
    x1 = int(math.ceil(max(c[0] for c in corners)))
    y1 = int(math.ceil(max(c[1] for c in corners)))
    w, h = max(1, x1 - x0), max(1, y1 - y0)
    mask = Buf(w, h)
    px = mask.px
    norm = tf.norm or 1.0
    ex = expand * norm
    bd = (band or 0.0) * norm
    for yy in range(h):
        Y = y0 + yy + 0.5
        row = yy * w
        for xx in range(w):
            lx, ly = tf.unapply(x0 + xx + 0.5, Y)
            d = sdf(lx, ly) * norm
            cov = clamp(0.5 - (d - ex), 0.0, 1.0) if band is None else clamp(0.5 - (abs(d) - bd - ex), 0.0, 1.0)
            if cov > 0.0:
                px[(row + xx) * 4 + 3] = cov
    return mask, x0, y0


def paint(mask: Buf, color, tf: Transform, ox: int, oy: int, opacity: float) -> Buf:
    """Маска → premultiplied цветной буфер (color — кортеж или Gradient)."""
    out = Buf(mask.w, mask.h)
    src, dst = mask.px, out.px
    gradient = hasattr(color, "color_at")
    for i in range(0, len(src), 4):
        a = src[i + 3] * opacity
        if a <= 0.0:
            continue
        if gradient:
            idx = i >> 2
            lx, ly = tf.unapply(ox + (idx % mask.w) + 0.5, oy + (idx // mask.w) + 0.5)
            r, g, b, ca = color.color_at(lx, ly)
            a *= ca
        else:
            r, g, b, ca = color
            a *= ca
        if a <= 0.0:
            continue
        dst[i], dst[i + 1], dst[i + 2], dst[i + 3] = r * a, g * a, b * a, a
    return out


def build_background(w: int, h: int, kind: str) -> Buf:
    buf = Buf(w, h)
    if kind == "none":
        return buf
    if kind == "checker":
        for y in range(h):
            for x in range(w):
                c = 0.24 if ((x // 8 + y // 8) % 2 == 0) else 0.17
                i = (y * w + x) * 4
                buf.px[i] = buf.px[i + 1] = buf.px[i + 2] = c
                buf.px[i + 3] = 1.0
        return buf
    palette = {"dark": ((0.086, 0.102, 0.133), (0.031, 0.039, 0.055)),
               "light": ((0.94, 0.95, 0.97), (0.78, 0.81, 0.86)),
               "game": ((0.19, 0.24, 0.17), (0.043, 0.055, 0.043))}[kind]
    top, bottom = palette
    for y in range(h):
        t = y / max(1, h - 1)
        r = top[0] + (bottom[0] - top[0]) * t
        g = top[1] + (bottom[1] - top[1]) * t
        b = top[2] + (bottom[2] - top[2]) * t
        for x in range(w):
            i = (y * w + x) * 4
            buf.px[i], buf.px[i + 1], buf.px[i + 2], buf.px[i + 3] = r, g, b, 1.0
    return buf


def render_preset(doc: dict, scale: float = 2.0, bg: str = "dark") -> Buf:
    canvas = doc.get("canvas", {})
    cw, ch = float(canvas.get("size", [128, 128])[0]), float(canvas.get("size", [128, 128])[1])
    origin = canvas.get("origin", [cw / 2.0, ch / 2.0])
    ox, oy = float(origin[0]), float(origin[1])
    W, H = max(1, int(round(cw * scale))), max(1, int(round(ch * scale)))
    out_scale = W / cw

    style = doc.get("style", {})
    root = Transform((out_scale, 0.0, 0.0, out_scale, ox * out_scale, oy * out_scale), out_scale)
    items = flatten(doc.get("layers", []), root, float(style.get("opacity", 1.0)))

    buf = build_background(W, H, bg)

    # 1) глобальное свечение — под всеми слоями
    glow = style.get("glow") or {}
    if glow.get("enabled") and float(glow.get("radius", 0)) > 0:
        radius = float(glow.get("radius", 4))
        gbuf = Buf(W, H)
        for layer, tf, op in items:
            got = shape_mask(layer, tf, expand=radius, band=None)
            if not got:
                continue
            mask, x0, y0 = got
            soft = mask.blurred_alpha(radius * out_scale)
            soft.tint_alpha(parse_color(glow.get("color", "#FFFFFF")))
            gbuf.composite(soft, x0, y0, op)
        buf.composite(gbuf, 0, 0, 1.0)

    # 2) единая обводка — под слоями, поверх свечения
    outline = style.get("outline") or {}
    if outline.get("enabled", True) and float(outline.get("width", 0)) > 0:
        ow = float(outline.get("width", 1))
        ocolor = parse_color(outline.get("color", "#000000B3"))
        obuf = Buf(W, H)
        for layer, tf, op in items:
            sw = float(layer.get("strokeWidth", 0) or 0)
            got = shape_mask(layer, tf, expand=ow + sw / 2.0, band=None)
            if not got:
                continue
            mask, x0, y0 = got
            obuf.composite(paint(mask, ocolor, tf, x0, y0, op), x0, y0)
        buf.composite(obuf, 0, 0, 1.0)

    # 3) слои снизу вверх; тень каждого слоя — под ним
    for layer, tf, op in items:
        shadow = layer.get("shadow") or {}
        if shadow.get("enabled", True) and (shadow.get("color") or float(shadow.get("blur", 0) or 0) > 0):
            blur = float(shadow.get("blur", 4) or 0)
            got = shape_mask(layer, tf, expand=blur, band=None)
            if got:
                mask, x0, y0 = got
                soft = mask.blurred_alpha(blur * out_scale)
                soft.tint_alpha(parse_color(shadow.get("color", "#000000FF")))
                off = shadow.get("offset", [0, 0])
                buf.composite(soft, int(x0 + float(off[0]) * out_scale),
                              int(y0 + float(off[1]) * out_scale), op)
        got = shape_mask(layer, tf, expand=0.0, band=None)
        if not got:
            continue
        mask, x0, y0 = got
        if layer.get("fill") is not None:
            buf.composite(paint(mask, parse_color(layer["fill"]), tf, x0, y0, op), x0, y0)
        sw = float(layer.get("strokeWidth", 0) or 0)
        if sw > 0 and layer.get("stroke"):
            band = shape_mask(layer, tf, expand=0.0, band=sw / 2.0)
            if band:
                m2, bx, by = band
                buf.composite(paint(m2, parse_color(layer["stroke"]), tf, bx, by, op), bx, by)
    return buf


# --------------------------------------------------------------------------- CLI

def main(argv):
    ap = argparse.ArgumentParser(description="Референсный рендер Overley Crosshair DSL v1 → PNG")
    ap.add_argument("presets", nargs="+", help="файлы пресетов (можно glob)")
    ap.add_argument("-o", "--out", default="assets/previews", help="каталог или .png-файл вывода")
    ap.add_argument("--scale", type=float, default=2.0, help="масштаб вывода (2 → 256px для холста 128)")
    ap.add_argument("--bg", default="dark", choices=["dark", "light", "game", "checker", "none"])
    ap.add_argument("--suffix", default="", help="суффикс имени файла, например -light")
    args = ap.parse_args(argv)

    files = []
    for pattern in args.presets:
        hit = sorted(glob.glob(pattern))
        files.extend(hit or ([pattern] if os.path.exists(pattern) else []))
    if not files:
        print("не найдено ни одного файла", file=sys.stderr)
        return 2

    failed = 0
    for path in files:
        try:
            with open(path, encoding="utf-8") as fh:
                doc = json.load(fh)
        except (OSError, json.JSONDecodeError) as exc:
            print(f"FAIL {path}: {exc}", file=sys.stderr)
            failed += 1
            continue
        name = os.path.basename(path).split(".ovcross")[0]
        out_path = args.out if args.out.endswith(".png") else os.path.join(args.out, f"{name}{args.suffix}.png")
        buf = render_preset(doc, scale=args.scale, bg=args.bg)
        write_png(out_path, buf)
        base = build_background(buf.w, buf.h, args.bg)
        drawn = sum(1 for i in range(0, len(buf.px), 4)
                    if abs(buf.px[i + 3] - base.px[i + 3]) > 0.004
                    or abs(buf.px[i] - base.px[i]) > 0.004)
        flag = "" if drawn else "  ⚠ ПУСТОЙ КАДР"
        print(f"{os.path.relpath(path)} → {out_path}  ({buf.w}×{buf.h}, px с покрытием: {drawn}){flag}")
        if not drawn:
            failed += 1
    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
