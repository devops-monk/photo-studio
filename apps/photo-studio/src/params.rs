//! Parameter editors for adjustments and filters (shared by dialogs and the Properties panel).

use egui::{Color32, CornerRadius, Pos2, Sense, Stroke, Ui};
use studio_core::Rgba;
use studio_core::adjust::{Adjustment, ColorRange, Curve, LevelsChannel};
use studio_core::filters::{Filter, NoiseDist, PolarMode, RadialMode};
use studio_core::gradient::Gradient;
use studio_core::histogram::Histogram;

use crate::theme::Tokens;
use crate::widgets::{c32, rgba, segmented, slider_row, slider_row_u8, toggle_row};

/// Editor state that isn't part of the adjustment itself.
#[derive(Default, Clone)]
pub struct ParamState {
    pub channel: usize,
    pub range: usize,
    pub drag_point: Option<usize>,
}

fn color_edit(ui: &mut Ui, label: &str, c: &mut Rgba) -> bool {
    let mut col = c32(*c);
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.add_sized([86.0, 20.0], egui::Label::new(egui::RichText::new(label).color(Tokens::get(ui.ctx()).text_dim)));
        if egui::color_picker::color_edit_button_srgba(ui, &mut col, egui::color_picker::Alpha::Opaque).changed() {
            *c = rgba(col);
            changed = true;
        }
    });
    changed
}

/// Draw a histogram into `rect`.
pub fn draw_histogram(painter: &egui::Painter, rect: egui::Rect, h: &Histogram, channel: usize, color: Color32) {
    let data = h.channel(channel);
    let max = data.iter().copied().max().unwrap_or(1).max(1) as f32;
    // Square-root scale so mid-heights stay readable.
    let max = max.sqrt();
    let mut pts = vec![rect.left_bottom()];
    for (i, &v) in data.iter().enumerate() {
        let x = rect.left() + rect.width() * i as f32 / 255.0;
        let y = rect.bottom() - rect.height() * (v as f32).sqrt() / max;
        pts.push(Pos2::new(x, y));
    }
    pts.push(rect.right_bottom());
    let mut mesh = egui::Mesh::default();
    for w in pts.windows(2).skip(1).take(255) {
        let (a, b) = (w[0], w[1]);
        let i = mesh.vertices.len() as u32;
        mesh.colored_vertex(Pos2::new(a.x, rect.bottom()), color);
        mesh.colored_vertex(a, color);
        mesh.colored_vertex(b, color);
        mesh.colored_vertex(Pos2::new(b.x, rect.bottom()), color);
        mesh.add_triangle(i, i + 1, i + 2);
        mesh.add_triangle(i, i + 2, i + 3);
    }
    painter.add(egui::Shape::mesh(mesh));
}

const CHANNELS: [(usize, &str); 4] = [(0, "RGB"), (1, "Red"), (2, "Green"), (3, "Blue")];

fn channel_color(c: usize, alpha: u8) -> Color32 {
    match c {
        1 => Color32::from_rgba_unmultiplied(255, 69, 58, alpha),
        2 => Color32::from_rgba_unmultiplied(48, 209, 88, alpha),
        3 => Color32::from_rgba_unmultiplied(10, 132, 255, alpha),
        _ => Color32::from_rgba_unmultiplied(160, 160, 165, alpha),
    }
}

fn hist_channel(c: usize) -> usize {
    match c {
        1 => 0,
        2 => 1,
        3 => 2,
        _ => 3,
    }
}

