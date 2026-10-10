//! Reading and writing images, layered projects (.pstudio) and Photoshop documents (.psd).

use std::fs::File;
use std::io::{BufWriter, Cursor, Read, Write};
use std::path::Path;

use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader};
use serde::{Deserialize, Serialize};

use crate::blend::BlendMode;
use crate::color::Rgba;
use crate::document::Document;
use crate::image::{Image, Mask};
use crate::layer::{Layer, LayerId, LayerKind};
use crate::styles::LayerStyles;

#[derive(Debug, thiserror::Error)]
pub enum IoError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Image(#[from] image::ImageError),
    #[error("project file: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("project manifest: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Photoshop file: {0}")]
    Psd(String),
    #[error("unsupported file type: {0}")]
    Unsupported(String),
}

pub type Result<T> = std::result::Result<T, IoError>;

/// File extensions the app can open.
pub const OPEN_EXTENSIONS: &[&str] = &[
    "pstudio", "psd", "ora", "svg", "png", "jpg", "jpeg", "webp", "bmp", "tif", "tiff", "gif", "tga", "ico", "qoi",
    "ppm", "pgm", "pnm", "exr", "hdr",
];

/// Flat formats the app can export to.
pub const EXPORT_EXTENSIONS: &[&str] = &["png", "jpg", "webp", "bmp", "tiff", "gif", "tga", "ico", "qoi", "ppm", "exr"];

pub const PROJECT_EXTENSION: &str = "pstudio";

#[derive(Clone, Copy, Debug)]
pub struct ExportOptions {
    /// 1..100 for JPEG.
    pub jpeg_quality: u8,
    /// Backdrop for formats without alpha.
    pub matte: Rgba,
}

impl Default for ExportOptions {
    fn default() -> Self {
        ExportOptions { jpeg_quality: 90, matte: Rgba::WHITE }
    }
}

fn ext(path: &Path) -> String {
    path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase()
}

/// Decode any supported raster file, honouring EXIF orientation.
pub fn read_image(path: &Path) -> Result<Image> {
    let reader = ImageReader::open(path)?.with_guessed_format()?;
    let mut decoder = reader.into_decoder()?;
    let orientation = decoder.orientation()?;
    let mut img = DynamicImage::from_decoder(decoder)?;
    img.apply_orientation(orientation);
    Ok(Image::from_rgba_image(img.into_rgba8()))
}

pub fn decode_image_bytes(bytes: &[u8]) -> Result<Image> {
    let img = image::load_from_memory(bytes)?;
    Ok(Image::from_rgba_image(img.into_rgba8()))
}

/// Open any supported file as a document.
pub fn open(path: &Path) -> Result<Document> {
    match ext(path).as_str() {
        "pstudio" => load_project(path),
        "psd" | "psb" => read_psd(&std::fs::read(path)?),
        "ora" => crate::formats::read_ora(&std::fs::read(path)?),
        "svg" => {
            let img = crate::formats::rasterize_svg(&std::fs::read(path)?, 8192)?;
            let mut doc = Document::from_image(img);
            if let Some(stem) = path.file_stem() {
                doc.layers[0].name = stem.to_string_lossy().into_owned();
            }
            Ok(doc)
        }
        _ => {
            let img = read_image(path)?;
            Ok(Document::from_image(img))
        }
    }
}

fn over_matte(img: &Image, matte: Rgba) -> Image {
    let mut bg = Image::filled(img.width(), img.height(), matte);
    crate::paint::composite_onto(&mut bg, img, BlendMode::Normal, 1.0, None);
    bg
}

