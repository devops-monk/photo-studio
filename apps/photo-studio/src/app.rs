//! Application state and command dispatch.

use std::path::{Path, PathBuf};

use studio_core::adjust::{self, Adjustment};
use studio_core::filters::{Filter, FilterCtx};
use studio_core::image::{Image, Mask};
use studio_core::layer::{FillLayer, Layer, LayerKind};
use studio_core::paint::StrokeEngine;
use studio_core::{Document, Rgba, io, select};

use crate::actions::{Action, ModifyKind};
use crate::canvas::CanvasState;
use crate::dialogs::Dialog;
use crate::doc::DocTab;
use crate::params::ParamState;
use crate::theme::ThemeMode;
use crate::tools::{GROUPS, Tool, ToolOptions};

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub theme: ThemeMode,
    pub recent: Vec<PathBuf>,
    pub pixel_grid: bool,
    pub opts: ToolOptions,
    pub fg: Rgba,
    pub bg: Rgba,
    pub history_limit: usize,
    pub jpeg_quality: u8,
    pub swatches: Vec<Rgba>,
    pub last_dir: Option<PathBuf>,
    pub show_welcome_animation: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs {
            theme: ThemeMode::System,
            recent: Vec::new(),
            pixel_grid: true,
            opts: ToolOptions::default(),
            fg: Rgba::BLACK,
            bg: Rgba::WHITE,
            history_limit: 60,
            jpeg_quality: 90,
            swatches: Vec::new(),
            last_dir: None,
            show_welcome_animation: true,
        }
    }
}

/// Where brush strokes land: layer pixels, or the layer mask shown as grey pixels.
pub enum StrokeTarget {
    Pixels,
    Mask(Image),
}

pub struct ActiveStroke {
    pub engine: StrokeEngine,
    pub target: StrokeTarget,
    pub label: &'static str,
}

pub enum Confirm {
    Close(usize),
    Quit,
}

pub struct App {
    pub docs: Vec<DocTab>,
    pub active: usize,
    pub tool: Tool,
    pub group_tool: Vec<Tool>,
    pub opts: ToolOptions,
    pub fg: Rgba,
    pub bg: Rgba,
    pub prefs: Prefs,
    pub dialog: Option<Dialog>,
    pub cv: CanvasState,
    pub stroke: Option<ActiveStroke>,
    pub toast: Option<(String, f64)>,
    pub last_filter: Option<Filter>,
    pub last_selection: Option<Mask>,
    pub clipboard: Option<Image>,
    pub confirm: Option<Confirm>,
    pub quitting: bool,
    pub props: ParamState,
    /// Brush strokes and fills target the active layer's mask.
    pub edit_mask: bool,
    pub show_edges: bool,
    pub props_pending: Option<(f64, &'static str)>,
    pub last_title: String,
    pub logo: crate::welcome::LogoAnim,
    pub show_right_panel: bool,
    /// Edit › Define Pattern.
    pub custom_pattern: Option<Image>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, files: Vec<PathBuf>) -> Self {
        crate::theme::install_fonts(&cc.egui_ctx);
        let prefs: Prefs = cc.storage.and_then(|s| eframe::get_value(s, "prefs")).unwrap_or_default();
        crate::theme::apply(&cc.egui_ctx, prefs.theme);
        let mut app = App {
            docs: Vec::new(),
            active: 0,
            tool: Tool::Brush,
            group_tool: GROUPS.iter().map(|g| g[0]).collect(),
            opts: prefs.opts.clone(),
            fg: prefs.fg,
            bg: prefs.bg,
            prefs,
            dialog: None,
            cv: CanvasState::default(),
            stroke: None,
            toast: None,
            last_filter: None,
            last_selection: None,
            clipboard: None,
            confirm: None,
            quitting: false,
            props: ParamState::default(),
            edit_mask: false,
            show_edges: true,
            props_pending: None,
            last_title: String::new(),
            logo: crate::welcome::LogoAnim::default(),
            show_right_panel: true,
            custom_pattern: None,
        };
        app.prefs.recent.retain(|p| p.exists());
        for f in files {
            app.open_path(&f);
        }
        app
    }

    pub fn save_prefs(&mut self, storage: &mut dyn eframe::Storage) {
        self.prefs.opts = self.opts.clone();
        self.prefs.fg = self.fg;
        self.prefs.bg = self.bg;
        eframe::set_value(storage, "prefs", &self.prefs);
    }

    pub fn tab(&self) -> Option<&DocTab> {
        self.docs.get(self.active)
    }

    pub fn tab_mut(&mut self) -> Option<&mut DocTab> {
        self.docs.get_mut(self.active)
    }

    pub fn notify(&mut self, ctx: &egui::Context, msg: impl Into<String>) {
        self.toast = Some((msg.into(), ctx.input(|i| i.time)));
    }

