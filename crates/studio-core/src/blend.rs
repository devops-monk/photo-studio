//! Blend modes and the per-pixel compositing equation.
//!
//! Compositing follows the W3C Compositing and Blending model: the blend function mixes source
//! and backdrop colour where both are present, and plain source-over handles the rest.

use serde::{Deserialize, Serialize};

use crate::color::{luma, to_u8};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BlendMode {
    #[default]
    Normal,
    Dissolve,
    Darken,
    Multiply,
    ColorBurn,
    LinearBurn,
    DarkerColor,
    Lighten,
    Screen,
    ColorDodge,
    LinearDodge,
    LighterColor,
    Overlay,
    SoftLight,
    HardLight,
    VividLight,
    LinearLight,
    PinLight,
    HardMix,
    Difference,
    Exclusion,
    Subtract,
    Divide,
    Hue,
    Saturation,
    Color,
    Luminosity,
}

impl BlendMode {
    /// Grouped like the familiar blend-mode menu; `None` marks a separator.
    pub const MENU: &'static [Option<BlendMode>] = &[
        Some(Self::Normal),
        Some(Self::Dissolve),
        None,
        Some(Self::Darken),
        Some(Self::Multiply),
        Some(Self::ColorBurn),
        Some(Self::LinearBurn),
        Some(Self::DarkerColor),
        None,
        Some(Self::Lighten),
        Some(Self::Screen),
        Some(Self::ColorDodge),
        Some(Self::LinearDodge),
        Some(Self::LighterColor),
        None,
        Some(Self::Overlay),
        Some(Self::SoftLight),
        Some(Self::HardLight),
        Some(Self::VividLight),
        Some(Self::LinearLight),
        Some(Self::PinLight),
        Some(Self::HardMix),
        None,
        Some(Self::Difference),
        Some(Self::Exclusion),
        Some(Self::Subtract),
        Some(Self::Divide),
        None,
        Some(Self::Hue),
        Some(Self::Saturation),
        Some(Self::Color),
        Some(Self::Luminosity),
    ];

    pub fn all() -> impl Iterator<Item = BlendMode> {
        Self::MENU.iter().flatten().copied()
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::Dissolve => "Dissolve",
            Self::Darken => "Darken",
            Self::Multiply => "Multiply",
            Self::ColorBurn => "Color Burn",
            Self::LinearBurn => "Linear Burn",
            Self::DarkerColor => "Darker Color",
            Self::Lighten => "Lighten",
            Self::Screen => "Screen",
            Self::ColorDodge => "Color Dodge",
            Self::LinearDodge => "Linear Dodge (Add)",
            Self::LighterColor => "Lighter Color",
            Self::Overlay => "Overlay",
            Self::SoftLight => "Soft Light",
            Self::HardLight => "Hard Light",
            Self::VividLight => "Vivid Light",
            Self::LinearLight => "Linear Light",
            Self::PinLight => "Pin Light",
            Self::HardMix => "Hard Mix",
            Self::Difference => "Difference",
            Self::Exclusion => "Exclusion",
            Self::Subtract => "Subtract",
            Self::Divide => "Divide",
            Self::Hue => "Hue",
            Self::Saturation => "Saturation",
            Self::Color => "Color",
            Self::Luminosity => "Luminosity",
        }
    }

    pub fn from_name(s: &str) -> Option<BlendMode> {
        let k: String = s.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_lowercase();
        let norm = |m: &BlendMode| m.label().chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_lowercase();
        Self::all().find(|m| norm(m) == k).or_else(|| Self::all().find(|m| k.len() > 3 && norm(m).starts_with(&k)))
    }

    /// Blend backdrop `b` with source `s` (both straight RGB in 0..1).
    pub fn blend(&self, b: [f32; 3], s: [f32; 3]) -> [f32; 3] {
        let sep = |f: fn(f32, f32) -> f32| [f(b[0], s[0]), f(b[1], s[1]), f(b[2], s[2])];
        match self {
            Self::Normal | Self::Dissolve => s,
            Self::Darken => sep(|b, s| b.min(s)),
            Self::Multiply => sep(|b, s| b * s),
            Self::ColorBurn => sep(color_burn),
            Self::LinearBurn => sep(|b, s| (b + s - 1.0).max(0.0)),
            Self::DarkerColor => {
                if luma(s[0], s[1], s[2]) < luma(b[0], b[1], b[2]) { s } else { b }
            }
            Self::Lighten => sep(|b, s| b.max(s)),
            Self::Screen => sep(screen),
            Self::ColorDodge => sep(color_dodge),
            Self::LinearDodge => sep(|b, s| (b + s).min(1.0)),
            Self::LighterColor => {
                if luma(s[0], s[1], s[2]) > luma(b[0], b[1], b[2]) { s } else { b }
            }
            Self::Overlay => sep(|b, s| hard_light(s, b)),
            Self::SoftLight => sep(soft_light),
            Self::HardLight => sep(hard_light),
            Self::VividLight => sep(|b, s| {
                if s <= 0.5 { color_burn(b, 2.0 * s) } else { color_dodge(b, 2.0 * (s - 0.5)) }
            }),
            Self::LinearLight => sep(|b, s| (b + 2.0 * s - 1.0).clamp(0.0, 1.0)),
            Self::PinLight => sep(|b, s| {
                if s <= 0.5 { b.min(2.0 * s) } else { b.max(2.0 * s - 1.0) }
            }),
            Self::HardMix => sep(|b, s| if b + s >= 1.0 { 1.0 } else { 0.0 }),
            Self::Difference => sep(|b, s| (b - s).abs()),
            Self::Exclusion => sep(|b, s| b + s - 2.0 * b * s),
            Self::Subtract => sep(|b, s| (b - s).max(0.0)),
            Self::Divide => sep(|b, s| if s <= 0.0 { if b <= 0.0 { 0.0 } else { 1.0 } } else { (b / s).min(1.0) }),
            Self::Hue => set_lum(set_sat(s, sat(b)), lum(b)),
            Self::Saturation => set_lum(set_sat(b, sat(s)), lum(b)),
            Self::Color => set_lum(s, lum(b)),
            Self::Luminosity => set_lum(b, lum(s)),
        }
    }
}