/// Encode a flat image to bytes in the format implied by `extension`.
pub fn encode(img: &Image, extension: &str, opts: &ExportOptions) -> Result<Vec<u8>> {
    let mut out = Cursor::new(Vec::new());
    let e = extension.to_ascii_lowercase();
    match e.as_str() {
        "jpg" | "jpeg" => {
            let rgb = DynamicImage::ImageRgba8(over_matte(img, opts.matte).to_rgba_image()).into_rgb8();
            let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, opts.jpeg_quality.clamp(1, 100));
            rgb.write_with_encoder(enc)?;
        }
        "bmp" | "ppm" | "pnm" => {
            let rgb = DynamicImage::ImageRgba8(over_matte(img, opts.matte).to_rgba_image()).into_rgb8();
            let fmt = if e == "bmp" { ImageFormat::Bmp } else { ImageFormat::Pnm };
            rgb.write_to(&mut out, fmt)?;
        }
        "exr" => {
            let f = DynamicImage::ImageRgba8(img.to_rgba_image()).into_rgba32f();
            f.write_to(&mut out, ImageFormat::OpenExr)?;
        }
        "ico" => {
            let max = img.width().max(img.height());
            let src = if max > 256 {
                let s = 256.0 / max as f32;
                crate::transform::resize(
                    img,
                    ((img.width() as f32 * s).round() as u32).max(1),
                    ((img.height() as f32 * s).round() as u32).max(1),
                    crate::transform::Resample::Lanczos,
                )
            } else {
                img.clone()
            };
            src.to_rgba_image().write_to(&mut out, ImageFormat::Ico)?;
        }
        other => {
            let fmt = ImageFormat::from_extension(other).ok_or_else(|| IoError::Unsupported(other.to_string()))?;
            img.to_rgba_image().write_to(&mut out, fmt)?;
        }
    }
    Ok(out.into_inner())
}

/// Export the flattened document to a raster file.
pub fn export(doc: &Document, path: &Path, opts: &ExportOptions) -> Result<()> {
    let e = ext(path);
    if e == "psd" {
        return save_psd(doc, path);
    }
    if e == "ora" {
        return write_atomic(path, &crate::formats::ora_bytes(doc)?);
    }
    let bytes = encode(&doc.composite(), &e, opts)?;
    write_atomic(path, &bytes)
}

