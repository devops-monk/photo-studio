//! Integer rectangles and small geometry helpers.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, w: u32, h: u32) -> Self {
        Self { x, y, w, h }
    }

    pub const fn sized(w: u32, h: u32) -> Self {
        Self { x: 0, y: 0, w, h }
    }

    /// The rectangle spanning two corner points, in either order.
    pub fn from_points(a: (f32, f32), b: (f32, f32)) -> Self {
        let x0 = a.0.min(b.0).floor() as i32;
        let y0 = a.1.min(b.1).floor() as i32;
        let x1 = a.0.max(b.0).ceil() as i32;
        let y1 = a.1.max(b.1).ceil() as i32;
        Self::new(x0, y0, (x1 - x0).max(0) as u32, (y1 - y0).max(0) as u32)
    }

    pub fn right(&self) -> i32 {
        self.x + self.w as i32
    }

    pub fn bottom(&self) -> i32 {
        self.y + self.h as i32
    }

    pub fn is_empty(&self) -> bool {
        self.w == 0 || self.h == 0
    }

    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && x < self.right() && y < self.bottom()
    }

    pub fn intersect(&self, o: &Rect) -> Rect {
        let x0 = self.x.max(o.x);
        let y0 = self.y.max(o.y);
        let x1 = self.right().min(o.right());
        let y1 = self.bottom().min(o.bottom());
        if x1 <= x0 || y1 <= y0 { Rect::default() } else { Rect::new(x0, y0, (x1 - x0) as u32, (y1 - y0) as u32) }
    }

    pub fn union(&self, o: &Rect) -> Rect {
        if self.is_empty() {
            return *o;
        }
        if o.is_empty() {
            return *self;
        }
        let x0 = self.x.min(o.x);
        let y0 = self.y.min(o.y);
        let x1 = self.right().max(o.right());
        let y1 = self.bottom().max(o.bottom());
        Rect::new(x0, y0, (x1 - x0) as u32, (y1 - y0) as u32)
    }

    pub fn inflate(&self, by: i32) -> Rect {
        Rect::new(
            self.x - by,
            self.y - by,
            (self.w as i32 + 2 * by).max(0) as u32,
            (self.h as i32 + 2 * by).max(0) as u32,
        )
    }
}

/// A 2D affine transform `[a c e; b d f]` mapping (x, y) to (a x + c y + e, b x + d y + f).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Affine {
    pub a: f32,
    pub b: f32,
    pub c: f32,
    pub d: f32,
    pub e: f32,
    pub f: f32,
}

impl Affine {
    pub const IDENTITY: Affine = Affine { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: 0.0, f: 0.0 };

    pub fn translate(x: f32, y: f32) -> Self {
        Affine { e: x, f: y, ..Self::IDENTITY }
    }

    pub fn scale(sx: f32, sy: f32) -> Self {
        Affine { a: sx, d: sy, ..Self::IDENTITY }
    }

    pub fn rotate(rad: f32) -> Self {
        let (s, c) = rad.sin_cos();
        Affine { a: c, b: s, c: -s, d: c, e: 0.0, f: 0.0 }
    }

    /// `self` applied after `o`.
    pub fn then(&self, o: &Affine) -> Affine {
        // o * self in matrix terms: apply self first, then o.
        Affine {
            a: o.a * self.a + o.c * self.b,
            b: o.b * self.a + o.d * self.b,
            c: o.a * self.c + o.c * self.d,
            d: o.b * self.c + o.d * self.d,
            e: o.a * self.e + o.c * self.f + o.e,
            f: o.b * self.e + o.d * self.f + o.f,
        }
    }

    pub fn apply(&self, x: f32, y: f32) -> (f32, f32) {
        (self.a * x + self.c * y + self.e, self.b * x + self.d * y + self.f)
    }

    pub fn invert(&self) -> Option<Affine> {
        let det = self.a * self.d - self.b * self.c;
        if det.abs() < 1e-12 {
            return None;
        }
        let id = 1.0 / det;
        Some(Affine {
            a: self.d * id,
            b: -self.b * id,
            c: -self.c * id,
            d: self.a * id,
            e: (self.c * self.f - self.d * self.e) * id,
            f: (self.b * self.e - self.a * self.f) * id,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intersect_and_union() {
        let a = Rect::new(0, 0, 10, 10);
        let b = Rect::new(5, 5, 10, 10);
        assert_eq!(a.intersect(&b), Rect::new(5, 5, 5, 5));
        assert_eq!(a.union(&b), Rect::new(0, 0, 15, 15));
        assert!(a.intersect(&Rect::new(20, 20, 1, 1)).is_empty());
    }

    #[test]
    fn affine_inverse_round_trips() {
        let t = Affine::rotate(0.7).then(&Affine::translate(3.0, -2.0)).then(&Affine::scale(2.0, 0.5));
        let inv = t.invert().unwrap();
        let (x, y) = t.apply(4.0, 9.0);
        let (bx, by) = inv.apply(x, y);
        assert!((bx - 4.0).abs() < 1e-4 && (by - 9.0).abs() < 1e-4);
    }
}
