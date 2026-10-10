//! Every command in the app, with its keyboard shortcut.

use egui::{Key, KeyboardShortcut, Modifiers};
use studio_core::adjust::Adjustment;
use studio_core::filters::Filter;
use studio_core::transform::Rotation;

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Action {
    // File
    New,
    NewFromClipboard,
    Open,
    OpenPath(std::path::PathBuf),
    Close,
    Save,
    SaveAs,
    ExportAs,
    QuickExportPng,
    Revert,
    Quit,
    // Edit
    Undo,
    Redo,
    StepBackward,
    Cut,
    Copy,
    CopyMerged,
    Paste,
    PasteInPlace,
    PasteInto,
    Align(Option<i8>, Option<i8>),
    Clear,
    Fill,
    ContentAwareFill,
    Stroke,
    FreeTransform,
    DefinePattern,
    PathToSelection,
    FillPath,
    StrokePath,
    PathToShape,
    DeletePath,
    Straighten,
    FlipLayer(bool),
    RotateLayer(Rotation),
    Preferences,
    // Image
    Adjust(Adjustment),
    /// Apply with these exact settings, no dialog (recorded in Actions).
    ApplyAdjustment(Adjustment),
    ApplyFilter(Filter),
    ResizeImage(u32, u32),
    StartRecording,
    StopRecording,
    PlayMacro(usize),
    DeleteMacro(usize),
    Batch,
    AutoTone,
    AutoContrast,
    AutoColor,
    Equalize,
    ShadowsHighlights,
    ImageSize,
    CanvasSize,
    RotateCanvas(Rotation),
    RotateArbitrary,
    FlipCanvas(bool),
    CropToSelection,
    Trim,
    DuplicateImage,
    // Layer
    NewLayer,
    NewFillSolid,
    NewFillGradient,
    NewFillPattern,
    NewAdjustmentLayer(Adjustment),
    DuplicateLayer,
    DeleteLayer,
    LayerViaCopy,
    LayerViaCut,
    LayerStyle,
    ClearLayerStyle,
    MaskRevealAll,
    MaskHideAll,
    MaskFromSelection,
    MaskApply,
    MaskDelete,
    MaskToggle,
    MaskInvert,
    ToggleClipping,
    GroupLayers,
    Ungroup,
    NewGroup,
    Rasterize,
    ConvertToSmart,
    BringToFront,
    BringForward,
    SendBackward,
    SendToBack,
    MergeDown,
    MergeVisible,
    Flatten,
    SelectLayerAbove,
    SelectLayerBelow,
    // Select
    SelectAll,
    Deselect,
    Reselect,
    InverseSelection,
    SelectSubject,
    ColorRange,
    LoadLayerSelection,
    ToggleQuickMask,
    SaveSelection,
    LoadChannel(usize),
    DeleteChannel(usize),
    Modify(ModifyKind),
    // Filter
    RepeatFilter,
    Filter(Filter),
    // View
    ZoomIn,
    ZoomOut,
    FitOnScreen,
    ActualPixels,
    TogglePixelGrid,
    ToggleRulers,
    ToggleGuides,
    ToggleGrid,
    ToggleSnap,
    NewGuide,
    ClearGuides,
    CommandPalette,
    ToggleSelectionEdges,
    ToggleTheme,
    // Help
    About,
    Shortcuts,
    Website,
    // Colours
    SwapColors,
    DefaultColors,
    // Brush size
    BrushSmaller,
    BrushLarger,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ModifyKind {
    Feather,
    Expand,
    Contract,
    Border,
    Smooth,
}

impl ModifyKind {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Feather => "Feather",
            Self::Expand => "Expand",
            Self::Contract => "Contract",
            Self::Border => "Border",
            Self::Smooth => "Smooth",
        }
    }
}

const CMD: Modifiers = Modifiers::COMMAND;
const CMD_SHIFT: Modifiers = Modifiers { shift: true, ..Modifiers::COMMAND };
const CMD_ALT: Modifiers = Modifiers { alt: true, ..Modifiers::COMMAND };
const CMD_ALT_SHIFT: Modifiers = Modifiers { alt: true, shift: true, ..Modifiers::COMMAND };
const SHIFT: Modifiers = Modifiers::SHIFT;
const NONE: Modifiers = Modifiers::NONE;

