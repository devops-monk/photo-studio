//! The application menu bar.

use egui::Ui;
use studio_core::adjust::Adjustment;
use studio_core::filters::{Filter, FilterGroup};
use studio_core::transform::Rotation;

use crate::actions::{Action, ModifyKind, shortcut_text};
use crate::app::App;

/// Human-readable name of an action (menus and the shortcuts sheet).
pub fn action_label(a: &Action) -> String {
    match a {
        Action::New => "New…".into(),
        Action::NewFromClipboard => "New from Clipboard".into(),
        Action::Open => "Open…".into(),
        Action::OpenPath(p) => p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        Action::Close => "Close".into(),
        Action::Save => "Save".into(),
        Action::SaveAs => "Save As…".into(),
        Action::ExportAs => "Export As…".into(),
        Action::QuickExportPng => "Quick Export as PNG…".into(),
        Action::Revert => "Revert".into(),
        Action::Quit => "Quit".into(),
        Action::Undo => "Undo".into(),
        Action::Redo => "Redo".into(),
        Action::StepBackward => "Step Backward".into(),
        Action::Cut => "Cut".into(),
        Action::Copy => "Copy".into(),
        Action::CopyMerged => "Copy Merged".into(),
        Action::Paste => "Paste".into(),
        Action::PasteInPlace => "Paste in Place".into(),
        Action::PasteInto => "Paste Into".into(),
        Action::Align(h, v) => match (h, v) {
            (Some(-1), None) => "Align Left Edges".into(),
            (Some(0), None) => "Align Horizontal Centers".into(),
            (Some(_), None) => "Align Right Edges".into(),
            (None, Some(-1)) => "Align Top Edges".into(),
            (None, Some(0)) => "Align Vertical Centers".into(),
            (None, Some(_)) => "Align Bottom Edges".into(),
            _ => "Center".into(),
        },
        Action::Clear => "Clear".into(),
        Action::Fill => "Fill…".into(),
        Action::Stroke => "Stroke…".into(),
        Action::FreeTransform => "Free Transform".into(),
        Action::DefinePattern => "Define Pattern".into(),
        Action::PathToSelection => "Make Selection from Path".into(),
        Action::FillPath => "Fill Path".into(),
        Action::StrokePath => "Stroke Path".into(),
        Action::PathToShape => "Shape Layer from Path".into(),
        Action::DeletePath => "Delete Path".into(),
        Action::Straighten => "Straighten with Ruler".into(),
        Action::FlipLayer(true) => "Flip Horizontal".into(),
        Action::FlipLayer(false) => "Flip Vertical".into(),
        Action::RotateLayer(Rotation::Cw90) => "Rotate 90° Clockwise".into(),
        Action::RotateLayer(Rotation::Ccw90) => "Rotate 90° Counter Clockwise".into(),
        Action::RotateLayer(Rotation::Half) => "Rotate 180°".into(),
        Action::Preferences => "Preferences…".into(),
        Action::Adjust(a) => format!("{}…", a.label()),
        Action::AutoTone => "Auto Tone".into(),
        Action::AutoContrast => "Auto Contrast".into(),
        Action::AutoColor => "Auto Color".into(),
        Action::Equalize => "Equalize".into(),
        Action::ShadowsHighlights => "Shadows/Highlights…".into(),
        Action::ImageSize => "Image Size…".into(),
        Action::CanvasSize => "Canvas Size…".into(),
        Action::RotateCanvas(Rotation::Cw90) => "90° Clockwise".into(),
        Action::RotateCanvas(Rotation::Ccw90) => "90° Counter Clockwise".into(),
        Action::RotateCanvas(Rotation::Half) => "180°".into(),
        Action::RotateArbitrary => "Arbitrary…".into(),
        Action::FlipCanvas(true) => "Flip Canvas Horizontal".into(),
        Action::FlipCanvas(false) => "Flip Canvas Vertical".into(),
        Action::CropToSelection => "Crop".into(),
        Action::Trim => "Trim Transparent Pixels".into(),
        Action::DuplicateImage => "Duplicate Image".into(),
        Action::NewLayer => "New Layer".into(),
        Action::NewFillSolid => "Solid Color".into(),
        Action::NewFillGradient => "Gradient".into(),
        Action::NewAdjustmentLayer(a) => a.label().into(),
        Action::DuplicateLayer => "Duplicate Layer".into(),
        Action::DeleteLayer => "Delete Layer".into(),
        Action::LayerViaCopy => "Layer via Copy".into(),
        Action::LayerViaCut => "Layer via Cut".into(),
        Action::LayerStyle => "Layer Style…".into(),
        Action::ClearLayerStyle => "Clear Layer Style".into(),
        Action::MaskRevealAll => "Reveal All".into(),
        Action::MaskHideAll => "Hide All".into(),
        Action::MaskFromSelection => "Reveal Selection".into(),
        Action::MaskApply => "Apply".into(),
        Action::MaskDelete => "Delete".into(),
        Action::MaskToggle => "Disable / Enable".into(),
        Action::MaskInvert => "Invert".into(),
        Action::ToggleClipping => "Create / Release Clipping Mask".into(),
        Action::GroupLayers => "Group Layers".into(),
        Action::Ungroup => "Ungroup Layers".into(),
        Action::NewGroup => "New Group".into(),
        Action::Rasterize => "Rasterize Layer".into(),
        Action::ConvertToSmart => "Convert to Smart Object".into(),
        Action::BringToFront => "Bring to Front".into(),
        Action::BringForward => "Bring Forward".into(),
        Action::SendBackward => "Send Backward".into(),
        Action::SendToBack => "Send to Back".into(),
        Action::MergeDown => "Merge Down".into(),
        Action::MergeVisible => "Merge Visible".into(),
        Action::Flatten => "Flatten Image".into(),
        Action::SelectLayerAbove => "Select Layer Above".into(),
        Action::SelectLayerBelow => "Select Layer Below".into(),
        Action::SelectAll => "All".into(),
        Action::Deselect => "Deselect".into(),
        Action::Reselect => "Reselect".into(),
        Action::InverseSelection => "Inverse".into(),
        Action::SelectSubject => "Subject".into(),
        Action::ColorRange => "Color Range…".into(),
        Action::LoadLayerSelection => "Load Layer Transparency".into(),
        Action::ToggleQuickMask => "Edit in Quick Mask Mode".into(),
        Action::SaveSelection => "Save Selection".into(),
        Action::LoadChannel(_) => "Load Selection".into(),
        Action::DeleteChannel(_) => "Delete Channel".into(),
        Action::Modify(k) => format!("{}…", k.label()),
        Action::RepeatFilter => "Repeat Last Filter".into(),
        Action::Filter(f) => {
            if f.has_params() {
                format!("{}…", f.label())
            } else {
                f.label().into()
            }
        }
        Action::ZoomIn => "Zoom In".into(),
        Action::ZoomOut => "Zoom Out".into(),
        Action::FitOnScreen => "Fit on Screen".into(),
        Action::ActualPixels => "100%".into(),
        Action::TogglePixelGrid => "Pixel Grid".into(),
        Action::ToggleRulers => "Rulers".into(),
        Action::ToggleGuides => "Guides".into(),
        Action::ToggleGrid => "Grid".into(),
        Action::ToggleSnap => "Snap".into(),
        Action::NewGuide => "New Guide…".into(),
        Action::ClearGuides => "Clear Guides".into(),
        Action::CommandPalette => "Search Commands…".into(),
        Action::ToggleSelectionEdges => "Selection Edges".into(),
        Action::ToggleTheme => "Toggle Light / Dark".into(),
        Action::About => "About Photo Studio".into(),
        Action::Shortcuts => "Keyboard Shortcuts".into(),
        Action::Website => "Photo Studio Website".into(),
        Action::SwapColors => "Swap Colors".into(),
        Action::DefaultColors => "Default Colors".into(),
        Action::BrushSmaller => "Decrease Brush Size".into(),
        Action::BrushLarger => "Increase Brush Size".into(),
    }
}

