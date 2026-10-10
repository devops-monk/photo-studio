//! The canvas view: drawing the document and handling every tool's pointer interaction.

use egui::{Color32, CursorIcon, Pos2, Rect as ERect, Sense, Shape, Stroke, Ui, Vec2};
use studio_core::Rgba;
use studio_core::document::{DocState, Document};
use studio_core::filters::FilterCtx;
use studio_core::geom::{Affine, Rect};
use studio_core::gradient::Gradient;
use studio_core::image::Image;
use studio_core::layer::{LayerId, LayerKind};
use studio_core::paint::{self, BrushTip, StrokeEngine, StrokeKind};
use studio_core::select::{self, SelectOp};
use studio_core::shape::ShapeLayer;
use studio_core::text::TextLayer;

use crate::app::{ActiveStroke, App, StrokeTarget};
use crate::theme::Tokens;
use crate::tools::Tool;

pub struct TextEdit {
    pub layer_id: LayerId,
    pub is_new: bool,
}

pub struct CropBox {
    pub a: (f32, f32),
    pub b: (f32, f32),
    drag: Option<(usize, (f32, f32), (f32, f32), (f32, f32))>,
}

impl CropBox {
    fn rect(&self) -> (f32, f32, f32, f32) {
        (self.a.0.min(self.b.0), self.a.1.min(self.b.1), self.a.0.max(self.b.0), self.a.1.max(self.b.1))
    }

    pub fn canvas_rect(&self, w: u32, h: u32) -> Rect {
        let (x0, y0, x1, y1) = self.rect();
        Rect::from_points((x0, y0), (x1, y1)).intersect(&Rect::sized(w, h))
    }
}

enum Drag {
    Marquee { start: (f32, f32), cur: (f32, f32), op: SelectOp, ellipse: bool, constrain: bool },
    Lasso { points: Vec<(f32, f32)>, op: SelectOp },
    Move { start: (f32, f32), base: DocState, applied: (i32, i32) },
    Pan { start_pan: Vec2, start: Pos2 },
    Gradient { start: (f32, f32), cur: (f32, f32) },
    Shape { start: (f32, f32), layer_id: LayerId },
    QuickSelect { subtract: bool },
    Eyedrop,
}

#[derive(Clone, Copy)]
enum TDrag {
    Move { start: (f32, f32), t0: (f32, f32) },
    Rotate { a0: f32, r0: f32 },
    Scale { handle: usize, start: (f32, f32, f32, f32) },
}

/// Free Transform of the active layer (or its selected pixels).
pub struct TransformSession {
    base: DocState,
    /// Content bounds before transforming: x0, y0, x1, y1.
    bounds: (f32, f32, f32, f32),
    sx: f32,
    sy: f32,
    rot: f32,
    tx: f32,
    ty: f32,
    drag: Option<TDrag>,
    applied: Option<[f32; 5]>,
}

impl TransformSession {
    pub fn begin(doc: &Document) -> Option<Self> {
        let layer = doc.active_layer()?;
        if layer.is_adjustment() || layer.locked {
            return None;
        }
        let b = match &doc.selection {
            Some(m) => m.bounds()?,
            None => layer.pixels.opaque_bounds()?,
        };
        Some(TransformSession {
            base: doc.state(),
            bounds: (b.x as f32, b.y as f32, b.right() as f32, b.bottom() as f32),
            sx: 1.0,
            sy: 1.0,
            rot: 0.0,
            tx: 0.0,
            ty: 0.0,
            drag: None,
            applied: None,
        })
    }

    fn center(&self) -> (f32, f32) {
        ((self.bounds.0 + self.bounds.2) / 2.0, (self.bounds.1 + self.bounds.3) / 2.0)
    }

    pub fn affine(&self) -> Affine {
        let (cx, cy) = self.center();
        Affine::translate(-cx, -cy)
            .then(&Affine::scale(self.sx, self.sy))
            .then(&Affine::rotate(self.rot))
            .then(&Affine::translate(cx + self.tx, cy + self.ty))
    }

    /// Corner points (TL, TR, BR, BL) after transforming.
    fn corners(&self) -> [(f32, f32); 4] {
        let t = self.affine();
        let (x0, y0, x1, y1) = self.bounds;
        [t.apply(x0, y0), t.apply(x1, y0), t.apply(x1, y1), t.apply(x0, y1)]
    }

    /// Handles: 0..4 corners, 4..8 edge midpoints (top, right, bottom, left).
    fn handles(&self) -> [(f32, f32); 8] {
        let c = self.corners();
        let mid = |a: (f32, f32), b: (f32, f32)| ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
        [c[0], c[1], c[2], c[3], mid(c[0], c[1]), mid(c[1], c[2]), mid(c[2], c[3]), mid(c[3], c[0])]
    }

    /// Point `p` in the box's unrotated, unscaled-centre frame.
    fn local(&self, p: (f32, f32), st: (f32, f32, f32, f32)) -> (f32, f32) {
        let (cx, cy) = self.center();
        let (dx, dy) = (p.0 - cx - st.2, p.1 - cy - st.3);
        let (s, c) = self.rot.sin_cos();
        (dx * c + dy * s, -dx * s + dy * c)
    }

    fn key(&self) -> [f32; 5] {
        [self.sx, self.sy, self.rot, self.tx, self.ty]
    }
}

pub struct CanvasState {
    pub view_rect: ERect,
    drag: Option<Drag>,
    pub poly: Vec<(f32, f32)>,
    pub crop: Option<CropBox>,
    pub transform: Option<TransformSession>,
    pub text_edit: Option<TextEdit>,
    pub clone_source: Option<(f32, f32)>,
    clone_offset: Option<(f32, f32)>,
    pub hover: Option<(f32, f32)>,
    last_paint: Option<(f32, f32)>,
}

impl Default for CanvasState {
    fn default() -> Self {
        CanvasState {
            view_rect: ERect::from_min_size(Pos2::ZERO, Vec2::new(1200.0, 800.0)),
            drag: None,
            poly: Vec::new(),
            crop: None,
            transform: None,
            text_edit: None,
            clone_source: None,
            clone_offset: None,
            hover: None,
            last_paint: None,
        }
    }
}

struct View {
    origin: Pos2,
    zoom: f32,
}

impl View {
    fn to_canvas(&self, p: Pos2) -> (f32, f32) {
        ((p.x - self.origin.x) / self.zoom, (p.y - self.origin.y) / self.zoom)
    }
    fn to_screen(&self, p: (f32, f32)) -> Pos2 {
        Pos2::new(self.origin.x + p.0 * self.zoom, self.origin.y + p.1 * self.zoom)
    }
}

fn select_op(m: egui::Modifiers, default: SelectOp) -> SelectOp {
    match (m.shift, m.alt) {
        (true, true) => SelectOp::Intersect,
        (true, false) => SelectOp::Add,
        (false, true) => SelectOp::Subtract,
        _ => default,
    }
}

