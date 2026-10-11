//! Window layout: menu bar, document tabs, options bar, tool rail, dock, status bar.

use egui::{Color32, CornerRadius, Key, Sense, Stroke, Ui, Vec2};
use egui_phosphor::regular as ph;
use studio_core::blend::BlendMode;
use studio_core::gradient::{Gradient, GradientShape};
use studio_core::paint::ToneRange;
use studio_core::select::SelectOp;
use studio_core::text::{Align, FontFace};

use crate::actions::{Action, shortcuts};
use crate::app::{App, Confirm};
use crate::theme::Tokens;
use crate::tools::{GROUPS, Tool};
use crate::widgets::{c32, icon_button, label_dim, segmented, swatch};

pub fn frame(app: &mut App, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    let t = Tokens::get(&ctx);
    let mut actions: Vec<Action> = Vec::new();
    let now = ctx.input(|i| i.time);

    handle_input(app, &ctx, &mut actions);
    demo_hook(app, &ctx);
    if std::env::var("PHOTO_STUDIO_DEMO").is_ok_and(|v| v.starts_with("icons")) {
        icon_gallery(app, &ctx);
    }
    app.flush_props(now, false);

    egui::Panel::top("menu").frame(egui::Frame::new().fill(t.window).inner_margin(egui::Margin::symmetric(8, 4))).show(
        ui,
        |ui| {
            ui.horizontal(|ui| {
                crate::menus::bar(app, ui, &mut actions);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let dark = ui.visuals().dark_mode;
                    if icon_button(ui, if dark { ph::SUN } else { ph::MOON }, "Toggle light / dark", false, 24.0)
                        .clicked()
                    {
                        actions.push(Action::ToggleTheme);
                    }
                    if icon_button(ui, ph::SIDEBAR_SIMPLE, "Show / hide panels", !app.show_right_panel, 24.0).clicked()
                    {
                        app.show_right_panel = !app.show_right_panel;
                    }
                });
            });
        },
    );

    if !app.docs.is_empty() {
        egui::Panel::top("tabs")
            .frame(egui::Frame::new().fill(t.window).inner_margin(egui::Margin {
                left: 8,
                right: 8,
                top: 0,
                bottom: 4,
            }))
            .show(ui, |ui| tabs(app, ui, &mut actions));
        egui::Panel::top("options")
            .frame(
                egui::Frame::new()
                    .fill(t.panel)
                    .inner_margin(egui::Margin::symmetric(12, 6))
                    .stroke(Stroke::new(1.0, t.separator)),
            )
            .show(ui, |ui| options_bar(app, ui, &mut actions));
    }

    egui::Panel::bottom("status")
        .frame(egui::Frame::new().fill(t.window).inner_margin(egui::Margin::symmetric(12, 4)))
        .show(ui, |ui| status_bar(app, ui));

    egui::Panel::left("tools")
        .resizable(false)
        .exact_size(52.0)
        .frame(
            egui::Frame::new()
                .fill(t.panel)
                .inner_margin(egui::Margin::symmetric(6, 8))
                .stroke(Stroke::new(1.0, t.separator)),
        )
        .show(ui, |ui| toolbar(app, ui, &mut actions));

    if app.show_right_panel {
        egui::Panel::right("dock")
            .resizable(true)
            .default_size(300.0)
            .min_size(250.0)
            .max_size(460.0)
            .frame(egui::Frame::new().fill(t.panel).stroke(Stroke::new(1.0, t.separator)))
            .show(ui, |ui| crate::panels::dock(app, ui, &mut actions));
    }

    egui::CentralPanel::default().frame(egui::Frame::new().fill(t.canvas)).show(ui, |ui| {
        if app.docs.is_empty() {
            crate::welcome::show(app, ui, &mut actions);
        } else {
            crate::canvas::show(app, ui);
        }
    });

    crate::dialogs::show(app, &ctx, &mut actions);
    confirm(app, &ctx);
    toast(app, &ctx);

    for a in actions {
        app.run(&ctx, a);
    }

    // Window title.
    let title = match app.tab() {
        Some(t) => format!("{} — Photo Studio", t.title()),
        None => "Photo Studio".into(),
    };
    if title != app.last_title {
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
        app.last_title = title;
    }

    screenshot_hook(&ctx);

    // Close with unsaved work asks first.
    if ctx.input(|i| i.viewport().close_requested()) && !app.quitting && app.docs.iter().any(|d| d.is_dirty()) {
        ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        app.confirm = Some(Confirm::Quit);
    }
}

fn handle_input(app: &mut App, ctx: &egui::Context, actions: &mut Vec<Action>) {
    // Dropped files.
    let dropped: Vec<std::path::PathBuf> = ctx.input(|i| {
        i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect()
    });
    for p in dropped {
        actions.push(Action::OpenPath(p));
    }
    if app.dialog.is_some() || app.confirm.is_some() {
        return;
    }
    let typing = ctx.egui_wants_keyboard_input();
    // Commit/cancel transient sessions.
    if !typing {
        let (enter, esc) = ctx.input(|i| (i.key_pressed(Key::Enter), i.key_pressed(Key::Escape)));
        if enter {
            if let Some(bp) = app.cv.pen.take() {
                crate::canvas::finish_pen(app, bp);
                return;
            }
            if app.cv.transform.is_some() {
                crate::canvas::commit_transform(app);
                return;
            }
            if app.cv.crop.is_some() {
                crate::canvas::commit_crop(app);
                return;
            }
        }
        if esc {
            if app.cv.transform.is_some() {
                crate::canvas::cancel_transform(app);
                return;
            }
            app.cv.crop = None;
            app.cv.poly.clear();
            app.cv.pen = None;
            if app.tool == Tool::RotateView {
                if let Some(t) = app.tab_mut() {
                    t.rotation = 0.0;
                }
            }
        }
    }
    // Command shortcuts (work even while typing when they use ⌘/Ctrl).
    for (sc, a) in shortcuts() {
        if typing && !sc.modifiers.command {
            continue;
        }
        if typing
            && matches!(
                a,
                Action::Undo
                    | Action::Redo
                    | Action::Copy
                    | Action::Cut
                    | Action::Paste
                    | Action::SelectAll
                    | Action::PathToSelection
            )
        {
            continue;
        }
        if ctx.input_mut(|i| i.consume_shortcut(&sc)) {
            actions.push(a);
        }
    }
    if typing {
        return;
    }
    // Tool keys; Shift cycles inside a group.
    let pressed: Vec<(Key, egui::Modifiers)> = ctx.input(|i| {
        i.events
            .iter()
            .filter_map(|e| match e {
                egui::Event::Key { key, pressed: true, repeat: false, modifiers, .. }
                    if !modifiers.command && !modifiers.alt =>
                {
                    Some((*key, *modifiers))
                }
                _ => None,
            })
            .collect()
    });
    for (key, m) in pressed {
        if let Some(gi) = GROUPS.iter().position(|g| g[0].key() == Some(key)) {
            let g = GROUPS[gi];
            let cur = app.group_tool[gi];
            let next = if m.shift && app.tool.group() == g {
                g[(g.iter().position(|t| *t == cur).unwrap_or(0) + 1) % g.len()]
            } else {
                cur
            };
            app.set_tool(next);
        } else if let Some(t) = GROUPS.iter().flat_map(|g| g.iter()).find(|t| t.key() == Some(key)) {
            // A tool with its own key inside a group (Rotate View in the Hand group).
            app.set_tool(*t);
        }
        if app.tool == Tool::Move && app.docs.get(app.active).is_some() {
            let step = if m.shift { 10 } else { 1 };
            let d = match key {
                Key::ArrowLeft => Some((-step, 0)),
                Key::ArrowRight => Some((step, 0)),
                Key::ArrowUp => Some((0, -step)),
                Key::ArrowDown => Some((0, step)),
                _ => None,
            };
            if let Some((dx, dy)) = d {
                app.edit("Nudge", |doc| doc.nudge_active(dx, dy));
            }
        }
        if key == Key::Q && !m.shift && app.tab().is_some() {
            actions.push(Action::ToggleQuickMask);
        }
        if matches!(key, Key::Delete | Key::Backspace) && app.tab().is_some() {
            actions.push(Action::Clear);
        }
        // Number keys set opacity for painting tools, like other editors.
        if app.tool.uses_brush() {
            let n = match key {
                Key::Num1 => Some(0.1),
                Key::Num2 => Some(0.2),
                Key::Num3 => Some(0.3),
                Key::Num4 => Some(0.4),
                Key::Num5 => Some(0.5),
                Key::Num6 => Some(0.6),
                Key::Num7 => Some(0.7),
                Key::Num8 => Some(0.8),
                Key::Num9 => Some(0.9),
                Key::Num0 => Some(1.0),
                _ => None,
            };
            if let Some(v) = n {
                app.opts.opacity = v;
            }
        }
    }
}