fn item(ui: &mut Ui, out: &mut Vec<Action>, a: Action, enabled: bool) {
    let sc = shortcut_text(ui.ctx(), &a);
    let r = ui.add_enabled(enabled, egui::Button::new(action_label(&a)).shortcut_text(sc));
    if r.clicked() {
        out.push(a);
        ui.close();
    }
}

fn check(ui: &mut Ui, out: &mut Vec<Action>, a: Action, on: bool) {
    let sc = shortcut_text(ui.ctx(), &a);
    let label = format!("{}  {}", if on { "✓" } else { "    " }, action_label(&a));
    if ui.add(egui::Button::new(label).shortcut_text(sc)).clicked() {
        out.push(a);
        ui.close();
    }
}

pub fn bar(app: &App, ui: &mut Ui, out: &mut Vec<Action>) {
    let doc = app.tab().is_some();
    let tab = app.tab();
    let has_sel = tab.is_some_and(|t| t.doc.selection.is_some());
    let multi = tab.is_some_and(|t| t.doc.layers.len() > 1);
    let can_undo = tab.is_some_and(|t| t.history.can_undo());
    let can_redo = tab.is_some_and(|t| t.history.can_redo());
    let has_mask = tab.and_then(|t| t.doc.active_layer()).is_some_and(|l| l.mask.is_some());

    egui::MenuBar::new().ui(ui, |ui| {
        ui.menu_button("File", |ui| {
            item(ui, out, Action::New, true);
            item(ui, out, Action::NewFromClipboard, true);
            item(ui, out, Action::Open, true);
            ui.menu_button("Open Recent", |ui| {
                if app.prefs.recent.is_empty() {
                    ui.add_enabled(false, egui::Button::new("No recent files"));
                }
                for p in &app.prefs.recent {
                    if ui.button(p.display().to_string()).clicked() {
                        out.push(Action::OpenPath(p.clone()));
                        ui.close();
                    }
                }
            });
            ui.separator();
            item(ui, out, Action::Close, doc);
            item(ui, out, Action::Save, doc);
            item(ui, out, Action::SaveAs, doc);
            item(ui, out, Action::Revert, doc && can_undo);
            ui.separator();
            item(ui, out, Action::ExportAs, doc);
            item(ui, out, Action::QuickExportPng, doc);
            ui.separator();
            item(ui, out, Action::Quit, true);
        });
        ui.menu_button("Edit", |ui| {
            let undo =
                tab.and_then(|t| t.history.undo_label()).map(|l| format!("Undo {l}")).unwrap_or_else(|| "Undo".into());
            let redo =
                tab.and_then(|t| t.history.redo_label()).map(|l| format!("Redo {l}")).unwrap_or_else(|| "Redo".into());
            if ui
                .add_enabled(can_undo, egui::Button::new(undo).shortcut_text(shortcut_text(ui.ctx(), &Action::Undo)))
                .clicked()
            {
                out.push(Action::Undo);
                ui.close();
            }
            if ui
                .add_enabled(can_redo, egui::Button::new(redo).shortcut_text(shortcut_text(ui.ctx(), &Action::Redo)))
                .clicked()
            {
                out.push(Action::Redo);
                ui.close();
            }
            ui.separator();
            item(ui, out, Action::Cut, doc);
            item(ui, out, Action::Copy, doc);
            item(ui, out, Action::CopyMerged, doc);
            item(ui, out, Action::Paste, true);
            item(ui, out, Action::PasteInPlace, doc);
            item(ui, out, Action::PasteInto, has_sel);
            item(ui, out, Action::Clear, doc);
            ui.separator();
            item(ui, out, Action::Fill, doc);
            item(ui, out, Action::Stroke, has_sel);
            ui.separator();
            item(ui, out, Action::FreeTransform, doc);
            item(ui, out, Action::DefinePattern, has_sel);
            ui.menu_button("Transform", |ui| {
                item(ui, out, Action::RotateLayer(Rotation::Half), doc);
                item(ui, out, Action::RotateLayer(Rotation::Cw90), doc);
                item(ui, out, Action::RotateLayer(Rotation::Ccw90), doc);
                ui.separator();
                item(ui, out, Action::FlipLayer(true), doc);
                item(ui, out, Action::FlipLayer(false), doc);
            });
            ui.separator();
            item(ui, out, Action::Preferences, true);
        });
        ui.menu_button("Image", |ui| {
            ui.menu_button("Adjustments", |ui| {
                for a in Adjustment::catalog() {
                    item(ui, out, Action::Adjust(a), doc);
                }
                ui.separator();
                item(ui, out, Action::ShadowsHighlights, doc);
                item(ui, out, Action::Equalize, doc);
            });
            ui.separator();
            item(ui, out, Action::AutoTone, doc);
            item(ui, out, Action::AutoContrast, doc);
            item(ui, out, Action::AutoColor, doc);
            ui.separator();
            item(ui, out, Action::ImageSize, doc);
            item(ui, out, Action::CanvasSize, doc);
            ui.menu_button("Image Rotation", |ui| {
                item(ui, out, Action::RotateCanvas(Rotation::Half), doc);
                item(ui, out, Action::RotateCanvas(Rotation::Cw90), doc);
                item(ui, out, Action::RotateCanvas(Rotation::Ccw90), doc);
                item(ui, out, Action::RotateArbitrary, doc);
                ui.separator();
                item(ui, out, Action::FlipCanvas(true), doc);
                item(ui, out, Action::FlipCanvas(false), doc);
            });
            item(ui, out, Action::CropToSelection, doc);
            item(ui, out, Action::Trim, doc);
            ui.separator();
            item(ui, out, Action::DuplicateImage, doc);
        });
        ui.menu_button("Layer", |ui| {
            item(ui, out, Action::NewLayer, doc);
            ui.menu_button("New Fill Layer", |ui| {
                item(ui, out, Action::NewFillSolid, doc);
                item(ui, out, Action::NewFillGradient, doc);
            });
            ui.menu_button("New Adjustment Layer", |ui| {
                for a in Adjustment::catalog() {
                    item(ui, out, Action::NewAdjustmentLayer(a), doc);
                }
            });
            item(ui, out, Action::NewGroup, doc);
            item(ui, out, Action::DuplicateLayer, doc);
            item(ui, out, Action::DeleteLayer, doc && multi);
            ui.separator();
            item(ui, out, Action::GroupLayers, doc);
            item(ui, out, Action::Ungroup, doc && tab.and_then(|t| t.doc.active_layer()).is_some_and(|l| l.is_group()));
            ui.separator();
            item(ui, out, Action::LayerViaCopy, doc);
            item(ui, out, Action::LayerViaCut, doc);
            ui.separator();
            ui.menu_button("Layer Style", |ui| {
                item(ui, out, Action::LayerStyle, doc);
                item(ui, out, Action::ClearLayerStyle, doc);
            });
            ui.menu_button("Layer Mask", |ui| {
                item(ui, out, Action::MaskRevealAll, doc);
                item(ui, out, Action::MaskHideAll, doc);
                item(ui, out, Action::MaskFromSelection, has_sel);
                ui.separator();
                item(ui, out, Action::MaskInvert, has_mask);
                item(ui, out, Action::MaskToggle, has_mask);
                item(ui, out, Action::MaskApply, has_mask);
                item(ui, out, Action::MaskDelete, has_mask);
            });
            item(ui, out, Action::ToggleClipping, doc);
            item(ui, out, Action::ConvertToSmart, doc);
            item(ui, out, Action::Rasterize, doc);
            ui.separator();
            ui.menu_button(if has_sel { "Align to Selection" } else { "Align to Canvas" }, |ui| {
                for (h, v) in [
                    (Some(-1), None),
                    (Some(0), None),
                    (Some(1), None),
                    (None, Some(-1)),
                    (None, Some(0)),
                    (None, Some(1)),
                ] {
                    item(ui, out, Action::Align(h, v), doc);
                }
            });
            ui.menu_button("Arrange", |ui| {
                item(ui, out, Action::BringToFront, doc);
                item(ui, out, Action::BringForward, doc);
                item(ui, out, Action::SendBackward, doc);
                item(ui, out, Action::SendToBack, doc);
            });
            ui.separator();
            item(ui, out, Action::MergeDown, doc && multi);
            item(ui, out, Action::MergeVisible, doc && multi);
            item(ui, out, Action::Flatten, doc);
        });
        ui.menu_button("Select", |ui| {
            item(ui, out, Action::SelectAll, doc);
            item(ui, out, Action::Deselect, has_sel);
            item(ui, out, Action::Reselect, doc && app.last_selection.is_some());
            item(ui, out, Action::InverseSelection, doc);
            ui.separator();
            item(ui, out, Action::SelectSubject, doc);
            item(ui, out, Action::ColorRange, doc);
            item(ui, out, Action::LoadLayerSelection, doc);
            item(ui, out, Action::SaveSelection, has_sel);
            ui.menu_button("Load Selection", |ui| {
                let channels: Vec<String> =
                    tab.map(|t| t.doc.channels.iter().map(|c| c.0.clone()).collect()).unwrap_or_default();
                if channels.is_empty() {
                    ui.add_enabled(false, egui::Button::new("No saved selections"));
                }
                for (i, n) in channels.iter().enumerate() {
                    if ui.button(n).clicked() {
                        out.push(Action::LoadChannel(i));
                        ui.close();
                    }
                }
            });
            check(ui, out, Action::ToggleQuickMask, app.quick_mask);
            ui.separator();
            ui.menu_button("Modify", |ui| {
                for k in [
                    ModifyKind::Border,
                    ModifyKind::Smooth,
                    ModifyKind::Expand,
                    ModifyKind::Contract,
                    ModifyKind::Feather,
                ] {
                    item(ui, out, Action::Modify(k), has_sel);
                }
            });
        });
        ui.menu_button("Filter", |ui| {
            let last = app
                .last_filter
                .as_ref()
                .map(|f| format!("Repeat {}", f.label()))
                .unwrap_or_else(|| "Repeat Last Filter".into());
            if ui
                .add_enabled(
                    doc && app.last_filter.is_some(),
                    egui::Button::new(last).shortcut_text(shortcut_text(ui.ctx(), &Action::RepeatFilter)),
                )
                .clicked()
            {
                out.push(Action::RepeatFilter);
                ui.close();
            }
            ui.separator();
            for g in FilterGroup::ALL {
                ui.menu_button(g.label(), |ui| {
                    for f in Filter::catalog().into_iter().filter(|f| f.group() == g) {
                        item(ui, out, Action::Filter(f), doc);
                    }
                });
            }
        });
        ui.menu_button("View", |ui| {
            item(ui, out, Action::ZoomIn, doc);
            item(ui, out, Action::ZoomOut, doc);
            item(ui, out, Action::FitOnScreen, doc);
            item(ui, out, Action::ActualPixels, doc);
            ui.separator();
            check(ui, out, Action::ToggleRulers, app.prefs.show_rulers);
            check(ui, out, Action::ToggleGuides, app.prefs.show_guides);
            check(ui, out, Action::ToggleGrid, app.prefs.show_grid);
            check(ui, out, Action::ToggleSnap, app.prefs.snap);
            item(ui, out, Action::NewGuide, doc);
            item(ui, out, Action::ClearGuides, doc && tab.is_some_and(|t| !t.doc.guides.is_empty()));
            ui.separator();
            check(ui, out, Action::TogglePixelGrid, app.prefs.pixel_grid);
            check(ui, out, Action::ToggleSelectionEdges, app.show_edges);
            ui.separator();
            item(ui, out, Action::ToggleTheme, true);
        });
        ui.menu_button("Help", |ui| {
            item(ui, out, Action::CommandPalette, true);
            item(ui, out, Action::Shortcuts, true);
            item(ui, out, Action::Website, true);
            ui.separator();
            item(ui, out, Action::About, true);
        });
    });
}

