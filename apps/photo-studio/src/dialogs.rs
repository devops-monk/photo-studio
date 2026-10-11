//! Dialogs. Adjustment, filter, colour-range and layer-style dialogs preview live on the canvas.

use egui::{Context, Ui};
use egui_phosphor::regular as ph;
use studio_core::adjust::{self, Adjustment};
use studio_core::blend::BlendMode;
use studio_core::document::{Anchor, DocState, Document};
use studio_core::filters::{Filter, FilterCtx};
use studio_core::image::Image;
use studio_core::layer::LayerKind;
use studio_core::styles::{BevelStyle, LayerStyles, StrokePosition};
use studio_core::transform::Resample;
use studio_core::{Rgba, io, paint, select};

use crate::actions::{Action, ModifyKind, shortcuts};
use crate::app::App;
use crate::doc::DocTab;
use crate::params::{ParamState, adjustment_ui, filter_ui};
use crate::theme::ThemeMode;
use crate::widgets::{label_dim, primary_button, secondary_button, segmented, slider_row, toggle_row};

pub struct Preview {
    base: DocState,
    dirty_at: Option<f64>,
}

impl Preview {
    fn new(doc: &Document) -> Self {
        Preview { base: doc.state(), dirty_at: Some(0.0) }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Background {
    White,
    Black,
    Transparent,
    Foreground,
    Background,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FillWith {
    Foreground,
    Background,
    Black,
    White,
    Gray,
}

pub enum Dialog {
    NewDocument { name: String, w: u32, h: u32, bg: Background, preset: usize },
    Export { format: usize, quality: u8, scale: f32 },
    Adjust { adj: Adjustment, st: ParamState, pv: Preview },
    ShadowsHighlights { shadows: f32, highlights: f32, radius: f32, pv: Preview },
    Filter { filter: Filter, pv: Preview },
    ImageSize { w: u32, h: u32, keep: bool, method: Resample, orig: (u32, u32) },
    CanvasSize { w: u32, h: u32, anchor: Anchor, fill: Background, orig: (u32, u32) },
    RotateArbitrary { angle: f32 },
    Modify { kind: ModifyKind, amount: f32 },
    ColorRange { color: Rgba, fuzziness: u8, pv: Preview },
    Fill { with: FillWith, opacity: f32, blend: BlendMode },
    Stroke { width: f32, position: StrokePosition },
    LayerStyle { styles: LayerStyles, section: usize, pv: Preview },
    NewGuide { horizontal: bool, pos: f32 },
    Batch { action: usize, format: usize },
    EditNote { index: usize, text: String, is_new: bool },
    CommandPalette { query: String, selected: usize },
    Preferences,
    About,
    Shortcuts,
}

const PRESETS: &[(&str, u32, u32)] = &[
    ("Custom", 0, 0),
    ("Full HD 1920 × 1080", 1920, 1080),
    ("4K UHD 3840 × 2160", 3840, 2160),
    ("Square 1080 × 1080", 1080, 1080),
    ("Portrait post 1080 × 1350", 1080, 1350),
    ("Story 1080 × 1920", 1080, 1920),
    ("Video thumbnail 1280 × 720", 1280, 720),
    ("A4 @ 300 ppi", 2480, 3508),
    ("US Letter @ 300 ppi", 2550, 3300),
    ("Phone wallpaper 1179 × 2556", 1179, 2556),
    ("App icon 1024 × 1024", 1024, 1024),
    ("Web banner 1500 × 500", 1500, 500),
];

const EXPORT_FORMATS: &[(&str, &str)] = &[
    ("PNG", "png"),
    ("JPEG", "jpg"),
    ("WebP (lossless)", "webp"),
    ("AVIF", "avif"),
    ("TIFF", "tiff"),
    ("BMP", "bmp"),
    ("GIF", "gif"),
    ("TGA", "tga"),
    ("ICO", "ico"),
    ("QOI", "qoi"),
    ("Photoshop (PSD)", "psd"),
    ("Photoshop large (PSB)", "psb"),
    ("OpenRaster (ORA, layered)", "ora"),
    ("OpenEXR", "exr"),
];

impl Dialog {
    pub fn new_document(clip: Option<(u32, u32)>) -> Self {
        let (w, h) = clip.unwrap_or((1920, 1080));
        Dialog::NewDocument {
            name: "Untitled".into(),
            w,
            h,
            bg: Background::White,
            preset: if clip.is_some() { 0 } else { 1 },
        }
    }
    pub fn export(quality: u8) -> Self {
        Dialog::Export { format: 0, quality, scale: 1.0 }
    }
    pub fn adjust(adj: Adjustment, doc: &Document) -> Self {
        Dialog::Adjust { adj, st: ParamState::default(), pv: Preview::new(doc) }
    }
    pub fn shadows_highlights(doc: &Document) -> Self {
        Dialog::ShadowsHighlights { shadows: 35.0, highlights: 0.0, radius: 30.0, pv: Preview::new(doc) }
    }
    pub fn filter(f: Filter, doc: &Document) -> Self {
        Dialog::Filter { filter: f, pv: Preview::new(doc) }
    }
    pub fn image_size(doc: &Document) -> Self {
        Dialog::ImageSize {
            w: doc.width,
            h: doc.height,
            keep: true,
            method: Resample::Bicubic,
            orig: (doc.width, doc.height),
        }
    }
    pub fn canvas_size(doc: &Document) -> Self {
        Dialog::CanvasSize {
            w: doc.width,
            h: doc.height,
            anchor: Anchor::Center,
            fill: Background::White,
            orig: (doc.width, doc.height),
        }
    }
    pub fn color_range(c: Rgba, doc: &Document) -> Self {
        Dialog::ColorRange { color: c, fuzziness: 40, pv: Preview::new(doc) }
    }
    pub fn fill() -> Self {
        Dialog::Fill { with: FillWith::Foreground, opacity: 1.0, blend: BlendMode::Normal }
    }
    pub fn stroke() -> Self {
        Dialog::Stroke { width: 4.0, position: StrokePosition::Center }
    }
    pub fn layer_style(doc: &Document) -> Self {
        let styles = doc.active_layer().map(|l| l.styles.clone()).unwrap_or_default();
        Dialog::LayerStyle { styles, section: 0, pv: Preview::new(doc) }
    }
}

fn bg_color(b: Background, fg: Rgba, bgc: Rgba) -> Option<Rgba> {
    match b {
        Background::White => Some(Rgba::WHITE),
        Background::Black => Some(Rgba::BLACK),
        Background::Transparent => None,
        Background::Foreground => Some(fg),
        Background::Background => Some(bgc),
    }
}

fn bg_picker(ui: &mut Ui, v: &mut Background) {
    egui::ComboBox::from_id_salt("bg-pick")
        .selected_text(match v {
            Background::White => "White",
            Background::Black => "Black",
            Background::Transparent => "Transparent",
            Background::Foreground => "Foreground color",
            Background::Background => "Background color",
        })
        .show_ui(ui, |ui| {
            ui.selectable_value(v, Background::White, "White");
            ui.selectable_value(v, Background::Black, "Black");
            ui.selectable_value(v, Background::Transparent, "Transparent");
            ui.selectable_value(v, Background::Foreground, "Foreground color");
            ui.selectable_value(v, Background::Background, "Background color");
        });
}

/// Buttons row; returns (ok, cancel).
fn buttons(ui: &mut Ui, ok_label: &str) -> (bool, bool) {
    let mut ok = false;
    let mut cancel = false;
    ui.add_space(8.0);
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), 30.0),
        egui::Layout::right_to_left(egui::Align::Center),
        |ui| {
            ok = primary_button(ui, ok_label).clicked();
            cancel = secondary_button(ui, "Cancel").clicked();
        },
    );
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        cancel = true;
    }
    if ui.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.any()) && ui.memory(|m| m.focused().is_none()) {
        ok = true;
    }
    (ok, cancel)
}

