//! Tools, their toolbar grouping, and their options.

use egui_phosphor::regular as ph;
use studio_core::blend::BlendMode;
use studio_core::gradient::GradientShape;
use studio_core::paint::{BrushTip, ToneRange};
use studio_core::select::SelectOp;
use studio_core::shape::ShapeKind;
use studio_core::text::{Align, FontFace};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Tool {
    Move,
    RectMarquee,
    EllipseMarquee,
    Lasso,
    PolyLasso,
    QuickSelect,
    MagicWand,
    Crop,
    Eyedropper,
    SpotHeal,
    Heal,
    Brush,
    Pencil,
    CloneStamp,
    Eraser,
    Gradient,
    Bucket,
    Blur,
    Sharpen,
    Smudge,
    Dodge,
    Burn,
    Sponge,
    Text,
    Rectangle,
    RoundedRect,
    Ellipse,
    Polygon,
    Star,
    Line,
    Hand,
    Zoom,
}

/// Toolbar groups; the first tool in each is the group's default.
pub const GROUPS: &[&[Tool]] = &[
    &[Tool::Move],
    &[Tool::RectMarquee, Tool::EllipseMarquee],
    &[Tool::Lasso, Tool::PolyLasso],
    &[Tool::QuickSelect, Tool::MagicWand],
    &[Tool::Crop],
    &[Tool::Eyedropper],
    &[Tool::SpotHeal, Tool::Heal],
    &[Tool::Brush, Tool::Pencil],
    &[Tool::CloneStamp],
    &[Tool::Eraser],
    &[Tool::Gradient, Tool::Bucket],
    &[Tool::Blur, Tool::Sharpen, Tool::Smudge],
    &[Tool::Dodge, Tool::Burn, Tool::Sponge],
    &[Tool::Text],
    &[Tool::Rectangle, Tool::RoundedRect, Tool::Ellipse, Tool::Polygon, Tool::Star, Tool::Line],
    &[Tool::Hand],
    &[Tool::Zoom],
];

