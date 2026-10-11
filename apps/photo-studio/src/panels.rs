//! The right-hand dock: Navigator, Histogram, Color, Adjustments, Properties, History, Layers.

use egui::{Color32, CornerRadius, Pos2, Sense, Stroke, Ui, Vec2};
use egui_phosphor::regular as ph;
use studio_core::Rgba;
use studio_core::adjust::Adjustment;
use studio_core::blend::BlendMode;
use studio_core::color::{hsv_to_rgb, rgb_to_hsv};
use studio_core::gradient::{Gradient, GradientShape};
use studio_core::layer::{FillLayer, LayerKind};
use studio_core::shape::ShapeKind;
use studio_core::text::{Align, FontFace};

use crate::actions::Action;
use crate::app::App;
use crate::theme::Tokens;
use crate::widgets::{
    c32, card, icon_button, label_dim, rgba, segmented, slider_row, small_button, swatch, toggle_row,
};

pub fn dock(app: &mut App, ui: &mut Ui, actions: &mut Vec<Action>) {
    let t = Tokens::get(ui.ctx());
    egui::Panel::bottom("layers-panel")
        .resizable(true)
        .default_size(340.0)
        .min_size(180.0)
        .max_size((ui.available_height() * 0.7).max(200.0))
        .frame(egui::Frame::new().fill(t.panel).inner_margin(egui::Margin::symmetric(10, 8)))
        .show(ui, |ui| layers(app, ui, actions));
    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(t.panel).inner_margin(egui::Margin::symmetric(10, 10)))
        .show(ui, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                if !app.docs.is_empty() {
                    card(ui, "nav", "Navigator", true, |ui| navigator(app, ui));
                    card(ui, "hist", "Histogram", false, |ui| histogram(app, ui));
                }
                card(ui, "color", "Color", true, |ui| color_panel(app, ui));
                card(ui, "actions", "Actions", false, |ui| actions_panel(app, ui, actions));
                if !app.docs.is_empty() {
                    card(ui, "adj", "Adjustments", true, |ui| adjustments(ui, actions));
                    properties(app, ui);
                    card(ui, "paths", "Paths", false, |ui| paths_panel(app, ui, actions));
                    card(ui, "channels", "Channels", false, |ui| channels_panel(app, ui, actions));
                    card(ui, "history", "History", false, |ui| history(app, ui));
                }
            });
        });
}

fn navigator(app: &mut App, ui: &mut Ui) {
    let view = app.cv.view_rect;
    let ctx = ui.ctx().clone();
    let Some(tab) = app.tab_mut() else { return };
    let Some(tex) = tab.nav_texture(&ctx) else {
        return;
    };
    let (w, h) = (tab.doc.width as f32, tab.doc.height as f32);
    let avail = ui.available_width();
    let s = (avail / w).min(150.0 / h);
    let size = Vec2::new(w * s, h * s);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(avail, size.y), Sense::click_and_drag());
    let img_rect = egui::Rect::from_center_size(rect.center(), size);
    let p = ui.painter_at(rect);
    crate::widgets::checker(&p, img_rect, 5.0);
    p.image(tex, img_rect, egui::Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
    // Visible area.
    let origin = view.center() + tab.pan - Vec2::new(w, h) * tab.zoom / 2.0;
    let vis_min = (view.min - origin) / tab.zoom;
    let vis_max = (view.max - origin) / tab.zoom;
    let vr = egui::Rect::from_min_max(img_rect.min + vis_min * s, img_rect.min + vis_max * s).intersect(img_rect);
    p.rect_stroke(vr, 2.0, Stroke::new(1.5, Color32::from_rgb(255, 69, 58)), egui::StrokeKind::Middle);
    if let Some(pos) = resp.interact_pointer_pos().filter(|_| resp.dragged() || resp.clicked()) {
        let c = (pos - img_rect.min) / s;
        tab.pan = Vec2::new(w / 2.0 - c.x, h / 2.0 - c.y) * tab.zoom;
    }
    ui.horizontal(|ui| {
        let mut pct = tab.zoom * 100.0;
        ui.label(egui::RichText::new(ph::MAGNIFYING_GLASS_MINUS).color(Tokens::get(ui.ctx()).text_dim));
        ui.spacing_mut().slider_width = ui.available_width() - 90.0;
        if ui.add(egui::Slider::new(&mut pct, 1.0..=3200.0).logarithmic(true).show_value(false)).changed() {
            let f = pct / 100.0 / tab.zoom;
            tab.zoom_at(f, view.center(), view.center());
        }
        ui.label(egui::RichText::new(ph::MAGNIFYING_GLASS_PLUS).color(Tokens::get(ui.ctx()).text_dim));
        label_dim(ui, format!("{:.0}%", tab.zoom * 100.0));
    });
}

fn histogram(app: &mut App, ui: &mut Ui) {
    let Some(tab) = app.tab_mut() else { return };
    let h = tab.histogram().clone();
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(w, 90.0), Sense::hover());
    let p = ui.painter_at(rect);
    let dark = ui.visuals().dark_mode;
    p.rect_filled(rect, CornerRadius::same(6), if dark { Color32::from_gray(26) } else { Color32::from_gray(248) });
    for (c, col) in [
        (0, Color32::from_rgba_unmultiplied(255, 69, 58, 90)),
        (1, Color32::from_rgba_unmultiplied(48, 209, 88, 90)),
        (2, Color32::from_rgba_unmultiplied(10, 132, 255, 90)),
    ] {
        crate::params::draw_histogram(&p, rect.shrink(2.0), &h, c, col);
    }
    egui::Grid::new("hist-stats").num_columns(4).spacing([10.0, 2.0]).show(ui, |ui| {
        label_dim(ui, "Mean");
        ui.label(format!("{:.1}", h.mean(3)));
        label_dim(ui, "Median");
        ui.label(format!("{}", h.median(3)));
        ui.end_row();
        label_dim(ui, "Std Dev");
        ui.label(format!("{:.1}", h.std_dev(3)));
        label_dim(ui, "Pixels");
        ui.label(format!("{}", h.count()));
        ui.end_row();
    });
}