fn tabs(app: &mut App, ui: &mut Ui, actions: &mut Vec<Action>) {
    let t = Tokens::get(ui.ctx());
    let mut close = None;
    egui::ScrollArea::horizontal().scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(
        ui,
        |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                for (i, d) in app.docs.iter().enumerate() {
                    let sel = i == app.active;
                    let text = format!("{}  {:.0}%", d.title(), d.zoom * 100.0);
                    let galley = ui.painter().layout_no_wrap(
                        text,
                        egui::FontId::proportional(12.5),
                        if sel { t.text } else { t.text_dim },
                    );
                    let size = Vec2::new(galley.size().x + 44.0, 28.0);
                    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
                    let bg = if sel {
                        t.panel
                    } else if resp.hovered() {
                        t.hover
                    } else {
                        Color32::TRANSPARENT
                    };
                    ui.painter().rect_filled(rect, CornerRadius { nw: 8, ne: 8, sw: 0, se: 0 }, bg);
                    ui.painter().galley(rect.left_center() + Vec2::new(12.0, -galley.size().y / 2.0), galley, t.text);
                    let x = egui::Rect::from_center_size(rect.right_center() - Vec2::new(16.0, 0.0), Vec2::splat(18.0));
                    let xr = ui.interact(x, ui.id().with(("close-tab", i)), Sense::click());
                    if resp.hovered() || sel || xr.hovered() {
                        if xr.hovered() {
                            ui.painter().rect_filled(x, 4.0, t.hover);
                        }
                        ui.painter().text(
                            x.center(),
                            egui::Align2::CENTER_CENTER,
                            ph::X,
                            egui::FontId::proportional(12.0),
                            t.text_dim,
                        );
                    }
                    if xr.clicked() {
                        close = Some(i);
                    } else if resp.clicked() {
                        app.active = i;
                    }
                    if resp.middle_clicked() {
                        close = Some(i);
                    }
                }
                if icon_button(ui, ph::PLUS, "New (⌘N)", false, 24.0).clicked() {
                    actions.push(Action::New);
                }
            });
        },
    );
    if let Some(i) = close {
        app.request_close(i);
    }
}

/// Draw a short sample stroke with `tip`, for the brush popover.
fn tip_preview(ui: &mut Ui, tip: &studio_core::paint::BrushTip) {
    use studio_core::paint::{StrokeEngine, StrokeKind};
    let (w, h) = (240u32, 56u32);
    let mut img = studio_core::Image::new(w, h);
    let mut t = *tip;
    t.size = t.size.clamp(2.0, 36.0);
    let col = if ui.visuals().dark_mode { studio_core::Rgba::WHITE } else { studio_core::Rgba::BLACK };
    let mut s = StrokeEngine::new(
        &img,
        StrokeKind::Paint { color: col, blend: studio_core::BlendMode::Normal },
        t,
        1.0,
        1.0,
        None,
    );
    for i in 0..=60 {
        let x = 16.0 + i as f32 * (w as f32 - 32.0) / 60.0;
        let y = h as f32 / 2.0 + (i as f32 / 60.0 * std::f32::consts::TAU).sin() * 12.0;
        s.stroke_to(&mut img, x, y, 1.0);
    }
    let tex = ui.ctx().load_texture("tip-preview", crate::doc::to_color_image(&img), egui::TextureOptions::LINEAR);
    ui.add(egui::Image::new((tex.id(), egui::vec2(w as f32, h as f32))));
}

fn tip_controls(ui: &mut Ui, tip: &mut studio_core::paint::BrushTip, show_hardness: bool) {
    let r = ui
        .button(format!("{} {}", ph::PAINT_BRUSH_BROAD, crate::i18n::tr("Brush")))
        .on_hover_text(crate::i18n::tr("Presets and brush settings"));
    egui::Popup::from_toggle_button_response(&r).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(
        |ui| {
            egui::ScrollArea::vertical().max_height(560.0).show(ui, |ui| tip_settings(ui, tip));
        },
    );
    label_dim(ui, "Size");
    ui.add(egui::DragValue::new(&mut tip.size).range(1.0..=2500.0).suffix(" px").speed(0.5));
    if show_hardness {
        label_dim(ui, "Hardness");
        let mut h = tip.hardness * 100.0;
        if ui.add(egui::DragValue::new(&mut h).range(0.0..=100.0).suffix("%")).changed() {
            tip.hardness = h / 100.0;
        }
    }
}

