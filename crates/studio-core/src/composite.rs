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
        let tree = Tree::new(layers);
        let ctx = DrawCtx { layers, w, h, region };
        self.draw_list(&ctx, &tree, &tree.top, &mut out, 1.0, None);
        out
    }

    /// Draw the layers at `list` (bottom to top) into `out`.
    /// `k` multiplies opacity and `outer` masks coverage (pass-through group ancestors).
    fn draw_list(&mut self, c: &DrawCtx, tree: &Tree, list: &[usize], out: &mut Image, k: f32, outer: Option<&Outer>) {
        let mut clip_base: Option<usize> = None;
        for &li in list {
            let layer = &c.layers[li];
            if !layer.clip {
                clip_base = Some(li);
            }
            let base = if layer.clip { clip_base.filter(|&b| b != li).map(|b| &c.layers[b]) } else { None };
            if !layer.visible || layer.opacity <= 0.0 || base.is_some_and(|b| !b.visible) {
                continue;
            }
            if let LayerKind::Group { pass_through, .. } = layer.kind {
                let children = tree.children.get(&layer.id).map(Vec::as_slice).unwrap_or(&[]);
                let group_outer = Outer { parent: outer, mask: layer.active_mask().cloned(), w: c.w };
                if pass_through && layer.blend == BlendMode::Normal {
                    self.draw_list(c, tree, children, out, k * layer.opacity, Some(&group_outer));
                } else {
                    let mut tmp = Image::new(c.region.w, c.region.h);
                    self.draw_list(c, tree, children, &mut tmp, 1.0, None);
                    let pass = RegionPass { image: &tmp, blend: layer.blend, opacity: k * layer.opacity };
                    blend_region(out, &pass, c, layer.active_mask(), outer, base);
                }
                continue;
            }
            self.draw_layer(c, layer, base, out, k, outer);
        }
    }

    fn draw_layer(
        &mut self,
        c: &DrawCtx,
        layer: &Layer,
        base: Option<&Layer>,
        out: &mut Image,
        k: f32,
        outer: Option<&Outer>,
    ) {
        let (w, region) = (c.w as usize, c.region);
        let rw = region.w as usize;
        let opacity = layer.opacity * k;
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
                    if let Some(o) = outer {
                        cov *= o.coverage(x, y);
                    }
                    if cov <= 0.0 || p[3] == 0 {
                        continue;
                    }
                    let orig = [p[0], p[1], p[2], p[3]];
                    let adj = prep.apply(orig);
                    for ch in 0..3 {
                        p[ch] = (orig[ch] as f32 + (adj[ch] as f32 - orig[ch] as f32) * cov).round() as u8;
                    }
                }
            });
            return;
        }

        if layer.pixels.size() != (c.w, c.h) {
            return;
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
            let kk = pass.opacity * opacity;
            out.pixels_mut().par_chunks_mut(rw * 4).enumerate().for_each(|(ry, row)| {
                let y = region.y as usize + ry;
                for (rx, p) in row.chunks_exact_mut(4).enumerate() {
                    let x = region.x as usize + rx;
                    let i = y * w + x;
                    let s = &src[i * 4..i * 4 + 4];
                    if s[3] == 0 {
                        continue;
                    }
                    let mut a = s[3] as f32 / 255.0 * kk;
                    if let Some(m) = mask {
                        a *= m.data()[i] as f32 / 255.0;
                    }
                    if base.is_some() {
                        a *= clip_alpha(x as u32, y as u32);
                    }
                    if let Some(o) = outer {
                        a *= o.coverage(x as u32, y as u32);
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
}

struct DrawCtx<'a> {
    layers: &'a [Layer],
    w: u32,
    h: u32,
    region: Rect,
}

/// Masks of enclosing pass-through groups.
struct Outer<'a> {
    parent: Option<&'a Outer<'a>>,
    mask: Option<crate::image::Mask>,
    w: u32,
}

impl Outer<'_> {
    fn coverage(&self, x: u32, y: u32) -> f32 {
        let own = self.mask.as_ref().map_or(1.0, |m| m.data()[(y * self.w + x) as usize] as f32 / 255.0);
        own * self.parent.map_or(1.0, |p| p.coverage(x, y))
    }
}

/// A region-sized image composited as one layer (an isolated group).
struct RegionPass<'a> {
    image: &'a Image,
    blend: BlendMode,
    opacity: f32,
}