/// Saturation/value square plus hue bar, editing `c` in place.
pub fn color_picker(ui: &mut Ui, id: &str, c: &mut Rgba) -> bool {
    let store = ui.make_persistent_id(id);
    let rgb = [c.r() as f32 / 255.0, c.g() as f32 / 255.0, c.b() as f32 / 255.0];
    let (mut h, mut s, mut v) = rgb_to_hsv(rgb[0], rgb[1], rgb[2]);
    // Keep hue stable for greys, where RGB alone forgets it.
    let remembered: Option<f32> = ui.data(|d| d.get_temp(store));
    if s < 1e-3 || v < 1e-3 {
        if let Some(rh) = remembered {
            h = rh;
        }
    }
    let mut changed = false;
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 130.0), Sense::click_and_drag());
    let p = ui.painter_at(rect);
    let (hr, hg, hb) = hsv_to_rgb(h, 1.0, 1.0);
    let hue_col = Color32::from_rgb((hr * 255.0) as u8, (hg * 255.0) as u8, (hb * 255.0) as u8);
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(rect.left_top(), Color32::WHITE);
    mesh.colored_vertex(rect.right_top(), hue_col);
    mesh.colored_vertex(rect.right_bottom(), Color32::BLACK);
    mesh.colored_vertex(rect.left_bottom(), Color32::BLACK);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    p.add(egui::Shape::mesh(mesh));
    if let Some(pos) = resp.interact_pointer_pos().filter(|_| resp.dragged() || resp.clicked()) {
        s = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
        v = 1.0 - ((pos.y - rect.top()) / rect.height()).clamp(0.0, 1.0);
        changed = true;
    }
    let knob = Pos2::new(rect.left() + s * rect.width(), rect.top() + (1.0 - v) * rect.height());
    p.circle_stroke(knob, 6.0, Stroke::new(2.0, Color32::WHITE));
    p.circle_stroke(knob, 7.5, Stroke::new(1.0, Color32::from_black_alpha(120)));

    ui.add_space(4.0);
    let (bar, bresp) = ui.allocate_exact_size(Vec2::new(w, 12.0), Sense::click_and_drag());
    let mut mesh = egui::Mesh::default();
    let n = 36;
    for i in 0..n {
        let (a, b) = (i as f32 / n as f32, (i + 1) as f32 / n as f32);
        let col = |t: f32| {
            let (r, g, b) = hsv_to_rgb(t * 360.0, 1.0, 1.0);
            Color32::from_rgb((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
        };
        let idx = mesh.vertices.len() as u32;
        mesh.colored_vertex(Pos2::new(bar.left() + bar.width() * a, bar.top()), col(a));
        mesh.colored_vertex(Pos2::new(bar.left() + bar.width() * b, bar.top()), col(b));
        mesh.colored_vertex(Pos2::new(bar.left() + bar.width() * b, bar.bottom()), col(b));
        mesh.colored_vertex(Pos2::new(bar.left() + bar.width() * a, bar.bottom()), col(a));
        mesh.add_triangle(idx, idx + 1, idx + 2);
        mesh.add_triangle(idx, idx + 2, idx + 3);
    }
    ui.painter().add(egui::Shape::mesh(mesh));
    if let Some(pos) = bresp.interact_pointer_pos().filter(|_| bresp.dragged() || bresp.clicked()) {
        h = ((pos.x - bar.left()) / bar.width()).clamp(0.0, 0.9999) * 360.0;
        changed = true;
    }
    let hx = bar.left() + h / 360.0 * bar.width();
    ui.painter().rect_stroke(
        egui::Rect::from_center_size(Pos2::new(hx, bar.center().y), Vec2::new(6.0, 16.0)),
        3.0,
        Stroke::new(2.0, Color32::WHITE),
        egui::StrokeKind::Middle,
    );

    if changed {
        ui.data_mut(|d| d.insert_temp(store, h));
        let (r, g, b) = hsv_to_rgb(h, s, v);
        *c = Rgba::rgba((r * 255.0).round() as u8, (g * 255.0).round() as u8, (b * 255.0).round() as u8, c.a());
    }
    ui.horizontal(|ui| {
        let mut hex = c.hex();
        ui.label("#");
        if ui.add(egui::TextEdit::singleline(&mut hex).desired_width(64.0).font(egui::TextStyle::Monospace)).changed() {
            if let Some(n) = Rgba::from_hex(&hex) {
                *c = n;
                changed = true;
            }
        }
        for (i, name) in ["R", "G", "B"].iter().enumerate() {
            let mut v = c.0[i];
            label_dim(ui, *name);
            if ui.add(egui::DragValue::new(&mut v).range(0..=255).speed(1.0)).changed() {
                c.0[i] = v;
                changed = true;
            }
        }
    });
    changed
}

const PALETTE: [[u8; 3]; 20] = [
    [0, 0, 0],
    [72, 72, 74],
    [142, 142, 147],
    [199, 199, 204],
    [255, 255, 255],
    [255, 59, 48],
    [255, 149, 0],
    [255, 204, 0],
    [52, 199, 89],
    [0, 199, 190],
    [48, 176, 199],
    [0, 122, 255],
    [88, 86, 214],
    [175, 82, 222],
    [255, 45, 85],
    [162, 132, 94],
    [120, 60, 30],
    [30, 60, 120],
    [20, 90, 50],
    [250, 220, 200],
];

fn color_panel(app: &mut App, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    ui.horizontal(|ui| {
        // Foreground/background chips.
        let (rect, _) = ui.allocate_exact_size(Vec2::new(52.0, 44.0), Sense::hover());
        let bg_r = egui::Rect::from_min_size(rect.min + Vec2::new(18.0, 14.0), Vec2::splat(28.0));
        let fg_r = egui::Rect::from_min_size(rect.min, Vec2::splat(28.0));
        let p = ui.painter();
        p.rect(bg_r, 6.0, c32(app.bg), Stroke::new(1.5, t.border), egui::StrokeKind::Inside);
        p.rect(
            fg_r,
            6.0,
            c32(app.fg),
            Stroke::new(1.5, if app.prefs.theme == crate::theme::ThemeMode::Light { t.border } else { t.text_dim }),
            egui::StrokeKind::Inside,
        );
        if ui.interact(bg_r, ui.id().with("bgchip"), Sense::click()).clicked() {
            std::mem::swap(&mut app.fg, &mut app.bg);
        }
        ui.vertical(|ui| {
            if ui
                .small_button(format!("{} {}", ph::ARROWS_LEFT_RIGHT, crate::i18n::tr("Swap")))
                .on_hover_text(crate::i18n::tr("Swap colors (X)"))
                .clicked()
            {
                std::mem::swap(&mut app.fg, &mut app.bg);
            }
            if ui
                .small_button(crate::i18n::tr("Default"))
                .on_hover_text(crate::i18n::tr("Black and white (D)"))
                .clicked()
            {
                app.fg = Rgba::BLACK;
                app.bg = Rgba::WHITE;
            }
        });
    });
    ui.add_space(4.0);
    let mut fg = app.fg;
    if color_picker(ui, "fg-picker", &mut fg) {
        app.fg = fg;
    }
    ui.add_space(6.0);
    let sw = Vec2::splat(((ui.available_width() - 9.0 * 4.0) / 10.0).floor());
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::splat(4.0);
        for c in PALETTE {
            let c = Rgba::rgb(c[0], c[1], c[2]);
            let r = swatch(ui, c, sw, false).on_hover_text(format!("#{}", c.hex()));
            if r.clicked() {
                if ui.input(|i| i.modifiers.alt) { app.bg = c } else { app.fg = c }
            }
        }
    });
    if !app.prefs.swatches.is_empty() {
        ui.add_space(4.0);
        label_dim(ui, "Recent");
        let recent = app.prefs.swatches.clone();
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = Vec2::splat(4.0);
            for c in recent {
                if swatch(ui, c, sw, false).clicked() {
                    app.fg = c;
                }
            }
        });
    }
}