pub fn show(app: &mut App, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let (view_rect, resp) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
    app.cv.view_rect = view_rect;
    let painter = ui.painter_at(view_rect);
    painter.rect_filled(view_rect, 0.0, t.canvas);
    if app.docs.is_empty() {
        return;
    }
    let ctx = ui.ctx().clone();
    let active = app.active;

    // ---- view navigation (wheel, pinch, keyboard) ----
    {
        let tab = &mut app.docs[active];
        if tab.fit_pending {
            tab.fit(view_rect.size());
            tab.fit_pending = false;
        }
        tab.refresh(&ctx);
        if resp.hovered() || resp.dragged() {
            let (zoom_delta, scroll, pointer) =
                ctx.input(|i| (i.zoom_delta(), i.smooth_scroll_delta, i.pointer.hover_pos()));
            if (zoom_delta - 1.0).abs() > 1e-4 {
                let anchor = pointer.unwrap_or(view_rect.center());
                tab.zoom_at(zoom_delta, anchor, view_rect.center());
            } else if scroll != Vec2::ZERO {
                tab.pan += scroll;
            }
        }
    }

    let tab = &app.docs[active];
    let (dw, dh) = (tab.doc.width as f32, tab.doc.height as f32);
    let zoom = tab.zoom;
    let origin = view_rect.center() + tab.pan - Vec2::new(dw, dh) * zoom / 2.0;
    let view = View { origin, zoom };
    let img_rect = ERect::from_min_size(origin, Vec2::new(dw, dh) * zoom);

    // ---- draw document ----
    painter.rect_filled(
        img_rect.translate(Vec2::new(0.0, 6.0)).expand(2.0),
        4.0,
        Color32::from_black_alpha(if t.dark { 70 } else { 30 }),
    );
    crate::widgets::checker(&painter, img_rect, 8.0);
    tab.paint(&painter, origin);
    if app.prefs.pixel_grid && zoom >= 10.0 {
        let vis = img_rect.intersect(view_rect);
        let (cx0, cy0) = view.to_canvas(vis.min);
        let (cx1, cy1) = view.to_canvas(vis.max);
        let grid = Stroke::new(1.0, Color32::from_white_alpha(28));
        for x in cx0.floor() as i32..=cx1.ceil() as i32 {
            let sx = view.to_screen((x as f32, 0.0)).x;
            painter.vline(sx, vis.y_range(), grid);
        }
        for y in cy0.floor() as i32..=cy1.ceil() as i32 {
            let sy = view.to_screen((0.0, y as f32)).y;
            painter.hline(vis.x_range(), sy, grid);
        }
    }

    let time = ctx.input(|i| i.time);
    let mods = ctx.input(|i| i.modifiers);
    let pointer = resp.hover_pos().or_else(|| resp.interact_pointer_pos());
    app.cv.hover = pointer.filter(|p| view_rect.contains(*p)).map(|p| view.to_canvas(p));

    // ---- selection outline ----
    if app.show_edges {
        draw_ants(app, &painter, &view, time);
    }

    // ---- pointer handling ----
    let space = ctx.input(|i| i.key_down(egui::Key::Space)) && app.cv.text_edit.is_none();
    let middle = ctx.input(|i| i.pointer.middle_down());
    let panning_tool = app.tool == Tool::Hand || space || middle;

    if resp.drag_started() && (panning_tool || ctx.input(|i| i.pointer.button_down(egui::PointerButton::Middle))) {
        let tab = &app.docs[active];
        app.cv.drag = Some(Drag::Pan { start_pan: tab.pan, start: resp.interact_pointer_pos().unwrap_or_default() });
    }
    if let Some(Drag::Pan { start_pan, start }) = &app.cv.drag {
        if let Some(p) = resp.interact_pointer_pos() {
            let np = *start_pan + (p - *start);
            app.docs[active].pan = np;
        }
        if !resp.dragged() {
            app.cv.drag = None;
        }
        ctx.set_cursor_icon(CursorIcon::Grabbing);
    } else if panning_tool {
        ctx.set_cursor_icon(CursorIcon::Grab);
        if resp.double_clicked() && app.tool == Tool::Hand {
            app.docs[active].fit(view_rect.size());
        }
    } else if app.cv.transform.is_some() {
        handle_transform(app, &resp, &view, mods);
    } else {
        handle_tool(app, &ctx, &resp, &view, mods);
    }

    // ---- overlays ----
    draw_overlays(app, &painter, &view, pointer, time);

    // Keep animating ants and in-progress work.
    if app.docs.get(active).is_some_and(|t| t.doc.selection.is_some()) && app.show_edges {
        ctx.request_repaint_after(std::time::Duration::from_millis(120));
    }

    text_editor(app, ui, &view);
}

fn sample_source(app: &App) -> Option<Image> {
    let tab = app.tab()?;
    if app.opts.sample_merged {
        Some(tab.composite.clone())
    } else {
        tab.doc.active_layer().map(|l| if l.pixels.width() == 0 { tab.composite.clone() } else { l.pixels.clone() })
    }
}

