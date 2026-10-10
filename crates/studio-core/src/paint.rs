//! Brush engine and pixel-painting operations.
//!
//! A [`StrokeEngine`] owns one drag of a brush. Paint, erase, clone and heal accumulate
//! coverage in a stroke buffer and recompute pixels from the layer as it was when the stroke
//! began, so overlapping dabs never exceed the stroke opacity. Smudge, dodge/burn, sponge and
//! blur/sharpen act on the live pixels dab by dab.

use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use crate::blend::{BlendMode, composite_pixel};
use crate::color::{Rgba, hsl_to_rgb, luma, rgb_to_hsl, to_u8};
use crate::geom::Rect;
use crate::gradient::{Gradient, GradientShape};
use crate::image::{Image, Mask};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BrushTip {
    /// Diameter in pixels.
    pub size: f32,
    /// 0 = soft, 1 = hard edge.
    pub hardness: f32,
    /// Dab spacing as a fraction of the diameter.
    pub spacing: f32,
    /// Aliased (pencil) edges.
    pub aliased: bool,
    /// Tip rotation in degrees.
    pub angle: f32,
    /// 1 = round, smaller values flatten the tip into an ellipse.
    pub roundness: f32,
    /// Random dab offset, as a fraction of the diameter.
    pub scatter: f32,
    /// Random per-dab size reduction (0..1).
    pub size_jitter: f32,
    /// Random per-dab flow reduction (0..1).
    pub opacity_jitter: f32,
    /// Stroke smoothing (0..1): how lazily the brush follows the pointer.
    pub smoothing: f32,
}

impl Default for BrushTip {
    fn default() -> Self {
        BrushTip {
            size: 30.0,
            hardness: 0.8,
            spacing: 0.12,
            aliased: false,
            angle: 0.0,
            roundness: 1.0,
            scatter: 0.0,
            size_jitter: 0.0,
            opacity_jitter: 0.0,
            smoothing: 0.0,
        }
    }
}

/// Built-in brush presets.
pub fn brush_presets() -> Vec<(&'static str, BrushTip)> {
    let d = BrushTip::default();
    vec![
        ("Hard Round", BrushTip { hardness: 1.0, spacing: 0.1, ..d }),
        ("Soft Round", BrushTip { hardness: 0.0, spacing: 0.1, ..d }),
        ("Airbrush", BrushTip { size: 80.0, hardness: 0.0, spacing: 0.05, ..d }),
        ("Calligraphy", BrushTip { hardness: 0.95, angle: 45.0, roundness: 0.22, spacing: 0.04, ..d }),
        (
            "Chalk",
            BrushTip {
                hardness: 0.6,
                roundness: 0.7,
                scatter: 0.25,
                size_jitter: 0.4,
                opacity_jitter: 0.6,
                spacing: 0.15,
                ..d
            },
        ),
        (
            "Spray",
            BrushTip {
                size: 10.0,
                hardness: 0.7,
                scatter: 2.5,
                size_jitter: 0.7,
                opacity_jitter: 0.5,
                spacing: 0.3,
                ..d
            },
        ),
        ("Ink", BrushTip { size: 8.0, hardness: 1.0, spacing: 0.05, smoothing: 0.6, ..d }),
        ("Marker", BrushTip { size: 24.0, hardness: 0.9, roundness: 0.55, angle: -30.0, spacing: 0.06, ..d }),
    ]
}

