//! Photo Studio's image engine.
//!
//! Everything here is UI-free: the desktop app and the CLI both drive the same document
//! model, compositor, adjustments, filters, brushes and file formats.

pub mod adjust;
pub mod blend;
pub mod color;
pub mod composite;
pub mod document;
pub mod filters;
pub mod geom;
pub mod gradient;
pub mod histogram;
pub mod history;
pub mod image;
pub mod io;
pub mod layer;
pub mod paint;
pub mod path;
pub mod select;
pub mod shape;
pub mod styles;
pub mod text;
pub mod transform;

pub use crate::adjust::Adjustment;
pub use crate::blend::BlendMode;
pub use crate::color::Rgba;
pub use crate::document::Document;
pub use crate::filters::Filter;
pub use crate::geom::Rect;
pub use crate::image::{Image, Mask};
pub use crate::layer::{Layer, LayerKind};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