fn begin_stroke(app: &mut App, ctx: &egui::Context, p: (f32, f32)) -> bool {
    let tool = app.tool;
    let opts = app.opts.clone();
    let (fg, bg) = (app.fg, app.bg);
    let edit_mask = app.edit_mask;
    let Some(tab) = app.docs.get_mut(app.active) else {
        return false;
    };
    let sel = tab.doc.selection.clone();
    let Some(layer) = tab.doc.active_layer_mut() else {
        return false;
    };
    if layer.locked {
        app.toast = Some(("The layer is locked".into(), ctx.input(|i| i.time)));
        return false;
    }
    let mask_target = edit_mask && layer.mask.is_some();
    if !mask_target {
        match layer.kind {
            LayerKind::Raster => {}
            LayerKind::Text(_) | LayerKind::Shape(_) => layer.rasterize(),
            _ => {
                app.toast = Some(("Pick a pixel layer (or its mask) to paint on".into(), ctx.input(|i| i.time)));
                return false;
            }
        }
    }
    let target_img = if mask_target { layer.mask.as_ref().unwrap().to_image() } else { layer.pixels.clone() };
    let mask_gray = |c: Rgba| {
        let l = studio_core::color::luma(c.r() as f32, c.g() as f32, c.b() as f32).round() as u8;
        Rgba::rgb(l, l, l)
    };
    let tip = if tool == Tool::Eraser { opts.eraser_tip } else { opts.tip };
    let (kind, label, tip) = match tool {
        Tool::Brush => (
            StrokeKind::Paint { color: if mask_target { mask_gray(fg) } else { fg }, blend: opts.brush_blend },
            "Brush",
            tip,
        ),
        Tool::Pencil => (
            StrokeKind::Paint { color: if mask_target { mask_gray(fg) } else { fg }, blend: opts.brush_blend },
            "Pencil",
            BrushTip { aliased: true, hardness: 1.0, ..tip },
        ),
        Tool::Eraser if mask_target => {
            (StrokeKind::Paint { color: mask_gray(bg), blend: studio_core::BlendMode::Normal }, "Eraser", tip)
        }
        Tool::Eraser => (StrokeKind::Erase, "Eraser", tip),
        Tool::CloneStamp | Tool::Heal => {
            let Some(src) = app.cv.clone_source else {
                app.toast = Some(("Alt-click to set a source point first".into(), ctx.input(|i| i.time)));
                return false;
            };
            let offset = *app.cv.clone_offset.get_or_insert((src.0 - p.0, src.1 - p.1));
            let source = if opts.sample_merged { tab.composite.clone() } else { target_img.clone() };
            if tool == Tool::Heal {
                (paint::heal_kind(source, &target_img, offset, tip.size), "Healing Brush", tip)
            } else {
                (StrokeKind::Clone { source, offset }, "Clone Stamp", tip)
            }
        }
        Tool::SpotHeal => {
            let offset = paint::spot_heal_offset(&target_img, p.0, p.1, tip.size);
            (paint::heal_kind(target_img.clone(), &target_img, offset, tip.size), "Spot Healing", tip)
        }
        Tool::Blur => (StrokeKind::Blur { strength: opts.strength }, "Blur Tool", tip),
        Tool::Sharpen => (StrokeKind::Sharpen { strength: opts.strength }, "Sharpen Tool", tip),
        Tool::Smudge => (StrokeKind::Smudge { strength: opts.strength }, "Smudge", tip),
        Tool::Dodge => (StrokeKind::Dodge { range: opts.tone_range, exposure: opts.exposure }, "Dodge", tip),
        Tool::Burn => (StrokeKind::Burn { range: opts.tone_range, exposure: opts.exposure }, "Burn", tip),
        Tool::Sponge => (StrokeKind::Sponge { saturate: opts.sponge_saturate, flow: opts.flow }, "Sponge", tip),
        _ => return false,
    };
    let engine = StrokeEngine::new(&target_img, kind, tip, opts.opacity, opts.flow, sel);
    let target = if mask_target { StrokeTarget::Mask(target_img) } else { StrokeTarget::Pixels };
    app.stroke = Some(ActiveStroke { engine, target, label });
    true
}

fn continue_stroke(app: &mut App, p: (f32, f32), pressure: f32) {
    let Some(st) = app.stroke.as_mut() else {
        return;
    };
    let Some(tab) = app.docs.get_mut(app.active) else {
        return;
    };
    let Some(layer) = tab.doc.active_layer_mut() else {
        return;
    };
    let dirty = match &mut st.target {
        StrokeTarget::Pixels => st.engine.stroke_to(&mut layer.pixels, p.0, p.1, pressure),
        StrokeTarget::Mask(img) => {
            let d = st.engine.stroke_to(img, p.0, p.1, pressure);
            if let Some(m) = layer.mask.as_mut() {
                let w = m.width() as usize;
                let src = img.pixels();
                let md = m.data_mut();
                for y in d.y..d.bottom() {
                    for x in d.x..d.right() {
                        let i = y as usize * w + x as usize;
                        md[i] = src[i * 4];
                    }
                }
            }
            d
        }
    };
    // Styles and adjustment layers above can reach beyond the dab, so refresh generously.
    if layer.styles.any() {
        tab.invalidate_all();
    } else {
        tab.invalidate(dirty.inflate(2));
    }
}

fn end_stroke(app: &mut App) {
    if let Some(st) = app.stroke.take() {
        if let Some(tab) = app.tab_mut() {
            tab.commit(st.label);
        }
    }
}

