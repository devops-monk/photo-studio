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
pub struct BrushTip {
    /// Diameter in pixels.
    pub size: f32,
    /// 0 = soft, 1 = hard edge.
    pub hardness: f32,
    /// Dab spacing as a fraction of the diameter.
    pub spacing: f32,
    /// Aliased (pencil) edges.
    pub aliased: bool,
}

impl Default for BrushTip {
    fn default() -> Self {
        BrushTip { size: 30.0, hardness: 0.8, spacing: 0.12, aliased: false }
    }
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
        }
    }

    pub fn tip(&self) -> BrushTip {
        self.tip
    }

    /// Extend the stroke to (x, y); returns the canvas area that changed.
    pub fn stroke_to(&mut self, target: &mut Image, x: f32, y: f32, pressure: f32) -> Rect {
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
        let tip = BrushTip { size: (self.tip.size * pressure.clamp(0.05, 1.0)).max(1.0), ..self.tip };
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
            let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
            tip.falloff(d) * sel.map_or(1.0, |m| m.get(x as u32, y as u32) as f32 / 255.0)
        };

        match &self.kind {
            StrokeKind::Paint { .. } | StrokeKind::Erase | StrokeKind::Clone { .. } | StrokeKind::Heal { .. } => {
                let flow = self.flow;
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
        let tip = BrushTip { size: 10.0, hardness: 1.0, spacing: 0.05, aliased: false };
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
