//! Paint.NET documents (.pdn).
//!
//! A .pdn file is "PDN3", a 24-bit little-endian header length, an XML header, then a .NET
//! BinaryFormatter (MS-NRBF) object graph describing the document and its layers, followed by
//! each layer's pixels as gzip-compressed chunks.

use std::collections::HashMap;
use std::io::Read;

use crate::blend::BlendMode;
use crate::document::Document;
use crate::image::Image;
use crate::io::{IoError, Result};
use crate::layer::Layer;

fn bad(msg: impl Into<String>) -> IoError {
    IoError::Unsupported(format!("Paint.NET file: {}", msg.into()))
}

// ---------------------------------------------------------------------------------------------
// MS-NRBF.

#[derive(Clone, Debug)]
pub(crate) enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    /// Reference to an object, array or string record by id.
    Ref(i32),
    /// Inline primitive array (bytes are kept as-is).
    Bytes(Vec<u8>),
    List(Vec<Value>),
}

#[derive(Clone, Debug)]
pub(crate) struct Obj {
    pub class: String,
    pub members: Vec<(String, Value)>,
}

impl Obj {
    fn get(&self, name: &str) -> Option<&Value> {
        // Exact name first; members of base classes are prefixed ("ArrayList+_items").
        self.members
            .iter()
            .find(|(n, _)| n == name)
            .or_else(|| self.members.iter().find(|(n, _)| n.rsplit(['+', '.']).next() == Some(name)))
            .map(|(_, v)| v)
    }
}

#[derive(Clone)]
struct ClassMeta {
    name: String,
    members: Vec<String>,
    /// (binary type, primitive type) per member; None when the record carries no types.
    types: Option<Vec<(u8, u8)>>,
}

#[derive(Default)]
pub(crate) struct Graph {
    pub objects: HashMap<i32, Obj>,
    pub strings: HashMap<i32, String>,
    pub lists: HashMap<i32, Value>,
    pub root: i32,
}

impl Graph {
    pub(crate) fn resolve<'a>(&'a self, v: &'a Value) -> &'a Value {
        let mut v = v;
        for _ in 0..16 {
            match v {
                Value::Ref(id) => match self.lists.get(id) {
                    Some(l) => v = l,
                    None => return v,
                },
                _ => return v,
            }
        }
        v
    }

    pub(crate) fn obj(&self, v: &Value) -> Option<&Obj> {
        match v {
            Value::Ref(id) => self.objects.get(id),
            _ => None,
        }
    }

    pub(crate) fn str(&self, v: &Value) -> Option<String> {
        match v {
            Value::Str(s) => Some(s.clone()),
            Value::Ref(id) => self.strings.get(id).cloned(),
            _ => None,
        }
    }

    pub(crate) fn int(&self, v: &Value) -> Option<i64> {
        match v {
            Value::Int(i) => Some(*i),
            Value::Bool(b) => Some(*b as i64),
            Value::Float(f) => Some(*f as i64),
            _ => None,
        }
    }
}

struct Nrbf<'a> {
    b: &'a [u8],
    p: usize,
    meta: HashMap<i32, ClassMeta>,
    g: Graph,
}

/// What a record decoded to, for the member it is the value of.
enum Rec {
    Value(Value),
    /// ObjectNullMultiple: this many nulls.
    Nulls(usize),
    End,
}