fn tip_settings(ui: &mut Ui, tip: &mut studio_core::paint::BrushTip) {
    ui.set_width(260.0);
    ui.label(egui::RichText::new("Presets").font(crate::theme::medium(12.5)));
    ui.horizontal_wrapped(|ui| {
        for (name, preset) in studio_core::paint::brush_presets() {
            if ui.small_button(name).clicked() {
                let size = tip.size;
                *tip = preset;
                // Keep the user's size unless the preset is about size.
                if !matches!(name, "Airbrush" | "Spray" | "Ink") {
                    tip.size = size;
                }
            }
        }
    });
    ui.separator();
    tip_preview(ui, tip);
    let pct_slider = |ui: &mut Ui, label: &str, v: &mut f32| {
        let mut p = *v * 100.0;
        if crate::widgets::slider_row(ui, label, &mut p, 0.0..=100.0, "%", 0) {
            *v = p / 100.0;
        }
    };
    crate::widgets::slider_row(ui, "Size", &mut tip.size, 1.0..=2500.0, " px", 0);
    pct_slider(ui, "Hardness", &mut tip.hardness);
    let mut sp = tip.spacing * 100.0;
    if crate::widgets::slider_row(ui, "Spacing", &mut sp, 1.0..=300.0, "%", 0) {
        tip.spacing = sp / 100.0;
    }
    crate::widgets::slider_row(ui, "Angle", &mut tip.angle, -180.0..=180.0, "°", 0);
    pct_slider(ui, "Roundness", &mut tip.roundness);
    tip.roundness = tip.roundness.max(0.05);
    let mut sc = tip.scatter * 100.0;
    if crate::widgets::slider_row(ui, "Scatter", &mut sc, 0.0..=500.0, "%", 0) {
        tip.scatter = sc / 100.0;
    }
    pct_slider(ui, "Size jitter", &mut tip.size_jitter);
    pct_slider(ui, "Opacity jitter", &mut tip.opacity_jitter);
    pct_slider(ui, "Smoothing", &mut tip.smoothing);
    ui.label(egui::RichText::new("Color dynamics").font(crate::theme::medium(12.0)));
    pct_slider(ui, "Hue jitter", &mut tip.hue_jitter);
    pct_slider(ui, "Fg/Bg jitter", &mut tip.fg_bg_jitter);
    ui.label(egui::RichText::new("Texture").font(crate::theme::medium(12.0)));
    ui.horizontal_wrapped(|ui| {
        for t in studio_core::paint::BrushTexture::ALL {
            ui.selectable_value(&mut tip.texture, t, t.name());
        }
    });
    if tip.texture != studio_core::paint::BrushTexture::None {
        let mut sc = tip.texture_scale * 100.0;
        if crate::widgets::slider_row(ui, "Scale", &mut sc, 25.0..=400.0, "%", 0) {
            tip.texture_scale = sc / 100.0;
        }
        pct_slider(ui, "Depth", &mut tip.texture_depth);
    }
    ui.label(egui::RichText::new("Dual brush").font(crate::theme::medium(12.0)));
    let mut on = tip.dual_size > 0.0;
    if ui.checkbox(&mut on, crate::i18n::tr("Enabled")).changed() {
        tip.dual_size = if on { (tip.size / 6.0).max(2.0) } else { 0.0 };
    }
    if tip.dual_size > 0.0 {
        crate::widgets::slider_row(ui, "Tip size", &mut tip.dual_size, 1.0..=200.0, " px", 0);
        let mut n = tip.dual_count as f32;
        if crate::widgets::slider_row(ui, "Count", &mut n, 1.0..=32.0, "", 0) {
            tip.dual_count = n as u32;
        }
        let mut sc = tip.dual_scatter * 100.0;
        if crate::widgets::slider_row(ui, "Dual scatter", &mut sc, 0.0..=150.0, "%", 0) {
            tip.dual_scatter = sc / 100.0;
        }
    }
}

fn pct(ui: &mut Ui, label: &str, v: &mut f32) {
    label_dim(ui, label);
    let mut p = *v * 100.0;
    ui.spacing_mut().slider_width = 90.0;
    if ui.add(egui::Slider::new(&mut p, 1.0..=100.0).suffix("%").max_decimals(0)).changed() {
        *v = p / 100.0;
    }
}

fn blend_combo(ui: &mut Ui, id: &str, m: &mut BlendMode) {
    egui::ComboBox::from_id_salt(id).width(110.0).selected_text(crate::i18n::tr(m.label())).show_ui(ui, |ui| {
        for b in BlendMode::all() {
            ui.selectable_value(m, b, crate::i18n::tr(b.label()));
        }
    });
}

fn select_ops(ui: &mut Ui, op: &mut SelectOp) {
    let icons = [
        (SelectOp::Replace, ph::SQUARE),
        (SelectOp::Add, ph::PLUS_SQUARE),
        (SelectOp::Subtract, ph::MINUS_SQUARE),
        (SelectOp::Intersect, ph::INTERSECT_SQUARE),
    ];
    for (o, ic) in icons {
        if icon_button(ui, ic, o.label(), *op == o, 24.0).clicked() {
            *op = o;
        }
    }
}

