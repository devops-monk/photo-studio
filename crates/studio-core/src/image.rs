//! Copy-on-write raster buffers: RGBA images and 8-bit masks.
//!
//! Pixel storage sits behind an `Arc`, so cloning a buffer (for an undo snapshot) is O(1) and
//! the first write afterwards copies it. Every mutable borrow bumps a generation counter that
//! caches key on.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::color::Rgba;
use crate::geom::Rect;

static GENERATION: AtomicU64 = AtomicU64::new(1);

fn next_gen() -> u64 {
    GENERATION.fetch_add(1, Ordering::Relaxed)
}

/// Straight-alpha RGBA8 image.
#[derive(Clone, Debug)]
pub struct Image {
    width: u32,
    height: u32,
    data: Arc<Vec<u8>>,
    generation: u64,
}

impl PartialEq for Image {
    fn eq(&self, o: &Self) -> bool {
        self.width == o.width && self.height == o.height && self.data == o.data
    }
}

impl Image {
    pub fn new(width: u32, height: u32) -> Self {
        Self::filled(width, height, Rgba::TRANSPARENT)
    }

    pub fn filled(width: u32, height: u32, c: Rgba) -> Self {
        let n = width as usize * height as usize;
        let mut v = Vec::with_capacity(n * 4);
        for _ in 0..n {
            v.extend_from_slice(&c.0);
        }
        Self::from_raw(width, height, v)
    }

    /// Panics if `data.len() != width * height * 4`.
    pub fn from_raw(width: u32, height: u32, data: Vec<u8>) -> Self {
        assert_eq!(data.len(), width as usize * height as usize * 4, "pixel buffer size");
        Self { width, height, data: Arc::new(data), generation: next_gen() }
    }

    pub fn from_rgba_image(img: image::RgbaImage) -> Self {
        let (w, h) = img.dimensions();
        Self::from_raw(w, h, img.into_raw())
    }

    pub fn to_rgba_image(&self) -> image::RgbaImage {
        image::RgbaImage::from_raw(self.width, self.height, self.data.as_ref().clone()).expect("valid buffer")
    }