impl<'a> Nrbf<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.p.checked_add(n).filter(|&e| e <= self.b.len()).ok_or_else(|| bad("truncated"))?;
        let s = &self.b[self.p..end];
        self.p = end;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn lps(&mut self) -> Result<String> {
        let mut len = 0usize;
        for shift in (0..35).step_by(7) {
            let b = self.u8()?;
            len |= ((b & 0x7f) as usize) << shift;
            if b & 0x80 == 0 {
                break;
            }
        }
        Ok(String::from_utf8_lossy(self.take(len)?).into_owned())
    }

    fn primitive(&mut self, t: u8) -> Result<Value> {
        Ok(match t {
            1 => Value::Bool(self.u8()? != 0),
            2 => Value::Int(self.u8()? as i64),
            3 => {
                // Char: one UTF-8 sequence.
                let first = self.u8()?;
                let extra = match first {
                    0xF0.. => 3,
                    0xE0.. => 2,
                    0xC0.. => 1,
                    _ => 0,
                };
                self.take(extra)?;
                Value::Int(first as i64)
            }
            5 | 18 => Value::Str(self.lps()?),
            6 => Value::Float(f64::from_le_bytes(self.take(8)?.try_into().unwrap())),
            7 => Value::Int(i16::from_le_bytes(self.take(2)?.try_into().unwrap()) as i64),
            8 => Value::Int(self.i32()? as i64),
            9 | 12 | 13 => Value::Int(i64::from_le_bytes(self.take(8)?.try_into().unwrap())),
            10 => Value::Int(self.u8()? as i8 as i64),
            11 => Value::Float(f32::from_le_bytes(self.take(4)?.try_into().unwrap()) as f64),
            14 => Value::Int(u16::from_le_bytes(self.take(2)?.try_into().unwrap()) as i64),
            15 => Value::Int(u32::from_le_bytes(self.take(4)?.try_into().unwrap()) as i64),
            16 => Value::Int(u64::from_le_bytes(self.take(8)?.try_into().unwrap()) as i64),
            17 => Value::Null,
            t => return Err(bad(format!("primitive type {t}"))),
        })
    }

    fn class_info(&mut self) -> Result<(i32, String, Vec<String>)> {
        let id = self.i32()?;
        let name = self.lps()?;
        let n = self.i32()?.max(0) as usize;
        let mut members = Vec::with_capacity(n.min(1024));
        for _ in 0..n {
            members.push(self.lps()?);
        }
        Ok((id, name, members))
    }

    fn member_types(&mut self, n: usize) -> Result<Vec<(u8, u8)>> {
        let kinds: Vec<u8> = self.take(n)?.to_vec();
        let mut out = Vec::with_capacity(n);
        for k in kinds {
            let extra = match k {
                0 | 7 => self.u8()?,
                3 => {
                    self.lps()?;
                    0
                }
                4 => {
                    self.lps()?;
                    self.i32()?;
                    0
                }
                _ => 0,
            };
            out.push((k, extra));
        }
        Ok(out)
    }

    /// Read the member values of an object with `meta`.
    fn members(&mut self, meta: &ClassMeta) -> Result<Vec<(String, Value)>> {
        let mut out = Vec::with_capacity(meta.members.len());
        let mut pending_nulls = 0usize;
        for (i, name) in meta.members.iter().enumerate() {
            if pending_nulls > 0 {
                pending_nulls -= 1;
                out.push((name.clone(), Value::Null));
                continue;
            }
            let v = match meta.types.as_ref().map(|t| t[i]) {
                Some((0, prim)) => self.primitive(prim)?,
                _ => match self.record()? {
                    Rec::Value(v) => v,
                    Rec::Nulls(n) => {
                        pending_nulls = n.saturating_sub(1);
                        Value::Null
                    }
                    Rec::End => return Err(bad("unexpected end inside an object")),
                },
            };
            out.push((name.clone(), v));
        }
        Ok(out)
    }

    fn values(&mut self, n: usize) -> Result<Vec<Value>> {
        let mut out = Vec::with_capacity(n.min(1 << 16));
        while out.len() < n {
            match self.record()? {
                Rec::Value(v) => out.push(v),
                Rec::Nulls(k) => out.extend(std::iter::repeat_n(Value::Null, k)),
                Rec::End => return Err(bad("unexpected end inside an array")),
            }
        }
        Ok(out)
    }

    fn record(&mut self) -> Result<Rec> {
        let kind = self.u8()?;
        Ok(match kind {
            0 => {
                self.g.root = self.i32()?;
                self.take(12)?;
                return self.record();
            }
            1 => {
                let id = self.i32()?;
                let meta_id = self.i32()?;
                let meta = self.meta.get(&meta_id).cloned().ok_or_else(|| bad("unknown class metadata"))?;
                let members = self.members(&meta)?;
                self.g.objects.insert(id, Obj { class: meta.name.clone(), members });
                Rec::Value(Value::Ref(id))
            }
            2..=5 => {
                let (id, name, member_names) = self.class_info()?;
                let types = if kind == 4 || kind == 5 { Some(self.member_types(member_names.len())?) } else { None };
                if kind == 3 || kind == 5 {
                    self.i32()?; // library id
                }
                let meta = ClassMeta { name: name.clone(), members: member_names, types };
                self.meta.insert(id, meta.clone());
                let members = self.members(&meta)?;
                self.g.objects.insert(id, Obj { class: name, members });
                Rec::Value(Value::Ref(id))
            }
            6 => {
                let id = self.i32()?;
                let s = self.lps()?;
                self.g.strings.insert(id, s.clone());
                Rec::Value(Value::Ref(id))
            }
            7 => {
                let id = self.i32()?;
                let array_kind = self.u8()?;
                let rank = self.i32()?.max(0) as usize;
                let mut count = 1usize;
                for _ in 0..rank {
                    count = count.saturating_mul(self.i32()?.max(0) as usize);
                }
                if matches!(array_kind, 3..=5) {
                    for _ in 0..rank {
                        self.i32()?;
                    }
                }
                let t = self.member_types(1)?[0];
                let v = if t.0 == 0 { self.primitive_array(t.1, count)? } else { Value::List(self.values(count)?) };
                self.g.lists.insert(id, v);
                Rec::Value(Value::Ref(id))
            }
            8 => {
                let t = self.u8()?;
                Rec::Value(self.primitive(t)?)
            }
            9 => Rec::Value(Value::Ref(self.i32()?)),
            10 => Rec::Value(Value::Null),
            11 => Rec::End,
            12 => {
                self.i32()?;
                self.lps()?;
                return self.record();
            }
            13 => Rec::Nulls(self.u8()? as usize),
            14 => Rec::Nulls(self.i32()?.max(0) as usize),
            15 => {
                let id = self.i32()?;
                let n = self.i32()?.max(0) as usize;
                let t = self.u8()?;
                let v = self.primitive_array(t, n)?;
                self.g.lists.insert(id, v);
                Rec::Value(Value::Ref(id))
            }
            16 | 17 => {
                let id = self.i32()?;
                let n = self.i32()?.max(0) as usize;
                let v = Value::List(self.values(n)?);
                self.g.lists.insert(id, v);
                Rec::Value(Value::Ref(id))
            }
            k => return Err(bad(format!("record type {k}"))),
        })
    }

    fn primitive_array(&mut self, t: u8, n: usize) -> Result<Value> {
        if t == 2 {
            return Ok(Value::Bytes(self.take(n)?.to_vec()));
        }
        let mut v = Vec::with_capacity(n.min(1 << 16));
        for _ in 0..n {
            v.push(self.primitive(t)?);
        }
        Ok(Value::List(v))
    }
}

