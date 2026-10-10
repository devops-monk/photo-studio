//! Selection construction, combination and refinement.

use std::collections::VecDeque;

use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use crate::color::Rgba;
use crate::filters::{blur_mask, morph_mask, outline_mask};
use crate::geom::Rect;
use crate::image::{Image, Mask};
use crate::shape::{path_mask, polygon_path};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SelectOp {
    #[default]
    Replace,
    Add,
    Subtract,
    Intersect,
}

impl SelectOp {
    pub const ALL: [SelectOp; 4] = [Self::Replace, Self::Add, Self::Subtract, Self::Intersect];

    pub fn label(&self) -> &'static str {
        match self {
            Self::Replace => "New selection",
            Self::Add => "Add to selection",
            Self::Subtract => "Subtract from selection",
            Self::Intersect => "Intersect with selection",
        }
    }
}

pub fn rect_mask(w: u32, h: u32, r: Rect) -> Mask {
    let mut m = Mask::new(w, h);
    let r = r.intersect(&Rect::sized(w, h));
    let d = m.data_mut();
    for y in r.y..r.bottom() {
        let s = y as usize * w as usize;
        d[s + r.x as usize..s + r.right() as usize].fill(255);
    }
    m
}

pub fn ellipse_mask(w: u32, h: u32, a: (f32, f32), b: (f32, f32)) -> Mask {
    match crate::shape::path(crate::shape::ShapeKind::Ellipse, a, b) {
        Some(p) => path_mask(&p, w, h, true),
        None => Mask::new(w, h),
    }
}

pub fn polygon_mask(w: u32, h: u32, points: &[(f32, f32)]) -> Mask {
    if points.len() < 3 {
        return Mask::new(w, h);
    }
    match polygon_path(points) {
        Some(p) => path_mask(&p, w, h, true),
        None => Mask::new(w, h),
    }
}

/// Combine a new selection shape with the current selection.
pub fn combine(current: Option<&Mask>, new: Mask, op: SelectOp) -> Mask {
    let Some(cur) = current else {
        return match op {
            SelectOp::Subtract | SelectOp::Intersect => Mask::new(new.width(), new.height()),
            _ => new,
        };
    };
    let data = cur
        .data()
        .iter()
        .zip(new.data())
        .map(|(&a, &b)| match op {
            SelectOp::Replace => b,
            SelectOp::Add => a.max(b),
            SelectOp::Subtract => ((a as u32 * (255 - b as u32) + 127) / 255) as u8,
            SelectOp::Intersect => ((a as u32 * b as u32 + 127) / 255) as u8,
        })
        .collect();
    Mask::from_raw(new.width(), new.height(), data)
}

#[inline]
fn color_dist(a: [u8; 4], b: [u8; 4]) -> u32 {
    let d = (0..3).map(|i| (a[i] as i32 - b[i] as i32).unsigned_abs()).max().unwrap_or(0);
    // Transparent pixels only match transparent pixels.
    d.max((a[3] as i32 - b[3] as i32).unsigned_abs())
}

/// Magic wand / paint bucket region: pixels within `tolerance` of the colour at (x, y).
pub fn flood_mask(img: &Image, x: u32, y: u32, tolerance: u8, contiguous: bool, anti_alias: bool) -> Mask {
    let (w, h) = img.size();
    let mut m = Mask::new(w, h);
    if x >= w || y >= h {
        return m;
    }
    let seed = img.get(x, y).0;
    let tol = tolerance as u32;
    let px = img.pixels();
    let matches = |i: usize| color_dist([px[i * 4], px[i * 4 + 1], px[i * 4 + 2], px[i * 4 + 3]], seed) <= tol;
    {
        let d = m.data_mut();
        if contiguous {
            let mut q = VecDeque::new();
            let start = y as usize * w as usize + x as usize;
            d[start] = 255;
            q.push_back(start);
            while let Some(i) = q.pop_front() {
                let (cx, cy) = (i % w as usize, i / w as usize);
                let mut visit = |j: usize| {
                    if d[j] == 0 && matches(j) {
                        d[j] = 255;
                        q.push_back(j);
                    }
                };
                if cx > 0 {
                    visit(i - 1);
                }
                if cx + 1 < w as usize {
                    visit(i + 1);
                }
                if cy > 0 {
                    visit(i - w as usize);
                }
                if cy + 1 < h as usize {
                    visit(i + w as usize);
                }
            }
        } else {
            d.par_iter_mut().enumerate().for_each(|(i, v)| {
                if matches(i) {
                    *v = 255;
                }
            });
        }
    }
    if anti_alias { blur_mask(&m, 1.0) } else { m }
}

/// Select every pixel near `color`, with soft falloff over `fuzziness`.
pub fn color_range(img: &Image, color: Rgba, fuzziness: u8) -> Mask {
    let f = fuzziness.max(1) as f32;
    let data = img
        .pixels()
        .par_chunks_exact(4)
        .map(|p| {
            let d = color_dist([p[0], p[1], p[2], 255], color.0) as f32;
            ((1.0 - d / f).clamp(0.0, 1.0) * 255.0).round() as u8
        })
        .collect();
    Mask::from_raw(img.width(), img.height(), data)
}

