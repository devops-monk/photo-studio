//! Resampling and geometric transforms of images and masks.

use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use crate::geom::Affine;
use crate::image::{Image, Mask};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Resample {
    Nearest,
    Bilinear,
    #[default]
    Bicubic,
    Lanczos,
}

impl Resample {
    pub const ALL: [Resample; 4] = [Self::Nearest, Self::Bilinear, Self::Bicubic, Self::Lanczos];

    pub fn label(&self) -> &'static str {
        match self {
            Self::Nearest => "Nearest Neighbor (hard edges)",
            Self::Bilinear => "Bilinear",
            Self::Bicubic => "Bicubic (smooth gradients)",
            Self::Lanczos => "Lanczos (sharpest)",
        }
    }

    fn filter(&self) -> image::imageops::FilterType {
        use image::imageops::FilterType;
        match self {
            Self::Nearest => FilterType::Nearest,
            Self::Bilinear => FilterType::Triangle,
            Self::Bicubic => FilterType::CatmullRom,
            Self::Lanczos => FilterType::Lanczos3,
        }
    }
}

/// Resize in premultiplied space so transparent pixels don't fringe.
pub fn resize(img: &Image, w: u32, h: u32, method: Resample) -> Image {
    if img.size() == (w, h) {
        return img.clone();
    }
    if img.width() == 0 || w == 0 || h == 0 {
        return Image::new(w, h);
    }
    let pre: Vec<f32> = img
        .pixels()
        .chunks_exact(4)
        .flat_map(|p| {
            let a = p[3] as f32 / 255.0;
            [p[0] as f32 / 255.0 * a, p[1] as f32 / 255.0 * a, p[2] as f32 / 255.0 * a, a]
        })
        .collect();
    let buf = image::Rgba32FImage::from_raw(img.width(), img.height(), pre).expect("size");
    let out = image::imageops::resize(&buf, w, h, method.filter());
    let mut v = Vec::with_capacity((w * h * 4) as usize);
    for p in out.pixels() {
        let a = p[3].clamp(0.0, 1.0);
        if a <= 1e-5 {
            v.extend_from_slice(&[0, 0, 0, 0]);
        } else {
            for c in 0..3 {
                v.push(crate::color::to_u8(p[c] / a));
            }
            v.push(crate::color::to_u8(a));
        }
    }
    Image::from_raw(w, h, v)
}

