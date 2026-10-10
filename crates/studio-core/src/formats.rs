//! OpenRaster (Krita, GIMP, MyPaint) and SVG.

use std::io::{Cursor, Read, Write};

use image::DynamicImage;

use crate::blend::BlendMode;
use crate::composite::Tree;
use crate::document::Document;
use crate::image::Image;
use crate::io::{IoError, Result, png_bytes};
use crate::layer::{Layer, LayerId, LayerKind};

/// Render an SVG at its natural size, scaled down to fit `max_side` if needed.
pub fn rasterize_svg(bytes: &[u8], max_side: u32) -> Result<Image> {
    use resvg::{tiny_skia as ts, usvg};
    let mut opt = usvg::Options::default();
    opt.fontdb_mut().load_system_fonts();
    let tree = usvg::Tree::from_data(bytes, &opt).map_err(|e| IoError::Unsupported(format!("SVG: {e}")))?;
    let size = tree.size();
    let scale = (max_side as f32 / size.width().max(size.height())).clamp(1e-3, 1.0);
    let (w, h) = (((size.width() * scale).ceil() as u32).max(1), ((size.height() * scale).ceil() as u32).max(1));
    let mut pm = ts::Pixmap::new(w, h).ok_or_else(|| IoError::Unsupported("SVG has no size".into()))?;
    resvg::render(&tree, ts::Transform::from_scale(scale, scale), &mut pm.as_mut());
    let mut v = Vec::with_capacity((w * h * 4) as usize);
    for p in pm.pixels() {
        let c = p.demultiply();
        v.extend_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
    }
    Ok(Image::from_raw(w, h, v))
}

fn ora_op(m: BlendMode) -> &'static str {
    match m {
        BlendMode::Multiply => "svg:multiply",
        BlendMode::Screen => "svg:screen",
        BlendMode::Overlay => "svg:overlay",
        BlendMode::Darken => "svg:darken",
        BlendMode::Lighten => "svg:lighten",
        BlendMode::ColorDodge => "svg:color-dodge",
        BlendMode::ColorBurn => "svg:color-burn",
        BlendMode::HardLight => "svg:hard-light",
        BlendMode::SoftLight => "svg:soft-light",
        BlendMode::Difference => "svg:difference",
        BlendMode::Exclusion => "svg:exclusion",
        BlendMode::Hue => "svg:hue",
        BlendMode::Saturation => "svg:saturation",
        BlendMode::Color => "svg:color",
        BlendMode::Luminosity => "svg:luminosity",
        BlendMode::LinearDodge => "svg:plus",
        _ => "svg:src-over",
    }
}

fn ora_mode(op: &str) -> BlendMode {
    match op.trim_start_matches("svg:").trim_start_matches("krita:") {
        "multiply" => BlendMode::Multiply,
        "screen" => BlendMode::Screen,
        "overlay" => BlendMode::Overlay,
        "darken" => BlendMode::Darken,
        "lighten" => BlendMode::Lighten,
        "color-dodge" => BlendMode::ColorDodge,
        "color-burn" => BlendMode::ColorBurn,
        "hard-light" => BlendMode::HardLight,
        "soft-light" => BlendMode::SoftLight,
        "difference" => BlendMode::Difference,
        "exclusion" => BlendMode::Exclusion,
        "hue" => BlendMode::Hue,
        "saturation" => BlendMode::Saturation,
        "color" => BlendMode::Color,
        "luminosity" => BlendMode::Luminosity,
        "plus" => BlendMode::LinearDodge,
        _ => BlendMode::Normal,
    }
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Write an OpenRaster file. Text, shape and fill layers are written as pixels with masks
/// baked in; adjustment layers have no OpenRaster equivalent and appear only in the merged image.
pub fn ora_bytes(doc: &Document) -> Result<Vec<u8>> {
    let mut zw = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    // The spec requires an uncompressed `mimetype` entry first.
    zw.start_file("mimetype", stored)?;
    zw.write_all(b"image/openraster")?;
    let tree = Tree::new(&doc.layers);
    let mut xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><image version="0.0.3" w="{}" h="{}"><stack>"#,
        doc.width, doc.height
    );
    let mut files: Vec<(String, Image)> = Vec::new();
    emit(doc, &tree, &tree.top, &mut xml, &mut files);
    xml.push_str("</stack></image>");
    let deflated = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    zw.start_file("stack.xml", deflated)?;
    zw.write_all(xml.as_bytes())?;
    for (name, img) in files {
        zw.start_file(name, stored)?;
        zw.write_all(&png_bytes(&DynamicImage::ImageRgba8(img.to_rgba_image()))?)?;
    }
    let comp = doc.composite();
    zw.start_file("mergedimage.png", stored)?;
    zw.write_all(&png_bytes(&DynamicImage::ImageRgba8(comp.to_rgba_image()))?)?;
    let s = (256.0 / comp.width().max(comp.height()).max(1) as f32).min(1.0);
    let thumb = crate::transform::resize(
        &comp,
        ((comp.width() as f32 * s) as u32).max(1),
        ((comp.height() as f32 * s) as u32).max(1),
        crate::transform::Resample::Bilinear,
    );
    zw.start_file("Thumbnails/thumbnail.png", stored)?;
    zw.write_all(&png_bytes(&DynamicImage::ImageRgba8(thumb.to_rgba_image()))?)?;
    Ok(zw.finish()?.into_inner())
}