/// Icon for each adjustment, used in the Adjustments panel and menus.
pub fn adjustment_icon(a: &Adjustment) -> &'static str {
    match a {
        Adjustment::BrightnessContrast { .. } => ph::SUN,
        Adjustment::Levels { .. } => ph::CHART_BAR,
        Adjustment::Curves { .. } => ph::WAVE_SINE,
        Adjustment::Exposure { .. } => ph::APERTURE,
        Adjustment::Vibrance { .. } => ph::DROP_HALF,
        Adjustment::HueSaturation { .. } => ph::PALETTE,
        Adjustment::ColorBalance { .. } => ph::SCALES,
        Adjustment::BlackWhite { .. } => ph::CIRCLE_HALF,
        Adjustment::PhotoFilter { .. } => ph::CAMERA,
        Adjustment::ChannelMixer { .. } => ph::SLIDERS,
        Adjustment::SelectiveColor { .. } => ph::EYEDROPPER_SAMPLE,
        Adjustment::GradientMap { .. } => ph::GRADIENT,
        Adjustment::Invert => ph::SWAP,
        Adjustment::Posterize { .. } => ph::STACK_SIMPLE,
        Adjustment::Threshold { .. } => ph::CIRCLE_HALF_TILT,
        Adjustment::Desaturate => ph::DROP_SLASH,
    }
}

fn adjustments(ui: &mut Ui, actions: &mut Vec<Action>) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::splat(2.0);
        for adj in Adjustment::catalog() {
            let tip = format!("Add a {} adjustment layer", adj.label());
            if icon_button(ui, adjustment_icon(&adj), &tip, false, 30.0).clicked() {
                actions.push(Action::NewAdjustmentLayer(adj));
            }
        }
    });
}

