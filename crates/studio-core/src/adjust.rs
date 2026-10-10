//! Colour and tone adjustments.
//!
//! Every [`Adjustment`] is a pure per-pixel function, so it can be applied destructively to a
//! layer or kept live as an adjustment layer. Operations that need whole-image statistics
//! (auto tone, equalize, shadows/highlights) live as free functions below.

use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use crate::color::{Rgba, hsl_to_rgb, luma, rgb_to_hsl, to_u8};
use crate::gradient::Gradient;
use crate::histogram::Histogram;
use crate::image::{Image, Mask};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct LevelsChannel {
    pub in_black: u8,
    pub in_white: u8,
    /// Midtone gamma; 1.0 is neutral, larger brightens.
    pub gamma: f32,
    pub out_black: u8,
    pub out_white: u8,
}

impl Default for LevelsChannel {
    fn default() -> Self {
        Self { in_black: 0, in_white: 255, gamma: 1.0, out_black: 0, out_white: 255 }
    }
}

impl LevelsChannel {
    fn map(&self, v: f32) -> f32 {
        let ib = self.in_black as f32 / 255.0;
        let iw = (self.in_white.max(self.in_black.saturating_add(1))) as f32 / 255.0;
        let t = ((v - ib) / (iw - ib)).clamp(0.0, 1.0).powf(1.0 / self.gamma.max(0.01));
        let ob = self.out_black as f32 / 255.0;
        let ow = self.out_white as f32 / 255.0;
        ob + t * (ow - ob)
    }

    pub fn is_identity(&self) -> bool {
        *self == Self::default()
    }
}

/// Curve control points in 0..255, sorted by input.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Curve {
    pub points: Vec<(u8, u8)>,
}

impl Default for Curve {
    fn default() -> Self {
        Curve { points: vec![(0, 0), (255, 255)] }
    }
}

impl Curve {
    pub fn is_identity(&self) -> bool {
        self.points.iter().all(|&(x, y)| x == y)
    }

    /// Monotone cubic (Fritsch–Carlson) interpolation into a 256-entry table.
    pub fn lut(&self) -> [u8; 256] {
        let mut pts: Vec<(f32, f32)> = self.points.iter().map(|&(x, y)| (x as f32, y as f32)).collect();
        pts.sort_by(|a, b| a.0.total_cmp(&b.0));
        pts.dedup_by(|a, b| a.0 == b.0);
        let mut lut = [0u8; 256];
        if pts.is_empty() {
            for (i, v) in lut.iter_mut().enumerate() {
                *v = i as u8;
            }
            return lut;
        }
        if pts.len() == 1 {
            return [pts[0].1.clamp(0.0, 255.0) as u8; 256];
        }
        let n = pts.len();
        let d: Vec<f32> = (0..n - 1).map(|i| (pts[i + 1].1 - pts[i].1) / (pts[i + 1].0 - pts[i].0)).collect();
        let mut m = vec![0.0f32; n];
        m[0] = d[0];
        m[n - 1] = d[n - 2];
        for i in 1..n - 1 {
            m[i] = if d[i - 1] * d[i] <= 0.0 { 0.0 } else { (d[i - 1] + d[i]) / 2.0 };
        }
        for i in 0..n - 1 {
            if d[i] == 0.0 {
                m[i] = 0.0;
                m[i + 1] = 0.0;
            } else {
                let a = m[i] / d[i];
                let b = m[i + 1] / d[i];
                let s = a * a + b * b;
                if s > 9.0 {
                    let t = 3.0 / s.sqrt();
                    m[i] = t * a * d[i];
                    m[i + 1] = t * b * d[i];
                }
            }
        }
        for (x, out) in lut.iter_mut().enumerate() {
            let x = x as f32;
            let y = if x <= pts[0].0 {
                pts[0].1
            } else if x >= pts[n - 1].0 {
                pts[n - 1].1
            } else {
                let i = pts.iter().rposition(|p| p.0 <= x).unwrap().min(n - 2);
                let h = pts[i + 1].0 - pts[i].0;
                let t = (x - pts[i].0) / h;
                let (t2, t3) = (t * t, t * t * t);
                (2.0 * t3 - 3.0 * t2 + 1.0) * pts[i].1
                    + (t3 - 2.0 * t2 + t) * h * m[i]
                    + (-2.0 * t3 + 3.0 * t2) * pts[i + 1].1
                    + (t3 - t2) * h * m[i + 1]
            };
            *out = y.round().clamp(0.0, 255.0) as u8;
        }
        lut
    }
}

