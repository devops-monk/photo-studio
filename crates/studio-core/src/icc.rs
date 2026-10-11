//! Colour management. The working space is sRGB: files with other embedded ICC profiles are
//! converted on open, CMYK and Lab sources are converted to RGB, exports carry an sRGB profile,
//! and soft proofing previews how an image will reproduce through an output profile.

use std::sync::Arc;

use moxcms::{ColorProfile, DataColorSpace, Layout, RenderingIntent, TransformExecutor, TransformOptions};
use rayon::prelude::*;

use crate::image::Image;

/// The sRGB profile, encoded for embedding in exported files.
pub fn srgb_profile_bytes() -> Vec<u8> {
    ColorProfile::new_srgb().encode().unwrap_or_default()
}

/// A grayscale (gamma 2.2) output profile, for proofing black-and-white reproduction.
pub fn gray_profile_bytes() -> Vec<u8> {
    let mut p = ColorProfile::new_gray_with_gamma(2.2);
    p.color_space = DataColorSpace::Gray;
    p.encode().unwrap_or_default()
}

type Exec = Arc<dyn TransformExecutor<u8> + Send + Sync>;

fn options(intent: RenderingIntent) -> TransformOptions {
    TransformOptions { rendering_intent: intent, ..TransformOptions::default() }
}

/// Run a 4-channel transform over every pixel in parallel, keeping alpha.
fn apply(exec: &Exec, px: &mut [u8], keep_alpha: bool) {
    px.par_chunks_mut(4 * 4096).for_each(|chunk| {
        let alpha: Vec<u8> = chunk.chunks_exact(4).map(|p| p[3]).collect();
        let src = chunk.to_vec();
        if exec.transform(&src, chunk).is_ok() && keep_alpha {
            for (p, a) in chunk.chunks_exact_mut(4).zip(alpha) {
                p[3] = a;
            }
        }
    });
}

/// Whether a transform leaves colours unchanged (the source is effectively sRGB already).
fn is_identity(exec: &Exec) -> bool {
    let mut probe = Vec::new();
    for v in (0..=255u8).step_by(15) {
        probe.extend_from_slice(&[v, v, v, 255, v, 0, 0, 255, 0, v, 0, 255, 0, 0, v, 255]);
    }
    let mut out = vec![0u8; probe.len()];
    exec.transform(&probe, &mut out).is_ok() && probe.iter().zip(&out).all(|(a, b)| a.abs_diff(*b) <= 1)
}

/// Convert an image tagged with `icc` into sRGB in place. Returns whether anything changed.
pub fn convert_to_srgb(img: &mut Image, icc: &[u8]) -> bool {
    let Ok(src) = ColorProfile::new_from_slice(icc) else { return false };
    if src.color_space != DataColorSpace::Rgb {
        return false;
    }
    let dst = ColorProfile::new_srgb();
    let Ok(exec) = src.create_transform_8bit(Layout::Rgba, &dst, Layout::Rgba, options(RenderingIntent::Perceptual))
    else {
        return false;
    };
    let exec: Exec = exec;
    if is_identity(&exec) {
        return false;
    }
    apply(&exec, img.pixels_mut(), true);
    true
}

/// CMYK ink amounts (0 = none, 255 = full) to RGBA, through `icc` when given.
pub fn cmyk_to_rgba(cmyk: &[[u8; 4]], alpha: &[u8], icc: Option<&[u8]>) -> Vec<u8> {
    let mut out = vec![0u8; cmyk.len() * 4];
    let exec = icc
        .and_then(|b| ColorProfile::new_from_slice(b).ok())
        .filter(|p| p.color_space == DataColorSpace::Cmyk)
        .and_then(|p| {
            p.create_transform_8bit(
                Layout::Rgba,
                &ColorProfile::new_srgb(),
                Layout::Rgba,
                options(RenderingIntent::Perceptual),
            )
            .ok()
        });
    match exec {
        Some(exec) => {
            let src: Vec<u8> = cmyk.iter().flatten().copied().collect();
            let _ = exec.transform(&src, &mut out);
        }
        None => {
            // No profile: the naive device conversion.
            for (o, c) in out.chunks_exact_mut(4).zip(cmyk) {
                let k = 255 - c[3] as u32;
                for i in 0..3 {
                    o[i] = ((255 - c[i] as u32) * k / 255) as u8;
                }
            }
        }
    }
    for (o, &a) in out.chunks_exact_mut(4).zip(alpha.iter().chain(std::iter::repeat(&255))) {
        o[3] = a;
    }
    out
}

