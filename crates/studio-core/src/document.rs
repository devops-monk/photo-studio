//! The document: canvas size, layer stack and selection, plus the edits that span layers.

use crate::color::Rgba;
use crate::composite::flatten;
use crate::geom::{Affine, Rect};
use crate::image::{Image, Mask};
use crate::layer::{FillLayer, Layer, LayerId, LayerKind};
use crate::transform::{self, Resample, Rotation};

#[derive(Clone, Debug)]
pub struct Document {
    pub width: u32,
    pub height: u32,
    /// Bottom to top.
    pub layers: Vec<Layer>,
    pub active: usize,
    pub selection: Option<Mask>,
    pub next_id: LayerId,
    /// Pixels per inch, for print sizes.
    pub ppi: f32,
}

/// Everything history needs to restore.
#[derive(Clone, Debug)]
pub struct DocState {
    width: u32,
    height: u32,
    layers: Vec<Layer>,
    active: usize,
    selection: Option<Mask>,
}

/// Where existing pixels sit when the canvas grows or shrinks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Anchor {
    TopLeft,
    Top,
    TopRight,
    Left,
    #[default]
    Center,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
}

impl Anchor {
    pub const GRID: [Anchor; 9] = [
        Self::TopLeft,
        Self::Top,
        Self::TopRight,
        Self::Left,
        Self::Center,
        Self::Right,
        Self::BottomLeft,
        Self::Bottom,
        Self::BottomRight,
    ];

    fn factors(&self) -> (f32, f32) {
        let i = Self::GRID.iter().position(|a| a == self).unwrap();
        ((i % 3) as f32 / 2.0, (i / 3) as f32 / 2.0)
    }
}

enum Params {
    Offset(f32, f32),
    Scale(f32, f32),
    Rasterize,
}

impl Document {
    /// A new document with a "Background" layer filled with `bg`, or a transparent "Layer 1".
    pub fn new(width: u32, height: u32, bg: Option<Rgba>) -> Self {
        let mut doc = Document { width, height, layers: Vec::new(), active: 0, selection: None, next_id: 1, ppi: 72.0 };
        let (name, img) = match bg {
            Some(c) => ("Background", Image::filled(width, height, c)),
            None => ("Layer 1", Image::new(width, height)),
        };
        let id = doc.new_id();
        doc.layers.push(Layer::raster(id, name, img));
        doc
    }

    pub fn from_image(img: Image) -> Self {
        let mut doc = Document {
            width: img.width(),
            height: img.height(),
            layers: Vec::new(),
            active: 0,
            selection: None,
            next_id: 1,
            ppi: 72.0,
        };
        let id = doc.new_id();
        doc.layers.push(Layer::raster(id, "Background", img));
        doc
    }