/// Parse an NRBF stream; returns the graph and the offset just past MessageEnd.
pub(crate) fn parse_nrbf(b: &[u8]) -> Result<(Graph, usize)> {
    let mut n = Nrbf { b, p: 0, meta: HashMap::new(), g: Graph::default() };
    loop {
        match n.record()? {
            Rec::End => return Ok((n.g, n.p)),
            Rec::Value(_) | Rec::Nulls(_) => {}
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Paint.NET.

/// Paint.NET's blend modes, in its enum order.
const PDN_BLENDS: [&str; 14] = [
    "Normal",
    "Multiply",
    "Additive",
    "ColorBurn",
    "ColorDodge",
    "Reflect",
    "Glow",
    "Overlay",
    "Difference",
    "Negation",
    "Lighten",
    "Darken",
    "Screen",
    "Xor",
];

fn blend_from_class(class: &str) -> BlendMode {
    let op = class.rsplit('+').next().unwrap_or(class).trim_end_matches("BlendOp");
    match op {
        "Multiply" => BlendMode::Multiply,
        "Additive" => BlendMode::LinearDodge,
        "ColorBurn" => BlendMode::ColorBurn,
        "ColorDodge" => BlendMode::ColorDodge,
        "Overlay" => BlendMode::Overlay,
        "Difference" => BlendMode::Difference,
        "Lighten" => BlendMode::Lighten,
        "Darken" => BlendMode::Darken,
        "Screen" => BlendMode::Screen,
        "Xor" => BlendMode::Exclusion,
        "Negation" => BlendMode::Exclusion,
        "Glow" => BlendMode::ColorDodge,
        "Reflect" => BlendMode::ColorDodge,
        _ => BlendMode::Normal,
    }
}

/// Decode one layer's pixel stream (BGRA, premultiplied = false) at `p`; returns the bytes and
/// the offset after it.
fn read_block(b: &[u8], mut p: usize, len: usize) -> Result<(Vec<u8>, usize)> {
    let take = |p: &mut usize, n: usize| -> Result<&[u8]> {
        let s = b.get(*p..*p + n).ok_or_else(|| bad("pixel data truncated"))?;
        *p += n;
        Ok(s)
    };
    let format = take(&mut p, 1)?[0];
    let chunk = u32::from_be_bytes(take(&mut p, 4)?.try_into().unwrap()) as usize;
    let mut out = vec![0u8; len];
    if format == 1 {
        // Uncompressed.
        out.copy_from_slice(take(&mut p, len)?);
        return Ok((out, p));
    }
    if chunk == 0 {
        return Err(bad("bad chunk size"));
    }
    let chunks = len.div_ceil(chunk);
    for _ in 0..chunks {
        let index = u32::from_be_bytes(take(&mut p, 4)?.try_into().unwrap()) as usize;
        let size = u32::from_be_bytes(take(&mut p, 4)?.try_into().unwrap()) as usize;
        let data = take(&mut p, size)?;
        let start = index.checked_mul(chunk).filter(|&s| s < len).ok_or_else(|| bad("bad chunk index"))?;
        let end = (start + chunk).min(len);
        let mut z = flate2::read::GzDecoder::new(data);
        z.read_exact(&mut out[start..end]).map_err(|e| bad(format!("chunk: {e}")))?;
    }
    Ok((out, p))
}

pub fn read(bytes: &[u8]) -> Result<Document> {
    if bytes.get(..4) != Some(b"PDN3") || bytes.len() < 7 {
        return Err(bad("not a Paint.NET file"));
    }
    let hlen = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], 0]) as usize;
    let mut p = 7 + hlen;
    match bytes.get(p..p + 2) {
        Some([0x00, 0x01]) => p += 2,
        Some([0x1f, 0x8b]) => return Err(bad("this older compressed format isn't supported")),
        _ => return Err(bad("unknown body")),
    }
    let (g, used) = parse_nrbf(&bytes[p..])?;
    let mut data_at = p + used;

    let root = g.objects.get(&g.root).ok_or_else(|| bad("no document"))?;
    let w = root.get("width").and_then(|v| g.int(v)).ok_or_else(|| bad("no width"))? as u32;
    let h = root.get("height").and_then(|v| g.int(v)).ok_or_else(|| bad("no height"))? as u32;
    if w == 0 || h == 0 || w > 65_535 || h > 65_535 {
        return Err(bad("bad size"));
    }
    // layers → LayerList → ArrayList._items (array of layer refs) + _size.
    let list = root.get("layers").and_then(|v| g.obj(v)).ok_or_else(|| bad("no layer list"))?;
    let items = list.get("_items").map(|v| g.resolve(v)).ok_or_else(|| bad("no layer items"))?;
    let size = list.get("_size").and_then(|v| g.int(v)).unwrap_or(0).max(0) as usize;
    let layer_refs: Vec<&Value> = match items {
        Value::List(v) => v.iter().take(size).collect(),
        _ => Vec::new(),
    };

    let mut doc = Document::new(w, h, None);
    doc.layers.clear();
    for lv in layer_refs {
        let Some(layer) = g.obj(lv) else { continue };
        // The base Layer's properties hold name, visibility, opacity and (4.x) the blend mode.
        let props = layer
            .members
            .iter()
            .filter_map(|(_, v)| g.obj(v))
            .find(|o| o.class.ends_with("Layer+LayerProperties"))
            .or_else(|| layer.get("properties").and_then(|v| g.obj(v)));
        let name = props.and_then(|p| p.get("name")).and_then(|v| g.str(v)).unwrap_or_else(|| "Layer".into());
        let visible = props.and_then(|p| p.get("visible")).and_then(|v| g.int(v)).is_none_or(|v| v != 0);
        let opacity = props.and_then(|p| p.get("opacity")).and_then(|v| g.int(v)).unwrap_or(255) as f32 / 255.0;
        let enum_blend = props
            .and_then(|p| p.get("blendMode"))
            .and_then(|v| g.obj(v))
            .and_then(|o| o.get("value__"))
            .and_then(|v| g.int(v))
            .and_then(|i| PDN_BLENDS.get(i as usize))
            .map(|n| blend_from_class(n));
        // 3.x: an op object on the bitmap layer's own properties.
        let op_blend = layer
            .members
            .iter()
            .filter_map(|(_, v)| g.obj(v))
            .filter_map(|o| o.get("blendOp"))
            .filter_map(|v| g.obj(v))
            .map(|o| blend_from_class(&o.class))
            .next();
        let blend = enum_blend.or(op_blend).unwrap_or(BlendMode::Normal);
        let surface = layer.get("surface").and_then(|v| g.obj(v)).ok_or_else(|| bad("layer without pixels"))?;
        let sw = surface.get("width").and_then(|v| g.int(v)).unwrap_or(w as i64) as usize;
        let sh = surface.get("height").and_then(|v| g.int(v)).unwrap_or(h as i64) as usize;
        let stride = surface.get("stride").and_then(|v| g.int(v)).unwrap_or(sw as i64 * 4) as usize;
        let (bgra, next) = read_block(bytes, data_at, stride * sh)?;
        data_at = next;
        let mut rgba = vec![0u8; w as usize * h as usize * 4];
        for y in 0..sh.min(h as usize) {
            for x in 0..sw.min(w as usize) {
                let s = y * stride + x * 4;
                let d = (y * w as usize + x) * 4;
                rgba[d..d + 4].copy_from_slice(&[bgra[s + 2], bgra[s + 1], bgra[s], bgra[s + 3]]);
            }
        }
        let id = doc.new_id();
        let mut l = Layer::raster(id, name, Image::from_raw(w, h, rgba));
        l.visible = visible;
        l.opacity = opacity;
        l.blend = blend;
        doc.layers.push(l);
    }
    if doc.layers.is_empty() {
        return Err(bad("no layers"));
    }
    doc.active = doc.layers.len() - 1;
    Ok(doc)
}

