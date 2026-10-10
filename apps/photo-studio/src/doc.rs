//! An open document: the model, its history, view state and GPU textures.

use std::collections::HashMap;
use std::path::PathBuf;

use egui::{ColorImage, TextureHandle, TextureOptions};
use studio_core::Document;
use studio_core::composite::Compositor;
use studio_core::geom::Rect;
use studio_core::history::History;
use studio_core::image::Image;
use studio_core::layer::LayerId;

const TILE: u32 = 1024;

struct Tile {
    rect: Rect,
    tex: TextureHandle,
}

pub struct DocTab {
    pub doc: Document,
    pub history: History,
    pub path: Option<PathBuf>,
    pub name: String,
    revision: u64,
    saved_revision: u64,
    pub zoom: f32,
    /// Offset of the image centre from the viewport centre, in screen points.
    pub pan: egui::Vec2,
    pub fit_pending: bool,
    compositor: Compositor,
    /// Full-resolution composite (kept for the eyedropper, navigator and histogram).
    pub composite: Image,
    composite_gen: u64,
    tiles: Vec<Tile>,
    dirty: Option<Rect>,
    tiles_stale: bool,
    thumbs: HashMap<(LayerId, bool), (u64, TextureHandle)>,
    nav_tex: Option<(u64, TextureHandle)>,
    pub ants: Option<(u64, Vec<((i32, i32), (i32, i32))>)>,
    pub histogram: Option<(u64, studio_core::histogram::Histogram)>,
    /// The file this document was opened from (for flat images, `path` stays empty).
    pub source: Option<PathBuf>,
}

