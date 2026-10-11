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
    "pstudio", "psd", "psb", "ora", "svg", "png", "jpg", "jpeg", "webp", "bmp", "tif", "tiff", "gif", "tga", "ico",
    "qoi", "ppm", "pgm", "pnm", "exr", "hdr",
];

/// Flat formats the app can export to.
pub const EXPORT_EXTENSIONS: &[&str] =
    &["png", "jpg", "webp", "avif", "bmp", "tiff", "gif", "tga", "ico", "qoi", "ppm", "exr"];

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
        "tif" | "tiff" => {
            let bytes = std::fs::read(path)?;
            match read_layered_tiff(&bytes) {
                Some(doc) => Ok(doc),
                None => Ok(Document::from_image(read_image(path)?)),
            }
        }
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
        "avif" => {
            // Speed 6 balances encode time and size; quality follows the JPEG setting.
            let enc =
                image::codecs::avif::AvifEncoder::new_with_speed_quality(&mut out, 6, opts.jpeg_quality.clamp(1, 100));
            img.to_rgba_image().write_with_encoder(enc)?;
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
    if e == "psb" {
        return save_psb(doc, path);
    }
    if e == "ora" {
        return write_atomic(path, &crate::formats::ora_bytes(doc)?);
    }
    if e == "tif" || e == "tiff" {
        return write_atomic(path, &layered_tiff_bytes(doc)?);
    }
    let bytes = encode(&doc.composite(), &e, opts)?;
    write_atomic(path, &bytes)
}

/// TIFF's UNDEFINED byte type, which Photoshop's layer tag uses.
struct Undefined<'a>(&'a [u8]);

impl tiff::encoder::TiffValue for Undefined<'_> {
    const BYTE_LEN: u8 = 1;
    const FIELD_TYPE: tiff::tags::Type = tiff::tags::Type::UNDEFINED;

    fn count(&self) -> usize {
        self.0.len()
    }

    fn data(&self) -> std::borrow::Cow<'_, [u8]> {
        std::borrow::Cow::Borrowed(self.0)
    }
}

fn tiff_err(e: tiff::TiffError) -> IoError {
    IoError::Image(image::ImageError::Encoding(image::error::EncodingError::new(
        image::error::ImageFormatHint::Exact(ImageFormat::Tiff),
        e,
    )))
}

/// An LZW-compressed RGBA TIFF of the composite, with the layers in Photoshop's private tag so
/// Photoshop (and Photo Studio) reopen it layered while every other reader sees the flat image.
pub fn layered_tiff_bytes(doc: &Document) -> Result<Vec<u8>> {
    use tiff::encoder::{Compression, TiffEncoder, colortype::RGBA8};
    let comp = doc.composite();
    let block = crate::photoshop::tiff_layer_block(doc);
    let mut out = Cursor::new(Vec::new());
    {
        let mut enc = TiffEncoder::new(&mut out).map_err(tiff_err)?.with_compression(Compression::Lzw);
        let mut img = enc.new_image::<RGBA8>(doc.width, doc.height).map_err(tiff_err)?;
        if doc.layers.len() > 1 || doc.layers.iter().any(|l| l.mask.is_some() || l.is_group()) {
            img.encoder()
                .write_tag(tiff::tags::Tag::Unknown(crate::photoshop::TIFF_LAYERS_TAG), Undefined(&block))
                .map_err(tiff_err)?;
        }
        img.write_data(comp.pixels()).map_err(tiff_err)?;
    }
    Ok(out.into_inner())
}