/// Write via a temporary file and rename, so a failed save never truncates the original.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_extension(format!("{}.tmp", ext(path)));
    {
        let mut f = BufWriter::new(File::create(&tmp)?);
        f.write_all(bytes)?;
        f.flush()?;
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Native project format: a zip with a JSON manifest and one PNG per layer and mask.

#[derive(Serialize, Deserialize)]
struct Manifest {
    app: String,
    version: u32,
    width: u32,
    height: u32,
    ppi: f32,
    active: usize,
    layers: Vec<LayerMeta>,
    #[serde(default)]
    guides: Vec<crate::document::Guide>,
    #[serde(default)]
    paths: Vec<crate::path::NamedPath>,
    #[serde(default)]
    channels: Vec<String>,
}

#[derive(Serialize, Deserialize)]
struct LayerMeta {
    id: LayerId,
    name: String,
    visible: bool,
    locked: bool,
    opacity: f32,
    blend: BlendMode,
    kind: LayerKind,
    has_pixels: bool,
    has_mask: bool,
    mask_enabled: bool,
    clip: bool,
    styles: LayerStyles,
    #[serde(default)]
    parent: Option<LayerId>,
    #[serde(default = "one")]
    fill_opacity: f32,
    #[serde(default)]
    label: Option<u8>,
}

fn one() -> f32 {
    1.0
}

pub(crate) fn png_bytes(img: &image::DynamicImage) -> Result<Vec<u8>> {
    let mut c = Cursor::new(Vec::new());
    img.write_to(&mut c, ImageFormat::Png)?;
    Ok(c.into_inner())
}

pub fn project_bytes(doc: &Document) -> Result<Vec<u8>> {
    let mut zw = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let deflated = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let manifest = Manifest {
        app: "photo-studio".into(),
        version: 1,
        width: doc.width,
        height: doc.height,
        ppi: doc.ppi,
        active: doc.active,
        guides: doc.guides.clone(),
        paths: doc.paths.clone(),
        channels: doc.channels.iter().map(|(n, _)| n.clone()).collect(),
        layers: doc
            .layers
            .iter()
            .map(|l| LayerMeta {
                id: l.id,
                name: l.name.clone(),
                visible: l.visible,
                locked: l.locked,
                opacity: l.opacity,
                blend: l.blend,
                kind: l.kind.clone(),
                has_pixels: matches!(l.kind, LayerKind::Raster),
                has_mask: l.mask.is_some(),
                mask_enabled: l.mask_enabled,
                clip: l.clip,
                styles: l.styles.clone(),
                parent: l.parent,
                fill_opacity: l.fill_opacity,
                label: l.label,
            })
            .collect(),
    };
    zw.start_file("manifest.json", deflated)?;
    zw.write_all(&serde_json::to_vec_pretty(&manifest)?)?;
    for l in &doc.layers {
        if matches!(l.kind, LayerKind::Raster) {
            zw.start_file(format!("layers/{}.png", l.id), stored)?;
            zw.write_all(&png_bytes(&DynamicImage::ImageRgba8(l.pixels.to_rgba_image()))?)?;
        }
        if let Some(src) = &l.source {
            zw.start_file(format!("sources/{}.png", l.id), stored)?;
            zw.write_all(&png_bytes(&DynamicImage::ImageRgba8(src.to_rgba_image()))?)?;
        }
        if let Some(m) = &l.mask {
            let g = image::GrayImage::from_raw(m.width(), m.height(), m.data().to_vec()).expect("mask size");
            zw.start_file(format!("masks/{}.png", l.id), stored)?;
            zw.write_all(&png_bytes(&DynamicImage::ImageLuma8(g))?)?;
        }
    }
    for (i, (_, m)) in doc.channels.iter().enumerate() {
        let g = image::GrayImage::from_raw(m.width(), m.height(), m.data().to_vec()).expect("mask size");
        zw.start_file(format!("channels/{i}.png"), stored)?;
        zw.write_all(&png_bytes(&DynamicImage::ImageLuma8(g))?)?;
    }
    // A flattened preview lets other tools show a thumbnail.
    zw.start_file("preview.png", stored)?;
    let comp = doc.composite();
    let s = 512.0 / comp.width().max(comp.height()).max(1) as f32;
    let thumb = if s < 1.0 {
        crate::transform::resize(
            &comp,
            ((comp.width() as f32 * s) as u32).max(1),
            ((comp.height() as f32 * s) as u32).max(1),
            crate::transform::Resample::Bilinear,
        )
    } else {
        comp
    };
    zw.write_all(&png_bytes(&DynamicImage::ImageRgba8(thumb.to_rgba_image()))?)?;
    Ok(zw.finish()?.into_inner())
}

pub fn save_project(doc: &Document, path: &Path) -> Result<()> {
    write_atomic(path, &project_bytes(doc)?)
}

pub fn load_project(path: &Path) -> Result<Document> {
    project_from_bytes(&std::fs::read(path)?)
}

pub fn project_from_bytes(bytes: &[u8]) -> Result<Document> {
    let mut za = zip::ZipArchive::new(Cursor::new(bytes))?;
    let mut s = String::new();
    za.by_name("manifest.json")?.read_to_string(&mut s)?;
    let m: Manifest = serde_json::from_str(&s)?;
    let mut doc = Document::new(m.width, m.height, None);
    doc.layers.clear();
    doc.ppi = m.ppi;
    doc.guides = m.guides;
    doc.paths = m.paths;
    let mut read_png = |name: String| -> Result<Option<DynamicImage>> {
        match za.by_name(&name) {
            Ok(mut f) => {
                let mut b = Vec::new();
                f.read_to_end(&mut b)?;
                Ok(Some(image::load_from_memory_with_format(&b, ImageFormat::Png)?))
            }
            Err(zip::result::ZipError::FileNotFound) => Ok(None),
            Err(e) => Err(e.into()),
        }
    };
    for meta in m.layers {
        let mut layer = Layer::with_kind(meta.id, meta.name, meta.kind, m.width, m.height);
        if meta.has_pixels {
            if let Some(img) = read_png(format!("layers/{}.png", meta.id))? {
                let img = Image::from_rgba_image(img.into_rgba8());
                if img.size() == (m.width, m.height) {
                    layer.pixels = img;
                }
            }
        }
        if matches!(layer.kind, LayerKind::Smart { .. }) {
            if let Some(img) = read_png(format!("sources/{}.png", meta.id))? {
                layer.source = Some(Image::from_rgba_image(img.into_rgba8()));
                layer.render(m.width, m.height);
            }
        }
        if meta.has_mask {
            if let Some(img) = read_png(format!("masks/{}.png", meta.id))? {
                let g = img.into_luma8();
                if g.dimensions() == (m.width, m.height) {
                    layer.mask = Some(Mask::from_raw(m.width, m.height, g.into_raw()));
                }
            }
        }
        layer.visible = meta.visible;
        layer.locked = meta.locked;
        layer.opacity = meta.opacity;
        layer.blend = meta.blend;
        layer.mask_enabled = meta.mask_enabled;
        layer.clip = meta.clip;
        layer.styles = meta.styles;
        layer.parent = meta.parent;
        layer.fill_opacity = meta.fill_opacity;
        layer.label = meta.label;
        doc.next_id = doc.next_id.max(meta.id + 1);
        doc.layers.push(layer);
    }
    for (i, name) in m.channels.iter().enumerate() {
        if let Some(img) = read_png(format!("channels/{i}.png"))? {
            let g = img.into_luma8();
            if g.dimensions() == (m.width, m.height) {
                doc.channels.push((name.clone(), Mask::from_raw(m.width, m.height, g.into_raw())));
            }
        }
    }
    if doc.layers.is_empty() {
        let id = doc.new_id();
        doc.layers.push(Layer::raster(id, "Layer 1", Image::new(m.width, m.height)));
    }
    doc.active = m.active.min(doc.layers.len() - 1);
    Ok(doc)
}

// ---------------------------------------------------------------------------------------------
// Photoshop documents.

pub fn read_psd(bytes: &[u8]) -> Result<Document> {
    let psd = psd::Psd::from_bytes(bytes).map_err(|e| IoError::Psd(e.to_string()))?;
    let (w, h) = (psd.width(), psd.height());
    let mut doc = Document::new(w, h, None);
    doc.layers.clear();
    // Groups first get ids, so layers can point at them.
    let mut group_ids = std::collections::HashMap::new();
    for gid in psd.group_ids_in_order() {
        let id = doc.new_id();
        group_ids.insert(*gid, id);
    }
    // The psd crate lists layers top to bottom; the document stacks bottom to top.
    for pl in psd.layers().iter().rev() {
        let rgba = pl.rgba();
        if rgba.len() != (w * h * 4) as usize {
            continue;
        }
        let id = doc.new_id();
        let mut l = Layer::raster(id, pl.name(), Image::from_raw(w, h, rgba));
        // psd 0.3 reports the flag bit that Photoshop sets for *hidden* layers as "visible".
        l.visible = !pl.visible();
        l.opacity = pl.opacity() as f32 / 255.0;
        l.blend = BlendMode::from_name(&format!("{:?}", pl.blend_mode())).unwrap_or_default();
        l.clip = pl.is_clipping_mask();
        l.parent = pl.parent_id().and_then(|p| group_ids.get(&p).copied());
        doc.layers.push(l);
    }
    // Place each group directly above its topmost member, deepest groups first.
    let groups = psd.groups();
    let depth = |mut g: u32| {
        let mut d = 0;
        while let Some(p) = groups.get(&g).and_then(|x| x.parent_id()) {
            d += 1;
            g = p;
            if d > 64 {
                break;
            }
        }
        d
    };
    let mut order: Vec<u32> = psd.group_ids_in_order().clone();
    order.sort_by_key(|g| std::cmp::Reverse(depth(*g)));
    for gid in order {
        let (Some(g), Some(&our)) = (groups.get(&gid), group_ids.get(&gid)) else { continue };
        let Some(last) = (0..doc.layers.len()).rev().find(|&i| doc.is_descendant(i, our)) else { continue };
        let blend = format!("{:?}", g.blend_mode());
        let pass_through = blend.eq_ignore_ascii_case("PassThrough");
        let mut l = Layer::with_kind(our, g.name(), LayerKind::Group { pass_through, collapsed: false }, w, h);
        l.visible = !g.visible();
        l.opacity = g.opacity() as f32 / 255.0;
        l.blend = if pass_through { BlendMode::Normal } else { BlendMode::from_name(&blend).unwrap_or_default() };
        l.parent = g.parent_id().and_then(|p| group_ids.get(&p).copied());
        doc.layers.insert(last + 1, l);
    }
    if doc.layers.is_empty() {
        // No layer records: use the merged image.
        let id = doc.new_id();
        doc.layers.push(Layer::raster(id, "Background", Image::from_raw(w, h, psd.rgba())));
    }
    doc.active = doc.layers.len() - 1;
    Ok(doc)
}

fn psd_blend_key(m: BlendMode) -> &'static [u8; 4] {
    match m {
        BlendMode::Normal => b"norm",
        BlendMode::Dissolve => b"diss",
        BlendMode::Darken => b"dark",
        BlendMode::Multiply => b"mul ",
        BlendMode::ColorBurn => b"idiv",
        BlendMode::LinearBurn => b"lbrn",
        BlendMode::DarkerColor => b"dkCl",
        BlendMode::Lighten => b"lite",
        BlendMode::Screen => b"scrn",
        BlendMode::ColorDodge => b"div ",
        BlendMode::LinearDodge => b"lddg",
        BlendMode::LighterColor => b"lgCl",
        BlendMode::Overlay => b"over",
        BlendMode::SoftLight => b"sLit",
        BlendMode::HardLight => b"hLit",
        BlendMode::VividLight => b"vLit",
        BlendMode::LinearLight => b"lLit",
        BlendMode::PinLight => b"pLit",
        BlendMode::HardMix => b"hMix",
        BlendMode::Difference => b"diff",
        BlendMode::Exclusion => b"smud",
        BlendMode::Subtract => b"fsub",
        BlendMode::Divide => b"fdiv",
        BlendMode::Hue => b"hue ",
        BlendMode::Saturation => b"sat ",
        BlendMode::Color => b"colr",
        BlendMode::Luminosity => b"lum ",
    }
}