/// CIE L*a*b* (D50) to 8-bit sRGB.
pub fn lab_to_srgb(l: f32, a: f32, b: f32) -> [u8; 3] {
    let fy = (l + 16.0) / 116.0;
    let fx = fy + a / 500.0;
    let fz = fy - b / 200.0;
    let inv = |t: f32| if t > 6.0 / 29.0 { t * t * t } else { 3.0 * (6.0f32 / 29.0).powi(2) * (t - 4.0 / 29.0) };
    // D50 white, then Bradford-adapted to D65 sRGB.
    let (x, y, z) = (0.9642 * inv(fx), inv(fy), 0.8251 * inv(fz));
    let r = 3.133_856 * x - 1.616_867 * y - 0.490_614_6 * z;
    let g = -0.978_768_4 * x + 1.916_141_5 * y + 0.033_454 * z;
    let bl = 0.071_945_3 * x - 0.228_991_4 * y + 1.405_242_7 * z;
    let enc = |c: f32| {
        let c = c.clamp(0.0, 1.0);
        let v = if c <= 0.0031308 { 12.92 * c } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
        (v * 255.0).round() as u8
    };
    [enc(r), enc(g), enc(bl)]
}

/// A soft proof: simulates reproducing sRGB through an output profile (a printer's CMYK, say).
pub struct Proof {
    to_device: Exec,
    from_device: Exec,
    /// Gamut check: round trip with absolute colorimetric to see what the device can't hold.
    device_layout: Layout,
    pub name: String,
}

impl Proof {
    pub fn new(icc: &[u8]) -> Result<Proof, String> {
        let dev = ColorProfile::new_from_slice(icc).map_err(|e| format!("not a usable ICC profile: {e:?}"))?;
        let device_layout = match dev.color_space {
            DataColorSpace::Rgb | DataColorSpace::Cmyk => Layout::Rgba,
            DataColorSpace::Gray => Layout::GrayAlpha,
            other => return Err(format!("{other:?} profiles can't be used for proofing")),
        };
        let srgb = ColorProfile::new_srgb();
        let intent = RenderingIntent::RelativeColorimetric;
        let to_device: Exec = srgb
            .create_transform_8bit(Layout::Rgba, &dev, device_layout, options(intent))
            .map_err(|e| format!("{e:?}"))?;
        let from_device: Exec = dev
            .create_transform_8bit(device_layout, &srgb, Layout::Rgba, options(intent))
            .map_err(|e| format!("{e:?}"))?;
        let name = match &dev.description {
            Some(moxcms::ProfileText::PlainString(t)) => t.clone(),
            Some(moxcms::ProfileText::Localizable(v)) => {
                v.iter().find(|l| l.language == "en").or(v.first()).map(|l| l.value.clone()).unwrap_or_default()
            }
            Some(moxcms::ProfileText::Description(d)) => d.ascii_string.clone(),
            None => String::new(),
        };
        let name = name.trim_matches(char::from(0)).trim().to_string();
        let name = if name.is_empty() { format!("{:?} profile", dev.color_space) } else { name };
        Ok(Proof { to_device, from_device, device_layout, name })
    }

