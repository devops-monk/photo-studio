//! Small reusable widgets in the app's visual language.

use egui::{Color32, CornerRadius, Response, Sense, Stroke, Ui, Vec2};
use studio_core::Rgba;

use crate::theme::{RADIUS, Tokens};

pub fn c32(c: Rgba) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), c.a())
}

pub fn rgba(c: Color32) -> Rgba {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    Rgba::rgba(r, g, b, a)
}

/// A square icon button with hover and selected states.
pub fn icon_button(ui: &mut Ui, icon: &str, tooltip: &str, selected: bool, size: f32) -> Response {
    let t = Tokens::get(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    let bg = if selected {
        t.accent
    } else if resp.hovered() {
        t.hover
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, CornerRadius::same(7), bg);
    let fg = if selected {
        t.accent_text
    } else if resp.hovered() {
        t.text
    } else {
        t.text_dim
    };
    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, icon, egui::FontId::proportional(size * 0.56), fg);
    if !tooltip.is_empty() { resp.on_hover_text(tooltip) } else { resp }
}

/// A compact icon+label button used in panel footers and dialogs.
pub fn small_button(ui: &mut Ui, icon: &str, tooltip: &str) -> Response {
    icon_button(ui, icon, tooltip, false, 26.0)
}

/// A filled accent button.
pub fn primary_button(ui: &mut Ui, text: &str) -> Response {
    let t = Tokens::get(ui.ctx());
    ui.add(
        egui::Button::new(
            egui::RichText::new(crate::i18n::tr(text)).color(t.accent_text).font(crate::theme::medium(13.0)),
        )
        .fill(t.accent)
        .corner_radius(CornerRadius::same(RADIUS))
        .min_size(egui::vec2(84.0, 28.0)),
    )
}

pub fn secondary_button(ui: &mut Ui, text: &str) -> Response {
    ui.add(
        egui::Button::new(crate::i18n::tr(text))
            .corner_radius(CornerRadius::same(RADIUS))
            .min_size(egui::vec2(84.0, 28.0)),
    )
}

/// A titled, collapsible card used to group panel content.
pub fn card<R>(ui: &mut Ui, id: &str, title: &str, default_open: bool, add: impl FnOnce(&mut Ui) -> R) -> Option<R> {
    let t = Tokens::get(ui.ctx());
    let id = ui.make_persistent_id(id);
    let mut open = ui.data_mut(|d| *d.get_persisted_mut_or(id, default_open));
    let mut out = None;
    egui::Frame::new()
        .fill(t.card)
        .corner_radius(CornerRadius::same(10))
        .stroke(Stroke::new(1.0, t.separator))
        .inner_margin(egui::Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            let header = ui.horizontal(|ui| {
                let caret = if open { egui_phosphor::regular::CARET_DOWN } else { egui_phosphor::regular::CARET_RIGHT };
                ui.label(egui::RichText::new(caret).color(t.text_faint).size(11.0));
                ui.label(egui::RichText::new(crate::i18n::tr(title)).font(crate::theme::medium(12.5)).color(t.text));
                ui.allocate_space(egui::vec2(ui.available_width(), 1.0));
            });
            let r = ui.interact(header.response.rect, id.with("hdr"), Sense::click());
            if r.clicked() {
                open = !open;
                ui.data_mut(|d| d.insert_persisted(id, open));
            }
            if open {
                ui.add_space(4.0);
                out = Some(add(ui));
            }
        });
    ui.add_space(6.0);
    out
}

/// "Label ─────●─── value" row.
pub fn slider_row(
    ui: &mut Ui,
    label: &str,
    v: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    suffix: &str,
    decimals: usize,
) -> bool {
    let t = Tokens::get(ui.ctx());
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.add_sized([86.0, 20.0], egui::Label::new(egui::RichText::new(label).color(t.text_dim)).truncate());
        let w = (ui.available_width() - 64.0).max(60.0);
        ui.spacing_mut().slider_width = w;
        let r = ui.add(egui::Slider::new(v, range.clone()).show_value(false));
        changed |= r.changed();
        let r = ui.add_sized(
            [56.0, 20.0],
            egui::DragValue::new(v).range(range).fixed_decimals(decimals).suffix(suffix).speed(0.5),
        );
        changed |= r.changed();
    });
    changed
}

pub fn slider_row_u8(ui: &mut Ui, label: &str, v: &mut u8, suffix: &str) -> bool {
    let mut f = *v as f32;
    let c = slider_row(ui, label, &mut f, 0.0..=255.0, suffix, 0);
    *v = f.round().clamp(0.0, 255.0) as u8;
    c
}

pub fn slider_row_pct(ui: &mut Ui, label: &str, v: &mut f32) -> bool {
    let mut p = *v * 100.0;
    let c = slider_row(ui, label, &mut p, 0.0..=100.0, "%", 0);
    *v = p / 100.0;
    c
}