/// Write `list` (bottom-first indices) top-first, as OpenRaster lists layers.
fn emit(doc: &Document, tree: &Tree, list: &[usize], xml: &mut String, files: &mut Vec<(String, Image)>) {
    for &i in list.iter().rev() {
        let l = &doc.layers[i];
        let vis = if l.visible { "visible" } else { "hidden" };
        if l.is_group() {
            let isolation =
                if matches!(l.kind, LayerKind::Group { pass_through: true, .. }) { "auto" } else { "isolate" };
            xml.push_str(&format!(
                r#"<stack name="{}" opacity="{:.3}" visibility="{vis}" composite-op="{}" isolation="{isolation}">"#,
                esc(&l.name),
                l.opacity,
                ora_op(l.blend)
            ));
            if let Some(children) = tree.children.get(&l.id) {
                emit(doc, tree, children, xml, files);
            }
            xml.push_str("</stack>");
            continue;
        }
        if l.pixels.size() != (doc.width, doc.height) {
            continue;
        }
        let mut px = l.pixels.clone();
        if let Some(m) = l.active_mask() {
            let d = m.data();
            for (k, p) in px.pixels_mut().chunks_exact_mut(4).enumerate() {
                p[3] = ((p[3] as u32 * d[k] as u32 + 127) / 255) as u8;
            }
        }
        let src = format!("data/layer{}.png", l.id);
        xml.push_str(&format!(
            r#"<layer name="{}" src="{src}" x="0" y="0" opacity="{:.3}" visibility="{vis}" composite-op="{}"/>"#,
            esc(&l.name),
            l.opacity,
            ora_op(l.blend)
        ));
        files.push((src, px));
    }
}

struct Item {
    name: String,
    src: Option<String>,
    x: i32,
    y: i32,
    opacity: f32,
    visible: bool,
    blend: BlendMode,
    /// Some(pass_through) for groups.
    group: Option<bool>,
    depth: usize,
}