fn curves_editor(ui: &mut Ui, curve: &mut Curve, hist: Option<&Histogram>, st: &mut ParamState) -> bool {
    let t = Tokens::get(ui.ctx());
    let side = ui.available_width().min(280.0);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(side, side), Sense::click_and_drag());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, CornerRadius::same(6), if t.dark { Color32::from_gray(28) } else { Color32::from_gray(250) });
    if let Some(h) = hist {
        draw_histogram(&p, rect.shrink(1.0), h, hist_channel(st.channel), channel_color(st.channel, 50));
    }
    for i in 1..4 {
        let f = i as f32 / 4.0;
        p.vline(rect.left() + rect.width() * f, rect.y_range(), Stroke::new(1.0, t.separator));
        p.hline(rect.x_range(), rect.top() + rect.height() * f, Stroke::new(1.0, t.separator));
    }
    p.line_segment([rect.left_bottom(), rect.right_top()], Stroke::new(1.0, t.text_faint));
    let to_screen =
        |x: f32, y: f32| Pos2::new(rect.left() + rect.width() * x / 255.0, rect.bottom() - rect.height() * y / 255.0);
    let from_screen = |pos: Pos2| {
        (
            ((pos.x - rect.left()) / rect.width() * 255.0).clamp(0.0, 255.0),
            ((rect.bottom() - pos.y) / rect.height() * 255.0).clamp(0.0, 255.0),
        )
    };
    let lut = curve.lut();
    let line: Vec<Pos2> = (0..256).map(|i| to_screen(i as f32, lut[i] as f32)).collect();
    p.add(egui::Shape::line(
        line,
        Stroke::new(2.0, if st.channel == 0 { t.text } else { channel_color(st.channel, 255) }),
    ));

    let mut changed = false;
    if resp.drag_started() || resp.clicked() {
        if let Some(pos) = resp.interact_pointer_pos() {
            let near = curve.points.iter().position(|&(x, y)| to_screen(x as f32, y as f32).distance(pos) < 10.0);
            st.drag_point = match near {
                Some(i) => Some(i),
                None => {
                    let (x, y) = from_screen(pos);
                    curve.points.push((x.round() as u8, y.round() as u8));
                    curve.points.sort_by_key(|p| p.0);
                    changed = true;
                    curve.points.iter().position(|&(px, _)| px == x.round() as u8)
                }
            };
        }
    }
    if resp.dragged() {
        if let (Some(i), Some(pos)) = (st.drag_point, resp.interact_pointer_pos()) {
            let outside = !rect.expand(24.0).contains(pos);
            let last = curve.points.len() - 1;
            if outside && i != 0 && i != last && curve.points.len() > 2 {
                curve.points.remove(i);
                st.drag_point = None;
            } else if i < curve.points.len() {
                let (x, y) = from_screen(pos);
                let lo = if i == 0 { 0.0 } else { curve.points[i - 1].0 as f32 + 1.0 };
                let hi = if i == last { 255.0 } else { curve.points[i + 1].0 as f32 - 1.0 };
                curve.points[i] = (x.clamp(lo, hi.max(lo)).round() as u8, y.round() as u8);
            }
            changed = true;
        }
    }
    if resp.drag_stopped() {
        st.drag_point = None;
    }
    for (i, &(x, y)) in curve.points.iter().enumerate() {
        let c = to_screen(x as f32, y as f32);
        let active = st.drag_point == Some(i);
        p.circle(c, if active { 5.5 } else { 4.5 }, if active { t.accent } else { t.card }, Stroke::new(1.5, t.text));
    }
    ui.horizontal(|ui| {
        if let Some(pos) = resp.hover_pos() {
            let (x, y) = from_screen(pos);
            crate::widgets::label_dim(ui, format!("Input {:.0}  Output {:.0}", x, y));
        } else {
            crate::widgets::label_dim(ui, "Click to add a point · drag off to remove");
        }
    });
    if ui.button(crate::i18n::tr("Reset curve")).clicked() {
        *curve = Curve::default();
        changed = true;
    }
    changed
}

