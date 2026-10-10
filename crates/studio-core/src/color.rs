//! Colour type and colour-space conversions.

use serde::{Deserialize, Serialize};

/// Straight (non-premultiplied) 8-bit RGBA.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Rgba(pub [u8; 4]);

impl Rgba {
    pub const BLACK: Rgba = Rgba([0, 0, 0, 255]);
    pub const WHITE: Rgba = Rgba([255, 255, 255, 255]);
    pub const TRANSPARENT: Rgba = Rgba([0, 0, 0, 0]);

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Rgba([r, g, b, 255])
    }

    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Rgba([r, g, b, a])
    }

    pub fn r(&self) -> u8 {
        self.0[0]
    }
    pub fn g(&self) -> u8 {
        self.0[1]
    }
    pub fn b(&self) -> u8 {
        self.0[2]
    }
    pub fn a(&self) -> u8 {
        self.0[3]
    }

    pub fn to_f32(&self) -> [f32; 4] {
        self.0.map(|c| c as f32 / 255.0)
    }

    pub fn from_f32(c: [f32; 4]) -> Self {
        Rgba(c.map(to_u8))
    }

    pub fn hex(&self) -> String {
        format!("{:02X}{:02X}{:02X}", self.0[0], self.0[1], self.0[2])
    }

    pub fn from_hex(s: &str) -> Option<Self> {
        let s = s.trim().trim_start_matches('#');
        let v = |i: usize| u8::from_str_radix(s.get(i..i + 2)?, 16).ok();
        match s.len() {
            6 => Some(Rgba::rgb(v(0)?, v(2)?, v(4)?)),
            8 => Some(Rgba::rgba(v(0)?, v(2)?, v(4)?, v(6)?)),
            3 => {
                let d = |i: usize| u8::from_str_radix(s.get(i..i + 1)?, 16).ok().map(|n| n * 17);
                Some(Rgba::rgb(d(0)?, d(1)?, d(2)?))
            }
            _ => None,
        }
    }

    pub fn lerp(a: Rgba, b: Rgba, t: f32) -> Rgba {
        let t = t.clamp(0.0, 1.0);
        let mut o = [0u8; 4];
        for i in 0..4 {
            o[i] = (a.0[i] as f32 + (b.0[i] as f32 - a.0[i] as f32) * t).round() as u8;
        }
        Rgba(o)
    }
}

#[inline]
pub fn to_u8(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

/// Rec. 601 luma, the weighting most editors use for "luminosity".
#[inline]
pub fn luma(r: f32, g: f32, b: f32) -> f32 {
    0.299 * r + 0.587 * g + 0.114 * b
}

/// RGB (0..1) to HSL (h in 0..360, s and l in 0..1).
pub fn rgb_to_hsl(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    if (max - min).abs() < 1e-6 {
        return (0.0, 0.0, l);
    }
    let d = max - min;
    let s = if l > 0.5 { d / (2.0 - max - min) } else { d / (max + min) };
    let h = if max == r {
        (g - b) / d + if g < b { 6.0 } else { 0.0 }
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    (h * 60.0, s, l)
}

pub fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (f32, f32, f32) {
    if s <= 0.0 {
        return (l, l, l);
    }
    let h = h.rem_euclid(360.0) / 360.0;
    let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let p = 2.0 * l - q;
    let f = |mut t: f32| {
        if t < 0.0 {
            t += 1.0;
        }
        if t > 1.0 {
            t -= 1.0;
        }
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    };
    (f(h + 1.0 / 3.0), f(h), f(h - 1.0 / 3.0))
}

/// RGB (0..1) to HSV (h in 0..360, s and v in 0..1).
pub fn rgb_to_hsv(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let s = if max <= 0.0 { 0.0 } else { d / max };
    if d < 1e-6 {
        return (0.0, s, max);
    }
    let h = if max == r {
        ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    (h * 60.0, s, max)
}

pub fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (f32, f32, f32) {
    let h = h.rem_euclid(360.0) / 60.0;
    let c = v * s;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    (r + m, g + m, b + m)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trip() {
        let c = Rgba::rgb(18, 52, 86);
        assert_eq!(Rgba::from_hex(&c.hex()), Some(c));
        assert_eq!(Rgba::from_hex("#fff"), Some(Rgba::WHITE));
        assert_eq!(Rgba::from_hex("zz"), None);
    }

    #[test]
    fn hsl_and_hsv_round_trip() {
        for &(r, g, b) in &[(0.2, 0.4, 0.9), (1.0, 0.0, 0.0), (0.5, 0.5, 0.5), (0.1, 0.8, 0.3)] {
            let (h, s, l) = rgb_to_hsl(r, g, b);
            let (r2, g2, b2) = hsl_to_rgb(h, s, l);
            assert!((r - r2).abs() < 1e-4 && (g - g2).abs() < 1e-4 && (b - b2).abs() < 1e-4);
            let (h, s, v) = rgb_to_hsv(r, g, b);
            let (r3, g3, b3) = hsv_to_rgb(h, s, v);
            assert!((r - r3).abs() < 1e-4 && (g - g3).abs() < 1e-4 && (b - b3).abs() < 1e-4);
        }
    }
}
