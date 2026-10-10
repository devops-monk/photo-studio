//! Per-channel histograms and their statistics.

use crate::color::luma;
use crate::image::{Image, Mask};

#[derive(Clone, Debug, PartialEq)]
pub struct Histogram {
    pub red: [u64; 256],
    pub green: [u64; 256],
    pub blue: [u64; 256],
    pub luma: [u64; 256],
}

impl Histogram {
    /// Counts pixels with non-zero alpha (and non-zero mask coverage when given).
    pub fn of(img: &Image, mask: Option<&Mask>) -> Self {
        let mut h = Histogram { red: [0; 256], green: [0; 256], blue: [0; 256], luma: [0; 256] };
        for (i, p) in img.pixels().chunks_exact(4).enumerate() {
            if p[3] == 0 || mask.is_some_and(|m| m.data()[i] == 0) {
                continue;
            }
            h.red[p[0] as usize] += 1;
            h.green[p[1] as usize] += 1;
            h.blue[p[2] as usize] += 1;
            let l = luma(p[0] as f32, p[1] as f32, p[2] as f32).round() as usize;
            h.luma[l.min(255)] += 1;
        }
        h
    }

    /// 0 = red, 1 = green, 2 = blue, 3 = luminosity.
    pub fn channel(&self, c: usize) -> &[u64; 256] {
        match c {
            0 => &self.red,
            1 => &self.green,
            2 => &self.blue,
            _ => &self.luma,
        }
    }

    pub fn count(&self) -> u64 {
        self.luma.iter().sum()
    }

    pub fn mean(&self, c: usize) -> f32 {
        let n = self.count();
        if n == 0 {
            return 0.0;
        }
        let s: u64 = self.channel(c).iter().enumerate().map(|(i, &v)| i as u64 * v).sum();
        s as f32 / n as f32
    }

    pub fn std_dev(&self, c: usize) -> f32 {
        let n = self.count();
        if n == 0 {
            return 0.0;
        }
        let m = self.mean(c);
        let s: f32 = self.channel(c).iter().enumerate().map(|(i, &v)| (i as f32 - m).powi(2) * v as f32).sum();
        (s / n as f32).sqrt()
    }

    pub fn median(&self, c: usize) -> u8 {
        let half = self.count().div_ceil(2);
        let mut acc = 0;
        for (i, &v) in self.channel(c).iter().enumerate() {
            acc += v;
            if acc >= half && acc > 0 {
                return i as u8;
            }
        }
        0
    }

    /// Lowest and highest levels after discarding `clip` (fraction) of pixels at each end.
    pub fn clip_points(&self, c: usize, clip: f32) -> (u8, u8) {
        let ch = self.channel(c);
        let n = self.count();
        if n == 0 {
            return (0, 255);
        }
        let limit = (n as f32 * clip) as u64;
        let mut acc = 0;
        let mut lo = 0;
        for (i, &v) in ch.iter().enumerate() {
            acc += v;
            if acc > limit {
                lo = i;
                break;
            }
        }
        acc = 0;
        let mut hi = 255;
        for (i, &v) in ch.iter().enumerate().rev() {
            acc += v;
            if acc > limit {
                hi = i;
                break;
            }
        }
        if hi <= lo { (0, 255) } else { (lo as u8, hi as u8) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Rgba;

    #[test]
    fn statistics() {
        let mut img = Image::filled(2, 2, Rgba::rgb(0, 0, 0));
        img.put(1, 1, Rgba::rgb(200, 200, 200));
        let h = Histogram::of(&img, None);
        assert_eq!(h.count(), 4);
        assert_eq!(h.mean(0), 50.0);
        assert_eq!(h.median(0), 0);
        assert_eq!(h.clip_points(0, 0.0), (0, 200));
    }
}
