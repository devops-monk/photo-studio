//! Spatial filters: blurs, sharpening, noise, distortions, pixelation, stylize and render.
//!
//! Filters work in premultiplied float internally so transparent edges never bleed black.
//! [`Filter::apply`] limits work to the selection's bounds (plus the filter's reach) and
//! blends the result back by selection coverage.

use std::f32::consts::{PI, TAU};

use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use crate::blend::pixel_noise;
use crate::color::{Rgba, luma, to_u8};
use crate::geom::Rect;
use crate::image::{Image, Mask};

/// Premultiplied float RGBA buffer.
#[derive(Clone)]
struct Buf {
    w: usize,
    h: usize,
    px: Vec<[f32; 4]>,
}

impl Buf {
    fn from_image(img: &Image) -> Self {
        let px = img
            .pixels()
            .chunks_exact(4)
            .map(|p| {
                let a = p[3] as f32 / 255.0;
                [p[0] as f32 / 255.0 * a, p[1] as f32 / 255.0 * a, p[2] as f32 / 255.0 * a, a]
            })
            .collect();
        Buf { w: img.width() as usize, h: img.height() as usize, px }
    }

    fn to_image(&self) -> Image {
        let mut out = Vec::with_capacity(self.px.len() * 4);
        for p in &self.px {
            let a = p[3].clamp(0.0, 1.0);
            if a <= 1e-6 {
                out.extend_from_slice(&[0, 0, 0, 0]);
            } else {
                out.extend_from_slice(&[to_u8(p[0] / a), to_u8(p[1] / a), to_u8(p[2] / a), to_u8(a)]);
            }
        }
        Image::from_raw(self.w as u32, self.h as u32, out)
    }

    #[inline]
    fn at(&self, x: i32, y: i32) -> [f32; 4] {
        let x = x.clamp(0, self.w as i32 - 1) as usize;
        let y = y.clamp(0, self.h as i32 - 1) as usize;
        self.px[y * self.w + x]
    }

    /// Bilinear sample with clamped edges (pixel centres at integer + 0.5).
    fn sample(&self, fx: f32, fy: f32) -> [f32; 4] {
        let x = fx - 0.5;
        let y = fy - 0.5;
        let x0 = x.floor();
        let y0 = y.floor();
        let (tx, ty) = (x - x0, y - y0);
        let (x0, y0) = (x0 as i32, y0 as i32);
        let a = self.at(x0, y0);
        let b = self.at(x0 + 1, y0);
        let c = self.at(x0, y0 + 1);
        let d = self.at(x0 + 1, y0 + 1);
        let mut o = [0.0; 4];
        for i in 0..4 {
            o[i] = (a[i] * (1.0 - tx) + b[i] * tx) * (1.0 - ty) + (c[i] * (1.0 - tx) + d[i] * tx) * ty;
        }
        o
    }

    fn transpose(&self) -> Buf {
        let mut px = vec![[0.0; 4]; self.px.len()];
        px.par_chunks_mut(self.h).enumerate().for_each(|(x, col)| {
            for (y, v) in col.iter_mut().enumerate() {
                *v = self.px[y * self.w + x];
            }
        });
        Buf { w: self.h, h: self.w, px }
    }

    /// Horizontal running-sum box blur of radius `r` with clamped edges.
    fn box_h(&mut self, r: usize) {
        if r == 0 || self.w == 0 {
            return;
        }
        let w = self.w;
        let norm = 1.0 / (2 * r + 1) as f32;
        self.px.par_chunks_mut(w).for_each(|row| {
            let src = row.to_vec();
            let get = |i: isize| src[i.clamp(0, w as isize - 1) as usize];
            let mut acc = [0.0f32; 4];
            for i in -(r as isize)..=(r as isize) {
                let p = get(i);
                for c in 0..4 {
                    acc[c] += p[c];
                }
            }
            for x in 0..w {
                for c in 0..4 {
                    row[x][c] = acc[c] * norm;
                }
                let add = get(x as isize + r as isize + 1);
                let sub = get(x as isize - r as isize);
                for c in 0..4 {
                    acc[c] += add[c] - sub[c];
                }
            }
        });
    }

    fn box_blur(&mut self, rx: usize, ry: usize) {
        self.box_h(rx);
        let mut t = self.transpose();
        t.box_h(ry);
        *self = t.transpose();
    }

    /// Gaussian approximated by three box passes.
    fn gaussian(&mut self, sigma: f32) {
        if sigma < 0.3 {
            return;
        }
        for r in box_radii(sigma) {
            self.box_h(r);
        }
        let mut t = self.transpose();
        for r in box_radii(sigma) {
            t.box_h(r);
        }
        *self = t.transpose();
    }
}

/// Radii of three box blurs whose convolution approximates a Gaussian of `sigma`.
fn box_radii(sigma: f32) -> [usize; 3] {
    let n = 3.0;
    let w_ideal = (12.0 * sigma * sigma / n + 1.0).sqrt();
    let mut wl = w_ideal.floor() as i32;
    if wl % 2 == 0 {
        wl -= 1;
    }
    let wu = wl + 2;
    let m_ideal =
        (12.0 * sigma * sigma - n * (wl * wl) as f32 - 4.0 * n * wl as f32 - 3.0 * n) / (-4.0 * wl as f32 - 4.0);
    let m = m_ideal.round() as i32;
    std::array::from_fn(|i| (((if (i as i32) < m { wl } else { wu }) - 1) / 2).max(0) as usize)
}

/// Gaussian-blur a mask (used for feathering and shadows). `radius` ≈ 2σ.
pub fn blur_mask(m: &Mask, radius: f32) -> Mask {
    if radius < 0.5 {
        return m.clone();
    }
    let (w, h) = (m.width(), m.height());
    let mut b =
        Buf { w: w as usize, h: h as usize, px: m.data().iter().map(|&v| [v as f32 / 255.0, 0.0, 0.0, 0.0]).collect() };
    b.gaussian(radius / 2.0);
    Mask::from_raw(w, h, b.px.iter().map(|p| to_u8(p[0])).collect())
}