impl BrushTip {
    /// Coverage (0..1) at distance `d` from the dab centre.
    #[inline]
    pub fn falloff(&self, d: f32) -> f32 {
        let r = self.size / 2.0;
        if self.aliased {
            return if d <= r.max(0.5) { 1.0 } else { 0.0 };
        }
        if d >= r + 0.5 {
            return 0.0;
        }
        let edge = (r + 0.5 - d).clamp(0.0, 1.0);
        let h = self.hardness.clamp(0.0, 1.0);
        let inner = r * h;
        if d <= inner {
            return edge;
        }
        let t = ((d - inner) / (r - inner).max(1e-3)).clamp(0.0, 1.0);
        // Smooth Gaussian-like shoulder.
        let s = 1.0 - t * t * (3.0 - 2.0 * t);
        s * edge
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToneRange {
    Shadows,
    Midtones,
    Highlights,
}

#[derive(Clone, Debug)]
pub enum StrokeKind {
    Paint {
        color: Rgba,
        blend: BlendMode,
    },
    Erase,
    /// Copy from `source` displaced by `offset` (source = dest + offset).
    Clone {
        source: Image,
        offset: (f32, f32),
    },
    /// Like Clone, but matches the destination's tone and colour.
    Heal {
        source: Image,
        offset: (f32, f32),
        source_low: Image,
        dest_low: Image,
    },
    /// Paint with a repeating tile (Pattern Stamp).
    Pattern {
        pattern: Image,
    },
    /// Erase only colours close to `sample` (Background Eraser).
    BackgroundErase {
        sample: [u8; 3],
        tolerance: u8,
    },
    Smudge {
        strength: f32,
    },
    Dodge {
        range: ToneRange,
        exposure: f32,
    },
    Burn {
        range: ToneRange,
        exposure: f32,
    },
    Sponge {
        saturate: bool,
        flow: f32,
    },
    /// Liquify's Forward Warp: push pixels along the drag.
    Warp {
        strength: f32,
    },
    Blur {
        strength: f32,
    },
    Sharpen {
        strength: f32,
    },
}

pub struct StrokeEngine {
    kind: StrokeKind,
    tip: BrushTip,
    opacity: f32,
    flow: f32,
    selection: Option<Mask>,
    base: Image,
    coverage: Vec<f32>,
    last: Option<(f32, f32)>,
    carry: Option<Vec<[f32; 4]>>,
    width: u32,
    height: u32,
    dabs: u32,
    smooth: Option<(f32, f32)>,
    prev_dab: Option<(f32, f32)>,
}

impl StrokeEngine {
    /// Start a stroke on `target` (layer pixels, or a mask rendered as a grey image).
    pub fn new(
        target: &Image,
        kind: StrokeKind,
        tip: BrushTip,
        opacity: f32,
        flow: f32,
        selection: Option<Mask>,
    ) -> Self {
        let (w, h) = target.size();
        StrokeEngine {
            kind,
            tip,
            opacity: opacity.clamp(0.0, 1.0),
            flow: flow.clamp(0.01, 1.0),
            selection,
            base: target.clone(),
            coverage: vec![0.0; w as usize * h as usize],
            last: None,
            carry: None,
            width: w,
            height: h,
            dabs: 0,
            smooth: None,
            prev_dab: None,
        }
    }

    pub fn tip(&self) -> BrushTip {
        self.tip
    }

    /// Extend the stroke to (x, y); returns the canvas area that changed.
    pub fn stroke_to(&mut self, target: &mut Image, x: f32, y: f32, pressure: f32) -> Rect {
        // Smoothing: the brush trails the pointer.
        let (x, y) = match self.smooth {
            Some((sx, sy)) if self.tip.smoothing > 0.0 => {
                let k = 1.0 - self.tip.smoothing.clamp(0.0, 0.95);
                (sx + (x - sx) * k, sy + (y - sy) * k)
            }
            _ => (x, y),
        };
        self.smooth = Some((x, y));
        let step = (self.tip.size * self.tip.spacing).max(1.0);
        let mut dirty = Rect::default();
        match self.last {
            None => {
                dirty = self.dab(target, x, y, pressure);
                self.last = Some((x, y));
            }
            Some((lx, ly)) => {
                let d = ((x - lx).powi(2) + (y - ly).powi(2)).sqrt();
                if d < step {
                    return dirty;
                }
                let n = (d / step).floor() as i32;
                let mut px = lx;
                let mut py = ly;
                for i in 1..=n {
                    let t = i as f32 * step / d;
                    px = lx + (x - lx) * t;
                    py = ly + (y - ly) * t;
                    dirty = dirty.union(&self.dab(target, px, py, pressure));
                }
                self.last = Some((px, py));
            }
        }
        dirty
    }

    fn dab(&mut self, target: &mut Image, cx: f32, cy: f32, pressure: f32) -> Rect {
        // Deterministic per-dab randomness, so strokes replay identically.
        self.dabs = self.dabs.wrapping_add(1);
        let n = self.dabs;
        let rnd = |k: u32| crate::blend::pixel_noise(n, k.wrapping_mul(7919));
        let jitter = 1.0 - self.tip.size_jitter.clamp(0.0, 1.0) * rnd(1);
        let size = (self.tip.size * pressure.clamp(0.05, 1.0) * jitter).max(1.0);
        let (cx, cy) = if self.tip.scatter > 0.0 {
            let a = rnd(2) * std::f32::consts::TAU;
            let d = rnd(3) * self.tip.scatter * self.tip.size;
            (cx + a.cos() * d, cy + a.sin() * d)
        } else {
            (cx, cy)
        };
        let flow_k = 1.0 - self.tip.opacity_jitter.clamp(0.0, 1.0) * rnd(4);
        let tip = BrushTip { size, ..self.tip };
        let (sa, ca) = tip.angle.to_radians().sin_cos();
        let round = tip.roundness.clamp(0.05, 1.0);
        let r = tip.size / 2.0 + 1.0;
        let rect = Rect::new(
            (cx - r).floor() as i32,
            (cy - r).floor() as i32,
            (2.0 * r).ceil() as u32 + 1,
            (2.0 * r).ceil() as u32 + 1,
        )
        .intersect(&Rect::sized(self.width, self.height));
        if rect.is_empty() {
            return rect;
        }
        let w = self.width as usize;
        let sel = self.selection.as_ref();
        let cov_at = |x: i32, y: i32| {
            let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
            // Into the tip's frame: rotate, then stretch the short axis.
            let u = dx * ca + dy * sa;
            let v = (-dx * sa + dy * ca) / round;
            let d = (u * u + v * v).sqrt();
            tip.falloff(d) * sel.map_or(1.0, |m| m.get(x as u32, y as u32) as f32 / 255.0)
        };

        match &self.kind {
            StrokeKind::Paint { .. }
            | StrokeKind::Erase
            | StrokeKind::Clone { .. }
            | StrokeKind::Heal { .. }
            | StrokeKind::Pattern { .. }
            | StrokeKind::BackgroundErase { .. } => {
                let flow = self.flow * flow_k;
                for y in rect.y..rect.bottom() {
                    for x in rect.x..rect.right() {
                        let c = cov_at(x, y);
                        if c <= 0.0 {
                            continue;
                        }
                        let i = y as usize * w + x as usize;
                        let cv = &mut self.coverage[i];
                        *cv = (*cv + c * flow * (1.0 - *cv)).min(1.0);
                    }
                }
                self.resolve(target, rect);
            }
            StrokeKind::Smudge { strength } => {
                let s = *strength;
                let size = rect.w as usize * rect.h as usize;
                let carry = self.carry.get_or_insert_with(Vec::new);
                let fresh = carry.len() != size;
                if fresh {
                    carry.clear();
                    for y in rect.y..rect.bottom() {
                        for x in rect.x..rect.right() {
                            carry.push(target.get(x as u32, y as u32).to_f32());
                        }
                    }
                    return rect;
                }
                let px = target.pixels_mut();
                let mut k = 0;
                for y in rect.y..rect.bottom() {
                    for x in rect.x..rect.right() {
                        let c = cov_at(x, y) * s;
                        let i = (y as usize * w + x as usize) * 4;
                        let cur = [
                            px[i] as f32 / 255.0,
                            px[i + 1] as f32 / 255.0,
                            px[i + 2] as f32 / 255.0,
                            px[i + 3] as f32 / 255.0,
                        ];
                        let mut out = cur;
                        for ch in 0..4 {
                            out[ch] = cur[ch] + (carry[k][ch] - cur[ch]) * c;
                            px[i + ch] = to_u8(out[ch]);
                            carry[k][ch] = carry[k][ch] * 0.85 + out[ch] * 0.15;
                        }
                        k += 1;
                    }
                }
            }
            StrokeKind::Dodge { .. } | StrokeKind::Burn { .. } | StrokeKind::Sponge { .. } => {
                let kind = self.kind.clone();
                let flow = self.flow * self.opacity;
                let px = target.pixels_mut();
                for y in rect.y..rect.bottom() {
                    for x in rect.x..rect.right() {
                        let c = cov_at(x, y) * flow;
                        if c <= 0.0 {
                            continue;
                        }
                        let i = (y as usize * w + x as usize) * 4;
                        let rgb = [px[i] as f32 / 255.0, px[i + 1] as f32 / 255.0, px[i + 2] as f32 / 255.0];
                        let out = tone_dab(&kind, rgb, c);
                        for ch in 0..3 {
                            px[i + ch] = to_u8(out[ch]);
                        }
                    }
                }
            }
            StrokeKind::Warp { strength } => {
                let Some((px0, py0)) = self.prev_dab.replace((cx, cy)) else {
                    return rect;
                };
                let (dx, dy) = ((cx - px0) * strength, (cy - py0) * strength);
                if dx == 0.0 && dy == 0.0 {
                    return rect;
                }
                // Each pixel samples from behind the motion, weighted by the tip falloff.
                let src = target.clone();
                let px = target.pixels_mut();
                for y in rect.y..rect.bottom() {
                    for x in rect.x..rect.right() {
                        let c = cov_at(x, y);
                        if c <= 0.0 {
                            continue;
                        }
                        let s = src.sample_bilinear(x as f32 + 0.5 - dx * c, y as f32 + 0.5 - dy * c);
                        let i = (y as usize * w + x as usize) * 4;
                        let out = [to_u8(s[0]), to_u8(s[1]), to_u8(s[2]), to_u8(s[3])];
                        px[i..i + 4].copy_from_slice(&out);
                    }
                }
            }
            StrokeKind::Blur { strength } | StrokeKind::Sharpen { strength } => {
                let sharpen = matches!(self.kind, StrokeKind::Sharpen { .. });
                let s = *strength * self.flow;
                let src = target.crop(rect.inflate(1));
                let px = target.pixels_mut();
                for y in rect.y..rect.bottom() {
                    for x in rect.x..rect.right() {
                        let c = cov_at(x, y) * s;
                        if c <= 0.0 {
                            continue;
                        }
                        let (sx, sy) = ((x - rect.x + 1), (y - rect.y + 1));
                        let i = (y as usize * w + x as usize) * 4;
                        for ch in 0..3 {
                            let mut avg = 0.0;
                            for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1), (0, 0)] {
                                avg += src.get_clamped(sx + dx, sy + dy).0[ch] as f32;
                            }
                            avg /= 5.0;
                            let cur = px[i + ch] as f32;
                            let v = if sharpen { cur + (cur - avg) * c * 1.5 } else { cur + (avg - cur) * c };
                            px[i + ch] = v.round().clamp(0.0, 255.0) as u8;
                        }
                    }
                }
            }
        }
        rect
    }