pub fn read_ora(bytes: &[u8]) -> Result<Document> {
    use quick_xml::events::{BytesStart, Event};
    let mut za = zip::ZipArchive::new(Cursor::new(bytes))?;
    let mut xml = String::new();
    za.by_name("stack.xml")?.read_to_string(&mut xml)?;
    let mut reader = quick_xml::Reader::from_str(&xml);
    let attr = |e: &BytesStart, key: &str| -> Option<String> {
        e.attributes()
            .flatten()
            .find(|a| a.key.as_ref() == key.as_bytes())
            .and_then(|a| a.unescape_value().ok().map(|v| v.into_owned()))
    };
    let num =
        |e: &BytesStart, key: &str, default: f32| attr(e, key).and_then(|v| v.parse::<f32>().ok()).unwrap_or(default);
    let (mut w, mut h) = (0u32, 0u32);
    // Items in file order: top first, each group before its children.
    let mut items: Vec<Item> = Vec::new();
    let mut depth = 0usize;
    loop {
        let ev = reader.read_event().map_err(|e| IoError::Unsupported(format!("OpenRaster XML: {e}")))?;
        let (e, is_start) = match &ev {
            Event::Start(e) => (e, true),
            Event::Empty(e) => (e, false),
            Event::End(e) if e.name().as_ref() == b"stack" => {
                depth = depth.saturating_sub(1);
                continue;
            }
            Event::Eof => break,
            _ => continue,
        };
        match e.name().as_ref() {
            b"image" => {
                w = num(e, "w", 0.0) as u32;
                h = num(e, "h", 0.0) as u32;
            }
            b"stack" => {
                // The outermost stack is the image itself, not a group.
                if depth > 0 {
                    items.push(Item {
                        name: attr(e, "name").unwrap_or_else(|| "Group".into()),
                        src: None,
                        x: 0,
                        y: 0,
                        opacity: num(e, "opacity", 1.0),
                        visible: attr(e, "visibility").as_deref() != Some("hidden"),
                        blend: ora_mode(&attr(e, "composite-op").unwrap_or_default()),
                        group: Some(attr(e, "isolation").as_deref() != Some("isolate")),
                        depth,
                    });
                }
                if is_start {
                    depth += 1;
                }
            }
            b"layer" => items.push(Item {
                name: attr(e, "name").unwrap_or_else(|| "Layer".into()),
                src: attr(e, "src"),
                x: num(e, "x", 0.0) as i32,
                y: num(e, "y", 0.0) as i32,
                opacity: num(e, "opacity", 1.0),
                visible: attr(e, "visibility").as_deref() != Some("hidden"),
                blend: ora_mode(&attr(e, "composite-op").unwrap_or_default()),
                group: None,
                depth,
            }),
            _ => {}
        }
    }
    if w == 0 || h == 0 {
        return Err(IoError::Unsupported("OpenRaster file without a size".into()));
    }
    let mut doc = Document::new(w, h, None);
    doc.layers.clear();
    let ids: Vec<LayerId> = items.iter().map(|_| doc.new_id()).collect();
    // An item's parent is the nearest earlier group one level up.
    let parent_of = |i: usize| -> Option<LayerId> {
        let d = items[i].depth;
        if d <= 1 {
            return None;
        }
        (0..i).rev().find(|&j| items[j].group.is_some() && items[j].depth == d - 1).map(|j| ids[j])
    };
    let mut layers: Vec<Layer> = Vec::new();
    for (i, it) in items.iter().enumerate() {
        let mut l = match it.group {
            Some(pass_through) => {
                Layer::with_kind(ids[i], it.name.clone(), LayerKind::Group { pass_through, collapsed: false }, w, h)
            }
            None => {
                let mut px = Image::new(w, h);
                if let Some(src) = &it.src {
                    if let Ok(mut f) = za.by_name(src) {
                        let mut b = Vec::new();
                        f.read_to_end(&mut b)?;
                        let img = Image::from_rgba_image(image::load_from_memory(&b)?.into_rgba8());
                        px.blit(&img, it.x, it.y);
                    }
                }
                Layer::raster(ids[i], it.name.clone(), px)
            }
        };
        l.opacity = it.opacity.clamp(0.0, 1.0);
        l.visible = it.visible;
        l.blend = it.blend;
        l.parent = parent_of(i);
        layers.push(l);
    }
    order(&layers, None, &mut doc.layers);
    if doc.layers.is_empty() {
        let id = doc.new_id();
        doc.layers.push(Layer::raster(id, "Layer 1", Image::new(w, h)));
    }
    doc.active = doc.layers.len() - 1;
    Ok(doc)
}

/// File order (top first, groups before children) → document order (bottom first, groups
/// after their children).
fn order(layers: &[Layer], parent: Option<LayerId>, out: &mut Vec<Layer>) {
    for l in layers.iter().rev().filter(|l| l.parent == parent) {
        if l.is_group() {
            order(layers, Some(l.id), out);
        }
        out.push(l.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Rgba;

    #[test]
    fn ora_round_trip_with_groups() {
        let mut d = Document::new(16, 12, Some(Rgba::rgb(20, 40, 60)));
        d.add_empty_layer("Paint");
        d.layers[1].pixels.put(3, 4, Rgba::rgb(255, 0, 0));
        d.layers[1].blend = BlendMode::Screen;
        d.layers[1].opacity = 0.5;
        d.group_active("Folder");
        d.add_empty_layer("Top");
        let back = read_ora(&ora_bytes(&d).unwrap()).unwrap();
        let names: Vec<&str> = back.layers.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["Background", "Paint", "Top", "Folder"]);
        assert!(back.layers[3].is_group());
        assert_eq!(back.layers[1].parent, Some(back.layers[3].id));
        assert_eq!(back.layers[2].parent, Some(back.layers[3].id));
        assert_eq!(back.layers[1].blend, BlendMode::Screen);
        assert!((back.layers[1].opacity - 0.5).abs() < 0.01);
        assert_eq!(back.layers[1].pixels.get(3, 4), Rgba::rgb(255, 0, 0));
        assert_eq!(back.composite(), d.composite());
    }

    #[test]
    fn svg_rasterises() {
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20"><rect width="20" height="20" fill="#ff0000"/></svg>"##;
        let img = rasterize_svg(svg, 4096).unwrap();
        assert_eq!(img.size(), (40, 20));
        assert_eq!(img.get(5, 5), Rgba::rgb(255, 0, 0));
        assert_eq!(img.get(30, 5).a(), 0);
    }
}