    pub fn set_tool(&mut self, t: Tool) {
        if self.tool != t {
            self.finish_transient();
        }
        self.tool = t;
        if let Some(gi) = GROUPS.iter().position(|g| g.contains(&t)) {
            self.group_tool[gi] = t;
        }
    }

    /// Commit or drop in-progress tool sessions (crop box, text edit, polygon lasso).
    pub fn finish_transient(&mut self) {
        self.cv.poly.clear();
        if self.cv.crop.is_some() {
            self.cv.crop = None;
        }
        if let Some(te) = self.cv.text_edit.take() {
            self.end_text_edit(te);
        }
    }

    pub fn add_doc(&mut self, tab: DocTab) {
        self.docs.push(tab);
        self.active = self.docs.len() - 1;
        self.edit_mask = false;
        if let Some(t) = self.docs.last_mut() {
            t.history.set_limit(self.prefs.history_limit);
        }
    }

    pub fn remember_recent(&mut self, p: &Path) {
        self.prefs.recent.retain(|r| r != p);
        self.prefs.recent.insert(0, p.to_path_buf());
        self.prefs.recent.truncate(12);
        if let Some(dir) = p.parent() {
            self.prefs.last_dir = Some(dir.to_path_buf());
        }
    }

    pub fn open_path(&mut self, p: &Path) {
        if let Some(i) = self.docs.iter().position(|d| d.path.as_deref() == Some(p)) {
            self.active = i;
            return;
        }
        match io::open(p) {
            Ok(doc) => {
                let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "Untitled".into());
                let is_project = p.extension().is_some_and(|e| e.eq_ignore_ascii_case(io::PROJECT_EXTENSION));
                let path = is_project.then(|| p.to_path_buf());
                let mut tab = DocTab::new(doc, name, path);
                if !is_project {
                    // Saving an opened JPEG/PNG writes a project next to it; export writes flat files.
                    tab.path = None;
                    tab.mark_saved();
                }
                tab.source = Some(p.to_path_buf());
                self.add_doc(tab);
                self.remember_recent(p);
            }
            Err(e) => {
                self.toast = Some((format!("Couldn't open {}: {e}", p.display()), 0.0));
            }
        }
    }

    fn dialog_dir(&self) -> Option<PathBuf> {
        self.prefs.last_dir.clone().or_else(|| std::env::var_os("HOME").map(PathBuf::from))
    }

    pub fn open_dialog(&mut self) {
        let mut d = rfd::FileDialog::new()
            .set_title("Open")
            .add_filter("All supported", io::OPEN_EXTENSIONS)
            .add_filter("Photo Studio project", &[io::PROJECT_EXTENSION])
            .add_filter("Photoshop", &["psd"])
            .add_filter("Images", &io::OPEN_EXTENSIONS[2..]);
        if let Some(dir) = self.dialog_dir() {
            d = d.set_directory(dir);
        }
        if let Some(files) = d.pick_files() {
            for f in files {
                self.open_path(&f);
            }
        }
    }

    /// Save as a layered project. Returns false if cancelled or failed.
    pub fn save(&mut self, ctx: &egui::Context, index: usize, save_as: bool) -> bool {
        let Some(tab) = self.docs.get(index) else {
            return false;
        };
        let path = match (&tab.path, save_as) {
            (Some(p), false) => p.clone(),
            _ => {
                let stem = Path::new(&tab.name)
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "Untitled".into());
                let mut d = rfd::FileDialog::new()
                    .set_title("Save Project")
                    .set_file_name(format!("{stem}.{}", io::PROJECT_EXTENSION))
                    .add_filter("Photo Studio project", &[io::PROJECT_EXTENSION])
                    .add_filter("Photoshop document", &["psd"]);
                if let Some(dir) =
                    tab.source.as_ref().and_then(|s| s.parent().map(Path::to_path_buf)).or_else(|| self.dialog_dir())
                {
                    d = d.set_directory(dir);
                }
                match d.save_file() {
                    Some(p) => p,
                    None => return false,
                }
            }
        };
        let is_psd = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("psd"));
        let tab = &self.docs[index];
        let result = if is_psd { io::save_psd(&tab.doc, &path) } else { io::save_project(&tab.doc, &path) };
        match result {
            Ok(()) => {
                let tab = &mut self.docs[index];
                if !is_psd {
                    tab.path = Some(path.clone());
                }
                tab.name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                tab.mark_saved();
                self.remember_recent(&path);
                self.notify(ctx, format!("Saved {}", path.display()));
                true
            }
            Err(e) => {
                self.notify(ctx, format!("Save failed: {e}"));
                false
            }
        }
    }

    pub fn export_to(&mut self, ctx: &egui::Context, path: &Path, opts: &io::ExportOptions, scale: f32) {
        let Some(tab) = self.tab() else { return };
        let result = if (scale - 1.0).abs() > 1e-3 {
            let mut d = tab.doc.clone();
            let w = ((d.width as f32 * scale).round() as u32).max(1);
            let h = ((d.height as f32 * scale).round() as u32).max(1);
            d.resize_image(w, h, studio_core::transform::Resample::Bicubic);
            io::export(&d, path, opts)
        } else {
            io::export(&tab.doc, path, opts)
        };
        match result {
            Ok(()) => {
                if let Some(dir) = path.parent() {
                    self.prefs.last_dir = Some(dir.to_path_buf());
                }
                self.notify(ctx, format!("Exported {}", path.display()));
            }
            Err(e) => self.notify(ctx, format!("Export failed: {e}")),
        }
    }

    pub fn close_doc(&mut self, index: usize) {
        if index < self.docs.len() {
            self.docs.remove(index);
            self.active = self.active.min(self.docs.len().saturating_sub(1));
            self.cv = CanvasState::default();
            self.stroke = None;
            self.dialog = None;
        }
    }

    pub fn request_close(&mut self, index: usize) {
        if self.docs.get(index).is_some_and(|d| d.is_dirty()) {
            self.confirm = Some(Confirm::Close(index));
        } else {
            self.close_doc(index);
        }
    }

    // ---- helpers used by many commands ----

    /// Run `f` on the active document and record `label` in history.
    pub fn edit(&mut self, label: &str, f: impl FnOnce(&mut Document)) {
        if let Some(tab) = self.tab_mut() {
            f(&mut tab.doc);
            tab.commit(label);
        }
    }

    /// Like `edit`, but only for a raster active layer (text and shapes are rasterised first).
    pub fn edit_pixels(&mut self, ctx: &egui::Context, label: &str, f: impl FnOnce(&mut Image, Option<&Mask>)) {
        let edit_mask = self.edit_mask;
        let Some(tab) = self.tab_mut() else { return };
        let sel = tab.doc.selection.clone();
        let Some(layer) = tab.doc.active_layer_mut() else {
            return;
        };
        if layer.locked {
            self.notify(ctx, "The layer is locked");
            return;
        }
        if edit_mask {
            if let Some(m) = layer.mask.as_mut() {
                let mut img = m.to_image();
                f(&mut img, sel.as_ref());
                *m = Mask::from_raw(img.width(), img.height(), img.pixels().chunks_exact(4).map(|p| p[0]).collect());
                tab.commit(label);
                return;
            }
        }
        match layer.kind {
            LayerKind::Adjustment(_) | LayerKind::Fill(_) => {
                self.notify(ctx, "Select a pixel layer first");
                return;
            }
            LayerKind::Text(_) | LayerKind::Shape(_) => {
                layer.rasterize();
            }
            LayerKind::Raster => {}
        }
        f(&mut layer.pixels, sel.as_ref());
        tab.commit(label);
    }

    fn filter_ctx(&self) -> FilterCtx {
        FilterCtx { fg: self.fg, bg: self.bg }
    }

    pub fn copy(&mut self, ctx: &egui::Context, merged: bool, cut: bool) {
        let Some(tab) = self.tab() else { return };
        let doc = &tab.doc;
        let src = if merged {
            doc.composite()
        } else {
            doc.active_layer().map(|l| l.pixels.clone()).unwrap_or_else(|| Image::new(0, 0))
        };
        if src.width() == 0 {
            return;
        }
        let mut px = src;
        if let Some(sel) = &doc.selection {
            let d = sel.data();
            for (i, p) in px.pixels_mut().chunks_exact_mut(4).enumerate() {
                p[3] = ((p[3] as u32 * d[i] as u32 + 127) / 255) as u8;
            }
        }
        let crop = px.opaque_bounds().unwrap_or(px.bounds());
        let out = px.crop(crop);
        if let Ok(mut cb) = arboard::Clipboard::new() {
            let _ = cb.set_image(arboard::ImageData {
                width: out.width() as usize,
                height: out.height() as usize,
                bytes: std::borrow::Cow::Owned(out.pixels().to_vec()),
            });
        }
        self.clipboard = Some(out);
        if cut {
            self.edit_pixels(ctx, "Cut", |img, sel| img.clear(sel));
        }
    }

    fn clipboard_image(&mut self) -> Option<Image> {
        if let Ok(mut cb) = arboard::Clipboard::new() {
            if let Ok(img) = cb.get_image() {
                if img.width > 0 && img.height > 0 && img.bytes.len() == img.width * img.height * 4 {
                    return Some(Image::from_raw(img.width as u32, img.height as u32, img.bytes.into_owned()));
                }
            }
        }
        self.clipboard.clone()
    }

    pub fn paste(&mut self, ctx: &egui::Context) {
        let Some(img) = self.clipboard_image() else {
            self.notify(ctx, "The clipboard has no image");
            return;
        };
        if self.docs.is_empty() {
            self.add_doc(DocTab::new(Document::from_image(img), "Pasted".into(), None));
            return;
        }
        self.edit("Paste", |doc| {
            let mut px = Image::new(doc.width, doc.height);
            // Paste centred on the selection if there is one, else on the canvas.
            let (cx, cy) = doc
                .selection
                .as_ref()
                .and_then(|m| m.bounds())
                .map(|b| (b.x + b.w as i32 / 2, b.y + b.h as i32 / 2))
                .unwrap_or((doc.width as i32 / 2, doc.height as i32 / 2));
            px.blit(&img, cx - img.width() as i32 / 2, cy - img.height() as i32 / 2);
            let id = doc.new_id();
            let name = doc.unique_name("Layer");
            doc.insert_layer(Layer::raster(id, name, px));
            doc.selection = None;
        });
        self.edit_mask = false;
    }

    pub fn apply_adjustment_now(&mut self, ctx: &egui::Context, adj: Adjustment) {
        let label = adj.label();
        self.edit_pixels(ctx, label, |img, sel| adjust::apply(img, &adj, sel));
    }

    pub fn apply_filter_now(&mut self, ctx: &egui::Context, f: Filter) {
        let fc = self.filter_ctx();
        let label = f.label();
        self.last_filter = Some(f.clone());
        self.edit_pixels(ctx, label, |img, sel| f.apply(img, sel, &fc));
    }

    fn modify_selection(&mut self, kind: ModifyKind, amount: f32) {
        self.edit(&format!("{} Selection", kind.label()), |doc| {
            if let Some(m) = &doc.selection {
                let out = match kind {
                    ModifyKind::Feather => select::feather(m, amount),
                    ModifyKind::Expand => select::expand(m, amount),
                    ModifyKind::Contract => select::contract(m, amount),
                    ModifyKind::Border => select::border(m, amount),
                    ModifyKind::Smooth => select::smooth(m, amount),
                };
                doc.set_selection(out);
            }
        });
    }

    pub fn apply_modify(&mut self, kind: ModifyKind, amount: f32) {
        self.modify_selection(kind, amount);
    }

    fn arrange(&mut self, label: &str, to: impl FnOnce(usize, usize) -> usize) {
        let Some(tab) = self.tab_mut() else { return };
        let n = tab.doc.layers.len();
        let from = tab.doc.active;
        let target = to(from, n).min(n - 1);
        if target != from {
            tab.doc.move_layer(from, target);
            tab.commit(label);
        }
    }

    /// Dispatch a command.
    pub fn run(&mut self, ctx: &egui::Context, a: Action) {
        let has_doc = !self.docs.is_empty();
        match a {
            Action::New => self.dialog = Some(Dialog::new_document(self.clipboard_size())),
            Action::NewFromClipboard => {
                if let Some(img) = self.clipboard_image() {
                    self.add_doc(DocTab::new(Document::from_image(img), "Clipboard".into(), None));
                } else {
                    self.notify(ctx, "The clipboard has no image");
                }
            }
            Action::Open => self.open_dialog(),
            Action::OpenPath(p) => self.open_path(&p),
            Action::Close => {
                if has_doc {
                    self.request_close(self.active);
                }
            }
            Action::Save => {
                if has_doc {
                    self.save(ctx, self.active, false);
                }
            }
            Action::SaveAs => {
                if has_doc {
                    self.save(ctx, self.active, true);
                }
            }
            Action::ExportAs => {
                if has_doc {
                    self.dialog = Some(Dialog::export(self.prefs.jpeg_quality));
                }
            }
            Action::QuickExportPng => {
                if let Some(tab) = self.tab() {
                    let stem = Path::new(&tab.name)
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "Untitled".into());
                    let mut d = rfd::FileDialog::new().set_file_name(format!("{stem}.png")).add_filter("PNG", &["png"]);
                    if let Some(dir) = self.dialog_dir() {
                        d = d.set_directory(dir);
                    }
                    if let Some(p) = d.save_file() {
                        self.export_to(ctx, &p, &io::ExportOptions::default(), 1.0);
                    }
                }
            }
            Action::Revert => {
                if let Some(tab) = self.tab_mut() {
                    tab.goto_history(0);
                    tab.commit("Revert");
                }
            }
            Action::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Action::Undo => {
                if self.cv.text_edit.is_some() || self.cv.transform.is_some() {
                    return;
                }
                if let Some(tab) = self.tab_mut() {
                    if let Some(l) = tab.undo() {
                        let msg = format!("Undo {l}");
                        self.notify(ctx, msg);
                    }
                }
            }
            Action::Redo | Action::StepBackward if self.cv.text_edit.is_some() => {}
            Action::Redo => {
                if let Some(tab) = self.tab_mut() {
                    if let Some(l) = tab.redo() {
                        let msg = format!("Redo {l}");
                        self.notify(ctx, msg);
                    }
                }
            }
            Action::StepBackward => self.run(ctx, Action::Undo),
            Action::Cut => self.copy(ctx, false, true),
            Action::Copy => self.copy(ctx, false, false),
            Action::CopyMerged => self.copy(ctx, true, false),
            Action::Paste => self.paste(ctx),
            Action::Clear => self.edit_pixels(ctx, "Clear", |img, sel| img.clear(sel)),
            Action::Fill => {
                if has_doc {
                    self.dialog = Some(Dialog::fill());
                }
            }
            Action::Stroke => {
                if self.tab().is_some_and(|t| t.doc.selection.is_some()) {
                    self.dialog = Some(Dialog::stroke());
                } else {
                    self.notify(ctx, "Make a selection to stroke");
                }
            }
            Action::FreeTransform => {
                if let Some(tab) = self.tab() {
                    if let Some(s) = crate::canvas::TransformSession::begin(&tab.doc) {
                        self.cv.transform = Some(s);
                        self.set_tool(Tool::Move);
                    }
                }
            }
            Action::FlipLayer(h) => {
                self.transform_layer(ctx, if h { "Flip Horizontal" } else { "Flip Vertical" }, |w, hh| {
                    let (cx, cy) = (w / 2.0, hh / 2.0);
                    studio_core::geom::Affine::translate(-cx, -cy)
                        .then(&studio_core::geom::Affine::scale(if h { -1.0 } else { 1.0 }, if h { 1.0 } else { -1.0 }))
                        .then(&studio_core::geom::Affine::translate(cx, cy))
                })
            }
            Action::RotateLayer(r) => {
                let deg = match r {
                    studio_core::transform::Rotation::Cw90 => 90.0f32,
                    studio_core::transform::Rotation::Ccw90 => -90.0,
                    studio_core::transform::Rotation::Half => 180.0,
                };
                self.transform_layer(ctx, "Rotate Layer", |w, h| {
                    let (cx, cy) = (w / 2.0, h / 2.0);
                    studio_core::geom::Affine::translate(-cx, -cy)
                        .then(&studio_core::geom::Affine::rotate(deg.to_radians()))
                        .then(&studio_core::geom::Affine::translate(cx, cy))
                });
            }
            Action::Preferences => self.dialog = Some(Dialog::Preferences),
            Action::DefinePattern => {
                let Some(tab) = self.tab() else { return };
                let Some(b) = tab.doc.selection.as_ref().and_then(|m| m.bounds()) else {
                    self.notify(ctx, "Select an area to use as a pattern");
                    return;
                };
                if b.w > 1024 || b.h > 1024 {
                    self.notify(ctx, "Patterns can be at most 1024 × 1024 px");
                    return;
                }
                self.custom_pattern = Some(tab.composite.crop(b));
                self.opts.pattern = studio_core::paint::builtin_patterns(self.fg, self.bg).len();
                self.notify(ctx, format!("Pattern defined ({} × {})", b.w, b.h));
            }
            Action::Straighten => {
                let Some((a, b)) = self.cv.ruler else { return };
                let mut angle = (b.1 - a.1).atan2(b.0 - a.0).to_degrees();
                // Level whichever axis the line is closest to.
                if angle.abs() > 45.0 && angle.abs() < 135.0 {
                    angle -= 90.0 * angle.signum();
                } else if angle.abs() >= 135.0 {
                    angle -= 180.0 * angle.signum();
                }
                if angle.abs() > 0.01 {
                    self.edit("Straighten", |d| {
                        d.rotate_arbitrary(-angle);
                    });
                    self.cv.ruler = None;
                    if let Some(t) = self.tab_mut() {
                        t.fit_pending = true;
                    }
                }
            }
            Action::Adjust(adj) => {
                if has_doc {
                    if matches!(adj, Adjustment::Invert | Adjustment::Desaturate) {
                        self.apply_adjustment_now(ctx, adj);
                    } else if let Some(tab) = self.tab() {
                        self.dialog = Some(Dialog::adjust(adj, &tab.doc));
                    }
                }
            }
            Action::AutoTone | Action::AutoContrast | Action::AutoColor => {
                let Some(tab) = self.tab() else { return };
                let Some(layer) = tab.doc.active_layer() else {
                    return;
                };
                if layer.pixels.width() == 0 {
                    return;
                }
                let adj = match a {
                    Action::AutoTone => adjust::auto_tone(&layer.pixels, 0.001),
                    Action::AutoContrast => adjust::auto_contrast(&layer.pixels, 0.001),
                    _ => adjust::auto_color(&layer.pixels),
                };
                let label = match a {
                    Action::AutoTone => "Auto Tone",
                    Action::AutoContrast => "Auto Contrast",
                    _ => "Auto Color",
                };
                self.edit_pixels(ctx, label, |img, sel| adjust::apply(img, &adj, sel));
            }
            Action::Equalize => self.edit_pixels(ctx, "Equalize", adjust::equalize),
            Action::ShadowsHighlights => {
                if let Some(tab) = self.tab() {
                    self.dialog = Some(Dialog::shadows_highlights(&tab.doc));
                }
            }
            Action::ImageSize => {
                if let Some(tab) = self.tab() {
                    self.dialog = Some(Dialog::image_size(&tab.doc));
                }
            }
            Action::CanvasSize => {
                if let Some(tab) = self.tab() {
                    self.dialog = Some(Dialog::canvas_size(&tab.doc));
                }
            }
            Action::RotateCanvas(r) => self.edit("Rotate Canvas", |d| d.rotate(r)),
            Action::RotateArbitrary => {
                if has_doc {
                    self.dialog = Some(Dialog::RotateArbitrary { angle: 15.0 });
                }
            }
            Action::FlipCanvas(h) => {
                self.edit(if h { "Flip Canvas Horizontal" } else { "Flip Canvas Vertical" }, |d| d.flip(h))
            }
            Action::CropToSelection => {
                if self.tab().is_some_and(|t| t.doc.selection.is_some()) {
                    self.edit("Crop", |d| d.crop_to_selection());
                } else if has_doc {
                    self.set_tool(Tool::Crop);
                }
            }
            Action::Trim => self.edit("Trim", |d| d.trim()),
            Action::DuplicateImage => {
                if let Some(tab) = self.tab() {
                    let doc = tab.doc.clone();
                    let name = format!(
                        "{} copy",
                        Path::new(&tab.name).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
                    );
                    self.add_doc(DocTab::new(doc, name, None));
                }
            }
            Action::NewLayer => {
                self.edit("New Layer", |d| {
                    let n = d.unique_name("Layer");
                    d.add_empty_layer(&n);
                });
                self.edit_mask = false;
            }
            Action::NewFillSolid => {
                let c = self.fg;
                self.edit("New Fill Layer", |d| {
                    d.add_layer_of_kind("Color Fill", LayerKind::Fill(FillLayer::Solid(c)));
                });
            }
            Action::NewFillGradient => {
                let (fg, bg) = (self.fg, self.bg);
                self.edit("New Fill Layer", |d| {
                    let (w, h) = (d.width as f32, d.height as f32);
                    d.add_layer_of_kind(
                        "Gradient Fill",
                        LayerKind::Fill(FillLayer::Gradient {
                            gradient: studio_core::gradient::Gradient::two(fg, bg),
                            shape: studio_core::gradient::GradientShape::Linear,
                            start: (w / 2.0, 0.0),
                            end: (w / 2.0, h),
                        }),
                    );
                });
            }
            Action::NewAdjustmentLayer(adj) => {
                let name = adj.label().to_string();
                self.edit(&format!("New {name} Layer"), |d| {
                    d.add_layer_of_kind(&name, LayerKind::Adjustment(adj));
                });
                self.props = ParamState::default();
                self.show_right_panel = true;
            }
            Action::DuplicateLayer => self.edit("Duplicate Layer", |d| {
                d.duplicate_layer(d.active);
            }),
            Action::DeleteLayer => {
                if self.tab().is_some_and(|t| t.doc.layers.len() > 1) {
                    self.edit("Delete Layer", |d| {
                        d.delete_layer(d.active);
                    });
                    self.edit_mask = false;
                }
            }
            Action::LayerViaCopy => self.edit("Layer via Copy", |d| {
                d.layer_via(false);
            }),
            Action::LayerViaCut => self.edit("Layer via Cut", |d| {
                d.layer_via(true);
            }),
            Action::LayerStyle => {
                if let Some(tab) = self.tab() {
                    if tab.doc.active_layer().is_some_and(|l| !l.is_adjustment()) {
                        self.dialog = Some(Dialog::layer_style(&tab.doc));
                    }
                }
            }
            Action::ClearLayerStyle => self.edit("Clear Layer Style", |d| {
                if let Some(l) = d.active_layer_mut() {
                    l.styles.disable_all();
                }
            }),
            Action::MaskRevealAll | Action::MaskHideAll | Action::MaskFromSelection => {
                let reveal = !matches!(a, Action::MaskHideAll);
                let use_sel = matches!(a, Action::MaskFromSelection);
                self.edit("Add Layer Mask", |d| {
                    if !use_sel {
                        let sel = d.selection.take();
                        d.add_mask(reveal);
                        d.selection = sel;
                    } else {
                        d.add_mask(true);
                        d.selection = None;
                    }
                });
                self.edit_mask = true;
            }
            Action::MaskApply => {
                self.edit("Apply Layer Mask", |d| d.apply_mask());
                self.edit_mask = false;
            }
            Action::MaskDelete => {
                self.edit("Delete Layer Mask", |d| {
                    if let Some(l) = d.active_layer_mut() {
                        l.mask = None;
                    }
                });
                self.edit_mask = false;
            }
            Action::MaskToggle => self.edit("Toggle Layer Mask", |d| {
                if let Some(l) = d.active_layer_mut() {
                    l.mask_enabled = !l.mask_enabled;
                }
            }),
            Action::MaskInvert => self.edit("Invert Layer Mask", |d| {
                if let Some(m) = d.active_layer_mut().and_then(|l| l.mask.as_mut()) {
                    m.invert();
                }
            }),
            Action::ToggleClipping => self.edit("Clipping Mask", |d| {
                let i = d.active;
                if i > 0 {
                    if let Some(l) = d.active_layer_mut() {
                        l.clip = !l.clip;
                    }
                }
            }),
            Action::Rasterize => self.edit("Rasterize Layer", |d| {
                if let Some(l) = d.active_layer_mut() {
                    l.rasterize();
                }
            }),
            Action::BringToFront => self.arrange("Bring to Front", |_, n| n - 1),
            Action::BringForward => self.arrange("Bring Forward", |i, _| i + 1),
            Action::SendBackward => self.arrange("Send Backward", |i, _| i.saturating_sub(1)),
            Action::SendToBack => self.arrange("Send to Back", |_, _| 0),
            Action::MergeDown => {
                if self.tab().is_some_and(|t| t.doc.active > 0) {
                    self.edit("Merge Down", |d| {
                        d.merge_down(d.active);
                    });
                }
            }
            Action::MergeVisible => self.edit("Merge Visible", |d| d.merge_visible()),
            Action::Flatten => self.edit("Flatten Image", |d| d.flatten_image()),
            Action::SelectLayerAbove | Action::SelectLayerBelow => {
                if let Some(tab) = self.tab_mut() {
                    let n = tab.doc.layers.len();
                    tab.doc.active = if a == Action::SelectLayerAbove {
                        (tab.doc.active + 1).min(n - 1)
                    } else {
                        tab.doc.active.saturating_sub(1)
                    };
                    self.edit_mask = false;
                }
            }
            Action::SelectAll => self.edit("Select All", |d| d.select_all()),
            Action::Deselect => {
                if let Some(sel) = self.tab().and_then(|t| t.doc.selection.clone()) {
                    self.last_selection = Some(sel);
                    self.edit("Deselect", |d| d.deselect());
                }
            }
            Action::Reselect => {
                if let Some(sel) = self.last_selection.clone() {
                    self.edit("Reselect", |d| {
                        if (sel.width(), sel.height()) == (d.width, d.height) {
                            d.set_selection(sel);
                        }
                    });
                }
            }
            Action::InverseSelection => self.edit("Inverse", |d| d.invert_selection()),
            Action::SelectSubject => {
                let Some(tab) = self.tab() else { return };
                let img = tab.doc.composite();
                let m = select::select_subject(&img);
                self.edit("Select Subject", |d| d.set_selection(m));
            }
            Action::ColorRange => {
                if let Some(tab) = self.tab() {
                    self.dialog = Some(Dialog::color_range(self.fg, &tab.doc));
                }
            }
            Action::LoadLayerSelection => self.edit("Load Selection", |d| d.select_layer_pixels(d.active)),
            Action::Modify(kind) => {
                if self.tab().is_some_and(|t| t.doc.selection.is_some()) {
                    self.dialog =
                        Some(Dialog::Modify { kind, amount: if kind == ModifyKind::Smooth { 4.0 } else { 8.0 } });
                } else {
                    self.notify(ctx, "Make a selection first");
                }
            }
            Action::RepeatFilter => {
                if let Some(f) = self.last_filter.clone() {
                    self.apply_filter_now(ctx, f);
                }
            }
            Action::Filter(f) => {
                if has_doc {
                    if f.has_params() {
                        if let Some(tab) = self.tab() {
                            self.dialog = Some(Dialog::filter(f, &tab.doc));
                        }
                    } else {
                        self.apply_filter_now(ctx, f);
                    }
                }
            }
            Action::ZoomIn | Action::ZoomOut => {
                let up = a == Action::ZoomIn;
                let center = self.cv.view_rect.center();
                if let Some(tab) = self.tab_mut() {
                    let z = crate::doc::zoom_step(tab.zoom, up);
                    let f = z / tab.zoom;
                    tab.zoom_at(f, center, center);
                }
            }
            Action::FitOnScreen => {
                let size = self.cv.view_rect.size();
                if let Some(tab) = self.tab_mut() {
                    tab.fit(size);
                }
            }
            Action::ActualPixels => {
                if let Some(tab) = self.tab_mut() {
                    tab.zoom = 1.0;
                    tab.pan = egui::Vec2::ZERO;
                }
            }
            Action::TogglePixelGrid => self.prefs.pixel_grid = !self.prefs.pixel_grid,
            Action::ToggleSelectionEdges => self.show_edges = !self.show_edges,
            Action::ToggleTheme => {
                let dark = ctx.global_style().visuals.dark_mode;
                self.prefs.theme = if dark { ThemeMode::Light } else { ThemeMode::Dark };
                crate::theme::apply(ctx, self.prefs.theme);
            }
            Action::About => self.dialog = Some(Dialog::About),
            Action::Shortcuts => self.dialog = Some(Dialog::Shortcuts),
            Action::Website => ctx.open_url(egui::OpenUrl::new_tab("https://photo-studio.devops-monk.com")),
            Action::SwapColors => std::mem::swap(&mut self.fg, &mut self.bg),
            Action::DefaultColors => {
                self.fg = Rgba::BLACK;
                self.bg = Rgba::WHITE;
            }
            Action::BrushSmaller | Action::BrushLarger => {
                let up = a == Action::BrushLarger;
                let tip = if self.tool == Tool::Eraser { &mut self.opts.eraser_tip } else { &mut self.opts.tip };
                let step = if tip.size < 10.0 {
                    1.0
                } else if tip.size < 50.0 {
                    5.0
                } else if tip.size < 200.0 {
                    10.0
                } else {
                    50.0
                };
                tip.size = (tip.size + if up { step } else { -step }).clamp(1.0, 2500.0);
            }
        }
    }

    fn transform_layer(
        &mut self,
        ctx: &egui::Context,
        label: &str,
        make: impl FnOnce(f32, f32) -> studio_core::geom::Affine,
    ) {
        let Some(tab) = self.tab_mut() else { return };
        if tab.doc.active_layer().is_none_or(|l| l.is_adjustment() || l.locked) {
            self.notify(ctx, "Select an unlocked pixel layer");
            return;
        }
        let (w, h) = (tab.doc.width as f32, tab.doc.height as f32);
        let t = make(w, h);
        tab.doc.transform_active(&t);
        tab.commit(label);
    }

    fn clipboard_size(&mut self) -> Option<(u32, u32)> {
        self.clipboard.as_ref().map(|i| (i.width(), i.height()))
    }

    /// End a text-editing session, deleting the layer if the text is empty.
    pub fn end_text_edit(&mut self, te: crate::canvas::TextEdit) {
        let Some(tab) = self.tab_mut() else { return };
        let Some(idx) = tab.doc.index_of(te.layer_id) else {
            return;
        };
        let empty = matches!(&tab.doc.layers[idx].kind, LayerKind::Text(t) if t.text.trim().is_empty());
        if empty {
            tab.doc.delete_layer(idx);
            tab.invalidate_all();
            if !te.is_new {
                tab.commit("Delete Text");
            }
        } else {
            let name: String = match &tab.doc.layers[idx].kind {
                LayerKind::Text(t) => t.text.lines().next().unwrap_or("Text").chars().take(28).collect(),
                _ => "Text".into(),
            };
            tab.doc.layers[idx].name = name;
            tab.commit(if te.is_new { "Type Layer" } else { "Edit Type" });
        }
    }

    /// Record pending Properties-panel edits once the user pauses.
    pub fn flush_props(&mut self, now: f64, force: bool) {
        if let Some((t, label)) = self.props_pending {
            if force || now - t > 0.6 {
                self.props_pending = None;
                if let Some(tab) = self.tab_mut() {
                    tab.commit(label);
                }
            }
        }
    }

    pub fn push_swatch(&mut self, c: Rgba) {
        self.prefs.swatches.retain(|s| *s != c);
        self.prefs.swatches.insert(0, c);
        self.prefs.swatches.truncate(24);
    }
}

/// The pattern at `index`: built-ins first, then the user's defined pattern.
pub fn pattern_at(custom: &Option<Image>, index: usize, fg: Rgba, bg: Rgba) -> Option<Image> {
    let mut all: Vec<Image> = studio_core::paint::builtin_patterns(fg, bg).into_iter().map(|p| p.1).collect();
    if let Some(c) = custom {
        all.push(c.clone());
    }
    all.get(index).cloned().or_else(|| all.first().cloned())
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        crate::shell::frame(self, ui);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.save_prefs(storage);
    }

    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        visuals.panel_fill.to_normalized_gamma_f32()
    }
}