fn screen(b: f32, s: f32) -> f32 {
    b + s - b * s
}

fn hard_light(b: f32, s: f32) -> f32 {
    if s <= 0.5 { b * 2.0 * s } else { screen(b, 2.0 * s - 1.0) }
}

fn color_dodge(b: f32, s: f32) -> f32 {
    if b <= 0.0 {
        0.0
    } else if s >= 1.0 {
        1.0
    } else {
        (b / (1.0 - s)).min(1.0)
    }
}

fn color_burn(b: f32, s: f32) -> f32 {
    if b >= 1.0 {
        1.0
    } else if s <= 0.0 {
        0.0
    } else {
        1.0 - ((1.0 - b) / s).min(1.0)
    }
}

fn soft_light(b: f32, s: f32) -> f32 {
    if s <= 0.5 {
        b - (1.0 - 2.0 * s) * b * (1.0 - b)
    } else {
        let d = if b <= 0.25 { ((16.0 * b - 12.0) * b + 4.0) * b } else { b.sqrt() };
        b + (2.0 * s - 1.0) * (d - b)
    }
}

fn lum(c: [f32; 3]) -> f32 {
    0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2]
}

fn clip_color(c: [f32; 3]) -> [f32; 3] {
    let l = lum(c);
    let n = c[0].min(c[1]).min(c[2]);
    let x = c[0].max(c[1]).max(c[2]);
    let mut c = c;
    if n < 0.0 {
        for v in &mut c {
            *v = l + (*v - l) * l / (l - n);
        }
    }
    if x > 1.0 {
        for v in &mut c {
            *v = l + (*v - l) * (1.0 - l) / (x - l);
        }
    }
    c
}