fn options_bar(app: &mut App, ui: &mut Ui, actions: &mut Vec<Action>) {
    let t = Tokens::get(ui.ctx());
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        ui.label(egui::RichText::new(app.tool.icon()).size(16.0).color(t.accent));
        ui.label(egui::RichText::new(crate::i18n::tr(app.tool.label())).font(crate::theme::medium(13.0)));
        ui.add(egui::Separator::default().vertical());
        let o = &mut app.opts;
        match app.tool {
            Tool::Brush | Tool::Pencil => {
                tip_controls(ui, &mut o.tip, app.tool == Tool::Brush);
                label_dim(ui, "Mode");
                blend_combo(ui, "brush-mode", &mut o.brush_blend);
                pct(ui, "Opacity", &mut o.opacity);
                if app.tool == Tool::Brush {
                    pct(ui, "Flow", &mut o.flow);
                }
            }
            Tool::Eraser => {
                tip_controls(ui, &mut o.eraser_tip, true);
                pct(ui, "Opacity", &mut o.opacity);
                pct(ui, "Flow", &mut o.flow);
            }
            Tool::Patch | Tool::ContentMove => {
                label_dim(
                    ui,
                    if app.tool == Tool::Patch {
                        "Draw around the area to fix, then drag the selection onto good texture"
                    } else {
                        "Draw around an object, then drag it to its new place"
                    },
                );
            }
            Tool::Remove => {
                label_dim(ui, "Size");
                ui.add(egui::DragValue::new(&mut o.tip.size).range(2.0..=800.0).suffix(" px"));
                label_dim(ui, "Paint over what should disappear · Edit › Content-Aware Fill works on selections");
            }
            Tool::CloneStamp | Tool::Heal | Tool::SpotHeal => {
                tip_controls(ui, &mut o.tip, true);
                if app.tool != Tool::SpotHeal {
                    pct(ui, "Opacity", &mut o.opacity);
                    ui.checkbox(&mut o.sample_merged, crate::i18n::tr("Sample all layers"));
                    if app.cv.clone_source.is_none() {
                        label_dim(ui, "Alt-click to set the source");
                    }
                }
            }
            Tool::MixerBrush => {
                tip_controls(ui, &mut o.tip, true);
                pct(ui, "Wet", &mut o.strength);
                pct(ui, "Load", &mut o.flow);
                label_dim(ui, "Paint from the foreground color mixes with the canvas");
            }
            Tool::Blur | Tool::Sharpen | Tool::Smudge | Tool::Liquify => {
                tip_controls(ui, &mut o.tip, true);
                pct(ui, "Strength", &mut o.strength);
            }
            Tool::Dodge | Tool::Burn => {
                tip_controls(ui, &mut o.tip, true);
                label_dim(ui, "Range");
                segmented(
                    ui,
                    &mut o.tone_range,
                    &[
                        (ToneRange::Shadows, "Shadows"),
                        (ToneRange::Midtones, "Midtones"),
                        (ToneRange::Highlights, "Highlights"),
                    ],
                );
                pct(ui, "Exposure", &mut o.exposure);
            }
            Tool::Sponge => {
                tip_controls(ui, &mut o.tip, true);
                segmented(ui, &mut o.sponge_saturate, &[(false, "Desaturate"), (true, "Saturate")]);
                pct(ui, "Flow", &mut o.flow);
            }
            Tool::RectMarquee | Tool::EllipseMarquee | Tool::Lasso | Tool::PolyLasso | Tool::MagneticLasso => {
                select_ops(ui, &mut o.select_op);
                label_dim(ui, "Feather");
                ui.add(egui::DragValue::new(&mut o.feather).range(0.0..=250.0).suffix(" px"));
                if ui.button(crate::i18n::tr("Select Subject")).clicked() {
                    actions.push(Action::SelectSubject);
                }
            }
            Tool::ObjectSelect => {
                select_ops(ui, &mut o.select_op);
                label_dim(ui, "Drag a box around an object");
                if ui.button(crate::i18n::tr("Select Subject")).clicked() {
                    actions.push(Action::SelectSubject);
                }
            }
            Tool::QuickSelect => {
                label_dim(ui, "Size");
                ui.add(egui::DragValue::new(&mut o.tip.size).range(1.0..=500.0).suffix(" px"));
                label_dim(ui, "Alt subtracts");
                if ui.button(crate::i18n::tr("Select Subject")).clicked() {
                    actions.push(Action::SelectSubject);
                }
            }
            Tool::MagicWand | Tool::Bucket => {
                if app.tool == Tool::MagicWand {
                    select_ops(ui, &mut o.select_op);
                }
                label_dim(ui, "Tolerance");
                ui.add(egui::DragValue::new(&mut o.tolerance).range(0..=255));
                ui.checkbox(&mut o.anti_alias, crate::i18n::tr("Anti-alias"));
                ui.checkbox(&mut o.contiguous, crate::i18n::tr("Contiguous"));
                ui.checkbox(&mut o.sample_merged, crate::i18n::tr("Sample all layers"));
                if app.tool == Tool::Bucket {
                    pct(ui, "Opacity", &mut o.opacity);
                } else if ui.button(crate::i18n::tr("Select Subject")).clicked() {
                    actions.push(Action::SelectSubject);
                }
            }
            Tool::Eyedropper => {
                ui.checkbox(&mut o.sample_merged, crate::i18n::tr("Sample all layers"));
                label_dim(ui, "Alt-click picks the background color");
            }
            Tool::Gradient => {
                let presets = Gradient::presets(app.fg, app.bg);
                let name = presets.get(o.gradient_preset).map_or("", |p| p.0);
                egui::ComboBox::from_id_salt("grad-preset").width(170.0).selected_text(name).show_ui(ui, |ui| {
                    for (i, (n, _)) in presets.iter().enumerate() {
                        ui.selectable_value(&mut o.gradient_preset, i, *n);
                    }
                });
                segmented(ui, &mut o.gradient_shape, &GradientShape::ALL.map(|s| (s, s.label())));
                ui.checkbox(&mut o.gradient_reverse, crate::i18n::tr("Reverse"));
                label_dim(ui, "Mode");
                blend_combo(ui, "grad-mode", &mut o.gradient_blend);
                pct(ui, "Opacity", &mut o.gradient_opacity);
            }
            Tool::Crop => {
                if app.cv.crop.is_some() {
                    if crate::widgets::primary_button(ui, &format!("{} Crop", ph::CHECK)).clicked() {
                        crate::canvas::commit_crop(app);
                    }
                    if ui.button(crate::i18n::tr("Cancel")).clicked() {
                        app.cv.crop = None;
                    }
                } else {
                    label_dim(ui, "Drag on the image to set the crop");
                    if ui.button(crate::i18n::tr("Crop to Selection")).clicked() {
                        actions.push(Action::CropToSelection);
                    }
                }
            }
            Tool::Text => {
                egui::ComboBox::from_id_salt("font-face").width(150.0).selected_text(o.face.label()).show_ui(
                    ui,
                    |ui| {
                        for f in FontFace::ALL {
                            ui.selectable_value(&mut o.face, f, f.label());
                        }
                    },
                );
                label_dim(ui, "Size");
                ui.add(egui::DragValue::new(&mut o.font_size).range(4.0..=1000.0).suffix(" px"));
                segmented(
                    ui,
                    &mut o.align,
                    &[
                        (Align::Left, ph::TEXT_ALIGN_LEFT),
                        (Align::Center, ph::TEXT_ALIGN_CENTER),
                        (Align::Right, ph::TEXT_ALIGN_RIGHT),
                    ],
                );
                if icon_button(ui, ph::ARROWS_DOWN_UP, "Vertical type", o.vertical_text, 26.0).clicked() {
                    o.vertical_text = !o.vertical_text;
                }
                label_dim(ui, "Uses the foreground color");
            }
            Tool::Rectangle
            | Tool::RoundedRect
            | Tool::Ellipse
            | Tool::Triangle
            | Tool::Polygon
            | Tool::Star
            | Tool::Line
            | Tool::Heart
            | Tool::Arrow
            | Tool::SpeechBubble
            | Tool::Checkmark => {
                if app.tool != Tool::Line {
                    ui.checkbox(&mut o.fill_enabled, crate::i18n::tr("Fill (foreground)"));
                    ui.checkbox(&mut o.stroke_enabled, crate::i18n::tr("Stroke (background)"));
                }
                label_dim(ui, "Width");
                ui.add(egui::DragValue::new(&mut o.stroke_width).range(0.5..=200.0).suffix(" px"));
                ui.checkbox(&mut o.dashed, crate::i18n::tr("Dashed"));
                match app.tool {
                    Tool::RoundedRect => {
                        label_dim(ui, "Radius");
                        ui.add(egui::DragValue::new(&mut o.corner_radius).range(0.0..=1000.0).suffix(" px"));
                    }
                    Tool::Polygon | Tool::Star => {
                        label_dim(ui, if app.tool == Tool::Star { "Points" } else { "Sides" });
                        ui.add(egui::DragValue::new(&mut o.sides).range(3..=64));
                        if app.tool == Tool::Star {
                            pct(ui, "Inner", &mut o.star_inner);
                        }
                    }
                    _ => {}
                }
            }
            Tool::Move => {
                if app.cv.transform.is_some() {
                    label_dim(
                        ui,
                        "Drag handles to scale (Shift frees the ratio, Alt from centre) · drag outside to rotate",
                    );
                    if crate::widgets::primary_button(ui, &format!("{} Apply", ph::CHECK)).clicked() {
                        crate::canvas::commit_transform(app);
                    }
                    if ui.button(crate::i18n::tr("Cancel")).clicked() {
                        crate::canvas::cancel_transform(app);
                    }
                } else {
                    if ui.button(format!("{} Free Transform", ph::BOUNDING_BOX)).clicked() {
                        actions.push(Action::FreeTransform);
                    }
                    if icon_button(ui, ph::FLIP_HORIZONTAL, "Flip horizontal", false, 26.0).clicked() {
                        actions.push(Action::FlipLayer(true));
                    }
                    if icon_button(ui, ph::FLIP_VERTICAL, "Flip vertical", false, 26.0).clicked() {
                        actions.push(Action::FlipLayer(false));
                    }
                    if icon_button(ui, ph::ARROW_CLOCKWISE, "Rotate 90° clockwise", false, 26.0).clicked() {
                        actions.push(Action::RotateLayer(studio_core::transform::Rotation::Cw90));
                    }
                    ui.add(egui::Separator::default().vertical());
                    let target =
                        if app.tab().is_some_and(|t| t.doc.selection.is_some()) { "selection" } else { "canvas" };
                    for (icon, h, v, what) in [
                        (ph::ALIGN_LEFT, Some(-1), None, "left edges"),
                        (ph::ALIGN_CENTER_HORIZONTAL, Some(0), None, "horizontal centers"),
                        (ph::ALIGN_RIGHT, Some(1), None, "right edges"),
                        (ph::ALIGN_TOP, None, Some(-1), "top edges"),
                        (ph::ALIGN_CENTER_VERTICAL, None, Some(0), "vertical centers"),
                        (ph::ALIGN_BOTTOM, None, Some(1), "bottom edges"),
                    ] {
                        if icon_button(ui, icon, &format!("Align {what} to the {target}"), false, 26.0).clicked() {
                            actions.push(Action::Align(h, v));
                        }
                    }
                    if icon_button(ui, ph::COLUMNS, "Distribute horizontally (3+ layers)", false, 26.0).clicked() {
                        actions.push(Action::Distribute(true));
                    }
                    if icon_button(ui, ph::ROWS, "Distribute vertically (3+ layers)", false, 26.0).clicked() {
                        actions.push(Action::Distribute(false));
                    }
                }
            }
            Tool::Pen | Tool::DirectSelect | Tool::PathSelect => {
                if app.cv.pen.is_some()
                    && crate::widgets::primary_button(ui, &format!("{} Finish Path", ph::CHECK)).clicked()
                {
                    if let Some(bp) = app.cv.pen.take() {
                        crate::canvas::finish_pen(app, bp);
                    }
                }
                if ui.button(crate::i18n::tr("Make Selection")).on_hover_text("⌘↩").clicked() {
                    actions.push(Action::PathToSelection);
                }
                if ui.button(crate::i18n::tr("Fill")).clicked() {
                    actions.push(Action::FillPath);
                }
                if ui.button(crate::i18n::tr("Stroke")).clicked() {
                    actions.push(Action::StrokePath);
                }
                if ui.button(crate::i18n::tr("Shape Layer")).clicked() {
                    actions.push(Action::PathToShape);
                }
            }
            Tool::PatternStamp => {
                tip_controls(ui, &mut o.tip, true);
                let mut names: Vec<&str> =
                    studio_core::paint::builtin_patterns(app.fg, app.bg).iter().map(|p| p.0).collect();
                if app.custom_pattern.is_some() {
                    names.push("Defined pattern");
                }
                o.pattern = o.pattern.min(names.len() - 1);
                egui::ComboBox::from_id_salt("pattern").width(130.0).selected_text(names[o.pattern]).show_ui(
                    ui,
                    |ui| {
                        for (i, n) in names.iter().enumerate() {
                            ui.selectable_value(&mut o.pattern, i, *n);
                        }
                    },
                );
                pct(ui, "Opacity", &mut o.opacity);
                label_dim(ui, "Edit › Define Pattern adds your own");
            }
            Tool::HistoryBrush => {
                tip_controls(ui, &mut o.tip, true);
                pct(ui, "Opacity", &mut o.opacity);
                label_dim(ui, "Source: the image as it was opened");
            }
            Tool::BackgroundEraser => {
                tip_controls(ui, &mut o.eraser_tip, true);
                label_dim(ui, "Tolerance");
                ui.add(egui::DragValue::new(&mut o.tolerance).range(0..=255));
            }
            Tool::MagicEraser => {
                label_dim(ui, "Tolerance");
                ui.add(egui::DragValue::new(&mut o.tolerance).range(0..=255));
                ui.checkbox(&mut o.anti_alias, crate::i18n::tr("Anti-alias"));
                ui.checkbox(&mut o.contiguous, crate::i18n::tr("Contiguous"));
                ui.checkbox(&mut o.sample_merged, crate::i18n::tr("Sample all layers"));
            }
            Tool::RedEye => {
                pct(ui, "Darken", &mut o.red_eye_darken);
                label_dim(ui, "Click an eye, or drag a box around it");
            }
            Tool::Note | Tool::Count | Tool::Slice => {
                if let Some(tab) = app.tab() {
                    let a = &tab.doc.annotations;
                    label_dim(
                        ui,
                        format!("{} notes · {} counted · {} slices", a.notes.len(), a.counts.len(), a.slices.len()),
                    );
                }
                if app.tool == Tool::Slice && ui.button(crate::i18n::tr("Export Slices…")).clicked() {
                    actions.push(Action::ExportSlices);
                }
                if ui.button(crate::i18n::tr("Clear All")).clicked() {
                    actions.push(Action::ClearAnnotations);
                }
            }
            Tool::Ruler => match app.cv.ruler {
                Some((a, b)) => {
                    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
                    ui.label(format!(
                        "W {:.1}  H {:.1}  ·  L {:.1} px  ·  A {:.1}°",
                        dx.abs(),
                        dy.abs(),
                        dx.hypot(dy),
                        -dy.atan2(dx).to_degrees()
                    ));
                    if ui
                        .button(crate::i18n::tr("Straighten"))
                        .on_hover_text(crate::i18n::tr("Rotate the canvas so the line is level"))
                        .clicked()
                    {
                        actions.push(Action::Straighten);
                    }
                    if ui.button(crate::i18n::tr("Clear")).clicked() {
                        app.cv.ruler = None;
                    }
                }
                None => {
                    label_dim(ui, "Drag along something that should be level");
                }
            },
            Tool::RotateView => {
                if let Some(tab) = app.tab_mut() {
                    label_dim(ui, "Angle");
                    ui.add(egui::DragValue::new(&mut tab.rotation).range(-180.0..=180.0).suffix("°").speed(1.0));
                    if ui.button(crate::i18n::tr("Reset View")).clicked() {
                        tab.rotation = 0.0;
                    }
                }
            }
            Tool::Hand | Tool::Zoom => {
                if ui.button(crate::i18n::tr("Fit on Screen")).clicked() {
                    actions.push(Action::FitOnScreen);
                }
                if ui.button("100%").clicked() {
                    actions.push(Action::ActualPixels);
                }
            }
        }
        if app.quick_mask {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(format!(
                        "{} Quick Mask — paint black to deselect, white to select · Q to exit",
                        ph::SELECTION
                    ))
                    .color(t.danger),
                );
            });
        } else if app.edit_mask {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(egui::RichText::new(format!("{} Editing layer mask", ph::CIRCLE_HALF_TILT)).color(t.accent));
            });
        }
    });
}