/// Everything the command palette can run.
pub fn all_commands() -> Vec<Action> {
    use studio_core::transform::Rotation;
    let mut v = vec![
        Action::New,
        Action::NewFromClipboard,
        Action::Open,
        Action::Close,
        Action::Save,
        Action::SaveAs,
        Action::ExportAs,
        Action::QuickExportPng,
        Action::Revert,
        Action::Undo,
        Action::Redo,
        Action::Cut,
        Action::Copy,
        Action::CopyMerged,
        Action::Paste,
        Action::PasteInPlace,
        Action::PasteInto,
        Action::Align(Some(-1), None),
        Action::Align(Some(0), None),
        Action::Align(Some(1), None),
        Action::Align(None, Some(-1)),
        Action::Align(None, Some(0)),
        Action::Align(None, Some(1)),
        Action::Clear,
        Action::Fill,
        Action::Stroke,
        Action::FreeTransform,
        Action::DefinePattern,
        Action::PathToSelection,
        Action::FillPath,
        Action::StrokePath,
        Action::PathToShape,
        Action::DeletePath,
        Action::FlipLayer(true),
        Action::FlipLayer(false),
        Action::RotateLayer(Rotation::Cw90),
        Action::RotateLayer(Rotation::Ccw90),
        Action::RotateLayer(Rotation::Half),
        Action::Preferences,
        Action::AutoTone,
        Action::AutoContrast,
        Action::AutoColor,
        Action::Equalize,
        Action::ShadowsHighlights,
        Action::ImageSize,
        Action::CanvasSize,
        Action::RotateCanvas(Rotation::Cw90),
        Action::RotateCanvas(Rotation::Ccw90),
        Action::RotateCanvas(Rotation::Half),
        Action::RotateArbitrary,
        Action::FlipCanvas(true),
        Action::FlipCanvas(false),
        Action::CropToSelection,
        Action::Trim,
        Action::DuplicateImage,
        Action::NewLayer,
        Action::NewGroup,
        Action::GroupLayers,
        Action::Ungroup,
        Action::NewFillSolid,
        Action::NewFillGradient,
        Action::DuplicateLayer,
        Action::DeleteLayer,
        Action::LayerViaCopy,
        Action::LayerViaCut,
        Action::LayerStyle,
        Action::ClearLayerStyle,
        Action::MaskRevealAll,
        Action::MaskHideAll,
        Action::MaskFromSelection,
        Action::MaskApply,
        Action::MaskDelete,
        Action::MaskToggle,
        Action::MaskInvert,
        Action::ToggleClipping,
        Action::Rasterize,
        Action::ConvertToSmart,
        Action::BringToFront,
        Action::BringForward,
        Action::SendBackward,
        Action::SendToBack,
        Action::MergeDown,
        Action::MergeVisible,
        Action::Flatten,
        Action::SelectAll,
        Action::Deselect,
        Action::Reselect,
        Action::InverseSelection,
        Action::SelectSubject,
        Action::ColorRange,
        Action::LoadLayerSelection,
        Action::ToggleQuickMask,
        Action::SaveSelection,
        Action::Modify(ModifyKind::Feather),
        Action::Modify(ModifyKind::Expand),
        Action::Modify(ModifyKind::Contract),
        Action::Modify(ModifyKind::Border),
        Action::Modify(ModifyKind::Smooth),
        Action::RepeatFilter,
        Action::ZoomIn,
        Action::ZoomOut,
        Action::FitOnScreen,
        Action::ActualPixels,
        Action::ToggleRulers,
        Action::ToggleGuides,
        Action::ToggleGrid,
        Action::ToggleSnap,
        Action::NewGuide,
        Action::ClearGuides,
        Action::TogglePixelGrid,
        Action::ToggleSelectionEdges,
        Action::ToggleTheme,
        Action::Straighten,
        Action::About,
        Action::Shortcuts,
        Action::Website,
    ];
    for a in Adjustment::catalog() {
        v.push(Action::Adjust(a.clone()));
        v.push(Action::NewAdjustmentLayer(a));
    }
    for f in Filter::catalog() {
        v.push(Action::Filter(f));
    }
    v
}