/// Layers stored by Photoshop (or us) in a TIFF, if any.
pub fn read_layered_tiff(bytes: &[u8]) -> Option<Document> {
    let mut dec = tiff::decoder::Decoder::new(Cursor::new(bytes)).ok()?.with_limits(tiff::decoder::Limits::unlimited());
    let (w, h) = dec.dimensions().ok()?;
    let mut buf = tiff::tags::ValueBuffer::empty(tiff::tags::Type::UNDEFINED);
    dec.image_ifd().find_tag_buf(tiff::tags::Tag::Unknown(crate::photoshop::TIFF_LAYERS_TAG), &mut buf).ok()??;
    crate::photoshop::read_tiff_layers(buf.as_bytes(), w, h).ok().flatten()
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
    #[serde(default)]
    annotations: crate::document::Annotations,
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
    #[serde(default)]
    vector_mask: Option<crate::path::BezierPath>,
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
        annotations: doc.annotations.clone(),
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
                vector_mask: l.vector_mask.clone(),
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
    doc.annotations = m.annotations;
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
        layer.vector_mask = meta.vector_mask;
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

/// Read a PSD or PSB. The native reader handles both (and masks, ZIP data, 16-bit); the psd
/// crate is a fallback for anything it rejects.
pub fn read_psd(bytes: &[u8]) -> Result<Document> {
    match crate::photoshop::read(bytes) {
        Ok(doc) => Ok(doc),
        Err(native) => {
            let fallback = std::panic::catch_unwind(|| read_psd_crate(bytes));
            match fallback {
                Ok(Ok(doc)) => Ok(doc),
                _ => Err(native),
            }
        }
    }
}

fn read_psd_crate(bytes: &[u8]) -> Result<Document> {
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

/// Write a layered PSD.
pub fn psd_bytes(doc: &Document) -> Result<Vec<u8>> {
    crate::photoshop::write(doc, false)
}

pub fn save_psd(doc: &Document, path: &Path) -> Result<()> {
    write_atomic(path, &psd_bytes(doc)?)
}

/// Write a layered PSB (Photoshop large document format).
pub fn save_psb(doc: &Document, path: &Path) -> Result<()> {
    write_atomic(path, &crate::photoshop::write(doc, true)?)
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
        d.annotations.notes.push(((3.0, 4.0), "Fix the sky".into()));
        d.annotations.counts.push((7.0, 8.0));
        d.annotations.slices.push(crate::geom::Rect::new(0, 0, 8, 6));
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
        assert_eq!(back.annotations, d.annotations);
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
    fn layered_tiff_round_trip() {
        let mut d = Document::new(20, 10, Some(Rgba::rgb(20, 40, 60)));
        d.add_empty_layer("Top layer");
        d.layers[1].pixels.put(3, 4, Rgba::rgb(255, 0, 0));
        d.layers[1].blend = BlendMode::Screen;
        let bytes = layered_tiff_bytes(&d).unwrap();
        // Any reader sees the composite.
        let flat = image::load_from_memory_with_format(&bytes, ImageFormat::Tiff).unwrap().into_rgba8();
        assert_eq!(flat.get_pixel(3, 4).0, d.composite().get(3, 4).0);
        // We get the layers back.
        let back = read_layered_tiff(&bytes).expect("layers");
        assert_eq!(back.layers.len(), 2);
        assert_eq!(back.layers[1].name, "Top layer");
        assert_eq!(back.layers[1].blend, BlendMode::Screen);
        assert_eq!(back.layers[1].pixels.get(3, 4), Rgba::rgb(255, 0, 0));
        // A single plain layer writes an ordinary TIFF.
        let plain = layered_tiff_bytes(&Document::new(4, 4, Some(Rgba::WHITE))).unwrap();
        assert!(read_layered_tiff(&plain).is_none());
    }

    #[test]
    fn encodes_every_export_format() {
        let img = Image::filled(8, 8, Rgba::rgba(10, 20, 30, 200));
        for e in EXPORT_EXTENSIONS {
            let bytes = encode(&img, e, &ExportOptions::default()).unwrap_or_else(|err| panic!("{e}: {err}"));
            if *e == "avif" {
                // Encode-only (decoding needs a native AV1 decoder): check the container brand.
                assert_eq!(&bytes[4..12], b"ftypavif");
                continue;
            }
            let fmt = ImageFormat::from_extension(e).unwrap();
            let back = image::load_from_memory_with_format(&bytes, fmt).unwrap_or_else(|err| panic!("{e}: {err}"));
            assert_eq!(back.width(), 8, "{e}");
        }
    }
}