pub fn feather(m: &Mask, radius: f32) -> Mask {
    blur_mask(m, radius)
}

pub fn expand(m: &Mask, by: f32) -> Mask {
    morph_mask(m, by.abs())
}

pub fn contract(m: &Mask, by: f32) -> Mask {
    morph_mask(m, -by.abs())
}

pub fn border(m: &Mask, width: f32) -> Mask {
    let a = outline_mask(m, width / 2.0, true, false);
    let b = outline_mask(m, width / 2.0, false, true);
    let data = a.data().iter().zip(b.data()).map(|(&x, &y)| x.max(y)).collect();
    Mask::from_raw(m.width(), m.height(), data)
}

/// Round off jagged corners: blur, then re-threshold.
pub fn smooth(m: &Mask, radius: f32) -> Mask {
    let b = blur_mask(m, radius);
    let data = b.data().iter().map(|&v| if v >= 128 { 255 } else { 0 }).collect();
    Mask::from_raw(m.width(), m.height(), data)
}

/// Quick Selection: grow a region from (x, y) across similar colour, kept near the brush.
pub fn quick_select(img: &Image, x: f32, y: f32, brush: f32) -> Mask {
    let (w, h) = img.size();
    let m = flood_mask(img, x as u32, y as u32, 40, true, false);
    let reach = brush * 3.0;
    let mut out = m;
    let d = out.data_mut();
    for (i, v) in d.iter_mut().enumerate() {
        if *v != 0 {
            let (px, py) = ((i % w as usize) as f32, (i / w as usize) as f32);
            if (px - x).powi(2) + (py - y).powi(2) > reach * reach {
                *v = 0;
            }
        }
    }
    let _ = h;
    smooth(&out, 2.0)
}

/// Select Subject: separate the subject from a background modelled by the image border.
pub fn select_subject(img: &Image) -> Mask {
    let (w, h) = img.size();
    if w < 4 || h < 4 {
        return Mask::filled(w, h, 255);
    }
    // Background palette: colours sampled around the border, clustered coarsely.
    let mut samples = Vec::new();
    let step = ((w + h) / 200).max(1);
    for x in (0..w).step_by(step as usize) {
        samples.push(img.get(x, 0).to_f32());
        samples.push(img.get(x, h - 1).to_f32());
    }
    for y in (0..h).step_by(step as usize) {
        samples.push(img.get(0, y).to_f32());
        samples.push(img.get(w - 1, y).to_f32());
    }
    let mut centres: Vec<[f32; 4]> = Vec::new();
    for s in &samples {
        if !centres.iter().any(|c| (0..3).map(|i| (c[i] - s[i]).powi(2)).sum::<f32>() < 0.01) {
            centres.push(*s);
        }
        if centres.len() > 48 {
            break;
        }
    }
    // Distance to nearest background colour, combined with a soft centre prior.
    let lum_dist: Vec<f32> = img
        .pixels()
        .par_chunks_exact(4)
        .enumerate()
        .map(|(i, p)| {
            let c = [p[0] as f32 / 255.0, p[1] as f32 / 255.0, p[2] as f32 / 255.0];
            let d = centres
                .iter()
                .map(|b| (0..3).map(|k| (b[k] - c[k]).powi(2)).sum::<f32>().sqrt())
                .fold(f32::MAX, f32::min);
            let (x, y) = ((i % w as usize) as f32 / w as f32 - 0.5, (i / w as usize) as f32 / h as f32 - 0.5);
            let prior = 1.0 - (x * x + y * y).sqrt() * 0.6;
            d * prior
        })
        .collect();
    let mut sorted = lum_dist.clone();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let threshold = (sorted[sorted.len() / 2] * 1.4).max(0.12);
    let raw: Vec<u8> = lum_dist.iter().map(|&d| if d > threshold { 255 } else { 0 }).collect();
    let m = Mask::from_raw(w, h, raw);
    let m = largest_component(&m);
    let m = fill_holes(&m);
    feather(&smooth(&m, 3.0), 1.5)
}

/// Keep only the largest 4-connected region.
pub fn largest_component(m: &Mask) -> Mask {
    let (w, h) = (m.width() as usize, m.height() as usize);
    let src = m.data();
    let mut label = vec![0u32; w * h];
    let mut best = (0u32, 0usize);
    let mut next = 1;
    for start in 0..w * h {
        if src[start] < 128 || label[start] != 0 {
            continue;
        }
        let mut q = VecDeque::from([start]);
        label[start] = next;
        let mut n = 0;
        while let Some(i) = q.pop_front() {
            n += 1;
            let (x, y) = (i % w, i / w);
            for j in
                [(x > 0).then(|| i - 1), (x + 1 < w).then(|| i + 1), (y > 0).then(|| i - w), (y + 1 < h).then(|| i + w)]
                    .into_iter()
                    .flatten()
            {
                if src[j] >= 128 && label[j] == 0 {
                    label[j] = next;
                    q.push_back(j);
                }
            }
        }
        if n > best.1 {
            best = (next, n);
        }
        next += 1;
    }
    Mask::from_raw(m.width(), m.height(), label.iter().map(|&l| if l == best.0 && l != 0 { 255 } else { 0 }).collect())
}

