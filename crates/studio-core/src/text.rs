//! Type layers: layout and rasterisation with the bundled fonts.

use std::sync::OnceLock;

use ab_glyph::{point, Font, FontRef, PxScale, ScaleFont};
use serde::{Deserialize, Serialize};

use crate::color::Rgba;
use crate::geom::Rect;
use crate::image::Image;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FontFace {
    #[default]
    Sans,
    SansMedium,
    SansBold,
    SansItalic,
    Serif,
    SerifBold,
    Mono,
}

impl FontFace {
    pub const ALL: [FontFace; 7] =
        [Self::Sans, Self::SansMedium, Self::SansBold, Self::SansItalic, Self::Serif, Self::SerifBold, Self::Mono];

    pub fn label(&self) -> &'static str {
        match self {
            Self::Sans => "Inter Regular",
            Self::SansMedium => "Inter Medium",
            Self::SansBold => "Inter Bold",
            Self::SansItalic => "Inter Italic",
            Self::Serif => "Noto Serif",
            Self::SerifBold => "Noto Serif Bold",
            Self::Mono => "JetBrains Mono",
        }
    }

    pub fn bytes(&self) -> &'static [u8] {
        match self {
            Self::Sans => include_bytes!("../../../assets/fonts/Inter-Regular.ttf"),
            Self::SansMedium => include_bytes!("../../../assets/fonts/Inter-Medium.ttf"),
            Self::SansBold => include_bytes!("../../../assets/fonts/Inter-Bold.ttf"),
            Self::SansItalic => include_bytes!("../../../assets/fonts/Inter-Italic.ttf"),
            Self::Serif => include_bytes!("../../../assets/fonts/NotoSerif-Regular.ttf"),
            Self::SerifBold => include_bytes!("../../../assets/fonts/NotoSerif-Bold.ttf"),
            Self::Mono => include_bytes!("../../../assets/fonts/JetBrainsMono-Regular.ttf"),
        }
    }

    fn font(&self) -> &'static FontRef<'static> {
        static FONTS: OnceLock<Vec<FontRef<'static>>> = OnceLock::new();
        let all = FONTS.get_or_init(|| {
            FontFace::ALL.iter().map(|f| FontRef::try_from_slice(f.bytes()).expect("bundled font")).collect()
        });
        &all[FontFace::ALL.iter().position(|f| f == self).unwrap()]
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextLayer {
    pub text: String,
    pub face: FontFace,
    /// Pixel size of the em.
    pub size: f32,
    pub color: Rgba,
    /// Top-left of the text block.
    pub x: f32,
    pub y: f32,
    pub align: Align,
    /// Line height as a multiple of the font's natural line spacing.
    pub leading: f32,
    /// Extra letter spacing in thousandths of an em.
    pub tracking: f32,
    /// Wrap width for paragraph text; `None` for point text.
    pub box_width: Option<f32>,
}

impl TextLayer {
    pub fn new(text: impl Into<String>, x: f32, y: f32, size: f32, color: Rgba) -> Self {
        TextLayer {
            text: text.into(),
            face: FontFace::Sans,
            size,
            color,
            x,
            y,
            align: Align::Left,
            leading: 1.0,
            tracking: 0.0,
            box_width: None,
        }
    }
}

struct Line {
    glyphs: Vec<(ab_glyph::GlyphId, f32)>,
    width: f32,
}

fn layout(t: &TextLayer) -> (Vec<Line>, f32, f32) {
    let font = t.face.font();
    let sf = font.as_scaled(PxScale::from(t.size.max(1.0)));
    let track = t.tracking / 1000.0 * t.size;
    let line_h = (sf.ascent() - sf.descent() + sf.line_gap()) * t.leading.max(0.1);
    let measure = |s: &str| -> Line {
        let mut x = 0.0;
        let mut glyphs = Vec::new();
        let mut prev = None;
        for ch in s.chars() {
            let id = sf.glyph_id(ch);
            if let Some(p) = prev {
                x += sf.kern(p, id);
            }
            glyphs.push((id, x));
            x += sf.h_advance(id) + track;
            prev = Some(id);
        }
        Line { glyphs, width: (x - track).max(0.0) }
    };
    let mut lines = Vec::new();
    for para in t.text.split('\n') {
        match t.box_width {
            Some(bw) if bw > 0.0 => {
                let mut cur = String::new();
                for word in para.split(' ') {
                    let cand = if cur.is_empty() { word.to_string() } else { format!("{cur} {word}") };
                    if !cur.is_empty() && measure(&cand).width > bw {
                        lines.push(measure(&cur));
                        cur = word.to_string();
                    } else {
                        cur = cand;
                    }
                }
                lines.push(measure(&cur));
            }
            _ => lines.push(measure(para)),
        }
    }
    (lines, line_h, sf.ascent())
}

/// The text block's bounds in canvas pixels.
pub fn bounds(t: &TextLayer) -> Rect {
    let (lines, line_h, _) = layout(t);
    let width = t.box_width.unwrap_or_else(|| lines.iter().map(|l| l.width).fold(0.0, f32::max));
    Rect::new(t.x.floor() as i32, t.y.floor() as i32, width.ceil().max(1.0) as u32, (line_h * lines.len() as f32).ceil().max(1.0) as u32)
}

pub fn render(t: &TextLayer, w: u32, h: u32) -> Image {
    let mut cov = vec![0f32; w as usize * h as usize];
    let font = t.face.font();
    let scale = PxScale::from(t.size.max(1.0));
    let (lines, line_h, ascent) = layout(t);
    let block_w = t.box_width.unwrap_or_else(|| lines.iter().map(|l| l.width).fold(0.0, f32::max));
    for (li, line) in lines.iter().enumerate() {
        let dx = match t.align {
            Align::Left => 0.0,
            Align::Center => (block_w - line.width) / 2.0,
            Align::Right => block_w - line.width,
        };
        let baseline = t.y + ascent + li as f32 * line_h;
        for &(id, gx) in &line.glyphs {
            let g = id.with_scale_and_position(scale, point(t.x + dx + gx, baseline));
            if let Some(og) = font.outline_glyph(g) {
                let b = og.px_bounds();
                og.draw(|px, py, c| {
                    let x = b.min.x as i32 + px as i32;
                    let y = b.min.y as i32 + py as i32;
                    if x >= 0 && y >= 0 && (x as u32) < w && (y as u32) < h {
                        let i = y as usize * w as usize + x as usize;
                        cov[i] = (cov[i] + c).min(1.0);
                    }
                });
            }
        }
    }
    let mut data = Vec::with_capacity(cov.len() * 4);
    for c in cov {
        data.extend_from_slice(&[t.color.r(), t.color.g(), t.color.b(), (c * t.color.a() as f32).round() as u8]);
    }
    Image::from_raw(w, h, data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_ink_inside_bounds() {
        let t = TextLayer::new("Hello", 10.0, 10.0, 32.0, Rgba::BLACK);
        let img = render(&t, 200, 80);
        let ink = img.opaque_bounds().expect("some ink");
        let b = bounds(&t);
        assert!(ink.x >= b.x - 2 && ink.right() <= b.right() + 2, "{ink:?} vs {b:?}");
    }

    #[test]
    fn wraps_paragraph_text() {
        let mut t = TextLayer::new("one two three four five six", 0.0, 0.0, 20.0, Rgba::BLACK);
        let single = bounds(&t);
        t.box_width = Some(80.0);
        let wrapped = bounds(&t);
        assert!(wrapped.h > single.h * 2);
    }
}
