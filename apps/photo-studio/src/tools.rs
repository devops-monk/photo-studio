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
    MagneticLasso,
    QuickSelect,
    ObjectSelect,
    MagicWand,
    Crop,
    Eyedropper,
    Ruler,
    SpotHeal,
    Remove,
    Heal,
    Patch,
    ContentMove,
    RedEye,
    Brush,
    Pencil,
    MixerBrush,
    CloneStamp,
    PatternStamp,
    HistoryBrush,
    Eraser,
    BackgroundEraser,
    MagicEraser,
    Gradient,
    Bucket,
    Blur,
    Sharpen,
    Smudge,
    Liquify,
    Dodge,
    Burn,
    Sponge,
    Text,
    Pen,
    PathSelect,
    DirectSelect,
    Rectangle,
    RoundedRect,
    Ellipse,
    Polygon,
    Star,
    Triangle,
    Heart,
    Arrow,
    SpeechBubble,
    Checkmark,
    Line,
    Hand,
    RotateView,
    Zoom,
}

/// Toolbar groups; the first tool in each is the group's default.
pub const GROUPS: &[&[Tool]] = &[
    &[Tool::Move],
    &[Tool::RectMarquee, Tool::EllipseMarquee],
    &[Tool::Lasso, Tool::PolyLasso, Tool::MagneticLasso],
    &[Tool::ObjectSelect, Tool::QuickSelect, Tool::MagicWand],
    &[Tool::Crop],
    &[Tool::Eyedropper, Tool::Ruler],
    &[Tool::SpotHeal, Tool::Remove, Tool::Heal, Tool::Patch, Tool::ContentMove, Tool::RedEye],
    &[Tool::Brush, Tool::Pencil, Tool::MixerBrush],
    &[Tool::CloneStamp, Tool::PatternStamp],
    &[Tool::HistoryBrush],
    &[Tool::Eraser, Tool::BackgroundEraser, Tool::MagicEraser],
    &[Tool::Gradient, Tool::Bucket],
    &[Tool::Blur, Tool::Sharpen, Tool::Smudge, Tool::Liquify],
    &[Tool::Dodge, Tool::Burn, Tool::Sponge],
    &[Tool::Text],
    &[Tool::Pen, Tool::PathSelect, Tool::DirectSelect],
    &[
        Tool::Rectangle,
        Tool::RoundedRect,
        Tool::Ellipse,
        Tool::Triangle,
        Tool::Polygon,
        Tool::Star,
        Tool::Line,
        Tool::Heart,
        Tool::Arrow,
        Tool::SpeechBubble,
        Tool::Checkmark,
    ],
    &[Tool::Hand, Tool::RotateView],
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
            Self::MagneticLasso => "Magnetic Lasso",
            Self::QuickSelect => "Quick Selection",
            Self::ObjectSelect => "Object Selection",
            Self::MagicWand => "Magic Wand",
            Self::Crop => "Crop",
            Self::Eyedropper => "Eyedropper",
            Self::Ruler => "Ruler",
            Self::RedEye => "Red Eye",
            Self::PatternStamp => "Pattern Stamp",
            Self::HistoryBrush => "History Brush",
            Self::BackgroundEraser => "Background Eraser",
            Self::MagicEraser => "Magic Eraser",
            Self::Triangle => "Triangle",
            Self::Heart => "Heart",
            Self::Arrow => "Arrow",
            Self::SpeechBubble => "Speech Bubble",
            Self::Checkmark => "Checkmark",
            Self::SpotHeal => "Spot Healing Brush",
            Self::Remove => "Remove",
            Self::Patch => "Patch",
            Self::ContentMove => "Content-Aware Move",
            Self::Heal => "Healing Brush",
            Self::Brush => "Brush",
            Self::Pencil => "Pencil",
            Self::MixerBrush => "Mixer Brush",
            Self::CloneStamp => "Clone Stamp",
            Self::Eraser => "Eraser",
            Self::Gradient => "Gradient",
            Self::Bucket => "Paint Bucket",
            Self::Blur => "Blur",
            Self::Sharpen => "Sharpen",
            Self::Smudge => "Smudge",
            Self::Liquify => "Liquify (Forward Warp)",
            Self::Dodge => "Dodge",
            Self::Burn => "Burn",
            Self::Sponge => "Sponge",
            Self::Text => "Type",
            Self::Pen => "Pen",
            Self::DirectSelect => "Direct Selection",
            Self::PathSelect => "Path Selection",
            Self::Rectangle => "Rectangle",
            Self::RoundedRect => "Rounded Rectangle",
            Self::Ellipse => "Ellipse",
            Self::Polygon => "Polygon",
            Self::Star => "Star",
            Self::Line => "Line",
            Self::Hand => "Hand",
            Self::RotateView => "Rotate View",
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
            Self::MagneticLasso => ph::MAGNET,
            Self::QuickSelect => ph::SPARKLE,
            Self::ObjectSelect => ph::USER_FOCUS,
            Self::MagicWand => ph::MAGIC_WAND,
            Self::Crop => ph::CROP,
            Self::Eyedropper => ph::EYEDROPPER,
            Self::Ruler => ph::RULER,
            Self::RedEye => ph::EYE,
            Self::PatternStamp => ph::CHECKERBOARD,
            Self::HistoryBrush => ph::CLOCK_COUNTER_CLOCKWISE,
            Self::BackgroundEraser => ph::SELECTION_BACKGROUND,
            Self::MagicEraser => ph::BROOM,
            Self::Triangle => ph::TRIANGLE,
            Self::Heart => ph::HEART,
            Self::Arrow => ph::ARROW_FAT_RIGHT,
            Self::SpeechBubble => ph::CHAT_TEARDROP,
            Self::Checkmark => ph::CHECK_FAT,
            Self::SpotHeal | Self::Heal => ph::BANDAIDS,
            Self::Remove => ph::SCRIBBLE_LOOP,
            Self::Patch => ph::PUZZLE_PIECE,
            Self::ContentMove => ph::ARROWS_OUT,
            Self::Brush => ph::PAINT_BRUSH,
            Self::Pencil => ph::PENCIL_SIMPLE,
            Self::MixerBrush => ph::PAINT_BRUSH_HOUSEHOLD,
            Self::CloneStamp => ph::STAMP,
            Self::Eraser => ph::ERASER,
            Self::Gradient => ph::GRADIENT,
            Self::Bucket => ph::PAINT_BUCKET,
            Self::Blur => ph::DROP,
            Self::Sharpen => ph::DIAMOND,
            Self::Smudge => ph::HAND_POINTING,
            Self::Liquify => ph::WAVES,
            Self::Dodge => ph::SUN_DIM,
            Self::Burn => ph::FIRE,
            Self::Sponge => ph::DROP_HALF,
            Self::Text => ph::TEXT_T,
            Self::Pen => ph::PEN_NIB,
            Self::DirectSelect => ph::CURSOR,
            Self::PathSelect => ph::NAVIGATION_ARROW,
            Self::Rectangle => ph::RECTANGLE,
            Self::RoundedRect => ph::SQUARE,
            Self::Ellipse => ph::CIRCLE,
            Self::Polygon => ph::HEXAGON,
            Self::Star => ph::STAR,
            Self::Line => ph::LINE_SEGMENT,
            Self::Hand => ph::HAND,
            Self::RotateView => ph::ARROWS_CLOCKWISE,
            Self::Zoom => ph::MAGNIFYING_GLASS,
        }
    }

    /// Single-key shortcut; pressing it repeatedly with Shift cycles the group.
    pub fn key(&self) -> Option<egui::Key> {
        use egui::Key;
        Some(match self {
            // Like other editors, the blur/smudge group has no single-key shortcut.
            Self::Blur | Self::Sharpen | Self::Smudge | Self::Liquify => return None,
            Self::Move => Key::V,
            Self::RectMarquee | Self::EllipseMarquee => Key::M,
            Self::Lasso | Self::PolyLasso | Self::MagneticLasso => Key::L,
            Self::QuickSelect | Self::MagicWand | Self::ObjectSelect => Key::W,
            Self::Crop => Key::C,
            Self::Eyedropper | Self::Ruler => Key::I,
            Self::SpotHeal | Self::Heal | Self::RedEye | Self::Remove | Self::Patch | Self::ContentMove => Key::J,
            Self::Brush | Self::Pencil | Self::MixerBrush => Key::B,
            Self::CloneStamp | Self::PatternStamp => Key::S,
            Self::HistoryBrush => Key::Y,
            Self::Eraser | Self::BackgroundEraser | Self::MagicEraser => Key::E,
            Self::Gradient | Self::Bucket => Key::G,
            Self::RotateView => Key::R,
            Self::Dodge | Self::Burn | Self::Sponge => Key::O,
            Self::Text => Key::T,
            Self::Pen => Key::P,
            Self::DirectSelect | Self::PathSelect => Key::A,
            Self::Rectangle
            | Self::RoundedRect
            | Self::Ellipse
            | Self::Triangle
            | Self::Polygon
            | Self::Star
            | Self::Line
            | Self::Heart
            | Self::Arrow
            | Self::SpeechBubble
            | Self::Checkmark => Key::U,
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
                | Self::MixerBrush
                | Self::Eraser
                | Self::CloneStamp
                | Self::PatternStamp
                | Self::HistoryBrush
                | Self::BackgroundEraser
                | Self::SpotHeal
                | Self::Heal
                | Self::Blur
                | Self::Sharpen
                | Self::Smudge
                | Self::Liquify
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
            Self::Triangle => ShapeKind::Triangle,
            Self::Heart => ShapeKind::Heart,
            Self::Arrow => ShapeKind::Arrow,
            Self::SpeechBubble => ShapeKind::SpeechBubble,
            Self::Checkmark => ShapeKind::Checkmark,
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
            Self::MagneticLasso => "Trace roughly along an edge; the outline snaps to it",
            Self::PolyLasso => "Click to add points · double-click or click the start to close · Esc cancels",
            Self::QuickSelect => "Paint over the area to select · Alt to subtract",
            Self::ObjectSelect => "Drag a box around an object to select it · Shift adds, Alt subtracts",
            Self::MagicWand => "Click to select similar colors",
            Self::Crop => "Drag a crop box · Enter to apply · Esc to cancel",
            Self::Eyedropper => "Click to pick the foreground color · Alt for background",
            Self::SpotHeal => "Paint over blemishes to blend them away",
            Self::Remove => "Paint over an object to remove it; the area is filled from its surroundings",
            Self::Patch => "Outline an area, then drag it onto clean texture to patch it",
            Self::ContentMove => "Outline an object, then drag it; the gap fills itself",
            Self::Heal | Self::CloneStamp => "Alt-click to set the source, then paint",
            Self::Brush | Self::Pencil => "Paint · [ and ] change size · Shift-click draws a straight line",
            Self::MixerBrush => "Paint that mixes with the colours already on the canvas",
            Self::Eraser => "Erase to transparency",
            Self::BackgroundEraser => "Erases the color under the centre of the brush, sparing edges",
            Self::MagicEraser => "Click to erase similar connected colors",
            Self::RedEye => "Drag a box around an eye to remove red-eye",
            Self::PatternStamp => "Paint with the chosen pattern",
            Self::HistoryBrush => "Paint back the image as it was when opened",
            Self::Ruler => "Drag to measure distance and angle · Straighten levels the image",
            Self::Gradient => "Drag to draw a gradient",
            Self::Bucket => "Click to fill similar colors",
            Self::Blur | Self::Sharpen | Self::Smudge => "Paint to soften, sharpen or smear",
            Self::Liquify => "Drag to push pixels along · lower strength for subtle reshaping",
            Self::Dodge | Self::Burn | Self::Sponge => "Paint to lighten, darken or change saturation",
            Self::Text => "Click to add text, or click existing text to edit it",
            Self::Pen => "Click for corners, drag for curves · click the first point to close · Enter finishes",
            Self::DirectSelect => "Drag anchors and handles of the selected path",
            Self::PathSelect => "Click a path to select it; drag to move the whole path",
            Self::Rectangle
            | Self::RoundedRect
            | Self::Ellipse
            | Self::Triangle
            | Self::Polygon
            | Self::Star
            | Self::Line
            | Self::Heart
            | Self::Arrow
            | Self::SpeechBubble
            | Self::Checkmark => "Drag to draw a shape layer · Shift constrains",
            Self::Hand => "Drag to pan · double-click to fit",
            Self::RotateView => "Drag to turn the view · Shift snaps to 15° · double-click or Esc resets",
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
    pub pattern: usize,
    pub red_eye_darken: f32,
    pub vertical_text: bool,
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
            pattern: 0,
            red_eye_darken: 0.5,
            vertical_text: false,
        }
    }
}