fn properties(app: &mut App, ui: &mut Ui) {
    let now = ui.input(|i| i.time);
    let Some(tab) = app.docs.get_mut(app.active) else {
        return;
    };
    let (w, h) = (tab.doc.width, tab.doc.height);
    let active = tab.doc.active;
    let hist = tab.histogram().clone();
    let Some(layer) = tab.doc.layers.get_mut(active) else {
        return;
    };
    let title = match &layer.kind {
        LayerKind::Adjustment(a) => format!("Properties · {}", a.label()),
        LayerKind::Text(_) => "Properties · Type".into(),
        LayerKind::Shape(_) => "Properties · Shape".into(),
        LayerKind::Fill(_) => "Properties · Fill".into(),
        LayerKind::Group { .. } => "Properties · Group".into(),
        LayerKind::Smart { .. } => "Properties · Smart Object".into(),
        LayerKind::Raster => return,
    };
    let mut changed = false;
    let mut label = "Edit Properties";
    card(ui, "props", &title, true, |ui| match &mut layer.kind {
        LayerKind::Adjustment(adj) => {
            changed = crate::params::adjustment_ui(ui, adj, Some(&hist), &mut app.props);
            label = "Edit Adjustment";
        }
        LayerKind::Text(tl) => {
            label = "Edit Type";
            changed |=
                ui.add(egui::TextEdit::multiline(&mut tl.text).desired_rows(2).desired_width(f32::INFINITY)).changed();
            egui::ComboBox::from_id_salt("prop-font")
                .selected_text(tl.face.label())
                .width(ui.available_width())
                .show_ui(ui, |ui| {
                    for f in FontFace::ALL {
                        changed |= ui.selectable_value(&mut tl.face, f, f.label()).changed();
                    }
                });
            changed |= slider_row(ui, "Size", &mut tl.size, 4.0..=800.0, " px", 0);
            changed |= slider_row(ui, "Leading", &mut tl.leading, 0.5..=3.0, "×", 2);
            changed |= slider_row(ui, "Tracking", &mut tl.tracking, -200.0..=800.0, "", 0);
            changed |= segmented(
                ui,
                &mut tl.align,
                &[
                    (Align::Left, ph::TEXT_ALIGN_LEFT),
                    (Align::Center, ph::TEXT_ALIGN_CENTER),
                    (Align::Right, ph::TEXT_ALIGN_RIGHT),
                ],
            );
            let mut col = tl.color;
            if color_picker(ui, "text-color", &mut col) {
                tl.color = col;
                changed = true;
            }
            changed |= toggle_row(ui, "Vertical", &mut tl.vertical);
            let mut wrap = tl.box_width.is_some();
            if toggle_row(ui, "Paragraph (wrap)", &mut wrap) {
                tl.box_width = if wrap { Some((w as f32 - tl.x).max(100.0) * 0.8) } else { None };
                changed = true;
            }
            if let Some(bw) = &mut tl.box_width {
                changed |= slider_row(ui, "Box width", bw, 20.0..=w as f32, " px", 0);
            }
        }
        LayerKind::Shape(s) => {
            label = "Edit Shape";
            let mut fill = s.fill.is_some();
            if toggle_row(ui, "Fill", &mut fill) {
                s.fill = fill.then_some(app.fg);
                changed = true;
            }
            if let Some(f) = &mut s.fill {
                changed |= color_picker(ui, "shape-fill", f);
            }
            let mut stroke = s.stroke.is_some();
            if toggle_row(ui, "Stroke", &mut stroke) {
                s.stroke = stroke.then_some(app.bg);
                changed = true;
            }
            if let Some(sc) = &mut s.stroke {
                let mut col = c32(*sc);
                if egui::color_picker::color_edit_button_srgba(ui, &mut col, egui::color_picker::Alpha::OnlyBlend)
                    .changed()
                {
                    *sc = rgba(col);
                    changed = true;
                }
                changed |= slider_row(ui, "Width", &mut s.stroke_width, 0.5..=200.0, " px", 1);
                changed |= toggle_row(ui, "Dashed", &mut s.dashed);
            }
            match &mut s.kind {
                ShapeKind::RoundedRect { radius } => changed |= slider_row(ui, "Radius", radius, 0.0..=500.0, " px", 0),
                ShapeKind::Polygon { sides } => {
                    let mut v = *sides as f32;
                    changed |= slider_row(ui, "Sides", &mut v, 3.0..=64.0, "", 0);
                    *sides = v as u32;
                }
                ShapeKind::Star { points, inner } => {
                    let mut v = *points as f32;
                    changed |= slider_row(ui, "Points", &mut v, 3.0..=64.0, "", 0);
                    *points = v as u32;
                    changed |= slider_row(ui, "Inner radius", inner, 0.05..=1.0, "", 2);
                }
                _ => {}
            }
        }
        LayerKind::Fill(fl) => {
            label = "Edit Fill";
            match fl {
                FillLayer::Solid(c) => changed |= color_picker(ui, "fill-solid", c),
                FillLayer::Pattern { pattern, fg, bg, scale } => {
                    let names: Vec<&str> = studio_core::paint::builtin_patterns(*fg, *bg).iter().map(|p| p.0).collect();
                    egui::ComboBox::from_id_salt("fill-pattern")
                        .selected_text(names.get(*pattern).copied().unwrap_or(""))
                        .show_ui(ui, |ui| {
                            for (k, n) in names.iter().enumerate() {
                                changed |= ui.selectable_value(pattern, k, *n).changed();
                            }
                        });
                    for (lbl, col) in [("Foreground", fg), ("Background", bg)] {
                        ui.horizontal(|ui| {
                            label_dim(ui, lbl);
                            let mut cc = c32(*col);
                            if egui::color_picker::color_edit_button_srgba(
                                ui,
                                &mut cc,
                                egui::color_picker::Alpha::OnlyBlend,
                            )
                            .changed()
                            {
                                *col = rgba(cc);
                                changed = true;
                            }
                        });
                    }
                    changed |= slider_row(ui, "Scale", scale, 10.0..=1000.0, "%", 0);
                }
                FillLayer::Gradient { gradient, shape, start, end } => {
                    egui::ComboBox::from_id_salt("fill-grad-shape").selected_text(shape.label()).show_ui(ui, |ui| {
                        for s in GradientShape::ALL {
                            changed |= ui.selectable_value(shape, s, s.label()).changed();
                        }
                    });
                    ui.horizontal_wrapped(|ui| {
                        for (name, g) in Gradient::presets(app.fg, app.bg) {
                            if ui.small_button(name).clicked() {
                                *gradient = g;
                                changed = true;
                            }
                        }
                    });
                    let mut angle = (end.1 - start.1).atan2(end.0 - start.0).to_degrees();
                    if slider_row(ui, "Angle", &mut angle, -180.0..=180.0, "°", 0) {
                        let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
                        let len = ((end.0 - start.0).hypot(end.1 - start.1) / 2.0).max(1.0);
                        let (s, c) = angle.to_radians().sin_cos();
                        *start = (cx - c * len, cy - s * len);
                        *end = (cx + c * len, cy + s * len);
                        changed = true;
                    }
                }
            }
        }
        LayerKind::Smart { filters, .. } => {
            label = "Edit Smart Filters";
            if filters.is_empty() {
                label_dim(ui, "Filters you apply to this layer appear here and stay editable.");
            }
            let mut remove = None;
            for (i, sf) in filters.iter_mut().enumerate().rev() {
                ui.horizontal(|ui| {
                    changed |= ui.checkbox(&mut sf.enabled, "").changed();
                    ui.label(sf.filter.label());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if small_button(ui, ph::TRASH, "Remove filter").clicked() {
                            remove = Some(i);
                        }
                    });
                });
                ui.indent(("sf", i), |ui| {
                    changed |= crate::params::filter_ui(ui, &mut sf.filter);
                });
            }
            if let Some(i) = remove {
                filters.remove(i);
                changed = true;
            }
        }
        LayerKind::Group { pass_through, .. } => {
            label = "Group Properties";
            changed |= toggle_row(ui, "Pass Through", pass_through);
            label_dim(
                ui,
                if *pass_through {
                    "Children blend straight into the layers below."
                } else {
                    "The group is flattened first, then blended as one layer; adjustments inside stay inside."
                },
            );
        }
        LayerKind::Raster => {}
    });
    if changed {
        layer.render(w, h);
        tab.invalidate_all();
        app.props_pending = Some((now, label));
    }
}

fn channels_panel(app: &mut App, ui: &mut Ui, actions: &mut Vec<Action>) {
    let t = Tokens::get(ui.ctx());
    let Some(tab) = app.tab() else { return };
    let names: Vec<String> = tab.doc.channels.iter().map(|c| c.0.clone()).collect();
    let mut qm = app.quick_mask;
    if toggle_row(ui, "Quick Mask (Q)", &mut qm) {
        actions.push(Action::ToggleQuickMask);
    }
    if names.is_empty() {
        label_dim(ui, "Saved selections appear here.");
    }
    for (i, n) in names.iter().enumerate() {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(format!("{}  {}", ph::SELECTION_BACKGROUND, n)).color(t.text));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if small_button(ui, ph::TRASH, "Delete").clicked() {
                    actions.push(Action::DeleteChannel(i));
                }
                if small_button(ui, ph::SELECTION, "Load as selection").clicked() {
                    actions.push(Action::LoadChannel(i));
                }
            });
        });
    }
    if ui.button(format!("{} Save Selection", ph::FLOPPY_DISK)).clicked() {
        actions.push(Action::SaveSelection);
    }
}

