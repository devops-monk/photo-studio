//! Start screen with the animated logo, quick actions and recent files.

use egui::{Color32, CornerRadius, Sense, Stroke, Ui, Vec2};
use egui_phosphor::regular as ph;

use crate::actions::Action;
use crate::app::App;
use crate::theme::Tokens;

#[derive(Default)]
pub struct LogoAnim {
    tex: Option<egui::TextureHandle>,
    key: Option<(u32, u32)>,
}

/// Draw the iris at `size` points, animating while the preference allows it.
pub fn logo(app: &mut App, ui: &mut Ui, size: f32) {
    let ctx = ui.ctx().clone();
    let px = (size * ctx.pixels_per_point()).round().max(16.0) as u32;
    let animate = app.prefs.show_welcome_animation;
    let time = ctx.input(|i| i.time);
    // 6 s loop, ~40 distinct frames per second of motion.
    let phase = if animate { ((time / 6.0).fract()) as f32 } else { crate::logo::ICON_PHASE };
    let frame = (phase * 240.0) as u32;
    if app.logo.key != Some((px, frame)) {
        let rgba = crate::logo::rgba(px, phase, false);
        let img = egui::ColorImage::from_rgba_unmultiplied([px as usize, px as usize], &rgba);
        match &mut app.logo.tex {
            Some(t) => t.set(img, egui::TextureOptions::LINEAR),
            None => app.logo.tex = Some(ctx.load_texture("logo", img, egui::TextureOptions::LINEAR)),
        }
        app.logo.key = Some((px, frame));
    }
    if let Some(t) = &app.logo.tex {
        ui.add(egui::Image::new((t.id(), Vec2::splat(size))));
    }
    if animate {
        ctx.request_repaint_after(std::time::Duration::from_millis(33));
    }
}

fn action_tile(ui: &mut Ui, icon: &str, title: &str, sub: &str) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(200.0, 74.0), Sense::click());
    let hover = ui.ctx().animate_bool_responsive(resp.id, resp.hovered());
    let bg = if t.dark {
        Color32::from_rgb(44 + (8.0 * hover) as u8, 44 + (8.0 * hover) as u8, 48 + (8.0 * hover) as u8)
    } else {
        Color32::WHITE
    };
    let p = ui.painter();
    p.rect(
        rect,
        CornerRadius::same(14),
        bg,
        Stroke::new(1.0, if hover > 0.5 { t.accent } else { t.separator }),
        egui::StrokeKind::Inside,
    );
    p.text(
        rect.left_center() + Vec2::new(22.0, 0.0),
        egui::Align2::CENTER_CENTER,
        icon,
        egui::FontId::proportional(24.0),
        t.accent,
    );
    p.text(
        rect.left_center() + Vec2::new(44.0, -9.0),
        egui::Align2::LEFT_CENTER,
        title,
        crate::theme::medium(14.0),
        t.text,
    );
    p.text(
        rect.left_center() + Vec2::new(44.0, 10.0),
        egui::Align2::LEFT_CENTER,
        sub,
        egui::FontId::proportional(11.5),
        t.text_dim,
    );
    resp
}

pub fn show(app: &mut App, ui: &mut Ui, actions: &mut Vec<Action>) {
    let t = Tokens::get(ui.ctx());
    let rect = ui.max_rect();
    ui.painter().rect_filled(rect, 0.0, t.canvas);
    // Soft colour wash behind the logo.
    let c = rect.center() - Vec2::new(0.0, 120.0);
    for (i, col) in crate::logo::BLADES.iter().enumerate() {
        let a = i as f32 / 6.0 * std::f32::consts::TAU + ui.input(|i| i.time) as f32 * 0.05;
        let pos = c + Vec2::new(a.cos(), a.sin()) * 90.0;
        ui.painter().circle_filled(
            pos,
            140.0,
            Color32::from_rgba_unmultiplied(col[0], col[1], col[2], if t.dark { 6 } else { 10 }),
        );
    }
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.vertical_centered(|ui| {
            ui.add_space((rect.height() * 0.08).max(24.0));
            logo(app, ui, 150.0);
            ui.add_space(10.0);
            ui.label(egui::RichText::new("Photo Studio").font(crate::theme::bold(34.0)).color(t.text));
            ui.label(
                egui::RichText::new("Layers, masks, adjustments and filters — fast, native and offline.")
                    .size(15.0)
                    .color(t.text_dim),
            );
            ui.add_space(26.0);
            let w = 3.0 * 200.0 + 2.0 * 12.0;
            ui.allocate_ui_with_layout(Vec2::new(w, 80.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                if action_tile(ui, ph::FILE_PLUS, "New Image", "Blank canvas, any size").clicked() {
                    actions.push(Action::New);
                }
                if action_tile(ui, ph::FOLDER_OPEN, "Open…", "PSD, PNG, JPEG, WebP, TIFF…").clicked() {
                    actions.push(Action::Open);
                }
                if action_tile(ui, ph::CLIPBOARD, "From Clipboard", "Paste a screenshot").clicked() {
                    actions.push(Action::NewFromClipboard);
                }
            });
            ui.add_space(26.0);
            if !app.prefs.recent.is_empty() {
                ui.label(egui::RichText::new("Recent").font(crate::theme::medium(13.0)).color(t.text_dim));
                ui.add_space(6.0);
                let recent = app.prefs.recent.clone();
                egui::Frame::new()
                    .fill(t.card)
                    .corner_radius(CornerRadius::same(12))
                    .stroke(Stroke::new(1.0, t.separator))
                    .inner_margin(6)
                    .show(ui, |ui| {
                        ui.set_width(w - 12.0);
                        for p in recent.iter().take(8) {
                            let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                            let dir = p.parent().map(|d| d.display().to_string()).unwrap_or_default();
                            let (r, resp) =
                                ui.allocate_exact_size(Vec2::new(ui.available_width(), 34.0), Sense::click());
                            if resp.hovered() {
                                ui.painter().rect_filled(r, 8.0, t.hover);
                            }
                            ui.painter().text(
                                r.left_center() + Vec2::new(14.0, 0.0),
                                egui::Align2::CENTER_CENTER,
                                ph::IMAGE,
                                egui::FontId::proportional(16.0),
                                t.text_dim,
                            );
                            ui.painter().text(
                                r.left_center() + Vec2::new(32.0, 0.0),
                                egui::Align2::LEFT_CENTER,
                                &name,
                                egui::FontId::proportional(13.0),
                                t.text,
                            );
                            ui.painter().text(
                                r.right_center() - Vec2::new(12.0, 0.0),
                                egui::Align2::RIGHT_CENTER,
                                ellipsize(&dir, 48),
                                egui::FontId::proportional(11.0),
                                t.text_faint,
                            );
                            if resp.clicked() {
                                actions.push(Action::OpenPath(p.clone()));
                            }
                        }
                    });
                ui.add_space(18.0);
            }
            ui.label(
                egui::RichText::new(format!("{}  Drop images anywhere to open them", ph::DOWNLOAD_SIMPLE))
                    .color(t.text_faint)
                    .size(12.0),
            );
        });
    });
}

fn ellipsize(s: &str, max: usize) -> String {
    let n = s.chars().count();
    if n <= max { s.to_string() } else { format!("…{}", s.chars().skip(n - max + 1).collect::<String>()) }
}