fn modal<R>(ctx: &Context, id: &str, title: &str, width: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Modal::new(egui::Id::new(id))
        .show(ctx, |ui| {
            ui.set_width(width);
            ui.label(egui::RichText::new(title).font(crate::theme::medium(16.0)));
            ui.add_space(8.0);
            add(ui)
        })
        .inner
}

fn panel_window<R>(ctx: &Context, title: &str, add: impl FnOnce(&mut Ui) -> R) -> Option<R> {
    let screen = ctx.content_rect();
    egui::Window::new(title)
        .id(egui::Id::new(("dlg", title)))
        .collapsible(false)
        .resizable(false)
        .default_pos(egui::pos2(screen.right() - 700.0, screen.top() + 90.0))
        .default_width(330.0)
        .show(ctx, |ui| add(ui))
        .and_then(|r| r.inner)
}

/// Apply `f` to the active layer (or mask) of `doc`, from the preview base.
fn preview_apply(app: &mut App, pv: &mut Preview, now: f64, f: impl FnOnce(&mut Image, Option<&studio_core::Mask>)) {
    let Some(at) = pv.dirty_at else { return };
    if now - at < 0.06 {
        return;
    }
    pv.dirty_at = None;
    let edit_mask = app.edit_mask;
    let Some(tab) = app.tab_mut() else { return };
    tab.doc.restore(&pv.base);
    let sel = tab.doc.selection.clone();
    if let Some(layer) = tab.doc.active_layer_mut() {
        if edit_mask {
            if let Some(m) = layer.mask.as_mut() {
                let mut img = m.to_image();
                f(&mut img, sel.as_ref());
                *m = studio_core::Mask::from_raw(
                    img.width(),
                    img.height(),
                    img.pixels().chunks_exact(4).map(|p| p[0]).collect(),
                );
            }
        } else {
            if matches!(layer.kind, LayerKind::Text(_) | LayerKind::Shape(_)) {
                layer.rasterize();
            }
            // Smart objects preview on their rendered pixels; OK adds a smart filter.
            if matches!(layer.kind, LayerKind::Raster | LayerKind::Smart { .. }) && !layer.locked {
                f(&mut layer.pixels, sel.as_ref());
            }
        }
    }
    tab.invalidate_all();
}

fn restore(app: &mut App, pv: &Preview) {
    if let Some(tab) = app.tab_mut() {
        tab.doc.restore(&pv.base);
        tab.invalidate_all();
    }
}

fn commit(app: &mut App, label: &str) {
    if let Some(tab) = app.tab_mut() {
        tab.commit(label);
    }
}