impl DocTab {
    pub fn new(doc: Document, name: String, path: Option<PathBuf>) -> Self {
        let history = History::new(&doc, if path.is_some() { "Open" } else { "New" });
        DocTab {
            composite: Image::new(doc.width, doc.height),
            doc,
            history,
            path,
            name,
            revision: 0,
            saved_revision: 0,
            zoom: 1.0,
            pan: egui::Vec2::ZERO,
            fit_pending: true,
            compositor: Compositor::new(),
            composite_gen: 0,
            tiles: Vec::new(),
            dirty: None,
            tiles_stale: true,
            thumbs: HashMap::new(),
            nav_tex: None,
            ants: None,
            histogram: None,
            source: None,
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.revision != self.saved_revision
    }

    pub fn mark_saved(&mut self) {
        self.saved_revision = self.revision;
    }

    pub fn title(&self) -> String {
        format!("{}{}", self.name, if self.is_dirty() { " •" } else { "" })
    }

    /// Recomposite `r` (canvas pixels) on the next refresh.
    pub fn invalidate(&mut self, r: Rect) {
        let r = r.intersect(&self.doc.bounds());
        if r.is_empty() {
            return;
        }
        self.dirty = Some(self.dirty.map_or(r, |d| d.union(&r)));
    }

    pub fn invalidate_all(&mut self) {
        self.dirty = Some(self.doc.bounds());
        if self.composite.size() != (self.doc.width, self.doc.height) {
            self.tiles_stale = true;
        }
    }

    /// Record an edit in history.
    pub fn commit(&mut self, label: &str) {
        self.history.commit(&self.doc, label);
        self.revision += 1;
        self.invalidate_all();
    }

    pub fn undo(&mut self) -> Option<String> {
        let r = self.history.undo(&mut self.doc);
        if r.is_some() {
            self.revision += 1;
            self.invalidate_all();
        }
        r
    }

    pub fn redo(&mut self) -> Option<String> {
        let r = self.history.redo(&mut self.doc);
        if r.is_some() {
            self.revision += 1;
            self.invalidate_all();
        }
        r
    }

    pub fn goto_history(&mut self, i: usize) {
        self.history.goto(&mut self.doc, i);
        self.revision += 1;
        self.invalidate_all();
    }

    /// Bring the composite and textures up to date.
    pub fn refresh(&mut self, ctx: &egui::Context) {
        let (w, h) = (self.doc.width, self.doc.height);
        if self.composite.size() != (w, h) || self.tiles_stale {
            self.composite = Image::new(w, h);
            self.tiles.clear();
            self.dirty = Some(self.doc.bounds());
            self.tiles_stale = false;
            self.fit_pending |= self.composite.width() == 0;
        }
        let Some(d) = self.dirty.take() else { return };
        self.compositor.prune(&self.doc.layers);
        let part = self.compositor.composite_region(&self.doc.layers, w, h, d);
        self.composite.blit(&part, d.x, d.y);
        self.composite_gen = self.composite.generation();

        let opts = TextureOptions {
            magnification: egui::TextureFilter::Nearest,
            minification: egui::TextureFilter::Linear,
            wrap_mode: egui::TextureWrapMode::ClampToEdge,
            mipmap_mode: Some(egui::TextureFilter::Linear),
        };
        if self.tiles.is_empty() {
            for ty in (0..h).step_by(TILE as usize) {
                for tx in (0..w).step_by(TILE as usize) {
                    let rect = Rect::new(tx as i32, ty as i32, TILE.min(w - tx), TILE.min(h - ty));
                    let img = to_color_image(&self.composite.crop(rect));
                    let tex = ctx.load_texture(format!("tile-{}-{tx}-{ty}", self.doc_key()), img, opts);
                    self.tiles.push(Tile { rect, tex });
                }
            }
        } else {
            for t in &mut self.tiles {
                let inter = t.rect.intersect(&d);
                if inter.is_empty() {
                    continue;
                }
                // Whole-tile updates keep mipmaps consistent.
                t.tex.set(to_color_image(&self.composite.crop(t.rect)), opts);
            }
        }
        if self.doc.selection.is_none() {
            self.ants = None;
        }
    }

    fn doc_key(&self) -> usize {
        self as *const _ as usize
    }

    /// Draw the image tiles into `screen` (the image's on-screen rect).
    pub fn paint(&self, painter: &egui::Painter, origin: egui::Pos2) {
        let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
        for t in &self.tiles {
            let min = origin + egui::vec2(t.rect.x as f32, t.rect.y as f32) * self.zoom;
            let r = egui::Rect::from_min_size(min, egui::vec2(t.rect.w as f32, t.rect.h as f32) * self.zoom);
            painter.image(t.tex.id(), r, uv, egui::Color32::WHITE);
        }
    }

    /// Thumbnail texture of a layer (or its mask), regenerated when its pixels change.
    pub fn thumb(&mut self, ctx: &egui::Context, index: usize, mask: bool) -> Option<egui::TextureId> {
        let layer = self.doc.layers.get(index)?;
        let (stamp, src) = if mask {
            let m = layer.mask.as_ref()?;
            (m.generation(), None)
        } else {
            if layer.pixels.width() == 0 {
                return None;
            }
            (layer.pixels.generation(), Some(&layer.pixels))
        };
        let key = (layer.id, mask);
        if let Some((g, t)) = self.thumbs.get(&key) {
            if *g == stamp {
                return Some(t.id());
            }
        }
        let side = 64.0;
        let (w, h) = (self.doc.width as f32, self.doc.height as f32);
        let s = (side / w.max(h)).min(1.0);
        let (tw, th) = (((w * s).round() as u32).max(1), ((h * s).round() as u32).max(1));
        let img = match src {
            Some(px) => studio_core::transform::resize(px, tw, th, studio_core::transform::Resample::Bilinear),
            None => {
                let m = layer.mask.as_ref()?;
                studio_core::transform::resize(&m.to_image(), tw, th, studio_core::transform::Resample::Bilinear)
            }
        };
        let tex =
            ctx.load_texture(format!("thumb-{}-{}", layer.id, mask), to_color_image(&img), TextureOptions::LINEAR);
        let id = tex.id();
        self.thumbs.insert(key, (stamp, tex));
        Some(id)
    }

    /// Small texture of the whole composite for the Navigator.
    pub fn nav_texture(&mut self, ctx: &egui::Context) -> Option<egui::TextureId> {
        if self.composite.width() == 0 {
            return None;
        }
        if let Some((g, t)) = &self.nav_tex {
            if *g == self.composite_gen {
                return Some(t.id());
            }
        }
        let s = (300.0 / self.composite.width().max(self.composite.height()) as f32).min(1.0);
        let img = studio_core::transform::resize(
            &self.composite,
            ((self.composite.width() as f32 * s) as u32).max(1),
            ((self.composite.height() as f32 * s) as u32).max(1),
            studio_core::transform::Resample::Bilinear,
        );
        let tex = ctx.load_texture("navigator", to_color_image(&img), TextureOptions::LINEAR);
        let id = tex.id();
        self.nav_tex = Some((self.composite_gen, tex));
        Some(id)
    }

    pub fn histogram(&mut self) -> &studio_core::histogram::Histogram {
        let stamp = self.composite_gen;
        if self.histogram.as_ref().is_none_or(|(g, _)| *g != stamp) {
            // Sample a reduced copy for speed on big images.
            let c = &self.composite;
            let s = (512.0 / c.width().max(c.height()).max(1) as f32).min(1.0);
            let small = if s < 1.0 {
                studio_core::transform::resize(
                    c,
                    ((c.width() as f32 * s) as u32).max(1),
                    ((c.height() as f32 * s) as u32).max(1),
                    studio_core::transform::Resample::Nearest,
                )
            } else {
                c.clone()
            };
            self.histogram = Some((stamp, studio_core::histogram::Histogram::of(&small, None)));
        }
        &self.histogram.as_ref().unwrap().1
    }

    /// Fit the image inside `avail` (screen points).
    pub fn fit(&mut self, avail: egui::Vec2) {
        let (w, h) = (self.doc.width as f32, self.doc.height as f32);
        if w <= 0.0 || h <= 0.0 {
            return;
        }
        let z = ((avail.x - 48.0) / w).min((avail.y - 48.0) / h);
        self.zoom = z.clamp(0.01, 1.0);
        self.pan = egui::Vec2::ZERO;
    }

    /// Zoom by `factor` keeping the canvas point under `anchor` (screen) fixed.
    pub fn zoom_at(&mut self, factor: f32, anchor: egui::Pos2, view_center: egui::Pos2) {
        let new_zoom = (self.zoom * factor).clamp(0.01, 64.0);
        let f = new_zoom / self.zoom;
        let c = view_center + self.pan;
        let new_c = anchor + (c - anchor) * f;
        self.pan = new_c - view_center;
        self.zoom = new_zoom;
    }
}

pub fn to_color_image(img: &Image) -> ColorImage {
    ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.pixels())
}

/// The next zoom stop above or below the current zoom.
pub fn zoom_step(z: f32, up: bool) -> f32 {
    const STOPS: [f32; 22] = [
        0.01, 0.02, 0.03, 0.05, 0.0625, 0.083, 0.125, 0.167, 0.25, 0.333, 0.5, 0.667, 1.0, 1.5, 2.0, 3.0, 4.0, 6.0,
        8.0, 12.0, 16.0, 32.0,
    ];
    if up {
        STOPS.iter().copied().find(|&s| s > z * 1.001).unwrap_or(64.0)
    } else {
        STOPS.iter().rev().copied().find(|&s| s < z * 0.999).unwrap_or(0.01)
    }
}
