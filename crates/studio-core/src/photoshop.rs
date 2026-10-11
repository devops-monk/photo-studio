//! Photoshop documents: PSD and its large-document variant PSB.
//!
//! Reads 8- and 16-bit RGB and grayscale files with raw, RLE or ZIP channel data, groups,
//! layer masks and Unicode layer names. Writes RLE-compressed 8-bit RGB with the same.

use std::io::Read;

use crate::blend::BlendMode;
use crate::document::Document;
use crate::geom::Rect;
use crate::image::{Image, Mask};
use crate::io::{IoError, Result};
use crate::layer::{Layer, LayerId, LayerKind};

/// PSD stores at most 30,000 pixels per side; PSB up to 300,000.
pub const PSD_MAX_SIDE: u32 = 30_000;
pub const PSB_MAX_SIDE: u32 = 300_000;

fn bad(msg: impl Into<String>) -> IoError {
    IoError::Psd(msg.into())
}

// ---------------------------------------------------------------------------------------------
// Reading.

struct Cur<'a> {
    b: &'a [u8],
    p: usize,
}

impl<'a> Cur<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.p.checked_add(n).filter(|&e| e <= self.b.len()).ok_or_else(|| bad("file is truncated"))?;
        let s = &self.b[self.p..end];
        self.p = end;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_be_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn i16(&mut self) -> Result<i16> {
        Ok(self.u16()? as i16)
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn i32(&mut self) -> Result<i32> {
        Ok(self.u32()? as i32)
    }
    fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_be_bytes(self.take(8)?.try_into().unwrap()))
    }
    /// A section length: 4 bytes in PSD, 8 in PSB.
    fn len(&mut self, big: bool) -> Result<usize> {
        let n = if big { self.u64()? } else { self.u32()? as u64 };
        usize::try_from(n).map_err(|_| bad("section too large"))
    }
    fn sub(&mut self, n: usize) -> Result<Cur<'a>> {
        Ok(Cur { b: self.take(n)?, p: 0 })
    }
    fn left(&self) -> usize {
        self.b.len() - self.p
    }
}

struct Header {
    big: bool,
    channels: u16,
    w: u32,
    h: u32,
    depth: u16,
    mode: u16,
}

/// Tagged blocks whose length field is 8 bytes in PSB files.
const LONG_KEYS: [&[u8; 4]; 13] = [
    b"LMsk", b"Lr16", b"Lr32", b"Layr", b"Mt16", b"Mt32", b"Mtrn", b"Alph", b"FMsk", b"lnk2", b"FEid", b"FXid", b"PxSD",
];

/// Additional-information blocks: (key, data).
fn tagged_blocks<'a>(c: &mut Cur<'a>, big: bool) -> Result<Vec<([u8; 4], &'a [u8])>> {
    let mut out = Vec::new();
    while c.left() >= 12 {
        let sig = c.take(4)?;
        if sig != b"8BIM" && sig != b"8B64" {
            break;
        }
        let key: [u8; 4] = c.take(4)?.try_into().unwrap();
        let n = c.len(big && LONG_KEYS.contains(&&key))?;
        let data = c.take(n.min(c.left()))?;
        out.push((key, data));
        // Blocks are padded to an even length (and some writers pad to 4).
        if n % 2 == 1 && c.left() > 0 {
            c.take(1)?;
        }
        while c.left() > 0 && c.left() < 12 && c.b[c.p] == 0 {
            c.take(1)?;
        }
    }
    Ok(out)
}

fn unpack_bits(src: &[u8], out: &mut Vec<u8>, want: usize) {
    let mut i = 0;
    let start = out.len();
    while i < src.len() && out.len() - start < want {
        let n = src[i] as i8;
        i += 1;
        if n >= 0 {
            let k = n as usize + 1;
            let end = (i + k).min(src.len());
            out.extend_from_slice(&src[i..end]);
            i = end;
        } else if n != -128 {
            if let Some(&v) = src.get(i) {
                out.extend(std::iter::repeat_n(v, (1 - n as isize) as usize));
            }
            i += 1;
        }
    }
    out.resize(start + want, 0);
}

