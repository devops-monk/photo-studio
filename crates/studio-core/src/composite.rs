//! Flattening a layer stack into one image.

use std::collections::HashMap;
use std::sync::Arc;

use rayon::prelude::*;

use crate::blend::{BlendMode, composite_pixel, pixel_noise};
use crate::geom::Rect;
use crate::image::Image;
use crate::layer::{Layer, LayerId, LayerKind};
use crate::styles::{LayerStyles, Pass, render_passes};

#[derive(Clone, PartialEq)]
struct StyleKey {
    pixels: u64,
    mask: Option<u64>,
    styles: LayerStyles,
    blend: BlendMode,
}

/// Composites layers, caching expensive layer-style renders between calls.
#[derive(Default)]
pub struct Compositor {
    cache: HashMap<LayerId, (StyleKey, Arc<Vec<Pass>>)>,
}

impl Compositor {
    pub fn new() -> Self {
        Self::default()
    }

    fn passes(&mut self, layer: &Layer) -> Option<Arc<Vec<Pass>>> {
        if !layer.styles.any() || layer.pixels.width() == 0 {
            return None;
        }
        let key = StyleKey {
            pixels: layer.pixels.generation(),
            mask: layer.active_mask().map(|m| m.generation()),
            styles: layer.styles.clone(),
            blend: layer.blend,
        };
        if let Some((k, p)) = self.cache.get(&layer.id) {
            if *k == key {
                return Some(p.clone());
            }
        }
        let content = match layer.active_mask() {
            Some(m) => {
                let mut c = layer.pixels.clone();
                let md = m.data();
                for (i, p) in c.pixels_mut().chunks_exact_mut(4).enumerate() {
                    p[3] = ((p[3] as u32 * md[i] as u32 + 127) / 255) as u8;
                }
                c
            }
            None => layer.pixels.clone(),
        };
        let passes = Arc::new(render_passes(&content, &layer.styles, layer.blend));
        self.cache.insert(layer.id, (key, passes.clone()));
        Some(passes)
    }

    /// Drop cache entries for layers that no longer exist.
    pub fn prune(&mut self, layers: &[Layer]) {
        self.cache.retain(|id, _| layers.iter().any(|l| l.id == *id));
    }

    /// Composite the whole canvas.
    pub fn composite(&mut self, layers: &[Layer], w: u32, h: u32) -> Image {
        self.composite_region(layers, w, h, Rect::sized(w, h))
    }

    /// Composite only `region` of a `w × h` canvas; the result is `region`-sized.
    pub fn composite_region(&mut self, layers: &[Layer], w: u32, h: u32, region: Rect) -> Image {
        let region = region.intersect(&Rect::sized(w, h));
        let mut out = Image::new(region.w, region.h);
        if region.is_empty() {
            return out;
        }
        let rw = region.w as usize;
        let cw = w as usize;
        let mut clip_base: Option<usize> = None;
        for (li, layer) in layers.iter().enumerate() {
            if !layer.clip {
                clip_base = Some(li);
            }
            let base = if layer.clip { clip_base.filter(|&b| b != li).map(|b| &layers[b]) } else { None };
            if !layer.visible || layer.opacity <= 0.0 || base.is_some_and(|b| !b.visible) {
                continue;
            }
            let opacity = layer.opacity;
            let clip_alpha = |x: u32, y: u32| base.map_or(1.0, |b| b.content_alpha(x, y) as f32 / 255.0);

            if let LayerKind::Adjustment(adj) = &layer.kind {
                let prep = adj.prepare();
                let mask = layer.active_mask();
                out.pixels_mut().par_chunks_mut(rw * 4).enumerate().for_each(|(ry, row)| {
                    let y = (region.y as usize + ry) as u32;
                    for (rx, p) in row.chunks_exact_mut(4).enumerate() {
                        let x = (region.x as usize + rx) as u32;
                        let mut cov = opacity * clip_alpha(x, y);
                        if let Some(m) = mask {
                            cov *= m.get(x, y) as f32 / 255.0;
                        }
                        if cov <= 0.0 || p[3] == 0 {
                            continue;
                        }
                        let orig = [p[0], p[1], p[2], p[3]];
                        let adj = prep.apply(orig);
                        for c in 0..3 {
                            p[c] = (orig[c] as f32 + (adj[c] as f32 - orig[c] as f32) * cov).round() as u8;
                        }
                    }
                });
                continue;
            }

            if layer.pixels.size() != (w, h) {
                continue;
            }
            let styled = self.passes(layer);
            let single;
            let passes: &[Pass] = match &styled {
                Some(p) => p,
                None => {
                    single = [Pass { image: layer.pixels.clone(), blend: layer.blend, opacity: 1.0 }];
                    &single
                }
            };
            let mask = if styled.is_some() { None } else { layer.active_mask() };
            for pass in passes {
                let src = pass.image.pixels();
                let mode = pass.blend;
                let k = pass.opacity * opacity;
                out.pixels_mut().par_chunks_mut(rw * 4).enumerate().for_each(|(ry, row)| {
                    let y = region.y as usize + ry;
                    for (rx, p) in row.chunks_exact_mut(4).enumerate() {
                        let x = region.x as usize + rx;
                        let i = y * cw + x;
                        let s = &src[i * 4..i * 4 + 4];
                        if s[3] == 0 {
                            continue;
                        }
                        let mut a = s[3] as f32 / 255.0 * k;
                        if let Some(m) = mask {
                            a *= m.data()[i] as f32 / 255.0;
                        }
                        if base.is_some() {
                            a *= clip_alpha(x as u32, y as u32);
                        }
                        if mode == BlendMode::Dissolve {
                            a = if pixel_noise(x as u32, y as u32) < a { 1.0 } else { 0.0 };
                        }
                        if a <= 0.0 {
                            continue;
                        }
                        let rgb = [s[0] as f32 / 255.0, s[1] as f32 / 255.0, s[2] as f32 / 255.0, a];
                        let r = composite_pixel([p[0], p[1], p[2], p[3]], rgb, mode);
                        p.copy_from_slice(&r);
                    }
                });
            }
        }
        out
    }
}

