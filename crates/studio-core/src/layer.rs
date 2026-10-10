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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum FillLayer {
    Solid(Rgba),
    Gradient { gradient: Gradient, shape: GradientShape, start: (f32, f32), end: (f32, f32) },
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
        }
    }

    /// Text, shape and fill layers are drawn from parameters rather than painted.
    pub fn is_generated(&self) -> bool {
        matches!(self, Self::Text(_) | Self::Shape(_) | Self::Fill(_))
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
}

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
            LayerKind::Raster | LayerKind::Adjustment(_) | LayerKind::Group { .. } => {}
        }
    }

    /// Turn a text, shape or fill layer into plain pixels.
    pub fn rasterize(&mut self) {
        if self.kind.is_generated() {
            self.kind = LayerKind::Raster;
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