pub fn show(app: &mut App, ctx: &Context, actions: &mut Vec<Action>) {
    let Some(mut d) = app.dialog.take() else {
        return;
    };
    let now = ctx.input(|i| i.time);
    let keep = match &mut d {
        Dialog::NewDocument { name, w, h, bg, preset } => modal(ctx, "new-doc", "New Document", 380.0, |ui| {
            egui::Grid::new("newdoc").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                label_dim(ui, "Name");
                ui.text_edit_singleline(name);
                ui.end_row();
                label_dim(ui, "Preset");
                egui::ComboBox::from_id_salt("preset").width(220.0).selected_text(PRESETS[*preset].0).show_ui(
                    ui,
                    |ui| {
                        for (i, p) in PRESETS.iter().enumerate() {
                            if ui.selectable_value(preset, i, p.0).clicked() && i > 0 {
                                (*w, *h) = (p.1, p.2);
                            }
                        }
                    },
                );
                ui.end_row();
                label_dim(ui, "Width");
                if ui.add(egui::DragValue::new(w).range(1..=30000).suffix(" px")).changed() {
                    *preset = 0;
                }
                ui.end_row();
                label_dim(ui, "Height");
                if ui.add(egui::DragValue::new(h).range(1..=30000).suffix(" px")).changed() {
                    *preset = 0;
                }
                ui.end_row();
                label_dim(ui, "Background");
                bg_picker(ui, bg);
                ui.end_row();
            });
            if ui.button(format!("{} Swap orientation", ph::ARROWS_LEFT_RIGHT)).clicked() {
                std::mem::swap(w, h);
            }
            let mp = (*w as f64 * *h as f64) / 1e6;
            label_dim(ui, format!("{mp:.1} megapixels · {:.0} MB in memory per layer", mp * 4.0));
            let (ok, cancel) = buttons(ui, "Create");
            if ok {
                let doc = Document::new(*w, *h, bg_color(*bg, app.fg, app.bg));
                let mut tab = DocTab::new(doc, name.clone(), None);
                tab.mark_saved();
                app.add_doc(tab);
            }
            !(ok || cancel)
        }),
        Dialog::Export { format, quality, scale } => modal(ctx, "export", "Export As", 360.0, |ui| {
            egui::Grid::new("export").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                label_dim(ui, "Format");
                egui::ComboBox::from_id_salt("fmt").selected_text(EXPORT_FORMATS[*format].0).show_ui(ui, |ui| {
                    for (i, f) in EXPORT_FORMATS.iter().enumerate() {
                        ui.selectable_value(format, i, f.0);
                    }
                });
                ui.end_row();
                if matches!(EXPORT_FORMATS[*format].1, "jpg" | "avif") {
                    label_dim(ui, "Quality");
                    ui.add(egui::Slider::new(quality, 1..=100));
                    ui.end_row();
                }
                if !matches!(EXPORT_FORMATS[*format].1, "psd" | "psb") {
                    label_dim(ui, "Scale");
                    let mut pct = *scale * 100.0;
                    if ui.add(egui::Slider::new(&mut pct, 5.0..=400.0).suffix("%")).changed() {
                        *scale = pct / 100.0;
                    }
                    ui.end_row();
                }
            });
            if let Some(tab) = app.tab() {
                label_dim(
                    ui,
                    format!(
                        "Output: {} × {} px",
                        (tab.doc.width as f32 * *scale).round(),
                        (tab.doc.height as f32 * *scale).round()
                    ),
                );
                if matches!(EXPORT_FORMATS[*format].1, "jpg" | "bmp") {
                    label_dim(ui, "Transparent areas are flattened onto white.");
                }
            }
            let (ok, cancel) = buttons(ui, "Export…");
            if ok {
                let ext = EXPORT_FORMATS[*format].1;
                let stem = app
                    .tab()
                    .map(|t| {
                        std::path::Path::new(&t.name)
                            .file_stem()
                            .map(|s| s.to_string_lossy().into_owned())
                            .unwrap_or_default()
                    })
                    .unwrap_or_default();
                let mut fd = rfd::FileDialog::new()
                    .set_file_name(format!("{stem}.{ext}"))
                    .add_filter(EXPORT_FORMATS[*format].0, &[ext]);
                if let Some(dir) = app.prefs.last_dir.clone() {
                    fd = fd.set_directory(dir);
                }
                app.prefs.jpeg_quality = *quality;
                if let Some(path) = fd.save_file() {
                    let opts = io::ExportOptions { jpeg_quality: *quality, ..Default::default() };
                    app.export_to(ctx, &path, &opts, *scale);
                }
            }
            !(ok || cancel)
        }),
        Dialog::Adjust { adj, st, pv } => {
            let title = adj.label();
            let hist = app.tab_mut().map(|t| t.histogram().clone());
            let r = panel_window(ctx, title, |ui| {
                if adjustment_ui(ui, adj, hist.as_ref(), st) {
                    pv.dirty_at = Some(now);
                }
                buttons(ui, "Apply")
            });
            let a = adj.clone();
            preview_apply(app, pv, now, |img, sel| adjust::apply(img, &a, sel));
            if pv.dirty_at.is_some() {
                ctx.request_repaint();
            }
            match r {
                Some((true, _)) => {
                    pv.dirty_at = Some(-1.0);
                    preview_apply(app, pv, now, |img, sel| adjust::apply(img, &a, sel));
                    commit(app, title);
                    app.record(Action::ApplyAdjustment(a.clone()));
                    false
                }
                Some((_, true)) | None => {
                    restore(app, pv);
                    false
                }
                _ => true,
            }
        }
        Dialog::ShadowsHighlights { shadows, highlights, radius, pv } => {
            let r = panel_window(ctx, "Shadows/Highlights", |ui| {
                let mut c = slider_row(ui, "Shadows", shadows, 0.0..=100.0, "%", 0);
                c |= slider_row(ui, "Highlights", highlights, 0.0..=100.0, "%", 0);
                c |= slider_row(ui, "Radius", radius, 1.0..=200.0, " px", 0);
                if c {
                    pv.dirty_at = Some(now);
                }
                buttons(ui, "Apply")
            });
            let (s, h, rad) = (*shadows, *highlights, *radius);
            preview_apply(app, pv, now, |img, sel| adjust::shadows_highlights(img, s, h, rad, sel));
            match r {
                Some((true, _)) => {
                    commit(app, "Shadows/Highlights");
                    false
                }
                Some((_, true)) | None => {
                    restore(app, pv);
                    false
                }
                _ => true,
            }
        }
        Dialog::Filter { filter, pv } => {
            let title = filter.label();
            let r = panel_window(ctx, title, |ui| {
                if filter_ui(ui, filter) {
                    pv.dirty_at = Some(now);
                }
                if pv.dirty_at.is_some() {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        label_dim(ui, "Previewing…");
                    });
                }
                buttons(ui, "Apply")
            });
            let f = filter.clone();
            let fc = FilterCtx { fg: app.fg, bg: app.bg };
            preview_apply(app, pv, now, |img, sel| f.apply(img, sel, &fc));
            if pv.dirty_at.is_some() {
                ctx.request_repaint();
            }
            match r {
                Some((true, _)) => {
                    app.last_filter = Some(filter.clone());
                    app.record(Action::ApplyFilter(filter.clone()));
                    // Smart objects get the filter as an editable smart filter.
                    let smart = app.tab().and_then(|t| t.doc.active_layer()).is_some_and(|l| l.is_smart());
                    if smart {
                        restore(app, pv);
                        app.add_smart_filter(filter.clone());
                    } else {
                        pv.dirty_at = Some(-1.0);
                        preview_apply(app, pv, now, |img, sel| f.apply(img, sel, &fc));
                        commit(app, title);
                    }
                    false
                }
                Some((_, true)) | None => {
                    restore(app, pv);
                    false
                }
                _ => true,
            }
        }
        Dialog::ImageSize { w, h, keep, method, orig } => modal(ctx, "img-size", "Image Size", 360.0, |ui| {
            let aspect = orig.0 as f32 / orig.1 as f32;
            egui::Grid::new("imgsize").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                label_dim(ui, "Width");
                if ui.add(egui::DragValue::new(w).range(1..=30000).suffix(" px")).changed() && *keep {
                    *h = ((*w as f32 / aspect).round() as u32).max(1);
                }
                ui.end_row();
                label_dim(ui, "Height");
                if ui.add(egui::DragValue::new(h).range(1..=30000).suffix(" px")).changed() && *keep {
                    *w = ((*h as f32 * aspect).round() as u32).max(1);
                }
                ui.end_row();
                label_dim(ui, "Percent");
                let mut pct = *w as f32 / orig.0 as f32 * 100.0;
                if ui.add(egui::DragValue::new(&mut pct).range(1.0..=1000.0).suffix("%").speed(0.5)).changed() {
                    *w = ((orig.0 as f32 * pct / 100.0).round() as u32).max(1);
                    *h = ((orig.1 as f32 * pct / 100.0).round() as u32).max(1);
                }
                ui.end_row();
                label_dim(ui, "Constrain");
                crate::widgets::toggle(ui, keep);
                ui.end_row();
                label_dim(ui, "Resample");
                egui::ComboBox::from_id_salt("resample").selected_text(method.label()).show_ui(ui, |ui| {
                    for m in Resample::ALL {
                        ui.selectable_value(method, m, m.label());
                    }
                });
                ui.end_row();
            });
            label_dim(ui, format!("Was {} × {} px", orig.0, orig.1));
            let (ok, cancel) = buttons(ui, "Resize");
            if ok && (*w, *h) != *orig {
                let (nw, nh, m) = (*w, *h, *method);
                app.edit("Image Size", |d| d.resize_image(nw, nh, m));
                app.record(Action::ResizeImage(nw, nh));
                if let Some(t) = app.tab_mut() {
                    t.fit_pending = true;
                }
            }
            !(ok || cancel)
        }),
        Dialog::CanvasSize { w, h, anchor, fill, orig } => modal(ctx, "canvas-size", "Canvas Size", 360.0, |ui| {
            egui::Grid::new("cansize").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                label_dim(ui, "Width");
                ui.add(egui::DragValue::new(w).range(1..=30000).suffix(" px"));
                ui.end_row();
                label_dim(ui, "Height");
                ui.add(egui::DragValue::new(h).range(1..=30000).suffix(" px"));
                ui.end_row();
                label_dim(ui, "Anchor");
                egui::Grid::new("anchor").spacing([2.0, 2.0]).show(ui, |ui| {
                    for (i, a) in Anchor::GRID.iter().enumerate() {
                        let sel = *anchor == *a;
                        if crate::widgets::icon_button(
                            ui,
                            if sel { ph::CIRCLE } else { ph::DOT_OUTLINE },
                            "",
                            sel,
                            24.0,
                        )
                        .clicked()
                        {
                            *anchor = *a;
                        }
                        if i % 3 == 2 {
                            ui.end_row();
                        }
                    }
                });
                ui.end_row();
                label_dim(ui, "New area");
                bg_picker(ui, fill);
                ui.end_row();
            });
            let (ok, cancel) = buttons(ui, "Apply");
            if ok && (*w, *h) != *orig {
                let (nw, nh, a, f) = (*w, *h, *anchor, bg_color(*fill, app.fg, app.bg));
                app.edit("Canvas Size", |d| d.resize_canvas(nw, nh, a, f));
                if let Some(t) = app.tab_mut() {
                    t.fit_pending = true;
                }
            }
            !(ok || cancel)
        }),
        Dialog::RotateArbitrary { angle } => modal(ctx, "rotate", "Rotate Canvas", 320.0, |ui| {
            slider_row(ui, "Angle", angle, -180.0..=180.0, "°", 1);
            label_dim(ui, "Positive angles turn clockwise. The canvas grows to fit.");
            let (ok, cancel) = buttons(ui, "Rotate");
            if ok {
                let a = *angle;
                app.edit("Rotate Canvas", |d| d.rotate_arbitrary(a));
                if let Some(t) = app.tab_mut() {
                    t.fit_pending = true;
                }
            }
            !(ok || cancel)
        }),
        Dialog::Modify { kind, amount } => modal(ctx, "modify", &format!("{} Selection", kind.label()), 320.0, |ui| {
            slider_row(
                ui,
                if *kind == ModifyKind::Feather { "Radius" } else { "Amount" },
                amount,
                0.5..=250.0,
                " px",
                1,
            );
            let (ok, cancel) = buttons(ui, "OK");
            if ok {
                app.apply_modify(*kind, *amount);
            }
            !(ok || cancel)
        }),
        Dialog::ColorRange { color, fuzziness, pv } => {
            let r = panel_window(ctx, "Color Range", |ui| {
                let mut c = crate::panels::color_picker(ui, "range-color", color);
                if ui.button(format!("{} Use foreground color", ph::EYEDROPPER)).clicked() {
                    *color = app.fg;
                    c = true;
                }
                c |= crate::widgets::slider_row_u8(ui, "Fuzziness", fuzziness, "");
                if c {
                    pv.dirty_at = Some(now);
                }
                buttons(ui, "Select")
            });
            if pv.dirty_at.is_some_and(|t| now - t > 0.06) {
                pv.dirty_at = None;
                let (col, fz) = (*color, *fuzziness);
                if let Some(tab) = app.tab_mut() {
                    tab.doc.restore(&pv.base);
                    let m = select::color_range(&tab.composite, col, fz);
                    tab.doc.set_selection(m);
                    tab.invalidate_all();
                }
            }
            match r {
                Some((true, _)) => {
                    commit(app, "Color Range");
                    false
                }
                Some((_, true)) | None => {
                    restore(app, pv);
                    false
                }
                _ => true,
            }
        }
        Dialog::Fill { with, opacity, blend } => modal(ctx, "fill", "Fill", 320.0, |ui| {
            egui::Grid::new("fill").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                label_dim(ui, "Contents");
                egui::ComboBox::from_id_salt("fillwith")
                    .selected_text(match with {
                        FillWith::Foreground => "Foreground Color",
                        FillWith::Background => "Background Color",
                        FillWith::Black => "Black",
                        FillWith::White => "White",
                        FillWith::Gray => "50% Gray",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(with, FillWith::Foreground, "Foreground Color");
                        ui.selectable_value(with, FillWith::Background, "Background Color");
                        ui.selectable_value(with, FillWith::Black, "Black");
                        ui.selectable_value(with, FillWith::White, "White");
                        ui.selectable_value(with, FillWith::Gray, "50% Gray");
                    });
                ui.end_row();
                label_dim(ui, "Mode");
                egui::ComboBox::from_id_salt("fillmode").selected_text(blend.label()).show_ui(ui, |ui| {
                    for m in BlendMode::all() {
                        ui.selectable_value(blend, m, m.label());
                    }
                });
                ui.end_row();
            });
            crate::widgets::slider_row_pct(ui, "Opacity", opacity);
            let (ok, cancel) = buttons(ui, "Fill");
            if ok {
                let c = match with {
                    FillWith::Foreground => app.fg,
                    FillWith::Background => app.bg,
                    FillWith::Black => Rgba::BLACK,
                    FillWith::White => Rgba::WHITE,
                    FillWith::Gray => Rgba::rgb(128, 128, 128),
                };
                let (o, b) = (*opacity, *blend);
                app.edit_pixels(ctx, "Fill", |img, sel| {
                    let all = studio_core::Mask::filled(img.width(), img.height(), 255);
                    paint::fill_region(img, sel.unwrap_or(&all), c, b, o);
                });
            }
            !(ok || cancel)
        }),
        Dialog::Stroke { width, position } => modal(ctx, "stroke", "Stroke Selection", 320.0, |ui| {
            slider_row(ui, "Width", width, 1.0..=250.0, " px", 0);
            segmented(
                ui,
                position,
                &[
                    (StrokePosition::Inside, "Inside"),
                    (StrokePosition::Center, "Center"),
                    (StrokePosition::Outside, "Outside"),
                ],
            );
            label_dim(ui, "Uses the foreground color.");
            let (ok, cancel) = buttons(ui, "Stroke");
            if ok {
                let (wd, pos, fg) = (*width, *position, app.fg);
                app.edit_pixels(ctx, "Stroke", |img, sel| {
                    if let Some(s) = sel {
                        let ring = match pos {
                            StrokePosition::Outside => studio_core::filters::outline_mask(s, wd, true, false),
                            StrokePosition::Inside => studio_core::filters::outline_mask(s, wd, false, true),
                            StrokePosition::Center => select::border(s, wd),
                        };
                        paint::fill_region(img, &ring, fg, BlendMode::Normal, 1.0);
                    }
                });
            }
            !(ok || cancel)
        }),
        Dialog::LayerStyle { styles, section, pv } => {
            let mut changed = false;
            let r = panel_window(ctx, "Layer Style", |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(150.0);
                        for (i, n) in STYLE_NAMES.iter().enumerate() {
                            let on = match i {
                                0 => &mut styles.bevel.enabled,
                                1 => &mut styles.stroke.enabled,
                                2 => &mut styles.inner_shadow.enabled,
                                3 => &mut styles.inner_glow.enabled,
                                4 => &mut styles.satin.enabled,
                                5 => &mut styles.color_overlay.enabled,
                                6 => &mut styles.gradient_overlay.enabled,
                                7 => &mut styles.pattern_overlay.enabled,
                                8 => &mut styles.outer_glow.enabled,
                                _ => &mut styles.drop_shadow.enabled,
                            };
                            ui.horizontal(|ui| {
                                changed |= ui.checkbox(on, "").changed();
                                if ui.selectable_label(*section == i, *n).clicked() {
                                    *section = i;
                                }
                            });
                        }
                    });
                    ui.separator();
                    ui.vertical(|ui| {
                        ui.set_width(300.0);
                        changed |= style_section(ui, styles, *section);
                    });
                });
                buttons(ui, "OK")
            });
            if changed {
                if let Some(tab) = app.tab_mut() {
                    if let Some(l) = tab.doc.active_layer_mut() {
                        l.styles = styles.clone();
                    }
                    tab.invalidate_all();
                }
            }
            match r {
                Some((true, _)) => {
                    commit(app, "Layer Style");
                    false
                }
                Some((_, true)) | None => {
                    restore(app, pv);
                    false
                }
                _ => true,
            }
        }
        Dialog::EditNote { index, text, is_new } => modal(ctx, "note", "Note", 340.0, |ui| {
            let r = ui.add(
                egui::TextEdit::multiline(text).desired_rows(4).desired_width(f32::INFINITY).hint_text("Write a note…"),
            );
            r.request_focus();
            let mut delete = false;
            ui.add_space(6.0);
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), 30.0),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    let ok = primary_button(ui, "Done").clicked();
                    let cancel =
                        secondary_button(ui, "Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape));
                    delete = !*is_new && secondary_button(ui, "Delete").clicked();
                    let i = *index;
                    if let Some(tab) = app.tab_mut() {
                        if ok && !text.trim().is_empty() {
                            if let Some(n) = tab.doc.annotations.notes.get_mut(i) {
                                n.1 = text.clone();
                            }
                            tab.commit("Note");
                        } else if delete || ((ok || cancel) && *is_new) {
                            if i < tab.doc.annotations.notes.len() {
                                tab.doc.annotations.notes.remove(i);
                            }
                            if delete {
                                tab.commit("Delete Note");
                            }
                        }
                    }
                    *is_new = !(ok || cancel || delete) && *is_new;
                    if ok || cancel || delete {
                        *index = usize::MAX;
                    }
                },
            );
            *index != usize::MAX
        }),
        Dialog::Batch { action, format } => modal(ctx, "batch", "Batch", 400.0, |ui| {
            let names: Vec<String> = app.prefs.macros.iter().map(|m| m.0.clone()).collect();
            *action = (*action).min(names.len().saturating_sub(1));
            egui::Grid::new("batch").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                label_dim(ui, "Action");
                egui::ComboBox::from_id_salt("batch-action")
                    .selected_text(names.get(*action).cloned().unwrap_or_default())
                    .show_ui(ui, |ui| {
                        for (i, n) in names.iter().enumerate() {
                            ui.selectable_value(action, i, n);
                        }
                    });
                ui.end_row();
                label_dim(ui, "Save as");
                egui::ComboBox::from_id_salt("batch-fmt").selected_text(EXPORT_FORMATS[*format].0).show_ui(ui, |ui| {
                    for (i, f) in EXPORT_FORMATS.iter().enumerate() {
                        ui.selectable_value(format, i, f.0);
                    }
                });
                ui.end_row();
            });
            label_dim(ui, "Pick a folder of images, then where to save the results.");
            let (ok, cancel) = buttons(ui, "Choose Folders…");
            if ok {
                let src = rfd::FileDialog::new().set_title("Images to process").pick_folder();
                let dst = src.as_ref().and_then(|_| rfd::FileDialog::new().set_title("Save results to").pick_folder());
                if let (Some(src), Some(dst)) = (src, dst) {
                    let ext = EXPORT_FORMATS[*format].1;
                    let (done, failed) = crate::app::run_batch(app, ctx, *action, &src, &dst, ext);
                    app.notify(ctx, format!("Batch finished: {done} saved, {failed} failed"));
                }
            }
            !(ok || cancel)
        }),
        Dialog::NewGuide { horizontal, pos } => modal(ctx, "new-guide", "New Guide", 320.0, |ui| {
            segmented(ui, horizontal, &[(true, "Horizontal"), (false, "Vertical")]);
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                label_dim(ui, "Position");
                ui.add(egui::DragValue::new(pos).suffix(" px").speed(1.0));
            });
            let (ok, cancel) = buttons(ui, "Add");
            if ok {
                let g = studio_core::document::Guide { horizontal: *horizontal, pos: *pos };
                app.edit("New Guide", |d| d.guides.push(g));
                app.prefs.show_guides = true;
            }
            !(ok || cancel)
        }),
        Dialog::CommandPalette { query, selected } => {
            let mut run = None;
            let mut close = false;
            let screen = ctx.content_rect();
            egui::Modal::new(egui::Id::new("palette"))
                .area(
                    egui::Modal::default_area(egui::Id::new("palette-area"))
                        .anchor(egui::Align2::CENTER_TOP, [0.0, screen.height() * 0.12]),
                )
                .show(ctx, |ui| {
                    ui.set_width(560.0);
                    let r = ui.add(
                        egui::TextEdit::singleline(query)
                            .hint_text(format!("{}  Search commands, adjustments and filters", ph::MAGNIFYING_GLASS))
                            .font(egui::FontId::proportional(17.0))
                            .desired_width(f32::INFINITY)
                            .margin(egui::vec2(10.0, 8.0)),
                    );
                    r.request_focus();
                    if r.changed() {
                        *selected = 0;
                    }
                    let mut hits: Vec<(i32, String, Action)> = crate::menus::all_commands()
                        .into_iter()
                        .filter_map(|a| {
                            let label = crate::menus::palette_label(&a);
                            crate::menus::fuzzy(query, &label).map(|s| (s, label, a))
                        })
                        .collect();
                    hits.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
                    hits.truncate(12);
                    let (down, up, enter, esc) = ui.input(|i| {
                        (
                            i.key_pressed(egui::Key::ArrowDown),
                            i.key_pressed(egui::Key::ArrowUp),
                            i.key_pressed(egui::Key::Enter),
                            i.key_pressed(egui::Key::Escape),
                        )
                    });
                    if down {
                        *selected = (*selected + 1).min(hits.len().saturating_sub(1));
                    }
                    if up {
                        *selected = selected.saturating_sub(1);
                    }
                    ui.add_space(6.0);
                    for (i, (_, label, a)) in hits.iter().enumerate() {
                        let sc = crate::actions::shortcut_text(ctx, a);
                        let resp = ui.add_sized(
                            [ui.available_width(), 28.0],
                            egui::Button::selectable(i == *selected, label.as_str()).shortcut_text(sc),
                        );
                        if resp.clicked() {
                            run = Some(a.clone());
                        }
                    }
                    if hits.is_empty() {
                        label_dim(ui, "No matching commands");
                    }
                    if enter {
                        run = hits.get(*selected).map(|h| h.2.clone());
                    }
                    if esc {
                        close = true;
                    }
                });
            if let Some(a) = run {
                actions.push(a);
                false
            } else {
                !close
            }
        }
        Dialog::Preferences => modal(ctx, "prefs", "Preferences", 380.0, |ui| {
            let mut theme = app.prefs.theme;
            ui.horizontal(|ui| {
                label_dim(ui, "Appearance");
                if segmented(
                    ui,
                    &mut theme,
                    &[(ThemeMode::System, "Auto"), (ThemeMode::Light, "Light"), (ThemeMode::Dark, "Dark")],
                ) {
                    app.prefs.theme = theme;
                    crate::theme::apply(ctx, theme);
                }
            });
            ui.add_space(6.0);
            toggle_row(ui, "Pixel grid at high zoom", &mut app.prefs.pixel_grid);
            toggle_row(ui, "Animated logo on the start screen", &mut app.prefs.show_welcome_animation);
            let mut hl = app.prefs.history_limit as f32;
            if slider_row(ui, "History states", &mut hl, 5.0..=200.0, "", 0) {
                app.prefs.history_limit = hl as usize;
                for d in &mut app.docs {
                    d.history.set_limit(app.prefs.history_limit);
                }
            }
            if ui.button("Clear recent files").clicked() {
                app.prefs.recent.clear();
            }
            if ui.button("Reset tool options").clicked() {
                app.opts = Default::default();
            }
            let (ok, cancel) = buttons(ui, "Done");
            !(ok || cancel)
        }),
        Dialog::About => modal(ctx, "about", "", 360.0, |ui| {
            ui.vertical_centered(|ui| {
                crate::welcome::logo(app, ui, 110.0);
                ui.label(egui::RichText::new("Photo Studio").font(crate::theme::bold(22.0)));
                label_dim(ui, format!("Version {}", env!("CARGO_PKG_VERSION")));
                ui.add_space(6.0);
                ui.label("A fast, layer-based photo editor for macOS, Windows and Linux.");
                ui.add_space(4.0);
                label_dim(ui, "© 2026 Abhay Pratap Singh · MIT License");
                label_dim(ui, "Inter, Noto Serif and JetBrains Mono fonts under the SIL Open Font License.");
                ui.add_space(6.0);
                ui.hyperlink_to("photo-studio.devops-monk.com", "https://photo-studio.devops-monk.com");
                ui.hyperlink_to("Source on GitHub", "https://github.com/devops-monk/photo-studio");
            });
            let (ok, cancel) = buttons(ui, "Close");
            !(ok || cancel)
        }),
        Dialog::Shortcuts => modal(ctx, "shortcuts", "Keyboard Shortcuts", 520.0, |ui| {
            egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                egui::Grid::new("sc").num_columns(2).striped(true).spacing([24.0, 4.0]).show(ui, |ui| {
                    ui.label(egui::RichText::new("Tools").strong());
                    ui.end_row();
                    for g in crate::tools::GROUPS {
                        let names: Vec<&str> = g.iter().map(|t| t.label()).collect();
                        ui.label(names.join(" / "));
                        if let Some(k) = g[0].key() {
                            ui.label(egui::RichText::new(k.name()).monospace());
                        }
                        ui.end_row();
                    }
                    ui.label(egui::RichText::new("Commands").strong());
                    ui.end_row();
                    for (s, a) in shortcuts() {
                        ui.label(crate::menus::action_label(&a));
                        ui.label(egui::RichText::new(ctx.format_shortcut(&s)).monospace());
                        ui.end_row();
                    }
                    for (k, what) in [
                        ("Space + drag", "Pan"),
                        ("⌘/Ctrl + scroll or pinch", "Zoom"),
                        ("Shift (marquee/lasso)", "Add to selection"),
                        ("Alt (marquee/lasso)", "Subtract from selection"),
                        ("Alt-click (clone/heal)", "Set source"),
                        ("Arrow keys (Move)", "Nudge 1 px, Shift 10 px"),
                        ("Enter / Esc", "Commit / cancel crop and transform"),
                    ] {
                        ui.label(what);
                        ui.label(egui::RichText::new(k).monospace());
                        ui.end_row();
                    }
                });
            });
            let (ok, cancel) = buttons(ui, "Close");
            !(ok || cancel)
        }),
    };
    if keep && app.dialog.is_none() {
        app.dialog = Some(d);
    }
}