/// One-shot composite without a persistent cache.
pub fn flatten(layers: &[Layer], w: u32, h: u32) -> Image {
    Compositor::new().composite(layers, w, h)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adjust::Adjustment;
    use crate::color::Rgba;
    use crate::image::Mask;

    #[test]
    fn stacks_with_opacity_and_mask() {
        let bottom = Layer::raster(1, "bg", Image::filled(2, 1, Rgba::BLACK));
        let mut top = Layer::raster(2, "top", Image::filled(2, 1, Rgba::WHITE));
        top.opacity = 0.5;
        top.mask = Some(Mask::from_raw(2, 1, vec![255, 0]));
        let out = flatten(&[bottom, top], 2, 1);
        assert_eq!(out.get(0, 0), Rgba::rgb(128, 128, 128));
        assert_eq!(out.get(1, 0), Rgba::BLACK);
    }

    #[test]
    fn adjustment_layer_affects_below_only() {
        let bottom = Layer::raster(1, "bg", Image::filled(1, 1, Rgba::rgb(10, 20, 30)));
        let adj = Layer::with_kind(2, "inv", LayerKind::Adjustment(Adjustment::Invert), 1, 1);
        let out = flatten(&[bottom, adj], 1, 1);
        assert_eq!(out.get(0, 0), Rgba::rgb(245, 235, 225));
    }

    #[test]
    fn clipping_mask_uses_base_alpha() {
        let mut base_px = Image::new(2, 1);
        base_px.put(0, 0, Rgba::BLACK);
        let base = Layer::raster(1, "base", base_px);
        let mut clipped = Layer::raster(2, "c", Image::filled(2, 1, Rgba::WHITE));
        clipped.clip = true;
        let out = flatten(&[base, clipped], 2, 1);
        assert_eq!(out.get(0, 0), Rgba::WHITE);
        assert_eq!(out.get(1, 0).a(), 0);
    }

    #[test]
    fn region_matches_full_composite() {
        let mut img = Image::new(8, 8);
        for y in 0..8 {
            for x in 0..8 {
                img.put(x, y, Rgba::rgba((x * 30) as u8, (y * 30) as u8, 90, 200));
            }
        }
        let layers = vec![Layer::raster(1, "a", Image::filled(8, 8, Rgba::WHITE)), Layer::raster(2, "b", img)];
        let full = flatten(&layers, 8, 8);
        let part = Compositor::new().composite_region(&layers, 8, 8, Rect::new(2, 3, 4, 4));
        assert_eq!(part, full.crop(Rect::new(2, 3, 4, 4)));
    }
}