/// Label shown in the palette, with context for commands whose short name is ambiguous.
pub fn palette_label(a: &Action) -> String {
    match a {
        Action::Adjust(x) => format!("Image › Adjustments › {}", x.label()),
        Action::NewAdjustmentLayer(x) => format!("New Adjustment Layer › {}", x.label()),
        Action::Filter(f) => format!("Filter › {} › {}", f.group().label(), f.label()),
        Action::Modify(_) => format!("Select › Modify › {}", action_label(a)),
        Action::RotateCanvas(_) => format!("Image Rotation › {}", action_label(a)),
        Action::FlipLayer(_) | Action::RotateLayer(_) => format!("Transform › {}", action_label(a)),
        Action::NewFillSolid | Action::NewFillGradient => format!("New Fill Layer › {}", action_label(a)),
        _ => action_label(a).trim_end_matches('…').to_string(),
    }
}

/// Subsequence match score (higher is better), or None.
pub fn fuzzy(query: &str, text: &str) -> Option<i32> {
    let q: Vec<char> = query.to_lowercase().chars().filter(|c| !c.is_whitespace()).collect();
    if q.is_empty() {
        return Some(0);
    }
    let t: Vec<char> = text.to_lowercase().chars().collect();
    let mut score = 0;
    let mut qi = 0;
    let mut last = None;
    for (i, &c) in t.iter().enumerate() {
        if qi < q.len() && c == q[qi] {
            score += if last == Some(i.wrapping_sub(1)) { 6 } else { 1 };
            if i == 0 || !t[i - 1].is_alphanumeric() {
                score += 4;
            }
            last = Some(i);
            qi += 1;
        }
    }
    (qi == q.len()).then(|| score - t.len() as i32 / 8)
}