fn actions_panel(app: &mut App, ui: &mut Ui, actions: &mut Vec<Action>) {
    let t = Tokens::get(ui.ctx());
    ui.horizontal(|ui| match &app.recording {
        Some(steps) => {
            ui.label(egui::RichText::new(format!("{} Recording · {} steps", ph::RECORD, steps.len())).color(t.danger));
            if ui.button(format!("{} Stop", ph::STOP)).clicked() {
                actions.push(Action::StopRecording);
            }
        }
        None => {
            if ui.button(egui::RichText::new(format!("{} Record", ph::RECORD)).color(t.danger)).clicked() {
                actions.push(Action::StartRecording);
            }
            if ui.button(format!("{} Batch…", ph::FOLDERS)).clicked() {
                actions.push(Action::Batch);
            }
        }
    });
    if app.prefs.macros.is_empty() {
        label_dim(ui, "Record a sequence of edits, then replay it on any image or a whole folder.");
    }
    for (i, (name, steps)) in app.prefs.macros.iter().enumerate() {
        let summary: Vec<String> = steps.iter().map(crate::menus::action_label).collect();
        ui.horizontal(|ui| {
            ui.label(format!("{}  {name}", ph::PLAY_CIRCLE)).on_hover_text(summary.join("\n"));
            label_dim(ui, format!("{} steps", steps.len()));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if small_button(ui, ph::TRASH, "Delete").clicked() {
                    actions.push(Action::DeleteMacro(i));
                }
                if small_button(ui, ph::PLAY, "Play on this image").clicked() {
                    actions.push(Action::PlayMacro(i));
                }
            });
        });
    }
}

fn paths_panel(app: &mut App, ui: &mut Ui, actions: &mut Vec<Action>) {
    let t = Tokens::get(ui.ctx());
    let Some(tab) = app.tab() else { return };
    let names: Vec<String> = tab.doc.paths.iter().map(|p| p.name.clone()).collect();
    if names.is_empty() {
        label_dim(ui, "Draw with the Pen tool (P) to make a path.");
        return;
    }
    for (i, n) in names.iter().enumerate() {
        let sel = app.active_path == Some(i);
        let text = egui::RichText::new(format!("{}  {}", ph::BEZIER_CURVE, n)).color(t.text);
        if ui.add_sized([ui.available_width(), 24.0], egui::Button::selectable(sel, text)).clicked() {
            app.active_path = if sel { None } else { Some(i) };
        }
    }
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        if small_button(ui, ph::SELECTION, "Load path as a selection (⌘↩)").clicked() {
            actions.push(Action::PathToSelection);
        }
        if small_button(ui, ph::PAINT_BUCKET, "Fill path with the foreground color").clicked() {
            actions.push(Action::FillPath);
        }
        if small_button(ui, ph::PAINT_BRUSH, "Stroke path with the foreground color").clicked() {
            actions.push(Action::StrokePath);
        }
        if small_button(ui, ph::POLYGON, "Make a shape layer from the path").clicked() {
            actions.push(Action::PathToShape);
        }
        if small_button(ui, ph::TRASH, "Delete path").clicked() {
            actions.push(Action::DeletePath);
        }
    });
}

fn history(app: &mut App, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let Some(tab) = app.tab_mut() else { return };
    let cur = tab.history.current();
    let labels: Vec<String> = tab.history.labels().map(str::to_owned).collect();
    let mut goto = None;
    egui::ScrollArea::vertical().max_height(180.0).stick_to_bottom(true).show(ui, |ui| {
        for (i, l) in labels.iter().enumerate() {
            let sel = i == cur;
            let future = i > cur;
            let text =
                egui::RichText::new(format!("{}  {}", if i == 0 { ph::IMAGE } else { ph::CLOCK_COUNTER_CLOCKWISE }, l))
                    .color(if future { t.text_faint } else { t.text });
            if ui.add_sized([ui.available_width(), 22.0], egui::Button::selectable(sel, text)).clicked() {
                goto = Some(i);
            }
        }
    });
    if let Some(i) = goto {
        tab.goto_history(i);
    }
}

