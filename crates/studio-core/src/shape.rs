//! Vector shape layers and path rasterisation (via tiny-skia).

use serde::{Deserialize, Serialize};
use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Stroke, StrokeDash, Transform};

use crate::color::Rgba;
use crate::image::{Image, Mask};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum ShapeKind {
    Rectangle,
    RoundedRect { radius: f32 },
    Ellipse,
    Line,
    Polygon { sides: u32 },
    Star { points: u32, inner: f32 },
    Triangle,
    Heart,
    Arrow,
    SpeechBubble,
    Checkmark,
}

impl ShapeKind {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Rectangle => "Rectangle",
            Self::RoundedRect { .. } => "Rounded Rectangle",
            Self::Ellipse => "Ellipse",
            Self::Line => "Line",
            Self::Polygon { .. } => "Polygon",
            Self::Star { .. } => "Star",
            Self::Triangle => "Triangle",
            Self::Heart => "Heart",
            Self::Arrow => "Arrow",
            Self::SpeechBubble => "Speech Bubble",
            Self::Checkmark => "Checkmark",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ShapeLayer {
    pub kind: ShapeKind,
    /// Drag start and end in canvas pixels.
    pub from: (f32, f32),
    pub to: (f32, f32),
    pub fill: Option<Rgba>,
    pub stroke: Option<Rgba>,
    pub stroke_width: f32,
    pub dashed: bool,
}

fn color(c: Rgba) -> tiny_skia::Color {
    tiny_skia::Color::from_rgba8(c.r(), c.g(), c.b(), c.a())
}

/// The outline of `kind` within the box spanned by `a` and `b`.
pub fn path(kind: ShapeKind, a: (f32, f32), b: (f32, f32)) -> Option<tiny_skia::Path> {
    let (x0, y0) = (a.0.min(b.0), a.1.min(b.1));
    let (x1, y1) = (a.0.max(b.0), a.1.max(b.1));
    let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    let (rx, ry) = ((x1 - x0) / 2.0, (y1 - y0) / 2.0);
    let mut pb = PathBuilder::new();
    match kind {
        ShapeKind::Rectangle => {
            return Some(PathBuilder::from_rect(tiny_skia::Rect::from_ltrb(
                x0,
                y0,
                x1.max(x0 + 0.5),
                y1.max(y0 + 0.5),
            )?));
        }
        ShapeKind::RoundedRect { radius } => {
            let r = radius.min(rx).min(ry).max(0.0);
            let k = 0.552_284_8 * r;
            pb.move_to(x0 + r, y0);
            pb.line_to(x1 - r, y0);
            pb.cubic_to(x1 - r + k, y0, x1, y0 + r - k, x1, y0 + r);
            pb.line_to(x1, y1 - r);
            pb.cubic_to(x1, y1 - r + k, x1 - r + k, y1, x1 - r, y1);
            pb.line_to(x0 + r, y1);
            pb.cubic_to(x0 + r - k, y1, x0, y1 - r + k, x0, y1 - r);
            pb.line_to(x0, y0 + r);
            pb.cubic_to(x0, y0 + r - k, x0 + r - k, y0, x0 + r, y0);
            pb.close();
        }
        ShapeKind::Ellipse => {
            pb.push_oval(tiny_skia::Rect::from_ltrb(x0, y0, x1.max(x0 + 0.5), y1.max(y0 + 0.5))?);
        }
        ShapeKind::Line => {
            pb.move_to(a.0, a.1);
            pb.line_to(b.0, b.1);
        }
        ShapeKind::Polygon { sides } => {
            let n = sides.max(3);
            for i in 0..n {
                let t = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::TAU / n as f32;
                let (x, y) = (cx + rx * t.cos(), cy + ry * t.sin());
                if i == 0 { pb.move_to(x, y) } else { pb.line_to(x, y) }
            }
            pb.close();
        }
        ShapeKind::Star { points, inner } => {
            let n = points.max(3) * 2;
            for i in 0..n {
                let t = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::TAU / n as f32;
                let k = if i % 2 == 0 { 1.0 } else { inner.clamp(0.05, 1.0) };
                let (x, y) = (cx + rx * k * t.cos(), cy + ry * k * t.sin());
                if i == 0 { pb.move_to(x, y) } else { pb.line_to(x, y) }
            }
            pb.close();
        }
        ShapeKind::Triangle => {
            pb.move_to(cx, y0);
            pb.line_to(x1, y1);
            pb.line_to(x0, y1);
            pb.close();
        }
        ShapeKind::Heart => {
            let (w, h) = (x1 - x0, y1 - y0);
            let p = |u: f32, v: f32| (x0 + u * w, y0 + v * h);
            let m = |pb: &mut PathBuilder, a: (f32, f32)| pb.move_to(a.0, a.1);
            m(&mut pb, p(0.5, 0.25));
            let c = |pb: &mut PathBuilder, a: (f32, f32), b: (f32, f32), e: (f32, f32)| {
                pb.cubic_to(a.0, a.1, b.0, b.1, e.0, e.1)
            };
            c(&mut pb, p(0.5, 0.0), p(0.0, 0.0), p(0.0, 0.3));
            c(&mut pb, p(0.0, 0.6), p(0.4, 0.75), p(0.5, 1.0));
            c(&mut pb, p(0.6, 0.75), p(1.0, 0.6), p(1.0, 0.3));
            c(&mut pb, p(1.0, 0.0), p(0.5, 0.0), p(0.5, 0.25));
            pb.close();
        }
        ShapeKind::Arrow => {
            let (w, h) = (x1 - x0, y1 - y0);
            let p = |u: f32, v: f32| (x0 + u * w, y0 + v * h);
            let pts = [p(0.0, 0.3), p(0.6, 0.3), p(0.6, 0.0), p(1.0, 0.5), p(0.6, 1.0), p(0.6, 0.7), p(0.0, 0.7)];
            pb.move_to(pts[0].0, pts[0].1);
            for q in &pts[1..] {
                pb.line_to(q.0, q.1);
            }
            pb.close();
        }
        ShapeKind::SpeechBubble => {
            let (w, h) = (x1 - x0, y1 - y0);
            let body = y0 + h * 0.78;
            let r = (w.min(h) * 0.18).min((body - y0) / 2.0);
            let k = 0.552_284_8 * r;
            pb.move_to(x0 + r, y0);
            pb.line_to(x1 - r, y0);
            pb.cubic_to(x1 - r + k, y0, x1, y0 + r - k, x1, y0 + r);
            pb.line_to(x1, body - r);
            pb.cubic_to(x1, body - r + k, x1 - r + k, body, x1 - r, body);
            pb.line_to(x0 + w * 0.42, body);
            pb.line_to(x0 + w * 0.22, y1);
            pb.line_to(x0 + w * 0.26, body);
            pb.line_to(x0 + r, body);
            pb.cubic_to(x0 + r - k, body, x0, body - r + k, x0, body - r);
            pb.line_to(x0, y0 + r);
            pb.cubic_to(x0, y0 + r - k, x0 + r - k, y0, x0 + r, y0);
            pb.close();
        }
        ShapeKind::Checkmark => {
            let (w, h) = (x1 - x0, y1 - y0);
            let p = |u: f32, v: f32| (x0 + u * w, y0 + v * h);
            let pts = [p(0.0, 0.55), p(0.15, 0.4), p(0.38, 0.62), p(0.85, 0.05), p(1.0, 0.2), p(0.38, 0.92)];
            pb.move_to(pts[0].0, pts[0].1);
            for q in &pts[1..] {
                pb.line_to(q.0, q.1);
            }
            pb.close();
        }
    }
    pb.finish()
}

/// Polygon through `points` (lasso selections).
pub fn polygon_path(points: &[(f32, f32)]) -> Option<tiny_skia::Path> {
    let mut pb = PathBuilder::new();
    let (first, rest) = points.split_first()?;
    pb.move_to(first.0, first.1);
    for p in rest {
        pb.line_to(p.0, p.1);
    }
    pb.close();
    pb.finish()
}

fn pixmap_to_image(pm: &Pixmap) -> Image {
    let mut v = Vec::with_capacity(pm.data().len());
    for p in pm.pixels() {
        let c = p.demultiply();
        v.extend_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
    }
    Image::from_raw(pm.width(), pm.height(), v)
}

pub fn render(s: &ShapeLayer, w: u32, h: u32) -> Image {
    let Some(mut pm) = Pixmap::new(w.max(1), h.max(1)) else {
        return Image::new(w, h);
    };
    if let Some(p) = path(s.kind, s.from, s.to) {
        let mut paint = Paint { anti_alias: true, ..Default::default() };
        if let Some(f) = s.fill.filter(|_| s.kind != ShapeKind::Line) {
            paint.set_color(color(f));
            pm.fill_path(&p, &paint, FillRule::Winding, Transform::identity(), None);
        }
        let stroke_color = if s.kind == ShapeKind::Line { s.stroke.or(s.fill) } else { s.stroke };
        if let Some(sc) = stroke_color {
            paint.set_color(color(sc));
            let width = s.stroke_width.max(0.5);
            let stroke = Stroke {
                width,
                line_cap: tiny_skia::LineCap::Round,
                line_join: tiny_skia::LineJoin::Round,
                dash: if s.dashed { StrokeDash::new(vec![width * 3.0, width * 2.0], 0.0) } else { None },
                ..Default::default()
            };
            pm.stroke_path(&p, &paint, &stroke, Transform::identity(), None);
        }
    }
    let img = pixmap_to_image(&pm);
    if (w, h) == img.size() { img } else { Image::new(w, h) }
}

/// Fill a path into a `w × h` coverage mask.
pub fn path_mask(p: &tiny_skia::Path, w: u32, h: u32, anti_alias: bool) -> Mask {
    let Some(mut pm) = Pixmap::new(w.max(1), h.max(1)) else {
        return Mask::new(w, h);
    };
    let mut paint = Paint { anti_alias, ..Default::default() };
    paint.set_color(tiny_skia::Color::WHITE);
    pm.fill_path(p, &paint, FillRule::EvenOdd, Transform::identity(), None);
    Mask::from_raw(w, h, pm.pixels().iter().map(|p| p.alpha()).collect())
}

/// Stroke a polyline into a mask (used by Edit › Stroke on selections).
pub fn stroke_mask(p: &tiny_skia::Path, width: f32, w: u32, h: u32) -> Mask {
    let Some(mut pm) = Pixmap::new(w.max(1), h.max(1)) else {
        return Mask::new(w, h);
    };
    let mut paint = Paint { anti_alias: true, ..Default::default() };
    paint.set_color(tiny_skia::Color::WHITE);
    pm.stroke_path(p, &paint, &Stroke { width, ..Default::default() }, Transform::identity(), None);
    Mask::from_raw(w, h, pm.pixels().iter().map(|p| p.alpha()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filled_rectangle() {
        let s = ShapeLayer {
            kind: ShapeKind::Rectangle,
            from: (2.0, 2.0),
            to: (8.0, 8.0),
            fill: Some(Rgba::rgb(255, 0, 0)),
            stroke: None,
            stroke_width: 1.0,
            dashed: false,
        };
        let img = render(&s, 10, 10);
        assert_eq!(img.get(5, 5), Rgba::rgb(255, 0, 0));
        assert_eq!(img.get(0, 0).a(), 0);
    }

    #[test]
    fn every_kind_has_a_path() {
        for k in [
            ShapeKind::Rectangle,
            ShapeKind::RoundedRect { radius: 4.0 },
            ShapeKind::Ellipse,
            ShapeKind::Line,
            ShapeKind::Polygon { sides: 6 },
            ShapeKind::Star { points: 5, inner: 0.5 },
            ShapeKind::Triangle,
            ShapeKind::Heart,
            ShapeKind::Arrow,
            ShapeKind::SpeechBubble,
            ShapeKind::Checkmark,
        ] {
            assert!(path(k, (1.0, 1.0), (30.0, 20.0)).is_some(), "{}", k.label());
        }
    }
}
