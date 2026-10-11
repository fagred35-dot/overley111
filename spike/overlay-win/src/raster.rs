//! CPU-растеризация подмножества DSL v1 → premultiplied BGRA (top-down).
//!
//! Это «черновой» рендер для спайка: он нужен, чтобы окно вообще что-то показало.
//! Эталон семантики — tools/render_preview.py (Python); на M1 оба заменяются
//! общим DrawList-компилятором с D2D-бэкендом и tiny-skia-бэкендом для golden-тестов
//! (docs/ARCHITECTURE.md §8, §15).

use crate::preset::{Color, Kind, Layer, Preset};

const AA: f32 = 0.5;

pub struct Frame {
    pub width: u32,
    pub height: u32,
    /// premultiplied BGRA, строки сверху вниз (для DIB с отрицательным biHeight)
    pub pixels: Vec<u8>,
}

struct Ctx {
    w: u32,
    h: u32,
    px: Vec<f32>, // premultiplied RGBA float
}

impl Ctx {
    fn blend_mask(&mut self, _layer: &Layer, m: &[f32], x0: i32, y0: i32, color: Color, opacity: f32) {
        let (mw, mh) = (m[0] as i32, m[1] as i32);
        let (r, g, b, ca) = (color.0, color.1, color.2, color.3);
        for yy in 0..mh {
            let dy = y0 + yy;
            if dy < 0 || dy >= self.h as i32 {
                continue;
            }
            for xx in 0..mw {
                let dx = x0 + xx;
                if dx < 0 || dx >= self.w as i32 {
                    continue;
                }
                let cov = m[2 + (yy as usize * mw as usize + xx as usize)];
                if cov <= 0.001 {
                    continue;
                }
                let a = (cov * opacity * ca).clamp(0.0, 1.0);
                let i = ((dy as u32 * self.w + dx as u32) * 4) as usize;
                let inv = 1.0 - a;
                self.px[i] = r * a + self.px[i] * inv;
                self.px[i + 1] = g * a + self.px[i + 1] * inv;
                self.px[i + 2] = b * a + self.px[i + 2] * inv;
                self.px[i + 3] = a + self.px[i + 3] * inv;
            }
        }
    }
}

/// Маска: [w, h, cov...] в координатах output.
fn mask(layer: &Layer, p: &Preset, out: f32, expand: f32, band: Option<f32>) -> (Vec<f32>, i32, i32) {
    // локальная система слоя → холст → output
    let (sx, sy) = (layer.scale[0], layer.scale[1]);
    let a = layer.rotate.to_radians();
    let (c, s) = (a.cos(), a.sin());
    let m11 = out * c * sx;
    let m12 = -out * s * sy;
    let m21 = out * s * sx;
    let m22 = out * c * sy;
    let tx = out * (p.origin[0] + layer.pos[0]);
    let ty = out * (p.origin[1] + layer.pos[1]);
    let det = m11 * m22 - m12 * m21;
    let norm = (out * (sx * sy).abs().sqrt()).max(1e-6);

    let (l0, t0, l1, b1) = local_bbox(&layer.kind);
    let pad = expand + band.unwrap_or(0.0) + 2.0;
    let mut xs = [0.0f32; 4];
    let mut ys = [0.0f32; 4];
    let corners = [(l0 - pad, t0 - pad), (l1 + pad, t0 - pad), (l0 - pad, b1 + pad), (l1 + pad, b1 + pad)];
    for (i, (cx, cy)) in corners.iter().enumerate() {
        xs[i] = m11 * cx + m12 * cy + tx;
        ys[i] = m21 * cx + m22 * cy + ty;
    }
    let x0 = xs.iter().cloned().fold(f32::INFINITY, f32::min).floor() as i32;
    let y0 = ys.iter().cloned().fold(f32::INFINITY, f32::min).floor() as i32;
    let x1 = xs.iter().cloned().fold(f32::NEG_INFINITY, f32::max).ceil() as i32;
    let y1 = ys.iter().cloned().fold(f32::NEG_INFINITY, f32::max).ceil() as i32;
    let (w, h) = ((x1 - x0).max(1), (y1 - y0).max(1));

    let mut buf = vec![0.0f32; 2 + (w as usize * h as usize)];
    buf[0] = w as f32;
    buf[1] = h as f32;
    let ex = expand * norm;
    let bd = band.unwrap_or(0.0) * norm;
    for yy in 0..h {
        let y = y0 as f32 + yy as f32 + 0.5;
        for xx in 0..w {
            let x = x0 as f32 + xx as f32 + 0.5;
            let dx = x - tx;
            let dy = y - ty;
            let lx = (m22 * dx - m12 * dy) / det;
            let ly = (-m21 * dx + m11 * dy) / det;
            let d = sdf(&layer.kind, lx, ly) * norm;
            let cov = match band {
                None => (AA - (d - ex)).clamp(0.0, 1.0),
                Some(_) => (AA - (d.abs() - bd - ex)).clamp(0.0, 1.0),
            };
            if cov > 0.0 {
                buf[2 + (yy as usize * w as usize + xx as usize)] = cov;
            }
        }
    }
    (buf, x0, y0)
}