/// Write an uncompressed, layered PSD. Text, shape and fill layers are written as pixels,
/// masks are baked in; adjustment layers show up only in the merged image.
pub fn psd_bytes(doc: &Document) -> Result<Vec<u8>> {
    let (w, h) = (doc.width, doc.height);
    if w > 30_000 || h > 30_000 {
        return Err(IoError::Psd("PSD is limited to 30,000 pixels per side".into()));
    }
    let mut b: Vec<u8> = Vec::new();
    let be16 = |v: &mut Vec<u8>, x: u16| v.extend_from_slice(&x.to_be_bytes());
    let be32 = |v: &mut Vec<u8>, x: u32| v.extend_from_slice(&x.to_be_bytes());

    // Header.
    b.extend_from_slice(b"8BPS");
    be16(&mut b, 1);
    b.extend_from_slice(&[0; 6]);
    be16(&mut b, 4); // channels in the merged image: RGBA
    be32(&mut b, h);
    be32(&mut b, w);
    be16(&mut b, 8);
    be16(&mut b, 3); // RGB
    be32(&mut b, 0); // colour mode data
    be32(&mut b, 0); // image resources

    // Layer info. Groups become Photoshop section records: a divider below the children and
    // the group record (open/closed folder) above them.
    enum Rec<'a> {
        Pixels(&'a Layer),
        /// Closes a group; carries the group's settings too, which some readers use.
        Divider(&'a Layer),
        Group(&'a Layer, bool),
    }
    let mut recs: Vec<Rec> = Vec::new();
    for (i, l) in doc.layers.iter().enumerate() {
        // Dividers for every group whose block starts here, outermost first.
        let mut opening: Vec<(usize, &Layer)> = doc
            .layers
            .iter()
            .enumerate()
            .filter(|(gi, g)| g.is_group() && doc.block(*gi).start == i && doc.block(*gi).len() > 1)
            .map(|(gi, g)| (doc.block(gi).len(), g))
            .collect();
        opening.sort_by_key(|(len, _)| std::cmp::Reverse(*len));
        for (_, g) in opening {
            recs.push(Rec::Divider(g));
        }
        match &l.kind {
            LayerKind::Group { collapsed, .. } if doc.block(i).len() > 1 => recs.push(Rec::Group(l, *collapsed)),
            _ if l.pixels.size() == (w, h) => recs.push(Rec::Pixels(l)),
            _ => {}
        }
    }
    let mut records = Vec::new();
    let mut channels = Vec::new();
    for rec in &recs {
        let (px, name, blend, opacity, clip, visible, section): (
            Image,
            String,
            BlendMode,
            f32,
            bool,
            bool,
            Option<(u32, &[u8; 4])>,
        ) = match rec {
            Rec::Pixels(l) => {
                let mut px = l.pixels.clone();
                if let Some(m) = l.active_mask() {
                    let d = m.data();
                    for (i, p) in px.pixels_mut().chunks_exact_mut(4).enumerate() {
                        p[3] = ((p[3] as u32 * d[i] as u32 + 127) / 255) as u8;
                    }
                }
                (px, l.name.clone(), l.blend, l.opacity, l.clip, l.visible, None)
            }
            Rec::Divider(g) => (
                Image::new(0, 0),
                "</Layer group>".into(),
                g.blend,
                g.opacity,
                false,
                g.visible,
                Some((3, psd_blend_key(g.blend))),
            ),
            Rec::Group(l, collapsed) => {
                let key: &[u8; 4] = match l.kind {
                    LayerKind::Group { pass_through: true, .. } => b"pass",
                    _ => psd_blend_key(l.blend),
                };
                (
                    Image::new(0, 0),
                    l.name.clone(),
                    l.blend,
                    l.opacity,
                    false,
                    l.visible,
                    Some((if *collapsed { 2 } else { 1 }, key)),
                )
            }
        };
        let r = px.opaque_bounds().unwrap_or(crate::geom::Rect::new(0, 0, 0, 0));
        let crop = px.crop(r);
        let n = (r.w * r.h) as usize;
        be32(&mut records, r.y as u32);
        be32(&mut records, r.x as u32);
        be32(&mut records, r.bottom() as u32);
        be32(&mut records, r.right() as u32);
        be16(&mut records, 4);
        for cid in [-1i16, 0, 1, 2] {
            records.extend_from_slice(&cid.to_be_bytes());
            be32(&mut records, 2 + n as u32);
        }
        records.extend_from_slice(b"8BIM");
        records.extend_from_slice(match section {
            Some((_, b"pass")) => b"pass",
            _ => psd_blend_key(blend),
        });
        records.push((opacity.clamp(0.0, 1.0) * 255.0).round() as u8);
        records.push(u8::from(clip));
        records.push(if visible { 0 } else { 2 });
        records.push(0);
        let name: Vec<u8> = name.chars().filter(|c| c.is_ascii()).take(255).map(|c| c as u8).collect();
        let mut pascal = vec![name.len() as u8];
        pascal.extend_from_slice(&name);
        while pascal.len() % 4 != 0 {
            pascal.push(0);
        }
        let mut extra = Vec::new();
        if let Some((kind, key)) = section {
            extra.extend_from_slice(b"8BIMlsct");
            be32(&mut extra, 12);
            be32(&mut extra, kind);
            extra.extend_from_slice(b"8BIM");
            extra.extend_from_slice(key);
        }
        be32(&mut records, 4 + 4 + pascal.len() as u32 + extra.len() as u32);
        be32(&mut records, 0); // mask data
        be32(&mut records, 0); // blending ranges
        records.extend_from_slice(&pascal);
        records.extend_from_slice(&extra);
        for c in [3usize, 0, 1, 2] {
            be16(&mut channels, 0);
            channels.extend(crop.pixels().chunks_exact(4).map(|p| p[c]));
        }
    }
    let layers = &recs;
    let mut layer_info = Vec::new();
    be16(&mut layer_info, layers.len() as u16);
    layer_info.extend_from_slice(&records);
    layer_info.extend_from_slice(&channels);
    if layer_info.len() % 2 == 1 {
        layer_info.push(0);
    }
    be32(&mut b, layer_info.len() as u32 + 4 + 4);
    be32(&mut b, layer_info.len() as u32);
    b.extend_from_slice(&layer_info);
    be32(&mut b, 0); // global layer mask info

    // Merged image, planar and uncompressed.
    be16(&mut b, 0);
    let comp = doc.composite();
    for c in 0..4 {
        b.extend(comp.pixels().chunks_exact(4).map(|p| p[c]));
    }
    Ok(b)
}

pub fn save_psd(doc: &Document, path: &Path) -> Result<()> {
    write_atomic(path, &psd_bytes(doc)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adjust::Adjustment;
    use crate::text::TextLayer;

    fn sample_doc() -> Document {
        let mut d = Document::new(16, 12, Some(Rgba::rgb(20, 40, 60)));
        d.add_empty_layer("Paint");
        d.layers[1].pixels.put(3, 4, Rgba::rgb(255, 0, 0));
        d.layers[1].blend = BlendMode::Screen;
        d.layers[1].opacity = 0.5;
        d.add_mask(true);
        d.add_layer_of_kind("Title", LayerKind::Text(TextLayer::new("Hi", 1.0, 1.0, 8.0, Rgba::WHITE)));
        d.add_layer_of_kind("Invert", LayerKind::Adjustment(Adjustment::Invert));
        d.layers[2].styles.drop_shadow.enabled = true;
        d.guides.push(crate::document::Guide { horizontal: true, pos: 5.0 });
        d.channels.push(("Alpha 1".into(), crate::select::rect_mask(16, 12, crate::geom::Rect::new(1, 1, 4, 4))));
        d
    }

    #[test]
    fn project_round_trip() {
        let d = sample_doc();
        let bytes = project_bytes(&d).unwrap();
        let back = project_from_bytes(&bytes).unwrap();
        assert_eq!(back.layers.len(), d.layers.len());
        for (a, b) in d.layers.iter().zip(&back.layers) {
            assert_eq!(a.name, b.name);
            assert_eq!(a.kind, b.kind);
            assert_eq!(a.blend, b.blend);
            assert_eq!(a.pixels, b.pixels);
            assert_eq!(a.mask, b.mask);
            assert_eq!(a.styles, b.styles);
        }
        assert_eq!(back.composite(), d.composite());
        assert_eq!(back.guides, d.guides);
        assert_eq!(back.channels, d.channels);
    }

    #[test]
    fn psd_round_trip() {
        let d = sample_doc();
        let bytes = psd_bytes(&d).unwrap();
        let back = read_psd(&bytes).unwrap();
        assert_eq!((back.width, back.height), (16, 12));
        // Adjustment layers are not written as layers.
        assert_eq!(back.layers.len(), 3);
        let names: Vec<&str> = back.layers.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["Background", "Paint", "Title"]);
        assert!(back.layers.iter().all(|l| l.visible));
        assert_eq!(back.layers[1].blend, BlendMode::Screen);
        assert_eq!(back.layers[1].pixels.get(3, 4), Rgba::rgb(255, 0, 0));
    }

    #[test]
    fn psd_groups_round_trip() {
        let mut d = Document::new(8, 8, Some(Rgba::WHITE));
        d.add_empty_layer("Inner");
        d.layers[1].pixels.put(2, 2, Rgba::rgb(255, 0, 0));
        d.group_active("Folder");
        d.layers[2].opacity = 0.5;
        if let LayerKind::Group { pass_through, .. } = &mut d.layers[2].kind {
            *pass_through = false;
        }
        let bytes = psd_bytes(&d).unwrap();
        let back = read_psd(&bytes).unwrap();
        let names: Vec<&str> = back.layers.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["Background", "Inner", "Folder"]);
        assert!(back.layers[2].is_group());
        assert_eq!(back.layers[1].parent, Some(back.layers[2].id));
        assert!((back.layers[2].opacity - 0.5).abs() < 0.01);
        assert!(back.layers.iter().all(|l| l.visible));
        // PSD stores opacity in 8 bits, so allow one level of rounding.
        let (a, b) = (back.composite(), d.composite());
        assert!(a.pixels().iter().zip(b.pixels()).all(|(x, y)| x.abs_diff(*y) <= 1));
    }

    #[test]
    fn encodes_every_export_format() {
        let img = Image::filled(8, 8, Rgba::rgba(10, 20, 30, 200));
        for e in EXPORT_EXTENSIONS {
            let bytes = encode(&img, e, &ExportOptions::default()).unwrap_or_else(|err| panic!("{e}: {err}"));
            let fmt = ImageFormat::from_extension(e).unwrap();
            let back = image::load_from_memory_with_format(&bytes, fmt).unwrap_or_else(|err| panic!("{e}: {err}"));
            assert_eq!(back.width(), 8, "{e}");
        }
    }
}
