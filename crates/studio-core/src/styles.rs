//! Layer styles: drop shadow, inner shadow, outer and inner glow, stroke, colour overlay.
//!
//! A styled layer composites as a stack of passes (effects below the content, the content,
//! then effects above it), each with its own blend mode and opacity.

use serde::{Deserialize, Serialize};

use crate::blend::BlendMode;
use crate::color::Rgba;
use crate::filters::{blur_mask, morph_mask, outline_mask, shift_mask};
use crate::gradient::{Gradient, GradientShape};
use crate::image::{Image, Mask};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Shadow {
    pub enabled: bool,
    pub color: Rgba,
    pub opacity: f32,
    pub angle: f32,
    pub distance: f32,
    pub size: f32,
    /// 0..1: how much of the size is solid before it blurs.
    pub spread: f32,
    pub blend: BlendMode,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Glow {
    pub enabled: bool,
    pub color: Rgba,
    pub opacity: f32,
    pub size: f32,
    pub spread: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum StrokePosition {
    #[default]
    Outside,
    Inside,
    Center,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StrokeStyle {
    pub enabled: bool,
    pub color: Rgba,
    pub opacity: f32,
    pub size: f32,
    pub position: StrokePosition,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ColorOverlay {
    pub enabled: bool,
    pub color: Rgba,
    pub opacity: f32,
    pub blend: BlendMode,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum BevelStyle {
    #[default]
    Inner,
    Outer,
    Emboss,
    Pillow,
}

impl BevelStyle {
    pub const ALL: [BevelStyle; 4] = [Self::Inner, Self::Outer, Self::Emboss, Self::Pillow];

    pub fn label(&self) -> &'static str {
        match self {
            Self::Inner => "Inner Bevel",
            Self::Outer => "Outer Bevel",
            Self::Emboss => "Emboss",
            Self::Pillow => "Pillow Emboss",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Bevel {
    pub enabled: bool,
    pub style: BevelStyle,
    /// 1..1000 percent.
    pub depth: f32,
    /// Raise (true) or sink (false).
    pub up: bool,
    pub size: f32,
    pub soften: f32,
    pub angle: f32,
    pub altitude: f32,
    pub highlight: Rgba,
    pub highlight_opacity: f32,
    pub shadow: Rgba,
    pub shadow_opacity: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Satin {
    pub enabled: bool,
    pub color: Rgba,
    pub opacity: f32,
    pub angle: f32,
    pub distance: f32,
    pub size: f32,
    pub invert: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GradientOverlay {
    pub enabled: bool,
    pub gradient: Gradient,
    pub shape: GradientShape,
    pub angle: f32,
    /// Percent of the layer's size the gradient spans.
    pub scale: f32,
    pub reverse: bool,
    pub opacity: f32,
    pub blend: BlendMode,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PatternOverlay {
    pub enabled: bool,
    /// Index into `paint::builtin_patterns`.
    pub pattern: usize,
    pub fg: Rgba,
    pub bg: Rgba,
    /// Percent.
    pub scale: f32,
    pub opacity: f32,
    pub blend: BlendMode,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LayerStyles {
    pub drop_shadow: Shadow,
    pub inner_shadow: Shadow,
    pub outer_glow: Glow,
    pub inner_glow: Glow,
    pub stroke: StrokeStyle,
    pub color_overlay: ColorOverlay,
    pub bevel: Bevel,
    pub satin: Satin,
    pub gradient_overlay: GradientOverlay,
    pub pattern_overlay: PatternOverlay,
}

impl Default for LayerStyles {
    fn default() -> Self {
        LayerStyles {
            drop_shadow: Shadow {
                enabled: false,
                color: Rgba::BLACK,
                opacity: 0.6,
                angle: 120.0,
                distance: 10.0,
                size: 14.0,
                spread: 0.0,
                blend: BlendMode::Multiply,
            },
            inner_shadow: Shadow {
                enabled: false,
                color: Rgba::BLACK,
                opacity: 0.6,
                angle: 120.0,
                distance: 5.0,
                size: 8.0,
                spread: 0.0,
                blend: BlendMode::Multiply,
            },
            outer_glow: Glow {
                enabled: false,
                color: Rgba::rgb(255, 240, 180),
                opacity: 0.75,
                size: 18.0,
                spread: 0.0,
            },
            inner_glow: Glow {
                enabled: false,
                color: Rgba::rgb(255, 240, 180),
                opacity: 0.75,
                size: 10.0,
                spread: 0.0,
            },
            stroke: StrokeStyle {
                enabled: false,
                color: Rgba::rgb(10, 132, 255),
                opacity: 1.0,
                size: 3.0,
                position: StrokePosition::Outside,
            },
            color_overlay: ColorOverlay {
                enabled: false,
                color: Rgba::rgb(255, 59, 48),
                opacity: 1.0,
                blend: BlendMode::Normal,
            },
            bevel: Bevel {
                enabled: false,
                style: BevelStyle::Inner,
                depth: 100.0,
                up: true,
                size: 6.0,
                soften: 0.0,
                angle: 120.0,
                altitude: 30.0,
                highlight: Rgba::WHITE,
                highlight_opacity: 0.75,
                shadow: Rgba::BLACK,
                shadow_opacity: 0.75,
            },
            satin: Satin {
                enabled: false,
                color: Rgba::BLACK,
                opacity: 0.5,
                angle: 19.0,
                distance: 11.0,
                size: 14.0,
                invert: true,
            },
            gradient_overlay: GradientOverlay {
                enabled: false,
                gradient: Gradient::two(Rgba::rgb(255, 55, 95), Rgba::rgb(10, 132, 255)),
                shape: GradientShape::Linear,
                angle: 90.0,
                scale: 100.0,
                reverse: false,
                opacity: 1.0,
                blend: BlendMode::Normal,
            },
            pattern_overlay: PatternOverlay {
                enabled: false,
                pattern: 0,
                fg: Rgba::rgb(40, 40, 40),
                bg: Rgba::rgb(220, 220, 220),
                scale: 100.0,
                opacity: 1.0,
                blend: BlendMode::Normal,
            },
        }
    }
}

impl LayerStyles {
    pub fn any(&self) -> bool {
        self.drop_shadow.enabled
            || self.inner_shadow.enabled
            || self.outer_glow.enabled
            || self.inner_glow.enabled
            || self.stroke.enabled
            || self.color_overlay.enabled
            || self.bevel.enabled
            || self.satin.enabled
            || self.gradient_overlay.enabled
            || self.pattern_overlay.enabled
    }

    pub fn disable_all(&mut self) {
        self.drop_shadow.enabled = false;
        self.inner_shadow.enabled = false;
        self.outer_glow.enabled = false;
        self.inner_glow.enabled = false;
        self.stroke.enabled = false;
        self.color_overlay.enabled = false;
        self.bevel.enabled = false;
        self.satin.enabled = false;
        self.gradient_overlay.enabled = false;
        self.pattern_overlay.enabled = false;
    }
}

/// One compositing pass of a styled layer.
#[derive(Clone, Debug)]
pub struct Pass {
    pub image: Image,
    pub blend: BlendMode,
    pub opacity: f32,
}

fn colorize(alpha: &Mask, c: Rgba) -> Image {
    let mut v = Vec::with_capacity(alpha.data().len() * 4);
    for &a in alpha.data() {
        v.extend_from_slice(&[c.r(), c.g(), c.b(), ((a as u32 * c.a() as u32 + 127) / 255) as u8]);
    }
    Image::from_raw(alpha.width(), alpha.height(), v)
}

fn mul_masks(a: &Mask, b: &Mask) -> Mask {
    let data = a.data().iter().zip(b.data()).map(|(&x, &y)| ((x as u32 * y as u32 + 127) / 255) as u8).collect();
    Mask::from_raw(a.width(), a.height(), data)
}

fn soft(alpha: &Mask, size: f32, spread: f32) -> Mask {
    let grow = size * spread.clamp(0.0, 1.0);
    let base = if grow >= 0.5 { morph_mask(alpha, grow) } else { alpha.clone() };
    blur_mask(&base, size * (1.0 - spread.clamp(0.0, 1.0)))
}

fn offset(angle: f32, distance: f32) -> (i32, i32) {
    let (s, c) = angle.to_radians().sin_cos();
    // Light comes *from* `angle`, so the shadow falls the other way.
    ((-c * distance).round() as i32, (s * distance).round() as i32)
}

/// Build the passes for `content` (layer pixels with the mask already applied).
pub fn render_passes(content: &Image, styles: &LayerStyles, blend: BlendMode) -> Vec<Pass> {
    let alpha = content.alpha_channel();
    let mut below = Vec::new();
    let mut above = Vec::new();

    let ds = &styles.drop_shadow;
    if ds.enabled {
        let (dx, dy) = offset(ds.angle, ds.distance);
        let m = soft(&shift_mask(&alpha, dx, dy), ds.size, ds.spread);
        below.push(Pass { image: colorize(&m, ds.color), blend: ds.blend, opacity: ds.opacity });
    }
    let og = &styles.outer_glow;
    if og.enabled {
        let m = soft(&alpha, og.size, og.spread);
        below.push(Pass { image: colorize(&m, og.color), blend: BlendMode::Screen, opacity: og.opacity });
    }

    let mut body = content.clone();
    // Overlays recolour the layer content itself, bottom to top: pattern, gradient, colour.
    let po = &styles.pattern_overlay;
    if po.enabled {
        let tiles = crate::paint::builtin_patterns(po.fg, po.bg);
        if let Some((_, tile)) = tiles.get(po.pattern).or(tiles.first()) {
            let k = (po.scale / 100.0).max(0.05);
            let (tw, th) = (tile.width() as f32 * k, tile.height() as f32 * k);
            let w = body.width();
            overlay_body(&mut body, po.blend, po.opacity, |x, y| {
                let tx = ((x as f32 / tw).fract() * tile.width() as f32) as u32 % tile.width().max(1);
                let ty = ((y as f32 / th).fract() * tile.height() as f32) as u32 % tile.height().max(1);
                let _ = w;
                tile.get(tx, ty).to_f32()
            });
        }
    }
    let go = &styles.gradient_overlay;
    if go.enabled {
        if let Some(b) = alpha.bounds() {
            let (cx, cy) = (b.x as f32 + b.w as f32 / 2.0, b.y as f32 + b.h as f32 / 2.0);
            let (s_, c_) = go.angle.to_radians().sin_cos();
            // Half the extent of the bounds along the gradient direction.
            let half = ((b.w as f32 * c_).abs() + (b.h as f32 * s_).abs()) / 2.0 * (go.scale / 100.0).max(0.05);
            let a = (cx - c_ * half, cy + s_ * half);
            let e = (cx + c_ * half, cy - s_ * half);
            let g = if go.reverse { go.gradient.reversed() } else { go.gradient.clone() };
            let shape = go.shape;
            let (ga, ge) = if matches!(shape, GradientShape::Radial | GradientShape::Diamond | GradientShape::Angle) {
                ((cx, cy), (cx + c_ * half, cy - s_ * half))
            } else {
                (a, e)
            };
            overlay_body(&mut body, go.blend, go.opacity, |x, y| {
                g.sample(shape.t_at(ga, ge, (x as f32 + 0.5, y as f32 + 0.5))).to_f32()
            });
        }
    }
    let co = &styles.color_overlay;
    if co.enabled {
        let c = co.color.to_f32();
        overlay_body(&mut body, co.blend, co.opacity, |_, _| c);
    }

    let sa = &styles.satin;
    if sa.enabled {
        // Two offset, blurred copies of the shape; their difference makes the silky band.
        let (dx, dy) = offset(sa.angle, sa.distance);
        let a1 = blur_mask(&shift_mask(&alpha, dx, dy), sa.size);
        let a2 = blur_mask(&shift_mask(&alpha, -dx, -dy), sa.size);
        let data: Vec<u8> = a1
            .data()
            .iter()
            .zip(a2.data())
            .zip(alpha.data())
            .map(|((&p, &q), &a)| {
                let d = (p as i32 - q as i32).unsigned_abs() as f32 / 255.0;
                let v = if sa.invert { 1.0 - d } else { d };
                (v * a as f32).round() as u8
            })
            .collect();
        let m = Mask::from_raw(alpha.width(), alpha.height(), data);
        above.push(Pass { image: colorize(&m, sa.color), blend: BlendMode::Multiply, opacity: sa.opacity });
    }

    let bv = &styles.bevel;
    if bv.enabled {
        let (hi, sh) = bevel_masks(&alpha, bv);
        let hp = Pass { image: colorize(&hi, bv.highlight), blend: BlendMode::Screen, opacity: bv.highlight_opacity };
        let sp = Pass { image: colorize(&sh, bv.shadow), blend: BlendMode::Multiply, opacity: bv.shadow_opacity };
        if bv.style == BevelStyle::Outer {
            below.push(sp);
            below.push(hp);
        } else {
            above.push(sp);
            above.push(hp);
        }
    }

    let is = &styles.inner_shadow;
    if is.enabled {
        let (dx, dy) = offset(is.angle, is.distance);
        let mut inv = alpha.clone();
        inv.invert();
        let m = soft(&shift_mask(&inv, dx, dy), is.size, is.spread);
        let m = mul_masks(&m, &alpha);
        above.push(Pass { image: colorize(&m, is.color), blend: is.blend, opacity: is.opacity });
    }
    let ig = &styles.inner_glow;
    if ig.enabled {
        let mut inv = alpha.clone();
        inv.invert();
        let m = mul_masks(&soft(&inv, ig.size, ig.spread), &alpha);
        above.push(Pass { image: colorize(&m, ig.color), blend: BlendMode::Screen, opacity: ig.opacity });
    }
    let st = &styles.stroke;
    if st.enabled {
        let m = match st.position {
            StrokePosition::Outside => outline_mask(&alpha, st.size, true, false),
            StrokePosition::Inside => outline_mask(&alpha, st.size, false, true),
            StrokePosition::Center => {
                let a = outline_mask(&alpha, st.size / 2.0, true, false);
                let b = outline_mask(&alpha, st.size / 2.0, false, true);
                let data = a.data().iter().zip(b.data()).map(|(&x, &y)| x.max(y)).collect();
                Mask::from_raw(alpha.width(), alpha.height(), data)
            }
        };
        above.push(Pass { image: colorize(&m, st.color), blend: BlendMode::Normal, opacity: st.opacity });
    }

    let mut passes = below;
    passes.push(Pass { image: body, blend, opacity: 1.0 });
    passes.extend(above);
    passes
}

/// Blend a per-pixel colour `f(x, y)` into the content, keeping its alpha.
fn overlay_body(body: &mut Image, mode: BlendMode, opacity: f32, f: impl Fn(u32, u32) -> [f32; 4]) {
    let w = body.width();
    for (i, p) in body.pixels_mut().chunks_exact_mut(4).enumerate() {
        if p[3] == 0 {
            continue;
        }
        let (x, y) = (i as u32 % w, i as u32 / w);
        let o = f(x, y);
        let b = [p[0] as f32 / 255.0, p[1] as f32 / 255.0, p[2] as f32 / 255.0];
        let m = mode.blend(b, [o[0], o[1], o[2]]);
        let k = opacity * o[3];
        for c in 0..3 {
            p[c] = crate::color::to_u8(b[c] + (m[c] - b[c]) * k);
        }
    }
}

/// Highlight and shadow coverage for Bevel & Emboss, from a height field built on the shape.
fn bevel_masks(alpha: &Mask, bv: &Bevel) -> (Mask, Mask) {
    let (w, h) = (alpha.width() as usize, alpha.height() as usize);
    let size = bv.size.max(1.0);
    let solid: Vec<bool> = alpha.data().iter().map(|&a| a >= 128).collect();
    let empty: Vec<bool> = solid.iter().map(|s| !s).collect();
    let d_in = crate::filters::distance_field(w, h, &empty);
    let d_out = crate::filters::distance_field(w, h, &solid);
    // Height ramps up over `size` pixels from the edge; where it ramps depends on the style.
    let height: Vec<u8> = (0..w * h)
        .map(|i| {
            let t = match bv.style {
                BevelStyle::Inner => {
                    if solid[i] {
                        (d_in[i] / size).min(1.0)
                    } else {
                        0.0
                    }
                }
                BevelStyle::Outer => {
                    if solid[i] {
                        1.0
                    } else {
                        (1.0 - d_out[i] / size).max(0.0)
                    }
                }
                BevelStyle::Emboss => {
                    if solid[i] {
                        0.5 + 0.5 * (d_in[i] / size).min(1.0)
                    } else {
                        0.5 * (1.0 - d_out[i] / size).max(0.0)
                    }
                }
                BevelStyle::Pillow => {
                    if solid[i] {
                        (d_in[i] / size).min(1.0)
                    } else {
                        (1.0 - d_out[i] / size).max(0.0) * 0.6
                    }
                }
            };
            let t = if bv.style == BevelStyle::Pillow && !solid[i] { -t } else { t };
            (((t * 0.5 + 0.5) * 255.0).clamp(0.0, 255.0)) as u8
        })
        .collect();
    let hm = blur_mask(&Mask::from_raw(alpha.width(), alpha.height(), height), 1.0 + bv.soften);
    let hd = hm.data();
    let k = bv.depth / 100.0 * if bv.up { 1.0 } else { -1.0 } * 4.0;
    let (sa, ca) = bv.angle.to_radians().sin_cos();
    let alt = bv.altitude.to_radians();
    let light = (ca * alt.cos(), -sa * alt.cos(), alt.sin());
    let mut hi = vec![0u8; w * h];
    let mut sh = vec![0u8; w * h];
    let get = |x: isize, y: isize| {
        hd[(y.clamp(0, h as isize - 1) as usize) * w + x.clamp(0, w as isize - 1) as usize] as f32 / 255.0
    };
    for y in 0..h {
        for x in 0..w {
            let (xi, yi) = (x as isize, y as isize);
            let gx = (get(xi + 1, yi) - get(xi - 1, yi)) * k;
            let gy = (get(xi, yi + 1) - get(xi, yi - 1)) * k;
            let n = (-gx, -gy, 1.0f32);
            let len = (n.0 * n.0 + n.1 * n.1 + n.2 * n.2).sqrt();
            let lit = (n.0 * light.0 + n.1 * light.1 + n.2 * light.2) / len;
            // Flat areas get `alt.sin()`; only the deviation from that shades.
            let v = lit - alt.sin();
            let i = y * w + x;
            let region = match bv.style {
                BevelStyle::Inner => alpha.data()[i] as f32 / 255.0,
                BevelStyle::Outer => 1.0 - alpha.data()[i] as f32 / 255.0,
                _ => 1.0,
            };
            hi[i] = ((v.max(0.0) * 2.5).min(1.0) * region * 255.0) as u8;
            sh[i] = (((-v).max(0.0) * 2.5).min(1.0) * region * 255.0) as u8;
        }
    }
    (Mask::from_raw(alpha.width(), alpha.height(), hi), Mask::from_raw(alpha.width(), alpha.height(), sh))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drop_shadow_falls_opposite_the_light() {
        let mut img = Image::new(40, 40);
        for y in 15..25 {
            for x in 15..25 {
                img.put(x, y, Rgba::WHITE);
            }
        }
        let mut s = LayerStyles::default();
        s.drop_shadow.enabled = true;
        s.drop_shadow.angle = 90.0; // light from the top → shadow below
        s.drop_shadow.distance = 6.0;
        s.drop_shadow.size = 0.0;
        let passes = render_passes(&img, &s, BlendMode::Normal);
        assert_eq!(passes.len(), 2);
        let sh = &passes[0].image;
        assert!(sh.get(20, 29).a() > 200, "shadow below");
        assert_eq!(sh.get(20, 12).a(), 0, "nothing above");
    }

    fn square() -> Image {
        let mut img = Image::new(40, 40);
        for y in 10..30 {
            for x in 10..30 {
                img.put(x, y, Rgba::rgb(128, 128, 128));
            }
        }
        img
    }

    #[test]
    fn bevel_lights_top_left_edge() {
        let mut s = LayerStyles::default();
        s.bevel.enabled = true;
        s.bevel.angle = 135.0;
        s.bevel.size = 5.0;
        let passes = render_passes(&square(), &s, BlendMode::Normal);
        let (shadow, highlight) = (&passes[passes.len() - 2].image, &passes[passes.len() - 1].image);
        assert!(highlight.get(11, 11).a() > shadow.get(11, 11).a(), "top-left is lit");
        assert!(shadow.get(28, 28).a() > highlight.get(28, 28).a(), "bottom-right is in shadow");
        assert_eq!(highlight.get(20, 20).a(), 0, "flat top is untouched");
    }

    #[test]
    fn overlays_recolour_content() {
        let mut s = LayerStyles::default();
        s.gradient_overlay.enabled = true;
        s.gradient_overlay.gradient = Gradient::two(Rgba::BLACK, Rgba::WHITE);
        s.gradient_overlay.angle = 0.0;
        let passes = render_passes(&square(), &s, BlendMode::Normal);
        let body = &passes[0].image;
        assert!(
            body.get(11, 20).r() < 40 && body.get(28, 20).r() > 215,
            "{:?} {:?}",
            body.get(11, 20),
            body.get(28, 20)
        );
        assert_eq!(body.get(5, 5).a(), 0);
        let mut s = LayerStyles::default();
        s.pattern_overlay.enabled = true;
        let body = render_passes(&square(), &s, BlendMode::Normal)[0].image.clone();
        assert_ne!(body.get(12, 12), body.get(20, 12), "checker alternates");
    }

    #[test]
    fn outside_stroke_surrounds_shape() {
        let mut img = Image::new(30, 30);
        for y in 10..20 {
            for x in 10..20 {
                img.put(x, y, Rgba::WHITE);
            }
        }
        let mut s = LayerStyles::default();
        s.stroke.enabled = true;
        s.stroke.size = 3.0;
        let passes = render_passes(&img, &s, BlendMode::Normal);
        let stroke = &passes.last().unwrap().image;
        assert!(stroke.get(8, 15).a() > 200);
        assert_eq!(stroke.get(15, 15).a(), 0);
        assert_eq!(stroke.get(3, 15).a(), 0);
    }
}