fn toolbar(app: &mut App, ui: &mut Ui, actions: &mut Vec<Action>) {
    let t = Tokens::get(ui.ctx());
    egui::ScrollArea::vertical().scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(
        ui,
        |ui| {
            ui.vertical_centered(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                let separators_after = [0usize, 4, 5, 10, 12, 14];
                for (gi, g) in GROUPS.iter().enumerate() {
                    let cur = app.group_tool[gi];
                    let selected = g.contains(&app.tool);
                    let key = g[0].key().map(|k| format!(" ({})", k.name())).unwrap_or_default();
                    let tip = if g.len() > 1 {
                        format!("{}{}\nRight-click for more", crate::i18n::tr(cur.label()), key)
                    } else {
                        format!("{}{}", crate::i18n::tr(cur.label()), key)
                    };
                    let r = icon_button(ui, cur.icon(), &tip, selected, 32.0);
                    if g.len() > 1 {
                        let tri = r.rect.right_bottom() - Vec2::new(5.0, 5.0);
                        ui.painter().add(egui::Shape::convex_polygon(
                            vec![tri, tri + Vec2::new(-4.0, 0.0), tri + Vec2::new(0.0, -4.0)],
                            if selected { Color32::WHITE } else { t.text_faint },
                            Stroke::NONE,
                        ));
                    }
                    if r.clicked() {
                        app.set_tool(cur);
                    }
                    if g.len() > 1 {
                        egui::Popup::context_menu(&r).show(|ui| {
                            for tool in g.iter() {
                                let label = format!("{}  {}", tool.icon(), crate::i18n::tr(tool.label()));
                                if ui.selectable_label(app.tool == *tool, label).clicked() {
                                    app.set_tool(*tool);
                                    ui.close();
                                }
                            }
                        });
                    }
                    if separators_after.contains(&gi) {
                        ui.add_space(3.0);
                        let rr = ui.available_rect_before_wrap();
                        ui.painter().hline(
                            rr.left() + 8.0..=rr.right() - 8.0,
                            ui.cursor().top(),
                            Stroke::new(1.0, t.separator),
                        );
                        ui.add_space(4.0);
                    }
                }
                ui.add_space(10.0);
                // Colour chips.
                let (rect, _) = ui.allocate_exact_size(Vec2::new(40.0, 40.0), Sense::hover());
                let bg_r = egui::Rect::from_min_size(rect.min + Vec2::new(14.0, 14.0), Vec2::splat(24.0));
                let fg_r = egui::Rect::from_min_size(rect.min + Vec2::new(2.0, 2.0), Vec2::splat(24.0));
                let p = ui.painter();
                p.rect(bg_r, 6.0, c32(app.bg), Stroke::new(1.5, t.border), egui::StrokeKind::Inside);
                p.rect(fg_r, 6.0, c32(app.fg), Stroke::new(1.5, Color32::WHITE), egui::StrokeKind::Outside);
                let fr = ui
                    .interact(fg_r, ui.id().with("fg-chip"), Sense::click())
                    .on_hover_text(crate::i18n::tr("Foreground color"));
                let br = ui
                    .interact(
                        bg_r.intersect(egui::Rect::from_min_max(fg_r.right_bottom() - Vec2::splat(12.0), bg_r.max)),
                        ui.id().with("bg-chip"),
                        Sense::click(),
                    )
                    .on_hover_text(crate::i18n::tr("Background color"));
                egui::Popup::from_toggle_button_response(&fr).show(|ui| {
                    ui.set_width(240.0);
                    let mut c = app.fg;
                    if crate::panels::color_picker(ui, "fg-pop", &mut c) {
                        app.fg = c;
                    }
                });
                egui::Popup::from_toggle_button_response(&br).show(|ui| {
                    ui.set_width(240.0);
                    let mut c = app.bg;
                    if crate::panels::color_picker(ui, "bg-pop", &mut c) {
                        app.bg = c;
                    }
                });
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    if icon_button(ui, ph::ARROWS_LEFT_RIGHT, "Swap (X)", false, 20.0).clicked() {
                        actions.push(Action::SwapColors);
                    }
                    if icon_button(ui, ph::CIRCLE_HALF, "Default (D)", false, 20.0).clicked() {
                        actions.push(Action::DefaultColors);
                    }
                });
                let _ = swatch;
            });
        },
    );
}

