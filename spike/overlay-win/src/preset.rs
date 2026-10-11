//! Чтение `.ovcross` (подмножество DSL v1) для спайка.
//!
//! Поддерживаются примитивы rect/circle/ring/line и поля, нужные для проверки оверлея:
//! canvas, style.outline, behavior.mode/offset, слои с pos/rotate/scale/opacity/fill/stroke.
//! Остальное (анимации, реактивность, path/poly/text/image) спайком игнорируется —
//! они появятся в полноценном DSL-компиляторе на M1 (docs/ROADMAP.md, M1-5/M1-6).

// FIXME-spike: serde здесь только для спайка; в M1 парсинг DSL живёт в crate `dsl`
// вместе со схемной валидацией (общая с tools/validate_presets.py).
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color(pub f32, pub f32, pub f32, pub f32);

impl Color {
    pub const WHITE: Color = Color(1.0, 1.0, 1.0, 1.0);

    /// #RGB / #RRGGBB / #RRGGBBAA
    pub fn parse(s: &str) -> Option<Color> {
        let s = s.trim().trim_start_matches('#');
        let h = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).ok();
        match s.len() {
            3 => Some(Color(
                h(0).map(|v| v as f32 * 17.0 / 255.0)?,
                h(1).map(|v| v as f32 * 17.0 / 255.0)?,
                h(2).map(|v| v as f32 * 17.0 / 255.0)?,
                1.0,
            )),
            6 => Some(Color(h(0)? as f32 / 255.0, h(2)? as f32 / 255.0, h(4)? as f32 / 255.0, 1.0)),
            8 => Some(Color(
                h(0)? as f32 / 255.0,
                h(2)? as f32 / 255.0,
                h(4)? as f32 / 255.0,
                h(6)? as f32 / 255.0,
            )),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct LayerRaw {
    pub id: Option<String>,
    #[serde(rename = "type")]
    pub kind: String,
    pub fill: Option<String>,
    pub stroke: Option<String>,
    #[serde(rename = "strokeWidth")]
    pub stroke_width: Option<f32>,
    pub pos: Option<[f32; 2]>,
    pub rotate: Option<f32>,
    pub scale: Option<[f32; 2]>,
    pub opacity: Option<f32>,
    // rect
    pub size: Option<[f32; 2]>,
    pub radius: Option<f32>,
    // circle / ring
    pub r: Option<f32>,
    pub thickness: Option<f32>,
    // line
    pub from: Option<[f32; 2]>,
    pub to: Option<[f32; 2]>,
    pub cap: Option<String>,
}

impl Default for LayerRaw {
    fn default() -> Self {
        Self {
            id: None, kind: String::new(), fill: None, stroke: None, stroke_width: None,
            pos: None, rotate: None, scale: None, opacity: None, size: None, radius: None,
            r: None, thickness: None, from: None, to: None, cap: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct CanvasRaw {
    pub size: Option<[f32; 2]>,
    pub origin: Option<[f32; 2]>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct OutlineRaw {
    pub color: Option<String>,
    pub width: Option<f32>,
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct StyleRaw {
    pub outline: Option<OutlineRaw>,
    pub opacity: Option<f32>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct BehaviorRaw {
    pub mode: Option<String>,
    pub offset: Option<[f32; 2]>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct PresetRaw {
    pub format: String,
    pub version: i32,
    pub canvas: Option<CanvasRaw>,
    pub style: Option<StyleRaw>,
    pub layers: Vec<LayerRaw>,
    pub behavior: Option<BehaviorRaw>,
}

impl Default for PresetRaw {
    fn default() -> Self {
        Self { format: String::new(), version: 0, canvas: None, style: None, layers: vec![], behavior: None }
    }
}

// --------------------------------------------------------------------------- нормализация

#[derive(Debug, Clone)]
pub enum Kind {
    Rect { w: f32, h: f32, radius: f32 },
    Circle { r: f32 },
    Ring { r: f32, thickness: f32 },
    Line { from: [f32; 2], to: [f32; 2], thickness: f32, round_cap: bool },
}

#[derive(Debug, Clone)]
pub struct Layer {
    pub kind: Kind,
    pub fill: Color,
    pub stroke: Option<Color>,
    pub stroke_width: f32,
    pub pos: [f32; 2],
    pub rotate: f32,
    pub scale: [f32; 2],
    pub opacity: f32,
}

#[derive(Debug, Clone)]
pub struct Preset {
    pub canvas: [f32; 2],
    pub origin: [f32; 2],
    pub outline_color: Color,
    pub outline_width: f32,
    pub global_opacity: f32,
    pub mode: Mode,
    pub offset: [f32; 2],
    pub layers: Vec<Layer>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Center,
    Cursor,
}

pub struct LoadError(pub String);

pub fn load(path: &str) -> Result<Preset, LoadError> {
    let text = std::fs::read_to_string(path).map_err(|e| LoadError(format!("{path}: {e}")))?;
    parse_str(&text).map_err(|e| LoadError(format!("{path}: {}", e.0)))
}

/// Разбор DSL v1 из строки (использует и файл, и канал управления UI→ядро).
pub fn parse_str(text: &str) -> Result<Preset, LoadError> {
    let raw: PresetRaw =
        serde_json::from_str(text).map_err(|e| LoadError(format!("не DSL v1: {e}")))?;
    if raw.format != "overley.crosshair" {
        return Err(LoadError(format!("format = {:?}, ожидался \"overley.crosshair\"", raw.format)));
    }
    if raw.version != 1 {
        return Err(LoadError(format!("version = {}, поддерживается только 1", raw.version)));
    }
    let canvas = raw.canvas.as_ref().and_then(|c| c.size).unwrap_or([128.0, 128.0]);
    let origin = raw
        .canvas
        .as_ref()
        .and_then(|c| c.origin)
        .unwrap_or([canvas[0] / 2.0, canvas[1] / 2.0]);

    let (outline_color, outline_width) = match raw.style.as_ref().and_then(|s| s.outline.as_ref()) {
        Some(o) if o.enabled.unwrap_or(true) => (
            o.color.as_deref().and_then(Color::parse).unwrap_or(Color(0.0, 0.0, 0.0, 0.7)),
            o.width.unwrap_or(1.0),
        ),
        _ => (Color(0.0, 0.0, 0.0, 0.0), 0.0),
    };

    let mut layers = Vec::new();
    for l in &raw.layers {
        let kind = match l.kind.as_str() {
            "rect" => {
                let s = l.size.ok_or_else(|| LoadError(format!("rect без size ({:?})", l.id)))?;
                Kind::Rect { w: s[0], h: s[1], radius: l.radius.unwrap_or(0.0) }
            }
            "circle" => Kind::Circle { r: l.r.ok_or_else(|| LoadError(format!("circle без r ({:?})", l.id)))? },
            "ring" => Kind::Ring {
                r: l.r.ok_or_else(|| LoadError(format!("ring без r ({:?})", l.id)))?,
                thickness: l.thickness.ok_or_else(|| LoadError(format!("ring без thickness ({:?})", l.id)))?,
            },
            "line" => Kind::Line {
                from: l.from.ok_or_else(|| LoadError(format!("line без from ({:?})", l.id)))?,
                to: l.to.ok_or_else(|| LoadError(format!("line без to ({:?})", l.id)))?,
                thickness: l.thickness.ok_or_else(|| LoadError(format!("line без thickness ({:?})", l.id)))?,
                round_cap: l.cap.as_deref() == Some("round"),
            },
            other => {
                eprintln!("  ! слой {:?} типа {other:?} спайком не рисуется — пропущен", l.id);
                continue;
            }
        };
        layers.push(Layer {
            kind,
            fill: l.fill.as_deref().and_then(Color::parse).unwrap_or(Color::WHITE),
            stroke: l.stroke.as_deref().and_then(Color::parse),
            stroke_width: l.stroke_width.unwrap_or(0.0),
            pos: l.pos.unwrap_or([0.0, 0.0]),
            rotate: l.rotate.unwrap_or(0.0),
            scale: l.scale.unwrap_or([1.0, 1.0]),
            opacity: l.opacity.unwrap_or(1.0),
        });
    }
    if layers.is_empty() {
        return Err(LoadError("нет поддерживаемых слоёв".into()));
    }

    let behavior = raw.behavior.clone().unwrap_or_default();
    Ok(Preset {
        canvas,
        origin,
        outline_color,
        outline_width,
        global_opacity: raw.style.as_ref().and_then(|s| s.opacity).unwrap_or(1.0),
        mode: match behavior.mode.as_deref() {
            Some("cursor") => Mode::Cursor,
            _ => Mode::Center,
        },
        offset: behavior.offset.unwrap_or([0.0, 0.0]),
        layers,
    })
}
