//! Bézier paths drawn with the Pen tool.

use serde::{Deserialize, Serialize};
use tiny_skia::PathBuilder;

use crate::image::Mask;

/// One anchor with its incoming and outgoing control points (absolute positions).
/// A corner anchor has both handles at the anchor itself.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Anchor {
    pub p: (f32, f32),
    pub h_in: (f32, f32),
    pub h_out: (f32, f32),
}

impl Anchor {
    pub fn corner(p: (f32, f32)) -> Self {
        Anchor { p, h_in: p, h_out: p }
    }

    /// A smooth anchor whose outgoing handle points at `out`, mirrored for the incoming one.
    pub fn smooth(p: (f32, f32), out: (f32, f32)) -> Self {
        Anchor { p, h_in: (2.0 * p.0 - out.0, 2.0 * p.1 - out.1), h_out: out }
    }

    pub fn is_corner(&self) -> bool {
        self.h_in == self.p && self.h_out == self.p
    }

    fn translate(&mut self, dx: f32, dy: f32) {
        for q in [&mut self.p, &mut self.h_in, &mut self.h_out] {
            q.0 += dx;
            q.1 += dy;
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BezierPath {
    pub anchors: Vec<Anchor>,
    pub closed: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NamedPath {
    pub name: String,
    pub path: BezierPath,
}

impl BezierPath {
    pub fn is_empty(&self) -> bool {
        self.anchors.is_empty()
    }

    pub fn translate(&mut self, dx: f32, dy: f32) {
        for a in &mut self.anchors {
            a.translate(dx, dy);
        }
    }

    /// Map every point (resize, crop).
    pub fn map(&mut self, f: impl Fn((f32, f32)) -> (f32, f32)) {
        for a in &mut self.anchors {
            a.p = f(a.p);
            a.h_in = f(a.h_in);
            a.h_out = f(a.h_out);
        }
    }

    pub fn to_skia(&self) -> Option<tiny_skia::Path> {
        let first = self.anchors.first()?;
        let mut pb = PathBuilder::new();
        pb.move_to(first.p.0, first.p.1);
        let seg = |pb: &mut PathBuilder, a: &Anchor, b: &Anchor| {
            if a.h_out == a.p && b.h_in == b.p {
                pb.line_to(b.p.0, b.p.1);
            } else {
                pb.cubic_to(a.h_out.0, a.h_out.1, b.h_in.0, b.h_in.1, b.p.0, b.p.1);
            }
        };
        for w in self.anchors.windows(2) {
            seg(&mut pb, &w[0], &w[1]);
        }
        if self.closed && self.anchors.len() > 1 {
            seg(&mut pb, self.anchors.last().unwrap(), first);
            pb.close();
        }
        if self.anchors.len() == 1 {
            pb.line_to(first.p.0 + 0.01, first.p.1);
        }
        pb.finish()
    }

    /// Points along the curve, for drawing it on screen.
    pub fn flatten(&self, steps: usize) -> Vec<(f32, f32)> {
        let mut out = Vec::new();
        let n = self.anchors.len();
        if n == 0 {
            return out;
        }
        let segs = if self.closed { n } else { n - 1 };
        out.push(self.anchors[0].p);
        for i in 0..segs {
            let a = &self.anchors[i];
            let b = &self.anchors[(i + 1) % n];
            for k in 1..=steps {
                let t = k as f32 / steps as f32;
                let u = 1.0 - t;
                let x =
                    u * u * u * a.p.0 + 3.0 * u * u * t * a.h_out.0 + 3.0 * u * t * t * b.h_in.0 + t * t * t * b.p.0;
                let y =
                    u * u * u * a.p.1 + 3.0 * u * u * t * a.h_out.1 + 3.0 * u * t * t * b.h_in.1 + t * t * t * b.p.1;
                out.push((x, y));
            }
        }
        out
    }

    /// Fill coverage (the path is closed implicitly).
    pub fn to_mask(&self, w: u32, h: u32) -> Mask {
        let mut closed = self.clone();
        closed.closed = true;
        match closed.to_skia() {
            Some(p) if self.anchors.len() >= 3 || !self.anchors.iter().all(Anchor::is_corner) => {
                crate::shape::path_mask(&p, w, h, true)
            }
            _ => Mask::new(w, h),
        }
    }

    pub fn stroke_mask(&self, width: f32, w: u32, h: u32) -> Mask {
        match self.to_skia() {
            Some(p) => crate::shape::stroke_mask(&p, width, w, h),
            None => Mask::new(w, h),
        }
    }

    /// Bounding box of anchors and handles: (x0, y0, x1, y1).
    pub fn bounds(&self) -> Option<(f32, f32, f32, f32)> {
        let mut it = self.anchors.iter().flat_map(|a| [a.p, a.h_in, a.h_out]);
        let first = it.next()?;
        Some(it.fold((first.0, first.1, first.0, first.1), |b, q| {
            (b.0.min(q.0), b.1.min(q.1), b.2.max(q.0), b.3.max(q.1))
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square() -> BezierPath {
        BezierPath {
            anchors: vec![
                Anchor::corner((2.0, 2.0)),
                Anchor::corner((12.0, 2.0)),
                Anchor::corner((12.0, 12.0)),
                Anchor::corner((2.0, 12.0)),
            ],
            closed: true,
        }
    }

    #[test]
    fn square_fills_inside() {
        let m = square().to_mask(16, 16);
        assert_eq!(m.get(7, 7), 255);
        assert_eq!(m.get(14, 14), 0);
    }

    #[test]
    fn smooth_anchor_mirrors_handle() {
        let a = Anchor::smooth((10.0, 10.0), (15.0, 10.0));
        assert_eq!(a.h_in, (5.0, 10.0));
        assert!(!a.is_corner());
    }

    #[test]
    fn curve_bulges_out() {
        // Two anchors with handles pulled up make an arch, closed back along the base.
        let p = BezierPath {
            anchors: vec![
                Anchor { p: (2.0, 20.0), h_in: (2.0, 20.0), h_out: (2.0, 2.0) },
                Anchor { p: (30.0, 20.0), h_in: (30.0, 2.0), h_out: (30.0, 20.0) },
            ],
            closed: true,
        };
        let m = p.to_mask(32, 32);
        assert_eq!(m.get(16, 12), 255, "inside the arch");
        assert_eq!(m.get(16, 25), 0);
        assert!(p.flatten(8).len() > 10);
    }

    #[test]
    fn stroke_follows_outline() {
        let m = square().stroke_mask(2.0, 16, 16);
        assert!(m.get(7, 2) > 200);
        assert_eq!(m.get(7, 7), 0);
    }
}