fn status_bar(app: &mut App, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 14.0;
        let dim = |s: String| egui::RichText::new(s).size(11.5).color(t.text_dim);
        if let Some(tab) = app.tab() {
            ui.label(dim(format!("{:.0}%", tab.zoom * 100.0)));
            ui.label(dim(format!("{} × {} px", tab.doc.width, tab.doc.height)));
            let n = tab.doc.layers.len();
            let word = crate::i18n::tr(if n == 1 { "count|layer" } else { "count|layers" });
            ui.label(dim(format!("{n} {word}")));
            if let Some((x, y)) = app.cv.hover {
                if x >= 0.0 && y >= 0.0 && x < tab.doc.width as f32 && y < tab.doc.height as f32 {
                    let c = tab.composite.get(x as u32, y as u32);
                    ui.label(dim(format!("X {:.0}  Y {:.0}", x.floor(), y.floor())));
                    let (r, _) = ui.allocate_exact_size(Vec2::new(12.0, 12.0), Sense::hover());
                    ui.painter().rect(r, 3.0, c32(c), Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
                    ui.label(dim(format!("#{}  α {}", c.hex(), c.a())));
                }
            }
            if let Some(b) = tab.doc.selection.as_ref().and_then(|m| m.bounds()) {
                ui.label(dim(format!("{} {} × {}", crate::i18n::tr("status|Selection"), b.w, b.h)));
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(dim(crate::i18n::tr(app.tool.hint()).into_owned()));
        });
    });
}