    /// The image as it would look reproduced on the device. With `gamut_warning`, colours the
    /// device can't reach (they shift by more than a small tolerance) are painted grey.
    pub fn apply(&self, img: &Image, gamut_warning: bool) -> Image {
        let mut out = img.clone();
        let ch = if self.device_layout == Layout::GrayAlpha { 2 } else { 4 };
        out.pixels_mut().par_chunks_mut(4 * 4096).for_each(|chunk| {
            let n = chunk.len() / 4;
            let mut dev = vec![0u8; n * ch];
            if self.to_device.transform(chunk, &mut dev).is_err() {
                return;
            }
            let mut back = vec![0u8; n * 4];
            if self.from_device.transform(&dev, &mut back).is_err() {
                return;
            }
            for (i, p) in chunk.chunks_exact_mut(4).enumerate() {
                let q = &back[i * 4..i * 4 + 3];
                let shift = (0..3).map(|c| (p[c] as i32 - q[c] as i32).abs()).max().unwrap_or(0);
                if gamut_warning && shift > 12 {
                    p[..3].copy_from_slice(&[128, 128, 128]);
                } else {
                    p[..3].copy_from_slice(q);
                }
            }
        });
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Rgba;

    #[test]
    fn srgb_profile_is_a_no_op() {
        let mut img = Image::filled(4, 4, Rgba::rgb(200, 30, 90));
        assert!(!convert_to_srgb(&mut img, &srgb_profile_bytes()));
        assert_eq!(img.get(1, 1), Rgba::rgb(200, 30, 90));
    }

    #[test]
    fn wide_gamut_converts_into_srgb() {
        // Pure Display P3 green is more saturated than sRGB can show: it clips to sRGB green.
        let p3 = ColorProfile::new_display_p3().encode().unwrap();
        let mut img = Image::filled(2, 2, Rgba::rgba(128, 128, 128, 77));
        img.put(0, 0, Rgba::rgb(0, 255, 0));
        assert!(convert_to_srgb(&mut img, &p3));
        let g = img.get(0, 0);
        assert!(g.g() > 240 && g.r() < 40, "{g:?}");
        // Neutral grey stays neutral, alpha is kept.
        let n = img.get(1, 1);
        assert!(n.r().abs_diff(n.g()) <= 1 && n.g().abs_diff(n.b()) <= 1 && n.a() == 77, "{n:?}");
    }

    #[test]
    fn naive_cmyk_and_lab() {
        let rgba = cmyk_to_rgba(&[[0, 0, 0, 0], [0, 255, 255, 0], [0, 0, 0, 255]], &[255, 255, 10], None);
        assert_eq!(&rgba[0..4], &[255, 255, 255, 255]);
        assert_eq!(&rgba[4..8], &[255, 0, 0, 255]);
        assert_eq!(&rgba[8..12], &[0, 0, 0, 10]);
        assert_eq!(lab_to_srgb(100.0, 0.0, 0.0), [255, 255, 255]);
        assert_eq!(lab_to_srgb(0.0, 0.0, 0.0), [0, 0, 0]);
        let red = lab_to_srgb(54.3, 80.8, 69.9);
        assert!(red[0] > 240 && red[1] < 30 && red[2] < 30, "{red:?}");
    }

    #[test]
    fn proof_through_srgb_is_identity_and_gray_profile_desaturates() {
        let img = Image::filled(3, 3, Rgba::rgb(220, 40, 60));
        let p = Proof::new(&srgb_profile_bytes()).unwrap();
        let out = p.apply(&img, false);
        let c = out.get(1, 1);
        assert!(c.r().abs_diff(220) <= 2 && c.g().abs_diff(40) <= 2, "{c:?}");
        let gp = Proof::new(&gray_profile_bytes()).unwrap();
        let g = gp.apply(&img, false).get(1, 1);
        assert!(g.r().abs_diff(g.g()) <= 2 && g.g().abs_diff(g.b()) <= 2, "grey proof: {g:?}");
        let warn = gp.apply(&img, true).get(1, 1);
        assert_eq!(warn, Rgba::rgb(128, 128, 128), "saturated red is out of a grey device's gamut");
    }
}