/// An Apple-style segmented control.
pub fn segmented<T: PartialEq + Copy>(ui: &mut Ui, value: &mut T, options: &[(T, &str)]) -> bool {
    let t = Tokens::get(ui.ctx());
    let mut changed = false;
    egui::Frame::new().fill(t.raised).corner_radius(CornerRadius::same(7)).inner_margin(egui::Margin::same(2)).show(
        ui,
        |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            ui.horizontal(|ui| {
                for (v, label) in options {
                    let sel = *value == *v;
                    let text = egui::RichText::new(*label).size(12.0).color(if sel { t.text } else { t.text_dim });
                    let b = egui::Button::new(text)
                        .fill(if sel {
                            if t.dark { Color32::from_rgb(92, 92, 98) } else { Color32::WHITE }
                        } else {
                            Color32::TRANSPARENT
                        })
                        .corner_radius(CornerRadius::same(5))
                        .min_size(egui::vec2(0.0, 20.0));
                    if ui.add(b).clicked() && !sel {
                        *value = *v;
                        changed = true;
                    }
                }
            });
        },
    );
    changed
}

/// An iOS-style switch.
pub fn toggle(ui: &mut Ui, on: &mut bool) -> Response {
    let t = Tokens::get(ui.ctx());
    let size = egui::vec2(34.0, 20.0);
    let (rect, mut resp) = ui.allocate_exact_size(size, Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let k = ui.ctx().animate_bool_responsive(resp.id, *on);
    let bg = Color32::from_rgb(
        egui::lerp(t.raised.r() as f32..=t.success.r() as f32, k) as u8,
        egui::lerp(t.raised.g() as f32..=t.success.g() as f32, k) as u8,
        egui::lerp(t.raised.b() as f32..=t.success.b() as f32, k) as u8,
    );
    ui.painter().rect_filled(rect, CornerRadius::same(10), bg);
    let cx = egui::lerp(rect.left() + 10.0..=rect.right() - 10.0, k);
    ui.painter().circle(
        egui::pos2(cx, rect.center().y),
        8.0,
        Color32::WHITE,
        Stroke::new(0.5, Color32::from_black_alpha(40)),
    );
    resp
}

pub fn toggle_row(ui: &mut Ui, label: &str, on: &mut bool) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(label);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            changed = toggle(ui, on).changed();
        });
    });
    changed
}

/// Paint a transparency checkerboard into `rect`.
pub fn checker(painter: &egui::Painter, rect: egui::Rect, cell: f32) {
    let dark = painter.ctx().global_style().visuals.dark_mode;
    let (a, b) = if dark {
        (Color32::from_gray(54), Color32::from_gray(70))
    } else {
        (Color32::from_gray(255), Color32::from_gray(220))
    };
    painter.rect_filled(rect, 0.0, a);
    let clip = painter.clip_rect().intersect(rect);
    if clip.width() <= 0.0 || clip.height() <= 0.0 {
        return;
    }
    let x0 = ((clip.left() - rect.left()) / cell).floor() as i32;
    let y0 = ((clip.top() - rect.top()) / cell).floor() as i32;
    let x1 = ((clip.right() - rect.left()) / cell).ceil() as i32;
    let y1 = ((clip.bottom() - rect.top()) / cell).ceil() as i32;
    // Batch into one mesh: big canvases have many cells.
    let mut mesh = egui::Mesh::default();
    for y in y0..y1 {
        for x in x0..x1 {
            if (x + y) % 2 == 0 {
                continue;
            }
            let r = egui::Rect::from_min_size(rect.min + egui::vec2(x as f32, y as f32) * cell, egui::vec2(cell, cell))
                .intersect(rect);
            mesh.add_colored_rect(r, b);
        }
    }
    painter.add(egui::Shape::mesh(mesh));
}

/// A colour chip; checkerboard shows through transparency.
pub fn swatch(ui: &mut Ui, c: Rgba, size: Vec2, selected: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let t = Tokens::get(ui.ctx());
    let p = ui.painter_at(rect);
    if c.a() < 255 {
        checker(&p, rect, 4.0);
    }
    p.rect_filled(rect, CornerRadius::same(4), c32(c));
    let stroke = if selected { Stroke::new(2.0, t.accent) } else { Stroke::new(1.0, Color32::from_black_alpha(60)) };
    p.rect_stroke(rect, CornerRadius::same(4), stroke, egui::StrokeKind::Inside);
    resp
}

pub fn label_dim(ui: &mut Ui, text: impl Into<String>) -> Response {
    let t = Tokens::get(ui.ctx());
    let text = text.into();
    ui.label(egui::RichText::new(crate::i18n::tr(&text)).color(t.text_dim).size(12.0))
}