fn confirm(app: &mut App, ctx: &egui::Context) {
    let Some(c) = &app.confirm else { return };
    let (title, idx) = match c {
        Confirm::Close(i) => (
            format!("Save changes to “{}” before closing?", app.docs.get(*i).map(|d| d.name.as_str()).unwrap_or("")),
            Some(*i),
        ),
        Confirm::Quit => ("You have unsaved changes. Quit anyway?".to_string(), None),
    };
    let mut choice = 0;
    egui::Modal::new(egui::Id::new("confirm")).show(ctx, |ui| {
        ui.set_width(360.0);
        ui.label(egui::RichText::new(ph::WARNING_CIRCLE).size(28.0).color(Tokens::get(ctx).accent));
        ui.label(egui::RichText::new(title).font(crate::theme::medium(15.0)));
        crate::widgets::label_dim(ui, "Your changes will be lost if you don't save them.");
        ui.add_space(10.0);
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), 30.0),
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| {
                if idx.is_some() {
                    if crate::widgets::primary_button(ui, "Save").clicked() {
                        choice = 1;
                    }
                } else if crate::widgets::primary_button(ui, "Quit").clicked() {
                    choice = 2;
                }
                if crate::widgets::secondary_button(ui, "Cancel").clicked() {
                    choice = 3;
                }
                if idx.is_some() && crate::widgets::secondary_button(ui, "Don't Save").clicked() {
                    choice = 2;
                }
            },
        );
        if ui.input(|i| i.key_pressed(Key::Escape)) {
            choice = 3;
        }
    });
    match (choice, idx) {
        (1, Some(i)) => {
            app.confirm = None;
            if app.save(ctx, i, false) {
                app.close_doc(i);
            }
        }
        (2, Some(i)) => {
            app.confirm = None;
            app.close_doc(i);
        }
        (2, None) => {
            app.confirm = None;
            app.quitting = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        (3, _) => app.confirm = None,
        _ => {}
    }
}

fn toast(app: &mut App, ctx: &egui::Context) {
    let Some((msg, at)) = app.toast.clone() else {
        return;
    };
    let now = ctx.input(|i| i.time);
    let at = if at == 0.0 { now } else { at };
    app.toast = Some((msg.clone(), at));
    let age = now - at;
    if age > 3.0 {
        app.toast = None;
        return;
    }
    let alpha = (1.0 - ((age - 2.5) / 0.5).clamp(0.0, 1.0)) as f32;
    let t = Tokens::get(ctx);
    let screen = ctx.content_rect();
    egui::Area::new(egui::Id::new("toast"))
        .fixed_pos(egui::pos2(screen.center().x, screen.bottom() - 70.0))
        .pivot(egui::Align2::CENTER_BOTTOM)
        .order(egui::Order::Tooltip)
        .interactable(false)
        .show(ctx, |ui| {
            ui.set_opacity(alpha);
            egui::Frame::new()
                .fill(if t.dark { Color32::from_rgb(58, 58, 62) } else { Color32::from_rgb(40, 40, 44) })
                .corner_radius(CornerRadius::same(20))
                .inner_margin(egui::Margin::symmetric(16, 9))
                .shadow(egui::Shadow { offset: [0, 4], blur: 16, spread: 0, color: Color32::from_black_alpha(80) })
                .show(ui, |ui| {
                    ui.label(egui::RichText::new(msg.as_str()).color(Color32::WHITE).size(13.0));
                });
        });
    ctx.request_repaint_after(std::time::Duration::from_millis(50));
}

/// `PHOTO_STUDIO_SCREENSHOT=out.png` renders a few frames, saves the window and quits
/// (used to check the UI in CI and during development).
fn screenshot_hook(ctx: &egui::Context) {
    let Ok(path) = std::env::var("PHOTO_STUDIO_SCREENSHOT") else {
        return;
    };
    let frame = ctx.cumulative_frame_nr();
    if std::env::var_os("PHOTO_STUDIO_DEBUG").is_some() {
        eprintln!("frame {frame}");
    }
    if frame >= 40 && frame.is_multiple_of(20) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
    }
    let shot = ctx.input(|i| {
        i.events.iter().find_map(|e| match e {
            egui::Event::Screenshot { image, .. } => Some(image.clone()),
            _ => None,
        })
    });
    if let Some(img) = shot {
        let [w, h] = img.size;
        let bytes: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_srgba_unmultiplied()).collect();
        let out = studio_core::Image::from_raw(w as u32, h as u32, bytes);
        if let Ok(png) = studio_core::io::encode(&out, "png", &Default::default()) {
            let _ = std::fs::write(&path, png);
        }
        std::process::exit(0);
    }
    ctx.request_repaint();
}

/// `PHOTO_STUDIO_DEMO=1` builds a small composition on the open image (UI checks only).
fn demo_hook(app: &mut App, ctx: &egui::Context) {
    if ctx.cumulative_frame_nr() == 2 {
        if let Ok(t) = std::env::var("PHOTO_STUDIO_THEME") {
            app.prefs.theme = if t == "light" { crate::theme::ThemeMode::Light } else { crate::theme::ThemeMode::Dark };
            crate::theme::apply(ctx, app.prefs.theme);
        }
    }
    let Ok(variant) = std::env::var("PHOTO_STUDIO_DEMO") else { return };
    if ctx.cumulative_frame_nr() != 5 || app.docs.is_empty() {
        return;
    }
    build_demo(app, ctx, &variant);
}