    /// Recompute coverage-based pixels from the stroke's base image.
    fn resolve(&self, target: &mut Image, rect: Rect) {
        let w = self.width as usize;
        let op = self.opacity;
        let base = self.base.pixels();
        let px = target.pixels_mut();
        for y in rect.y..rect.bottom() {
            for x in rect.x..rect.right() {
                let i = y as usize * w + x as usize;
                let a = self.coverage[i] * op;
                if a <= 0.0 {
                    continue;
                }
                let b = [base[i * 4], base[i * 4 + 1], base[i * 4 + 2], base[i * 4 + 3]];
                let out = match &self.kind {
                    StrokeKind::Paint { color, blend } => {
                        let c = color.to_f32();
                        composite_pixel(b, [c[0], c[1], c[2], a * c[3]], *blend)
                    }
                    StrokeKind::Erase => [b[0], b[1], b[2], (b[3] as f32 * (1.0 - a)).round() as u8],
                    StrokeKind::Clone { source, offset } => {
                        let s = source.sample_bilinear(x as f32 + 0.5 + offset.0, y as f32 + 0.5 + offset.1);
                        composite_pixel(b, [s[0], s[1], s[2], s[3] * a], BlendMode::Normal)
                    }
                    StrokeKind::Heal { source, offset, source_low, dest_low } => {
                        let (sx, sy) = (x as f32 + 0.5 + offset.0, y as f32 + 0.5 + offset.1);
                        let s = source.sample_bilinear(sx, sy);
                        let sl = source_low.sample_bilinear(sx, sy);
                        let dl = dest_low.get(x as u32, y as u32).to_f32();
                        let healed = [
                            (s[0] - sl[0] + dl[0]).clamp(0.0, 1.0),
                            (s[1] - sl[1] + dl[1]).clamp(0.0, 1.0),
                            (s[2] - sl[2] + dl[2]).clamp(0.0, 1.0),
                            s[3].max(b[3] as f32 / 255.0) * a,
                        ];
                        composite_pixel(b, healed, BlendMode::Normal)
                    }
                    StrokeKind::Pattern { pattern } => {
                        let (pw, ph) = pattern.size();
                        if pw == 0 || ph == 0 {
                            b
                        } else {
                            let s = pattern.get(x as u32 % pw, y as u32 % ph).to_f32();
                            composite_pixel(b, [s[0], s[1], s[2], s[3] * a], BlendMode::Normal)
                        }
                    }
                    StrokeKind::BackgroundErase { sample, tolerance } => {
                        let d =
                            (0..3).map(|i| (b[i] as i32 - sample[i] as i32).unsigned_abs()).max().unwrap_or(0) as f32;
                        let t = *tolerance as f32;
                        // Full erase inside the tolerance, a soft ramp just beyond it.
                        let m = if d <= t { 1.0 } else { (1.0 - (d - t) / 24.0).max(0.0) };
                        [b[0], b[1], b[2], (b[3] as f32 * (1.0 - a * m)).round() as u8]
                    }
                    _ => b,
                };
                px[i * 4..i * 4 + 4].copy_from_slice(&out);
            }
        }
    }
}