fn set_lum(c: [f32; 3], l: f32) -> [f32; 3] {
    let d = l - lum(c);
    clip_color([c[0] + d, c[1] + d, c[2] + d])
}

fn sat(c: [f32; 3]) -> f32 {
    c[0].max(c[1]).max(c[2]) - c[0].min(c[1]).min(c[2])
}

fn set_sat(c: [f32; 3], s: f32) -> [f32; 3] {
    let max = c[0].max(c[1]).max(c[2]);
    let min = c[0].min(c[1]).min(c[2]);
    if max - min <= 1e-6 {
        return [0.0; 3];
    }
    c.map(|v| (v - min) * s / (max - min))
}

/// A cheap, stable per-pixel hash in 0..1, used by Dissolve.
#[inline]
pub fn pixel_noise(x: u32, y: u32) -> f32 {
    let mut h = x.wrapping_mul(0x9E37_79B1) ^ y.wrapping_mul(0x85EB_CA77);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    (h & 0xFFFF) as f32 / 65535.0
}

/// Composite one source pixel over a backdrop pixel.
///
/// `src` is straight RGB plus an effective alpha in 0..1 that already includes layer opacity
/// and mask coverage.
#[inline]
pub fn composite_pixel(dst: [u8; 4], src: [f32; 4], mode: BlendMode) -> [u8; 4] {
    let a_s = src[3];
    if a_s <= 0.0 {
        return dst;
    }
    let a_b = dst[3] as f32 / 255.0;
    let cb = [dst[0] as f32 / 255.0, dst[1] as f32 / 255.0, dst[2] as f32 / 255.0];
    let cs = [src[0], src[1], src[2]];
    let a_r = a_s + a_b * (1.0 - a_s);
    if a_r <= 0.0 {
        return [0, 0, 0, 0];
    }
    let mixed = if a_b > 0.0 && mode != BlendMode::Normal { mode.blend(cb, cs) } else { cs };
    let mut out = [0u8; 4];
    for i in 0..3 {
        let num = (1.0 - a_b) * a_s * cs[i] + a_s * a_b * mixed[i] + (1.0 - a_s) * a_b * cb[i];
        out[i] = to_u8(num / a_r);
    }
    out[3] = to_u8(a_r);
    out
}

/// Normal-mode source-over in straight alpha, the common case.
#[inline]
pub fn mix_pixel(dst: [u8; 4], src: [f32; 4]) -> [u8; 4] {
    composite_pixel(dst, src, BlendMode::Normal)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_over_opaque() {
        let out = composite_pixel([0, 0, 0, 255], [1.0, 1.0, 1.0, 0.5], BlendMode::Normal);
        assert_eq!(out, [128, 128, 128, 255]);
    }

    #[test]
    fn multiply_and_screen_identities() {
        let b = [0.4, 0.6, 0.8];
        assert_eq!(BlendMode::Multiply.blend(b, [1.0; 3]), b);
        assert_eq!(BlendMode::Screen.blend(b, [0.0; 3]), b);
        assert_eq!(BlendMode::Difference.blend(b, b), [0.0; 3]);
    }

    #[test]
    fn transparent_backdrop_gets_source() {
        let out = composite_pixel([0, 0, 0, 0], [0.2, 0.4, 0.6, 1.0], BlendMode::Multiply);
        assert_eq!(out, [51, 102, 153, 255]);
    }

    #[test]
    fn luminosity_keeps_backdrop_hue() {
        let r = BlendMode::Luminosity.blend([1.0, 0.0, 0.0], [0.5, 0.5, 0.5]);
        assert!(r[0] > r[1] && (lum(r) - 0.5).abs() < 0.01);
    }

    #[test]
    fn every_mode_has_label_and_parses() {
        assert_eq!(BlendMode::all().count(), 27);
        for m in BlendMode::all() {
            assert_eq!(BlendMode::from_name(m.label()), Some(m));
        }
    }
}