/// Fill enclosed holes (background regions not connected to the border).
pub fn fill_holes(m: &Mask) -> Mask {
    let (w, h) = (m.width() as usize, m.height() as usize);
    let src = m.data();
    let mut outside = vec![false; w * h];
    let mut q = VecDeque::new();
    for x in 0..w {
        for y in [0, h - 1] {
            let i = y * w + x;
            if src[i] < 128 && !outside[i] {
                outside[i] = true;
                q.push_back(i);
            }
        }
    }
    for y in 0..h {
        for x in [0, w - 1] {
            let i = y * w + x;
            if src[i] < 128 && !outside[i] {
                outside[i] = true;
                q.push_back(i);
            }
        }
    }
    while let Some(i) = q.pop_front() {
        let (x, y) = (i % w, i / w);
        for j in
            [(x > 0).then(|| i - 1), (x + 1 < w).then(|| i + 1), (y > 0).then(|| i - w), (y + 1 < h).then(|| i + w)]
                .into_iter()
                .flatten()
        {
            if src[j] < 128 && !outside[j] {
                outside[j] = true;
                q.push_back(j);
            }
        }
    }
    Mask::from_raw(m.width(), m.height(), outside.iter().map(|&o| if o { 0 } else { 255 }).collect())
}

/// Pixel-edge segments along the 50% boundary of a mask, for drawing marching ants.
/// Each segment is ((x0, y0), (x1, y1)) in canvas pixels.
pub fn outline_segments(m: &Mask) -> Vec<((i32, i32), (i32, i32))> {
    let (w, h) = (m.width() as i32, m.height() as i32);
    let inside = |x: i32, y: i32| m.get_or_zero(x, y) >= 128;
    let mut segs = Vec::new();
    for y in 0..=h {
        let mut run: Option<i32> = None;
        for x in 0..=w {
            let edge = x < w && inside(x, y) != inside(x, y - 1);
            match (edge, run) {
                (true, None) => run = Some(x),
                (false, Some(s)) => {
                    segs.push(((s, y), (x, y)));
                    run = None;
                }
                _ => {}
            }
        }
    }
    for x in 0..=w {
        let mut run: Option<i32> = None;
        for y in 0..=h {
            let edge = y < h && inside(x, y) != inside(x - 1, y);
            match (edge, run) {
                (true, None) => run = Some(y),
                (false, Some(s)) => {
                    segs.push(((x, s), (x, y)));
                    run = None;
                }
                _ => {}
            }
        }
    }
    segs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combine_ops() {
        let a = rect_mask(10, 10, Rect::new(0, 0, 6, 10));
        let b = rect_mask(10, 10, Rect::new(4, 0, 6, 10));
        assert_eq!(combine(Some(&a), b.clone(), SelectOp::Add).bounds(), Some(Rect::new(0, 0, 10, 10)));
        assert_eq!(combine(Some(&a), b.clone(), SelectOp::Intersect).bounds(), Some(Rect::new(4, 0, 2, 10)));
        assert_eq!(combine(Some(&a), b, SelectOp::Subtract).bounds(), Some(Rect::new(0, 0, 4, 10)));
    }

    #[test]
    fn wand_contiguous_vs_global() {
        let mut img = Image::filled(10, 1, Rgba::WHITE);
        img.put(5, 0, Rgba::BLACK);
        let c = flood_mask(&img, 0, 0, 10, true, false);
        assert_eq!(c.bounds(), Some(Rect::new(0, 0, 5, 1)));
        let g = flood_mask(&img, 0, 0, 10, false, false);
        assert_eq!(g.data().iter().filter(|&&v| v == 255).count(), 9);
    }

    #[test]
    fn outline_of_square() {
        let m = rect_mask(8, 8, Rect::new(2, 2, 3, 3));
        let segs = outline_segments(&m);
        let len: i32 = segs.iter().map(|(a, b)| (b.0 - a.0).abs() + (b.1 - a.1).abs()).sum();
        assert_eq!(len, 12);
    }

    #[test]
    fn subject_on_plain_background() {
        let mut img = Image::filled(60, 60, Rgba::rgb(240, 240, 240));
        for y in 20..40 {
            for x in 20..40 {
                img.put(x, y, Rgba::rgb(200, 30, 30));
            }
        }
        let m = select_subject(&img);
        assert!(m.get(30, 30) > 200);
        assert!(m.get(5, 5) < 20);
    }
}
