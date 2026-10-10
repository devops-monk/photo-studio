//! A Model Context Protocol server over stdio, so agents can drive the editing engine.
//!
//! `photo-studio-cli mcp` speaks JSON-RPC 2.0, one message per line. Documents live in memory
//! for the session and are addressed by the `doc` id `open` returns.

use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::path::Path;

use serde_json::{Value, json};
use studio_core::{Document, io};

use crate::{Op, parse_filter, run_ops};

const PROTOCOL: &str = "2025-06-18";

struct Session {
    docs: HashMap<u64, Document>,
    next: u64,
}

fn tools() -> Value {
    let doc = json!({"type": "integer", "description": "Document id returned by `open`"});
    json!([
        {
            "name": "open",
            "description": "Open an image or project (PNG, JPEG, WebP, TIFF, PSD, ORA, SVG, .pstudio, …) and return a document id.",
            "inputSchema": {"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]}
        },
        {
            "name": "new",
            "description": "Create a blank document.",
            "inputSchema": {"type": "object", "properties": {
                "width": {"type": "integer"}, "height": {"type": "integer"},
                "background": {"type": "string", "description": "Hex colour, or 'transparent'"}
            }, "required": ["width", "height"]}
        },
        {
            "name": "info",
            "description": "Size, layers and average colour of a document.",
            "inputSchema": {"type": "object", "properties": {"doc": doc}, "required": ["doc"]}
        },
        {
            "name": "adjust",
            "description": "Apply an adjustment, e.g. 'brightness=20', 'autotone', 'hue=30', 'levels=10,240,1.1', 'bw', 'sepia'. Run `photo-studio-cli list` for all.",
            "inputSchema": {"type": "object", "properties": {"doc": doc, "spec": {"type": "string"}}, "required": ["doc", "spec"]}
        },
        {
            "name": "filter",
            "description": "Apply a filter, e.g. 'blur=4', 'unsharp=80,1.5', 'vignette=-50', 'oil=4', 'iris=15'.",
            "inputSchema": {"type": "object", "properties": {"doc": doc, "spec": {"type": "string"}}, "required": ["doc", "spec"]}
        },
        {
            "name": "transform",
            "description": "Resize ('50%', '1600', '800x600'), rotate (degrees), flip ('h'/'v'), crop ('x,y,w,h') or trim.",
            "inputSchema": {"type": "object", "properties": {
                "doc": doc,
                "resize": {"type": "string"}, "rotate": {"type": "number"},
                "flip": {"type": "string"}, "crop": {"type": "string"}, "trim": {"type": "boolean"}
            }, "required": ["doc"]}
        },
        {
            "name": "select_subject",
            "description": "Select the main subject, then optionally remove the background (makes it transparent).",
            "inputSchema": {"type": "object", "properties": {"doc": doc, "remove_background": {"type": "boolean"}}, "required": ["doc"]}
        },
        {
            "name": "export",
            "description": "Save the document. The extension picks the format (.png, .jpg, .webp, .psd, .ora, .pstudio, …).",
            "inputSchema": {"type": "object", "properties": {"doc": doc, "path": {"type": "string"}, "quality": {"type": "integer"}}, "required": ["doc", "path"]}
        },
        {
            "name": "close",
            "description": "Forget a document.",
            "inputSchema": {"type": "object", "properties": {"doc": doc}, "required": ["doc"]}
        }
    ])
}

fn text(s: impl Into<String>) -> Value {
    json!({"content": [{"type": "text", "text": s.into()}]})
}

fn summary(id: u64, d: &Document) -> String {
    let comp = d.composite();
    let h = studio_core::histogram::Histogram::of(&comp, None);
    let layers: Vec<String> = d.layers.iter().rev().map(|l| format!("{} ({})", l.name, l.kind.badge())).collect();
    format!(
        "doc {id}: {} × {} px, {} layer(s): {}. Mean RGB {:.0}, {:.0}, {:.0}.",
        d.width,
        d.height,
        d.layers.len(),
        layers.join(", "),
        h.mean(0),
        h.mean(1),
        h.mean(2)
    )
}

impl Session {
    fn doc(&mut self, args: &Value) -> Result<(u64, &mut Document), String> {
        let id = args.get("doc").and_then(Value::as_u64).ok_or("missing `doc`")?;
        self.docs.get_mut(&id).map(|d| (id, d)).ok_or_else(|| format!("no document {id}"))
    }

    fn call(&mut self, name: &str, args: &Value) -> Result<String, String> {
        let s = |k: &str| args.get(k).and_then(Value::as_str);
        match name {
            "open" => {
                let p = s("path").ok_or("missing `path`")?;
                let d = io::open(Path::new(p)).map_err(|e| format!("{p}: {e}"))?;
                let id = self.next;
                self.next += 1;
                let out = summary(id, &d);
                self.docs.insert(id, d);
                Ok(out)
            }
            "new" => {
                let w = args.get("width").and_then(Value::as_u64).ok_or("missing `width`")? as u32;
                let h = args.get("height").and_then(Value::as_u64).ok_or("missing `height`")? as u32;
                if w == 0 || h == 0 || w > 30_000 || h > 30_000 {
                    return Err("width and height must be 1..30000".into());
                }
                let bg = match s("background") {
                    Some("transparent") => None,
                    Some(hex) => Some(studio_core::Rgba::from_hex(hex).ok_or("bad colour")?),
                    None => Some(studio_core::Rgba::WHITE),
                };
                let id = self.next;
                self.next += 1;
                self.docs.insert(id, Document::new(w, h, bg));
                Ok(format!("doc {id}: {w} × {h} px"))
            }
            "info" => {
                let (id, d) = self.doc(args)?;
                Ok(summary(id, d))
            }
            "adjust" => {
                let spec = s("spec").ok_or("missing `spec`")?.to_string();
                let (id, d) = self.doc(args)?;
                run_ops(d, &[Op::Adjust(spec)])?;
                Ok(summary(id, d))
            }
            "filter" => {
                let spec = s("spec").ok_or("missing `spec`")?.to_string();
                parse_filter(&spec)?;
                let (id, d) = self.doc(args)?;
                run_ops(d, &[Op::Filter(spec)])?;
                Ok(summary(id, d))
            }
            "transform" => {
                let mut ops = Vec::new();
                if let Some(r) = s("resize") {
                    ops.push(Op::Resize(r.to_string()));
                }
                if let Some(deg) = args.get("rotate").and_then(Value::as_f64) {
                    ops.push(Op::Rotate(deg as f32));
                }
                if let Some(f) = s("flip") {
                    ops.push(Op::Flip(f.starts_with('h')));
                }
                if let Some(c) = s("crop") {
                    let v: Vec<i64> = c.split(',').filter_map(|x| x.trim().parse().ok()).collect();
                    if v.len() != 4 {
                        return Err("crop takes x,y,w,h".into());
                    }
                    ops.push(Op::Crop(studio_core::Rect::new(v[0] as i32, v[1] as i32, v[2] as u32, v[3] as u32)));
                }
                if args.get("trim").and_then(Value::as_bool) == Some(true) {
                    ops.push(Op::Trim);
                }
                let (id, d) = self.doc(args)?;
                run_ops(d, &ops)?;
                Ok(summary(id, d))
            }
            "select_subject" => {
                let remove = args.get("remove_background").and_then(Value::as_bool).unwrap_or(false);
                let (id, d) = self.doc(args)?;
                let m = studio_core::select::select_subject(&d.composite());
                let coverage = m.data().iter().filter(|&&v| v > 127).count() as f64 / m.data().len().max(1) as f64;
                if remove {
                    d.flatten_image();
                    let mask = m.clone();
                    let l = &mut d.layers[0];
                    for (i, p) in l.pixels.pixels_mut().chunks_exact_mut(4).enumerate() {
                        p[3] = ((p[3] as u32 * mask.data()[i] as u32 + 127) / 255) as u8;
                    }
                }
                d.set_selection(m);
                Ok(format!("{} Subject covers {:.0}% of the image.", summary(id, d), coverage * 100.0))
            }
            "export" => {
                let p = s("path").ok_or("missing `path`")?.to_string();
                let q = args.get("quality").and_then(Value::as_u64).unwrap_or(90).clamp(1, 100) as u8;
                let (_, d) = self.doc(args)?;
                crate::save(d, Path::new(&p), q)?;
                Ok(format!("saved {p}"))
            }
            "close" => {
                let id = args.get("doc").and_then(Value::as_u64).ok_or("missing `doc`")?;
                self.docs.remove(&id).map(|_| format!("closed doc {id}")).ok_or_else(|| format!("no document {id}"))
            }
            other => Err(format!("unknown tool `{other}`")),
        }
    }

    /// Handle one JSON-RPC message; `None` for notifications.
    fn handle(&mut self, msg: &Value) -> Option<Value> {
        let id = msg.get("id").cloned();
        let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
        let result = match method {
            "initialize" => Ok(json!({
                "protocolVersion": PROTOCOL,
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "photo-studio", "version": studio_core::VERSION}
            })),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({"tools": tools()})),
            "tools/call" => {
                let params = msg.get("params").cloned().unwrap_or(Value::Null);
                let name = params.get("name").and_then(Value::as_str).unwrap_or("");
                let args = params.get("arguments").cloned().unwrap_or(json!({}));
                Ok(match self.call(name, &args) {
                    Ok(t) => text(t),
                    Err(e) => {
                        let mut v = text(e);
                        v["isError"] = json!(true);
                        v
                    }
                })
            }
            _ if id.is_none() => return None, // notifications (e.g. notifications/initialized)
            other => Err(json!({"code": -32601, "message": format!("method not found: {other}")})),
        };
        let id = id?;
        Some(match result {
            Ok(r) => json!({"jsonrpc": "2.0", "id": id, "result": r}),
            Err(e) => json!({"jsonrpc": "2.0", "id": id, "error": e}),
        })
    }
}