fn levels_editor(ui: &mut Ui, l: &mut LevelsChannel, hist: Option<&Histogram>, channel: usize) -> bool {
    let t = Tokens::get(ui.ctx());
    let w = ui.available_width().min(300.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, 90.0), Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, CornerRadius::same(6), if t.dark { Color32::from_gray(28) } else { Color32::from_gray(250) });
    if let Some(h) = hist {
        draw_histogram(&p, rect.shrink(1.0), h, hist_channel(channel), channel_color(channel, 140));
    }
    // Input markers.
    for (v, c) in [(l.in_black, Color32::BLACK), (l.in_white, Color32::WHITE)] {
        let x = rect.left() + rect.width() * v as f32 / 255.0;
        p.add(egui::Shape::convex_polygon(
            vec![
                Pos2::new(x, rect.bottom() - 8.0),
                Pos2::new(x - 5.0, rect.bottom()),
                Pos2::new(x + 5.0, rect.bottom()),
            ],
            c,
            Stroke::new(1.0, t.text_faint),
        ));
    }
    let mut changed = false;
    changed |= slider_row_u8(ui, "Black", &mut l.in_black, "");
    changed |= slider_row(ui, "Midtones", &mut l.gamma, 0.1..=9.99, "", 2);
    changed |= slider_row_u8(ui, "White", &mut l.in_white, "");
    ui.add_space(4.0);
    crate::widgets::label_dim(ui, "Output levels");
    changed |= slider_row_u8(ui, "Black", &mut l.out_black, "");
    changed |= slider_row_u8(ui, "White", &mut l.out_white, "");
    if l.in_white <= l.in_black {
        l.in_white = l.in_black.saturating_add(1);
    }
    changed
}

fn gradient_bar(ui: &mut Ui, g: &Gradient) {
    let w = ui.available_width().min(300.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, 16.0), Sense::hover());
    let p = ui.painter_at(rect);
    crate::widgets::checker(&p, rect, 4.0);
    let mut mesh = egui::Mesh::default();
    let n = 64;
    for i in 0..n {
        let (a, b) = (i as f32 / n as f32, (i + 1) as f32 / n as f32);
        let ca = c32(g.sample(a));
        let cb = c32(g.sample(b));
        let x0 = rect.left() + rect.width() * a;
        let x1 = rect.left() + rect.width() * b;
        let idx = mesh.vertices.len() as u32;
        mesh.colored_vertex(Pos2::new(x0, rect.top()), ca);
        mesh.colored_vertex(Pos2::new(x1, rect.top()), cb);
        mesh.colored_vertex(Pos2::new(x1, rect.bottom()), cb);
        mesh.colored_vertex(Pos2::new(x0, rect.bottom()), ca);
        mesh.add_triangle(idx, idx + 1, idx + 2);
        mesh.add_triangle(idx, idx + 2, idx + 3);
    }
    p.add(egui::Shape::mesh(mesh));
    p.rect_stroke(
        rect,
        CornerRadius::same(3),
        Stroke::new(1.0, Color32::from_black_alpha(80)),
        egui::StrokeKind::Inside,
    );
}