fn handle_tool(app: &mut App, ctx: &egui::Context, resp: &egui::Response, view: &View, mods: egui::Modifiers) {
    let tool = app.tool;
    let pos = resp.interact_pointer_pos().or(resp.hover_pos());
    let Some(pos) = pos else { return };
    let p = view.to_canvas(pos);
    let primary_pressed =
        resp.drag_started_by(egui::PointerButton::Primary) || resp.clicked_by(egui::PointerButton::Primary);
    // Drags start where the button went down, not where the drag threshold was crossed.
    let origin = ctx.input(|i| i.pointer.press_origin()).map(|o| view.to_canvas(o)).unwrap_or(p);
    let active = app.active;

    // Cursor.
    let cursor = match tool {
        t if t.uses_brush() && tool != Tool::QuickSelect => CursorIcon::None,
        Tool::Move => CursorIcon::Move,
        Tool::Text => CursorIcon::Text,
        Tool::Zoom => {
            if mods.alt {
                CursorIcon::ZoomOut
            } else {
                CursorIcon::ZoomIn
            }
        }
        _ => CursorIcon::Crosshair,
    };
    if resp.hovered() {
        ctx.set_cursor_icon(cursor);
    }

    match tool {
        t if t.uses_brush() && t != Tool::QuickSelect => {
            if matches!(t, Tool::CloneStamp | Tool::Heal) && mods.alt && primary_pressed {
                app.cv.clone_source = Some(p);
                app.cv.clone_offset = None;
                return;
            }
            if primary_pressed && app.stroke.is_none() {
                let start = if mods.shift { app.cv.last_paint.unwrap_or(origin) } else { origin };
                if begin_stroke(app, ctx, start) {
                    continue_stroke(app, start, 1.0);
                    if start != p {
                        continue_stroke(app, p, 1.0);
                    }
                }
            } else if resp.dragged() && app.stroke.is_some() {
                let pressure = ctx.input(|i| {
                    i.events.iter().rev().find_map(|e| match e {
                        egui::Event::Touch { force: Some(f), .. } => Some(*f),
                        _ => None,
                    })
                });
                continue_stroke(app, p, pressure.unwrap_or(1.0));
            }
            if app.stroke.is_some()
                && (resp.drag_stopped() || resp.clicked() || !ctx.input(|i| i.pointer.primary_down()))
            {
                app.cv.last_paint = Some(p);
                end_stroke(app);
            }
        }
        Tool::QuickSelect => {
            if primary_pressed {
                app.cv.drag = Some(Drag::QuickSelect { subtract: mods.alt });
            }
            if let Some(Drag::QuickSelect { subtract }) = app.cv.drag {
                if resp.dragged() || primary_pressed {
                    let tab = &mut app.docs[active];
                    let src = tab.composite.clone();
                    let m = select::quick_select(&src, p.0, p.1, app.opts.tip.size);
                    let op = if subtract { SelectOp::Subtract } else { SelectOp::Add };
                    let new = select::combine(tab.doc.selection.as_ref(), m, op);
                    tab.doc.set_selection(new);
                }
                if resp.drag_stopped() || resp.clicked() {
                    app.cv.drag = None;
                    app.docs[active].commit("Quick Selection");
                }
            }
        }
        Tool::RectMarquee | Tool::EllipseMarquee => {
            if primary_pressed && resp.drag_started() {
                let op = select_op(mods, app.opts.select_op);
                app.cv.drag = Some(Drag::Marquee {
                    start: origin,
                    cur: p,
                    op,
                    ellipse: tool == Tool::EllipseMarquee,
                    constrain: false,
                });
            } else if resp.clicked() && !mods.shift && !mods.alt {
                // A click without dragging deselects.
                if app.docs[active].doc.selection.is_some() {
                    app.last_selection = app.docs[active].doc.selection.clone();
                    app.edit("Deselect", |d| d.deselect());
                }
            }
            if let Some(Drag::Marquee { start, cur, op, constrain, .. }) = &mut app.cv.drag {
                *cur = p;
                *constrain = mods.shift && *op == SelectOp::Replace;
                if *constrain {
                    let s = (cur.0 - start.0).abs().max((cur.1 - start.1).abs());
                    *cur = (start.0 + s * (cur.0 - start.0).signum(), start.1 + s * (cur.1 - start.1).signum());
                }
                if resp.drag_stopped() {
                    let Some(Drag::Marquee { start, cur, op, ellipse, .. }) = app.cv.drag.take() else {
                        unreachable!()
                    };
                    let feather = app.opts.feather;
                    let tab = &mut app.docs[active];
                    let (w, h) = (tab.doc.width, tab.doc.height);
                    let mut m = if ellipse {
                        select::ellipse_mask(w, h, start, cur)
                    } else {
                        select::rect_mask(w, h, Rect::from_points(start, cur))
                    };
                    if feather > 0.0 {
                        m = select::feather(&m, feather);
                    }
                    let new = select::combine(tab.doc.selection.as_ref(), m, op);
                    tab.doc.set_selection(new);
                    tab.commit(if ellipse { "Elliptical Marquee" } else { "Rectangular Marquee" });
                }
            }
        }
        Tool::Lasso => {
            if primary_pressed && resp.drag_started() {
                app.cv.drag = Some(Drag::Lasso { points: vec![origin], op: select_op(mods, app.opts.select_op) });
            }
            if let Some(Drag::Lasso { points, .. }) = &mut app.cv.drag {
                if points.last().is_none_or(|l| (l.0 - p.0).hypot(l.1 - p.1) * view.zoom > 2.0) {
                    points.push(p);
                }
                if resp.drag_stopped() {
                    let Some(Drag::Lasso { points, op }) = app.cv.drag.take() else { unreachable!() };
                    finish_polygon(app, &points, op, "Lasso");
                }
            }
        }
        Tool::PolyLasso => {
            if resp.clicked() {
                let close = app.cv.poly.len() >= 3
                    && (view.to_screen(app.cv.poly[0]).distance(pos) < 8.0 || resp.double_clicked());
                if close {
                    let pts = std::mem::take(&mut app.cv.poly);
                    finish_polygon(app, &pts, select_op(mods, app.opts.select_op), "Polygonal Lasso");
                } else {
                    app.cv.poly.push(p);
                }
            }
        }
        Tool::MagicWand => {
            if resp.clicked() {
                let Some(src) = sample_source(app) else {
                    return;
                };
                let (x, y) = (p.0.floor(), p.1.floor());
                if x < 0.0 || y < 0.0 || x >= src.width() as f32 || y >= src.height() as f32 {
                    return;
                }
                let o = &app.opts;
                let mut m = select::flood_mask(&src, x as u32, y as u32, o.tolerance, o.contiguous, o.anti_alias);
                if o.feather > 0.0 {
                    m = select::feather(&m, o.feather);
                }
                let op = select_op(mods, o.select_op);
                let tab = &mut app.docs[active];
                let new = select::combine(tab.doc.selection.as_ref(), m, op);
                tab.doc.set_selection(new);
                tab.commit("Magic Wand");
            }
        }
        Tool::Eyedropper => {
            if primary_pressed {
                app.cv.drag = Some(Drag::Eyedrop);
            }
            if matches!(app.cv.drag, Some(Drag::Eyedrop)) {
                let tab = &app.docs[active];
                let src = if app.opts.sample_merged {
                    &tab.composite
                } else {
                    tab.doc.active_layer().map(|l| &l.pixels).filter(|p| p.width() > 0).unwrap_or(&tab.composite)
                };
                if p.0 >= 0.0 && p.1 >= 0.0 && p.0 < src.width() as f32 && p.1 < src.height() as f32 {
                    let mut c = src.get(p.0 as u32, p.1 as u32);
                    c.0[3] = 255;
                    if mods.alt { app.bg = c } else { app.fg = c }
                }
                if resp.drag_stopped() || resp.clicked() {
                    app.cv.drag = None;
                    let c = if mods.alt { app.bg } else { app.fg };
                    app.push_swatch(c);
                }
            }
        }
        Tool::Bucket => {
            if resp.clicked() {
                let Some(src) = sample_source(app) else {
                    return;
                };
                if p.0 < 0.0 || p.1 < 0.0 || p.0 >= src.width() as f32 || p.1 >= src.height() as f32 {
                    return;
                }
                let o = app.opts.clone();
                let mut region =
                    select::flood_mask(&src, p.0 as u32, p.1 as u32, o.tolerance, o.contiguous, o.anti_alias);
                let fg = app.fg;
                app.edit_pixels(ctx, "Paint Bucket", |img, sel| {
                    if let Some(s) = sel {
                        region = select::combine(Some(s), region.clone(), SelectOp::Intersect);
                    }
                    paint::fill_region(img, &region, fg, o.brush_blend, o.opacity);
                });
            }
        }
        Tool::Gradient => {
            if primary_pressed && resp.drag_started() {
                app.cv.drag = Some(Drag::Gradient { start: origin, cur: p });
            }
            if let Some(Drag::Gradient { start, cur }) = &mut app.cv.drag {
                *cur = p;
                if mods.shift {
                    // Snap to 45°.
                    let (dx, dy) = (cur.0 - start.0, cur.1 - start.1);
                    let a = (dy.atan2(dx) / (std::f32::consts::PI / 4.0)).round() * std::f32::consts::PI / 4.0;
                    let l = dx.hypot(dy);
                    *cur = (start.0 + l * a.cos(), start.1 + l * a.sin());
                }
                if resp.drag_stopped() {
                    let Some(Drag::Gradient { start, cur }) = app.cv.drag.take() else { unreachable!() };
                    let o = app.opts.clone();
                    let presets = Gradient::presets(app.fg, app.bg);
                    let mut g =
                        presets.get(o.gradient_preset).map(|p| p.1.clone()).unwrap_or_else(|| presets[0].1.clone());
                    if o.gradient_reverse {
                        g = g.reversed();
                    }
                    let edit_mask = app.edit_mask;
                    app.edit_pixels(ctx, "Gradient", |img, sel| {
                        let mut gi =
                            paint::render_gradient(img.width(), img.height(), &g, o.gradient_shape, start, cur);
                        if edit_mask {
                            // Masks are greyscale.
                            for px in gi.pixels_mut().chunks_exact_mut(4) {
                                let l =
                                    studio_core::color::luma(px[0] as f32, px[1] as f32, px[2] as f32).round() as u8;
                                px[0] = l;
                                px[1] = l;
                                px[2] = l;
                            }
                        }
                        paint::composite_onto(img, &gi, o.gradient_blend, o.gradient_opacity, sel);
                    });
                }
            }
        }
        Tool::Crop => {
            let handle_at = |cb: &CropBox, pos: Pos2| -> Option<usize> {
                let (x0, y0, x1, y1) = cb.rect();
                let pts = [
                    (x0, y0),
                    (x1, y0),
                    (x1, y1),
                    (x0, y1),
                    ((x0 + x1) / 2.0, y0),
                    (x1, (y0 + y1) / 2.0),
                    ((x0 + x1) / 2.0, y1),
                    (x0, (y0 + y1) / 2.0),
                ];
                if let Some(i) = pts.iter().position(|&q| view.to_screen(q).distance(pos) < 10.0) {
                    return Some(i);
                }
                let r = ERect::from_two_pos(view.to_screen((x0, y0)), view.to_screen((x1, y1)));
                r.contains(pos).then_some(8)
            };
            if primary_pressed && resp.drag_started() {
                let opos = view.to_screen(origin);
                match app.cv.crop.as_ref().and_then(|cb| handle_at(cb, opos)) {
                    Some(h) => {
                        let cb = app.cv.crop.as_mut().unwrap();
                        cb.drag = Some((h, origin, cb.a, cb.b));
                    }
                    None => {
                        app.cv.crop = Some(CropBox { a: origin, b: origin, drag: Some((2, origin, origin, origin)) })
                    }
                }
            }
            if let Some(cb) = &mut app.cv.crop {
                if let Some((h, start, a0, b0)) = cb.drag {
                    let (dx, dy) = (p.0 - start.0, p.1 - start.1);
                    let (mut x0, mut y0, mut x1, mut y1) =
                        (a0.0.min(b0.0), a0.1.min(b0.1), a0.0.max(b0.0), a0.1.max(b0.1));
                    if a0 == b0 {
                        (x1, y1) = (p.0, p.1);
                    } else {
                        match h {
                            0 => (x0, y0) = (x0 + dx, y0 + dy),
                            1 => (x1, y0) = (x1 + dx, y0 + dy),
                            2 => (x1, y1) = (x1 + dx, y1 + dy),
                            3 => (x0, y1) = (x0 + dx, y1 + dy),
                            4 => y0 += dy,
                            5 => x1 += dx,
                            6 => y1 += dy,
                            7 => x0 += dx,
                            _ => {
                                x0 += dx;
                                x1 += dx;
                                y0 += dy;
                                y1 += dy;
                            }
                        }
                    }
                    cb.a = (x0.round(), y0.round());
                    cb.b = (x1.round(), y1.round());
                    if resp.drag_stopped() {
                        cb.drag = None;
                    }
                }
                if resp.double_clicked() {
                    commit_crop(app);
                }
            }
            if let Some(cb) = &app.cv.crop {
                if resp.hovered() {
                    if let Some(h) = handle_at(cb, pos) {
                        ctx.set_cursor_icon(match h {
                            0 | 2 => CursorIcon::ResizeNwSe,
                            1 | 3 => CursorIcon::ResizeNeSw,
                            4 | 6 => CursorIcon::ResizeVertical,
                            5 | 7 => CursorIcon::ResizeHorizontal,
                            _ => CursorIcon::Move,
                        });
                    }
                }
            }
        }
        Tool::Move => {
            if primary_pressed && resp.drag_started() {
                let tab = &app.docs[active];
                if tab.doc.active_layer().is_some_and(|l| !l.locked && !l.is_adjustment()) {
                    app.cv.drag = Some(Drag::Move { start: origin, base: tab.doc.state(), applied: (0, 0) });
                }
            }
            if let Some(Drag::Move { start, base, applied }) = &mut app.cv.drag {
                let (dx, dy) = ((p.0 - start.0).round() as i32, (p.1 - start.1).round() as i32);
                let (dx, dy) = if mods.shift { if dx.abs() > dy.abs() { (dx, 0) } else { (0, dy) } } else { (dx, dy) };
                if (dx, dy) != *applied {
                    let tab = &mut app.docs[active];
                    tab.doc.restore(base);
                    tab.doc.nudge_active(dx, dy);
                    tab.invalidate_all();
                    *applied = (dx, dy);
                }
                if resp.drag_stopped() {
                    let moved = *applied != (0, 0);
                    app.cv.drag = None;
                    if moved {
                        app.docs[active].commit("Move");
                    }
                }
            }
        }
        Tool::Text => {
            if resp.clicked() {
                if let Some(te) = app.cv.text_edit.take() {
                    app.end_text_edit(te);
                    return;
                }
                let tab = &mut app.docs[active];
                let hit = tab.doc.layers.iter().enumerate().rev().find_map(|(i, l)| match &l.kind {
                    LayerKind::Text(t)
                        if l.visible && studio_core::text::bounds(t).inflate(4).contains(p.0 as i32, p.1 as i32) =>
                    {
                        Some(i)
                    }
                    _ => None,
                });
                match hit {
                    Some(i) => {
                        tab.doc.active = i;
                        app.cv.text_edit = Some(TextEdit { layer_id: tab.doc.layers[i].id, is_new: false });
                    }
                    None => {
                        let o = &app.opts;
                        let mut tl = TextLayer::new("", p.0, p.1 - o.font_size * 0.8, o.font_size, app.fg);
                        tl.face = o.face;
                        tl.align = o.align;
                        tab.doc.add_layer_of_kind("Text", LayerKind::Text(tl));
                        tab.doc.layers[tab.doc.active].mask = None;
                        let id = tab.doc.layers[tab.doc.active].id;
                        tab.invalidate_all();
                        app.cv.text_edit = Some(TextEdit { layer_id: id, is_new: true });
                    }
                }
                app.edit_mask = false;
            }
        }
        Tool::Rectangle | Tool::RoundedRect | Tool::Ellipse | Tool::Polygon | Tool::Star | Tool::Line => {
            if primary_pressed && resp.drag_started() {
                let o = &app.opts;
                let kind = tool.shape_kind(o).unwrap();
                let shape = ShapeLayer {
                    kind,
                    from: origin,
                    to: p,
                    fill: o.fill_enabled.then_some(app.fg),
                    stroke: (o.stroke_enabled || tool == Tool::Line).then_some(if tool == Tool::Line {
                        app.fg
                    } else {
                        app.bg
                    }),
                    stroke_width: o.stroke_width,
                    dashed: o.dashed,
                };
                let tab = &mut app.docs[active];
                tab.doc.add_layer_of_kind(kind.label(), LayerKind::Shape(shape));
                tab.doc.layers[tab.doc.active].mask = None;
                let id = tab.doc.layers[tab.doc.active].id;
                app.cv.drag = Some(Drag::Shape { start: origin, layer_id: id });
                app.edit_mask = false;
            }
            if let Some(Drag::Shape { start, layer_id }) = &app.cv.drag {
                let (start, id) = (*start, *layer_id);
                let mut to = p;
                if mods.shift {
                    if tool == Tool::Line {
                        let (dx, dy) = (p.0 - start.0, p.1 - start.1);
                        let a = (dy.atan2(dx) / (std::f32::consts::PI / 4.0)).round() * std::f32::consts::PI / 4.0;
                        let l = dx.hypot(dy);
                        to = (start.0 + l * a.cos(), start.1 + l * a.sin());
                    } else {
                        let s = (p.0 - start.0).abs().max((p.1 - start.1).abs());
                        to = (start.0 + s * (p.0 - start.0).signum(), start.1 + s * (p.1 - start.1).signum());
                    }
                }
                let from = if mods.alt { (start.0 - (to.0 - start.0), start.1 - (to.1 - start.1)) } else { start };
                let tab = &mut app.docs[active];
                let (w, h) = (tab.doc.width, tab.doc.height);
                if let Some(i) = tab.doc.index_of(id) {
                    if let LayerKind::Shape(s) = &mut tab.doc.layers[i].kind {
                        s.from = from;
                        s.to = to;
                    }
                    tab.doc.layers[i].render(w, h);
                    tab.invalidate_all();
                }
                if resp.drag_stopped() {
                    app.cv.drag = None;
                    let tiny = (to.0 - start.0).abs() < 2.0 && (to.1 - start.1).abs() < 2.0;
                    let tab = &mut app.docs[active];
                    if tiny {
                        if let Some(i) = tab.doc.index_of(id) {
                            tab.doc.delete_layer(i);
                        }
                        tab.invalidate_all();
                    } else {
                        tab.commit(tool.label());
                    }
                }
            }
        }
        Tool::Zoom => {
            if resp.clicked() {
                let center = app.cv.view_rect.center();
                let tab = &mut app.docs[active];
                let z = crate::doc::zoom_step(tab.zoom, !mods.alt);
                let f = z / tab.zoom;
                tab.zoom_at(f, pos, center);
            }
        }
        _ => {}
    }
    let _ = FilterCtx::default();
}