/// Debug helper: the object graph's classes and members.
#[doc(hidden)]
pub fn dump(bytes: &[u8]) -> String {
    let hlen = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], 0]) as usize;
    match parse_nrbf(&bytes[7 + hlen + 2..]) {
        Ok((g, used)) => {
            let mut ids: Vec<_> = g.objects.keys().copied().collect();
            ids.sort();
            let mut s = format!("root {} nrbf {} bytes\n", g.root, used);
            for id in ids {
                let o = &g.objects[&id];
                s += &format!("#{id} {}\n", o.class);
                for (n, v) in &o.members {
                    let v = match v {
                        Value::Bytes(b) => format!("bytes[{}]", b.len()),
                        Value::Ref(r) => match g.strings.get(r) {
                            Some(t) => format!("\"{t}\""),
                            None => format!("→#{r}"),
                        },
                        other => format!("{other:?}"),
                    };
                    s += &format!("    {n} = {v}\n");
                }
            }
            s
        }
        Err(e) => format!("error: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Rgba;

    fn lps(out: &mut Vec<u8>, s: &str) {
        out.push(s.len() as u8);
        out.extend_from_slice(s.as_bytes());
    }

    /// A class record with typed members. `types` are (binary type, extra) as in MS-NRBF; for
    /// class-typed members the extra is the class name.
    fn class(out: &mut Vec<u8>, id: i32, name: &str, members: &[(&str, u8, &str)]) {
        out.push(5);
        out.extend_from_slice(&id.to_le_bytes());
        lps(out, name);
        out.extend_from_slice(&(members.len() as i32).to_le_bytes());
        for (m, ..) in members {
            lps(out, m);
        }
        for (_, t, _) in members {
            out.push(*t);
        }
        for (_, t, extra) in members {
            match t {
                0 => out.push(extra.parse::<u8>().unwrap()),
                4 => {
                    lps(out, extra);
                    out.extend_from_slice(&2i32.to_le_bytes());
                }
                _ => {}
            }
        }
        out.extend_from_slice(&2i32.to_le_bytes()); // library
    }

    fn reference(out: &mut Vec<u8>, id: i32) {
        out.push(9);
        out.extend_from_slice(&id.to_le_bytes());
    }

    fn gzip(data: &[u8]) -> Vec<u8> {
        let mut z = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        std::io::Write::write_all(&mut z, data).unwrap();
        z.finish().unwrap()
    }

    /// A one-layer 4×3 Paint.NET file shaped like Paint.NET 4's output.
    fn sample() -> Vec<u8> {
        let (w, h) = (4i32, 3i32);
        let mut n = vec![0u8];
        for v in [1i32, -1, 1, 0] {
            n.extend_from_slice(&v.to_le_bytes());
        }
        n.push(12);
        n.extend_from_slice(&2i32.to_le_bytes());
        lps(&mut n, "PaintDotNet.Base");
        class(
            &mut n,
            1,
            "PaintDotNet.Document",
            &[("layers", 4, "PaintDotNet.LayerList"), ("width", 0, "8"), ("height", 0, "8")],
        );
        reference(&mut n, 3);
        n.extend_from_slice(&w.to_le_bytes());
        n.extend_from_slice(&h.to_le_bytes());
        class(&mut n, 3, "PaintDotNet.LayerList", &[("ArrayList+_items", 5, ""), ("ArrayList+_size", 0, "8")]);
        reference(&mut n, 7);
        n.extend_from_slice(&1i32.to_le_bytes());
        n.push(16);
        n.extend_from_slice(&7i32.to_le_bytes());
        n.extend_from_slice(&2i32.to_le_bytes());
        reference(&mut n, 20);
        n.push(13); // one null slot
        n.push(1);
        class(
            &mut n,
            20,
            "PaintDotNet.BitmapLayer",
            &[("surface", 4, "PaintDotNet.Surface"), ("Layer+properties", 4, "PaintDotNet.Layer+LayerProperties")],
        );
        reference(&mut n, 23);
        reference(&mut n, 24);
        class(&mut n, 23, "PaintDotNet.Surface", &[("width", 0, "8"), ("height", 0, "8"), ("stride", 0, "8")]);
        for v in [w, h, w * 4] {
            n.extend_from_slice(&v.to_le_bytes());
        }
        class(
            &mut n,
            24,
            "PaintDotNet.Layer+LayerProperties",
            &[
                ("name", 1, ""),
                ("visible", 0, "1"),
                ("opacity", 0, "2"),
                ("blendMode", 4, "PaintDotNet.LayerBlendMode"),
            ],
        );
        n.push(6);
        n.extend_from_slice(&30i32.to_le_bytes());
        lps(&mut n, "Ink");
        n.push(1);
        n.push(128);
        class(&mut n, 31, "PaintDotNet.LayerBlendMode", &[("value__", 0, "8")]);
        n.extend_from_slice(&1i32.to_le_bytes()); // Multiply
        n.push(11);

        let mut f = b"PDN3".to_vec();
        let header = br#"<pdnImage width="4" height="3" layers="1" />"#;
        f.extend_from_slice(&(header.len() as u32).to_le_bytes()[..3]);
        f.extend_from_slice(header);
        f.extend_from_slice(&[0, 1]);
        f.extend_from_slice(&n);
        // Pixels: BGRA, red everywhere except a blue pixel at (1, 2); 48 bytes in 32-byte chunks,
        // written out of order.
        let mut bgra = Vec::new();
        for i in 0..(w * h) {
            bgra.extend_from_slice(if i == 2 * w + 1 { &[255, 0, 0, 255] } else { &[0, 0, 255, 255] });
        }
        f.push(0);
        f.extend_from_slice(&32u32.to_be_bytes());
        for (index, part) in [(1u32, &bgra[32..]), (0, &bgra[..32])] {
            let z = gzip(part);
            f.extend_from_slice(&index.to_be_bytes());
            f.extend_from_slice(&(z.len() as u32).to_be_bytes());
            f.extend_from_slice(&z);
        }
        f
    }

    #[test]
    fn reads_layers_properties_and_chunked_pixels() {
        let d = read(&sample()).unwrap();
        assert_eq!((d.width, d.height), (4, 3));
        assert_eq!(d.layers.len(), 1);
        let l = &d.layers[0];
        assert_eq!(l.name, "Ink");
        assert_eq!(l.blend, BlendMode::Multiply);
        assert!((l.opacity - 128.0 / 255.0).abs() < 1e-3);
        assert_eq!(l.pixels.get(0, 0), Rgba::rgb(255, 0, 0));
        assert_eq!(l.pixels.get(1, 2), Rgba::rgb(0, 0, 255), "BGRA order and the second chunk");
    }

    #[test]
    fn rejects_garbage_without_panicking() {
        assert!(read(b"PDN3\x05\x00\x00hello").is_err());
        let mut s = sample();
        s.truncate(s.len() - 20);
        assert!(read(&s).is_err());
        for cut in (8..200).step_by(7) {
            let _ = read(&sample()[..cut]);
        }
    }
}