/// Edit an adjustment's parameters; returns true when anything changed.
pub fn adjustment_ui(ui: &mut Ui, adj: &mut Adjustment, hist: Option<&Histogram>, st: &mut ParamState) -> bool {
    let mut c = false;
    match adj {
        Adjustment::BrightnessContrast { brightness, contrast } => {
            c |= slider_row(ui, "Brightness", brightness, -100.0..=100.0, "", 0);
            c |= slider_row(ui, "Contrast", contrast, -100.0..=100.0, "", 0);
        }
        Adjustment::Levels { master, red, green, blue } => {
            c |= segmented(ui, &mut st.channel, &CHANNELS);
            let ch = match st.channel {
                1 => red,
                2 => green,
                3 => blue,
                _ => master,
            };
            c |= levels_editor(ui, ch, hist, st.channel);
        }
        Adjustment::Curves { master, red, green, blue } => {
            let mut chn = st.channel;
            if segmented(ui, &mut chn, &CHANNELS) {
                st.channel = chn;
                st.drag_point = None;
            }
            let curve = match st.channel {
                1 => red,
                2 => green,
                3 => blue,
                _ => master,
            };
            c |= curves_editor(ui, curve, hist, st);
        }
        Adjustment::Exposure { exposure, offset, gamma } => {
            c |= slider_row(ui, "Exposure", exposure, -5.0..=5.0, "", 2);
            c |= slider_row(ui, "Offset", offset, -0.5..=0.5, "", 3);
            c |= slider_row(ui, "Gamma", gamma, 0.1..=4.0, "", 2);
        }
        Adjustment::Vibrance { vibrance, saturation } => {
            c |= slider_row(ui, "Vibrance", vibrance, -100.0..=100.0, "", 0);
            c |= slider_row(ui, "Saturation", saturation, -100.0..=100.0, "", 0);
        }
        Adjustment::HueSaturation { hue, saturation, lightness, colorize } => {
            c |= toggle_row(ui, "Colorize", colorize);
            if *colorize {
                if *hue < 0.0 {
                    *hue += 360.0;
                }
                c |= slider_row(ui, "Hue", hue, 0.0..=360.0, "°", 0);
                c |= slider_row(ui, "Saturation", saturation, 0.0..=100.0, "", 0);
            } else {
                if *hue > 180.0 {
                    *hue -= 360.0;
                }
                c |= slider_row(ui, "Hue", hue, -180.0..=180.0, "°", 0);
                c |= slider_row(ui, "Saturation", saturation, -100.0..=100.0, "", 0);
            }
            c |= slider_row(ui, "Lightness", lightness, -100.0..=100.0, "", 0);
            hue_strip(ui);
        }
        Adjustment::ColorBalance { shadows, midtones, highlights, preserve_luminosity } => {
            let mut tone = st.range.min(2);
            c |= segmented(ui, &mut tone, &[(0, "Shadows"), (1, "Midtones"), (2, "Highlights")]);
            st.range = tone;
            let v = match tone {
                0 => shadows,
                2 => highlights,
                _ => midtones,
            };
            c |= slider_row(ui, "Cyan ↔ Red", &mut v[0], -100.0..=100.0, "", 0);
            c |= slider_row(ui, "Magenta ↔ Green", &mut v[1], -100.0..=100.0, "", 0);
            c |= slider_row(ui, "Yellow ↔ Blue", &mut v[2], -100.0..=100.0, "", 0);
            c |= toggle_row(ui, "Preserve Luminosity", preserve_luminosity);
        }
        Adjustment::BlackWhite { weights, tint } => {
            for (i, n) in ["Reds", "Yellows", "Greens", "Cyans", "Blues", "Magentas"].iter().enumerate() {
                c |= slider_row(ui, n, &mut weights[i], -200.0..=300.0, "%", 0);
            }
            let mut on = tint.is_some();
            if toggle_row(ui, "Tint", &mut on) {
                *tint = if on { Some(Rgba::rgb(225, 211, 179)) } else { None };
                c = true;
            }
            if let Some(tc) = tint {
                c |= color_edit(ui, "Tint color", tc);
            }
        }
        Adjustment::PhotoFilter { color, density, preserve_luminosity } => {
            ui.horizontal(|ui| {
                for (name, col) in [
                    ("Warming", Rgba::rgb(236, 138, 0)),
                    ("Cooling", Rgba::rgb(0, 109, 255)),
                    ("Sepia", Rgba::rgb(172, 122, 51)),
                    ("Green", Rgba::rgb(25, 201, 25)),
                ] {
                    if ui.small_button(name).clicked() {
                        *color = col;
                        c = true;
                    }
                }
            });
            c |= color_edit(ui, "Color", color);
            c |= slider_row(ui, "Density", density, 0.0..=100.0, "%", 0);
            c |= toggle_row(ui, "Preserve Luminosity", preserve_luminosity);
        }
        Adjustment::ChannelMixer { red, green, blue, monochrome } => {
            c |= toggle_row(ui, "Monochrome", monochrome);
            let names = if *monochrome { vec![("Gray", 0usize)] } else { vec![("Red", 0), ("Green", 1), ("Blue", 2)] };
            let mut out = st.channel.min(2);
            if !*monochrome {
                c |= segmented(ui, &mut out, &names.iter().map(|(n, i)| (*i, *n)).collect::<Vec<_>>());
                st.channel = out;
            }
            let row = match if *monochrome { 0 } else { out } {
                1 => green,
                2 => blue,
                _ => red,
            };
            c |= slider_row(ui, "Red", &mut row[0], -200.0..=200.0, "%", 0);
            c |= slider_row(ui, "Green", &mut row[1], -200.0..=200.0, "%", 0);
            c |= slider_row(ui, "Blue", &mut row[2], -200.0..=200.0, "%", 0);
            c |= slider_row(ui, "Constant", &mut row[3], -200.0..=200.0, "%", 0);
            let total = row[0] + row[1] + row[2];
            crate::widgets::label_dim(ui, format!("Total {total:+.0}%"));
        }
        Adjustment::SelectiveColor { ranges, absolute } => {
            let mut idx = st.range.min(ranges.len().saturating_sub(1));
            egui::ComboBox::from_id_salt("sel-color-range")
                .selected_text(ranges.get(idx).map_or("", |r| r.0.label()))
                .show_ui(ui, |ui| {
                    for (i, (r, _)) in ranges.iter().enumerate() {
                        ui.selectable_value(&mut idx, i, r.label());
                    }
                });
            st.range = idx;
            if let Some((_, v)) = ranges.get_mut(idx) {
                c |= slider_row(ui, "Cyan", &mut v.c, -100.0..=100.0, "%", 0);
                c |= slider_row(ui, "Magenta", &mut v.m, -100.0..=100.0, "%", 0);
                c |= slider_row(ui, "Yellow", &mut v.y, -100.0..=100.0, "%", 0);
                c |= slider_row(ui, "Black", &mut v.k, -100.0..=100.0, "%", 0);
            }
            let mut mode = *absolute;
            if segmented(ui, &mut mode, &[(false, "Relative"), (true, "Absolute")]) {
                *absolute = mode;
                c = true;
            }
            let _ = ColorRange::ALL;
        }
        Adjustment::GradientMap { gradient, reverse } => {
            gradient_bar(ui, gradient);
            ui.horizontal_wrapped(|ui| {
                for (name, g) in Gradient::presets(Rgba::BLACK, Rgba::WHITE) {
                    if ui.small_button(name).clicked() {
                        *gradient = g;
                        c = true;
                    }
                }
            });
            for (i, stop) in gradient.stops.iter_mut().enumerate() {
                c |= color_edit(ui, &format!("Stop {}", i + 1), &mut stop.color);
            }
            c |= toggle_row(ui, "Reverse", reverse);
        }
        Adjustment::Posterize { levels } => {
            let mut v = *levels as f32;
            c |= slider_row(ui, "Levels", &mut v, 2.0..=64.0, "", 0);
            *levels = v.round() as u8;
        }
        Adjustment::Threshold { level } => {
            if let Some(h) = hist {
                let w = ui.available_width().min(300.0);
                let (rect, _) = ui.allocate_exact_size(egui::vec2(w, 60.0), Sense::hover());
                draw_histogram(&ui.painter_at(rect), rect, h, 3, channel_color(0, 160));
                let x = rect.left() + rect.width() * *level as f32 / 255.0;
                ui.painter().vline(x, rect.y_range(), Stroke::new(1.5, Tokens::get(ui.ctx()).accent));
            }
            c |= slider_row_u8(ui, "Threshold", level, "");
        }
        Adjustment::Invert | Adjustment::Desaturate => {
            crate::widgets::label_dim(ui, "No settings.");
        }
    }
    c
}