fn finish_polygon(app: &mut App, points: &[(f32, f32)], op: SelectOp, label: &str) {
    if points.len() < 3 {
        return;
    }
    let feather = app.opts.feather;
    let Some(tab) = app.tab_mut() else { return };
    let mut m = select::polygon_mask(tab.doc.width, tab.doc.height, points);
    if feather > 0.0 {
        m = select::feather(&m, feather);
    }
    let new = select::combine(tab.doc.selection.as_ref(), m, op);
    tab.doc.set_selection(new);
    tab.commit(label);
}

pub fn commit_crop(app: &mut App) {
    let Some(cb) = app.cv.crop.take() else { return };
    let Some(tab) = app.tab_mut() else { return };
    let r = cb.canvas_rect(tab.doc.width, tab.doc.height);
    if r.w >= 1 && r.h >= 1 {
        tab.doc.crop(r);
        tab.doc.selection = None;
        tab.commit("Crop");
        tab.fit_pending = true;
    }
}

pub fn commit_transform(app: &mut App) {
    if let Some(ts) = app.cv.transform.take() {
        if let Some(tab) = app.tab_mut() {
            if ts.applied.is_some() {
                tab.commit("Free Transform");
            } else {
                tab.doc.restore(&ts.base);
                tab.invalidate_all();
            }
        }
    }
}

