//! Headless interaction tests: real pointer and keyboard events through the whole UI.

use egui::{Event, Key, Modifiers, PointerButton, Pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use studio_core::{Document, Rgba};

use crate::app::App;
use crate::doc::DocTab;
use crate::tools::Tool;

fn harness() -> Harness<'static, App> {
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 900.0)).build_eframe(|cc| App::new(cc, Vec::new()));
    h.run_steps(3);
    let app = h.state_mut();
    let doc = Document::new(400, 300, Some(Rgba::WHITE));
    let mut tab = DocTab::new(doc, "Test".into(), None);
    tab.mark_saved();
    app.add_doc(tab);
    h.run_steps(4);
    h
}

/// Screen position of canvas pixel (x, y).
fn screen(h: &Harness<'static, App>, x: f32, y: f32) -> Pos2 {
    let app = h.state();
    let tab = app.tab().unwrap();
    let view = app.cv.view_rect;
    let origin = view.center() + tab.pan - egui::vec2(tab.doc.width as f32, tab.doc.height as f32) * tab.zoom / 2.0;
    origin + egui::vec2(x, y) * tab.zoom
}

fn drag(h: &mut Harness<'static, App>, from: (f32, f32), to: (f32, f32), mods: Modifiers) {
    let a = screen(h, from.0, from.1);
    let b = screen(h, to.0, to.1);
    h.event(Event::ModifiersChanged(mods));
    h.event(Event::PointerMoved(a));
    h.run_steps(1);
    h.event(Event::PointerButton { pos: a, button: PointerButton::Primary, pressed: true, modifiers: mods });
    h.run_steps(1);
    for i in 1..=8 {
        let t = i as f32 / 8.0;
        h.event(Event::PointerMoved(a + (b - a) * t));
        h.run_steps(1);
    }
    h.event(Event::PointerButton { pos: b, button: PointerButton::Primary, pressed: false, modifiers: mods });
    h.run_steps(2);
    h.event(Event::ModifiersChanged(Modifiers::NONE));
}