fn tone_dab(kind: &StrokeKind, rgb: [f32; 3], c: f32) -> [f32; 3] {
    let l = luma(rgb[0], rgb[1], rgb[2]);
    let weight = |range: &ToneRange| match range {
        ToneRange::Shadows => (1.0 - l).powi(2),
        ToneRange::Midtones => 1.0 - (2.0 * l - 1.0).powi(2),
        ToneRange::Highlights => l * l,
    };
    match kind {
        StrokeKind::Dodge { range, exposure } => {
            let k = c * exposure * weight(range) * 0.35;
            rgb.map(|v| v + (1.0 - v) * k)
        }
        StrokeKind::Burn { range, exposure } => {
            let k = c * exposure * weight(range) * 0.35;
            rgb.map(|v| v * (1.0 - k))
        }
        StrokeKind::Sponge { saturate, flow } => {
            let (h, s, l) = rgb_to_hsl(rgb[0], rgb[1], rgb[2]);
            let k = c * flow * 0.3;
            let s = if *saturate { s + (1.0 - s) * k } else { s * (1.0 - k) };
            let (r, g, b) = hsl_to_rgb(h, s, l);
            [r, g, b]
        }
        _ => rgb,
    }
}

/// Prepare a healing stroke: low-frequency copies drive the tone matching.
pub fn heal_kind(source: Image, dest: &Image, offset: (f32, f32), size: f32) -> StrokeKind {
    let radius = (size * 0.6).clamp(3.0, 60.0);
    let source_low = crate::filters::gaussian_blur(&source, radius);
    let dest_low = if source == *dest { source_low.clone() } else { crate::filters::gaussian_blur(dest, radius) };
    StrokeKind::Heal { source, offset, source_low, dest_low }
}