/// Gaussian-blur an image. `radius` is the blur radius in pixels (≈ 2σ, as editors label it).
pub fn gaussian_blur(img: &Image, radius: f32) -> Image {
    let mut b = Buf::from_image(img);
    b.gaussian(radius / 2.0);
    b.to_image()
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RadialMode {
    #[default]
    Spin,
    Zoom,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum NoiseDist {
    #[default]
    Uniform,
    Gaussian,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PolarMode {
    #[default]
    RectToPolar,
    PolarToRect,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Filter {
    // Blur
    GaussianBlur {
        radius: f32,
    },
    BoxBlur {
        radius: f32,
    },
    MotionBlur {
        angle: f32,
        distance: f32,
    },
    RadialBlur {
        amount: f32,
        mode: RadialMode,
        cx: f32,
        cy: f32,
    },
    SurfaceBlur {
        radius: f32,
        threshold: f32,
    },
    TiltShift {
        focus_y: f32,
        focus_height: f32,
        transition: f32,
        blur: f32,
    },
    /// Blur Gallery's Iris Blur: sharp inside an ellipse, blurring outward.
    IrisBlur {
        cx: f32,
        cy: f32,
        rx: f32,
        ry: f32,
        transition: f32,
        blur: f32,
    },
    // Sharpen
    Sharpen,
    SharpenMore,
    UnsharpMask {
        amount: f32,
        radius: f32,
        threshold: u8,
    },
    // Noise
    AddNoise {
        amount: f32,
        dist: NoiseDist,
        monochrome: bool,
        seed: u32,
    },
    Median {
        radius: f32,
    },
    ReduceNoise {
        strength: f32,
        preserve_details: f32,
    },
    DustScratches {
        radius: f32,
        threshold: u8,
    },
    // Distort
    Twirl {
        angle: f32,
    },
    Wave {
        amplitude: f32,
        wavelength: f32,
        horizontal: bool,
        vertical: bool,
    },
    Ripple {
        amount: f32,
        size: f32,
    },
    Pinch {
        amount: f32,
    },
    Spherize {
        amount: f32,
    },
    PolarCoordinates {
        mode: PolarMode,
    },
    // Pixelate
    Mosaic {
        cell: f32,
    },
    Crystallize {
        cell: f32,
        seed: u32,
    },
    ColorHalftone {
        radius: f32,
    },
    // Stylize
    Emboss {
        angle: f32,
        height: f32,
        amount: f32,
    },
    FindEdges,
    GlowingEdges {
        width: f32,
        brightness: f32,
    },
    OilPaint {
        radius: f32,
        levels: u8,
    },
    Solarize,
    // Render
    Clouds {
        seed: u32,
        scale: f32,
    },
    Vignette {
        amount: f32,
        midpoint: f32,
        feather: f32,
    },
    LensFlare {
        cx: f32,
        cy: f32,
        brightness: f32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterGroup {
    Blur,
    Sharpen,
    Noise,
    Distort,
    Pixelate,
    Stylize,
    Render,
}

impl FilterGroup {
    pub const ALL: [FilterGroup; 7] =
        [Self::Blur, Self::Sharpen, Self::Noise, Self::Distort, Self::Pixelate, Self::Stylize, Self::Render];

    pub fn label(&self) -> &'static str {
        match self {
            Self::Blur => "Blur",
            Self::Sharpen => "Sharpen",
            Self::Noise => "Noise",
            Self::Distort => "Distort",
            Self::Pixelate => "Pixelate",
            Self::Stylize => "Stylize",
            Self::Render => "Render",
        }
    }
}

/// Colours some filters draw with (Clouds).
#[derive(Clone, Copy, Debug)]
pub struct FilterCtx {
    pub fg: Rgba,
    pub bg: Rgba,
}

impl Default for FilterCtx {
    fn default() -> Self {
        Self { fg: Rgba::BLACK, bg: Rgba::WHITE }
    }
}

impl Filter {
    pub fn catalog() -> Vec<Filter> {
        vec![
            Self::GaussianBlur { radius: 4.0 },
            Self::BoxBlur { radius: 4.0 },
            Self::MotionBlur { angle: 0.0, distance: 20.0 },
            Self::RadialBlur { amount: 10.0, mode: RadialMode::Spin, cx: 0.5, cy: 0.5 },
            Self::SurfaceBlur { radius: 5.0, threshold: 15.0 },
            Self::TiltShift { focus_y: 0.5, focus_height: 0.2, transition: 0.2, blur: 12.0 },
            Self::IrisBlur { cx: 0.5, cy: 0.5, rx: 0.25, ry: 0.25, transition: 0.2, blur: 15.0 },
            Self::Sharpen,
            Self::SharpenMore,
            Self::UnsharpMask { amount: 80.0, radius: 1.5, threshold: 0 },
            Self::AddNoise { amount: 10.0, dist: NoiseDist::Gaussian, monochrome: true, seed: 7 },
            Self::Median { radius: 2.0 },
            Self::ReduceNoise { strength: 6.0, preserve_details: 40.0 },
            Self::DustScratches { radius: 2.0, threshold: 12 },
            Self::Twirl { angle: 90.0 },
            Self::Wave { amplitude: 8.0, wavelength: 60.0, horizontal: true, vertical: false },
            Self::Ripple { amount: 100.0, size: 12.0 },
            Self::Pinch { amount: 50.0 },
            Self::Spherize { amount: 60.0 },
            Self::PolarCoordinates { mode: PolarMode::RectToPolar },
            Self::Mosaic { cell: 12.0 },
            Self::Crystallize { cell: 16.0, seed: 3 },
            Self::ColorHalftone { radius: 6.0 },
            Self::Emboss { angle: 135.0, height: 3.0, amount: 100.0 },
            Self::FindEdges,
            Self::GlowingEdges { width: 2.0, brightness: 6.0 },
            Self::OilPaint { radius: 4.0, levels: 20 },
            Self::Solarize,
            Self::Clouds { seed: 11, scale: 120.0 },
            Self::Vignette { amount: -60.0, midpoint: 50.0, feather: 50.0 },
            Self::LensFlare { cx: 0.3, cy: 0.3, brightness: 100.0 },
        ]
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::GaussianBlur { .. } => "Gaussian Blur",
            Self::BoxBlur { .. } => "Box Blur",
            Self::MotionBlur { .. } => "Motion Blur",
            Self::RadialBlur { .. } => "Radial Blur",
            Self::SurfaceBlur { .. } => "Surface Blur",
            Self::TiltShift { .. } => "Tilt-Shift",
            Self::IrisBlur { .. } => "Iris Blur",
            Self::Sharpen => "Sharpen",
            Self::SharpenMore => "Sharpen More",
            Self::UnsharpMask { .. } => "Unsharp Mask",
            Self::AddNoise { .. } => "Add Noise",
            Self::Median { .. } => "Median",
            Self::ReduceNoise { .. } => "Reduce Noise",
            Self::DustScratches { .. } => "Dust & Scratches",
            Self::Twirl { .. } => "Twirl",
            Self::Wave { .. } => "Wave",
            Self::Ripple { .. } => "Ripple",
            Self::Pinch { .. } => "Pinch",
            Self::Spherize { .. } => "Spherize",
            Self::PolarCoordinates { .. } => "Polar Coordinates",
            Self::Mosaic { .. } => "Mosaic",
            Self::Crystallize { .. } => "Crystallize",
            Self::ColorHalftone { .. } => "Color Halftone",
            Self::Emboss { .. } => "Emboss",
            Self::FindEdges => "Find Edges",
            Self::GlowingEdges { .. } => "Glowing Edges",
            Self::OilPaint { .. } => "Oil Paint",
            Self::Solarize => "Solarize",
            Self::Clouds { .. } => "Clouds",
            Self::Vignette { .. } => "Vignette",
            Self::LensFlare { .. } => "Lens Flare",
        }
    }

    pub fn group(&self) -> FilterGroup {
        match self {
            Self::GaussianBlur { .. }
            | Self::BoxBlur { .. }
            | Self::MotionBlur { .. }
            | Self::RadialBlur { .. }
            | Self::SurfaceBlur { .. }
            | Self::TiltShift { .. }
            | Self::IrisBlur { .. } => FilterGroup::Blur,
            Self::Sharpen | Self::SharpenMore | Self::UnsharpMask { .. } => FilterGroup::Sharpen,
            Self::AddNoise { .. } | Self::Median { .. } | Self::ReduceNoise { .. } | Self::DustScratches { .. } => {
                FilterGroup::Noise
            }
            Self::Twirl { .. }
            | Self::Wave { .. }
            | Self::Ripple { .. }
            | Self::Pinch { .. }
            | Self::Spherize { .. }
            | Self::PolarCoordinates { .. } => FilterGroup::Distort,
            Self::Mosaic { .. } | Self::Crystallize { .. } | Self::ColorHalftone { .. } => FilterGroup::Pixelate,
            Self::Emboss { .. }
            | Self::FindEdges
            | Self::GlowingEdges { .. }
            | Self::OilPaint { .. }
            | Self::Solarize => FilterGroup::Stylize,
            Self::Clouds { .. } | Self::Vignette { .. } | Self::LensFlare { .. } => FilterGroup::Render,
        }
    }

    /// Whether the filter has settings worth a dialog.
    pub fn has_params(&self) -> bool {
        !matches!(self, Self::Sharpen | Self::SharpenMore | Self::FindEdges | Self::Solarize)
    }

    /// Whether the result depends on the whole canvas geometry (distortions, render), so it
    /// must run on the full layer rather than a crop around the selection.
    fn is_global(&self) -> bool {
        matches!(self.group(), FilterGroup::Distort | FilterGroup::Render)
            || matches!(self, Self::RadialBlur { .. } | Self::TiltShift { .. } | Self::IrisBlur { .. })
    }

    /// How far (in pixels) a filter reads around each output pixel.
    fn reach(&self) -> i32 {
        let r = match self {
            Self::GaussianBlur { radius } | Self::BoxBlur { radius } => radius * 1.6,
            Self::MotionBlur { distance, .. } => *distance,
            Self::SurfaceBlur { radius, .. } | Self::Median { radius } | Self::DustScratches { radius, .. } => *radius,
            Self::UnsharpMask { radius, .. } => radius * 1.6,
            Self::ReduceNoise { strength, .. } => strength + 2.0,
            Self::Mosaic { cell } | Self::Crystallize { cell, .. } => cell * 2.0,
            Self::ColorHalftone { radius } => radius * 2.0,
            Self::OilPaint { radius, .. } => *radius,
            Self::GlowingEdges { width, .. } => width * 2.0 + 2.0,
            _ => 2.0,
        };
        r.ceil() as i32 + 2
    }

    /// Run the filter on `img`, limited to `mask` coverage.
    pub fn apply(&self, img: &mut Image, mask: Option<&Mask>, ctx: &FilterCtx) {
        let bounds = img.bounds();
        let region = match mask.and_then(|m| m.bounds()) {
            _ if self.is_global() => bounds,
            Some(b) => b.inflate(self.reach()).intersect(&bounds),
            None if mask.is_some() => return,
            None => bounds,
        };
        if region.is_empty() {
            return;
        }
        let sub = if region == bounds { img.clone() } else { img.crop(region) };
        let out = self.run(&sub, ctx);
        let w = img.width() as usize;
        let ow = region.w as usize;
        let px = img.pixels_mut();
        px.par_chunks_mut(w * 4).enumerate().for_each(|(y, row)| {
            let y = y as i32;
            if y < region.y || y >= region.bottom() {
                return;
            }
            let oy = (y - region.y) as usize;
            for x in region.x..region.right() {
                let i = y as usize * w + x as usize;
                let cov = mask.map_or(255, |m| m.data()[i]) as u32;
                if cov == 0 {
                    continue;
                }
                let o = ((oy * ow) + (x - region.x) as usize) * 4;
                let src = &out.pixels()[o..o + 4];
                let p = &mut row[x as usize * 4..x as usize * 4 + 4];
                for c in 0..4 {
                    p[c] = ((p[c] as u32 * (255 - cov) + src[c] as u32 * cov + 127) / 255) as u8;
                }
            }
        });
    }

    /// The filter applied to the whole image, no selection.
    pub fn run(&self, img: &Image, ctx: &FilterCtx) -> Image {
        let (w, h) = (img.width() as f32, img.height() as f32);
        match self {
            Self::GaussianBlur { radius } => gaussian_blur(img, *radius),
            Self::BoxBlur { radius } => {
                let mut b = Buf::from_image(img);
                let r = radius.round().max(0.0) as usize;
                b.box_blur(r, r);
                b.to_image()
            }
            Self::MotionBlur { angle, distance } => {
                let b = Buf::from_image(img);
                let (s, c) = angle.to_radians().sin_cos();
                let n = (distance.max(1.0) as usize).max(2);
                map_buf(&b, |x, y| {
                    let mut acc = [0.0; 4];
                    for i in 0..n {
                        let t = i as f32 / (n - 1) as f32 - 0.5;
                        let p = b.sample(x + 0.5 + c * t * distance, y + 0.5 - s * t * distance);
                        for k in 0..4 {
                            acc[k] += p[k];
                        }
                    }
                    acc.map(|v| v / n as f32)
                })
            }
            Self::RadialBlur { amount, mode, cx, cy } => {
                let b = Buf::from_image(img);
                let (ox, oy) = (cx * w, cy * h);
                let n = 24;
                map_buf(&b, |x, y| {
                    let (dx, dy) = (x + 0.5 - ox, y + 0.5 - oy);
                    let mut acc = [0.0; 4];
                    for i in 0..n {
                        let t = i as f32 / (n - 1) as f32 - 0.5;
                        let (sx, sy) = match mode {
                            RadialMode::Spin => {
                                let a = t * amount.to_radians() * 2.0;
                                let (s, c) = a.sin_cos();
                                (ox + dx * c - dy * s, oy + dx * s + dy * c)
                            }
                            RadialMode::Zoom => {
                                let k = 1.0 + t * amount / 100.0;
                                (ox + dx * k, oy + dy * k)
                            }
                        };
                        let p = b.sample(sx, sy);
                        for k in 0..4 {
                            acc[k] += p[k];
                        }
                    }
                    acc.map(|v| v / n as f32)
                })
            }
            Self::SurfaceBlur { radius, threshold } => bilateral(img, *radius, *threshold),
            Self::TiltShift { focus_y, focus_height, transition, blur } => {
                let blurred = gaussian_blur(img, *blur);
                let mut out = img.clone();
                let iw = img.width() as usize;
                let src = blurred.pixels();
                out.pixels_mut().par_chunks_mut(iw * 4).enumerate().for_each(|(y, row)| {
                    let v = (y as f32 + 0.5) / h;
                    let d = ((v - focus_y).abs() - focus_height / 2.0).max(0.0);
                    let t = (d / transition.max(1e-3)).clamp(0.0, 1.0);
                    let t = t * t * (3.0 - 2.0 * t);
                    for (x, p) in row.chunks_exact_mut(4).enumerate() {
                        let s = &src[(y * iw + x) * 4..(y * iw + x) * 4 + 4];
                        for c in 0..4 {
                            p[c] = (p[c] as f32 + (s[c] as f32 - p[c] as f32) * t).round() as u8;
                        }
                    }
                });
                out
            }
            Self::IrisBlur { cx, cy, rx, ry, transition, blur } => {
                let blurred = gaussian_blur(img, *blur);
                let src = blurred.pixels();
                let iw = img.width() as usize;
                let mut out = img.clone();
                out.pixels_mut().par_chunks_mut(iw * 4).enumerate().for_each(|(y, row)| {
                    for (x, p) in row.chunks_exact_mut(4).enumerate() {
                        let nx = ((x as f32 + 0.5) / w - cx) / rx.max(1e-3);
                        let ny = ((y as f32 + 0.5) / h - cy) / ry.max(1e-3);
                        let d = (nx * nx + ny * ny).sqrt() - 1.0;
                        let t = (d / transition.max(1e-3)).clamp(0.0, 1.0);
                        let t = t * t * (3.0 - 2.0 * t);
                        let s = &src[(y * iw + x) * 4..(y * iw + x) * 4 + 4];
                        for c in 0..4 {
                            p[c] = (p[c] as f32 + (s[c] as f32 - p[c] as f32) * t).round() as u8;
                        }
                    }
                });
                out
            }
            Self::Sharpen => convolve3(img, [0.0, -0.25, 0.0, -0.25, 2.0, -0.25, 0.0, -0.25, 0.0]),
            Self::SharpenMore => convolve3(img, [-0.25, -0.25, -0.25, -0.25, 3.0, -0.25, -0.25, -0.25, -0.25]),
            Self::UnsharpMask { amount, radius, threshold } => {
                let blurred = gaussian_blur(img, *radius);
                let k = amount / 100.0;
                let t = *threshold as f32;
                zip_pixels(img, &blurred, |o, b| {
                    let mut r = o;
                    for c in 0..3 {
                        let d = o[c] as f32 - b[c] as f32;
                        if d.abs() >= t {
                            r[c] = (o[c] as f32 + d * k).round().clamp(0.0, 255.0) as u8;
                        }
                    }
                    r
                })
            }
            Self::AddNoise { amount, dist, monochrome, seed } => {
                let a = amount / 100.0;
                let mut out = img.clone();
                let iw = img.width() as usize;
                let s = *seed;
                out.pixels_mut().par_chunks_mut(iw * 4).enumerate().for_each(|(y, row)| {
                    for (x, p) in row.chunks_exact_mut(4).enumerate() {
                        let n = |k: u32| {
                            let u = pixel_noise(
                                x as u32 ^ s.wrapping_mul(31).wrapping_add(k * 7919),
                                y as u32 + k * 104_729,
                            );
                            match dist {
                                NoiseDist::Uniform => (u - 0.5) * 2.0,
                                NoiseDist::Gaussian => {
                                    let u2 = pixel_noise(y as u32 ^ (k * 15_485_863), x as u32 ^ s).max(1e-6);
                                    (-2.0 * u2.ln()).sqrt() * (TAU * u).cos() * 0.5
                                }
                            }
                        };
                        let mono = n(0);
                        for c in 0..3 {
                            let d = if *monochrome { mono } else { n(c as u32 + 1) };
                            p[c] = (p[c] as f32 + d * a * 255.0).round().clamp(0.0, 255.0) as u8;
                        }
                    }
                });
                out
            }
            Self::Median { radius } => median(img, radius.round().max(1.0) as i32),
            Self::ReduceNoise { strength, preserve_details } => {
                let smooth = bilateral(img, strength.max(1.0), 8.0 + strength * 3.0);
                let keep = preserve_details / 100.0;
                // Put back some of the high-frequency detail the bilateral pass removed.
                let detail = gaussian_blur(img, 1.0);
                zip3(img, &smooth, &detail, |o, s, d| {
                    let mut r = s;
                    for c in 0..3 {
                        let hf = o[c] as f32 - d[c] as f32;
                        r[c] = (s[c] as f32 + hf * keep).round().clamp(0.0, 255.0) as u8;
                    }
                    r
                })
            }
            Self::DustScratches { radius, threshold } => {
                let m = median(img, radius.round().max(1.0) as i32);
                let t = *threshold as i32;
                zip_pixels(img, &m, |o, md| {
                    let diff = (0..3).map(|c| (o[c] as i32 - md[c] as i32).abs()).max().unwrap_or(0);
                    if diff > t { md } else { o }
                })
            }
            Self::Twirl { angle } => {
                let r_max = w.min(h) / 2.0;
                let ang = angle.to_radians();
                warp(img, |x, y| {
                    let (dx, dy) = (x - w / 2.0, y - h / 2.0);
                    let r = (dx * dx + dy * dy).sqrt();
                    if r >= r_max {
                        return (x, y);
                    }
                    let a = ang * (1.0 - r / r_max).powi(2);
                    let (s, c) = a.sin_cos();
                    (w / 2.0 + dx * c - dy * s, h / 2.0 + dx * s + dy * c)
                })
            }
            Self::Wave { amplitude, wavelength, horizontal, vertical } => {
                let k = TAU / wavelength.max(1.0);
                warp(img, |x, y| {
                    let nx = if *horizontal { x + amplitude * (y * k).sin() } else { x };
                    let ny = if *vertical { y + amplitude * (x * k).sin() } else { y };
                    (nx, ny)
                })
            }
            Self::Ripple { amount, size } => {
                let a = amount / 100.0 * size * 0.4;
                let k = TAU / (size * 2.0).max(2.0);
                warp(img, |x, y| (x + a * (y * k).sin() + a * 0.5 * (y * k * 2.3).sin(), y + a * (x * k).sin()))
            }
            Self::Pinch { amount } => {
                let r_max = w.min(h) / 2.0;
                let a = amount / 100.0;
                warp(img, |x, y| {
                    let (dx, dy) = (x - w / 2.0, y - h / 2.0);
                    let r = (dx * dx + dy * dy).sqrt();
                    if r >= r_max || r < 1e-3 {
                        return (x, y);
                    }
                    let f = (PI * r / r_max / 2.0).sin().powf(-a);
                    let k = (r / r_max).powf(1.0) * f;
                    let s = k * r_max / r;
                    (w / 2.0 + dx * s, h / 2.0 + dy * s)
                })
            }
            Self::Spherize { amount } => {
                let r_max = w.min(h) / 2.0;
                let a = amount / 100.0;
                warp(img, |x, y| {
                    let (dx, dy) = (x - w / 2.0, y - h / 2.0);
                    let r = (dx * dx + dy * dy).sqrt() / r_max;
                    if !(1e-4..1.0).contains(&r) {
                        return (x, y);
                    }
                    let sphere = r.asin() / (PI / 2.0);
                    let nr = r + (sphere - r) * a;
                    let s = nr / r;
                    (w / 2.0 + dx * s, h / 2.0 + dy * s)
                })
            }
            Self::PolarCoordinates { mode } => {
                let r_max = (w.min(h)) / 2.0;
                match mode {
                    PolarMode::RectToPolar => warp(img, |x, y| {
                        let (dx, dy) = (x - w / 2.0, y - h / 2.0);
                        let r = (dx * dx + dy * dy).sqrt() / r_max;
                        let a = (dx.atan2(-dy) + TAU) % TAU;
                        (a / TAU * w, r * h)
                    }),
                    PolarMode::PolarToRect => warp(img, |x, y| {
                        let a = x / w * TAU;
                        let r = y / h * r_max;
                        (w / 2.0 + r * a.sin(), h / 2.0 - r * a.cos())
                    }),
                }
            }
            Self::Mosaic { cell } => {
                let cs = cell.round().max(1.0) as u32;
                let b = Buf::from_image(img);
                let (iw, ih) = img.size();
                let cols = iw.div_ceil(cs);
                let rows = ih.div_ceil(cs);
                let means: Vec<[f32; 4]> = (0..rows * cols)
                    .into_par_iter()
                    .map(|i| {
                        let (cx, cy) = (i % cols * cs, i / cols * cs);
                        let mut acc = [0.0; 4];
                        let mut n = 0.0;
                        for y in cy..(cy + cs).min(ih) {
                            for x in cx..(cx + cs).min(iw) {
                                let p = b.px[(y * iw + x) as usize];
                                for k in 0..4 {
                                    acc[k] += p[k];
                                }
                                n += 1.0;
                            }
                        }
                        acc.map(|v| v / n)
                    })
                    .collect();
                map_buf(&b, |x, y| means[((y as u32 / cs) * cols + x as u32 / cs) as usize])
            }
            Self::Crystallize { cell, seed } => {
                let cs = cell.max(2.0);
                let b = Buf::from_image(img);
                let site = |gx: i32, gy: i32| {
                    let jx = pixel_noise(gx as u32 ^ seed.wrapping_mul(977), gy as u32);
                    let jy = pixel_noise(gy as u32 ^ seed.wrapping_mul(613), (gx as u32).wrapping_add(17));
                    ((gx as f32 + jx) * cs, (gy as f32 + jy) * cs)
                };
                map_buf(&b, |x, y| {
                    let (gx, gy) = ((x / cs).floor() as i32, (y / cs).floor() as i32);
                    let mut best = (f32::MAX, (0.0, 0.0));
                    for oy in -1..=1 {
                        for ox in -1..=1 {
                            let s = site(gx + ox, gy + oy);
                            let d = (s.0 - x).powi(2) + (s.1 - y).powi(2);
                            if d < best.0 {
                                best = (d, s);
                            }
                        }
                    }
                    b.sample(best.1.0, best.1.1)
                })
            }
            Self::ColorHalftone { radius } => {
                let cell = radius.max(2.0) * 2.0;
                let b = Buf::from_image(img);
                let angles = [108f32, 162.0, 90.0, 45.0];
                map_buf(&b, |x, y| {
                    let src = b.at(x as i32, y as i32);
                    let a = src[3];
                    if a <= 0.0 {
                        return src;
                    }
                    let mut cmyk_on = [0.0f32; 3];
                    for ch in 0..3 {
                        let (s, c) = angles[ch].to_radians().sin_cos();
                        let (u, v) = (x * c + y * s, -x * s + y * c);
                        let (cu, cv) = ((u / cell).floor() * cell + cell / 2.0, (v / cell).floor() * cell + cell / 2.0);
                        let (px, py) = (cu * c - cv * s, cu * s + cv * c);
                        let sp = b.sample(px, py);
                        let ink = 1.0 - if sp[3] > 0.0 { sp[ch] / sp[3] } else { 1.0 };
                        let d = ((u - cu).powi(2) + (v - cv).powi(2)).sqrt() / (cell / 2.0 * 1.414);
                        cmyk_on[ch] = if d < ink.sqrt() { 1.0 } else { 0.0 };
                    }
                    [(1.0 - cmyk_on[0]) * a, (1.0 - cmyk_on[1]) * a, (1.0 - cmyk_on[2]) * a, a]
                })
            }
            Self::Emboss { angle, height, amount } => {
                let (s, c) = angle.to_radians().sin_cos();
                let (dx, dy) = (c * height, -s * height);
                let k = amount / 100.0;
                let src = img.clone();
                map_pixels(img, |x, y, p| {
                    let a = src.get_clamped((x as f32 + dx).round() as i32, (y as f32 + dy).round() as i32);
                    let b = src.get_clamped((x as f32 - dx).round() as i32, (y as f32 - dy).round() as i32);
                    let la = luma(a.r() as f32, a.g() as f32, a.b() as f32);
                    let lb = luma(b.r() as f32, b.g() as f32, b.b() as f32);
                    let v = (128.0 + (lb - la) * k).round().clamp(0.0, 255.0) as u8;
                    [v, v, v, p[3]]
                })
            }
            Self::FindEdges => {
                let src = img.clone();
                map_pixels(img, |x, y, p| {
                    let g = sobel(&src, x as i32, y as i32);
                    let o = g.map(|v| 255 - v.min(255.0) as u8);
                    [o[0], o[1], o[2], p[3]]
                })
            }
            Self::GlowingEdges { width, brightness } => {
                let soft = gaussian_blur(img, *width);
                let k = brightness / 4.0;
                map_pixels(img, |x, y, p| {
                    let g = sobel(&soft, x as i32, y as i32);
                    let o = g.map(|v| (v * k).min(255.0) as u8);
                    [o[0], o[1], o[2], p[3]]
                })
            }
            Self::OilPaint { radius, levels } => oil_paint(img, radius.round().max(1.0) as i32, (*levels).max(2)),
            Self::Solarize => map_pixels(img, |_, _, p| {
                let f = |v: u8| if v > 127 { 255 - v } else { v };
                [f(p[0]), f(p[1]), f(p[2]), p[3]]
            }),
            Self::Clouds { seed, scale } => {
                let (fg, bg) = (ctx.fg.to_f32(), ctx.bg.to_f32());
                map_pixels(img, |x, y, _| {
                    let t = fbm(x as f32 / scale, y as f32 / scale, *seed);
                    let mut o = [0u8; 4];
                    for c in 0..3 {
                        o[c] = to_u8(fg[c] + (bg[c] - fg[c]) * t);
                    }
                    o[3] = 255;
                    o
                })
            }
            Self::Vignette { amount, midpoint, feather } => {
                let a = amount / 100.0;
                let mid = midpoint / 100.0;
                let f = (feather / 100.0).max(0.01);
                map_pixels(img, |x, y, p| {
                    let nx = (x as f32 + 0.5) / w * 2.0 - 1.0;
                    let ny = (y as f32 + 0.5) / h * 2.0 - 1.0;
                    let d = (nx * nx + ny * ny).sqrt() / std::f32::consts::SQRT_2;
                    let t = ((d - mid * (1.0 - f)) / f).clamp(0.0, 1.0);
                    let t = t * t * (3.0 - 2.0 * t);
                    let mut o = p;
                    for c in 0..3 {
                        let v = p[c] as f32 / 255.0;
                        let nv = if a < 0.0 { v * (1.0 + a * t) } else { v + (1.0 - v) * a * t };
                        o[c] = to_u8(nv);
                    }
                    o
                })
            }
            Self::LensFlare { cx, cy, brightness } => {
                let (fx, fy) = (cx * w, cy * h);
                let (ax, ay) = (w - fx, h - fy);
                let diag = (w * w + h * h).sqrt();
                let b = brightness / 100.0;
                let rings: [(f32, f32, [f32; 3]); 5] = [
                    (0.25, 0.04, [0.9, 0.5, 1.0]),
                    (0.45, 0.025, [0.4, 1.0, 0.6]),
                    (0.7, 0.06, [0.5, 0.7, 1.0]),
                    (1.1, 0.035, [1.0, 0.8, 0.4]),
                    (1.35, 0.08, [0.6, 0.6, 1.0]),
                ];
                map_pixels(img, |x, y, p| {
                    let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                    let d = ((px - fx).powi(2) + (py - fy).powi(2)).sqrt() / diag;
                    let core = (-d * d / 0.0009).exp() * 1.4 + (-d / 0.08).exp() * 0.35;
                    let mut add = [core * 1.0, core * 0.95, core * 0.85];
                    for (t, r, col) in rings {
                        let (rx, ry) = (fx + (ax - fx) * t, fy + (ay - fy) * t);
                        let rd = ((px - rx).powi(2) + (py - ry).powi(2)).sqrt() / diag;
                        let ring = (1.0 - (rd / r)).clamp(0.0, 1.0).powf(0.6) * 0.18;
                        for c in 0..3 {
                            add[c] += ring * col[c];
                        }
                    }
                    let mut o = p;
                    for c in 0..3 {
                        let v = p[c] as f32 / 255.0;
                        o[c] = to_u8(1.0 - (1.0 - v) * (1.0 - (add[c] * b).min(1.0)));
                    }
                    o
                })
            }
        }
    }
}

/// Build an image from a per-pixel function over pixel corners (x, y are integer coordinates).
fn map_buf(b: &Buf, f: impl Fn(f32, f32) -> [f32; 4] + Sync) -> Image {
    let mut px = vec![[0.0f32; 4]; b.px.len()];
    px.par_chunks_mut(b.w).enumerate().for_each(|(y, row)| {
        for (x, v) in row.iter_mut().enumerate() {
            *v = f(x as f32, y as f32);
        }
    });
    Buf { w: b.w, h: b.h, px }.to_image()
}

fn map_pixels(img: &Image, f: impl Fn(u32, u32, [u8; 4]) -> [u8; 4] + Sync) -> Image {
    let mut out = img.clone();
    let w = img.width() as usize;
    out.pixels_mut().par_chunks_mut(w * 4).enumerate().for_each(|(y, row)| {
        for (x, p) in row.chunks_exact_mut(4).enumerate() {
            let r = f(x as u32, y as u32, [p[0], p[1], p[2], p[3]]);
            p.copy_from_slice(&r);
        }
    });
    out
}

fn zip_pixels(a: &Image, b: &Image, f: impl Fn([u8; 4], [u8; 4]) -> [u8; 4] + Sync) -> Image {
    let bp = b.pixels();
    let w = a.width() as usize;
    map_pixels(a, |x, y, p| {
        let i = (y as usize * w + x as usize) * 4;
        f(p, [bp[i], bp[i + 1], bp[i + 2], bp[i + 3]])
    })
}

fn zip3(a: &Image, b: &Image, c: &Image, f: impl Fn([u8; 4], [u8; 4], [u8; 4]) -> [u8; 4] + Sync) -> Image {
    let (bp, cp) = (b.pixels(), c.pixels());
    let w = a.width() as usize;
    map_pixels(a, |x, y, p| {
        let i = (y as usize * w + x as usize) * 4;
        f(p, [bp[i], bp[i + 1], bp[i + 2], bp[i + 3]], [cp[i], cp[i + 1], cp[i + 2], cp[i + 3]])
    })
}

/// Inverse warp: for each destination pixel centre, `f` returns the source position.
fn warp(img: &Image, f: impl Fn(f32, f32) -> (f32, f32) + Sync) -> Image {
    let b = Buf::from_image(img);
    map_buf(&b, |x, y| {
        let (sx, sy) = f(x + 0.5, y + 0.5);
        b.sample(sx, sy)
    })
}

fn convolve3(img: &Image, k: [f32; 9]) -> Image {
    let src = img.clone();
    map_pixels(img, |x, y, p| {
        let mut acc = [0.0f32; 3];
        for j in 0..3 {
            for i in 0..3 {
                let s = src.get_clamped(x as i32 + i - 1, y as i32 + j - 1);
                let kv = k[(j * 3 + i) as usize];
                for c in 0..3 {
                    acc[c] += s.0[c] as f32 * kv;
                }
            }
        }
        [
            acc[0].round().clamp(0.0, 255.0) as u8,
            acc[1].round().clamp(0.0, 255.0) as u8,
            acc[2].round().clamp(0.0, 255.0) as u8,
            p[3],
        ]
    })
}

fn sobel(img: &Image, x: i32, y: i32) -> [f32; 3] {
    let g = |dx: i32, dy: i32| img.get_clamped(x + dx, y + dy).0;
    let mut out = [0.0; 3];
    for c in 0..3 {
        let v = |dx, dy| g(dx, dy)[c] as f32;
        let gx = -v(-1, -1) - 2.0 * v(-1, 0) - v(-1, 1) + v(1, -1) + 2.0 * v(1, 0) + v(1, 1);
        let gy = -v(-1, -1) - 2.0 * v(0, -1) - v(1, -1) + v(-1, 1) + 2.0 * v(0, 1) + v(1, 1);
        out[c] = (gx * gx + gy * gy).sqrt() / 2.0;
    }
    out
}

/// Edge-preserving blur: neighbours count less the more their colour differs.
fn bilateral(img: &Image, radius: f32, threshold: f32) -> Image {
    let r = radius.round().clamp(1.0, 25.0) as i32;
    let step = (r / 6).max(1);
    let t = threshold.max(1.0);
    let src = img.clone();
    map_pixels(img, |x, y, p| {
        let mut acc = [0.0f32; 3];
        let mut wsum = 0.0;
        let mut dy = -r;
        while dy <= r {
            let mut dx = -r;
            while dx <= r {
                if dx * dx + dy * dy <= r * r {
                    let s = src.get_clamped(x as i32 + dx, y as i32 + dy).0;
                    let diff = (0..3).map(|c| (s[c] as f32 - p[c] as f32).abs()).sum::<f32>() / 3.0;
                    let w = (1.0 - diff / (2.5 * t)).max(0.0);
                    for c in 0..3 {
                        acc[c] += s[c] as f32 * w;
                    }
                    wsum += w;
                }
                dx += step;
            }
            dy += step;
        }
        if wsum <= 0.0 {
            return p;
        }
        [(acc[0] / wsum).round() as u8, (acc[1] / wsum).round() as u8, (acc[2] / wsum).round() as u8, p[3]]
    })
}

/// Per-channel median using a sliding histogram along each row.
fn median(img: &Image, r: i32) -> Image {
    let (w, h) = (img.width() as i32, img.height() as i32);
    let src = img.pixels();
    let area = ((2 * r + 1) * (2 * r + 1)) as u32;
    let half = area / 2 + 1;
    let mut out = img.clone();
    out.pixels_mut().par_chunks_mut(w as usize * 4).enumerate().for_each(|(y, row)| {
        let y = y as i32;
        let mut hist = [[0u32; 256]; 3];
        let get = |x: i32, yy: i32| {
            let x = x.clamp(0, w - 1) as usize;
            let yy = yy.clamp(0, h - 1) as usize;
            &src[(yy * w as usize + x) * 4..(yy * w as usize + x) * 4 + 3]
        };
        for dy in -r..=r {
            for dx in -r..=r {
                let p = get(dx, y + dy);
                for c in 0..3 {
                    hist[c][p[c] as usize] += 1;
                }
            }
        }
        for x in 0..w {
            for c in 0..3 {
                let mut acc = 0;
                for (v, &n) in hist[c].iter().enumerate() {
                    acc += n;
                    if acc >= half {
                        row[x as usize * 4 + c] = v as u8;
                        break;
                    }
                }
            }
            if x + 1 < w {
                for dy in -r..=r {
                    let old = get(x - r, y + dy);
                    let new = get(x + r + 1, y + dy);
                    for c in 0..3 {
                        hist[c][old[c] as usize] -= 1;
                        hist[c][new[c] as usize] += 1;
                    }
                }
            }
        }
    });
    out
}

/// Kuwahara-flavoured oil paint: the most common intensity bucket in the neighbourhood wins.
fn oil_paint(img: &Image, r: i32, levels: u8) -> Image {
    let src = img.clone();
    let lv = levels as usize;
    map_pixels(img, |x, y, p| {
        let mut count = vec![0u32; lv];
        let mut sum = vec![[0u32; 3]; lv];
        for dy in -r..=r {
            for dx in -r..=r {
                if dx * dx + dy * dy > r * r {
                    continue;
                }
                let s = src.get_clamped(x as i32 + dx, y as i32 + dy).0;
                let l = ((s[0] as usize + s[1] as usize + s[2] as usize) * (lv - 1)) / (3 * 255);
                count[l] += 1;
                for c in 0..3 {
                    sum[l][c] += s[c] as u32;
                }
            }
        }
        let (best, &n) = count.iter().enumerate().max_by_key(|(_, n)| **n).unwrap();
        if n == 0 {
            return p;
        }
        [(sum[best][0] / n) as u8, (sum[best][1] / n) as u8, (sum[best][2] / n) as u8, p[3]]
    })
}

fn value_noise(x: f32, y: f32, seed: u32) -> f32 {
    let (xi, yi) = (x.floor() as i32, y.floor() as i32);
    let (tx, ty) = (x - xi as f32, y - yi as f32);
    let s = |a: f32| a * a * (3.0 - 2.0 * a);
    let n = |i: i32, j: i32| {
        pixel_noise((i as u32).wrapping_add(seed.wrapping_mul(1013)), (j as u32).wrapping_add(seed.wrapping_mul(7)))
    };
    let (a, b, c, d) = (n(xi, yi), n(xi + 1, yi), n(xi, yi + 1), n(xi + 1, yi + 1));
    let (u, v) = (s(tx), s(ty));
    (a * (1.0 - u) + b * u) * (1.0 - v) + (c * (1.0 - u) + d * u) * v
}

/// Fractal (fBm) value noise in 0..1.
pub fn fbm(x: f32, y: f32, seed: u32) -> f32 {
    let mut amp = 0.5;
    let mut freq = 1.0;
    let mut sum = 0.0;
    let mut norm = 0.0;
    for o in 0..6 {
        sum += value_noise(x * freq, y * freq, seed.wrapping_add(o * 131)) * amp;
        norm += amp;
        amp *= 0.5;
        freq *= 2.0;
    }
    sum / norm
}

/// Offset a mask by (dx, dy) whole pixels, filling with zero.
pub fn shift_mask(m: &Mask, dx: i32, dy: i32) -> Mask {
    let (w, h) = (m.width() as i32, m.height() as i32);
    let mut out = Mask::new(w as u32, h as u32);
    let d = out.data_mut();
    for y in 0..h {
        for x in 0..w {
            d[(y * w + x) as usize] = m.get_or_zero(x - dx, y - dy);
        }
    }
    out
}

/// Grow (positive) or shrink (negative) a mask by a circular radius in pixels.
pub fn morph_mask(m: &Mask, radius: f32) -> Mask {
    if radius.abs() < 0.5 {
        return m.clone();
    }
    let grow = radius > 0.0;
    // Distance from each pixel to the nearest pixel on the other side of the 50% edge.
    let inside: Vec<bool> = m.data().iter().map(|&v| (v >= 128) == grow).collect();
    let dist = distance_field(m.width() as usize, m.height() as usize, &inside);
    // `dist` is the distance to the selection when growing, and to the outside when shrinking.
    let r = radius.abs();
    let data = m
        .data()
        .iter()
        .zip(dist.iter())
        .map(|(&v, &d)| {
            if grow {
                if v >= 128 { v } else { (r - d + 1.0).clamp(0.0, 1.0).mul_add(255.0, 0.0).max(v as f32) as u8 }
            } else if v < 128 {
                v
            } else {
                ((d - r).clamp(0.0, 1.0) * v as f32) as u8
            }
        })
        .collect();
    Mask::from_raw(m.width(), m.height(), data)
}

/// Euclidean distance (two-pass, exact per row/column — Felzenszwalb) to the nearest `true`.
pub fn distance_field(w: usize, h: usize, seed: &[bool]) -> Vec<f32> {
    const INF: f32 = 1e12;
    let mut f: Vec<f32> = seed.iter().map(|&s| if s { 0.0 } else { INF }).collect();
    let edt_1d = |f: &mut [f32]| {
        let n = f.len();
        let mut d = vec![0.0f32; n];
        let mut v = vec![0usize; n];
        let mut z = vec![0.0f32; n + 1];
        let mut k = 0;
        z[0] = -INF;
        z[1] = INF;
        for q in 1..n {
            loop {
                let p = v[k];
                let s = ((f[q] + (q * q) as f32) - (f[p] + (p * p) as f32)) / (2.0 * q as f32 - 2.0 * p as f32);
                if s <= z[k] && k > 0 {
                    k -= 1;
                } else {
                    k += 1;
                    v[k] = q;
                    z[k] = s;
                    z[k + 1] = INF;
                    break;
                }
            }
        }
        k = 0;
        for q in 0..n {
            while z[k + 1] < q as f32 {
                k += 1;
            }
            let p = v[k];
            d[q] = (q as f32 - p as f32).powi(2) + f[p];
        }
        f.copy_from_slice(&d);
    };
    f.par_chunks_mut(w).for_each(edt_1d);
    let mut t = vec![0.0f32; w * h];
    for y in 0..h {
        for x in 0..w {
            t[x * h + y] = f[y * w + x];
        }
    }
    t.par_chunks_mut(h).for_each(edt_1d);
    for y in 0..h {
        for x in 0..w {
            f[y * w + x] = t[x * h + y].sqrt();
        }
    }
    f
}

/// Stroke placement relative to the shape edge.
pub fn outline_mask(m: &Mask, width: f32, outside: bool, inside: bool) -> Mask {
    let w = m.width() as usize;
    let h = m.height() as usize;
    let solid: Vec<bool> = m.data().iter().map(|&v| v >= 128).collect();
    let empty: Vec<bool> = solid.iter().map(|b| !b).collect();
    let d_out = if outside { Some(distance_field(w, h, &solid)) } else { None };
    let d_in = if inside { Some(distance_field(w, h, &empty)) } else { None };
    let data = (0..w * h)
        .map(|i| {
            let mut v = 0.0f32;
            if let Some(d) = &d_out {
                if !solid[i] {
                    v = v.max((width - d[i] + 1.0).clamp(0.0, 1.0));
                } else if outside {
                    v = v.max(if inside { 1.0 } else { 0.0 });
                }
            }
            if let Some(d) = &d_in {
                if solid[i] {
                    v = v.max((width - d[i] + 1.0).clamp(0.0, 1.0));
                }
            }
            (v * 255.0).round() as u8
        })
        .collect();
    Mask::from_raw(m.width(), m.height(), data)
}

/// Crop a filter's region (helper for previews of large images).
pub fn region_for_preview(full: Rect, max_side: u32) -> f32 {
    let side = full.w.max(full.h) as f32;
    (max_side as f32 / side).min(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checker(n: u32) -> Image {
        let mut img = Image::new(n, n);
        for y in 0..n {
            for x in 0..n {
                let v = if (x / 4 + y / 4) % 2 == 0 { 255 } else { 0 };
                img.put(x, y, Rgba::rgb(v, v, v));
            }
        }
        img
    }

    #[test]
    fn box_radii_cover_sigma() {
        let r = box_radii(3.0);
        let var: f32 = r.iter().map(|&r| ((2 * r + 1).pow(2) - 1) as f32 / 12.0).sum();
        assert!((var.sqrt() - 3.0).abs() < 0.6);
    }

    #[test]
    fn blur_preserves_flat_colour_and_alpha() {
        let img = Image::filled(20, 20, Rgba::rgb(10, 200, 30));
        let out = gaussian_blur(&img, 6.0);
        assert_eq!(out.get(10, 10), Rgba::rgb(10, 200, 30));
        assert_eq!(out.get(0, 0).a(), 255);
    }

    #[test]
    fn blur_does_not_darken_transparent_edges() {
        let mut img = Image::new(20, 20);
        for y in 5..15 {
            for x in 5..15 {
                img.put(x, y, Rgba::rgb(255, 0, 0));
            }
        }
        let out = gaussian_blur(&img, 4.0);
        let edge = out.get(4, 10);
        assert!(edge.a() > 0 && edge.r() == 255, "{edge:?}");
    }

    #[test]
    fn every_filter_runs() {
        let img = checker(32);
        for f in Filter::catalog() {
            let out = f.run(&img, &FilterCtx::default());
            assert_eq!(out.size(), img.size(), "{}", f.label());
        }
    }

    #[test]
    fn median_removes_speck() {
        let mut img = Image::filled(9, 9, Rgba::rgb(100, 100, 100));
        img.put(4, 4, Rgba::rgb(255, 255, 255));
        let out = median(&img, 1);
        assert_eq!(out.get(4, 4), Rgba::rgb(100, 100, 100));
    }

    #[test]
    fn selection_limits_filter() {
        let mut img = checker(16);
        let before = img.clone();
        let mut m = Mask::new(16, 16);
        m.set(0, 0, 255);
        Filter::Solarize.apply(&mut img, Some(&m), &FilterCtx::default());
        assert_eq!(img.get(0, 0), Rgba::rgb(0, 0, 0));
        assert_eq!(img.get(8, 0), before.get(8, 0));
    }

    #[test]
    fn distance_field_and_morph() {
        let mut m = Mask::new(21, 21);
        m.set(10, 10, 255);
        let grown = morph_mask(&m, 3.0);
        assert_eq!(grown.get(13, 10), 255);
        assert_eq!(grown.get(15, 10), 0);
        let shrunk = morph_mask(&grown, -2.0);
        assert!(shrunk.get(10, 10) > 0 && shrunk.get(13, 10) == 0);
    }
}
