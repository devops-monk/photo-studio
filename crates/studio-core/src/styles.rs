//! Layer styles: drop shadow, inner shadow, outer and inner glow, stroke, colour overlay.
//!
//! A styled layer composites as a stack of passes (effects below the content, the content,
//! then effects above it), each with its own blend mode and opacity.

use serde::{Deserialize, Serialize};

use crate::blend::BlendMode;
use crate::color::Rgba;
use crate::filters::{blur_mask, morph_mask, outline_mask, shift_mask};
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LayerStyles {
    pub drop_shadow: Shadow,
    pub inner_shadow: Shadow,
    pub outer_glow: Glow,
    pub inner_glow: Glow,
    pub stroke: StrokeStyle,
    pub color_overlay: ColorOverlay,
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
    }

    pub fn disable_all(&mut self) {
        self.drop_shadow.enabled = false;
        self.inner_shadow.enabled = false;
        self.outer_glow.enabled = false;
        self.inner_glow.enabled = false;
        self.stroke.enabled = false;
        self.color_overlay.enabled = false;
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
    let co = &styles.color_overlay;
    if co.enabled {
        let overlay = colorize(&alpha, co.color);
        let px = body.pixels_mut();
        for (p, o) in px.chunks_exact_mut(4).zip(overlay.pixels().chunks_exact(4)) {
            let b = [p[0] as f32 / 255.0, p[1] as f32 / 255.0, p[2] as f32 / 255.0];
            let s = [o[0] as f32 / 255.0, o[1] as f32 / 255.0, o[2] as f32 / 255.0];
            let m = co.blend.blend(b, s);
            for c in 0..3 {
                p[c] = crate::color::to_u8(b[c] + (m[c] - b[c]) * co.opacity);
            }
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