/// Pick a nearby offset whose surroundings best match the spot at (x, y) (Spot Healing).
pub fn spot_heal_offset(img: &Image, x: f32, y: f32, size: f32) -> (f32, f32) {
    let ring = |cx: f32, cy: f32| -> [f32; 3] {
        let r = size * 0.75;
        let mut acc = [0.0; 3];
        for i in 0..16 {
            let a = i as f32 / 16.0 * std::f32::consts::TAU;
            let p = img.get_clamped((cx + r * a.cos()) as i32, (cy + r * a.sin()) as i32).to_f32();
            for c in 0..3 {
                acc[c] += p[c] / 16.0;
            }
        }
        acc
    };
    let target = ring(x, y);
    let dist = size * 1.4;
    (0..8)
        .map(|i| {
            let a = i as f32 / 8.0 * std::f32::consts::TAU;
            let o = (dist * a.cos(), dist * a.sin());
            let r = ring(x + o.0, y + o.1);
            let d: f32 = (0..3).map(|c| (r[c] - target[c]).powi(2)).sum();
            (d, o)
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, o)| o)
        .unwrap_or((dist, 0.0))
}

/// Remove red-eye inside `area`: strongly red pixels lose their red cast and darken.
pub fn red_eye(img: &mut Image, area: Rect, darken: f32) {
    let r = area.intersect(&img.bounds());
    let w = img.width() as usize;
    let (cx, cy) = (r.x as f32 + r.w as f32 / 2.0, r.y as f32 + r.h as f32 / 2.0);
    let (rx, ry) = ((r.w as f32 / 2.0).max(1.0), (r.h as f32 / 2.0).max(1.0));
    let px = img.pixels_mut();
    for y in r.y..r.bottom() {
        for x in r.x..r.right() {
            let dx = (x as f32 + 0.5 - cx) / rx;
            let dy = (y as f32 + 0.5 - cy) / ry;
            let d = dx * dx + dy * dy;
            if d > 1.0 {
                continue;
            }
            let i = (y as usize * w + x as usize) * 4;
            let (pr, pg, pb) = (px[i] as f32, px[i + 1] as f32, px[i + 2] as f32);
            let redness = pr - (pg + pb) / 2.0;
            if redness < 40.0 || pr < 60.0 {
                continue;
            }
            let k = ((redness - 40.0) / 60.0).clamp(0.0, 1.0) * (1.0 - d * d);
            let target = (pg + pb) / 2.0 * (1.0 - darken.clamp(0.0, 1.0) * 0.6);
            px[i] = (pr + (target - pr) * k).round() as u8;
            px[i + 1] = (pg + (pg * (1.0 - darken * 0.6) - pg) * k).round() as u8;
            px[i + 2] = (pb + (pb * (1.0 - darken * 0.6) - pb) * k).round() as u8;
        }
    }
}

