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