pub fn serve() -> Result<(), String> {
    let mut session = Session { docs: HashMap::new(), next: 1 };
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line.map_err(|e| e.to_string())?;
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(msg) => session.handle(&msg),
            Err(e) => Some(json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": e.to_string()}})),
        };
        if let Some(r) = reply {
            writeln!(out, "{r}").map_err(|e| e.to_string())?;
            out.flush().map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(s: &mut Session, id: u64, name: &str, args: Value) -> Value {
        s.handle(
            &json!({"jsonrpc": "2.0", "id": id, "method": "tools/call", "params": {"name": name, "arguments": args}}),
        )
        .unwrap()
    }

    #[test]
    fn protocol_round_trip() {
        let mut s = Session { docs: HashMap::new(), next: 1 };
        let init = s.handle(&json!({"jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {}})).unwrap();
        assert_eq!(init["result"]["serverInfo"]["name"], "photo-studio");
        assert!(s.handle(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"})).is_none());
        let list = s.handle(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"})).unwrap();
        assert!(list["result"]["tools"].as_array().unwrap().len() >= 8);

        let r = call(&mut s, 2, "new", json!({"width": 40, "height": 20, "background": "#ff0000"}));
        assert!(r["result"]["content"][0]["text"].as_str().unwrap().starts_with("doc 1"));
        let r = call(&mut s, 3, "adjust", json!({"doc": 1, "spec": "invert"}));
        assert!(r["result"]["content"][0]["text"].as_str().unwrap().contains("Mean RGB 0, 255, 255"), "{r}");
        let r = call(&mut s, 4, "transform", json!({"doc": 1, "resize": "50%"}));
        assert!(r["result"]["content"][0]["text"].as_str().unwrap().contains("20 × 10"));
        let bad = call(&mut s, 5, "filter", json!({"doc": 1, "spec": "nope"}));
        assert_eq!(bad["result"]["isError"], true);
        let path = std::env::temp_dir().join(format!("ps-mcp-{}.png", std::process::id()));
        let r = call(&mut s, 6, "export", json!({"doc": 1, "path": path.to_string_lossy()}));
        assert!(r["result"]["content"][0]["text"].as_str().unwrap().starts_with("saved"));
        assert_eq!(io::read_image(&path).unwrap().size(), (20, 10));
        std::fs::remove_file(path).ok();
    }
}