impl Tool {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Move => "Move",
            Self::RectMarquee => "Rectangular Marquee",
            Self::EllipseMarquee => "Elliptical Marquee",
            Self::Lasso => "Lasso",
            Self::PolyLasso => "Polygonal Lasso",
            Self::QuickSelect => "Quick Selection",
            Self::MagicWand => "Magic Wand",
            Self::Crop => "Crop",
            Self::Eyedropper => "Eyedropper",
            Self::SpotHeal => "Spot Healing Brush",
            Self::Heal => "Healing Brush",
            Self::Brush => "Brush",
            Self::Pencil => "Pencil",
            Self::CloneStamp => "Clone Stamp",
            Self::Eraser => "Eraser",
            Self::Gradient => "Gradient",
            Self::Bucket => "Paint Bucket",
            Self::Blur => "Blur",
            Self::Sharpen => "Sharpen",
            Self::Smudge => "Smudge",
            Self::Dodge => "Dodge",
            Self::Burn => "Burn",
            Self::Sponge => "Sponge",
            Self::Text => "Type",
            Self::Rectangle => "Rectangle",
            Self::RoundedRect => "Rounded Rectangle",
            Self::Ellipse => "Ellipse",
            Self::Polygon => "Polygon",
            Self::Star => "Star",
            Self::Line => "Line",
            Self::Hand => "Hand",
            Self::Zoom => "Zoom",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            Self::Move => ph::ARROWS_OUT_CARDINAL,
            Self::RectMarquee => ph::SELECTION,
            Self::EllipseMarquee => ph::CIRCLE_DASHED,
            Self::Lasso => ph::LASSO,
            Self::PolyLasso => ph::POLYGON,
            Self::QuickSelect => ph::SPARKLE,
            Self::MagicWand => ph::MAGIC_WAND,
            Self::Crop => ph::CROP,
            Self::Eyedropper => ph::EYEDROPPER,
            Self::SpotHeal | Self::Heal => ph::BANDAIDS,
            Self::Brush => ph::PAINT_BRUSH,
            Self::Pencil => ph::PENCIL_SIMPLE,
            Self::CloneStamp => ph::STAMP,
            Self::Eraser => ph::ERASER,
            Self::Gradient => ph::GRADIENT,
            Self::Bucket => ph::PAINT_BUCKET,
            Self::Blur => ph::DROP,
            Self::Sharpen => ph::TRIANGLE,
            Self::Smudge => ph::HAND_POINTING,
            Self::Dodge => ph::SUN_DIM,
            Self::Burn => ph::FIRE,
            Self::Sponge => ph::DROP_HALF,
            Self::Text => ph::TEXT_T,
            Self::Rectangle => ph::RECTANGLE,
            Self::RoundedRect => ph::SQUARE,
            Self::Ellipse => ph::CIRCLE,
            Self::Polygon => ph::HEXAGON,
            Self::Star => ph::STAR,
            Self::Line => ph::LINE_SEGMENT,
            Self::Hand => ph::HAND,
            Self::Zoom => ph::MAGNIFYING_GLASS,
        }
    }

    /// Single-key shortcut; pressing it repeatedly with Shift cycles the group.
    pub fn key(&self) -> Option<egui::Key> {
        use egui::Key;
        Some(match self {
            Self::Move => Key::V,
            Self::RectMarquee | Self::EllipseMarquee => Key::M,
            Self::Lasso | Self::PolyLasso => Key::L,
            Self::QuickSelect | Self::MagicWand => Key::W,
            Self::Crop => Key::C,
            Self::Eyedropper => Key::I,
            Self::SpotHeal | Self::Heal => Key::J,
            Self::Brush | Self::Pencil => Key::B,
            Self::CloneStamp => Key::S,
            Self::Eraser => Key::E,
            Self::Gradient | Self::Bucket => Key::G,
            Self::Blur | Self::Sharpen | Self::Smudge => Key::R,
            Self::Dodge | Self::Burn | Self::Sponge => Key::O,
            Self::Text => Key::T,
            Self::Rectangle | Self::RoundedRect | Self::Ellipse | Self::Polygon | Self::Star | Self::Line => Key::U,
            Self::Hand => Key::H,
            Self::Zoom => Key::Z,
        })
    }

    pub fn group(&self) -> &'static [Tool] {
        GROUPS.iter().find(|g| g.contains(self)).copied().unwrap_or(&[])
    }

    pub fn uses_brush(&self) -> bool {
        matches!(
            self,
            Self::Brush
                | Self::Pencil
                | Self::Eraser
                | Self::CloneStamp
                | Self::SpotHeal
                | Self::Heal
                | Self::Blur
                | Self::Sharpen
                | Self::Smudge
                | Self::Dodge
                | Self::Burn
                | Self::Sponge
                | Self::QuickSelect
        )
    }

    pub fn shape_kind(&self, o: &ToolOptions) -> Option<ShapeKind> {
        Some(match self {
            Self::Rectangle => ShapeKind::Rectangle,
            Self::RoundedRect => ShapeKind::RoundedRect { radius: o.corner_radius },
            Self::Ellipse => ShapeKind::Ellipse,
            Self::Polygon => ShapeKind::Polygon { sides: o.sides },
            Self::Star => ShapeKind::Star { points: o.sides, inner: o.star_inner },
            Self::Line => ShapeKind::Line,
            _ => return None,
        })
    }

    /// Short hint shown in the status bar.
    pub fn hint(&self) -> &'static str {
        match self {
            Self::Move => "Drag to move the layer or selection · arrow keys nudge (Shift ×10)",
            Self::RectMarquee | Self::EllipseMarquee => {
                "Drag to select · Shift adds, Alt subtracts · hold Shift after starting for a square"
            }
            Self::Lasso => "Drag a freehand outline",
            Self::PolyLasso => "Click to add points · double-click or click the start to close · Esc cancels",
            Self::QuickSelect => "Paint over the area to select · Alt to subtract",
            Self::MagicWand => "Click to select similar colors",
            Self::Crop => "Drag a crop box · Enter to apply · Esc to cancel",
            Self::Eyedropper => "Click to pick the foreground color · Alt for background",
            Self::SpotHeal => "Paint over blemishes to blend them away",
            Self::Heal | Self::CloneStamp => "Alt-click to set the source, then paint",
            Self::Brush | Self::Pencil => "Paint · [ and ] change size · Shift-click draws a straight line",
            Self::Eraser => "Erase to transparency",
            Self::Gradient => "Drag to draw a gradient",
            Self::Bucket => "Click to fill similar colors",
            Self::Blur | Self::Sharpen | Self::Smudge => "Paint to soften, sharpen or smear",
            Self::Dodge | Self::Burn | Self::Sponge => "Paint to lighten, darken or change saturation",
            Self::Text => "Click to add text, or click existing text to edit it",
            Self::Rectangle | Self::RoundedRect | Self::Ellipse | Self::Polygon | Self::Star | Self::Line => {
                "Drag to draw a shape layer · Shift constrains"
            }
            Self::Hand => "Drag to pan · double-click to fit",
            Self::Zoom => "Click to zoom in · Alt-click to zoom out",
        }
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ToolOptions {
    pub tip: BrushTip,
    pub opacity: f32,
    pub flow: f32,
    pub brush_blend: BlendMode,
    pub eraser_tip: BrushTip,
    pub select_op: SelectOp,
    pub feather: f32,
    pub tolerance: u8,
    pub contiguous: bool,
    pub anti_alias: bool,
    pub sample_merged: bool,
    pub gradient_preset: usize,
    pub gradient_shape: GradientShape,
    pub gradient_reverse: bool,
    pub gradient_blend: BlendMode,
    pub gradient_opacity: f32,
    pub fill_enabled: bool,
    pub stroke_enabled: bool,
    pub stroke_width: f32,
    pub dashed: bool,
    pub corner_radius: f32,
    pub sides: u32,
    pub star_inner: f32,
    pub face: FontFace,
    pub font_size: f32,
    pub align: Align,
    pub tone_range: ToneRange,
    pub exposure: f32,
    pub sponge_saturate: bool,
    pub strength: f32,
    pub crop_delete_pixels: bool,
}

impl Default for ToolOptions {
    fn default() -> Self {
        ToolOptions {
            tip: BrushTip::default(),
            opacity: 1.0,
            flow: 1.0,
            brush_blend: BlendMode::Normal,
            eraser_tip: BrushTip { size: 40.0, hardness: 0.6, ..BrushTip::default() },
            select_op: SelectOp::Replace,
            feather: 0.0,
            tolerance: 32,
            contiguous: true,
            anti_alias: true,
            sample_merged: false,
            gradient_preset: 0,
            gradient_shape: GradientShape::Linear,
            gradient_reverse: false,
            gradient_blend: BlendMode::Normal,
            gradient_opacity: 1.0,
            fill_enabled: true,
            stroke_enabled: false,
            stroke_width: 4.0,
            dashed: false,
            corner_radius: 24.0,
            sides: 5,
            star_inner: 0.5,
            face: FontFace::Sans,
            font_size: 64.0,
            align: Align::Left,
            tone_range: ToneRange::Midtones,
            exposure: 0.5,
            sponge_saturate: false,
            strength: 0.5,
            crop_delete_pixels: true,
        }
    }
}
