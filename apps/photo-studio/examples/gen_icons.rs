//! Regenerate every rendition of the logo:
//! `cargo run -p photo-studio --example gen_icons`

#[allow(dead_code)]
#[path = "../src/logo.rs"]
mod logo;

use std::path::Path;

fn png(path: &Path, size: u32, phase: f32, bg: bool) {
    let img = image::RgbaImage::from_raw(size, size, logo::rgba(size, phase, bg)).unwrap();
    img.save(path).unwrap();
}

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let icons = root.join("packaging/icons");
    std::fs::create_dir_all(&icons).unwrap();
    for s in [16, 32, 48, 64, 128, 256, 512, 1024] {
        png(&icons.join(format!("{s}.png")), s, logo::ICON_PHASE, true);
    }

    // Windows .ico with the standard sizes.
    let frames: Vec<_> = [16u32, 24, 32, 48, 64, 128, 256]
        .iter()
        .map(|&s| {
            image::codecs::ico::IcoFrame::as_png(
                &logo::rgba(s, logo::ICON_PHASE, true),
                s,
                s,
                image::ExtendedColorType::Rgba8,
            )
            .unwrap()
        })
        .collect();
    let f = std::fs::File::create(icons.join("photo-studio.ico")).unwrap();
    image::codecs::ico::IcoEncoder::new(f).encode_images(&frames).unwrap();

    // macOS iconset (packaging/macos/build.sh turns it into AppIcon.icns with iconutil).
    let set = root.join("packaging/macos/AppIcon.iconset");
    std::fs::create_dir_all(&set).unwrap();
    for s in [16u32, 32, 128, 256, 512] {
        png(&set.join(format!("icon_{s}x{s}.png")), s, logo::ICON_PHASE, true);
        png(&set.join(format!("icon_{s}x{s}@2x.png")), s * 2, logo::ICON_PHASE, true);
    }

    let site = root.join("website/assets");
    std::fs::create_dir_all(&site).unwrap();
    std::fs::write(site.join("logo-animated.svg"), logo::svg(true, true)).unwrap();
    std::fs::write(site.join("logo.svg"), logo::svg(false, true)).unwrap();
    std::fs::write(site.join("iris-animated.svg"), logo::svg(true, false)).unwrap();
    png(&site.join("favicon-32.png"), 32, logo::ICON_PHASE, true);
    png(&site.join("apple-touch-icon.png"), 180, logo::ICON_PHASE, true);
    png(&site.join("og-icon.png"), 512, logo::ICON_PHASE, true);
    println!("icons written under {}", root.display());
}
