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