/// Shortcut table. Order matters: more specific modifier combinations come first.
pub fn shortcuts() -> Vec<(KeyboardShortcut, Action)> {
    let s = |m, k, a| (KeyboardShortcut::new(m, k), a);
    vec![
        s(CMD_ALT_SHIFT, Key::E, Action::MergeVisible),
        s(CMD_ALT_SHIFT, Key::S, Action::ExportAs),
        s(CMD_ALT, Key::Z, Action::StepBackward),
        s(CMD_ALT, Key::I, Action::ImageSize),
        s(CMD_ALT, Key::C, Action::CanvasSize),
        s(CMD_ALT, Key::G, Action::ToggleClipping),
        s(CMD_ALT, Key::F, Action::RepeatFilter),
        s(Modifiers::ALT, Key::CloseBracket, Action::SelectLayerAbove),
        s(Modifiers::ALT, Key::OpenBracket, Action::SelectLayerBelow),
        s(CMD_ALT_SHIFT, Key::V, Action::PasteInto),
        s(CMD_SHIFT, Key::Z, Action::Redo),
        s(CMD_SHIFT, Key::V, Action::PasteInPlace),
        s(CMD_SHIFT, Key::N, Action::NewLayer),
        s(CMD_SHIFT, Key::G, Action::Ungroup),
        s(CMD_SHIFT, Key::S, Action::SaveAs),
        s(CMD_SHIFT, Key::I, Action::InverseSelection),
        s(CMD_SHIFT, Key::D, Action::Reselect),
        s(CMD_SHIFT, Key::C, Action::CopyMerged),
        s(CMD_SHIFT, Key::J, Action::LayerViaCut),
        s(CMD_SHIFT, Key::L, Action::AutoTone),
        s(CMD_SHIFT, Key::B, Action::AutoColor),
        s(CMD_SHIFT, Key::U, Action::Adjust(Adjustment::Desaturate)),
        s(CMD_SHIFT, Key::CloseBracket, Action::BringToFront),
        s(CMD_SHIFT, Key::OpenBracket, Action::SendToBack),
        s(CMD_SHIFT, Key::E, Action::QuickExportPng),
        s(CMD_SHIFT, Key::Semicolon, Action::ToggleSnap),
        s(CMD, Key::Enter, Action::PathToSelection),
        s(CMD, Key::N, Action::New),
        s(CMD, Key::O, Action::Open),
        s(CMD, Key::W, Action::Close),
        s(CMD, Key::S, Action::Save),
        s(CMD, Key::Q, Action::Quit),
        s(CMD, Key::Z, Action::Undo),
        s(CMD, Key::Y, Action::Redo),
        s(CMD, Key::X, Action::Cut),
        s(CMD, Key::C, Action::Copy),
        s(CMD, Key::V, Action::Paste),
        s(CMD, Key::T, Action::FreeTransform),
        s(CMD, Key::K, Action::Preferences),
        s(CMD, Key::A, Action::SelectAll),
        s(CMD, Key::D, Action::Deselect),
        s(CMD, Key::J, Action::LayerViaCopy),
        s(CMD, Key::E, Action::MergeDown),
        s(CMD, Key::G, Action::GroupLayers),
        s(CMD, Key::H, Action::ToggleSelectionEdges),
        s(CMD, Key::L, Action::Adjust(Adjustment::catalog()[1].clone())),
        s(CMD, Key::M, Action::Adjust(Adjustment::catalog()[2].clone())),
        s(CMD, Key::U, Action::Adjust(Adjustment::catalog()[5].clone())),
        s(CMD, Key::B, Action::Adjust(Adjustment::catalog()[6].clone())),
        s(CMD, Key::I, Action::Adjust(Adjustment::Invert)),
        s(CMD, Key::Plus, Action::ZoomIn),
        s(CMD, Key::Equals, Action::ZoomIn),
        s(CMD, Key::Minus, Action::ZoomOut),
        s(CMD, Key::Num0, Action::FitOnScreen),
        s(CMD, Key::Num1, Action::ActualPixels),
        s(CMD, Key::CloseBracket, Action::BringForward),
        s(CMD, Key::OpenBracket, Action::SendBackward),
        s(CMD, Key::Quote, Action::ToggleGrid),
        s(CMD, Key::R, Action::ToggleRulers),
        s(CMD, Key::Semicolon, Action::ToggleGuides),
        s(CMD, Key::F, Action::CommandPalette),
        s(SHIFT, Key::F5, Action::Fill),
        s(SHIFT, Key::Backspace, Action::ContentAwareFill),
        s(NONE, Key::X, Action::SwapColors),
        s(NONE, Key::D, Action::DefaultColors),
        s(NONE, Key::OpenBracket, Action::BrushSmaller),
        s(NONE, Key::CloseBracket, Action::BrushLarger),
        s(NONE, Key::F1, Action::Shortcuts),
    ]
}

pub fn shortcut_for(a: &Action) -> Option<KeyboardShortcut> {
    shortcuts().into_iter().find(|(_, x)| x == a).map(|(s, _)| s)
}

pub fn shortcut_text(ctx: &egui::Context, a: &Action) -> String {
    shortcut_for(a).map(|s| ctx.format_shortcut(&s)).unwrap_or_default()
}
