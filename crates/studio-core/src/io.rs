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
pub const OPEN_EXTENSIONS: &[&str] =
    &["pstudio", "psd", "png", "jpg", "jpeg", "webp", "bmp", "tif", "tiff", "gif", "tga", "ico", "qoi", "ppm", "pgm", "pnm"];

/// Flat formats the app can export to.
pub const EXPORT_EXTENSIONS: &[&str] = &["png", "jpg", "webp", "bmp", "tiff", "gif", "tga", "ico", "qoi", "ppm"];

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
}

fn png_bytes(img: &image::DynamicImage) -> Result<Vec<u8>> {
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
        if let Some(m) = &l.mask {
            let g = image::GrayImage::from_raw(m.width(), m.height(), m.data().to_vec()).expect("mask size");
            zw.start_file(format!("masks/{}.png", l.id), stored)?;
            zw.write_all(&png_bytes(&DynamicImage::ImageLuma8(g))?)?;
        }
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
        doc.next_id = doc.next_id.max(meta.id + 1);
        doc.layers.push(layer);
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
    for pl in psd.layers() {
        let rgba = pl.rgba();
        if rgba.len() != (w * h * 4) as usize {
            continue;
        }
        let id = doc.new_id();
        let mut l = Layer::raster(id, pl.name(), Image::from_raw(w, h, rgba));
        l.visible = pl.visible();
        l.opacity = pl.opacity() as f32 / 255.0;
        l.blend = BlendMode::from_name(&format!("{:?}", pl.blend_mode())).unwrap_or_default();
        l.clip = pl.is_clipping_mask();
        doc.layers.push(l);
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

    // Layer info.
    let layers: Vec<&Layer> = doc.layers.iter().filter(|l| l.pixels.size() == (w, h)).collect();
    let mut records = Vec::new();
    let mut channels = Vec::new();
    for l in &layers {
        let mut px = l.pixels.clone();
        if let Some(m) = l.active_mask() {
            let d = m.data();
            for (i, p) in px.pixels_mut().chunks_exact_mut(4).enumerate() {
                p[3] = ((p[3] as u32 * d[i] as u32 + 127) / 255) as u8;
            }
        }
        let r = px.opaque_bounds().unwrap_or(crate::geom::Rect::new(0, 0, 0, 0));
        let crop = px.crop(r);
        let n = (r.w * r.h) as usize;
        be32(&mut records, r.y as u32);
        be32(&mut records, r.x as u32);
        be32(&mut records, r.bottom() as u32);
        be32(&mut records, r.right() as u32);
        be16(&mut records, 4);
        for (cid, _) in [(-1i16, 3usize), (0, 0), (1, 1), (2, 2)] {
            records.extend_from_slice(&cid.to_be_bytes());
            be32(&mut records, 2 + n as u32);
        }
        records.extend_from_slice(b"8BIM");
        records.extend_from_slice(psd_blend_key(l.blend));
        records.push((l.opacity.clamp(0.0, 1.0) * 255.0).round() as u8);
        records.push(u8::from(l.clip));
        records.push(if l.visible { 0 } else { 2 });
        records.push(0);
        let name: Vec<u8> = l.name.chars().filter(|c| c.is_ascii()).take(255).map(|c| c as u8).collect();
        let mut pascal = vec![name.len() as u8];
        pascal.extend_from_slice(&name);
        while pascal.len() % 4 != 0 {
            pascal.push(0);
        }
        be32(&mut records, 4 + 4 + pascal.len() as u32);
        be32(&mut records, 0); // mask data
        be32(&mut records, 0); // blending ranges
        records.extend_from_slice(&pascal);
        for c in [3usize, 0, 1, 2] {
            be16(&mut channels, 0);
            channels.extend(crop.pixels().chunks_exact(4).map(|p| p[c]));
        }
    }
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
    }

    #[test]
    fn psd_round_trip() {
        let d = sample_doc();
        let bytes = psd_bytes(&d).unwrap();
        let back = read_psd(&bytes).unwrap();
        assert_eq!((back.width, back.height), (16, 12));
        // Adjustment layers are not written as layers.
        assert_eq!(back.layers.len(), 3);
        assert_eq!(back.layers[1].name, "Paint");
        assert_eq!(back.layers[1].blend, BlendMode::Screen);
        assert_eq!(back.layers[1].pixels.get(3, 4), Rgba::rgb(255, 0, 0));
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