/// Decode one channel's data (after the compression word) to 8-bit samples.
fn decode_channel(comp: u16, data: &[u8], w: usize, h: usize, depth: u16, big: bool) -> Result<Vec<u8>> {
    let bpp = (depth as usize / 8).max(1);
    let row = w * bpp;
    let raw = match comp {
        0 => {
            let mut v = data.to_vec();
            v.resize(row * h, 0);
            v
        }
        1 => {
            let cw = if big { 4 } else { 2 };
            let mut c = Cur { b: data, p: 0 };
            let mut counts = Vec::with_capacity(h);
            for _ in 0..h {
                counts.push(if big { c.u32()? as usize } else { c.u16()? as usize });
            }
            let mut v = Vec::with_capacity(row * h);
            let mut p = h * cw;
            for n in counts {
                let end = (p + n).min(data.len());
                unpack_bits(&data[p.min(end)..end], &mut v, row);
                p = end;
            }
            v
        }
        2 | 3 => {
            let mut v = Vec::with_capacity(row * h);
            flate2::read::ZlibDecoder::new(data).read_to_end(&mut v).map_err(|e| bad(format!("zip: {e}")))?;
            v.resize(row * h, 0);
            if comp == 3 {
                // Per-row delta prediction on samples.
                for y in 0..h {
                    let r = &mut v[y * row..(y + 1) * row];
                    if bpp == 1 {
                        for x in 1..w {
                            r[x] = r[x].wrapping_add(r[x - 1]);
                        }
                    } else if bpp == 2 {
                        for x in 1..w {
                            let prev = u16::from_be_bytes([r[2 * x - 2], r[2 * x - 1]]);
                            let cur = u16::from_be_bytes([r[2 * x], r[2 * x + 1]]).wrapping_add(prev);
                            r[2 * x..2 * x + 2].copy_from_slice(&cur.to_be_bytes());
                        }
                    }
                }
            }
            v
        }
        _ => return Err(bad(format!("unknown compression {comp}"))),
    };
    Ok(match depth {
        8 => raw,
        16 => raw.chunks_exact(2).map(|s| s[0]).collect(),
        32 => raw
            .chunks_exact(4)
            .map(|s| crate::color::to_u8(f32::from_be_bytes(s.try_into().unwrap()).powf(1.0 / 2.2)))
            .collect(),
        1 => {
            // Bitmap: 1 = black.
            let mut v = Vec::with_capacity(w * h);
            let stride = w.div_ceil(8);
            for y in 0..h {
                for x in 0..w {
                    let byte = data.get(y * stride + x / 8).copied().unwrap_or(0);
                    v.push(if byte >> (7 - x % 8) & 1 == 1 { 0 } else { 255 });
                }
            }
            v
        }
        d => return Err(bad(format!("{d}-bit channels are not supported"))),
    })
}

struct Record {
    rect: (i32, i32, i32, i32),
    channels: Vec<(i16, usize)>,
    blend: [u8; 4],
    opacity: u8,
    clip: bool,
    hidden: bool,
    name: String,
    /// lsct: 1/2 = open/closed folder, 3 = end-of-group divider.
    section: Option<(u32, Option<[u8; 4]>)>,
    mask: Option<((i32, i32, i32, i32), u8, bool)>,
}

fn blend_from_key(k: &[u8; 4]) -> BlendMode {
    match k {
        b"diss" => BlendMode::Dissolve,
        b"dark" => BlendMode::Darken,
        b"mul " => BlendMode::Multiply,
        b"idiv" => BlendMode::ColorBurn,
        b"lbrn" => BlendMode::LinearBurn,
        b"dkCl" => BlendMode::DarkerColor,
        b"lite" => BlendMode::Lighten,
        b"scrn" => BlendMode::Screen,
        b"div " => BlendMode::ColorDodge,
        b"lddg" => BlendMode::LinearDodge,
        b"lgCl" => BlendMode::LighterColor,
        b"over" => BlendMode::Overlay,
        b"sLit" => BlendMode::SoftLight,
        b"hLit" => BlendMode::HardLight,
        b"vLit" => BlendMode::VividLight,
        b"lLit" => BlendMode::LinearLight,
        b"pLit" => BlendMode::PinLight,
        b"hMix" => BlendMode::HardMix,
        b"diff" => BlendMode::Difference,
        b"smud" => BlendMode::Exclusion,
        b"fsub" => BlendMode::Subtract,
        b"fdiv" => BlendMode::Divide,
        b"hue " => BlendMode::Hue,
        b"sat " => BlendMode::Saturation,
        b"colr" => BlendMode::Color,
        b"lum " => BlendMode::Luminosity,
        _ => BlendMode::Normal,
    }
}

pub fn blend_key(m: BlendMode) -> &'static [u8; 4] {
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