/// The Layer Style dialog's effect list, in the familiar order.
const STYLE_NAMES: [&str; 10] = [
    "Bevel & Emboss",
    "Stroke",
    "Inner Shadow",
    "Inner Glow",
    "Satin",
    "Color Overlay",
    "Gradient Overlay",
    "Pattern Overlay",
    "Outer Glow",
    "Drop Shadow",
];

fn style_section(ui: &mut Ui, s: &mut LayerStyles, section: usize) -> bool {
    let mut c = false;
    let color = |ui: &mut Ui, label: &str, col: &mut Rgba| -> bool {
        let mut cc = crate::widgets::c32(*col);
        let mut changed = false;
        ui.horizontal(|ui| {
            label_dim(ui, label);
            if egui::color_picker::color_edit_button_srgba(ui, &mut cc, egui::color_picker::Alpha::Opaque).changed() {
                *col = crate::widgets::rgba(cc);
                changed = true;
            }
        });
        changed
    };
    let blend = |ui: &mut Ui, m: &mut BlendMode| -> bool {
        let mut changed = false;
        egui::ComboBox::from_id_salt(("style-blend", ui.id())).selected_text(m.label()).show_ui(ui, |ui| {
            for b in BlendMode::all() {
                changed |= ui.selectable_value(m, b, b.label()).changed();
            }
        });
        changed
    };
    match section {
        9 | 2 => {
            let sh = if section == 9 { &mut s.drop_shadow } else { &mut s.inner_shadow };
            c |= toggle_row(ui, "Enabled", &mut sh.enabled);
            c |= blend(ui, &mut sh.blend);
            c |= color(ui, "Color", &mut sh.color);
            c |= crate::widgets::slider_row_pct(ui, "Opacity", &mut sh.opacity);
            c |= slider_row(ui, "Angle", &mut sh.angle, -180.0..=180.0, "°", 0);
            c |= slider_row(ui, "Distance", &mut sh.distance, 0.0..=300.0, " px", 0);
            c |= crate::widgets::slider_row_pct(ui, if section == 9 { "Spread" } else { "Choke" }, &mut sh.spread);
            c |= slider_row(ui, "Size", &mut sh.size, 0.0..=250.0, " px", 0);
        }
        8 | 3 => {
            let g = if section == 8 { &mut s.outer_glow } else { &mut s.inner_glow };
            c |= toggle_row(ui, "Enabled", &mut g.enabled);
            c |= color(ui, "Color", &mut g.color);
            c |= crate::widgets::slider_row_pct(ui, "Opacity", &mut g.opacity);
            c |= crate::widgets::slider_row_pct(ui, if section == 8 { "Spread" } else { "Choke" }, &mut g.spread);
            c |= slider_row(ui, "Size", &mut g.size, 0.0..=250.0, " px", 0);
        }
        1 => {
            let st = &mut s.stroke;
            c |= toggle_row(ui, "Enabled", &mut st.enabled);
            c |= slider_row(ui, "Size", &mut st.size, 1.0..=250.0, " px", 0);
            c |= segmented(
                ui,
                &mut st.position,
                &[
                    (StrokePosition::Outside, "Outside"),
                    (StrokePosition::Inside, "Inside"),
                    (StrokePosition::Center, "Center"),
                ],
            );
            c |= color(ui, "Color", &mut st.color);
            c |= crate::widgets::slider_row_pct(ui, "Opacity", &mut st.opacity);
        }
        0 => {
            let b = &mut s.bevel;
            c |= toggle_row(ui, "Enabled", &mut b.enabled);
            egui::ComboBox::from_id_salt("bevel-style").selected_text(b.style.label()).show_ui(ui, |ui| {
                for st in BevelStyle::ALL {
                    c |= ui.selectable_value(&mut b.style, st, st.label()).changed();
                }
            });
            c |= segmented(ui, &mut b.up, &[(true, "Up"), (false, "Down")]);
            c |= slider_row(ui, "Depth", &mut b.depth, 1.0..=1000.0, "%", 0);
            c |= slider_row(ui, "Size", &mut b.size, 1.0..=250.0, " px", 0);
            c |= slider_row(ui, "Soften", &mut b.soften, 0.0..=16.0, " px", 0);
            c |= slider_row(ui, "Angle", &mut b.angle, -180.0..=180.0, "°", 0);
            c |= slider_row(ui, "Altitude", &mut b.altitude, 0.0..=90.0, "°", 0);
            c |= color(ui, "Highlight", &mut b.highlight);
            c |= crate::widgets::slider_row_pct(ui, "Opacity", &mut b.highlight_opacity);
            c |= color(ui, "Shadow", &mut b.shadow);
            c |= crate::widgets::slider_row_pct(ui, "Opacity ", &mut b.shadow_opacity);
        }
        4 => {
            let sa = &mut s.satin;
            c |= toggle_row(ui, "Enabled", &mut sa.enabled);
            c |= color(ui, "Color", &mut sa.color);
            c |= crate::widgets::slider_row_pct(ui, "Opacity", &mut sa.opacity);
            c |= slider_row(ui, "Angle", &mut sa.angle, -180.0..=180.0, "°", 0);
            c |= slider_row(ui, "Distance", &mut sa.distance, 1.0..=250.0, " px", 0);
            c |= slider_row(ui, "Size", &mut sa.size, 0.0..=250.0, " px", 0);
            c |= toggle_row(ui, "Invert", &mut sa.invert);
        }
        6 => {
            let go = &mut s.gradient_overlay;
            c |= toggle_row(ui, "Enabled", &mut go.enabled);
            c |= blend(ui, &mut go.blend);
            ui.horizontal_wrapped(|ui| {
                for (name, g) in studio_core::gradient::Gradient::presets(Rgba::BLACK, Rgba::WHITE) {
                    if ui.small_button(name).clicked() {
                        go.gradient = g;
                        c = true;
                    }
                }
            });
            for (i, stop) in go.gradient.stops.iter_mut().enumerate() {
                c |= color(ui, &format!("Stop {}", i + 1), &mut stop.color);
            }
            egui::ComboBox::from_id_salt("go-shape").selected_text(go.shape.label()).show_ui(ui, |ui| {
                for sh in studio_core::gradient::GradientShape::ALL {
                    c |= ui.selectable_value(&mut go.shape, sh, sh.label()).changed();
                }
            });
            c |= slider_row(ui, "Angle", &mut go.angle, -180.0..=180.0, "°", 0);
            c |= slider_row(ui, "Scale", &mut go.scale, 10.0..=150.0, "%", 0);
            c |= toggle_row(ui, "Reverse", &mut go.reverse);
            c |= crate::widgets::slider_row_pct(ui, "Opacity", &mut go.opacity);
        }
        7 => {
            let po = &mut s.pattern_overlay;
            c |= toggle_row(ui, "Enabled", &mut po.enabled);
            c |= blend(ui, &mut po.blend);
            let names: Vec<&str> = studio_core::paint::builtin_patterns(po.fg, po.bg).iter().map(|p| p.0).collect();
            egui::ComboBox::from_id_salt("po-pattern")
                .selected_text(names.get(po.pattern).copied().unwrap_or(""))
                .show_ui(ui, |ui| {
                    for (i, n) in names.iter().enumerate() {
                        c |= ui.selectable_value(&mut po.pattern, i, *n).changed();
                    }
                });
            c |= color(ui, "Foreground", &mut po.fg);
            c |= color(ui, "Background", &mut po.bg);
            c |= slider_row(ui, "Scale", &mut po.scale, 10.0..=1000.0, "%", 0);
            c |= crate::widgets::slider_row_pct(ui, "Opacity", &mut po.opacity);
        }
        _ => {
            let co = &mut s.color_overlay;
            c |= toggle_row(ui, "Enabled", &mut co.enabled);
            c |= blend(ui, &mut co.blend);
            c |= color(ui, "Color", &mut co.color);
            c |= crate::widgets::slider_row_pct(ui, "Opacity", &mut co.opacity);
        }
    }
    c
}