fn local_bbox(kind: &Kind) -> (f32, f32, f32, f32) {
    match kind {
        Kind::Rect { w, h, .. } => (-w / 2.0 - 2.0, -h / 2.0 - 2.0, w / 2.0 + 2.0, h / 2.0 + 2.0),
        Kind::Circle { r } => (-r - 2.0, -r - 2.0, r + 2.0, r + 2.0),
        Kind::Ring { r, thickness } => {
            let m = r + thickness / 2.0 + 2.0;
            (-m, -m, m, m)
        }
        Kind::Line { from, to, thickness, .. } => {
            let m = thickness / 2.0 + 3.0;
            (
                from[0].min(to[0]) - m,
                from[1].min(to[1]) - m,
                from[0].max(to[0]) + m,
                from[1].max(to[1]) + m,
            )
        }
    }
}

fn sdf(kind: &Kind, x: f32, y: f32) -> f32 {
    match kind {
        Kind::Rect { w, h, radius } => {
            let r = *radius;
            let dx = (x.abs() - w / 2.0 + r).max(0.0);
            let dy = (y.abs() - h / 2.0 + r).max(0.0);
            (dx * dx + dy * dy).sqrt() + (x.abs() - w / 2.0 + r).max(y.abs() - h / 2.0 + r).min(0.0) - r
        }
        Kind::Circle { r } => (x * x + y * y).sqrt() - r,
        Kind::Ring { r, thickness } => ((x * x + y * y).sqrt() - r).abs() - thickness / 2.0,
        Kind::Line { from, to, thickness, round_cap } => {
            let (ax, ay) = (from[0], from[1]);
            let (dx, dy) = (to[0] - ax, to[1] - ay);
            let l = (dx * dx + dy * dy).sqrt().max(1e-6);
            let (ux, uy) = (dx / l, dy / l);
            let (rx, ry) = (x - ax, y - ay);
            let along = rx * ux + ry * uy;
            let perp = (rx * uy - ry * ux).abs() - thickness / 2.0;
            if *round_cap {
                let t = along.clamp(0.0, l);
                let px = rx - ux * t;
                let py = ry - uy * t;
                (px * px + py * py).sqrt() - thickness / 2.0
            } else {
                perp.max(-along).max(along - l)
            }
        }
    }
}

pub fn render(p: &Preset, out_scale: f32) -> Frame {
    let w = (p.canvas[0] * out_scale).round().max(1.0) as u32;
    let h = (p.canvas[1] * out_scale).round().max(1.0) as u32;
    let mut ctx = Ctx { w, h, px: vec![0.0f32; (w * h * 4) as usize] };

    // 1) единая обводка под слоями
    if p.outline_width > 0.0 && p.outline_color.3 > 0.0 {
        for l in &p.layers {
            let sw = l.stroke_width;
            let (m, x0, y0) = mask(l, p, out_scale, p.outline_width + sw / 2.0, None);
            ctx.blend_mask(l, &m, x0, y0, p.outline_color, p.global_opacity * l.opacity);
        }
    }
    // 2) слои: заливка, затем обводка самого слоя
    for l in &p.layers {
        let op = p.global_opacity * l.opacity;
        let (m, x0, y0) = mask(l, p, out_scale, 0.0, None);
        ctx.blend_mask(l, &m, x0, y0, l.fill, op);
        if l.stroke_width > 0.0 {
            if let Some(sc) = l.stroke {
                let (m2, x0b, y0b) = mask(l, p, out_scale, 0.0, Some(l.stroke_width / 2.0));
                ctx.blend_mask(l, &m2, x0b, y0b, sc, op);
            }
        }
    }

    // premultiplied RGBA float → BGRA u8
    let mut pixels = vec![0u8; (w * h * 4) as usize];
    for i in 0..(w * h) as usize {
        let a = ctx.px[i * 4 + 3].clamp(0.0, 1.0);
        pixels[i * 4] = (ctx.px[i * 4 + 2] * 255.0).round() as u8; // B
        pixels[i * 4 + 1] = (ctx.px[i * 4 + 1] * 255.0).round() as u8; // G
        pixels[i * 4 + 2] = (ctx.px[i * 4] * 255.0).round() as u8; // R
        pixels[i * 4 + 3] = (a * 255.0).round() as u8; // A
    }
    Frame { width: w, height: h, pixels }
}