fn read_records(c: &mut Cur, big: bool) -> Result<Vec<Record>> {
    let count = c.i16()?.unsigned_abs() as usize;
    let mut recs = Vec::with_capacity(count);
    for _ in 0..count {
        let rect = (c.i32()?, c.i32()?, c.i32()?, c.i32()?);
        let nch = c.u16()? as usize;
        let mut channels = Vec::with_capacity(nch);
        for _ in 0..nch {
            channels.push((c.i16()?, c.len(big)?));
        }
        if c.take(4)? != b"8BIM" {
            return Err(bad("bad layer record"));
        }
        let blend: [u8; 4] = c.take(4)?.try_into().unwrap();
        let opacity = c.u8()?;
        let clip = c.u8()? == 1;
        let flags = c.u8()?;
        c.u8()?;
        let extra_len = c.u32()? as usize;
        let mut e = c.sub(extra_len)?;
        let mask_len = e.u32()? as usize;
        let mut m = e.sub(mask_len)?;
        let mask = if mask_len >= 20 {
            let r = (m.i32()?, m.i32()?, m.i32()?, m.i32()?);
            let default = m.u8()?;
            let mflags = m.u8()?;
            Some((r, default, mflags & 0x02 == 0))
        } else {
            None
        };
        let ranges = e.u32()? as usize;
        e.take(ranges)?;
        let n = e.u8()? as usize;
        let mut name = String::from_utf8_lossy(e.take(n)?).into_owned();
        let pad = (4 - (n + 1) % 4) % 4;
        e.take(pad.min(e.left()))?;
        let mut section = None;
        for (key, data) in tagged_blocks(&mut e, big)? {
            match &key {
                b"luni" if data.len() >= 4 => {
                    let k = u32::from_be_bytes(data[..4].try_into().unwrap()) as usize;
                    let units: Vec<u16> =
                        data[4..].chunks_exact(2).take(k).map(|s| u16::from_be_bytes([s[0], s[1]])).collect();
                    name = String::from_utf16_lossy(&units).trim_end_matches('\0').to_string();
                }
                b"lsct" | b"lsdk" if data.len() >= 4 => {
                    let kind = u32::from_be_bytes(data[..4].try_into().unwrap());
                    let key = (data.len() >= 12).then(|| data[8..12].try_into().unwrap());
                    section = Some((kind, key));
                }
                _ => {}
            }
        }
        recs.push(Record { rect, channels, blend, opacity, clip, hidden: flags & 0x02 != 0, name, section, mask });
    }
    Ok(recs)
}

fn read_header(c: &mut Cur) -> Result<Header> {
    if c.take(4)? != b"8BPS" {
        return Err(bad("not a Photoshop file"));
    }
    let big = match c.u16()? {
        1 => false,
        2 => true,
        v => return Err(bad(format!("unknown version {v}"))),
    };
    c.take(6)?;
    let channels = c.u16()?;
    let (h, w) = (c.u32()?, c.u32()?);
    let depth = c.u16()?;
    let mode = c.u16()?;
    let max = if big { PSB_MAX_SIDE } else { PSD_MAX_SIDE };
    if w == 0 || h == 0 || w > max || h > max {
        return Err(bad(format!("bad size {w} × {h}")));
    }
    if !matches!(mode, 0 | 1 | 3 | 8) {
        let name = match mode {
            2 => "indexed",
            4 => "CMYK",
            7 => "multichannel",
            9 => "Lab",
            _ => "this",
        };
        return Err(bad(format!("{name} color mode is not supported")));
    }
    Ok(Header { big, channels, w, h, depth, mode })
}

/// Assemble RGBA from planar channels keyed by id (0, 1, 2 = colour, -1 = alpha).
fn assemble(ch: &[(i16, Vec<u8>)], n: usize, gray: bool) -> Vec<u8> {
    let get = |id: i16| ch.iter().find(|c| c.0 == id).map(|c| &c.1);
    let (r, g, b, a) = if gray { (get(0), get(0), get(0), get(-1)) } else { (get(0), get(1), get(2), get(-1)) };
    let mut out = vec![0u8; n * 4];
    for i in 0..n {
        out[i * 4] = r.map_or(0, |v| v[i]);
        out[i * 4 + 1] = g.map_or(0, |v| v[i]);
        out[i * 4 + 2] = b.map_or(0, |v| v[i]);
        out[i * 4 + 3] = a.map_or(255, |v| v[i]);
    }
    out
}

/// Place a w×h RGBA tile at (x, y) on a canvas-sized image.
fn place(cw: u32, ch: u32, x0: i32, y0: i32, w: usize, h: usize, rgba: &[u8]) -> Image {
    let mut img = Image::new(cw, ch);
    let px = img.pixels_mut();
    for y in 0..h {
        let cy = y0 + y as i32;
        if cy < 0 || cy >= ch as i32 {
            continue;
        }
        for x in 0..w {
            let cx = x0 + x as i32;
            if cx < 0 || cx >= cw as i32 {
                continue;
            }
            let (s, d) = ((y * w + x) * 4, (cy as usize * cw as usize + cx as usize) * 4);
            px[d..d + 4].copy_from_slice(&rgba[s..s + 4]);
        }
    }
    img
}