pub fn cancel_transform(app: &mut App) {
    if let Some(ts) = app.cv.transform.take() {
        if let Some(tab) = app.tab_mut() {
            tab.doc.restore(&ts.base);
            tab.invalidate_all();
        }
    }
}

fn handle_transform(app: &mut App, resp: &egui::Response, view: &View, mods: egui::Modifiers) {
    let Some(pos) = resp.interact_pointer_pos().or(resp.hover_pos()) else {
        return;
    };
    let p = view.to_canvas(pos);
    let ctx = resp.ctx.clone();
    let active = app.active;
    let Some(ts) = app.cv.transform.as_mut() else {
        return;
    };

    let handles = ts.handles();
    let near = handles.iter().position(|&h| view.to_screen(h).distance(pos) < 9.0);
    let corners = ts.corners();
    let inside = point_in_quad(p, &corners);
    if ts.drag.is_none() && resp.hovered() {
        ctx.set_cursor_icon(match near {
            Some(0 | 2) => CursorIcon::ResizeNwSe,
            Some(1 | 3) => CursorIcon::ResizeNeSw,
            Some(4 | 6) => CursorIcon::ResizeVertical,
            Some(_) => CursorIcon::ResizeHorizontal,
            None if inside => CursorIcon::Move,
            None => CursorIcon::Alias,
        });
    }
    if resp.drag_started() {
        let p = ctx.input(|i| i.pointer.press_origin()).map(|o| view.to_canvas(o)).unwrap_or(p);
        let near = ts.handles().iter().position(|&h| view.to_screen(h).distance(view.to_screen(p)) < 9.0);
        let inside = point_in_quad(p, &ts.corners());
        let (cx, cy) = ts.center();
        let c = (cx + ts.tx, cy + ts.ty);
        ts.drag = Some(match near {
            Some(h) => TDrag::Scale { handle: h, start: (ts.sx, ts.sy, ts.tx, ts.ty) },
            None if inside => TDrag::Move { start: p, t0: (ts.tx, ts.ty) },
            None => TDrag::Rotate { a0: (p.1 - c.1).atan2(p.0 - c.0), r0: ts.rot },
        });
    }
    if let Some(d) = ts.drag {
        match d {
            TDrag::Move { start, t0 } => {
                ts.tx = t0.0 + p.0 - start.0;
                ts.ty = t0.1 + p.1 - start.1;
            }
            TDrag::Rotate { a0, r0 } => {
                let (cx, cy) = ts.center();
                let c = (cx + ts.tx, cy + ts.ty);
                let mut r = r0 + (p.1 - c.1).atan2(p.0 - c.0) - a0;
                if mods.shift {
                    let step = 15f32.to_radians();
                    r = (r / step).round() * step;
                }
                ts.rot = r;
            }
            TDrag::Scale { handle, start } => {
                let (hw, hh) = ((ts.bounds.2 - ts.bounds.0) / 2.0, (ts.bounds.3 - ts.bounds.1) / 2.0);
                let signs: [(f32, f32); 8] = [
                    (-1.0, -1.0),
                    (1.0, -1.0),
                    (1.0, 1.0),
                    (-1.0, 1.0),
                    (0.0, -1.0),
                    (1.0, 0.0),
                    (0.0, 1.0),
                    (-1.0, 0.0),
                ];
                let (sgx, sgy) = signs[handle];
                let l = ts.local(p, start);
                // Opposite side stays put (Alt scales about the centre).
                let anchor = if mods.alt { (0.0, 0.0) } else { (-sgx * hw * start.0, -sgy * hh * start.1) };
                let mut sx = if sgx != 0.0 {
                    ((l.0 - anchor.0) * sgx / (if mods.alt { hw } else { 2.0 * hw })).max(0.01)
                } else {
                    start.0
                };
                let mut sy = if sgy != 0.0 {
                    ((l.1 - anchor.1) * sgy / (if mods.alt { hh } else { 2.0 * hh })).max(0.01)
                } else {
                    start.1
                };
                // Corners keep proportions unless Shift is held.
                if sgx != 0.0 && sgy != 0.0 && !mods.shift {
                    let k = (sx / start.0).max(sy / start.1);
                    sx = start.0 * k;
                    sy = start.1 * k;
                }
                let new_center_local = if mods.alt {
                    (0.0, 0.0)
                } else {
                    (
                        if sgx != 0.0 { anchor.0 + sgx * hw * sx } else { 0.0 },
                        if sgy != 0.0 { anchor.1 + sgy * hh * sy } else { 0.0 },
                    )
                };
                let (s, c) = ts.rot.sin_cos();
                ts.sx = sx;
                ts.sy = sy;
                ts.tx = start.2 + new_center_local.0 * c - new_center_local.1 * s;
                ts.ty = start.3 + new_center_local.0 * s + new_center_local.1 * c;
            }
        }
        if resp.drag_stopped() {
            ts.drag = None;
        }
    }
    if resp.double_clicked() && inside {
        commit_transform(app);
        return;
    }
    let key = ts.key();
    if ts.applied != Some(key) && (ts.drag.is_some() || ts.applied.is_some()) {
        ts.applied = Some(key);
        let aff = ts.affine();
        let base = ts.base.clone();
        let tab = &mut app.docs[active];
        tab.doc.restore(&base);
        tab.doc.transform_active(&aff);
        tab.invalidate_all();
    }
}

