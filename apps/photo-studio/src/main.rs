//! Photo Studio desktop app.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod actions;
mod app;
mod canvas;
mod dialogs;
mod doc;
mod logo;
mod menus;
mod panels;
mod params;
mod shell;
mod theme;
mod tools;
#[cfg(test)]
mod ui_tests;
mod welcome;
mod widgets;

use std::path::PathBuf;

fn main() -> eframe::Result {
    env_logger::init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("photo-studio {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    let files: Vec<PathBuf> = args.iter().filter(|a| !a.starts_with('-')).map(PathBuf::from).collect();

    let icon = egui::IconData { rgba: logo::rgba(256, logo::ICON_PHASE, true), width: 256, height: 256 };
    // wgpu by default; PHOTO_STUDIO_RENDERER=glow for machines without a modern GPU driver.
    let renderer = match std::env::var("PHOTO_STUDIO_RENDERER").as_deref() {
        Ok("glow") | Ok("gl") | Ok("opengl") => eframe::Renderer::Glow,
        _ => eframe::Renderer::Wgpu,
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Photo Studio")
            .with_app_id("photo-studio")
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([960.0, 600.0])
            .with_icon(icon)
            .with_drag_and_drop(true),
        renderer,
        persist_window: true,
        ..Default::default()
    };
    eframe::run_native("Photo Studio", options, Box::new(move |cc| Ok(Box::new(app::App::new(cc, files)))))
}