    pub fn width(&self) -> u32 {
        self.width
    }
    pub fn height(&self) -> u32 {
        self.height
    }
    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }
    pub fn bounds(&self) -> Rect {
        Rect::sized(self.width, self.height)
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn pixels(&self) -> &[u8] {
        &self.data
    }

    pub fn pixels_mut(&mut self) -> &mut [u8] {
        self.generation = next_gen();
        Arc::make_mut(&mut self.data).as_mut_slice()
    }

    pub fn into_raw(self) -> Vec<u8> {
        Arc::try_unwrap(self.data).unwrap_or_else(|a| a.as_ref().clone())
    }

    #[inline]
    pub fn index(&self, x: u32, y: u32) -> usize {
        (y as usize * self.width as usize + x as usize) * 4
    }

    #[inline]
    pub fn get(&self, x: u32, y: u32) -> Rgba {
        let i = self.index(x, y);
        Rgba([self.data[i], self.data[i + 1], self.data[i + 2], self.data[i + 3]])
    }

    /// Like `get`, but clamps coordinates to the edge.
    #[inline]
    pub fn get_clamped(&self, x: i32, y: i32) -> Rgba {
        let x = x.clamp(0, self.width as i32 - 1) as u32;
        let y = y.clamp(0, self.height as i32 - 1) as u32;
        self.get(x, y)
    }

    pub fn put(&mut self, x: u32, y: u32, c: Rgba) {
        let i = self.index(x, y);
        self.pixels_mut()[i..i + 4].copy_from_slice(&c.0);
    }

    /// Bilinear sample at a continuous position (pixel centres at +0.5), premultiplied internally.
    pub fn sample_bilinear(&self, fx: f32, fy: f32) -> [f32; 4] {
        if self.width == 0 || self.height == 0 {
            return [0.0; 4];
        }
        let x = fx - 0.5;
        let y = fy - 0.5;
        let x0 = x.floor();
        let y0 = y.floor();
        let tx = x - x0;
        let ty = y - y0;
        let (x0, y0) = (x0 as i32, y0 as i32);
        let mut acc = [0.0f32; 4];
        for (dx, dy, w) in
            [(0, 0, (1.0 - tx) * (1.0 - ty)), (1, 0, tx * (1.0 - ty)), (0, 1, (1.0 - tx) * ty), (1, 1, tx * ty)]
        {
            let px = x0 + dx;
            let py = y0 + dy;
            if px < 0 || py < 0 || px >= self.width as i32 || py >= self.height as i32 {
                continue;
            }
            let c = self.get(px as u32, py as u32).0;
            let a = c[3] as f32 / 255.0;
            acc[0] += c[0] as f32 * a * w;
            acc[1] += c[1] as f32 * a * w;
            acc[2] += c[2] as f32 * a * w;
            acc[3] += a * w;
        }
        if acc[3] > 1e-6 {
            [acc[0] / acc[3] / 255.0, acc[1] / acc[3] / 255.0, acc[2] / acc[3] / 255.0, acc[3]]
        } else {
            [0.0; 4]
        }
    }

    /// The pixels inside `r` (clipped to the image) as a new image of size `r.w × r.h`;
    /// areas outside the image are transparent.
    pub fn crop(&self, r: Rect) -> Image {
        let mut out = Image::new(r.w, r.h);
        let clip = r.intersect(&self.bounds());
        if clip.is_empty() {
            return out;
        }
        let dst = out.pixels_mut();
        for y in clip.y..clip.bottom() {
            let s = self.index(clip.x as u32, y as u32);
            let d = (((y - r.y) as usize) * r.w as usize + (clip.x - r.x) as usize) * 4;
            let n = clip.w as usize * 4;
            dst[d..d + n].copy_from_slice(&self.data[s..s + n]);
        }
        out
    }

    /// Copy `src` into this image with its top-left at (x, y), replacing pixels.
    pub fn blit(&mut self, src: &Image, x: i32, y: i32) {
        let target = Rect::new(x, y, src.width, src.height).intersect(&self.bounds());
        if target.is_empty() {
            return;
        }
        let w = self.width as usize;
        let dst = self.pixels_mut();
        for ty in target.y..target.bottom() {
            let s = src.index((target.x - x) as u32, (ty - y) as u32);
            let d = (ty as usize * w + target.x as usize) * 4;
            let n = target.w as usize * 4;
            dst[d..d + n].copy_from_slice(&src.data[s..s + n]);
        }
    }

    /// The tight bounds of pixels with non-zero alpha.
    pub fn opaque_bounds(&self) -> Option<Rect> {
        let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0u32, 0u32);
        for y in 0..self.height {
            for x in 0..self.width {
                if self.data[self.index(x, y) + 3] != 0 {
                    x0 = x0.min(x);
                    y0 = y0.min(y);
                    x1 = x1.max(x);
                    y1 = y1.max(y);
                }
            }
        }
        (x0 != u32::MAX).then(|| Rect::new(x0 as i32, y0 as i32, x1 - x0 + 1, y1 - y0 + 1))
    }

    pub fn alpha_channel(&self) -> Mask {
        let data = self.data.chunks_exact(4).map(|p| p[3]).collect();
        Mask::from_raw(self.width, self.height, data)
    }

    /// Fill with `c` wherever `mask` (if any) allows, blending by mask coverage.
    pub fn fill(&mut self, c: Rgba, mask: Option<&Mask>) {
        let src = c.to_f32();
        let px = self.pixels_mut();
        for (i, p) in px.chunks_exact_mut(4).enumerate() {
            let cov = mask.map_or(1.0, |m| m.data()[i] as f32 / 255.0);
            if cov <= 0.0 {
                continue;
            }
            let out = crate::blend::mix_pixel([p[0], p[1], p[2], p[3]], [src[0], src[1], src[2], src[3] * cov]);
            p.copy_from_slice(&out);
        }
    }

    /// Clear to transparent where `mask` (if any) allows.
    pub fn clear(&mut self, mask: Option<&Mask>) {
        let px = self.pixels_mut();
        for (i, p) in px.chunks_exact_mut(4).enumerate() {
            let cov = mask.map_or(255, |m| m.data()[i]) as u32;
            p[3] = ((p[3] as u32 * (255 - cov) + 127) / 255) as u8;
        }
    }
}

/// An 8-bit coverage buffer used for selections, layer masks and brush stamps.
#[derive(Clone, Debug)]
pub struct Mask {
    width: u32,
    height: u32,
    data: Arc<Vec<u8>>,
    generation: u64,
}

impl PartialEq for Mask {
    fn eq(&self, o: &Self) -> bool {
        self.width == o.width && self.height == o.height && self.data == o.data
    }
}