fn point_in_quad(p: (f32, f32), q: &[(f32, f32); 4]) -> bool {
    let mut sign = 0.0f32;
    for i in 0..4 {
        let a = q[i];
        let b = q[(i + 1) % 4];
        let cross = (b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0);
        if cross.abs() < 1e-6 {
            continue;
        }
        if sign == 0.0 {
            sign = cross.signum();
        } else if cross.signum() != sign {
            return false;
        }
    }
    true
}

fn draw_ants(app: &mut App, painter: &egui::Painter, view: &View, time: f64) {
    let Some(tab) = app.docs.get_mut(app.active) else {
        return;
    };
    let Some(sel) = &tab.doc.selection else {
        return;
    };
    let stamp = sel.generation();
    if tab.ants.as_ref().is_none_or(|(g, _)| *g != stamp) {
        tab.ants = Some((stamp, select::outline_segments(sel)));
    }
    let segs = &tab.ants.as_ref().unwrap().1;
    let offset = (time * 16.0) as f32 % 8.0;
    let clip = painter.clip_rect();
    let black = Stroke::new(1.0, Color32::from_black_alpha(220));
    let white = Stroke::new(1.0, Color32::WHITE);
    let dashed = segs.len() < 8000;
    for &((x0, y0), (x1, y1)) in segs {
        let a = view.to_screen((x0 as f32, y0 as f32));
        let b = view.to_screen((x1 as f32, y1 as f32));
        if !clip.intersects(ERect::from_two_pos(a, b).expand(1.0)) {
            continue;
        }
        painter.line_segment([a, b], black);
        if dashed {
            painter.extend(Shape::dashed_line_with_offset(&[a, b], white, &[4.0], &[4.0], offset));
        }
    }
}