/// Built-in tiles for the Pattern Stamp and pattern fills.
pub fn builtin_patterns(fg: Rgba, bg: Rgba) -> Vec<(&'static str, Image)> {
    let tile = |n: u32, f: &dyn Fn(u32, u32) -> bool| {
        let mut img = Image::new(n, n);
        for y in 0..n {
            for x in 0..n {
                img.put(x, y, if f(x, y) { fg } else { bg });
            }
        }
        img
    };
    let mut noise = Image::new(64, 64);
    for y in 0..64 {
        for x in 0..64 {
            let t = crate::filters::fbm(x as f32 / 9.0, y as f32 / 9.0, 5);
            noise.put(x, y, Rgba::lerp(fg, bg, t));
        }
    }
    vec![
        ("Checker", tile(16, &|x, y| (x / 8 + y / 8) % 2 == 0)),
        ("Dots", tile(16, &|x, y| ((x as f32 - 7.5).powi(2) + (y as f32 - 7.5).powi(2)) < 12.0)),
        ("Stripes", tile(16, &|x, y| (x + y) % 16 < 6)),
        ("Grid", tile(16, &|x, y| x == 0 || y == 0)),
        ("Bricks", tile(32, &|x, y| y % 16 == 0 || (if (y / 16) % 2 == 0 { x % 32 == 0 } else { x % 32 == 16 }))),
        ("Clouds", noise),
    ]
}

/// Render a gradient filling a `w × h` image, with light dithering against banding.
pub fn render_gradient(w: u32, h: u32, g: &Gradient, shape: GradientShape, a: (f32, f32), b: (f32, f32)) -> Image {
    let lut: Vec<[f32; 4]> = (0..1024).map(|i| g.sample(i as f32 / 1023.0).to_f32()).collect();
    let mut img = Image::new(w, h);
    img.pixels_mut().par_chunks_mut(w as usize * 4).enumerate().for_each(|(y, row)| {
        for (x, p) in row.chunks_exact_mut(4).enumerate() {
            let t = shape.t_at(a, b, (x as f32 + 0.5, y as f32 + 0.5));
            let c = lut[(t * 1023.0).round() as usize];
            let dither = (crate::blend::pixel_noise(x as u32, y as u32) - 0.5) / 255.0;
            for i in 0..4 {
                p[i] = to_u8(c[i] + dither);
            }
        }
    });
    img
}

