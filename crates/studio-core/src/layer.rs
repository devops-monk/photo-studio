//! Layers and their content kinds.

use serde::{Deserialize, Serialize};

use crate::adjust::Adjustment;
use crate::blend::BlendMode;
use crate::color::Rgba;
use crate::gradient::{Gradient, GradientShape};
use crate::image::{Image, Mask};
use crate::shape::ShapeLayer;
use crate::styles::LayerStyles;
use crate::text::TextLayer;

pub type LayerId = u64;

/// A filter applied non-destructively to a smart object.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SmartFilter {
    pub filter: crate::filters::Filter,
    pub enabled: bool,
}

/// Row-major affine [a, b, c, d, e, f] mapping source pixels to the canvas.
pub const IDENTITY_XFORM: [f32; 6] = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

fn to_affine(x: &[f32; 6]) -> crate::geom::Affine {
    crate::geom::Affine { a: x[0], b: x[1], c: x[2], d: x[3], e: x[4], f: x[5] }
}

pub fn compose_xform(x: &[f32; 6], t: &crate::geom::Affine) -> [f32; 6] {
    let r = to_affine(x).then(t);
    [r.a, r.b, r.c, r.d, r.e, r.f]
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum FillLayer {
    Solid(Rgba),
    Gradient {
        gradient: Gradient,
        shape: GradientShape,
        start: (f32, f32),
        end: (f32, f32),
    },
    /// A built-in pattern (`paint::builtin_patterns`), tiled at `scale` percent.
    Pattern {
        pattern: usize,
        fg: Rgba,
        bg: Rgba,
        scale: f32,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum LayerKind {
    Raster,
    Text(TextLayer),
    Shape(ShapeLayer),
    Fill(FillLayer),
    Adjustment(Adjustment),
    /// A folder of layers. Its children sit directly below it in the stack and point at it
    /// through `Layer::parent`.
    /// Keeps its original pixels in `Layer::source`; transforms and filters are re-applied
    /// from them, so nothing is lost.
    Smart {
        filters: Vec<SmartFilter>,
        xform: [f32; 6],
    },
    Group {
        /// Pass-through groups blend their children straight into the layers below;
        /// otherwise the group is flattened on its own first (isolated).
        pass_through: bool,
        collapsed: bool,
    },
}

impl LayerKind {
    pub fn badge(&self) -> &'static str {
        match self {
            Self::Raster => "Pixel",
            Self::Text(_) => "Type",
            Self::Shape(_) => "Shape",
            Self::Fill(_) => "Fill",
            Self::Adjustment(_) => "Adjustment",
            Self::Group { .. } => "Group",
            Self::Smart { .. } => "Smart Object",
        }
    }

    /// Text, shape and fill layers are drawn from parameters rather than painted.
    pub fn is_generated(&self) -> bool {
        matches!(self, Self::Text(_) | Self::Shape(_) | Self::Fill(_) | Self::Smart { .. })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    pub id: LayerId,
    pub name: String,
    pub visible: bool,
    /// Locks pixels against painting and filters.
    pub locked: bool,
    pub opacity: f32,
    pub blend: BlendMode,
    pub kind: LayerKind,
    /// Canvas-sized pixels (0×0 for adjustment layers).
    pub pixels: Image,
    pub mask: Option<Mask>,
    pub mask_enabled: bool,
    /// Clip to the layer below (clipping mask).
    pub clip: bool,
    pub styles: LayerStyles,
    /// The group this layer belongs to.
    pub parent: Option<LayerId>,
    /// Original pixels of a smart object.
    pub source: Option<Image>,
    /// Opacity of the content only; layer styles keep `opacity`.
    pub fill_opacity: f32,
    /// Colour label index (see `LABEL_COLORS`).
    pub label: Option<u8>,
    /// A path that hides everything outside it (vector mask); combines with `mask`.
    pub vector_mask: Option<crate::path::BezierPath>,
}

/// Layer colour labels: red, orange, yellow, green, blue, violet, grey.
pub const LABEL_COLORS: [Rgba; 7] = [
    Rgba::rgb(255, 69, 58),
    Rgba::rgb(255, 159, 10),
    Rgba::rgb(255, 214, 10),
    Rgba::rgb(48, 209, 88),
    Rgba::rgb(10, 132, 255),
    Rgba::rgb(191, 90, 242),
    Rgba::rgb(142, 142, 147),
];

impl Layer {
    pub fn raster(id: LayerId, name: impl Into<String>, pixels: Image) -> Self {
        Layer {
            id,
            name: name.into(),
            visible: true,
            locked: false,
            opacity: 1.0,
            blend: BlendMode::Normal,
            kind: LayerKind::Raster,
            pixels,
            mask: None,
            mask_enabled: true,
            clip: false,
            styles: LayerStyles::default(),
            parent: None,
            source: None,
            fill_opacity: 1.0,
            label: None,
            vector_mask: None,
        }
    }

    pub fn with_kind(id: LayerId, name: impl Into<String>, kind: LayerKind, w: u32, h: u32) -> Self {
        let pixels = if matches!(kind, LayerKind::Adjustment(_) | LayerKind::Group { .. }) {
            Image::new(0, 0)
        } else {
            Image::new(w, h)
        };
        let mut l = Layer { kind, ..Layer::raster(id, name, pixels) };
        l.render(w, h);
        l
    }

    pub fn is_smart(&self) -> bool {
        matches!(self.kind, LayerKind::Smart { .. })
    }

    /// Make this layer a smart object whose source is its current pixels.
    pub fn convert_to_smart(&mut self) -> bool {
        if self.pixels.width() == 0 || self.is_smart() {
            return false;
        }
        self.source = Some(self.pixels.clone());
        self.kind = LayerKind::Smart { filters: Vec::new(), xform: IDENTITY_XFORM };
        true
    }

    pub fn is_group(&self) -> bool {
        matches!(self.kind, LayerKind::Group { .. })
    }

    pub fn is_adjustment(&self) -> bool {
        matches!(self.kind, LayerKind::Adjustment(_))
    }

    /// Whether pixel tools may paint on it directly.
    pub fn is_paintable(&self) -> bool {
        matches!(self.kind, LayerKind::Raster) && !self.locked
    }

    pub fn active_mask(&self) -> Option<&Mask> {
        if self.mask_enabled { self.mask.as_ref() } else { None }
    }

    /// Redraw generated content (text, shape, fill) at canvas size `w × h`.
    pub fn render(&mut self, w: u32, h: u32) {
        match &self.kind {
            LayerKind::Text(t) => self.pixels = crate::text::render(t, w, h),
            LayerKind::Shape(s) => self.pixels = crate::shape::render(s, w, h),
            LayerKind::Fill(FillLayer::Solid(c)) => self.pixels = Image::filled(w, h, *c),
            LayerKind::Fill(FillLayer::Gradient { gradient, shape, start, end }) => {
                self.pixels = crate::paint::render_gradient(w, h, gradient, *shape, *start, *end)
            }
            LayerKind::Fill(FillLayer::Pattern { pattern, fg, bg, scale }) => {
                let tiles = crate::paint::builtin_patterns(*fg, *bg);
                let Some((_, tile)) = tiles.get(*pattern).or(tiles.first()) else { return };
                let k = (scale / 100.0).max(0.05);
                let (tw, th) = (tile.width().max(1), tile.height().max(1));
                let mut img = Image::new(w, h);
                let px = img.pixels_mut();
                for y in 0..h {
                    for x in 0..w {
                        let c = tile.get(((x as f32 / k) as u32) % tw, ((y as f32 / k) as u32) % th);
                        let i = (y as usize * w as usize + x as usize) * 4;
                        px[i..i + 4].copy_from_slice(&c.0);
                    }
                }
                self.pixels = img;
            }
            LayerKind::Smart { filters, xform } => {
                let Some(src) = &self.source else { return };
                let mut img = if *xform == IDENTITY_XFORM && src.size() == (w, h) {
                    src.clone()
                } else {
                    crate::transform::affine(src, &to_affine(xform), w, h)
                };
                let ctx = crate::filters::FilterCtx::default();
                for f in filters.iter().filter(|f| f.enabled) {
                    f.filter.apply(&mut img, None, &ctx);
                }
                self.pixels = img;
            }
            LayerKind::Raster | LayerKind::Adjustment(_) | LayerKind::Group { .. } => {}
        }
    }

    /// Turn a text, shape or fill layer into plain pixels.
    pub fn rasterize(&mut self) {
        if self.kind.is_generated() {
            self.kind = LayerKind::Raster;
            self.source = None;
        }
    }

    /// Alpha of the layer content after its mask (not opacity), at a pixel.
    #[inline]
    pub fn content_alpha(&self, x: u32, y: u32) -> u8 {
        if self.pixels.width() == 0 {
            return 255;
        }
        let a = self.pixels.pixels()[self.pixels.index(x, y) + 3] as u32;
        match self.active_mask() {
            Some(m) => ((a * m.get(x, y) as u32 + 127) / 255) as u8,
            None => a as u8,
        }
    }
}