fn draw_overlays(app: &App, painter: &egui::Painter, view: &View, pointer: Option<Pos2>, time: f64) {
    let t = Tokens::get(painter.ctx());
    let dash = |pts: &[Pos2], closed: bool| {
        let mut v = pts.to_vec();
        if closed && !pts.is_empty() {
            v.push(pts[0]);
        }
        painter.add(Shape::line(v.clone(), Stroke::new(1.0, Color32::from_black_alpha(200))));
        painter.extend(Shape::dashed_line_with_offset(
            &v,
            Stroke::new(1.0, Color32::WHITE),
            &[4.0],
            &[4.0],
            (time * 16.0) as f32 % 8.0,
        ));
    };
    match &app.cv.drag {
        Some(Drag::Marquee { start, cur, ellipse, .. }) => {
            let r = ERect::from_two_pos(view.to_screen(*start), view.to_screen(*cur));
            if *ellipse {
                let pts: Vec<Pos2> = (0..64)
                    .map(|i| {
                        let a = i as f32 / 64.0 * std::f32::consts::TAU;
                        r.center() + Vec2::new(a.cos() * r.width() / 2.0, a.sin() * r.height() / 2.0)
                    })
                    .collect();
                dash(&pts, true);
            } else {
                dash(&[r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom()], true);
            }
            let (w, h) = ((cur.0 - start.0).abs().round(), (cur.1 - start.1).abs().round());
            painter.text(
                r.right_bottom() + Vec2::new(8.0, 8.0),
                egui::Align2::LEFT_TOP,
                format!("{w:.0} × {h:.0}"),
                egui::FontId::proportional(11.0),
                t.text,
            );
        }
        Some(Drag::Lasso { points, .. }) => {
            let pts: Vec<Pos2> = points.iter().map(|p| view.to_screen(*p)).collect();
            dash(&pts, false);
        }
        Some(Drag::Gradient { start, cur }) => {
            let (a, b) = (view.to_screen(*start), view.to_screen(*cur));
            painter.line_segment([a, b], Stroke::new(3.0, Color32::from_black_alpha(140)));
            painter.line_segment([a, b], Stroke::new(1.5, Color32::WHITE));
            painter.circle(a, 4.0, crate::widgets::c32(app.fg), Stroke::new(1.5, Color32::WHITE));
            painter.circle(b, 4.0, crate::widgets::c32(app.bg), Stroke::new(1.5, Color32::WHITE));
        }
        _ => {}
    }
    if !app.cv.poly.is_empty() {
        let mut pts: Vec<Pos2> = app.cv.poly.iter().map(|p| view.to_screen(*p)).collect();
        if let Some(p) = pointer {
            pts.push(p);
        }
        dash(&pts, false);
        painter.circle_stroke(view.to_screen(app.cv.poly[0]), 5.0, Stroke::new(1.5, t.accent));
    }
    if let Some(cb) = &app.cv.crop {
        let (x0, y0, x1, y1) = cb.rect();
        let r = ERect::from_two_pos(view.to_screen((x0, y0)), view.to_screen((x1, y1)));
        let clip = painter.clip_rect();
        let shade = Color32::from_black_alpha(130);
        for s in [
            ERect::from_min_max(clip.min, egui::pos2(clip.max.x, r.min.y)),
            ERect::from_min_max(egui::pos2(clip.min.x, r.max.y), clip.max),
            ERect::from_min_max(egui::pos2(clip.min.x, r.min.y), egui::pos2(r.min.x, r.max.y)),
            ERect::from_min_max(egui::pos2(r.max.x, r.min.y), egui::pos2(clip.max.x, r.max.y)),
        ] {
            painter.rect_filled(s, 0.0, shade);
        }
        let thin = Stroke::new(1.0, Color32::from_white_alpha(110));
        for k in 1..3 {
            let f = k as f32 / 3.0;
            painter.vline(r.left() + r.width() * f, r.y_range(), thin);
            painter.hline(r.x_range(), r.top() + r.height() * f, thin);
        }
        painter.rect_stroke(r, 0.0, Stroke::new(1.5, Color32::WHITE), egui::StrokeKind::Middle);
        for q in [
            r.left_top(),
            r.right_top(),
            r.right_bottom(),
            r.left_bottom(),
            r.center_top(),
            r.right_center(),
            r.center_bottom(),
            r.left_center(),
        ] {
            painter.rect_filled(ERect::from_center_size(q, Vec2::splat(8.0)), 2.0, Color32::WHITE);
        }
        painter.text(
            r.center_bottom() + Vec2::new(0.0, 10.0),
            egui::Align2::CENTER_TOP,
            format!("{:.0} × {:.0}  ·  Enter to crop", (x1 - x0).abs(), (y1 - y0).abs()),
            egui::FontId::proportional(11.5),
            Color32::WHITE,
        );
    }
    if let Some(ts) = &app.cv.transform {
        let c: Vec<Pos2> = ts.corners().iter().map(|p| view.to_screen(*p)).collect();
        painter.add(Shape::closed_line(c.clone(), Stroke::new(1.5, t.accent)));
        for h in ts.handles() {
            let p = view.to_screen(h);
            painter.rect(
                ERect::from_center_size(p, Vec2::splat(9.0)),
                2.0,
                Color32::WHITE,
                Stroke::new(1.0, t.accent),
                egui::StrokeKind::Middle,
            );
        }
        let (cx, cy) = ts.center();
        painter.circle_stroke(view.to_screen((cx + ts.tx, cy + ts.ty)), 4.0, Stroke::new(1.5, t.accent));
    }
    if let Some(src) = app.cv.clone_source {
        if matches!(app.tool, Tool::CloneStamp | Tool::Heal) {
            let s = view.to_screen(src);
            let k = Stroke::new(1.5, Color32::WHITE);
            painter.line_segment([s - Vec2::new(7.0, 0.0), s + Vec2::new(7.0, 0.0)], k);
            painter.line_segment([s - Vec2::new(0.0, 7.0), s + Vec2::new(0.0, 7.0)], k);
        }
    }
    // Brush outline.
    if let Some(p) = pointer.filter(|p| painter.clip_rect().contains(*p)) {
        if app.tool.uses_brush() && app.tool != Tool::QuickSelect && app.cv.transform.is_none() {
            let size = if app.tool == Tool::Eraser { app.opts.eraser_tip.size } else { app.opts.tip.size };
            let r = (size * view.zoom / 2.0).max(1.5);
            painter.circle_stroke(p, r, Stroke::new(1.0, Color32::from_black_alpha(200)));
            painter.circle_stroke(p, r + 1.0, Stroke::new(1.0, Color32::from_white_alpha(200)));
            if r < 4.0 {
                painter
                    .line_segment([p - Vec2::new(5.0, 0.0), p + Vec2::new(5.0, 0.0)], Stroke::new(1.0, Color32::WHITE));
                painter
                    .line_segment([p - Vec2::new(0.0, 5.0), p + Vec2::new(0.0, 5.0)], Stroke::new(1.0, Color32::WHITE));
            }
        }
        if app.tool == Tool::QuickSelect {
            painter.circle_stroke(p, (app.opts.tip.size * view.zoom / 2.0).max(3.0), Stroke::new(1.0, Color32::WHITE));
        }
    }
}

/// Floating editor for the type layer being edited.
fn text_editor(app: &mut App, ui: &mut Ui, view: &View) {
    let Some(te) = &app.cv.text_edit else { return };
    let id = te.layer_id;
    let active = app.active;
    let Some(tab) = app.docs.get_mut(active) else {
        return;
    };
    let Some(idx) = tab.doc.index_of(id) else {
        app.cv.text_edit = None;
        return;
    };
    let (w, h) = (tab.doc.width, tab.doc.height);
    let LayerKind::Text(tl) = &tab.doc.layers[idx].kind else {
        app.cv.text_edit = None;
        return;
    };
    let b = studio_core::text::bounds(tl);
    let anchor = view.to_screen((b.x as f32, b.bottom() as f32)) + Vec2::new(0.0, 10.0);
    let mut text = tl.text.clone();
    let mut done = false;
    let mut cancel = false;
    let t = Tokens::get(ui.ctx());
    egui::Area::new(egui::Id::new("text-edit-area")).fixed_pos(anchor).order(egui::Order::Foreground).show(
        ui.ctx(),
        |ui| {
            egui::Frame::popup(ui.style()).corner_radius(10).show(ui, |ui| {
                ui.set_width(300.0);
                ui.label(egui::RichText::new("Type").font(crate::theme::medium(12.0)).color(t.text_dim));
                let r = ui.add(
                    egui::TextEdit::multiline(&mut text)
                        .desired_rows(3)
                        .desired_width(f32::INFINITY)
                        .hint_text("Type something…"),
                );
                if !r.has_focus() && !r.lost_focus() {
                    r.request_focus();
                }
                ui.horizontal(|ui| {
                    if crate::widgets::primary_button(ui, "Done").clicked() {
                        done = true;
                    }
                    if crate::widgets::secondary_button(ui, "Cancel").clicked() {
                        cancel = true;
                    }
                    crate::widgets::label_dim(ui, "⌘/Ctrl + Enter");
                });
                if ui.input(|i| i.key_pressed(egui::Key::Enter) && i.modifiers.command) {
                    done = true;
                }
                if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    done = true;
                }
            });
        },
    );
    if let LayerKind::Text(tl) = &mut tab.doc.layers[idx].kind {
        if tl.text != text {
            tl.text = text;
            tab.doc.layers[idx].render(w, h);
            tab.invalidate_all();
        }
    }
    if cancel {
        let te = app.cv.text_edit.take().unwrap();
        let tab = &mut app.docs[active];
        if te.is_new {
            if let Some(i) = tab.doc.index_of(te.layer_id) {
                tab.doc.delete_layer(i);
            }
        } else {
            let cur = tab.history.current();
            tab.goto_history(cur);
        }
        tab.invalidate_all();
    } else if done {
        let te = app.cv.text_edit.take().unwrap();
        app.end_text_edit(te);
    }
}