fn layers(app: &mut App, ui: &mut Ui, actions: &mut Vec<Action>) {
    let t = Tokens::get(ui.ctx());
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(crate::i18n::tr("Layers")).font(crate::theme::medium(13.0)));
    });
    if app.docs.is_empty() {
        label_dim(ui, "Open or create an image to see its layers.");
        return;
    }
    let ctx = ui.ctx().clone();
    let now = ui.input(|i| i.time);
    let active = app.active;

    // Blend mode & opacity for the active layer.
    {
        let tab = &mut app.docs[active];
        let ai = tab.doc.active;
        let layer = &mut tab.doc.layers[ai];
        let mut changed = false;
        // The blend menu gives up width so longer translated labels still fit on the row.
        let label_w = |ui: &Ui, s: &str| {
            let font = egui::FontId::proportional(12.0);
            ui.painter().layout_no_wrap(crate::i18n::tr(s).into_owned(), font, egui::Color32::WHITE).size().x
        };
        let fixed = label_w(ui, "Opacity") + label_w(ui, "layer|Fill") + 2.0 * 52.0 + 6.0 * ui.spacing().item_spacing.x;
        let one_line = ui.available_width() >= fixed + 64.0 + 24.0;
        let combo_w = if one_line { (ui.available_width() - fixed - 24.0).clamp(64.0, 120.0) } else { 120.0 };
        ui.horizontal(|ui| {
            ui.scope(|ui| {
                egui::ComboBox::from_id_salt("blend-mode")
                    .width(combo_w)
                    .selected_text(crate::i18n::tr(layer.blend.label()))
                    .show_ui(ui, |ui| {
                        for m in BlendMode::MENU {
                            match m {
                                Some(m) => {
                                    changed |=
                                        ui.selectable_value(&mut layer.blend, *m, crate::i18n::tr(m.label())).changed()
                                }
                                None => {
                                    ui.separator();
                                }
                            }
                        }
                    });
            });
            let mut pct = layer.opacity * 100.0;
            label_dim(ui, "Opacity");
            if ui.add(egui::DragValue::new(&mut pct).range(0.0..=100.0).suffix("%").speed(1.0)).changed() {
                layer.opacity = pct / 100.0;
                changed = true;
            }
            if one_line {
                changed |= fill_control(ui, layer);
            }
        });
        if !one_line {
            ui.horizontal(|ui| changed |= fill_control(ui, layer));
        }
        ui.horizontal(|ui| {
            label_dim(ui, "Lock");
            let lock_icon = if layer.locked { ph::LOCK } else { ph::LOCK_OPEN };
            if icon_button(ui, lock_icon, "Lock pixels", layer.locked, 24.0).clicked() {
                layer.locked = !layer.locked;
                changed = true;
            }
            let clip_tip = "Clip to the layer below (⌥⌘G)";
            if icon_button(ui, ph::ARROW_ELBOW_DOWN_RIGHT, clip_tip, layer.clip, 24.0).clicked() && ai > 0 {
                layer.clip = !layer.clip;
                changed = true;
            }
            let mut opacity = layer.opacity;
            ui.spacing_mut().slider_width = ui.available_width() - 8.0;
            if ui.add(egui::Slider::new(&mut opacity, 0.0..=1.0).show_value(false)).changed() {
                layer.opacity = opacity;
                changed = true;
            }
        });
        if changed {
            tab.invalidate_all();
            app.props_pending = Some((now, "Layer Properties"));
        }
    }
    ui.add_space(4.0);

    let footer_h = 34.0;
    let list_h = (ui.available_height() - footer_h).max(60.0);
    let mut select: Option<usize> = None;
    let mut select_mask: Option<bool> = None;
    let mut toggle_vis: Option<usize> = None;
    let mut drag_to: Option<(usize, usize)> = None;
    let mut rename: Option<(usize, String)> = None;
    let mut open_style = false;
    let mut load_sel: Option<usize> = None;
    let n = app.docs[active].doc.layers.len();
    // Display order (top first), skipping the contents of collapsed groups.
    let rows: Vec<usize> = {
        let doc = &app.docs[active].doc;
        (0..n)
            .rev()
            .filter(|&i| {
                let mut p = doc.layers[i].parent;
                while let Some(id) = p {
                    match doc.index_of(id) {
                        Some(gi) => {
                            if matches!(doc.layers[gi].kind, LayerKind::Group { collapsed: true, .. }) {
                                return false;
                            }
                            p = doc.layers[gi].parent;
                        }
                        None => break,
                    }
                }
                true
            })
            .collect()
    };
    let mut toggle_collapse: Option<usize> = None;
    let mut set_label: Option<(usize, Option<u8>)> = None;
    let mut multi_click: Option<(usize, bool)> = None;
    egui::ScrollArea::vertical().max_height(list_h).auto_shrink([false, false]).show(ui, |ui| {
        for (row_pos, &i) in rows.iter().enumerate() {
            let depth = app.docs[active].doc.depth(i);
            let group_state = match app.docs[active].doc.layers[i].kind {
                LayerKind::Group { collapsed, .. } => Some(collapsed),
                _ => None,
            };
            let (is_active, name, visible, has_mask, mask_on, clip, kind_icon, badge, styled, locked, lid) = {
                let tab = &app.docs[active];
                let l = &tab.doc.layers[i];
                let icon = match &l.kind {
                    LayerKind::Raster => None,
                    LayerKind::Text(_) => Some(ph::TEXT_T),
                    LayerKind::Shape(_) => Some(ph::POLYGON),
                    LayerKind::Fill(_) => Some(ph::PAINT_BUCKET),
                    LayerKind::Adjustment(_) => Some(ph::CIRCLE_HALF),
                    LayerKind::Group { .. } => None,
                    LayerKind::Smart { .. } => Some(ph::CUBE),
                };
                (
                    i == tab.doc.active,
                    l.name.clone(),
                    l.visible,
                    l.mask.is_some(),
                    l.mask_enabled,
                    l.clip,
                    icon,
                    &crate::i18n::tr(l.kind.badge()),
                    l.styles.any(),
                    l.locked,
                    l.id,
                )
            };
            let thumb = app.docs[active].thumb(&ctx, i, false);
            let mthumb = if has_mask { app.docs[active].thumb(&ctx, i, true) } else { None };
            let row_h = 44.0;
            let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), row_h), Sense::click_and_drag());
            let p = ui.painter_at(rect);
            let in_multi = app.docs[active].multi.len() > 1 && app.docs[active].multi.contains(&i);
            let bg = if is_active || in_multi {
                t.accent_soft
            } else if resp.hovered() {
                t.hover
            } else {
                Color32::TRANSPARENT
            };
            p.rect_filled(rect, CornerRadius::same(8), bg);
            if let Some(lc) =
                app.docs[active].doc.layers[i].label.and_then(|k| studio_core::layer::LABEL_COLORS.get(k as usize))
            {
                p.rect_filled(
                    egui::Rect::from_min_size(rect.min + Vec2::new(1.0, 6.0), Vec2::new(3.0, rect.height() - 12.0)),
                    2.0,
                    c32(*lc),
                );
            }
            let mut x = rect.left() + 6.0 + depth as f32 * 14.0;
            // Visibility.
            let eye = egui::Rect::from_center_size(Pos2::new(x + 10.0, rect.center().y), Vec2::splat(20.0));
            let eye_r = ui.interact(eye, ui.id().with(("eye", lid)), Sense::click());
            p.text(
                eye.center(),
                egui::Align2::CENTER_CENTER,
                if visible { ph::EYE } else { ph::EYE_SLASH },
                egui::FontId::proportional(15.0),
                if visible { t.text_dim } else { t.text_faint },
            );
            if eye_r.clicked() {
                toggle_vis = Some(i);
            }
            x += 24.0;
            if clip {
                p.text(
                    Pos2::new(x + 6.0, rect.center().y),
                    egui::Align2::CENTER_CENTER,
                    ph::ARROW_ELBOW_DOWN_RIGHT,
                    egui::FontId::proportional(13.0),
                    t.text_dim,
                );
                x += 14.0;
            }
            // Thumbnail.
            if let Some(collapsed) = group_state {
                let caret = egui::Rect::from_center_size(Pos2::new(x + 6.0, rect.center().y), Vec2::splat(16.0));
                p.text(
                    caret.center(),
                    egui::Align2::CENTER_CENTER,
                    if collapsed { ph::CARET_RIGHT } else { ph::CARET_DOWN },
                    egui::FontId::proportional(12.0),
                    t.text_dim,
                );
                if ui.interact(caret, ui.id().with(("caret", lid)), Sense::click()).clicked() {
                    toggle_collapse = Some(i);
                }
                x += 14.0;
            }
            let th = egui::Rect::from_min_size(Pos2::new(x, rect.top() + 6.0), Vec2::new(40.0, 32.0));
            let editing_pixels = is_active && !app.edit_mask;
            match thumb {
                _ if group_state.is_some() => {
                    let folder = if group_state == Some(true) { ph::FOLDER_SIMPLE } else { ph::FOLDER_OPEN };
                    p.text(
                        th.center(),
                        egui::Align2::CENTER_CENTER,
                        folder,
                        egui::FontId::proportional(22.0),
                        t.accent,
                    );
                }
                Some(tex) => {
                    crate::widgets::checker(&p, th, 4.0);
                    p.image(tex, th, egui::Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
                }
                None => {
                    p.rect_filled(th, 4.0, t.raised);
                }
            }
            if let Some(ic) = kind_icon {
                let chip = egui::Rect::from_min_size(th.right_bottom() - Vec2::new(14.0, 14.0), Vec2::splat(14.0));
                p.rect_filled(chip, 3.0, t.card);
                p.text(chip.center(), egui::Align2::CENTER_CENTER, ic, egui::FontId::proportional(10.0), t.text);
            }
            p.rect_stroke(
                th,
                4.0,
                Stroke::new(
                    if editing_pixels && has_mask { 2.0 } else { 1.0 },
                    if editing_pixels && has_mask { t.accent } else { t.border },
                ),
                egui::StrokeKind::Outside,
            );
            let thr = ui.interact(th, ui.id().with(("thumb", lid)), Sense::click());
            if thr.clicked() {
                if ui.input(|i| i.modifiers.command) {
                    load_sel = Some(i);
                } else {
                    select = Some(i);
                    select_mask = Some(false);
                }
            }
            x += 46.0;
            if has_mask {
                let mr = egui::Rect::from_min_size(Pos2::new(x, rect.top() + 6.0), Vec2::new(40.0, 32.0));
                if let Some(tex) = mthumb {
                    p.image(tex, mr, egui::Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
                }
                if !mask_on {
                    p.line_segment([mr.left_top(), mr.right_bottom()], Stroke::new(2.0, t.danger));
                    p.line_segment([mr.right_top(), mr.left_bottom()], Stroke::new(2.0, t.danger));
                }
                let editing_mask = is_active && app.edit_mask;
                p.rect_stroke(
                    mr,
                    4.0,
                    Stroke::new(if editing_mask { 2.0 } else { 1.0 }, if editing_mask { t.accent } else { t.border }),
                    egui::StrokeKind::Outside,
                );
                let mrr = ui.interact(mr, ui.id().with(("mask", lid)), Sense::click());
                if mrr.clicked() {
                    select = Some(i);
                    select_mask = Some(true);
                }
                x += 46.0;
            }
            if app.docs[active].doc.layers[i].vector_mask.is_some() {
                let vr = egui::Rect::from_min_size(Pos2::new(x, rect.top() + 6.0), Vec2::new(32.0, 32.0));
                p.rect(vr, 4.0, t.raised, Stroke::new(1.0, t.border), egui::StrokeKind::Outside);
                p.text(
                    vr.center(),
                    egui::Align2::CENTER_CENTER,
                    ph::BEZIER_CURVE,
                    egui::FontId::proportional(16.0),
                    Color32::from_rgb(191, 90, 242),
                );
                x += 38.0;
            }
            // Name.
            let name_rect =
                egui::Rect::from_min_max(Pos2::new(x + 2.0, rect.top()), Pos2::new(rect.right() - 44.0, rect.bottom()));
            let renaming_id = ui.id().with(("rename", lid));
            let renaming = ui.data(|d| d.get_temp::<String>(renaming_id));
            if let Some(mut buf) = renaming {
                let r = ui.put(name_rect.shrink2(Vec2::new(0.0, 10.0)), egui::TextEdit::singleline(&mut buf));
                r.request_focus();
                if r.lost_focus() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    ui.data_mut(|d| d.remove::<String>(renaming_id));
                    if !buf.trim().is_empty() && buf != name {
                        rename = Some((i, buf));
                    }
                } else {
                    ui.data_mut(|d| d.insert_temp(renaming_id, buf));
                }
            } else {
                let galley = p.layout_no_wrap(
                    name.clone(),
                    egui::FontId::proportional(13.0),
                    if visible { t.text } else { t.text_faint },
                );
                p.galley(Pos2::new(name_rect.left(), rect.center().y - 13.0), galley, t.text);
                p.text(
                    Pos2::new(name_rect.left(), rect.center().y + 3.0),
                    egui::Align2::LEFT_TOP,
                    badge,
                    egui::FontId::proportional(10.5),
                    t.text_faint,
                );
            }
            let mut rx = rect.right() - 10.0;
            if locked {
                p.text(
                    Pos2::new(rx, rect.center().y),
                    egui::Align2::RIGHT_CENTER,
                    ph::LOCK,
                    egui::FontId::proportional(13.0),
                    t.text_dim,
                );
                rx -= 18.0;
            }
            if styled {
                let fx = egui::Rect::from_center_size(Pos2::new(rx - 6.0, rect.center().y), Vec2::new(18.0, 18.0));
                p.text(fx.center(), egui::Align2::CENTER_CENTER, "fx", crate::theme::bold(11.0), t.accent);
                if ui.interact(fx, ui.id().with(("fx", lid)), Sense::click()).clicked() {
                    select = Some(i);
                    open_style = true;
                }
            }
            if resp.clicked() {
                let m = ui.input(|inp| inp.modifiers);
                if m.command || m.shift {
                    multi_click = Some((i, m.shift));
                } else {
                    select = Some(i);
                    select_mask = Some(false);
                }
            }
            if resp.double_clicked() {
                ui.data_mut(|d| d.insert_temp(renaming_id, name.clone()));
            }
            // Drag to reorder.
            if resp.dragged() {
                if let Some(pp) = ui.input(|i| i.pointer.hover_pos()) {
                    p.hline(
                        rect.x_range(),
                        if pp.y < rect.center().y { rect.top() } else { rect.bottom() },
                        Stroke::new(2.0, t.accent),
                    );
                }
            }
            if resp.drag_stopped() {
                if let Some(pp) = ui.input(|i| i.pointer.hover_pos()) {
                    let dy = pp.y - rect.center().y;
                    let steps = (dy / (row_h + 2.0)).round() as i32;
                    let target_row = (row_pos as i32 + steps).clamp(0, rows.len() as i32 - 1) as usize;
                    let target = rows[target_row];
                    if target != i {
                        drag_to = Some((i, target));
                    }
                }
            }
            resp.context_menu(|ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    if ui.small_button(crate::i18n::tr("None")).clicked() {
                        set_label = Some((i, None));
                        ui.close();
                    }
                    for (k, c) in studio_core::layer::LABEL_COLORS.iter().enumerate() {
                        if swatch(ui, *c, Vec2::splat(16.0), false).clicked() {
                            set_label = Some((i, Some(k as u8)));
                            ui.close();
                        }
                    }
                });
                ui.separator();
                if ui.button(crate::i18n::tr("Duplicate Layer")).clicked() {
                    select = Some(i);
                    actions.push(Action::DuplicateLayer);
                }
                if ui.button(crate::i18n::tr("Delete Layer")).clicked() {
                    select = Some(i);
                    actions.push(Action::DeleteLayer);
                }
                if ui.button(crate::i18n::tr("Rename…")).clicked() {
                    ui.data_mut(|d| d.insert_temp(renaming_id, name.clone()));
                }
                ui.separator();
                if ui.button(crate::i18n::tr("Layer Style…")).clicked() {
                    select = Some(i);
                    actions.push(Action::LayerStyle);
                }
                if ui.button(crate::i18n::tr("Add Layer Mask")).clicked() {
                    select = Some(i);
                    actions.push(Action::MaskRevealAll);
                }
                if ui.button(crate::i18n::tr("Create Clipping Mask")).clicked() {
                    select = Some(i);
                    actions.push(Action::ToggleClipping);
                }
                if ui.button(crate::i18n::tr("Rasterize Layer")).clicked() {
                    select = Some(i);
                    actions.push(Action::Rasterize);
                }
                if ui.button(crate::i18n::tr("Select Pixels")).clicked() {
                    load_sel = Some(i);
                }
                ui.separator();
                if ui.button(crate::i18n::tr("Merge Down")).clicked() {
                    select = Some(i);
                    actions.push(Action::MergeDown);
                }
            });
            ui.add_space(2.0);
        }
    });

    let tab = &mut app.docs[active];
    if let Some(i) = toggle_vis {
        tab.doc.layers[i].visible = !tab.doc.layers[i].visible;
        tab.commit(if tab.doc.layers[i].visible { "Show Layer" } else { "Hide Layer" });
    }
    if let Some((i, range)) = multi_click {
        let cur = tab.doc.active;
        if tab.multi.is_empty() {
            tab.multi.push(cur);
        }
        if range {
            let (a, b) = (cur.min(i), cur.max(i));
            tab.multi = (a..=b).collect();
        } else if let Some(pos) = tab.multi.iter().position(|&k| k == i) {
            if tab.multi.len() > 1 {
                tab.multi.remove(pos);
            }
        } else {
            tab.multi.push(i);
        }
        tab.doc.active = if tab.multi.contains(&i) { i } else { *tab.multi.last().unwrap_or(&cur) };
        app.edit_mask = false;
    }
    if let Some(i) = select {
        tab.multi.clear();
        if tab.doc.active != i {
            app.stroke = None;
        }
        tab.doc.active = i;
        if let Some(m) = select_mask {
            app.edit_mask = m && tab.doc.layers[i].mask.is_some();
        }
        app.props = Default::default();
    }
    if let Some((i, lab)) = set_label {
        tab.doc.layers[i].label = lab;
        tab.commit("Color Label");
    }
    if let Some(i) = toggle_collapse {
        if let LayerKind::Group { collapsed, .. } = &mut tab.doc.layers[i].kind {
            *collapsed = !*collapsed;
        }
    }
    if let Some((from, to)) = drag_to {
        tab.doc.move_layer(from, to);
        tab.commit("Reorder Layers");
    }
    if let Some((i, name)) = rename {
        tab.doc.layers[i].name = name;
        tab.commit("Rename Layer");
    }
    if let Some(i) = load_sel {
        tab.doc.select_layer_pixels(i);
        tab.commit("Load Selection");
    }
    if open_style {
        actions.push(Action::LayerStyle);
    }

    // Footer.
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        if small_button(ui, ph::LINK, "Create clipping mask").clicked() {
            actions.push(Action::ToggleClipping);
        }
        if small_button(ui, "fx", "Layer style").clicked() {
            actions.push(Action::LayerStyle);
        }
        if small_button(ui, ph::CIRCLE_HALF_TILT, "Add layer mask (from selection if any)").clicked() {
            actions.push(Action::MaskFromSelection);
        }
        let r = small_button(ui, ph::CIRCLE_HALF, "New fill or adjustment layer");
        egui::Popup::menu(&r).show(|ui| {
            if ui.button(crate::i18n::tr("Solid Color…")).clicked() {
                actions.push(Action::NewFillSolid);
            }
            if ui.button(crate::i18n::tr("Gradient…")).clicked() {
                actions.push(Action::NewFillGradient);
            }
            if ui.button(crate::i18n::tr("Pattern…")).clicked() {
                actions.push(Action::NewFillPattern);
            }
            ui.separator();
            for a in Adjustment::catalog() {
                if ui.button(a.label()).clicked() {
                    actions.push(Action::NewAdjustmentLayer(a));
                }
            }
        });
        if small_button(ui, ph::PLUS, "New layer (⇧⌘N)").clicked() {
            actions.push(Action::NewLayer);
        }
        if small_button(ui, ph::COPY, "Duplicate layer").clicked() {
            actions.push(Action::DuplicateLayer);
        }
        if small_button(ui, ph::TRASH, "Delete layer").clicked() {
            actions.push(Action::DeleteLayer);
        }
    });
}

/// The layer's Fill (content opacity) control.
fn fill_control(ui: &mut Ui, layer: &mut studio_core::Layer) -> bool {
    let mut fill = layer.fill_opacity * 100.0;
    label_dim(ui, "layer|Fill").on_hover_text(crate::i18n::tr("Content opacity; layer styles stay at full strength"));
    if ui.add(egui::DragValue::new(&mut fill).range(0.0..=100.0).suffix("%").speed(1.0)).changed() {
        layer.fill_opacity = fill / 100.0;
        return true;
    }
    false
}