/// CMYK shifts for one Selective Color range, each in -100..100.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Cmyk {
    pub c: f32,
    pub m: f32,
    pub y: f32,
    pub k: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColorRange {
    Reds,
    Yellows,
    Greens,
    Cyans,
    Blues,
    Magentas,
    Whites,
    Neutrals,
    Blacks,
}

impl ColorRange {
    pub const ALL: [ColorRange; 9] = [
        Self::Reds,
        Self::Yellows,
        Self::Greens,
        Self::Cyans,
        Self::Blues,
        Self::Magentas,
        Self::Whites,
        Self::Neutrals,
        Self::Blacks,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            Self::Reds => "Reds",
            Self::Yellows => "Yellows",
            Self::Greens => "Greens",
            Self::Cyans => "Cyans",
            Self::Blues => "Blues",
            Self::Magentas => "Magentas",
            Self::Whites => "Whites",
            Self::Neutrals => "Neutrals",
            Self::Blacks => "Blacks",
        }
    }

    /// How strongly a pixel belongs to this range (0..1).
    fn weight(&self, c: [f32; 3]) -> f32 {
        let max = c[0].max(c[1]).max(c[2]);
        let min = c[0].min(c[1]).min(c[2]);
        let mid = c[0] + c[1] + c[2] - max - min;
        let primary = |i: usize| if c[i] == max { max - mid } else { 0.0 };
        let secondary = |i: usize, j: usize| {
            if c[i] != min && c[j] != min { mid - min } else { 0.0 }
        };
        match self {
            Self::Reds => primary(0),
            Self::Greens => primary(1),
            Self::Blues => primary(2),
            Self::Yellows => secondary(0, 1),
            Self::Cyans => secondary(1, 2),
            Self::Magentas => secondary(0, 2),
            Self::Whites => ((min - 0.5) * 2.0).max(0.0),
            Self::Blacks => ((0.5 - max) * 2.0).max(0.0),
            Self::Neutrals => 1.0 - ((max - 0.5).abs() + (min - 0.5).abs()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Adjustment {
    BrightnessContrast {
        brightness: f32,
        contrast: f32,
    },
    Levels {
        master: LevelsChannel,
        red: LevelsChannel,
        green: LevelsChannel,
        blue: LevelsChannel,
    },
    Curves {
        master: Curve,
        red: Curve,
        green: Curve,
        blue: Curve,
    },
    Exposure {
        exposure: f32,
        offset: f32,
        gamma: f32,
    },
    Vibrance {
        vibrance: f32,
        saturation: f32,
    },
    HueSaturation {
        hue: f32,
        saturation: f32,
        lightness: f32,
        colorize: bool,
    },
    /// Cyan–Red, Magenta–Green, Yellow–Blue shifts (-100..100) for each tonal range.
    ColorBalance {
        shadows: [f32; 3],
        midtones: [f32; 3],
        highlights: [f32; 3],
        preserve_luminosity: bool,
    },
    /// Percent weights for reds, yellows, greens, cyans, blues, magentas.
    BlackWhite {
        weights: [f32; 6],
        tint: Option<Rgba>,
    },
    PhotoFilter {
        color: Rgba,
        density: f32,
        preserve_luminosity: bool,
    },
    /// Rows for the red, green and blue outputs: [from R, from G, from B, constant] in percent.
    ChannelMixer {
        red: [f32; 4],
        green: [f32; 4],
        blue: [f32; 4],
        monochrome: bool,
    },
    SelectiveColor {
        ranges: Vec<(ColorRange, Cmyk)>,
        absolute: bool,
    },
    GradientMap {
        gradient: Gradient,
        reverse: bool,
    },
    Invert,
    Posterize {
        levels: u8,
    },
    Threshold {
        level: u8,
    },
    Desaturate,
}

impl Adjustment {
    pub fn label(&self) -> &'static str {
        match self {
            Self::BrightnessContrast { .. } => "Brightness/Contrast",
            Self::Levels { .. } => "Levels",
            Self::Curves { .. } => "Curves",
            Self::Exposure { .. } => "Exposure",
            Self::Vibrance { .. } => "Vibrance",
            Self::HueSaturation { .. } => "Hue/Saturation",
            Self::ColorBalance { .. } => "Color Balance",
            Self::BlackWhite { .. } => "Black & White",
            Self::PhotoFilter { .. } => "Photo Filter",
            Self::ChannelMixer { .. } => "Channel Mixer",
            Self::SelectiveColor { .. } => "Selective Color",
            Self::GradientMap { .. } => "Gradient Map",
            Self::Invert => "Invert",
            Self::Posterize { .. } => "Posterize",
            Self::Threshold { .. } => "Threshold",
            Self::Desaturate => "Desaturate",
        }
    }

    /// A neutral instance of every adjustment, in menu order.
    pub fn catalog() -> Vec<Adjustment> {
        vec![
            Self::BrightnessContrast { brightness: 0.0, contrast: 0.0 },
            Self::Levels {
                master: LevelsChannel::default(),
                red: LevelsChannel::default(),
                green: LevelsChannel::default(),
                blue: LevelsChannel::default(),
            },
            Self::Curves {
                master: Curve::default(),
                red: Curve::default(),
                green: Curve::default(),
                blue: Curve::default(),
            },
            Self::Exposure { exposure: 0.0, offset: 0.0, gamma: 1.0 },
            Self::Vibrance { vibrance: 0.0, saturation: 0.0 },
            Self::HueSaturation { hue: 0.0, saturation: 0.0, lightness: 0.0, colorize: false },
            Self::ColorBalance {
                shadows: [0.0; 3],
                midtones: [0.0; 3],
                highlights: [0.0; 3],
                preserve_luminosity: true,
            },
            Self::BlackWhite { weights: [40.0, 60.0, 40.0, 60.0, 20.0, 80.0], tint: None },
            Self::PhotoFilter { color: Rgba::rgb(236, 138, 0), density: 25.0, preserve_luminosity: true },
            Self::ChannelMixer {
                red: [100.0, 0.0, 0.0, 0.0],
                green: [0.0, 100.0, 0.0, 0.0],
                blue: [0.0, 0.0, 100.0, 0.0],
                monochrome: false,
            },
            Self::SelectiveColor {
                ranges: ColorRange::ALL.iter().map(|&r| (r, Cmyk::default())).collect(),
                absolute: false,
            },
            Self::GradientMap { gradient: Gradient::two(Rgba::BLACK, Rgba::WHITE), reverse: false },
            Self::Invert,
            Self::Posterize { levels: 4 },
            Self::Threshold { level: 128 },
            Self::Desaturate,
        ]
    }

    /// Build the per-pixel function once (tables and constants precomputed).
    pub fn prepare(&self) -> Prepared {
        let channel_luts = |m: &dyn Fn(usize, f32) -> f32| -> [[u8; 256]; 3] {
            let mut l = [[0u8; 256]; 3];
            for (c, lut) in l.iter_mut().enumerate() {
                for (i, v) in lut.iter_mut().enumerate() {
                    *v = to_u8(m(c, i as f32 / 255.0));
                }
            }
            l
        };
        match self {
            Self::BrightnessContrast { brightness, contrast } => {
                let b = brightness / 100.0;
                let k = if *contrast >= 0.0 { 1.0 / (1.0 - contrast / 100.5) } else { 1.0 + contrast / 100.0 };
                Prepared::Lut(channel_luts(&|_, v| {
                    let v = if b >= 0.0 { 1.0 - (1.0 - v).powf(1.0 + b) } else { v.powf(1.0 - b) };
                    (v - 0.5) * k + 0.5
                }))
            }
            Self::Levels { master, red, green, blue } => {
                let ch = [red, green, blue];
                Prepared::Lut(channel_luts(&|c, v| master.map(ch[c].map(v))))
            }
            Self::Curves { master, red, green, blue } => {
                let m = master.lut();
                let ch = [red.lut(), green.lut(), blue.lut()];
                let mut l = [[0u8; 256]; 3];
                for c in 0..3 {
                    for i in 0..256 {
                        l[c][i] = m[ch[c][i] as usize];
                    }
                }
                Prepared::Lut(l)
            }
            Self::Exposure { exposure, offset, gamma } => {
                let gain = 2f32.powf(*exposure);
                let g = 1.0 / gamma.max(0.01);
                Prepared::Lut(channel_luts(&|_, v| {
                    let lin = srgb_to_linear(v) * gain + offset;
                    linear_to_srgb(lin.max(0.0).powf(g))
                }))
            }
            Self::Invert => Prepared::Lut(channel_luts(&|_, v| 1.0 - v)),
            Self::Posterize { levels } => {
                let n = (*levels).max(2) as f32 - 1.0;
                Prepared::Lut(channel_luts(&|_, v| (v * n).round() / n))
            }
            other => Prepared::Func(other.clone()),
        }
    }

    fn apply_rgb(&self, c: [f32; 3]) -> [f32; 3] {
        let [r, g, b] = c;
        match self {
            Self::Vibrance { vibrance, saturation } => {
                let (h, s, l) = rgb_to_hsl(r, g, b);
                let v = vibrance / 100.0;
                // Vibrance boosts muted colours most, and spares skin-like oranges a little.
                let skin = if (10.0..50.0).contains(&h) { 0.5 } else { 1.0 };
                let mut s = if v >= 0.0 { s + v * (1.0 - s) * (1.0 - s) * skin } else { s * (1.0 + v) };
                let sat = saturation / 100.0;
                s = if sat >= 0.0 { s + (1.0 - s) * sat * s } else { s * (1.0 + sat) };
                let (r, g, b) = hsl_to_rgb(h, s.clamp(0.0, 1.0), l);
                [r, g, b]
            }
            Self::HueSaturation { hue, saturation, lightness, colorize } => {
                let lt = lightness / 100.0;
                let light = |v: f32| {
                    if lt >= 0.0 { v + (1.0 - v) * lt } else { v * (1.0 + lt) }
                };
                if *colorize {
                    let l = luma(r, g, b);
                    let (r, g, b) = hsl_to_rgb(*hue, (saturation / 100.0).clamp(0.0, 1.0), l);
                    return [light(r), light(g), light(b)];
                }
                let (h, s, l) = rgb_to_hsl(r, g, b);
                let sat = saturation / 100.0;
                let s = if sat >= 0.0 { s + (1.0 - s) * sat * s.sqrt() } else { s * (1.0 + sat) };
                let (r, g, b) = hsl_to_rgb(h + hue, s.clamp(0.0, 1.0), l);
                [light(r), light(g), light(b)]
            }
            Self::ColorBalance { shadows, midtones, highlights, preserve_luminosity } => {
                let (_, _, l0) = rgb_to_hsl(r, g, b);
                let ws = ((0.333 - l0) / 0.25 + 0.5).clamp(0.0, 1.0) * 0.7;
                let wh = ((l0 - 0.667) / 0.25 + 0.5).clamp(0.0, 1.0) * 0.7;
                let wm =
                    ((l0 - 0.333) / 0.25 + 0.5).clamp(0.0, 1.0) * ((0.667 - l0) / 0.25 + 0.5).clamp(0.0, 1.0) * 0.7;
                let mut out = [r, g, b];
                for i in 0..3 {
                    out[i] += (shadows[i] * ws + midtones[i] * wm + highlights[i] * wh) / 100.0;
                    out[i] = out[i].clamp(0.0, 1.0);
                }
                if *preserve_luminosity {
                    let (h, s, _) = rgb_to_hsl(out[0], out[1], out[2]);
                    let (r, g, b) = hsl_to_rgb(h, s, l0);
                    out = [r, g, b];
                }
                out
            }
            Self::BlackWhite { weights, tint } => {
                let max = r.max(g).max(b);
                let min = r.min(g).min(b);
                let mid = r + g + b - max - min;
                let w = |i: usize| weights[i] / 100.0;
                // Primary (max channel) and secondary (max+mid mix) hue weights.
                let (wp, ws) = if r >= g && r >= b {
                    (w(0), if g >= b { w(1) } else { w(5) })
                } else if g >= r && g >= b {
                    (w(2), if r >= b { w(1) } else { w(3) })
                } else {
                    (w(4), if g >= r { w(3) } else { w(5) })
                };
                let gray = (min + (max - mid) * wp + (mid - min) * ws).clamp(0.0, 1.0);
                match tint {
                    Some(t) => {
                        let (h, s, _) = rgb_to_hsl(t.r() as f32 / 255.0, t.g() as f32 / 255.0, t.b() as f32 / 255.0);
                        let (r, g, b) = hsl_to_rgb(h, s * 0.6, gray);
                        [r, g, b]
                    }
                    None => [gray; 3],
                }
            }
            Self::PhotoFilter { color, density, preserve_luminosity } => {
                let d = density / 100.0;
                let f = color.to_f32();
                let l0 = luma(r, g, b);
                let mut out = [
                    r * (1.0 - d) + r * f[0] * d * 2.0,
                    g * (1.0 - d) + g * f[1] * d * 2.0,
                    b * (1.0 - d) + b * f[2] * d * 2.0,
                ];
                // Warming filters are approximated by multiplying with the filter colour, then
                // optionally rescaling so overall brightness stays put.
                if *preserve_luminosity {
                    let l1 = luma(out[0], out[1], out[2]).max(1e-4);
                    for v in &mut out {
                        *v *= l0 / l1;
                    }
                }
                out.map(|v| v.clamp(0.0, 1.0))
            }
            Self::ChannelMixer { red, green, blue, monochrome } => {
                let row = |m: &[f32; 4]| (r * m[0] + g * m[1] + b * m[2]) / 100.0 + m[3] / 100.0;
                if *monochrome {
                    let v = row(red);
                    [v, v, v]
                } else {
                    [row(red), row(green), row(blue)]
                }
            }
            Self::SelectiveColor { ranges, absolute } => {
                let mut cmy = [1.0 - r, 1.0 - g, 1.0 - b];
                let orig = cmy;
                for (range, adj) in ranges {
                    let w = range.weight(c);
                    if w <= 0.0 {
                        continue;
                    }
                    let k = adj.k / 100.0;
                    for (i, a) in [adj.c, adj.m, adj.y].iter().enumerate() {
                        let a = a / 100.0;
                        let scale = if *absolute { 1.0 } else { orig[i] };
                        let base = if (a + k) >= 0.0 { 1.0 - orig[i] } else { orig[i] };
                        cmy[i] += w * ((a * scale).clamp(-orig[i], 1.0) + k * base);
                    }
                }
                [1.0 - cmy[0], 1.0 - cmy[1], 1.0 - cmy[2]].map(|v| v.clamp(0.0, 1.0))
            }
            Self::GradientMap { gradient, reverse } => {
                let t = luma(r, g, b);
                let t = if *reverse { 1.0 - t } else { t };
                let s = gradient.sample(t).to_f32();
                [s[0], s[1], s[2]]
            }
            Self::Threshold { level } => {
                let v = if luma(r, g, b) * 255.0 >= *level as f32 { 1.0 } else { 0.0 };
                [v; 3]
            }
            Self::Desaturate => {
                let max = r.max(g).max(b);
                let min = r.min(g).min(b);
                [(max + min) / 2.0; 3]
            }
            // LUT-based adjustments never reach here.
            _ => c,
        }
    }
}

pub enum Prepared {
    Lut([[u8; 256]; 3]),
    Func(Adjustment),
}

impl Prepared {
    #[inline]
    pub fn apply(&self, p: [u8; 4]) -> [u8; 4] {
        match self {
            Prepared::Lut(l) => [l[0][p[0] as usize], l[1][p[1] as usize], l[2][p[2] as usize], p[3]],
            Prepared::Func(a) => {
                let o = a.apply_rgb([p[0] as f32 / 255.0, p[1] as f32 / 255.0, p[2] as f32 / 255.0]);
                [to_u8(o[0]), to_u8(o[1]), to_u8(o[2]), p[3]]
            }
        }
    }
}

/// Mix the adjusted pixel back in by coverage `cov` (0..1).
#[inline]
fn mix(orig: [u8; 4], adj: [u8; 4], cov: f32) -> [u8; 4] {
    if cov >= 1.0 {
        return adj;
    }
    let mut o = orig;
    for i in 0..3 {
        o[i] = (orig[i] as f32 + (adj[i] as f32 - orig[i] as f32) * cov).round() as u8;
    }
    o
}

/// Apply a prepared per-pixel function to `img`, limited by `mask` and scaled by `amount`.
pub fn apply_prepared(img: &mut Image, prep: &Prepared, mask: Option<&Mask>, amount: f32) {
    let w = img.width() as usize;
    img.pixels_mut().par_chunks_mut(w * 4).enumerate().for_each(|(y, row)| {
        for (x, p) in row.chunks_exact_mut(4).enumerate() {
            let cov = mask.map_or(1.0, |m| m.data()[y * w + x] as f32 / 255.0) * amount;
            if cov <= 0.0 || p[3] == 0 {
                continue;
            }
            let orig = [p[0], p[1], p[2], p[3]];
            p.copy_from_slice(&mix(orig, prep.apply(orig), cov));
        }
    });
}

pub fn apply(img: &mut Image, adj: &Adjustment, mask: Option<&Mask>) {
    apply_prepared(img, &adj.prepare(), mask, 1.0);
}

/// Per-channel black and white points clipped by `clip` (fraction) — Auto Tone.
pub fn auto_tone(img: &Image, clip: f32) -> Adjustment {
    let h = Histogram::of(img, None);
    let ch = |c: usize| {
        let (lo, hi) = h.clip_points(c, clip);
        LevelsChannel { in_black: lo, in_white: hi, ..Default::default() }
    };
    Adjustment::Levels { master: LevelsChannel::default(), red: ch(0), green: ch(1), blue: ch(2) }
}

/// One black and white point for all channels together — Auto Contrast.
pub fn auto_contrast(img: &Image, clip: f32) -> Adjustment {
    let h = Histogram::of(img, None);
    let (lo, hi) = h.clip_points(3, clip);
    Adjustment::Levels {
        master: LevelsChannel { in_black: lo, in_white: hi, ..Default::default() },
        red: LevelsChannel::default(),
        green: LevelsChannel::default(),
        blue: LevelsChannel::default(),
    }
}

/// Neutralise a colour cast by scaling each channel to the same mean (grey world) — Auto Color.
pub fn auto_color(img: &Image) -> Adjustment {
    let h = Histogram::of(img, None);
    let means = [h.mean(0), h.mean(1), h.mean(2)];
    let target = (means[0] + means[1] + means[2]) / 3.0;
    let ch = |c: usize| {
        let gamma = if means[c] > 1.0 && target > 1.0 {
            ((means[c] / 255.0).ln() / (target / 255.0).ln()).clamp(0.5, 2.0)
        } else {
            1.0
        };
        let (lo, hi) = h.clip_points(c, 0.001);
        LevelsChannel { in_black: lo, in_white: hi, gamma: 1.0 / gamma, ..Default::default() }
    };
    Adjustment::Levels { master: LevelsChannel::default(), red: ch(0), green: ch(1), blue: ch(2) }
}

/// Histogram equalisation of brightness.
pub fn equalize(img: &mut Image, mask: Option<&Mask>) {
    let h = Histogram::of(img, mask);
    let total: u64 = h.luma.iter().sum();
    if total == 0 {
        return;
    }
    let mut cdf = [0u64; 256];
    let mut acc = 0;
    for i in 0..256 {
        acc += h.luma[i];
        cdf[i] = acc;
    }
    let lut: [u8; 256] = std::array::from_fn(|i| ((cdf[i] as f64 / total as f64) * 255.0).round() as u8);
    apply_prepared(img, &Prepared::Lut([lut, lut, lut]), mask, 1.0);
}

/// Lift shadows and recover highlights using a blurred luminance map as the tonal guide.
/// `shadows` and `highlights` are 0..100, `radius` in pixels.
pub fn shadows_highlights(img: &mut Image, shadows: f32, highlights: f32, radius: f32, mask: Option<&Mask>) {
    let (w, h) = img.size();
    let lum: Vec<u8> =
        img.pixels().chunks_exact(4).map(|p| (luma(p[0] as f32, p[1] as f32, p[2] as f32)).round() as u8).collect();
    let guide = crate::filters::blur_mask(&Mask::from_raw(w, h, lum), radius);
    let s = shadows / 100.0;
    let hl = highlights / 100.0;
    let gd = guide.data();
    let width = w as usize;
    img.pixels_mut().par_chunks_mut(width * 4).enumerate().for_each(|(y, row)| {
        for (x, p) in row.chunks_exact_mut(4).enumerate() {
            let i = y * width + x;
            let cov = mask.map_or(1.0, |m| m.data()[i] as f32 / 255.0);
            if cov <= 0.0 {
                continue;
            }
            let g = gd[i] as f32 / 255.0;
            let sw = (1.0 - g).powi(2);
            let hw = g.powi(2);
            let orig = [p[0], p[1], p[2], p[3]];
            let mut o = orig;
            for c in 0..3 {
                let v = p[c] as f32 / 255.0;
                let lifted = v + s * sw * (1.0 - v) * 0.9;
                let recovered = lifted - hl * hw * lifted * 0.6;
                o[c] = to_u8(recovered);
            }
            p.copy_from_slice(&mix(orig, o, cov));
        }
    });
}

fn srgb_to_linear(v: f32) -> f32 {
    if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}

fn linear_to_srgb(v: f32) -> f32 {
    if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn px(adj: &Adjustment, c: [u8; 4]) -> [u8; 4] {
        adj.prepare().apply(c)
    }

    #[test]
    fn neutral_catalog_entries_are_identity() {
        let c = [37, 140, 201, 255];
        for a in Adjustment::catalog() {
            if matches!(
                a,
                Adjustment::Invert
                    | Adjustment::Posterize { .. }
                    | Adjustment::Threshold { .. }
                    | Adjustment::Desaturate
                    | Adjustment::BlackWhite { .. }
                    | Adjustment::PhotoFilter { .. }
                    | Adjustment::GradientMap { .. }
            ) {
                continue;
            }
            let out = px(&a, c);
            for i in 0..3 {
                assert!((out[i] as i32 - c[i] as i32).abs() <= 2, "{} changed {c:?} to {out:?}", a.label());
            }
        }
    }

    #[test]
    fn invert_and_threshold() {
        assert_eq!(px(&Adjustment::Invert, [0, 100, 255, 7]), [255, 155, 0, 7]);
        assert_eq!(px(&Adjustment::Threshold { level: 128 }, [200, 200, 200, 255]), [255, 255, 255, 255]);
        assert_eq!(px(&Adjustment::Threshold { level: 128 }, [20, 20, 20, 255]), [0, 0, 0, 255]);
    }

    #[test]
    fn curve_through_points() {
        let c = Curve { points: vec![(0, 0), (128, 200), (255, 255)] };
        let l = c.lut();
        assert_eq!(l[0], 0);
        assert_eq!(l[128], 200);
        assert_eq!(l[255], 255);
        assert!(l.windows(2).all(|w| w[0] <= w[1]), "monotone");
    }

    #[test]
    fn levels_stretch() {
        let a = Adjustment::Levels {
            master: LevelsChannel { in_black: 50, in_white: 200, ..Default::default() },
            red: Default::default(),
            green: Default::default(),
            blue: Default::default(),
        };
        let out = px(&a, [50, 200, 125, 255]);
        assert_eq!((out[0], out[1]), (0, 255));
        assert!((127..=128).contains(&out[2]));
    }

    #[test]
    fn hue_shift_rotates_red_to_green() {
        let a = Adjustment::HueSaturation { hue: 120.0, saturation: 0.0, lightness: 0.0, colorize: false };
        assert_eq!(px(&a, [255, 0, 0, 255]), [0, 255, 0, 255]);
    }

    #[test]
    fn black_white_default_red() {
        let a = &Adjustment::catalog()[7];
        let out = px(a, [255, 0, 0, 255]);
        assert_eq!(out[0], out[1]);
        assert_eq!(out[0], 102); // 40% of red
    }

    #[test]
    fn masked_apply_leaves_unselected_pixels() {
        let mut img = Image::filled(2, 1, Rgba::rgb(10, 10, 10));
        let mask = Mask::from_raw(2, 1, vec![255, 0]);
        apply(&mut img, &Adjustment::Invert, Some(&mask));
        assert_eq!(img.get(0, 0), Rgba::rgb(245, 245, 245));
        assert_eq!(img.get(1, 0), Rgba::rgb(10, 10, 10));
    }
}