pub fn read(bytes: &[u8]) -> Result<Document> {
    let mut c = Cur { b: bytes, p: 0 };
    let hd = read_header(&mut c)?;
    let big = hd.big;
    let gray = matches!(hd.mode, 0 | 1 | 8);
    let cmlen = c.u32()? as usize;
    c.take(cmlen)?;
    let reslen = c.u32()? as usize;
    c.take(reslen)?;
    let lm_len = c.len(big)?;
    let mut lm = c.sub(lm_len)?;
    let (cw, chh) = (hd.w, hd.h);
    let mut doc = Document::new(cw, chh, None);
    doc.layers.clear();

    if lm_len > 0 {
        let li_len = lm.len(big)?;
        let mut li = lm.sub(li_len)?;
        // 16- and 32-bit files keep their layers in a tagged block after the global mask.
        if li_len == 0 && lm.left() >= 4 {
            let gm = lm.u32()? as usize;
            lm.take(gm.min(lm.left()))?;
            if let Some((_, data)) =
                tagged_blocks(&mut lm, big)?.into_iter().find(|(k, _)| k == b"Lr16" || k == b"Lr32")
            {
                li = Cur { b: data, p: 0 };
            }
        }
        if li.left() >= 2 {
            let recs = read_records(&mut li, big)?;
            build_layers(&mut doc, &recs, &mut li, &hd, gray)?;
        }
    }

    if doc.layers.is_empty() {
        // No layer records: use the merged image.
        let comp = c.u16()?;
        let (w, h) = (cw as usize, chh as usize);
        let nch = hd.channels as usize;
        let data = &bytes[c.p..];
        let mut planes = Vec::new();
        if comp == 1 {
            // Row counts for every channel come first, then the rows.
            let cwid = if big { 4 } else { 2 };
            let mut counts = Cur { b: data, p: 0 };
            let mut lens = Vec::with_capacity(nch * h);
            for _ in 0..nch * h {
                lens.push(if big { counts.u32()? as usize } else { counts.u16()? as usize });
            }
            let mut p = nch * h * cwid;
            let row = w * (hd.depth as usize / 8).max(1);
            for k in 0..nch.min(4) {
                let mut v = Vec::with_capacity(row * h);
                for y in 0..h {
                    let n = lens[k * h + y];
                    let end = (p + n).min(data.len());
                    unpack_bits(&data[p.min(end)..end], &mut v, row);
                    p = end;
                }
                planes.push(decode_channel(0, &v, w, h, hd.depth, big)?);
            }
        } else {
            let plane = w * h * (hd.depth as usize / 8).max(1);
            for k in 0..nch.min(4) {
                let s = (k * plane).min(data.len());
                let e = ((k + 1) * plane).min(data.len());
                planes.push(decode_channel(comp, &data[s..e], w, h, hd.depth, big)?);
            }
        }
        let ids: &[i16] = if gray { &[0, -1] } else { &[0, 1, 2, -1] };
        let ch: Vec<(i16, Vec<u8>)> = ids.iter().copied().zip(planes).collect();
        let id = doc.new_id();
        doc.layers.push(Layer::raster(id, "Background", Image::from_raw(cw, chh, assemble(&ch, w * h, gray))));
    }
    doc.active = doc.layers.len() - 1;
    Ok(doc)
}

