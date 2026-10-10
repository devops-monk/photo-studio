//! The document: canvas size, layer stack and selection, plus the edits that span layers.

use crate::color::Rgba;
use crate::composite::flatten;
use crate::geom::{Affine, Rect};
use crate::image::{Image, Mask};
use crate::layer::{FillLayer, Layer, LayerId, LayerKind};
use crate::transform::{self, Resample, Rotation};

/// A ruler guide: a horizontal or vertical line at `pos` canvas pixels.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Guide {
    pub horizontal: bool,
    pub pos: f32,
}

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
    pub guides: Vec<Guide>,
    pub paths: Vec<crate::path::NamedPath>,
    /// Saved selections (alpha channels).
    pub channels: Vec<(String, Mask)>,
}

/// Everything history needs to restore.
#[derive(Clone, Debug)]
pub struct DocState {
    width: u32,
    height: u32,
    layers: Vec<Layer>,
    active: usize,
    selection: Option<Mask>,
    guides: Vec<Guide>,
    paths: Vec<crate::path::NamedPath>,
    channels: Vec<(String, Mask)>,
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

impl DocState {
    /// A layer as it was in this state.
    pub fn layer(&self, id: LayerId) -> Option<&Layer> {
        self.layers.iter().find(|l| l.id == id)
    }

    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }
}