/// Composite `src` onto `dst` with a blend mode and opacity, limited by `mask`.
pub fn composite_onto(dst: &mut Image, src: &Image, blend: BlendMode, opacity: f32, mask: Option<&Mask>) {
    let w = dst.width() as usize;
    let sp = src.pixels();
    dst.pixels_mut().par_chunks_mut(w * 4).enumerate().for_each(|(y, row)| {
        for (x, p) in row.chunks_exact_mut(4).enumerate() {
            let i = y * w + x;
            let cov = mask.map_or(1.0, |m| m.data()[i] as f32 / 255.0) * opacity;
            if cov <= 0.0 {
                continue;
            }
            let s = &sp[i * 4..i * 4 + 4];
            let src = [s[0] as f32 / 255.0, s[1] as f32 / 255.0, s[2] as f32 / 255.0, s[3] as f32 / 255.0 * cov];
            let out = composite_pixel([p[0], p[1], p[2], p[3]], src, blend);
            p.copy_from_slice(&out);
        }
    });
}

/// Paint-bucket fill: fill `region` with `color`.
pub fn fill_region(dst: &mut Image, region: &Mask, color: Rgba, blend: BlendMode, opacity: f32) {
    let src = Image::filled(dst.width(), dst.height(), color);
    composite_onto(dst, &src, blend, opacity, Some(region));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn falloff_is_monotone_and_bounded() {
        let tip = BrushTip { size: 20.0, hardness: 0.3, ..Default::default() };
        assert_eq!(tip.falloff(0.0), 1.0);
        assert_eq!(tip.falloff(11.0), 0.0);
        let mut prev = 1.0;
        for i in 0..22 {
            let v = tip.falloff(i as f32 * 0.5);
            assert!(v <= prev + 1e-6);
            prev = v;
        }
    }

    #[test]
    fn overlapping_dabs_respect_opacity() {
        let mut img = Image::filled(40, 40, Rgba::WHITE);
        let tip = BrushTip { size: 10.0, hardness: 1.0, spacing: 0.05, ..Default::default() };
        let mut s = StrokeEngine::new(
            &img,
            StrokeKind::Paint { color: Rgba::BLACK, blend: BlendMode::Normal },
            tip,
            0.5,
            1.0,
            None,
        );
        for _ in 0..5 {
            s.stroke_to(&mut img, 20.0, 20.0, 1.0);
            s.stroke_to(&mut img, 22.0, 20.0, 1.0);
            s.stroke_to(&mut img, 20.0, 20.0, 1.0);
        }
        let c = img.get(20, 20);
        assert!((126..=129).contains(&c.r()), "{c:?}");
    }

    #[test]
    fn background_eraser_keeps_other_colours() {
        let mut img = Image::filled(20, 20, Rgba::WHITE);
        for y in 0..20 {
            img.put(10, y, Rgba::BLACK);
        }
        let tip = BrushTip { size: 12.0, hardness: 1.0, ..Default::default() };
        let mut s = StrokeEngine::new(
            &img,
            StrokeKind::BackgroundErase { sample: [255, 255, 255], tolerance: 20 },
            tip,
            1.0,
            1.0,
            None,
        );
        s.stroke_to(&mut img, 10.0, 10.0, 1.0);
        assert_eq!(img.get(8, 10).a(), 0);
        assert_eq!(img.get(10, 10), Rgba::BLACK);
    }

    #[test]
    fn pattern_stamp_tiles() {
        let pat = builtin_patterns(Rgba::BLACK, Rgba::WHITE).remove(0).1;
        let mut img = Image::filled(40, 40, Rgba::rgb(9, 9, 9));
        let tip = BrushTip { size: 40.0, hardness: 1.0, ..Default::default() };
        let mut s = StrokeEngine::new(&img, StrokeKind::Pattern { pattern: pat.clone() }, tip, 1.0, 1.0, None);
        s.stroke_to(&mut img, 20.0, 20.0, 1.0);
        assert_eq!(img.get(20, 20), pat.get(20 % 16, 20 % 16));
    }

    #[test]
    fn red_eye_removes_red() {
        let mut img = Image::filled(20, 20, Rgba::rgb(220, 30, 30));
        red_eye(&mut img, Rect::new(0, 0, 20, 20), 0.5);
        let c = img.get(10, 10);
        assert!(c.r() < 60, "{c:?}");
    }

    #[test]
    fn flat_tip_is_an_ellipse() {
        let mut img = Image::filled(60, 60, Rgba::WHITE);
        let tip = BrushTip { size: 40.0, hardness: 1.0, roundness: 0.25, angle: 0.0, ..Default::default() };
        let mut s = StrokeEngine::new(
            &img,
            StrokeKind::Paint { color: Rgba::BLACK, blend: BlendMode::Normal },
            tip,
            1.0,
            1.0,
            None,
        );
        s.stroke_to(&mut img, 30.0, 30.0, 1.0);
        assert_eq!(img.get(45, 30), Rgba::BLACK, "wide along the angle");
        assert_eq!(img.get(30, 45), Rgba::WHITE, "thin across it");
    }

    #[test]
    fn scatter_and_jitter_are_deterministic() {
        let tip = brush_presets().into_iter().find(|p| p.0 == "Spray").unwrap().1;
        let run = || {
            let mut img = Image::filled(80, 80, Rgba::WHITE);
            let mut s = StrokeEngine::new(
                &img,
                StrokeKind::Paint { color: Rgba::BLACK, blend: BlendMode::Normal },
                tip,
                1.0,
                1.0,
                None,
            );
            for x in 10..70 {
                s.stroke_to(&mut img, x as f32, 40.0, 1.0);
            }
            img
        };
        let (a, b) = (run(), run());
        assert_eq!(a, b);
        // Scattered dabs land off the stroke line.
        let off_line = (0..80).flat_map(|x| [(x, 25u32), (x, 55u32)]).any(|(x, y)| a.get(x, y) != Rgba::WHITE);
        assert!(off_line);
    }

    #[test]
    fn warp_pushes_pixels_along_the_drag() {
        let mut img = Image::filled(60, 20, Rgba::WHITE);
        for y in 0..20 {
            for x in 10..20 {
                img.put(x, y, Rgba::BLACK);
            }
        }
        let tip = BrushTip { size: 30.0, hardness: 0.9, spacing: 0.05, ..Default::default() };
        let mut s = StrokeEngine::new(&img, StrokeKind::Warp { strength: 1.0 }, tip, 1.0, 1.0, None);
        for x in 15..35 {
            s.stroke_to(&mut img, x as f32, 10.0, 1.0);
        }
        // The whole bar under the brush slides right with the drag.
        assert!((26..48).any(|x| img.get(x, 10).r() < 64), "black carried to the right");
        assert!(img.get(11, 10).r() > 128, "the bar's start was pushed away");
    }

    #[test]
    fn eraser_clears_alpha() {
        let mut img = Image::filled(20, 20, Rgba::rgb(9, 9, 9));
        let tip = BrushTip { size: 6.0, hardness: 1.0, ..Default::default() };
        let mut s = StrokeEngine::new(&img, StrokeKind::Erase, tip, 1.0, 1.0, None);
        s.stroke_to(&mut img, 10.0, 10.0, 1.0);
        assert_eq!(img.get(10, 10).a(), 0);
        assert_eq!(img.get(0, 0).a(), 255);
    }

    #[test]
    fn clone_copies_source() {
        let mut img = Image::filled(40, 20, Rgba::WHITE);
        for y in 0..20 {
            for x in 0..10 {
                img.put(x, y, Rgba::rgb(255, 0, 0));
            }
        }
        let tip = BrushTip { size: 6.0, hardness: 1.0, ..Default::default() };
        let mut s = StrokeEngine::new(
            &img,
            StrokeKind::Clone { source: img.clone(), offset: (-25.0, 0.0) },
            tip,
            1.0,
            1.0,
            None,
        );
        s.stroke_to(&mut img, 30.0, 10.0, 1.0);
        assert_eq!(img.get(30, 10), Rgba::rgb(255, 0, 0));
    }

    #[test]
    fn selection_masks_paint() {
        let mut img = Image::filled(20, 20, Rgba::WHITE);
        let sel = Mask::new(20, 20);
        let mut s = StrokeEngine::new(
            &img,
            StrokeKind::Paint { color: Rgba::BLACK, blend: BlendMode::Normal },
            BrushTip::default(),
            1.0,
            1.0,
            Some(sel),
        );
        s.stroke_to(&mut img, 10.0, 10.0, 1.0);
        assert_eq!(img.get(10, 10), Rgba::WHITE);
    }
}