fn blend_region(
    out: &mut Image,
    pass: &RegionPass,
    c: &DrawCtx,
    mask: Option<&crate::image::Mask>,
    outer: Option<&Outer>,
    base: Option<&Layer>,
) {
    let region = c.region;
    let rw = region.w as usize;
    let src = pass.image.pixels();
    out.pixels_mut().par_chunks_mut(rw * 4).enumerate().for_each(|(ry, row)| {
        let y = (region.y as usize + ry) as u32;
        for (rx, p) in row.chunks_exact_mut(4).enumerate() {
            let x = (region.x as usize + rx) as u32;
            let s = &src[(ry * rw + rx) * 4..(ry * rw + rx) * 4 + 4];
            if s[3] == 0 {
                continue;
            }
            let mut a = s[3] as f32 / 255.0 * pass.opacity;
            if let Some(m) = mask {
                a *= m.get(x, y) as f32 / 255.0;
            }
            if let Some(o) = outer {
                a *= o.coverage(x, y);
            }
            if let Some(b) = base {
                a *= b.content_alpha(x, y) as f32 / 255.0;
            }
            if a <= 0.0 {
                continue;
            }
            let r = composite_pixel(
                [p[0], p[1], p[2], p[3]],
                [s[0] as f32 / 255.0, s[1] as f32 / 255.0, s[2] as f32 / 255.0, a],
                pass.blend,
            );
            p.copy_from_slice(&r);
        }
    });
}

/// Layers arranged by group: top-level indices and each group's children, bottom to top.
pub struct Tree {
    pub top: Vec<usize>,
    pub children: HashMap<LayerId, Vec<usize>>,
}

impl Tree {
    pub fn new(layers: &[Layer]) -> Self {
        let ids: std::collections::HashSet<LayerId> = layers.iter().filter(|l| l.is_group()).map(|l| l.id).collect();
        let mut top = Vec::new();
        let mut children: HashMap<LayerId, Vec<usize>> = HashMap::new();
        for (i, l) in layers.iter().enumerate() {
            match l.parent.filter(|p| ids.contains(p)) {
                Some(p) => children.entry(p).or_default().push(i),
                None => top.push(i),
            }
        }
        Tree { top, children }
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

    fn group(id: u64, pass_through: bool) -> Layer {
        Layer::with_kind(id, "g", LayerKind::Group { pass_through, collapsed: false }, 2, 1)
    }

    #[test]
    fn hidden_group_hides_children() {
        let bg = Layer::raster(1, "bg", Image::filled(2, 1, Rgba::BLACK));
        let mut child = Layer::raster(2, "c", Image::filled(2, 1, Rgba::WHITE));
        child.parent = Some(3);
        let mut g = group(3, true);
        g.visible = false;
        assert_eq!(flatten(&[bg, child, g], 2, 1).get(0, 0), Rgba::BLACK);
    }

    #[test]
    fn group_opacity_applies_to_children() {
        for pass_through in [true, false] {
            let bg = Layer::raster(1, "bg", Image::filled(2, 1, Rgba::BLACK));
            let mut child = Layer::raster(2, "c", Image::filled(2, 1, Rgba::WHITE));
            child.parent = Some(3);
            let mut g = group(3, pass_through);
            g.opacity = 0.5;
            let out = flatten(&[bg, child, g], 2, 1);
            assert_eq!(out.get(0, 0), Rgba::rgb(128, 128, 128), "pass_through={pass_through}");
        }
    }

    #[test]
    fn isolated_group_keeps_adjustments_inside() {
        let bg = Layer::raster(1, "bg", Image::filled(1, 1, Rgba::rgb(10, 10, 10)));
        let mut child = Layer::raster(2, "c", Image::new(1, 1));
        child.parent = Some(4);
        let mut inv = Layer::with_kind(3, "inv", LayerKind::Adjustment(Adjustment::Invert), 1, 1);
        inv.parent = Some(4);
        let iso = Layer::with_kind(4, "g", LayerKind::Group { pass_through: false, collapsed: false }, 1, 1);
        // The invert only sees the (empty) group contents, so the background is untouched.
        assert_eq!(flatten(&[bg.clone(), child.clone(), inv.clone(), iso], 1, 1).get(0, 0), Rgba::rgb(10, 10, 10));
        let pt = Layer::with_kind(4, "g", LayerKind::Group { pass_through: true, collapsed: false }, 1, 1);
        assert_eq!(flatten(&[bg, child, inv, pt], 1, 1).get(0, 0), Rgba::rgb(245, 245, 245));
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