    pub fn new_id(&mut self) -> LayerId {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub fn bounds(&self) -> Rect {
        Rect::sized(self.width, self.height)
    }

    pub fn state(&self) -> DocState {
        DocState {
            width: self.width,
            height: self.height,
            layers: self.layers.clone(),
            active: self.active,
            selection: self.selection.clone(),
        }
    }

    pub fn restore(&mut self, s: &DocState) {
        self.width = s.width;
        self.height = s.height;
        self.layers = s.layers.clone();
        self.active = s.active.min(self.layers.len().saturating_sub(1));
        self.selection = s.selection.clone();
        if let Some(max) = self.layers.iter().map(|l| l.id).max() {
            self.next_id = self.next_id.max(max + 1);
        }
    }

    pub fn active_layer(&self) -> Option<&Layer> {
        self.layers.get(self.active)
    }

    pub fn active_layer_mut(&mut self) -> Option<&mut Layer> {
        self.layers.get_mut(self.active)
    }

    pub fn index_of(&self, id: LayerId) -> Option<usize> {
        self.layers.iter().position(|l| l.id == id)
    }

    pub fn composite(&self) -> Image {
        flatten(&self.layers, self.width, self.height)
    }

    /// The selection, or `None` when nothing (or everything) is selected.
    pub fn selection_mask(&self) -> Option<&Mask> {
        self.selection.as_ref()
    }

    pub fn unique_name(&self, base: &str) -> String {
        let mut n = self.layers.len() + 1;
        loop {
            let name = format!("{base} {n}");
            if !self.layers.iter().any(|l| l.name == name) {
                return name;
            }
            n += 1;
        }
    }

    /// Insert above the active layer and make it active.
    pub fn insert_layer(&mut self, layer: Layer) -> usize {
        let at = if self.layers.is_empty() { 0 } else { self.active + 1 };
        self.layers.insert(at, layer);
        self.active = at;
        at
    }

    pub fn add_empty_layer(&mut self, name: &str) -> usize {
        let id = self.new_id();
        let layer = Layer::raster(id, name, Image::new(self.width, self.height));
        self.insert_layer(layer)
    }

    pub fn add_layer_of_kind(&mut self, name: &str, kind: LayerKind) -> usize {
        let id = self.new_id();
        let mut layer = Layer::with_kind(id, name, kind, self.width, self.height);
        // New adjustment and fill layers take the selection as their mask.
        if let Some(sel) = &self.selection {
            if matches!(layer.kind, LayerKind::Adjustment(_) | LayerKind::Fill(_)) {
                layer.mask = Some(sel.clone());
            }
        }
        self.insert_layer(layer)
    }

    pub fn duplicate_layer(&mut self, index: usize) -> Option<usize> {
        let mut l = self.layers.get(index)?.clone();
        l.id = self.new_id();
        l.name = format!("{} copy", l.name);
        self.layers.insert(index + 1, l);
        self.active = index + 1;
        Some(index + 1)
    }

    pub fn delete_layer(&mut self, index: usize) -> bool {
        if self.layers.len() <= 1 || index >= self.layers.len() {
            return false;
        }
        self.layers.remove(index);
        // Deleting the active layer selects the one below it, like other editors.
        if self.active >= index {
            self.active = self.active.saturating_sub(1);
        }
        self.active = self.active.min(self.layers.len() - 1);
        true
    }

    pub fn move_layer(&mut self, from: usize, to: usize) {
        if from >= self.layers.len() || to >= self.layers.len() || from == to {
            return;
        }
        let l = self.layers.remove(from);
        self.layers.insert(to, l);
        self.active = to;
    }

    /// Merge the layer at `index` into the one below it.
    pub fn merge_down(&mut self, index: usize) -> bool {
        if index == 0 || index >= self.layers.len() {
            return false;
        }
        let mut lower = self.layers[index - 1].clone();
        let upper = self.layers[index].clone();
        // The lower layer keeps its own opacity and blend; merge the pair as seen through it.
        let mut lower_flat = lower.clone();
        lower_flat.opacity = 1.0;
        lower_flat.blend = crate::blend::BlendMode::Normal;
        lower_flat.styles.disable_all();
        let merged = flatten(&[flatten_layer(&lower_flat, self.width, self.height), upper], self.width, self.height);
        lower.pixels = merged;
        lower.kind = LayerKind::Raster;
        lower.mask = None;
        lower.styles.disable_all();
        self.layers[index - 1] = lower;
        self.layers.remove(index);
        self.active = index - 1;
        true
    }

    /// Replace all visible layers with one merged layer.
    pub fn merge_visible(&mut self) {
        let visible: Vec<Layer> = self.layers.iter().filter(|l| l.visible).cloned().collect();
        if visible.len() < 2 {
            return;
        }
        let merged = flatten(&visible, self.width, self.height);
        let first = self.layers.iter().position(|l| l.visible).unwrap();
        let id = self.new_id();
        let name = self.layers[first].name.clone();
        self.layers.retain(|l| !l.visible);
        let at = first.min(self.layers.len());
        self.layers.insert(at, Layer::raster(id, name, merged));
        self.active = at;
    }

    pub fn flatten_image(&mut self) {
        let mut merged = self.composite();
        // A flattened image has an opaque white backdrop, like a printed page.
        let white = Image::filled(self.width, self.height, Rgba::WHITE);
        let mut base = white;
        crate::paint::composite_onto(&mut base, &merged, crate::blend::BlendMode::Normal, 1.0, None);
        merged = base;
        let id = self.new_id();
        self.layers = vec![Layer::raster(id, "Background", merged)];
        self.active = 0;
    }

    /// Copy (or cut) the selected pixels of the active layer into a new layer.
    pub fn layer_via(&mut self, cut: bool) -> bool {
        let Some(layer) = self.active_layer() else { return false };
        if !matches!(layer.kind, LayerKind::Raster) {
            return false;
        }
        let mut pixels = layer.pixels.clone();
        if let Some(sel) = &self.selection {
            let d = sel.data();
            for (i, p) in pixels.pixels_mut().chunks_exact_mut(4).enumerate() {
                p[3] = ((p[3] as u32 * d[i] as u32 + 127) / 255) as u8;
            }
        }
        let name = format!("{} {}", layer.name, if cut { "cut" } else { "copy" });
        if cut {
            let sel = self.selection.clone();
            if let Some(l) = self.active_layer_mut() {
                l.pixels.clear(sel.as_ref());
            }
        }
        let id = self.new_id();
        self.insert_layer(Layer::raster(id, name, pixels));
        true
    }

    // ---- selection ----

    pub fn select_all(&mut self) {
        self.selection = Some(Mask::filled(self.width, self.height, 255));
    }

    pub fn deselect(&mut self) {
        self.selection = None;
    }

    pub fn invert_selection(&mut self) {
        match &mut self.selection {
            Some(m) => m.invert(),
            None => self.select_all(),
        }
        if self.selection.as_ref().is_some_and(|m| m.is_empty()) {
            self.selection = None;
        }
    }

    pub fn set_selection(&mut self, m: Mask) {
        self.selection = if m.is_empty() { None } else { Some(m) };
    }

    /// Select the opaque pixels of a layer.
    pub fn select_layer_pixels(&mut self, index: usize) {
        if let Some(l) = self.layers.get(index) {
            if l.pixels.width() != 0 {
                let m = l.pixels.alpha_channel();
                self.set_selection(m);
            }
        }
    }

    // ---- masks ----

    pub fn add_mask(&mut self, reveal: bool) {
        let sel = self.selection.clone();
        let (w, h) = (self.width, self.height);
        if let Some(l) = self.active_layer_mut() {
            l.mask = Some(sel.unwrap_or_else(|| Mask::filled(w, h, if reveal { 255 } else { 0 })));
            l.mask_enabled = true;
        }
    }

    pub fn apply_mask(&mut self) {
        if let Some(l) = self.active_layer_mut() {
            if let Some(m) = l.mask.take() {
                l.rasterize();
                let d = m.data();
                for (i, p) in l.pixels.pixels_mut().chunks_exact_mut(4).enumerate() {
                    p[3] = ((p[3] as u32 * d[i] as u32 + 127) / 255) as u8;
                }
            }
        }
    }

    // ---- geometry ----

    fn transform_all(
        &mut self,
        nw: u32,
        nh: u32,
        img: impl Fn(&Image) -> Image,
        mask: impl Fn(&Mask) -> Mask,
        params: Params,
    ) {
        for l in &mut self.layers {
            if l.kind.is_generated() {
                match params {
                    Params::Rasterize => l.rasterize(),
                    _ => scale_params(&mut l.kind, &params),
                }
            }
            if l.kind.is_generated() {
                l.render(nw, nh);
            } else if l.pixels.width() != 0 {
                l.pixels = img(&l.pixels);
            }
            if let Some(m) = &l.mask {
                l.mask = Some(mask(m));
            }
        }
        self.selection = self.selection.as_ref().map(&mask).filter(|m| !m.is_empty());
        self.width = nw;
        self.height = nh;
    }

    pub fn resize_image(&mut self, nw: u32, nh: u32, method: Resample) {
        let (sx, sy) = (nw as f32 / self.width as f32, nh as f32 / self.height as f32);
        self.transform_all(
            nw,
            nh,
            |i| transform::resize(i, nw, nh, method),
            |m| transform::resize_mask(m, nw, nh, method),
            Params::Scale(sx, sy),
        );
    }

    pub fn resize_canvas(&mut self, nw: u32, nh: u32, anchor: Anchor, fill: Option<Rgba>) {
        let (fx, fy) = anchor.factors();
        let dx = ((nw as f32 - self.width as f32) * fx).round() as i32;
        let dy = ((nh as f32 - self.height as f32) * fy).round() as i32;
        let bottom_id = self.layers.first().map(|l| l.id);
        self.transform_all(
            nw,
            nh,
            |i| {
                let mut out = Image::new(nw, nh);
                out.blit(i, dx, dy);
                out
            },
            |m| {
                let mut out = Mask::new(nw, nh);
                let d = out.data_mut();
                for y in 0..nh as i32 {
                    for x in 0..nw as i32 {
                        d[(y as u32 * nw + x as u32) as usize] = m.get_or_zero(x - dx, y - dy);
                    }
                }
                out
            },
            Params::Offset(dx as f32, dy as f32),
        );
        // New canvas area of the background takes the fill colour.
        if let (Some(c), Some(id)) = (fill, bottom_id) {
            if let Some(l) = self.layers.iter_mut().find(|l| l.id == id && matches!(l.kind, LayerKind::Raster)) {
                let mut bg = Image::filled(nw, nh, c);
                crate::paint::composite_onto(&mut bg, &l.pixels, crate::blend::BlendMode::Normal, 1.0, None);
                l.pixels = bg;
            }
        }
    }

    pub fn crop(&mut self, r: Rect) {
        let r = r.intersect(&self.bounds());
        if r.is_empty() {
            return;
        }
        self.transform_all(r.w, r.h, |i| i.crop(r), |m| m.crop(r), Params::Offset(-r.x as f32, -r.y as f32));
    }

    pub fn crop_to_selection(&mut self) {
        if let Some(b) = self.selection.as_ref().and_then(|m| m.bounds()) {
            self.crop(b);
            self.selection = None;
        }
    }

    /// Crop away transparent borders (all layers considered).
    pub fn trim(&mut self) {
        let comp = self.composite();
        if let Some(b) = comp.opaque_bounds() {
            self.crop(b);
        }
    }

    pub fn rotate(&mut self, r: Rotation) {
        let (nw, nh) = if r == Rotation::Half { (self.width, self.height) } else { (self.height, self.width) };
        self.transform_all(nw, nh, |i| transform::rotate(i, r), |m| transform::rotate_mask(m, r), Params::Rasterize);
    }

    pub fn flip(&mut self, horizontal: bool) {
        let (w, h) = (self.width, self.height);
        self.transform_all(w, h, |i| transform::flip(i, horizontal), |m| transform::flip_mask(m, horizontal), Params::Rasterize);
    }

    pub fn rotate_arbitrary(&mut self, deg: f32) {
        let (nw, nh, t) = transform::rotation_fit(self.width, self.height, deg);
        self.transform_all(nw, nh, |i| transform::affine(i, &t, nw, nh), |m| transform::affine_mask(m, &t, nw, nh), Params::Rasterize);
    }

    /// Transform the active layer's pixels (Free Transform), limited to the selection if any.
    pub fn transform_active(&mut self, t: &Affine) {
        let (w, h) = (self.width, self.height);
        let sel = self.selection.clone();
        let Some(l) = self.active_layer_mut() else { return };
        if l.is_adjustment() {
            return;
        }
        l.rasterize();
        match &sel {
            Some(m) => {
                let mut moving = l.pixels.clone();
                let d = m.data();
                for (i, p) in moving.pixels_mut().chunks_exact_mut(4).enumerate() {
                    p[3] = ((p[3] as u32 * d[i] as u32 + 127) / 255) as u8;
                }
                l.pixels.clear(Some(m));
                let moved = transform::affine(&moving, t, w, h);
                crate::paint::composite_onto(&mut l.pixels, &moved, crate::blend::BlendMode::Normal, 1.0, None);
            }
            None => l.pixels = transform::affine(&l.pixels, t, w, h),
        }
        if let Some(s) = &mut self.selection {
            *s = transform::affine_mask(s, t, w, h);
        }
    }

    /// Move the active layer (and selection) by whole pixels.
    pub fn nudge_active(&mut self, dx: i32, dy: i32) {
        if dx == 0 && dy == 0 {
            return;
        }
        let sel = self.selection.clone();
        let (w, h) = (self.width, self.height);
        let Some(l) = self.active_layer_mut() else { return };
        match &mut l.kind {
            LayerKind::Text(t) => {
                t.x += dx as f32;
                t.y += dy as f32;
                l.render(w, h);
            }
            LayerKind::Shape(s) => {
                s.from = (s.from.0 + dx as f32, s.from.1 + dy as f32);
                s.to = (s.to.0 + dx as f32, s.to.1 + dy as f32);
                l.render(w, h);
            }
            LayerKind::Raster => {
                if let Some(m) = &sel {
                    let mut moving = l.pixels.clone();
                    let d = m.data();
                    for (i, p) in moving.pixels_mut().chunks_exact_mut(4).enumerate() {
                        p[3] = ((p[3] as u32 * d[i] as u32 + 127) / 255) as u8;
                    }
                    l.pixels.clear(Some(m));
                    let mut shifted = Image::new(w, h);
                    shifted.blit(&moving, dx, dy);
                    crate::paint::composite_onto(&mut l.pixels, &shifted, crate::blend::BlendMode::Normal, 1.0, None);
                } else {
                    let mut out = Image::new(w, h);
                    out.blit(&l.pixels, dx, dy);
                    l.pixels = out;
                }
            }
            _ => {}
        }
        if let Some(m) = &mut l.mask {
            if sel.is_none() {
                *m = crate::filters::shift_mask(m, dx, dy);
            }
        }
        if let Some(s) = &mut self.selection {
            *s = crate::filters::shift_mask(s, dx, dy);
        }
    }
}

fn flatten_layer(l: &Layer, w: u32, h: u32) -> Layer {
    let mut out = l.clone();
    out.pixels = flatten(std::slice::from_ref(l), w, h);
    out.mask = None;
    out.kind = LayerKind::Raster;
    out
}

fn scale_params(kind: &mut LayerKind, p: &Params) {
    let map = |pt: (f32, f32)| match p {
        Params::Offset(dx, dy) => (pt.0 + dx, pt.1 + dy),
        Params::Scale(sx, sy) => (pt.0 * sx, pt.1 * sy),
        Params::Rasterize => pt,
    };
    let k = match p {
        Params::Scale(sx, sy) => (sx + sy) / 2.0,
        _ => 1.0,
    };
    match kind {
        LayerKind::Text(t) => {
            (t.x, t.y) = map((t.x, t.y));
            t.size *= k;
            if let Some(bw) = &mut t.box_width {
                *bw *= k;
            }
        }
        LayerKind::Shape(s) => {
            s.from = map(s.from);
            s.to = map(s.to);
            s.stroke_width *= k;
            if let crate::shape::ShapeKind::RoundedRect { radius } = &mut s.kind {
                *radius *= k;
            }
        }
        LayerKind::Fill(FillLayer::Gradient { start, end, .. }) => {
            *start = map(*start);
            *end = map(*end);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::TextLayer;

    #[test]
    fn layer_management() {
        let mut d = Document::new(10, 10, Some(Rgba::WHITE));
        d.add_empty_layer("A");
        d.add_empty_layer("B");
        assert_eq!(d.active, 2);
        d.duplicate_layer(2);
        assert_eq!(d.layers[3].name, "B copy");
        d.move_layer(3, 1);
        assert_eq!(d.layers[1].name, "B copy");
        assert!(d.delete_layer(1));
        assert_eq!(d.layers.len(), 3);
    }

    #[test]
    fn merge_down_combines_pixels() {
        let mut d = Document::new(2, 1, Some(Rgba::BLACK));
        d.add_empty_layer("top");
        d.layers[1].pixels.put(0, 0, Rgba::WHITE);
        assert!(d.merge_down(1));
        assert_eq!(d.layers.len(), 1);
        assert_eq!(d.layers[0].pixels.get(0, 0), Rgba::WHITE);
        assert_eq!(d.layers[0].pixels.get(1, 0), Rgba::BLACK);
    }

    #[test]
    fn canvas_resize_with_anchor() {
        let mut d = Document::new(2, 2, Some(Rgba::BLACK));
        d.resize_canvas(4, 4, Anchor::Center, Some(Rgba::WHITE));
        assert_eq!((d.width, d.height), (4, 4));
        assert_eq!(d.layers[0].pixels.get(0, 0), Rgba::WHITE);
        assert_eq!(d.layers[0].pixels.get(1, 1), Rgba::BLACK);
    }

    #[test]
    fn resize_scales_text_layers() {
        let mut d = Document::new(100, 100, Some(Rgba::WHITE));
        d.add_layer_of_kind("T", LayerKind::Text(TextLayer::new("Hi", 10.0, 10.0, 20.0, Rgba::BLACK)));
        d.resize_image(200, 200, Resample::Bilinear);
        match &d.layers[1].kind {
            LayerKind::Text(t) => {
                assert_eq!((t.x, t.y, t.size), (20.0, 20.0, 40.0));
            }
            _ => panic!("still text"),
        }
        assert_eq!(d.layers[1].pixels.size(), (200, 200));
    }

    #[test]
    fn crop_and_rotate() {
        let mut d = Document::new(10, 6, Some(Rgba::WHITE));
        d.crop(Rect::new(2, 1, 4, 3));
        assert_eq!((d.width, d.height), (4, 3));
        d.rotate(Rotation::Cw90);
        assert_eq!((d.width, d.height), (3, 4));
        d.rotate_arbitrary(45.0);
        assert!(d.width > 4);
    }

    #[test]
    fn layer_via_copy_respects_selection() {
        let mut d = Document::new(4, 4, Some(Rgba::BLACK));
        d.set_selection(crate::select::rect_mask(4, 4, Rect::new(0, 0, 2, 2)));
        d.layer_via(false);
        assert_eq!(d.layers.len(), 2);
        assert_eq!(d.layers[1].pixels.get(0, 0).a(), 255);
        assert_eq!(d.layers[1].pixels.get(3, 3).a(), 0);
    }
}