impl Document {
    /// A new document with a "Background" layer filled with `bg`, or a transparent "Layer 1".
    pub fn new(width: u32, height: u32, bg: Option<Rgba>) -> Self {
        let mut doc = Document {
            width,
            height,
            layers: Vec::new(),
            active: 0,
            selection: None,
            next_id: 1,
            ppi: 72.0,
            guides: Vec::new(),
            paths: Vec::new(),
            channels: Vec::new(),
        };
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
            guides: Vec::new(),
            paths: Vec::new(),
            channels: Vec::new(),
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
            guides: self.guides.clone(),
            paths: self.paths.clone(),
            channels: self.channels.clone(),
        }
    }

    pub fn restore(&mut self, s: &DocState) {
        self.width = s.width;
        self.height = s.height;
        self.layers = s.layers.clone();
        self.active = s.active.min(self.layers.len().saturating_sub(1));
        self.selection = s.selection.clone();
        self.guides = s.guides.clone();
        self.paths = s.paths.clone();
        self.channels = s.channels.clone();
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
    /// Insert above the active layer (inside it, if it is a group) and make it active.
    pub fn insert_layer(&mut self, mut layer: Layer) -> usize {
        let at = match self.layers.get(self.active) {
            None => 0,
            Some(a) if a.is_group() => {
                layer.parent = Some(a.id);
                self.active
            }
            Some(a) => {
                layer.parent = a.parent;
                self.active + 1
            }
        };
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

    /// Whether `index` is inside the group `group_id` at any depth.
    pub fn is_descendant(&self, index: usize, group_id: LayerId) -> bool {
        let mut p = self.layers.get(index).and_then(|l| l.parent);
        let mut guard = 0;
        while let Some(id) = p {
            if id == group_id {
                return true;
            }
            p = self.layers.iter().find(|l| l.id == id).and_then(|l| l.parent);
            guard += 1;
            if guard > 64 {
                break;
            }
        }
        false
    }

    /// Nesting depth (0 for top-level layers).
    pub fn depth(&self, index: usize) -> usize {
        let mut d = 0;
        let mut p = self.layers.get(index).and_then(|l| l.parent);
        while let Some(id) = p {
            d += 1;
            p = self.layers.iter().find(|l| l.id == id).and_then(|l| l.parent);
            if d > 64 {
                break;
            }
        }
        d
    }

    /// Visible, and every enclosing group visible.
    pub fn effectively_visible(&self, index: usize) -> bool {
        let Some(l) = self.layers.get(index) else { return false };
        l.visible
            && match l.parent.and_then(|p| self.index_of(p)) {
                Some(pi) => self.effectively_visible(pi),
                None => true,
            }
    }

    /// Index range of a layer together with everything inside it (a group's block).
    pub fn block(&self, index: usize) -> std::ops::Range<usize> {
        let Some(l) = self.layers.get(index) else { return index..index };
        if !l.is_group() {
            return index..index + 1;
        }
        let id = l.id;
        let mut start = index;
        while start > 0 && self.is_descendant(start - 1, id) {
            start -= 1;
        }
        start..index + 1
    }

    pub fn duplicate_layer(&mut self, index: usize) -> Option<usize> {
        let range = self.block(index);
        if range.is_empty() {
            return None;
        }
        let mut copies: Vec<Layer> = self.layers[range.clone()].to_vec();
        let mut remap = std::collections::HashMap::new();
        for l in &mut copies {
            let new = self.new_id();
            remap.insert(l.id, new);
            l.id = new;
        }
        for l in &mut copies {
            if let Some(p) = l.parent.and_then(|p| remap.get(&p)) {
                l.parent = Some(*p);
            }
        }
        if let Some(top) = copies.last_mut() {
            top.name = format!("{} copy", top.name);
        }
        let at = range.end;
        let n = copies.len();
        for (k, l) in copies.into_iter().enumerate() {
            self.layers.insert(at + k, l);
        }
        self.active = at + n - 1;
        Some(self.active)
    }

    pub fn delete_layer(&mut self, index: usize) -> bool {
        let range = self.block(index);
        if range.is_empty() || self.layers.len() <= range.len() {
            return false;
        }
        let n = range.len();
        self.layers.drain(range.clone());
        // Deleting the active layer selects the one below it, like other editors.
        if self.active >= range.start {
            self.active = if self.active < range.end { range.start.saturating_sub(1) } else { self.active - n };
        }
        self.active = self.active.min(self.layers.len() - 1);
        true
    }

    /// Move the layer (with its contents, if a group) so it ends up at index `to` in the current
    /// stack. It joins whatever group it lands in.
    pub fn move_layer(&mut self, from: usize, to: usize) {
        if from >= self.layers.len() || to >= self.layers.len() || from == to {
            return;
        }
        let range = self.block(from);
        if range.contains(&to) {
            return;
        }
        let block: Vec<Layer> = self.layers.drain(range.clone()).collect();
        let n = block.len();
        // `to` was an index in the old stack; shift it for the removed block.
        let mut at = if to > from { to + 1 - n } else { to };
        at = at.min(self.layers.len());
        let above = self.layers.get(at);
        let parent = above.map(|a| if a.is_group() { Some(a.id) } else { a.parent }).unwrap_or(None);
        for (k, l) in block.into_iter().enumerate() {
            self.layers.insert(at + k, l);
        }
        let top = at + n - 1;
        self.layers[top].parent = parent;
        self.active = top;
    }

    /// Put the active layer into a new group. Returns the group's index.
    pub fn group_active(&mut self, name: &str) -> Option<usize> {
        let index = self.active;
        let range = self.block(index);
        let parent = self.layers.get(index)?.parent;
        let id = self.new_id();
        let mut g = Layer::with_kind(
            id,
            name,
            LayerKind::Group { pass_through: true, collapsed: false },
            self.width,
            self.height,
        );
        g.parent = parent;
        let top = range.end - 1;
        self.layers[top].parent = Some(id);
        self.layers.insert(range.end, g);
        self.active = range.end;
        Some(range.end)
    }

    /// Dissolve a group, keeping its contents in place.
    pub fn ungroup(&mut self, index: usize) -> bool {
        let Some(g) = self.layers.get(index) else { return false };
        if !g.is_group() {
            return false;
        }
        let (id, parent) = (g.id, g.parent);
        for l in &mut self.layers {
            if l.parent == Some(id) {
                l.parent = parent;
            }
        }
        self.layers.remove(index);
        self.active = index.saturating_sub(1).min(self.layers.len().saturating_sub(1));
        true
    }

    /// Flatten a group into one pixel layer.
    pub fn merge_group(&mut self, index: usize) -> bool {
        let range = self.block(index);
        let Some(g) = self.layers.get(index) else { return false };
        if !g.is_group() {
            return false;
        }
        let mut sub: Vec<Layer> = self.layers[range.clone()].to_vec();
        // Lift the group to the top level so its own opacity and visibility still apply.
        if let Some(top) = sub.last_mut() {
            top.parent = None;
            top.visible = true;
        }
        let pixels = flatten(&sub, self.width, self.height);
        let (name, parent, visible) = (g.name.clone(), g.parent, g.visible);
        let id = self.new_id();
        let mut l = Layer::raster(id, name, pixels);
        l.parent = parent;
        l.visible = visible;
        let start = range.start;
        self.layers.drain(range);
        self.layers.insert(start, l);
        self.active = start;
        true
    }

    /// Merge the layer at `index` into the one below it.
    pub fn merge_down(&mut self, index: usize) -> bool {
        if self.layers.get(index).is_some_and(|l| l.is_group()) {
            return self.merge_group(index);
        }
        if index == 0 || index >= self.layers.len() || self.layers[index - 1].is_group() {
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
        let vis: Vec<bool> = (0..self.layers.len()).map(|i| self.effectively_visible(i)).collect();
        let pixel_layers = vis.iter().zip(&self.layers).filter(|(v, l)| **v && !l.is_group()).count();
        if pixel_layers < 2 {
            return;
        }
        let merged = flatten(&self.layers, self.width, self.height);
        let Some(first) = vis.iter().position(|v| *v) else { return };
        let id = self.new_id();
        let name = self.layers[first].name.clone();
        let mut k = 0;
        self.layers.retain(|_| {
            let keep = !vis[k];
            k += 1;
            keep
        });
        // Hidden groups that lost their children go too.
        let mut i = 0;
        while i < self.layers.len() {
            let gid = self.layers[i].id;
            if self.layers[i].is_group() && !self.layers.iter().any(|l| l.parent == Some(gid)) {
                self.layers.remove(i);
            } else {
                i += 1;
            }
        }
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
        let Some(layer) = self.active_layer() else {
            return false;
        };
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
        for (_, c) in &mut self.channels {
            *c = mask(c);
        }
        match params {
            Params::Offset(dx, dy) => {
                for g in &mut self.guides {
                    g.pos += if g.horizontal { dy } else { dx };
                }
                for p in &mut self.paths {
                    p.path.translate(dx, dy);
                }
            }
            Params::Scale(sx, sy) => {
                for g in &mut self.guides {
                    g.pos *= if g.horizontal { sy } else { sx };
                }
                for p in &mut self.paths {
                    p.path.map(|q| (q.0 * sx, q.1 * sy));
                }
            }
            // Guides and paths don't survive rotation meaningfully.
            Params::Rasterize => {
                self.guides.clear();
                self.paths.clear();
            }
        }
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
        self.transform_all(
            w,
            h,
            |i| transform::flip(i, horizontal),
            |m| transform::flip_mask(m, horizontal),
            Params::Rasterize,
        );
    }

    pub fn rotate_arbitrary(&mut self, deg: f32) {
        let (nw, nh, t) = transform::rotation_fit(self.width, self.height, deg);
        self.transform_all(
            nw,
            nh,
            |i| transform::affine(i, &t, nw, nh),
            |m| transform::affine_mask(m, &t, nw, nh),
            Params::Rasterize,
        );
    }

    /// Transform the active layer's pixels (Free Transform), limited to the selection if any.
    pub fn transform_active(&mut self, t: &Affine) {
        if self.active_layer().is_some_and(|l| l.is_group()) {
            let group = self.active;
            let range = self.block(group);
            let sel = self.selection.clone();
            for i in range.start..range.end - 1 {
                self.active = i;
                self.selection = sel.clone();
                self.transform_active(t);
            }
            self.active = group;
            return;
        }
        let (w, h) = (self.width, self.height);
        let sel = self.selection.clone();
        let Some(l) = self.active_layer_mut() else {
            return;
        };
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
        if self.active_layer().is_some_and(|l| l.is_group()) {
            let group = self.active;
            let range = self.block(group);
            let sel = self.selection.take();
            for i in range.start..range.end - 1 {
                self.active = i;
                self.nudge_active(dx, dy);
            }
            self.selection = sel.map(|s| crate::filters::shift_mask(&s, dx, dy));
            self.active = group;
            return;
        }
        let sel = self.selection.clone();
        let (w, h) = (self.width, self.height);
        let Some(l) = self.active_layer_mut() else {
            return;
        };
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
    fn groups_move_duplicate_delete_as_blocks() {
        let mut d = Document::new(4, 4, Some(Rgba::WHITE));
        d.add_empty_layer("A");
        d.add_empty_layer("B");
        d.active = 2;
        let g = d.group_active("Group 1").unwrap();
        assert_eq!(g, 3);
        assert_eq!(d.layers[2].parent, Some(d.layers[3].id));
        d.active = 1;
        // A moves into the group when dropped just under it.
        d.move_layer(1, 2);
        assert!(d.layers.iter().filter(|l| l.parent == Some(d.layers[3].id)).count() == 2);
        // Duplicating the group copies its children with fresh ids.
        d.duplicate_layer(3);
        assert_eq!(d.layers.len(), 7);
        let new_g = d.layers[6].id;
        assert_eq!(d.layers.iter().filter(|l| l.parent == Some(new_g)).count(), 2);
        // Deleting a group removes its contents.
        d.delete_layer(6);
        assert_eq!(d.layers.len(), 4);
        // Merging a group leaves one pixel layer in its place.
        assert!(d.merge_down(3));
        assert_eq!(d.layers.len(), 2);
        assert!(!d.layers[1].is_group());
    }

    #[test]
    fn ungroup_keeps_children() {
        let mut d = Document::new(4, 4, Some(Rgba::WHITE));
        d.add_empty_layer("A");
        d.group_active("G");
        assert!(d.ungroup(2));
        assert_eq!(d.layers.len(), 2);
        assert!(d.layers.iter().all(|l| l.parent.is_none()));
    }

    #[test]
    fn new_layer_inside_selected_group() {
        let mut d = Document::new(4, 4, Some(Rgba::WHITE));
        d.add_empty_layer("A");
        d.group_active("G");
        d.add_empty_layer("B");
        let gid = d.layers.iter().find(|l| l.is_group()).unwrap().id;
        assert_eq!(d.layers[d.active].parent, Some(gid));
        assert!(d.layers[d.active + 1].is_group());
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
    fn guides_follow_crop_and_resize() {
        let mut d = Document::new(100, 100, Some(Rgba::WHITE));
        d.guides.push(Guide { horizontal: false, pos: 50.0 });
        d.crop(Rect::new(10, 0, 80, 100));
        assert_eq!(d.guides[0].pos, 40.0);
        d.resize_image(160, 200, Resample::Bilinear);
        assert_eq!(d.guides[0].pos, 80.0);
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