/// A small composition on the open image, used for screenshots and UI checks.
pub fn build_demo(app: &mut App, ctx: &egui::Context, variant: &str) {
    use studio_core::adjust::{Adjustment, Curve, LevelsChannel};
    use studio_core::layer::LayerKind;
    let curves = Adjustment::Curves {
        master: Curve { points: vec![(0, 0), (70, 50), (190, 215), (255, 255)] },
        red: Curve::default(),
        green: Curve::default(),
        blue: Curve::default(),
    };
    let _ = LevelsChannel::default();
    app.run(ctx, Action::NewAdjustmentLayer(curves));
    let tab = &mut app.docs[app.active];
    let (w, h) = (tab.doc.width as f32, tab.doc.height as f32);
    let title = std::env::var("PHOTO_STUDIO_TITLE").unwrap_or_else(|_| "PHOTO STUDIO".into());
    let mut tl =
        studio_core::text::TextLayer::new(title.clone(), w * 0.52, h * 0.08, w * 0.05, studio_core::Rgba::WHITE);
    tl.face = studio_core::text::FontFace::SansBold;
    tl.tracking = 80.0;
    tab.doc.add_layer_of_kind(&title, LayerKind::Text(tl));
    let i = tab.doc.active;
    tab.doc.layers[i].styles.drop_shadow.enabled = true;
    tab.doc.layers[i].styles.stroke.enabled = true;
    tab.doc.layers[i].styles.stroke.size = 2.0;
    if variant == "styles" {
        let st = &mut tab.doc.layers[i].styles;
        st.stroke.enabled = false;
        st.bevel.enabled = true;
        st.bevel.size = 4.0;
        st.gradient_overlay.enabled = true;
        st.gradient_overlay.gradient =
            studio_core::gradient::Gradient::presets(studio_core::Rgba::BLACK, studio_core::Rgba::WHITE)[4].1.clone();
    }
    let shape = studio_core::shape::ShapeLayer {
        kind: studio_core::shape::ShapeKind::RoundedRect { radius: 18.0 },
        from: (w * 0.5, h * 0.05),
        to: (w * 0.97, h * 0.17),
        fill: Some(studio_core::Rgba::rgba(10, 30, 60, 170)),
        stroke: None,
        stroke_width: 2.0,
        dashed: false,
        path: None,
    };
    tab.doc.active = 1;
    tab.doc.add_layer_of_kind("Badge", LayerKind::Shape(shape));
    // Put the title and badge in a group: [bg, curves, badge, title] → group the title, pull the badge in.
    tab.doc.active = 3;
    tab.doc.group_active("Title card");
    let gid = tab.doc.layers[4].id;
    tab.doc.layers[2].parent = Some(gid);
    tab.doc.active = 3;
    let sel =
        studio_core::select::ellipse_mask(tab.doc.width, tab.doc.height, (w * 0.55, h * 0.45), (w * 0.85, h * 0.85));
    tab.doc.set_selection(sel);
    tab.commit("Demo");
    if variant == "filter" {
        tab.doc.active = 0;
        app.run(ctx, Action::Filter(studio_core::filters::Filter::Twirl { angle: 220.0 }));
    }
}

/// Every icon the UI uses, plus each tool's options bar (`PHOTO_STUDIO_DEMO=icons`).
fn icon_gallery(app: &mut App, ctx: &egui::Context) {
    let saved = app.tool;
    let tail = std::env::var("PHOTO_STUDIO_DEMO").as_deref() == Ok("icons2");
    egui::Window::new("Icon check").default_pos([60.0, 60.0]).default_width(1300.0).show(ctx, |ui| {
        if tail {
            let mut sink = Vec::new();
            for t in GROUPS.iter().flat_map(|g| g.iter()).skip_while(|t| **t != Tool::Blur) {
                app.tool = *t;
                ui.push_id(t.label(), |ui| options_bar(app, ui, &mut sink));
            }
            return;
        }
        ui.label(egui::RichText::new("Tools").strong());
        ui.horizontal_wrapped(|ui| {
            for g in GROUPS {
                for t in g.iter() {
                    ui.label(egui::RichText::new(format!("{} {}", t.icon(), t.label())).size(13.0));
                }
            }
        });
        ui.separator();
        ui.label(egui::RichText::new("Adjustments").strong());
        ui.horizontal_wrapped(|ui| {
            for a in studio_core::Adjustment::catalog() {
                ui.label(format!("{} {}", crate::panels::adjustment_icon(&a), a.label()));
            }
        });
        ui.separator();
        ui.label(egui::RichText::new("Panels & misc").strong());
        ui.horizontal_wrapped(|ui| {
            for (i, n) in [
                (ph::EYE, "eye"),
                (ph::EYE_SLASH, "hidden"),
                (ph::LOCK, "lock"),
                (ph::LOCK_OPEN, "unlock"),
                (ph::ARROW_ELBOW_DOWN_RIGHT, "clip"),
                (ph::LINK, "link"),
                (ph::CIRCLE_HALF_TILT, "mask"),
                (ph::PLUS, "new"),
                (ph::COPY, "duplicate"),
                (ph::TRASH, "delete"),
                (ph::TEXT_T, "type"),
                (ph::POLYGON, "shape"),
                (ph::PAINT_BUCKET, "fill"),
                (ph::IMAGE, "image"),
                (ph::CLOCK_COUNTER_CLOCKWISE, "history"),
                (ph::MAGNIFYING_GLASS_PLUS, "zoom+"),
                (ph::MAGNIFYING_GLASS_MINUS, "zoom-"),
                (ph::ARROWS_LEFT_RIGHT, "swap"),
                (ph::SUN, "light"),
                (ph::MOON, "dark"),
                (ph::SIDEBAR_SIMPLE, "sidebar"),
                (ph::X, "close"),
                (ph::CHECK, "check"),
                (ph::BOUNDING_BOX, "transform"),
                (ph::FLIP_HORIZONTAL, "flip h"),
                (ph::FLIP_VERTICAL, "flip v"),
                (ph::ARROW_CLOCKWISE, "rotate"),
                (ph::SQUARE, "replace"),
                (ph::PLUS_SQUARE, "add"),
                (ph::MINUS_SQUARE, "subtract"),
                (ph::INTERSECT_SQUARE, "intersect"),
                (ph::TEXT_ALIGN_LEFT, "left"),
                (ph::TEXT_ALIGN_CENTER, "center"),
                (ph::TEXT_ALIGN_RIGHT, "right"),
                (ph::FILE_PLUS, "new file"),
                (ph::FOLDER_OPEN, "open"),
                (ph::CLIPBOARD, "clipboard"),
                (ph::DOWNLOAD_SIMPLE, "drop"),
                (ph::WARNING_CIRCLE, "warning"),
                (ph::CARET_DOWN, "caret"),
                (ph::CARET_RIGHT, "caret r"),
                (ph::DOT_OUTLINE, "anchor"),
                (ph::CIRCLE, "anchor on"),
                (ph::EYEDROPPER, "pick"),
            ] {
                ui.label(format!("{i} {n}"));
            }
        });
        ui.separator();
        ui.label(egui::RichText::new("Options bars").strong());
        let mut sink = Vec::new();
        for g in GROUPS {
            for t in g.iter() {
                app.tool = *t;
                ui.push_id(t.label(), |ui| options_bar(app, ui, &mut sink));
            }
        }
    });
    app.tool = saved;
}