fn build_layers(doc: &mut Document, recs: &[Record], data: &mut Cur, hd: &Header, gray: bool) -> Result<()> {
    let (cw, chh) = (hd.w, hd.h);
    // Records run bottom to top; a divider opens a group whose folder record closes it.
    let mut open: Vec<LayerId> = Vec::new();
    for r in recs {
        let (top, left, bottom, right) = r.rect;
        let (w, h) = ((right - left).max(0) as usize, (bottom - top).max(0) as usize);
        let mut planes: Vec<(i16, Vec<u8>)> = Vec::new();
        let mut mask_plane = None;
        for &(id, len) in &r.channels {
            let mut ch = data.sub(len.min(data.left()))?;
            if len < 2 {
                continue;
            }
            let comp = ch.u16()?;
            let body = &ch.b[2..];
            if id == -2 {
                if let Some(((mt, ml, mb, mr), ..)) = r.mask {
                    let (mw, mh) = ((mr - ml).max(0) as usize, (mb - mt).max(0) as usize);
                    mask_plane = Some(decode_channel(comp, body, mw, mh, hd.depth, hd.big)?);
                }
            } else if id >= -1 {
                planes.push((id, decode_channel(comp, body, w, h, hd.depth, hd.big)?));
            }
        }
        let parent = open.last().copied();
        let blend = blend_from_key(&r.blend);
        match r.section {
            Some((3, _)) => {
                let id = doc.new_id();
                open.push(id);
            }
            Some((1 | 2, key)) => {
                let id = open.pop().unwrap_or_else(|| doc.new_id());
                let pass_through = key.as_ref().unwrap_or(&r.blend) == b"pass";
                let mut g = Layer::with_kind(
                    id,
                    r.name.clone(),
                    LayerKind::Group { pass_through, collapsed: r.section.unwrap().0 == 2 },
                    cw,
                    chh,
                );
                g.visible = !r.hidden;
                g.opacity = r.opacity as f32 / 255.0;
                g.blend = if pass_through { BlendMode::Normal } else { blend };
                g.parent = open.last().copied();
                doc.layers.push(g);
            }
            _ => {
                let rgba = assemble(&planes, w * h, gray);
                let id = doc.new_id();
                let mut l = Layer::raster(id, r.name.clone(), place(cw, chh, left, top, w, h, &rgba));
                l.visible = !r.hidden;
                l.opacity = r.opacity as f32 / 255.0;
                l.blend = blend;
                l.clip = r.clip;
                l.parent = parent;
                if let (Some(((mt, ml, mb, mr), default, enabled)), Some(mp)) = (r.mask, mask_plane) {
                    let mut m = Mask::from_raw(cw, chh, vec![default; cw as usize * chh as usize]);
                    let mw = (mr - ml).max(0) as usize;
                    let d = m.data_mut();
                    for y in 0..(mb - mt).max(0) as usize {
                        for x in 0..mw {
                            let (cx, cy) = (ml + x as i32, mt + y as i32);
                            if cx >= 0 && cy >= 0 && cx < cw as i32 && cy < chh as i32 {
                                d[cy as usize * cw as usize + cx as usize] = mp[y * mw + x];
                            }
                        }
                    }
                    l.mask = Some(m);
                    l.mask_enabled = enabled;
                }
                doc.layers.push(l);
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Writing.

/// PackBits-compress one row.
fn pack_bits(row: &[u8], out: &mut Vec<u8>) {
    let n = row.len();
    let mut i = 0;
    while i < n {
        // A run of at least 3 equal bytes?
        let mut run = 1;
        while i + run < n && run < 128 && row[i + run] == row[i] {
            run += 1;
        }
        if run >= 3 {
            out.push((1i16 - run as i16) as u8);
            out.push(row[i]);
            i += run;
            continue;
        }
        // Literal stretch until the next run of 3.
        let start = i;
        while i < n && i - start < 128 {
            if i + 2 < n && row[i] == row[i + 1] && row[i] == row[i + 2] {
                break;
            }
            i += 1;
        }
        out.push((i - start - 1) as u8);
        out.extend_from_slice(&row[start..i]);
    }
}

/// RLE-encode a w×h plane: compression word, row byte counts, rows.
fn rle_plane(plane: &[u8], w: usize, h: usize, big: bool) -> Vec<u8> {
    let mut counts = Vec::with_capacity(h * if big { 4 } else { 2 });
    let mut rows = Vec::new();
    for y in 0..h {
        let before = rows.len();
        pack_bits(&plane[y * w..(y + 1) * w], &mut rows);
        let n = rows.len() - before;
        if big {
            counts.extend_from_slice(&(n as u32).to_be_bytes());
        } else {
            counts.extend_from_slice(&(n as u16).to_be_bytes());
        }
    }
    let mut out = vec![0, 1];
    out.extend_from_slice(&counts);
    out.extend_from_slice(&rows);
    out
}

/// The layer-info block (count, records, channel data), padded to 4 bytes. Shared by PSD/PSB and
/// the layers Photoshop embeds in TIFF files.
fn layer_info(doc: &Document, big: bool) -> Vec<u8> {
    let (w, h) = (doc.width, doc.height);
    let be16 = |v: &mut Vec<u8>, x: u16| v.extend_from_slice(&x.to_be_bytes());
    let be32 = |v: &mut Vec<u8>, x: u32| v.extend_from_slice(&x.to_be_bytes());
    let len = |v: &mut Vec<u8>, x: usize| {
        if big {
            v.extend_from_slice(&(x as u64).to_be_bytes());
        } else {
            v.extend_from_slice(&(x as u32).to_be_bytes());
        }
    };
    // Groups become section records: a divider below the children and the folder above them.
    enum Rec<'a> {
        Pixels(&'a Layer),
        Divider(&'a Layer),
        Group(&'a Layer, bool),
    }
    let mut recs: Vec<Rec> = Vec::new();
    for (i, l) in doc.layers.iter().enumerate() {
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
        let (layer, section): (&Layer, Option<(u32, &[u8; 4])>) = match rec {
            Rec::Pixels(l) => (l, None),
            Rec::Divider(g) => (g, Some((3, blend_key(g.blend)))),
            Rec::Group(g, collapsed) => {
                let key: &[u8; 4] = match g.kind {
                    LayerKind::Group { pass_through: true, .. } => b"pass",
                    _ => blend_key(g.blend),
                };
                (g, Some((if *collapsed { 2 } else { 1 }, key)))
            }
        };
        let name = if matches!(rec, Rec::Divider(_)) { "</Layer group>".to_string() } else { layer.name.clone() };
        let px = if section.is_none() { Some(&layer.pixels) } else { None };
        let r = px.and_then(|p| p.opaque_bounds()).unwrap_or(Rect::new(0, 0, 0, 0));
        let (rw, rh) = (r.w as usize, r.h as usize);
        let crop = px.map(|p| p.crop(r));
        let mask = if section.is_none() { layer.mask.as_ref() } else { None };

        let mut chans: Vec<(i16, Vec<u8>)> = Vec::new();
        for (id, c) in [(-1i16, 3usize), (0, 0), (1, 1), (2, 2)] {
            let plane: Vec<u8> =
                crop.as_ref().map_or(Vec::new(), |img| img.pixels().chunks_exact(4).map(|p| p[c]).collect());
            chans.push((id, rle_plane(&plane, rw, rh, big)));
        }
        if let Some(m) = mask {
            chans.push((-2, rle_plane(m.data(), w as usize, h as usize, big)));
        }

        be32(&mut records, r.y as u32);
        be32(&mut records, r.x as u32);
        be32(&mut records, r.bottom() as u32);
        be32(&mut records, r.right() as u32);
        be16(&mut records, chans.len() as u16);
        for (id, data) in &chans {
            records.extend_from_slice(&id.to_be_bytes());
            len(&mut records, data.len());
        }
        records.extend_from_slice(b"8BIM");
        records.extend_from_slice(match section {
            Some((_, b"pass")) => b"pass",
            _ => blend_key(layer.blend),
        });
        records.push((layer.opacity.clamp(0.0, 1.0) * 255.0).round() as u8);
        records.push(u8::from(section.is_none() && layer.clip));
        records.push(if layer.visible { 0 } else { 2 });
        records.push(0);

        let mut extra = Vec::new();
        // Layer mask data: the whole canvas.
        if let Some(_m) = mask {
            be32(&mut extra, 20);
            be32(&mut extra, 0);
            be32(&mut extra, 0);
            be32(&mut extra, h);
            be32(&mut extra, w);
            extra.push(0); // default colour
            extra.push(if layer.mask_enabled { 0 } else { 2 });
            extra.extend_from_slice(&[0, 0]);
        } else {
            be32(&mut extra, 0);
        }
        be32(&mut extra, 0); // blending ranges
        let ascii: Vec<u8> = name.chars().filter(|c| c.is_ascii()).take(255).map(|c| c as u8).collect();
        let mut pascal = vec![ascii.len() as u8];
        pascal.extend_from_slice(&ascii);
        while pascal.len() % 4 != 0 {
            pascal.push(0);
        }
        extra.extend_from_slice(&pascal);
        // Unicode name.
        let units: Vec<u16> = name.encode_utf16().collect();
        let mut luni = Vec::new();
        be32(&mut luni, units.len() as u32);
        for u in &units {
            be16(&mut luni, *u);
        }
        while luni.len() % 4 != 0 {
            luni.push(0);
        }
        extra.extend_from_slice(b"8BIMluni");
        be32(&mut extra, luni.len() as u32);
        extra.extend_from_slice(&luni);
        if let Some((kind, key)) = section {
            extra.extend_from_slice(b"8BIMlsct");
            be32(&mut extra, 12);
            be32(&mut extra, kind);
            extra.extend_from_slice(b"8BIM");
            extra.extend_from_slice(key);
        }
        be32(&mut records, extra.len() as u32);
        records.extend_from_slice(&extra);
        for (_, data) in chans {
            channels.extend_from_slice(&data);
        }
    }

    let mut layer_info = Vec::new();
    be16(&mut layer_info, recs.len() as u16);
    layer_info.extend_from_slice(&records);
    layer_info.extend_from_slice(&channels);
    while layer_info.len() % 4 != 0 {
        layer_info.push(0);
    }
    layer_info
}

/// Write a layered, RLE-compressed PSD (`big = false`) or PSB (`big = true`). Text, shape and
/// fill layers are written as pixels; adjustment layers show up only in the merged image.
pub fn write(doc: &Document, big: bool) -> Result<Vec<u8>> {
    let (w, h) = (doc.width, doc.height);
    let max = if big { PSB_MAX_SIDE } else { PSD_MAX_SIDE };
    if w > max || h > max {
        return Err(bad(if big {
            "PSB is limited to 300,000 pixels per side".to_string()
        } else {
            "PSD is limited to 30,000 pixels per side; save as PSB instead".to_string()
        }));
    }
    let mut b: Vec<u8> = Vec::new();
    let be16 = |v: &mut Vec<u8>, x: u16| v.extend_from_slice(&x.to_be_bytes());
    let be32 = |v: &mut Vec<u8>, x: u32| v.extend_from_slice(&x.to_be_bytes());
    let len = |v: &mut Vec<u8>, x: usize| {
        if big {
            v.extend_from_slice(&(x as u64).to_be_bytes());
        } else {
            v.extend_from_slice(&(x as u32).to_be_bytes());
        }
    };

    b.extend_from_slice(b"8BPS");
    be16(&mut b, if big { 2 } else { 1 });
    b.extend_from_slice(&[0; 6]);
    be16(&mut b, 4); // channels in the merged image: RGBA
    be32(&mut b, h);
    be32(&mut b, w);
    be16(&mut b, 8);
    be16(&mut b, 3); // RGB
    be32(&mut b, 0); // colour mode data
    be32(&mut b, 0); // image resources

    let layer_info = layer_info(doc, big);
    let lw = if big { 8 } else { 4 };
    len(&mut b, layer_info.len() + lw + 4);
    len(&mut b, layer_info.len());
    b.extend_from_slice(&layer_info);
    be32(&mut b, 0); // global layer mask info

    // Merged image: RLE with every channel's row counts first.
    let comp = doc.composite();
    let (wu, hu) = (w as usize, h as usize);
    let mut counts = Vec::new();
    let mut rows = Vec::new();
    for c in 0..4 {
        let plane: Vec<u8> = comp.pixels().chunks_exact(4).map(|p| p[c]).collect();
        for y in 0..hu {
            let before = rows.len();
            pack_bits(&plane[y * wu..(y + 1) * wu], &mut rows);
            let n = rows.len() - before;
            if big {
                counts.extend_from_slice(&(n as u32).to_be_bytes());
            } else {
                counts.extend_from_slice(&(n as u16).to_be_bytes());
            }
        }
    }
    be16(&mut b, 1);
    b.extend_from_slice(&counts);
    b.extend_from_slice(&rows);
    Ok(b)
}

// ---------------------------------------------------------------------------------------------
// Layers inside TIFF files.

/// Photoshop's private TIFF tag (ImageSourceData) that carries layers.
pub const TIFF_LAYERS_TAG: u16 = 37724;
const TIFF_BLOCK: &[u8] = b"Adobe Photoshop Document Data Block\0";

/// The ImageSourceData payload for `doc`'s layers.
pub fn tiff_layer_block(doc: &Document) -> Vec<u8> {
    let li = layer_info(doc, false);
    let mut v = TIFF_BLOCK.to_vec();
    v.extend_from_slice(b"8BIMLayr");
    v.extend_from_slice(&(li.len() as u32).to_be_bytes());
    v.extend_from_slice(&li);
    v
}

/// Layers from an ImageSourceData payload, if it has any.
pub fn read_tiff_layers(data: &[u8], w: u32, h: u32) -> Result<Option<Document>> {
    let Some(rest) = data.strip_prefix(TIFF_BLOCK) else { return Ok(None) };
    let mut c = Cur { b: rest, p: 0 };
    for (key, block) in tagged_blocks(&mut c, false)? {
        let depth = match &key {
            b"Layr" => 8,
            b"Lr16" => 16,
            b"Lr32" => 32,
            _ => continue,
        };
        let hd = Header { big: false, channels: 4, w, h, depth, mode: 3 };
        let mut li = Cur { b: block, p: 0 };
        let recs = read_records(&mut li, false)?;
        let mut doc = Document::new(w, h, None);
        doc.layers.clear();
        build_layers(&mut doc, &recs, &mut li, &hd, false)?;
        if doc.layers.is_empty() {
            return Ok(None);
        }
        doc.active = doc.layers.len() - 1;
        return Ok(Some(doc));
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Rgba;

    fn doc() -> Document {
        let mut d = Document::new(40, 30, Some(Rgba::rgb(20, 40, 60)));
        d.add_empty_layer("Paint ✓");
        for x in 5..25 {
            d.layers[1].pixels.put(x, 7, Rgba::rgb(255, 0, 0));
        }
        d.layers[1].blend = BlendMode::Multiply;
        d.layers[1].opacity = 0.5;
        let mut m = Mask::from_raw(40, 30, vec![255; 1200]);
        m.data_mut()[7 * 40 + 6] = 0;
        d.layers[1].mask = Some(m);
        d
    }

    #[test]
    fn psb_holds_what_psd_cannot() {
        let mut d = Document::new(30_100, 3, Some(Rgba::rgb(1, 2, 3)));
        d.layers[0].pixels.put(30_050, 1, Rgba::rgb(200, 100, 50));
        assert!(write(&d, false).is_err(), "too wide for PSD");
        let back = read(&write(&d, true).unwrap()).unwrap();
        assert_eq!(back.width, 30_100);
        assert_eq!(back.layers[0].pixels.get(30_050, 1), Rgba::rgb(200, 100, 50));
    }

    #[test]
    fn packbits_round_trip() {
        let row: Vec<u8> = (0..300).map(|i| if i % 50 < 20 { 7 } else { (i * 13 % 251) as u8 }).collect();
        let mut packed = Vec::new();
        pack_bits(&row, &mut packed);
        assert!(packed.len() < row.len());
        let mut back = Vec::new();
        unpack_bits(&packed, &mut back, row.len());
        assert_eq!(back, row);
    }

    #[test]
    fn psd_and_psb_round_trip() {
        let d = doc();
        for big in [false, true] {
            let bytes = write(&d, big).unwrap();
            assert_eq!(&bytes[4..6], if big { &[0, 2] } else { &[0, 1] });
            let back = read(&bytes).unwrap();
            assert_eq!(back.layers.len(), 2);
            let l = &back.layers[1];
            assert_eq!(l.name, "Paint ✓", "unicode name");
            assert_eq!(l.blend, BlendMode::Multiply);
            assert!((l.opacity - 0.5).abs() < 0.01);
            assert_eq!(l.pixels.get(5, 7), Rgba::rgb(255, 0, 0));
            assert_eq!(l.mask.as_ref().unwrap().get(6, 7), 0, "mask kept");
            assert_eq!(back.layers[0].pixels.get(30, 20), Rgba::rgb(20, 40, 60));
        }
    }

    #[test]
    fn psd_output_still_reads_in_the_psd_crate() {
        let bytes = write(&doc(), false).unwrap();
        let p = psd::Psd::from_bytes(&bytes).unwrap();
        assert_eq!(p.layers().len(), 2);
        assert_eq!(&p.rgba()[..4], &[20, 40, 60, 255]);
    }

    #[test]
    fn reads_flat_zip_and_merged_only_files() {
        // A merged-image-only PSB, written by hand.
        let (w, h) = (3u32, 2u32);
        let mut b = b"8BPS\x00\x02\x00\x00\x00\x00\x00\x00\x00\x03".to_vec();
        b.extend_from_slice(&h.to_be_bytes());
        b.extend_from_slice(&w.to_be_bytes());
        b.extend_from_slice(&[0, 8, 0, 3]);
        b.extend_from_slice(&[0; 8]);
        b.extend_from_slice(&0u64.to_be_bytes());
        b.extend_from_slice(&[0, 0]);
        for c in [10u8, 20, 30] {
            b.extend(std::iter::repeat_n(c, 6));
        }
        let d = read(&b).unwrap();
        assert_eq!(d.layers[0].pixels.get(2, 1), Rgba::rgb(10, 20, 30));
        // Zip with prediction decodes deltas.
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        std::io::Write::write_all(&mut z, &[5, 1, 1, 9, 0, 0]).unwrap();
        let data = z.finish().unwrap();
        assert_eq!(decode_channel(3, &data, 3, 2, 8, false).unwrap(), vec![5, 6, 7, 9, 9, 9]);
    }

    #[test]
    fn groups_round_trip() {
        let mut d = doc();
        d.active = 1;
        d.group_active("Group 1");
        let bytes = write(&d, true).unwrap();
        let back = read(&bytes).unwrap();
        let g = back.layers.iter().find(|l| l.is_group()).expect("group");
        let child = back.layers.iter().find(|l| l.name == "Paint ✓").unwrap();
        assert_eq!(child.parent, Some(g.id));
    }
}