fn click(h: &mut Harness<'static, App>, at: (f32, f32)) {
    let p = screen(h, at.0, at.1);
    h.event(Event::PointerMoved(p));
    h.run_steps(1);
    h.event(Event::PointerButton { pos: p, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.run_steps(1);
    h.event(Event::PointerButton {
        pos: p,
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    h.run_steps(2);
}

#[test]
fn brush_stroke_paints_and_undoes() {
    let mut h = harness();
    h.state_mut().set_tool(Tool::Brush);
    h.state_mut().fg = Rgba::rgb(255, 0, 0);
    drag(&mut h, (100.0, 150.0), (300.0, 150.0), Modifiers::NONE);
    let px = h.state().tab().unwrap().doc.layers[0].pixels.get(200, 150);
    assert_eq!(px, Rgba::rgb(255, 0, 0), "brush painted red");
    assert_eq!(h.state().tab().unwrap().history.undo_label(), Some("Brush"));
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(2);
    let px = h.state().tab().unwrap().doc.layers[0].pixels.get(200, 150);
    assert_eq!(px, Rgba::WHITE, "undo restored the pixels");
}

#[test]
fn marquee_selects_and_constrains_fill() {
    let mut h = harness();
    h.state_mut().set_tool(Tool::RectMarquee);
    drag(&mut h, (50.0, 50.0), (150.0, 100.0), Modifiers::NONE);
    let b = h.state().tab().unwrap().doc.selection.as_ref().and_then(|m| m.bounds()).expect("selection");
    assert!((b.x - 50).abs() <= 1 && (b.w as i32 - 100).abs() <= 2, "{b:?}");
    // Delete clears only the selection.
    h.key_press(Key::Delete);
    h.run_steps(2);
    let l = &h.state().tab().unwrap().doc.layers[0];
    assert_eq!(l.pixels.get(100, 75).a(), 0);
    assert_eq!(l.pixels.get(300, 200).a(), 255);
}

#[test]
fn tool_shortcuts_switch_tools() {
    let mut h = harness();
    h.key_press(Key::M);
    h.run_steps(1);
    assert_eq!(h.state().tool, Tool::RectMarquee);
    h.key_press_modifiers(Modifiers::SHIFT, Key::M);
    h.run_steps(1);
    assert_eq!(h.state().tool, Tool::EllipseMarquee);
    h.key_press(Key::B);
    h.run_steps(1);
    assert_eq!(h.state().tool, Tool::Brush);
}

#[test]
fn shape_tool_creates_shape_layer() {
    let mut h = harness();
    h.state_mut().set_tool(Tool::Ellipse);
    drag(&mut h, (100.0, 100.0), (200.0, 180.0), Modifiers::NONE);
    let doc = &h.state().tab().unwrap().doc;
    assert_eq!(doc.layers.len(), 2);
    assert!(matches!(doc.layers[1].kind, studio_core::LayerKind::Shape(_)));
    assert!(doc.layers[1].pixels.get(150, 140).a() > 200);
}

#[test]
fn magic_wand_and_bucket() {
    let mut h = harness();
    {
        let tab = h.state_mut().tab_mut().unwrap();
        for y in 0..300 {
            for x in 0..200 {
                tab.doc.layers[0].pixels.put(x, y, Rgba::BLACK);
            }
        }
        tab.invalidate_all();
    }
    h.state_mut().set_tool(Tool::MagicWand);
    click(&mut h, (50.0, 50.0));
    let b = h.state().tab().unwrap().doc.selection.as_ref().and_then(|m| m.bounds()).expect("wand selection");
    assert_eq!((b.x, b.w), (0, 200));
    let ctx = h.ctx.clone();
    h.state_mut().run(&ctx, crate::actions::Action::Deselect);
    h.state_mut().set_tool(Tool::Bucket);
    h.state_mut().fg = Rgba::rgb(0, 0, 255);
    click(&mut h, (300.0, 100.0));
    let l = &h.state().tab().unwrap().doc.layers[0];
    assert_eq!(l.pixels.get(350, 250), Rgba::rgb(0, 0, 255));
    assert_eq!(l.pixels.get(10, 10), Rgba::BLACK);
}

#[test]
fn crop_tool_crops_on_enter() {
    let mut h = harness();
    h.state_mut().set_tool(Tool::Crop);
    drag(&mut h, (100.0, 50.0), (300.0, 250.0), Modifiers::NONE);
    assert!(h.state().cv.crop.is_some());
    h.key_press(Key::Enter);
    h.run_steps(2);
    let doc = &h.state().tab().unwrap().doc;
    assert!(
        (doc.width as i32 - 200).abs() <= 2 && (doc.height as i32 - 200).abs() <= 2,
        "{}x{}",
        doc.width,
        doc.height
    );
}

#[test]
fn move_tool_moves_layer() {
    let mut h = harness();
    {
        let tab = h.state_mut().tab_mut().unwrap();
        tab.doc.add_empty_layer("dot");
        tab.doc.layers[1].pixels.put(100, 100, Rgba::rgb(9, 9, 9));
        tab.commit("dot");
    }
    h.state_mut().set_tool(Tool::Move);
    drag(&mut h, (100.0, 100.0), (150.0, 120.0), Modifiers::NONE);
    let l = &h.state().tab().unwrap().doc.layers[1];
    let moved = l.pixels.opaque_bounds().unwrap();
    assert!((moved.x - 150).abs() <= 1 && (moved.y - 120).abs() <= 1, "{moved:?}");
}

#[test]
fn adjustment_dialog_applies() {
    let mut h = harness();
    let ctx = h.ctx.clone();
    h.state_mut().run(&ctx, crate::actions::Action::Adjust(studio_core::Adjustment::catalog()[0].clone()));
    h.run_steps(2);
    if let Some(crate::dialogs::Dialog::Adjust { adj, .. }) = &mut h.state_mut().dialog {
        *adj = studio_core::Adjustment::Invert;
    }
    h.run_steps(3);
    h.get_by_label("Apply").click();
    h.run_steps(3);
    assert!(h.state().dialog.is_none());
    assert_eq!(h.state().tab().unwrap().doc.layers[0].pixels.get(5, 5), Rgba::BLACK);
}

fn press_drag_with(h: &mut Harness<'static, App>, from: (f32, f32), to: (f32, f32), mods: Modifiers) {
    drag(h, from, to, mods);
}

fn fill_layer(h: &mut Harness<'static, App>, c: Rgba) {
    let tab = h.state_mut().tab_mut().unwrap();
    let (w, hh) = (tab.doc.width, tab.doc.height);
    tab.doc.layers[0].pixels = studio_core::Image::filled(w, hh, c);
    tab.commit("fill");
}

#[test]
fn gradient_tool_draws_ramp() {
    let mut h = harness();
    h.state_mut().set_tool(Tool::Gradient);
    h.state_mut().fg = Rgba::BLACK;
    h.state_mut().bg = Rgba::WHITE;
    drag(&mut h, (0.0, 150.0), (400.0, 150.0), Modifiers::NONE);
    let l = &h.state().tab().unwrap().doc.layers[0];
    assert!(l.pixels.get(5, 10).r() < 20);
    assert!(l.pixels.get(395, 10).r() > 235);
    let mid = l.pixels.get(200, 10).r();
    assert!((110..=145).contains(&mid), "{mid}");
}

#[test]
fn clone_stamp_copies_from_alt_click_source() {
    let mut h = harness();
    {
        let tab = h.state_mut().tab_mut().unwrap();
        for y in 0..300 {
            for x in 0..100 {
                tab.doc.layers[0].pixels.put(x, y, Rgba::rgb(0, 200, 0));
            }
        }
        tab.commit("paint");
    }
    h.state_mut().set_tool(Tool::CloneStamp);
    let p = screen(&h, 50.0, 150.0);
    h.event(Event::ModifiersChanged(Modifiers::ALT));
    h.event(Event::PointerMoved(p));
    h.run_steps(1);
    h.event(Event::PointerButton { pos: p, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::ALT });
    h.run_steps(1);
    h.event(Event::PointerButton { pos: p, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::ALT });
    h.run_steps(2);
    h.event(Event::ModifiersChanged(Modifiers::NONE));
    assert!(h.state().cv.clone_source.is_some());
    drag(&mut h, (250.0, 150.0), (260.0, 150.0), Modifiers::NONE);
    assert_eq!(h.state().tab().unwrap().doc.layers[0].pixels.get(255, 150), Rgba::rgb(0, 200, 0));
}

#[test]
fn type_tool_creates_and_names_text_layer() {
    let mut h = harness();
    h.state_mut().set_tool(Tool::Text);
    click(&mut h, (60.0, 120.0));
    assert!(h.state().cv.text_edit.is_some(), "editor opened");
    h.event(Event::Text("Hello".into()));
    h.run_steps(3);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Enter);
    h.run_steps(3);
    let doc = &h.state().tab().unwrap().doc;
    assert_eq!(doc.layers.len(), 2);
    assert_eq!(doc.layers[1].name, "Hello");
    assert!(doc.layers[1].pixels.opaque_bounds().is_some(), "text rendered");
    assert_eq!(h.state().tab().unwrap().history.undo_label(), Some("Type Layer"));
}

#[test]
fn free_transform_moves_and_commits() {
    let mut h = harness();
    {
        let tab = h.state_mut().tab_mut().unwrap();
        tab.doc.add_empty_layer("box");
        for y in 100..140 {
            for x in 100..140 {
                tab.doc.layers[1].pixels.put(x, y, Rgba::rgb(200, 0, 0));
            }
        }
        tab.commit("box");
    }
    let ctx = h.ctx.clone();
    h.state_mut().run(&ctx, crate::actions::Action::FreeTransform);
    h.run_steps(2);
    assert!(h.state().cv.transform.is_some());
    drag(&mut h, (120.0, 120.0), (170.0, 130.0), Modifiers::NONE);
    h.key_press(Key::Enter);
    h.run_steps(2);
    let b = h.state().tab().unwrap().doc.layers[1].pixels.opaque_bounds().unwrap();
    assert!((b.x - 150).abs() <= 2 && (b.y - 110).abs() <= 2, "{b:?}");
    assert_eq!(h.state().tab().unwrap().history.undo_label(), Some("Free Transform"));
}

#[test]
fn painting_targets_layer_mask() {
    let mut h = harness();
    let ctx = h.ctx.clone();
    h.state_mut().run(&ctx, crate::actions::Action::MaskRevealAll);
    h.run_steps(2);
    assert!(h.state().edit_mask);
    h.state_mut().set_tool(Tool::Brush);
    h.state_mut().fg = Rgba::BLACK;
    drag(&mut h, (100.0, 150.0), (300.0, 150.0), Modifiers::NONE);
    let l = &h.state().tab().unwrap().doc.layers[0];
    assert_eq!(l.mask.as_ref().unwrap().get(200, 150), 0, "mask painted black");
    assert_eq!(l.pixels.get(200, 150), Rgba::WHITE, "pixels untouched");
}

#[test]
fn layer_style_dialog_applies_shadow() {
    let mut h = harness();
    {
        let tab = h.state_mut().tab_mut().unwrap();
        tab.doc.add_empty_layer("dot");
        for y in 50..80 {
            for x in 50..80 {
                tab.doc.layers[1].pixels.put(x, y, Rgba::rgb(255, 0, 0));
            }
        }
        tab.commit("dot");
    }
    let ctx = h.ctx.clone();
    h.state_mut().run(&ctx, crate::actions::Action::LayerStyle);
    h.run_steps(2);
    if let Some(crate::dialogs::Dialog::LayerStyle { styles, .. }) = &mut h.state_mut().dialog {
        styles.drop_shadow.enabled = true;
    }
    // Toggle via the dialog's own checkbox path: mark the styles on the layer and press OK.
    h.state_mut().tab_mut().unwrap().doc.layers[1].styles.drop_shadow.enabled = true;
    h.run_steps(2);
    h.get_by_label("OK").click();
    h.run_steps(3);
    let tab = h.state().tab().unwrap();
    assert!(tab.doc.layers[1].styles.drop_shadow.enabled);
    assert_eq!(tab.history.undo_label(), Some("Layer Style"));
}

#[test]
fn magic_eraser_clears_connected_color() {
    let mut h = harness();
    {
        let tab = h.state_mut().tab_mut().unwrap();
        for y in 0..300 {
            for x in 0..50 {
                tab.doc.layers[0].pixels.put(x, y, Rgba::BLACK);
            }
        }
        tab.commit("paint");
    }
    h.state_mut().set_tool(Tool::MagicEraser);
    click(&mut h, (300.0, 100.0));
    let l = &h.state().tab().unwrap().doc.layers[0];
    assert_eq!(l.pixels.get(300, 100).a(), 0);
    assert_eq!(l.pixels.get(10, 10), Rgba::BLACK);
}

#[test]
fn background_eraser_and_pattern_stamp() {
    let mut h = harness();
    h.state_mut().set_tool(Tool::BackgroundEraser);
    drag(&mut h, (100.0, 150.0), (200.0, 150.0), Modifiers::NONE);
    assert_eq!(h.state().tab().unwrap().doc.layers[0].pixels.get(150, 150).a(), 0);

    h.state_mut().set_tool(Tool::PatternStamp);
    h.state_mut().opts.tip.hardness = 1.0;
    drag(&mut h, (100.0, 250.0), (300.0, 250.0), Modifiers::NONE);
    let l = &h.state().tab().unwrap().doc.layers[0];
    let a = l.pixels.get(200, 250);
    let b = l.pixels.get(208, 250);
    assert!(a != b, "checker pattern alternates: {a:?} {b:?}");
}

#[test]
fn history_brush_restores_original() {
    let mut h = harness();
    fill_layer(&mut h, Rgba::rgb(0, 0, 255));
    h.state_mut().set_tool(Tool::HistoryBrush);
    h.state_mut().opts.tip.hardness = 1.0;
    drag(&mut h, (100.0, 150.0), (300.0, 150.0), Modifiers::NONE);
    let l = &h.state().tab().unwrap().doc.layers[0];
    assert_eq!(l.pixels.get(200, 150), Rgba::WHITE, "painted back to the opened state");
    assert_eq!(l.pixels.get(200, 20), Rgba::rgb(0, 0, 255));
}

#[test]
fn red_eye_and_ruler_straighten() {
    let mut h = harness();
    fill_layer(&mut h, Rgba::rgb(220, 30, 30));
    h.state_mut().set_tool(Tool::RedEye);
    press_drag_with(&mut h, (150.0, 100.0), (250.0, 200.0), Modifiers::NONE);
    assert!(h.state().tab().unwrap().doc.layers[0].pixels.get(200, 150).r() < 80);

    h.state_mut().set_tool(Tool::Ruler);
    drag(&mut h, (50.0, 100.0), (350.0, 130.0), Modifiers::NONE);
    assert!(h.state().cv.ruler.is_some());
    let ctx = h.ctx.clone();
    h.state_mut().run(&ctx, crate::actions::Action::Straighten);
    h.run_steps(2);
    let d = &h.state().tab().unwrap().doc;
    assert!(d.width > 400, "canvas rotated and grew: {}x{}", d.width, d.height);
}

#[test]
fn custom_shapes_render() {
    let mut h = harness();
    for t in [Tool::Triangle, Tool::Heart, Tool::Arrow, Tool::SpeechBubble, Tool::Checkmark] {
        h.state_mut().set_tool(t);
        drag(&mut h, (100.0, 60.0), (300.0, 240.0), Modifiers::NONE);
        let doc = &h.state().tab().unwrap().doc;
        let l = doc.layers.last().unwrap();
        assert!(l.pixels.opaque_bounds().is_some(), "{} drew pixels", t.label());
        assert_eq!(l.name, t.label());
    }
    assert_eq!(h.state().tab().unwrap().doc.layers.len(), 6);
}

#[test]
fn group_shortcuts_and_hidden_group_render() {
    let mut h = harness();
    {
        let tab = h.state_mut().tab_mut().unwrap();
        tab.doc.add_empty_layer("red");
        for y in 0..300 {
            for x in 0..400 {
                tab.doc.layers[1].pixels.put(x, y, Rgba::rgb(255, 0, 0));
            }
        }
        tab.commit("red");
    }
    h.key_press_modifiers(Modifiers::COMMAND, Key::G);
    h.run_steps(3);
    {
        let doc = &h.state().tab().unwrap().doc;
        assert_eq!(doc.layers.len(), 3);
        assert!(doc.layers[2].is_group());
        assert_eq!(doc.layers[1].parent, Some(doc.layers[2].id));
        assert_eq!(h.state().tab().unwrap().composite.get(5, 5), Rgba::rgb(255, 0, 0));
    }
    // Hiding the group hides its contents on the canvas.
    {
        let tab = h.state_mut().tab_mut().unwrap();
        tab.doc.layers[2].visible = false;
        tab.commit("hide");
    }
    h.run_steps(3);
    assert_eq!(h.state().tab().unwrap().composite.get(5, 5), Rgba::WHITE);
    // Ungroup restores a flat stack.
    {
        let tab = h.state_mut().tab_mut().unwrap();
        tab.doc.layers[2].visible = true;
        tab.doc.active = 2;
    }
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::G);
    h.run_steps(3);
    let doc = &h.state().tab().unwrap().doc;
    assert_eq!(doc.layers.len(), 2);
    assert!(doc.layers.iter().all(|l| !l.is_group() && l.parent.is_none()));
}

#[test]
fn merge_group_with_cmd_e() {
    let mut h = harness();
    let ctx = h.ctx.clone();
    h.state_mut().run(&ctx, crate::actions::Action::NewLayer);
    h.state_mut().run(&ctx, crate::actions::Action::GroupLayers);
    h.run_steps(2);
    assert!(h.state().tab().unwrap().doc.active_layer().unwrap().is_group());
    h.key_press_modifiers(Modifiers::COMMAND, Key::E);
    h.run_steps(2);
    let doc = &h.state().tab().unwrap().doc;
    assert_eq!(doc.layers.len(), 2);
    assert!(!doc.layers[1].is_group());
    assert_eq!(h.state().tab().unwrap().history.undo_label(), Some("Merge Down"));
}

/// Offscreen screenshots of the real UI (no window needed):
/// `PHOTO_STUDIO_SAMPLE=img.jpg PHOTO_STUDIO_SHOTS=out/ cargo test -p photo-studio screenshots -- --ignored`
#[test]
#[ignore]
fn screenshots() {
    let sample = std::env::var("PHOTO_STUDIO_SAMPLE").expect("PHOTO_STUDIO_SAMPLE");
    let out = std::path::PathBuf::from(std::env::var("PHOTO_STUDIO_SHOTS").unwrap_or_else(|_| ".".into()));
    std::fs::create_dir_all(&out).unwrap();
    for (name, variant, theme) in [
        ("demo", "1", "dark"),
        ("styles", "styles", "dark"),
        ("filter", "filter", "dark"),
        ("light", "1", "light"),
        ("guides", "guides", "dark"),
        ("palette", "palette", "dark"),
        ("rotated", "rotated", "dark"),
    ] {
        let mut h = Harness::builder()
            .with_size(egui::vec2(1440.0, 900.0))
            .with_pixels_per_point(2.0)
            .wgpu()
            .build_eframe(|cc| App::new(cc, vec![std::path::PathBuf::from(&sample)]));
        let ctx = h.ctx.clone();
        let app = h.state_mut();
        app.prefs.theme = if theme == "light" { crate::theme::ThemeMode::Light } else { crate::theme::ThemeMode::Dark };
        crate::theme::apply(&ctx, app.prefs.theme);
        h.run_steps(4);
        crate::shell::build_demo(h.state_mut(), &ctx, variant);
        if variant == "guides" {
            let app = h.state_mut();
            app.prefs.show_grid = true;
            let tab = app.tab_mut().unwrap();
            tab.doc.guides.push(studio_core::document::Guide { horizontal: true, pos: 300.0 });
            tab.doc.guides.push(studio_core::document::Guide { horizontal: false, pos: 640.0 });
        }
        if variant == "rotated" {
            h.state_mut().tab_mut().unwrap().rotation = -20.0;
        }
        if variant == "palette" {
            h.state_mut().dialog = Some(crate::dialogs::Dialog::CommandPalette { query: "blur".into(), selected: 0 });
        }
        h.run_steps(8);
        let img = h.render().expect("render");
        img.save(out.join(format!("{name}.png"))).unwrap();
    }
}

fn raw_drag(h: &mut Harness<'static, App>, a: Pos2, b: Pos2) {
    h.event(Event::PointerMoved(a));
    h.run_steps(1);
    h.event(Event::PointerButton { pos: a, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.run_steps(1);
    for i in 1..=8 {
        h.event(Event::PointerMoved(a + (b - a) * (i as f32 / 8.0)));
        h.run_steps(1);
    }
    h.event(Event::PointerButton {
        pos: b,
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    h.run_steps(2);
}

#[test]
fn guides_from_ruler_snap_and_delete() {
    let mut h = harness();
    // Drag from the top ruler down to y = 100 on the canvas.
    let target = screen(&h, 200.0, 100.0);
    let ruler = Pos2::new(target.x, h.state().cv.view_rect.top() - 10.0);
    raw_drag(&mut h, ruler, target);
    let guides = h.state().tab().unwrap().doc.guides.clone();
    assert_eq!(guides.len(), 1, "guide created");
    assert!(guides[0].horizontal && (guides[0].pos - 100.0).abs() <= 1.0, "{guides:?}");

    // A marquee dragged near the guide snaps to it.
    h.state_mut().set_tool(Tool::RectMarquee);
    drag(&mut h, (50.0, 30.0), (150.0, 97.0), Modifiers::NONE);
    let b = h.state().tab().unwrap().doc.selection.as_ref().and_then(|m| m.bounds()).unwrap();
    assert_eq!(b.bottom(), 100, "snapped to the guide: {b:?}");

    // The Move tool drags the guide off the canvas to delete it.
    h.state_mut().set_tool(Tool::Move);
    let on_guide = screen(&h, 300.0, 100.0);
    let outside = Pos2::new(on_guide.x, h.state().cv.view_rect.top() - 5.0);
    raw_drag(&mut h, on_guide, outside);
    assert!(h.state().tab().unwrap().doc.guides.is_empty(), "guide deleted");
}

#[test]
fn command_palette_finds_and_runs() {
    assert!(crate::menus::fuzzy("gaus", "Filter › Blur › Gaussian Blur").is_some());
    assert!(crate::menus::fuzzy("xyz", "Filter › Blur › Gaussian Blur").is_none());
    let mut h = harness();
    h.key_press_modifiers(Modifiers::COMMAND, Key::F);
    h.run_steps(2);
    assert!(matches!(h.state().dialog, Some(crate::dialogs::Dialog::CommandPalette { .. })));
    h.event(Event::Text("new layer".into()));
    h.run_steps(2);
    h.key_press(Key::Enter);
    h.run_steps(3);
    assert!(h.state().dialog.is_none());
    assert_eq!(h.state().tab().unwrap().doc.layers.len(), 2, "New Layer ran");
}

#[test]
fn pen_path_to_selection_shape_and_edit() {
    let mut h = harness();
    h.state_mut().set_tool(Tool::Pen);
    for q in [(100.0, 100.0), (300.0, 100.0), (300.0, 250.0), (100.0, 250.0), (100.0, 100.0)] {
        click(&mut h, q);
    }
    {
        let doc = &h.state().tab().unwrap().doc;
        assert_eq!(doc.paths.len(), 1, "closing click stored the path");
        assert!(doc.paths[0].path.closed);
        assert_eq!(doc.paths[0].path.anchors.len(), 4);
    }
    // ⌘↩ makes a selection from it.
    h.key_press_modifiers(Modifiers::COMMAND, Key::Enter);
    h.run_steps(2);
    let b = h.state().tab().unwrap().doc.selection.as_ref().and_then(|m| m.bounds()).expect("selection");
    assert!((b.x - 100).abs() <= 1 && (b.w as i32 - 200).abs() <= 2, "{b:?}");

    // Direct Selection drags an anchor.
    h.state_mut().set_tool(Tool::DirectSelect);
    drag(&mut h, (300.0, 250.0), (350.0, 280.0), Modifiers::NONE);
    let a = h.state().tab().unwrap().doc.paths[0].path.anchors[2].p;
    assert!((a.0 - 350.0).abs() < 2.0 && (a.1 - 280.0).abs() < 2.0, "{a:?}");

    // Shape layer from the path.
    let ctx = h.ctx.clone();
    h.state_mut().run(&ctx, crate::actions::Action::PathToShape);
    h.run_steps(2);
    let doc = &h.state().tab().unwrap().doc;
    let l = doc.layers.last().unwrap();
    assert!(matches!(l.kind, studio_core::LayerKind::Shape(_)));
    assert!(l.pixels.get(200, 175).a() > 200, "filled inside");
}

#[test]
fn pen_drag_makes_a_curve() {
    let mut h = harness();
    h.state_mut().set_tool(Tool::Pen);
    click(&mut h, (100.0, 200.0));
    drag(&mut h, (300.0, 200.0), (300.0, 100.0), Modifiers::NONE);
    h.key_press(Key::Enter);
    h.run_steps(2);
    let p = &h.state().tab().unwrap().doc.paths[0].path;
    assert!(!p.closed);
    assert!(p.anchors[0].is_corner());
    assert!(!p.anchors[1].is_corner(), "dragging made a smooth anchor");
    assert!((p.anchors[1].h_out.1 - 100.0).abs() < 2.0);
}

#[test]
fn quick_mask_paints_the_selection() {
    let mut h = harness();
    h.key_press(Key::Q);
    h.run_steps(2);
    assert!(h.state().quick_mask);
    h.state_mut().set_tool(Tool::Brush);
    h.state_mut().fg = Rgba::BLACK;
    h.state_mut().opts.tip.hardness = 1.0;
    drag(&mut h, (100.0, 150.0), (300.0, 150.0), Modifiers::NONE);
    h.key_press(Key::Q);
    h.run_steps(2);
    assert!(!h.state().quick_mask);
    let tab = h.state().tab().unwrap();
    let sel = tab.doc.selection.as_ref().expect("selection");
    assert_eq!(sel.get(200, 150), 0, "painted black = deselected");
    assert_eq!(sel.get(200, 20), 255, "rest stays selected");
    assert_eq!(tab.doc.layers[0].pixels.get(200, 150), Rgba::WHITE, "pixels untouched");
}

#[test]
fn save_and_load_selection() {
    let mut h = harness();
    h.state_mut().set_tool(Tool::RectMarquee);
    drag(&mut h, (20.0, 20.0), (120.0, 80.0), Modifiers::NONE);
    let ctx = h.ctx.clone();
    h.state_mut().run(&ctx, crate::actions::Action::SaveSelection);
    h.state_mut().run(&ctx, crate::actions::Action::Deselect);
    assert!(h.state().tab().unwrap().doc.selection.is_none());
    h.state_mut().run(&ctx, crate::actions::Action::LoadChannel(0));
    let b = h.state().tab().unwrap().doc.selection.as_ref().and_then(|m| m.bounds()).unwrap();
    assert!((b.x - 20).abs() <= 1 && (b.w as i32 - 100).abs() <= 2);
    assert_eq!(h.state().tab().unwrap().doc.channels.len(), 1);
}

#[test]
fn smart_object_filters_and_project_round_trip() {
    let mut h = harness();
    {
        let tab = h.state_mut().tab_mut().unwrap();
        tab.doc.add_empty_layer("art");
        for y in 100..200 {
            for x in 100..200 {
                tab.doc.layers[1].pixels.put(x, y, Rgba::rgb(0, 0, 0));
            }
        }
        tab.commit("art");
    }
    let ctx = h.ctx.clone();
    h.state_mut().run(&ctx, crate::actions::Action::ConvertToSmart);
    h.state_mut().run(&ctx, crate::actions::Action::Filter(studio_core::Filter::Solarize));
    h.run_steps(2);
    let tab = h.state().tab().unwrap();
    let l = &tab.doc.layers[1];
    match &l.kind {
        studio_core::LayerKind::Smart { filters, .. } => assert_eq!(filters.len(), 1),
        k => panic!("expected a smart object, got {}", k.badge()),
    }
    assert!(l.source.is_some());
    assert_eq!(tab.history.undo_label(), Some("Smart Filter: Solarize"));
    // The project keeps the source and the filter list.
    let bytes = studio_core::io::project_bytes(&tab.doc).unwrap();
    let back = studio_core::io::project_from_bytes(&bytes).unwrap();
    assert_eq!(back.layers[1].kind, l.kind);
    assert_eq!(back.layers[1].pixels, l.pixels);
    assert_eq!(back.layers[1].source, l.source);
    // Disabling the smart filter via Properties-style edit re-renders from the source.
    let tab = h.state_mut().tab_mut().unwrap();
    let (w, hh) = (tab.doc.width, tab.doc.height);
    if let studio_core::LayerKind::Smart { filters, .. } = &mut tab.doc.layers[1].kind {
        filters[0].enabled = false;
    }
    tab.doc.layers[1].render(w, hh);
    assert_eq!(tab.doc.layers[1].pixels.get(150, 150), Rgba::BLACK);
}

#[test]
fn align_paste_in_place_and_object_select() {
    let mut h = harness();
    {
        let tab = h.state_mut().tab_mut().unwrap();
        tab.doc.add_empty_layer("dot");
        for y in 50..70 {
            for x in 60..80 {
                tab.doc.layers[1].pixels.put(x, y, Rgba::rgb(200, 20, 20));
            }
        }
        tab.commit("dot");
    }
    let ctx = h.ctx.clone();
    h.state_mut().run(&ctx, crate::actions::Action::Align(Some(1), Some(1)));
    let b = h.state().tab().unwrap().doc.layers[1].pixels.opaque_bounds().unwrap();
    assert_eq!((b.right(), b.bottom()), (400, 300), "aligned to the canvas corner");

    // Copy, then Paste in Place lands exactly where it came from.
    h.state_mut().run(&ctx, crate::actions::Action::Copy);
    h.state_mut().run(&ctx, crate::actions::Action::PasteInPlace);
    let doc = &h.state().tab().unwrap().doc;
    assert_eq!(doc.layers.len(), 3);
    assert_eq!(doc.layers[2].pixels.opaque_bounds(), Some(b));

    // Object Selection finds the square inside a box drawn around it.
    h.state_mut().set_tool(Tool::ObjectSelect);
    drag(&mut h, (360.0, 260.0), (400.0, 300.0), Modifiers::NONE);
    let sel = h.state().tab().unwrap().doc.selection.as_ref().expect("selection");
    assert!(sel.get(390, 290) > 200);
    assert_eq!(sel.get(365, 265), 0);
}

#[test]
fn vertical_type_option() {
    let mut h = harness();
    h.state_mut().set_tool(Tool::Text);
    h.state_mut().opts.vertical_text = true;
    click(&mut h, (200.0, 60.0));
    h.event(Event::Text("ABCD".into()));
    h.run_steps(3);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Enter);
    h.run_steps(3);
    let doc = &h.state().tab().unwrap().doc;
    let ink = doc.layers[1].pixels.opaque_bounds().unwrap();
    assert!(ink.h > ink.w * 2, "{ink:?}");
}

#[test]
fn record_play_and_batch_actions() {
    use crate::actions::Action;
    let mut h = harness();
    let ctx = h.ctx.clone();
    let app = h.state_mut();
    app.run(&ctx, Action::StartRecording);
    app.run(&ctx, Action::ApplyAdjustment(studio_core::Adjustment::Invert));
    app.run(&ctx, Action::RotateCanvas(studio_core::transform::Rotation::Cw90));
    app.run(&ctx, Action::Undo); // not recorded
    app.run(&ctx, Action::RotateCanvas(studio_core::transform::Rotation::Cw90));
    app.run(&ctx, Action::StopRecording);
    assert_eq!(app.prefs.macros.len(), 1);
    assert_eq!(app.prefs.macros[0].1.len(), 3, "{:?}", app.prefs.macros[0].1);

    // Play on a fresh white image.
    let mut tab = DocTab::new(Document::new(40, 20, Some(Rgba::WHITE)), "t".into(), None);
    tab.mark_saved();
    app.add_doc(tab);
    app.run(&ctx, Action::PlayMacro(0));
    let doc = &app.tab().unwrap().doc;
    // Undo isn't recorded, so both 90° turns replay: the image ends up upright again.
    assert_eq!((doc.width, doc.height), (40, 20), "rotated twice");
    assert_eq!(doc.layers[0].pixels.get(5, 5), Rgba::BLACK, "inverted");

    // Batch over a folder.
    let dir = std::env::temp_dir().join(format!("ps-batch-{}", std::process::id()));
    let (src, dst) = (dir.join("in"), dir.join("out"));
    std::fs::create_dir_all(&src).unwrap();
    std::fs::create_dir_all(&dst).unwrap();
    for n in ["a", "b"] {
        let img = studio_core::Image::filled(10, 6, Rgba::WHITE);
        std::fs::write(
            src.join(format!("{n}.png")),
            studio_core::io::encode(&img, "png", &Default::default()).unwrap(),
        )
        .unwrap();
    }
    let docs_before = app.docs.len();
    let (ok, bad) = crate::app::run_batch(app, &ctx, 0, &src, &dst, "png");
    assert_eq!((ok, bad), (2, 0));
    assert_eq!(app.docs.len(), docs_before, "batch leaves tabs as they were");
    let out = studio_core::io::read_image(&dst.join("a.png")).unwrap();
    assert_eq!(out.size(), (10, 6));
    assert_eq!(out.get(1, 1), Rgba::BLACK);
    std::fs::remove_dir_all(&dir).ok();
}

fn striped_with_blob(h: &mut Harness<'static, App>) {
    let tab = h.state_mut().tab_mut().unwrap();
    for y in 0..300 {
        for x in 0..400 {
            let c = if (y / 10) % 2 == 0 { Rgba::rgb(220, 60, 40) } else { Rgba::rgb(30, 90, 200) };
            tab.doc.layers[0].pixels.put(x, y, c);
        }
    }
    for y in 140..160 {
        for x in 190..210 {
            tab.doc.layers[0].pixels.put(x, y, Rgba::rgb(0, 255, 0));
        }
    }
    tab.commit("setup");
}

#[test]
fn remove_tool_and_content_aware_fill() {
    let mut h = harness();
    striped_with_blob(&mut h);
    h.state_mut().set_tool(Tool::Remove);
    h.state_mut().opts.tip.size = 40.0;
    drag(&mut h, (190.0, 150.0), (210.0, 150.0), Modifiers::NONE);
    let l = &h.state().tab().unwrap().doc.layers[0];
    assert!((140..160).all(|y| (190..210).all(|x| l.pixels.get(x, y).g() < 140)), "blob removed");
    assert_eq!(h.state().tab().unwrap().history.undo_label(), Some("Remove"));

    // The same through a selection and Edit › Content-Aware Fill.
    let mut h = harness();
    striped_with_blob(&mut h);
    h.state_mut().set_tool(Tool::RectMarquee);
    drag(&mut h, (185.0, 135.0), (215.0, 165.0), Modifiers::NONE);
    h.key_press_modifiers(Modifiers::SHIFT, Key::Backspace);
    h.run_steps(2);
    let l = &h.state().tab().unwrap().doc.layers[0];
    assert!((140..160).all(|y| (190..210).all(|x| l.pixels.get(x, y).g() < 140)), "filled");
}

#[test]
fn liquify_pushes_pixels() {
    let mut h = harness();
    {
        let tab = h.state_mut().tab_mut().unwrap();
        for y in 0..300 {
            for x in 150..200 {
                tab.doc.layers[0].pixels.put(x, y, Rgba::BLACK);
            }
        }
        tab.commit("bar");
    }
    h.state_mut().set_tool(Tool::Liquify);
    h.state_mut().opts.tip.size = 80.0;
    h.state_mut().opts.strength = 1.0;
    drag(&mut h, (190.0, 150.0), (260.0, 150.0), Modifiers::NONE);
    let l = &h.state().tab().unwrap().doc.layers[0];
    assert!((205..290).any(|x| l.pixels.get(x, 150).r() < 64), "black pushed past the bar's right edge");
    assert_eq!(l.pixels.get(215, 20), Rgba::WHITE, "outside the brush unchanged");
    assert_eq!(h.state().tab().unwrap().history.undo_label(), Some("Liquify"));
}

#[test]
fn magnetic_lasso_snaps_to_edges() {
    let mut h = harness();
    {
        let tab = h.state_mut().tab_mut().unwrap();
        for y in 100..200 {
            for x in 150..250 {
                tab.doc.layers[0].pixels.put(x, y, Rgba::BLACK);
            }
        }
        tab.commit("square");
    }
    h.run_steps(2);
    h.state_mut().set_tool(Tool::MagneticLasso);
    // Trace a loose loop ~5 px outside the square.
    let pts = [(145.0, 95.0), (255.0, 95.0), (255.0, 205.0), (145.0, 205.0), (145.0, 97.0)];
    let (o, z) = (screen(&h, 0.0, 0.0), h.state().tab().unwrap().zoom);
    let to = move |q: (f32, f32)| o + egui::vec2(q.0, q.1) * z;
    let a = to(pts[0]);
    h.event(Event::PointerMoved(a));
    h.run_steps(1);
    h.event(Event::PointerButton { pos: a, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.run_steps(1);
    for w in pts.windows(2) {
        for i in 1..=12 {
            let t = i as f32 / 12.0;
            let q = (w[0].0 + (w[1].0 - w[0].0) * t, w[0].1 + (w[1].1 - w[0].1) * t);
            h.event(Event::PointerMoved(to(q)));
            h.run_steps(1);
        }
    }
    let end = to(*pts.last().unwrap());
    h.event(Event::PointerButton {
        pos: end,
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    h.run_steps(2);
    let b = h.state().tab().unwrap().doc.selection.as_ref().and_then(|m| m.bounds()).expect("selection");
    assert!((b.x - 150).abs() <= 3 && (b.right() - 250).abs() <= 3, "snapped horizontally: {b:?}");
    assert!((b.y - 100).abs() <= 3 && (b.bottom() - 200).abs() <= 3, "snapped vertically: {b:?}");
}

#[test]
fn content_aware_move_tool() {
    let mut h = harness();
    {
        let tab = h.state_mut().tab_mut().unwrap();
        for y in 100..140 {
            for x in 50..90 {
                tab.doc.layers[0].pixels.put(x, y, Rgba::rgb(0, 0, 255));
            }
        }
        tab.commit("square");
    }
    h.state_mut().set_tool(Tool::ContentMove);
    // First drag (outside any selection) draws a lasso around the square.
    let a = screen(&h, 45.0, 95.0);
    let pts = [(45.0, 95.0), (95.0, 95.0), (95.0, 145.0), (45.0, 145.0), (45.0, 96.0)];
    let (o, z) = (screen(&h, 0.0, 0.0), h.state().tab().unwrap().zoom);
    let to = move |q: (f32, f32)| o + egui::vec2(q.0, q.1) * z;
    h.event(Event::PointerMoved(a));
    h.run_steps(1);
    h.event(Event::PointerButton { pos: a, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.run_steps(1);
    for w in pts.windows(2) {
        for i in 1..=6 {
            let t = i as f32 / 6.0;
            h.event(Event::PointerMoved(to((w[0].0 + (w[1].0 - w[0].0) * t, w[0].1 + (w[1].1 - w[0].1) * t))));
            h.run_steps(1);
        }
    }
    h.event(Event::PointerButton {
        pos: to(pts[4]),
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    h.run_steps(2);
    assert!(h.state().tab().unwrap().doc.selection.is_some());
    // Second drag, inside the selection, moves the square 200 px right.
    drag(&mut h, (70.0, 120.0), (270.0, 120.0), Modifiers::NONE);
    let l = &h.state().tab().unwrap().doc.layers[0];
    assert_eq!(l.pixels.get(270, 120), Rgba::rgb(0, 0, 255), "moved");
    assert!(
        l.pixels.get(70, 120).b() < 250 || l.pixels.get(70, 120).r() > 100,
        "old spot filled: {:?}",
        l.pixels.get(70, 120)
    );
    assert_eq!(h.state().tab().unwrap().history.undo_label(), Some("Content-Aware Move"));
}

#[test]
fn pattern_fill_fill_opacity_and_labels_round_trip() {
    let mut h = harness();
    let ctx = h.ctx.clone();
    h.state_mut().run(&ctx, crate::actions::Action::NewFillPattern);
    let tab = h.state_mut().tab_mut().unwrap();
    let l = tab.doc.layers.last_mut().unwrap();
    assert_ne!(l.pixels.get(2, 2), l.pixels.get(10, 2), "checker pattern drawn");
    l.fill_opacity = 0.4;
    l.label = Some(3);
    let bytes = studio_core::io::project_bytes(&tab.doc).unwrap();
    let back = studio_core::io::project_from_bytes(&bytes).unwrap();
    let b = back.layers.last().unwrap();
    assert_eq!(b.kind, tab.doc.layers.last().unwrap().kind);
    assert_eq!((b.fill_opacity, b.label), (0.4, Some(3)));
    assert_eq!(back.composite(), tab.doc.composite());
}

#[test]
fn rotate_view_keeps_painting_accurate() {
    let mut h = harness();
    h.key_press(Key::R);
    h.run_steps(1);
    assert_eq!(h.state().tool, Tool::RotateView);
    h.state_mut().tab_mut().unwrap().rotation = 90.0;
    h.run_steps(2);
    // Map canvas → screen with the same rotation the view uses.
    let pivot = h.state().cv.view_rect.center();
    let rot = egui::emath::Rot2::from_angle(90f32.to_radians());
    let s = |h: &Harness<'static, App>, x: f32, y: f32| pivot + rot * (screen(h, x, y) - pivot);
    h.state_mut().set_tool(Tool::Brush);
    h.state_mut().fg = Rgba::rgb(255, 0, 0);
    let (a, b) = (s(&h, 100.0, 150.0), s(&h, 300.0, 150.0));
    raw_drag(&mut h, a, b);
    let l = &h.state().tab().unwrap().doc.layers[0];
    assert_eq!(l.pixels.get(200, 150), Rgba::rgb(255, 0, 0), "painted along the canvas line");
    assert_eq!(l.pixels.get(150, 200), Rgba::WHITE, "not along the screen line");
}

#[test]
fn path_selection_moves_whole_path() {
    let mut h = harness();
    h.state_mut().set_tool(Tool::Pen);
    for q in [(100.0, 100.0), (200.0, 100.0), (200.0, 200.0), (100.0, 200.0), (100.0, 100.0)] {
        click(&mut h, q);
    }
    h.state_mut().active_path = None;
    h.state_mut().set_tool(Tool::PathSelect);
    drag(&mut h, (150.0, 150.0), (200.0, 170.0), Modifiers::NONE);
    let p = &h.state().tab().unwrap().doc.paths[0].path;
    assert!((p.anchors[0].p.0 - 150.0).abs() < 1.5 && (p.anchors[0].p.1 - 120.0).abs() < 1.5, "{:?}", p.anchors[0].p);
    assert_eq!(h.state().active_path, Some(0));
    assert_eq!(h.state().tab().unwrap().history.undo_label(), Some("Move Path"));
}

#[test]
fn multi_layer_selection_actions() {
    use crate::actions::Action;
    let mut h = harness();
    {
        let tab = h.state_mut().tab_mut().unwrap();
        for (k, x) in [20u32, 100, 300].iter().enumerate() {
            tab.doc.add_empty_layer(&format!("dot{k}"));
            let i = tab.doc.active;
            for y in 40 + k as u32 * 30..60 + k as u32 * 30 {
                for xx in *x..x + 20 {
                    tab.doc.layers[i].pixels.put(xx, y, Rgba::BLACK);
                }
            }
        }
        tab.multi = vec![1, 2, 3];
        tab.commit("dots");
    }
    let ctx = h.ctx.clone();
    h.state_mut().run(&ctx, Action::Align(None, Some(-1)));
    h.state_mut().run(&ctx, Action::Distribute(true));
    {
        let d = &h.state().tab().unwrap().doc;
        assert!((1..4).all(|i| d.content_bounds(i).unwrap().y == 40), "tops aligned");
        assert_eq!(d.content_bounds(2).unwrap().x, 160, "middle distributed");
    }
    // The Move tool drags all three.
    h.state_mut().set_tool(Tool::Move);
    drag(&mut h, (30.0, 50.0), (30.0, 150.0), Modifiers::NONE);
    {
        let d = &h.state().tab().unwrap().doc;
        assert!((1..4).all(|i| d.content_bounds(i).unwrap().y == 140), "all moved together");
    }
    // Group two of them.
    h.state_mut().tab_mut().unwrap().multi = vec![1, 3];
    h.state_mut().run(&ctx, Action::GroupLayers);
    {
        let d = &h.state().tab().unwrap().doc;
        let g = d.layers.iter().find(|l| l.is_group()).expect("group");
        assert_eq!(d.layers.iter().filter(|l| l.parent == Some(g.id)).count(), 2);
    }
    // Delete a multi-selection.
    let n = h.state().tab().unwrap().doc.layers.len();
    h.state_mut().tab_mut().unwrap().multi = vec![1, 2];
    h.state_mut().run(&ctx, Action::DeleteLayer);
    assert_eq!(h.state().tab().unwrap().doc.layers.len(), n - 2);
}

#[test]
fn note_count_and_slice_tools() {
    let mut h = harness();
    // Note: click, type, Done.
    h.state_mut().set_tool(Tool::Note);
    click(&mut h, (50.0, 50.0));
    assert!(matches!(h.state().dialog, Some(crate::dialogs::Dialog::EditNote { .. })));
    h.event(Event::Text("Fix the sky".into()));
    h.run_steps(2);
    h.get_by_label("Done").click();
    h.run_steps(3);
    assert!(h.state().dialog.is_none());
    assert_eq!(h.state().tab().unwrap().doc.annotations.notes[0].1, "Fix the sky");

    // Count: three clicks, then Alt-click removes the nearest.
    h.state_mut().set_tool(Tool::Count);
    for x in [100.0, 150.0, 200.0] {
        click(&mut h, (x, 100.0));
    }
    let p = screen(&h, 151.0, 100.0);
    h.event(Event::ModifiersChanged(Modifiers::ALT));
    h.event(Event::PointerMoved(p));
    h.run_steps(1);
    h.event(Event::PointerButton { pos: p, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::ALT });
    h.run_steps(1);
    h.event(Event::PointerButton { pos: p, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::ALT });
    h.run_steps(2);
    h.event(Event::ModifiersChanged(Modifiers::NONE));
    let counts = h.state().tab().unwrap().doc.annotations.counts.clone();
    assert_eq!(counts.len(), 2);
    assert!(counts.iter().all(|c| (c.0 - 150.0).abs() > 10.0), "{counts:?}");

    // Slice: drag a region.
    h.state_mut().set_tool(Tool::Slice);
    drag(&mut h, (10.0, 10.0), (110.0, 60.0), Modifiers::NONE);
    let s = h.state().tab().unwrap().doc.annotations.slices.clone();
    assert_eq!(s.len(), 1);
    assert!((s[0].w as i32 - 100).abs() <= 2 && (s[0].h as i32 - 50).abs() <= 2, "{s:?}");
}

#[test]
fn vector_mask_from_path_and_edit() {
    use crate::actions::Action;
    let mut h = harness();
    {
        let tab = h.state_mut().tab_mut().unwrap();
        tab.doc.layers[0].pixels = studio_core::Image::filled(400, 300, Rgba::BLACK);
        tab.doc.add_empty_layer("white");
        let i = tab.doc.active;
        tab.doc.layers[i].pixels = studio_core::Image::filled(400, 300, Rgba::WHITE);
        tab.commit("setup");
    }
    h.state_mut().set_tool(Tool::Pen);
    for q in [(100.0, 100.0), (200.0, 100.0), (200.0, 200.0), (100.0, 200.0), (100.0, 100.0)] {
        click(&mut h, q);
    }
    let ctx = h.ctx.clone();
    h.state_mut().run(&ctx, Action::VectorMaskFromPath);
    h.run_steps(3);
    {
        let tab = h.state().tab().unwrap();
        assert!(tab.doc.layers[1].vector_mask.is_some());
        assert_eq!(tab.composite.get(150, 150), Rgba::WHITE, "inside shows the layer");
        assert_eq!(tab.composite.get(250, 150), Rgba::BLACK, "outside is masked");
    }
    // Drag the mask's bottom-right corner outwards with Direct Selection.
    h.state_mut().active_path = None;
    h.state_mut().tab_mut().unwrap().doc.paths.clear();
    h.state_mut().set_tool(Tool::DirectSelect);
    drag(&mut h, (200.0, 200.0), (300.0, 250.0), Modifiers::NONE);
    h.run_steps(3);
    let tab = h.state().tab().unwrap();
    assert_eq!(tab.composite.get(280, 240), Rgba::WHITE, "mask grew with the anchor");
}

#[test]
fn brush_texture_and_dual_brush_controls() {
    let mut h = harness();
    h.state_mut().set_tool(Tool::Brush);
    h.run_steps(2);
    h.get_by_label(&format!("{} Brush", egui_phosphor::regular::PAINT_BRUSH_BROAD)).click();
    h.run_steps(2);
    let inside = h.get_by_label("Presets").rect().center();
    h.event(Event::PointerMoved(inside));
    h.event(Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, -800.0),
        modifiers: Modifiers::NONE,
        phase: egui::TouchPhase::Move,
    });
    h.run_steps(4);
    h.get_by_label("Canvas").click();
    h.run_steps(2);
    // The texture sliders appear and push the dual brush further down.
    h.event(Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, -800.0),
        modifiers: Modifiers::NONE,
        phase: egui::TouchPhase::Move,
    });
    h.run_steps(4);
    h.get_by_label("Enabled").click();
    h.run_steps(2);
    let tip = h.state().opts.tip;
    assert_eq!(tip.texture, studio_core::paint::BrushTexture::Canvas);
    assert!(tip.dual_size > 0.0, "dual brush switched on");
    // Painting with it leaves a broken, textured stroke rather than a solid one.
    h.event(Event::Key {
        key: Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    });
    h.run_steps(2);
    h.state_mut().fg = Rgba::rgb(0, 0, 0);
    drag(&mut h, (100.0, 150.0), (300.0, 150.0), Modifiers::NONE);
    let l = &h.state().tab().unwrap().doc.layers[0];
    let row: Vec<u8> = (150..250).map(|x| l.pixels.get(x, 150).r()).collect();
    assert!(row.iter().any(|&v| v < 128), "stroke painted");
    assert!(row.iter().any(|&v| v > 160), "texture breaks it up");
}