pub fn resize_mask(m: &Mask, w: u32, h: u32, method: Resample) -> Mask {
    if (m.width(), m.height()) == (w, h) {
        return m.clone();
    }
    let g = image::GrayImage::from_raw(m.width(), m.height(), m.data().to_vec()).expect("size");
    let out = image::imageops::resize(&g, w, h, method.filter());
    Mask::from_raw(w, h, out.into_raw())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rotation {
    Cw90,
    Ccw90,
    Half,
}

pub fn rotate(img: &Image, r: Rotation) -> Image {
    let (w, h) = img.size();
    let (nw, nh) = if r == Rotation::Half { (w, h) } else { (h, w) };
    let mut out = Image::new(nw, nh);
    let src = img.pixels();
    out.pixels_mut().par_chunks_mut(nw as usize * 4).enumerate().for_each(|(y, row)| {
        for x in 0..nw as usize {
            let (sx, sy) = match r {
                Rotation::Cw90 => (y, h as usize - 1 - x),
                Rotation::Ccw90 => (w as usize - 1 - y, x),
                Rotation::Half => (w as usize - 1 - x, h as usize - 1 - y),
            };
            let i = (sy * w as usize + sx) * 4;
            row[x * 4..x * 4 + 4].copy_from_slice(&src[i..i + 4]);
        }
    });
    out
}

pub fn rotate_mask(m: &Mask, r: Rotation) -> Mask {
    let img = rotate(&m.to_image(), r);
    Mask::from_raw(img.width(), img.height(), img.pixels().chunks_exact(4).map(|p| p[0]).collect())
}

pub fn flip(img: &Image, horizontal: bool) -> Image {
    let (w, h) = (img.width() as usize, img.height() as usize);
    let mut out = img.clone();
    let src = img.pixels();
    out.pixels_mut().par_chunks_mut(w * 4).enumerate().for_each(|(y, row)| {
        for x in 0..w {
            let (sx, sy) = if horizontal { (w - 1 - x, y) } else { (x, h - 1 - y) };
            let i = (sy * w + sx) * 4;
            row[x * 4..x * 4 + 4].copy_from_slice(&src[i..i + 4]);
        }
    });
    out
}

pub fn flip_mask(m: &Mask, horizontal: bool) -> Mask {
    let img = flip(&m.to_image(), horizontal);
    Mask::from_raw(img.width(), img.height(), img.pixels().chunks_exact(4).map(|p| p[0]).collect())
}

/// Render `img` through affine `t` (source → destination) onto a `w × h` transparent canvas.
pub fn affine(img: &Image, t: &Affine, w: u32, h: u32) -> Image {
    let Some(inv) = t.invert() else { return Image::new(w, h) };
    let mut out = Image::new(w, h);
    out.pixels_mut().par_chunks_mut(w as usize * 4).enumerate().for_each(|(y, row)| {
        for x in 0..w as usize {
            let (sx, sy) = inv.apply(x as f32 + 0.5, y as f32 + 0.5);
            let s = img.sample_bilinear(sx, sy);
            if s[3] <= 0.0 {
                continue;
            }
            row[x * 4..x * 4 + 4].copy_from_slice(&[
                crate::color::to_u8(s[0]),
                crate::color::to_u8(s[1]),
                crate::color::to_u8(s[2]),
                crate::color::to_u8(s[3]),
            ]);
        }
    });
    out
}

pub fn affine_mask(m: &Mask, t: &Affine, w: u32, h: u32) -> Mask {
    let img = affine(&m.to_image(), t, w, h);
    Mask::from_raw(w, h, img.pixels().chunks_exact(4).map(|p| ((p[0] as u32 * p[3] as u32) / 255) as u8).collect())
}

/// Rotate about the centre by `deg` degrees, growing the canvas to fit.
/// Returns the new size and the transform used.
pub fn rotation_fit(w: u32, h: u32, deg: f32) -> (u32, u32, Affine) {
    let rad = deg.to_radians();
    let (s, c) = (rad.sin().abs(), rad.cos().abs());
    let nw = (w as f32 * c + h as f32 * s).round().max(1.0) as u32;
    let nh = (w as f32 * s + h as f32 * c).round().max(1.0) as u32;
    let t = Affine::translate(-(w as f32) / 2.0, -(h as f32) / 2.0)
        .then(&Affine::rotate(rad))
        .then(&Affine::translate(nw as f32 / 2.0, nh as f32 / 2.0));
    (nw, nh, t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Rgba;

    #[test]
    fn rotations_compose_to_identity() {
        let mut img = Image::new(3, 2);
        img.put(0, 0, Rgba::rgb(1, 2, 3));
        img.put(2, 1, Rgba::rgb(4, 5, 6));
        let r = rotate(&rotate(&img, Rotation::Cw90), Rotation::Ccw90);
        assert_eq!(r, img);
        let cw = rotate(&img, Rotation::Cw90);
        assert_eq!(cw.size(), (2, 3));
        assert_eq!(cw.get(1, 0), Rgba::rgb(1, 2, 3));
        assert_eq!(rotate(&rotate(&img, Rotation::Half), Rotation::Half), img);
    }

    #[test]
    fn flip_twice_is_identity() {
        let mut img = Image::new(3, 3);
        img.put(0, 1, Rgba::WHITE);
        assert_eq!(flip(&img, true).get(2, 1), Rgba::WHITE);
        assert_eq!(flip(&flip(&img, false), false), img);
    }

    #[test]
    fn resize_keeps_edges_clean() {
        let mut img = Image::new(4, 4);
        img.put(1, 1, Rgba::rgb(255, 0, 0));
        let big = resize(&img, 16, 16, Resample::Bilinear);
        for p in big.pixels().chunks_exact(4) {
            if p[3] > 0 {
                assert!(p[0] > 200 && p[1] < 20, "fringe {p:?}");
            }
        }
    }

    #[test]
    fn rotation_fit_contains_canvas() {
        let (w, h, _) = rotation_fit(100, 50, 90.0);
        assert_eq!((w, h), (50, 100));
    }
}