impl Mask {
    pub fn new(width: u32, height: u32) -> Self {
        Self::filled(width, height, 0)
    }

    pub fn filled(width: u32, height: u32, v: u8) -> Self {
        Self::from_raw(width, height, vec![v; width as usize * height as usize])
    }

    pub fn from_raw(width: u32, height: u32, data: Vec<u8>) -> Self {
        assert_eq!(data.len(), width as usize * height as usize, "mask buffer size");
        Self { width, height, data: Arc::new(data), generation: next_gen() }
    }

    pub fn width(&self) -> u32 {
        self.width
    }
    pub fn height(&self) -> u32 {
        self.height
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn data(&self) -> &[u8] {
        &self.data
    }
    pub fn data_mut(&mut self) -> &mut [u8] {
        self.generation = next_gen();
        Arc::make_mut(&mut self.data).as_mut_slice()
    }

    #[inline]
    pub fn get(&self, x: u32, y: u32) -> u8 {
        self.data[y as usize * self.width as usize + x as usize]
    }

    /// Value at (x, y), zero outside the mask.
    #[inline]
    pub fn get_or_zero(&self, x: i32, y: i32) -> u8 {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            0
        } else {
            self.get(x as u32, y as u32)
        }
    }

    pub fn set(&mut self, x: u32, y: u32, v: u8) {
        let w = self.width as usize;
        self.data_mut()[y as usize * w + x as usize] = v;
    }

    pub fn is_empty(&self) -> bool {
        self.data.iter().all(|&v| v == 0)
    }

    pub fn is_full(&self) -> bool {
        self.data.iter().all(|&v| v == 255)
    }

    pub fn invert(&mut self) {
        for v in self.data_mut() {
            *v = 255 - *v;
        }
    }

    /// Tight bounds of non-zero coverage.
    pub fn bounds(&self) -> Option<Rect> {
        let w = self.width as usize;
        let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0, 0);
        for (i, &v) in self.data.iter().enumerate() {
            if v != 0 {
                let (x, y) = (i % w, i / w);
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x);
                y1 = y1.max(y);
            }
        }
        (x0 != usize::MAX).then(|| Rect::new(x0 as i32, y0 as i32, (x1 - x0 + 1) as u32, (y1 - y0 + 1) as u32))
    }

    pub fn crop(&self, r: Rect) -> Mask {
        let mut out = Mask::new(r.w, r.h);
        let w = r.w as usize;
        let d = out.data_mut();
        for y in 0..r.h as i32 {
            for x in 0..r.w as i32 {
                d[y as usize * w + x as usize] = self.get_or_zero(r.x + x, r.y + y);
            }
        }
        out
    }

    /// The mask as an opaque greyscale image, for display and export.
    pub fn to_image(&self) -> Image {
        let mut v = Vec::with_capacity(self.data.len() * 4);
        for &m in self.data.iter() {
            v.extend_from_slice(&[m, m, m, 255]);
        }
        Image::from_raw(self.width, self.height, v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clone_is_copy_on_write() {
        let a = Image::filled(4, 4, Rgba::WHITE);
        let mut b = a.clone();
        b.put(0, 0, Rgba::BLACK);
        assert_eq!(a.get(0, 0), Rgba::WHITE);
        assert_eq!(b.get(0, 0), Rgba::BLACK);
        assert_ne!(a.generation(), b.generation());
    }

    #[test]
    fn crop_and_blit() {
        let mut a = Image::new(8, 8);
        a.put(3, 4, Rgba::rgb(1, 2, 3));
        let c = a.crop(Rect::new(2, 3, 4, 4));
        assert_eq!(c.get(1, 1), Rgba::rgb(1, 2, 3));
        let mut b = Image::new(8, 8);
        b.blit(&c, 2, 3);
        assert_eq!(b.get(3, 4), Rgba::rgb(1, 2, 3));
        assert_eq!(a.opaque_bounds(), Some(Rect::new(3, 4, 1, 1)));
    }

    #[test]
    fn mask_bounds_and_invert() {
        let mut m = Mask::new(10, 10);
        m.set(2, 3, 255);
        m.set(5, 7, 10);
        assert_eq!(m.bounds(), Some(Rect::new(2, 3, 4, 5)));
        m.invert();
        assert_eq!(m.get(0, 0), 255);
        assert_eq!(m.get(2, 3), 0);
    }
}