fn hue_strip(ui: &mut Ui) {
    let w = ui.available_width().min(300.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, 8.0), Sense::hover());
    let mut mesh = egui::Mesh::default();
    let n = 36;
    for i in 0..n {
        let (a, b) = (i as f32 / n as f32, (i + 1) as f32 / n as f32);
        let col = |t: f32| {
            let (r, g, b) = studio_core::color::hsv_to_rgb(t * 360.0, 0.9, 1.0);
            Color32::from_rgb((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
        };
        let idx = mesh.vertices.len() as u32;
        mesh.colored_vertex(Pos2::new(rect.left() + rect.width() * a, rect.top()), col(a));
        mesh.colored_vertex(Pos2::new(rect.left() + rect.width() * b, rect.top()), col(b));
        mesh.colored_vertex(Pos2::new(rect.left() + rect.width() * b, rect.bottom()), col(b));
        mesh.colored_vertex(Pos2::new(rect.left() + rect.width() * a, rect.bottom()), col(a));
        mesh.add_triangle(idx, idx + 1, idx + 2);
        mesh.add_triangle(idx, idx + 2, idx + 3);
    }
    ui.painter().add(egui::Shape::mesh(mesh));
}

/// Edit a filter's parameters; returns true when anything changed.
pub fn filter_ui(ui: &mut Ui, f: &mut Filter) -> bool {
    let mut c = false;
    match f {
        Filter::GaussianBlur { radius } | Filter::BoxBlur { radius } => {
            c |= slider_row(ui, "Radius", radius, 0.1..=250.0, " px", 1);
        }
        Filter::MotionBlur { angle, distance } => {
            c |= slider_row(ui, "Angle", angle, -180.0..=180.0, "°", 0);
            c |= slider_row(ui, "Distance", distance, 1.0..=500.0, " px", 0);
        }
        Filter::RadialBlur { amount, mode, cx, cy } => {
            c |= segmented(ui, mode, &[(RadialMode::Spin, "Spin"), (RadialMode::Zoom, "Zoom")]);
            c |= slider_row(ui, "Amount", amount, 1.0..=100.0, "", 0);
            let (mut x, mut y) = (*cx * 100.0, *cy * 100.0);
            c |= slider_row(ui, "Center X", &mut x, 0.0..=100.0, "%", 0);
            c |= slider_row(ui, "Center Y", &mut y, 0.0..=100.0, "%", 0);
            (*cx, *cy) = (x / 100.0, y / 100.0);
        }
        Filter::SurfaceBlur { radius, threshold } => {
            c |= slider_row(ui, "Radius", radius, 1.0..=25.0, " px", 0);
            c |= slider_row(ui, "Threshold", threshold, 2.0..=255.0, "", 0);
        }
        Filter::TiltShift { focus_y, focus_height, transition, blur } => {
            let mut fy = *focus_y * 100.0;
            let mut fh = *focus_height * 100.0;
            let mut tr = *transition * 100.0;
            c |= slider_row(ui, "Focus", &mut fy, 0.0..=100.0, "%", 0);
            c |= slider_row(ui, "Sharp band", &mut fh, 0.0..=100.0, "%", 0);
            c |= slider_row(ui, "Transition", &mut tr, 1.0..=100.0, "%", 0);
            c |= slider_row(ui, "Blur", blur, 1.0..=80.0, " px", 0);
            (*focus_y, *focus_height, *transition) = (fy / 100.0, fh / 100.0, tr / 100.0);
        }
        Filter::PathBlur { points, speed } => {
            c |= slider_row(ui, "Speed", speed, 1.0..=200.0, " px", 0);
            let mut remove = None;
            for (i, (x, y)) in points.iter_mut().enumerate() {
                ui.label(egui::RichText::new(format!("Point {}", i + 1)).strong());
                let (mut px, mut py) = (*x * 100.0, *y * 100.0);
                c |= slider_row(ui, "X", &mut px, 0.0..=100.0, "%", 0);
                c |= slider_row(ui, "Y", &mut py, 0.0..=100.0, "%", 0);
                (*x, *y) = (px / 100.0, py / 100.0);
                if i >= 2 && ui.small_button(crate::i18n::tr("Remove point")).clicked() {
                    remove = Some(i);
                }
            }
            if let Some(i) = remove {
                points.remove(i);
                c = true;
            }
            if points.len() < 8 && ui.button(crate::i18n::tr("Add point")).clicked() {
                let last = *points.last().unwrap_or(&(0.5, 0.5));
                points.push(((last.0 + 0.1).min(1.0), last.1));
                c = true;
            }
        }
        Filter::FieldBlur { pins } => {
            let mut remove = None;
            for (i, (x, y, r)) in pins.iter_mut().enumerate() {
                ui.label(egui::RichText::new(format!("Pin {}", i + 1)).strong());
                let (mut px, mut py) = (*x * 100.0, *y * 100.0);
                c |= slider_row(ui, "X", &mut px, 0.0..=100.0, "%", 0);
                c |= slider_row(ui, "Y", &mut py, 0.0..=100.0, "%", 0);
                c |= slider_row(ui, "Blur", r, 0.0..=80.0, " px", 0);
                (*x, *y) = (px / 100.0, py / 100.0);
                if ui.small_button(crate::i18n::tr("Remove pin")).clicked() {
                    remove = Some(i);
                }
            }
            if let Some(i) = remove {
                pins.remove(i);
                c = true;
            }
            if pins.len() < 6 && ui.button(crate::i18n::tr("Add pin")).clicked() {
                pins.push((0.5, 0.5, 10.0));
                c = true;
            }
        }
        Filter::IrisBlur { cx, cy, rx, ry, transition, blur } => {
            for (label, v) in
                [("Center X", cx), ("Center Y", cy), ("Width", rx), ("Height", ry), ("Transition", transition)]
            {
                let mut p = *v * 100.0;
                if slider_row(ui, label, &mut p, 1.0..=100.0, "%", 0) {
                    *v = p / 100.0;
                    c = true;
                }
            }
            c |= slider_row(ui, "Blur", blur, 1.0..=80.0, " px", 0);
        }
        Filter::UnsharpMask { amount, radius, threshold } => {
            c |= slider_row(ui, "Amount", amount, 1.0..=500.0, "%", 0);
            c |= slider_row(ui, "Radius", radius, 0.1..=100.0, " px", 1);
            c |= slider_row_u8(ui, "Threshold", threshold, "");
        }
        Filter::AddNoise { amount, dist, monochrome, seed } => {
            c |= slider_row(ui, "Amount", amount, 0.1..=100.0, "%", 1);
            c |= segmented(ui, dist, &[(NoiseDist::Uniform, "Uniform"), (NoiseDist::Gaussian, "Gaussian")]);
            c |= toggle_row(ui, "Monochromatic", monochrome);
            if ui.small_button(crate::i18n::tr("New seed")).clicked() {
                *seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
                c = true;
            }
        }
        Filter::Median { radius } => c |= slider_row(ui, "Radius", radius, 1.0..=20.0, " px", 0),
        Filter::ReduceNoise { strength, preserve_details } => {
            c |= slider_row(ui, "Strength", strength, 1.0..=10.0, "", 0);
            c |= slider_row(ui, "Preserve details", preserve_details, 0.0..=100.0, "%", 0);
        }
        Filter::DustScratches { radius, threshold } => {
            c |= slider_row(ui, "Radius", radius, 1.0..=16.0, " px", 0);
            c |= slider_row_u8(ui, "Threshold", threshold, "");
        }
        Filter::Twirl { angle } => c |= slider_row(ui, "Angle", angle, -999.0..=999.0, "°", 0),
        Filter::Wave { amplitude, wavelength, horizontal, vertical } => {
            c |= slider_row(ui, "Amplitude", amplitude, 1.0..=100.0, " px", 0);
            c |= slider_row(ui, "Wavelength", wavelength, 2.0..=500.0, " px", 0);
            c |= toggle_row(ui, "Horizontal", horizontal);
            c |= toggle_row(ui, "Vertical", vertical);
        }
        Filter::Ripple { amount, size } => {
            c |= slider_row(ui, "Amount", amount, -999.0..=999.0, "%", 0);
            c |= slider_row(ui, "Size", size, 2.0..=100.0, " px", 0);
        }
        Filter::Pinch { amount } => c |= slider_row(ui, "Amount", amount, -100.0..=100.0, "%", 0),
        Filter::Spherize { amount } => c |= slider_row(ui, "Amount", amount, -100.0..=100.0, "%", 0),
        Filter::PolarCoordinates { mode } => {
            c |= segmented(
                ui,
                mode,
                &[(PolarMode::RectToPolar, "Rectangular → Polar"), (PolarMode::PolarToRect, "Polar → Rectangular")],
            );
        }
        Filter::Mosaic { cell } => c |= slider_row(ui, "Cell size", cell, 2.0..=200.0, " px", 0),
        Filter::Crystallize { cell, seed } => {
            c |= slider_row(ui, "Cell size", cell, 3.0..=300.0, " px", 0);
            if ui.small_button(crate::i18n::tr("New pattern")).clicked() {
                *seed = seed.wrapping_add(1);
                c = true;
            }
        }
        Filter::ColorHalftone { radius } => c |= slider_row(ui, "Max radius", radius, 2.0..=64.0, " px", 0),
        Filter::Emboss { angle, height, amount } => {
            c |= slider_row(ui, "Angle", angle, -180.0..=180.0, "°", 0);
            c |= slider_row(ui, "Height", height, 1.0..=10.0, " px", 0);
            c |= slider_row(ui, "Amount", amount, 1.0..=500.0, "%", 0);
        }
        Filter::GlowingEdges { width, brightness } => {
            c |= slider_row(ui, "Edge width", width, 1.0..=14.0, " px", 0);
            c |= slider_row(ui, "Brightness", brightness, 0.0..=20.0, "", 0);
        }
        Filter::OilPaint { radius, levels } => {
            c |= slider_row(ui, "Stylization", radius, 1.0..=10.0, "", 0);
            let mut l = *levels as f32;
            c |= slider_row(ui, "Detail", &mut l, 2.0..=64.0, "", 0);
            *levels = l as u8;
        }
        Filter::Clouds { seed, scale } => {
            c |= slider_row(ui, "Scale", scale, 8.0..=1000.0, " px", 0);
            crate::widgets::label_dim(ui, "Uses the foreground and background colors.");
            if ui.small_button(crate::i18n::tr("New clouds")).clicked() {
                *seed = seed.wrapping_add(7);
                c = true;
            }
        }
        Filter::Vignette { amount, midpoint, feather } => {
            c |= slider_row(ui, "Amount", amount, -100.0..=100.0, "", 0);
            c |= slider_row(ui, "Midpoint", midpoint, 0.0..=100.0, "", 0);
            c |= slider_row(ui, "Feather", feather, 1.0..=100.0, "", 0);
        }
        Filter::LensFlare { cx, cy, brightness } => {
            let (mut x, mut y) = (*cx * 100.0, *cy * 100.0);
            c |= slider_row(ui, "Center X", &mut x, 0.0..=100.0, "%", 0);
            c |= slider_row(ui, "Center Y", &mut y, 0.0..=100.0, "%", 0);
            c |= slider_row(ui, "Brightness", brightness, 10.0..=300.0, "%", 0);
            (*cx, *cy) = (x / 100.0, y / 100.0);
        }
        Filter::Sharpen | Filter::SharpenMore | Filter::FindEdges | Filter::Solarize => {
            crate::widgets::label_dim(ui, "No settings.");
        }
    }
    c
}
