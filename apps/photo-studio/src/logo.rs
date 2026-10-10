//! The Photo Studio mark: a six-blade camera iris in spectrum colours on a dark squircle.
//!
//! One geometry drives every rendition: app icons (PNG, ICNS, ICO), the animated logo on the
//! welcome screen, and the animated SVG on the website. `phase` (0..1, looping) opens and
//! closes the iris while it turns.

use std::f32::consts::{PI, TAU};

use tiny_skia::{
    BlendMode, Color, FillRule, GradientStop, LinearGradient, Paint, PathBuilder, Pixmap, Point, RadialGradient,
    Shader, SpreadMode, Stroke, Transform,
};

/// Blade colours, clockwise from the top.
pub const BLADES: [[u8; 3]; 6] = [
    [255, 55, 95],  // pink
    [255, 159, 10], // orange
    [255, 214, 10], // yellow
    [48, 209, 88],  // green
    [10, 132, 255], // blue
    [191, 90, 242], // purple
];

/// The phase used for the static icon: a pleasing, mid-open iris.
pub const ICON_PHASE: f32 = 0.30;

const ARC_STEPS: usize = 24;

fn polar(r: f32, a: f32) -> (f32, f32) {
    (r * a.cos(), r * a.sin())
}

/// Opening radius (fraction of the iris radius) and rotation (radians) at `phase`.
fn iris_state(phase: f32) -> (f32, f32) {
    let wave = 0.5 - 0.5 * (phase * TAU).cos();
    let open = 0.16 + 0.30 * wave;
    let rot = -PI / 2.0 + 0.55 * wave;
    (open, rot)
}

/// Blade outlines for an iris of radius 1 centred on the origin, as closed polygons.
/// Every blade has the same number of points at every phase, so they can be morphed.
pub fn blades(phase: f32) -> Vec<Vec<(f32, f32)>> {
    let (r0, rot) = iris_state(phase);
    let r = 1.0f32;
    let v: Vec<(f32, f32)> = (0..6).map(|k| polar(r0, rot + k as f32 * TAU / 6.0)).collect();
    // Where the extension of hexagon edge i (from v[i] through v[i+1]) meets the outer circle.
    let hit = |i: usize| -> (f32, f32) {
        let a = v[i % 6];
        let b = v[(i + 1) % 6];
        let d = (b.0 - a.0, b.1 - a.1);
        let qa = d.0 * d.0 + d.1 * d.1;
        let qb = 2.0 * (a.0 * d.0 + a.1 * d.1);
        let qc = a.0 * a.0 + a.1 * a.1 - r * r;
        let t = (-qb + (qb * qb - 4.0 * qa * qc).sqrt()) / (2.0 * qa);
        (a.0 + t * d.0, a.1 + t * d.1)
    };
    // A little overlap between neighbours lets the screen blend draw bright seams.
    let overlap = 0.16;
    (0..6)
        .map(|i| {
            let a0 = hit(i);
            let a1 = hit(i + 1);
            let t0 = a0.1.atan2(a0.0);
            let mut t1 = a1.1.atan2(a1.0);
            while t1 < t0 {
                t1 += TAU;
            }
            let t1 = t1 + overlap;
            let mut pts = vec![v[(i + 1) % 6]];
            for s in 0..=ARC_STEPS {
                let t = t0 + (t1 - t0) * s as f32 / ARC_STEPS as f32;
                pts.push(polar(r, t));
            }
            // Return along the next blade's edge, pushed slightly past the vertex for overlap.
            let n = v[(i + 2) % 6];
            let m = v[(i + 1) % 6];
            pts.push((n.0 + (n.0 - m.0) * overlap, n.1 + (n.1 - m.1) * overlap));
            pts
        })
        .collect()
}

fn lighten(c: [u8; 3], k: f32) -> Color {
    let f = |v: u8| (v as f32 / 255.0 + (1.0 - v as f32 / 255.0) * k).min(1.0);
    Color::from_rgba(f(c[0]), f(c[1]), f(c[2]), 1.0).unwrap()
}

fn rgb(c: [u8; 3], a: f32) -> Color {
    Color::from_rgba(c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0, a).unwrap()
}

fn squircle(pb: &mut PathBuilder, x: f32, y: f32, s: f32) {
    // Superellipse-like rounded square (continuous corners), approximated with cubics.
    let r = s * 0.2237;
    let k = r * 1.28;
    let (x1, y1) = (x + s, y + s);
    pb.move_to(x + k, y);
    pb.line_to(x1 - k, y);
    pb.cubic_to(x1 - k * 0.32, y, x1, y + k * 0.32, x1, y + k);
    pb.line_to(x1, y1 - k);
    pb.cubic_to(x1, y1 - k * 0.32, x1 - k * 0.32, y1, x1 - k, y1);
    pb.line_to(x + k, y1);
    pb.cubic_to(x + k * 0.32, y1, x, y1 - k * 0.32, x, y1 - k);
    pb.line_to(x, y + k);
    pb.cubic_to(x, y + k * 0.32, x + k * 0.32, y, x + k, y);
    pb.close();
}

/// Render the mark at `size × size` pixels. `background` draws the squircle tile; without
/// it the iris floats on transparency (used on the welcome screen).
pub fn render(size: u32, phase: f32, background: bool) -> Pixmap {
    let mut pm = Pixmap::new(size, size).expect("non-zero size");
    let s = size as f32;
    let (cx, cy) = (s / 2.0, s / 2.0);
    let radius = if background { s * 0.30 } else { s * 0.46 };

    if background {
        // macOS-style grid: the tile fills ~80% of the canvas.
        let m = s * 0.098;
        let tile = s - 2.0 * m;
        let mut pb = PathBuilder::new();
        squircle(&mut pb, m, m, tile);
        let path = pb.finish().unwrap();
        let mut paint = Paint { anti_alias: true, ..Default::default() };
        paint.shader = LinearGradient::new(
            Point::from_xy(cx, m),
            Point::from_xy(cx, m + tile),
            vec![
                GradientStop::new(0.0, Color::from_rgba8(40, 40, 62, 255)),
                GradientStop::new(1.0, Color::from_rgba8(10, 10, 18, 255)),
            ],
            SpreadMode::Pad,
            Transform::identity(),
        )
        .unwrap();
        pm.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
        // Soft glow behind the iris.
        paint.shader = RadialGradient::new(
            Point::from_xy(cx, cy),
            Point::from_xy(cx, cy),
            radius * 1.6,
            vec![
                GradientStop::new(0.0, Color::from_rgba8(120, 90, 255, 70)),
                GradientStop::new(1.0, Color::from_rgba8(120, 90, 255, 0)),
            ],
            SpreadMode::Pad,
            Transform::identity(),
        )
        .unwrap();
        pm.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
        // Hairline edge highlight.
        let mut edge = Paint { anti_alias: true, ..Default::default() };
        edge.set_color(Color::from_rgba8(255, 255, 255, 28));
        pm.stroke_path(
            &path,
            &edge,
            &Stroke { width: (s / 512.0).max(1.0), ..Default::default() },
            Transform::identity(),
            None,
        );
    }

    let tf = Transform::from_row(radius, 0.0, 0.0, radius, cx, cy);
    for (i, blade) in blades(phase).iter().enumerate() {
        let mut pb = PathBuilder::new();
        pb.move_to(blade[0].0, blade[0].1);
        for p in &blade[1..] {
            pb.line_to(p.0, p.1);
        }
        pb.close();
        let path = pb.finish().unwrap();
        let mid = blade[blade.len() / 2];
        let c = BLADES[i];
        let mut paint = Paint { anti_alias: true, blend_mode: BlendMode::Screen, ..Default::default() };
        paint.shader = LinearGradient::new(
            Point::from_xy(0.0, 0.0),
            Point::from_xy(mid.0, mid.1),
            vec![GradientStop::new(0.0, rgb(c, 0.95)), GradientStop::new(1.0, lighten(c, 0.12))],
            SpreadMode::Pad,
            Transform::identity(),
        )
        .unwrap_or(Shader::SolidColor(rgb(c, 0.95)));
        pm.fill_path(&path, &paint, FillRule::Winding, tf, None);
    }

    // Lens rim.
    let mut pb = PathBuilder::new();
    pb.push_circle(cx, cy, radius * 1.0);
    let rim = pb.finish().unwrap();
    let mut paint = Paint { anti_alias: true, ..Default::default() };
    paint.set_color(Color::from_rgba8(255, 255, 255, 50));
    pm.stroke_path(
        &rim,
        &paint,
        &Stroke { width: (radius * 0.035).max(1.0), ..Default::default() },
        Transform::identity(),
        None,
    );

    // Glass highlight across the top of the lens.
    let mut pb = PathBuilder::new();
    pb.push_oval(
        tiny_skia::Rect::from_xywh(cx - radius * 0.8, cy - radius * 0.95, radius * 1.6, radius * 0.9).unwrap(),
    );
    let gloss = pb.finish().unwrap();
    paint.shader = LinearGradient::new(
        Point::from_xy(cx, cy - radius),
        Point::from_xy(cx, cy - radius * 0.1),
        vec![
            GradientStop::new(0.0, Color::from_rgba8(255, 255, 255, 42)),
            GradientStop::new(1.0, Color::from_rgba8(255, 255, 255, 0)),
        ],
        SpreadMode::Pad,
        Transform::identity(),
    )
    .unwrap();
    pm.fill_path(&gloss, &paint, FillRule::Winding, Transform::identity(), None);
    pm
}

/// Straight-alpha RGBA bytes of the mark.
pub fn rgba(size: u32, phase: f32, background: bool) -> Vec<u8> {
    let pm = render(size, phase, background);
    let mut v = Vec::with_capacity(pm.data().len());
    for p in pm.pixels() {
        let c = p.demultiply();
        v.extend_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
    }
    v
}

#[allow(dead_code)] // used by examples/gen_icons.rs
fn blade_d(b: &[(f32, f32)]) -> String {
    let mut d = String::new();
    for (i, p) in b.iter().enumerate() {
        d.push_str(&format!("{}{:.4} {:.4} ", if i == 0 { "M" } else { "L" }, p.0, p.1));
    }
    d.push('Z');
    d
}

/// The mark as SVG (viewBox 0 0 1024 1024). With `animated`, the iris opens, closes and turns
/// on a loop using SMIL path morphing, which every current browser supports.
#[allow(dead_code)] // used by examples/gen_icons.rs
pub fn svg(animated: bool, background: bool) -> String {
    let frames = 12;
    let keys: Vec<Vec<Vec<(f32, f32)>>> = (0..=frames).map(|f| blades(f as f32 / frames as f32)).collect();
    let still = blades(ICON_PHASE);
    let radius = if background { 307.2 } else { 471.0 };
    let mut out = String::new();
    out.push_str(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024" role="img" aria-label="Photo Studio">"#,
    );
    out.push_str("<defs>");
    out.push_str(r##"<linearGradient id="tile" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#28283e"/><stop offset="1" stop-color="#0a0a12"/></linearGradient>"##);
    out.push_str(r##"<radialGradient id="glow"><stop offset="0" stop-color="#785aff" stop-opacity=".28"/><stop offset="1" stop-color="#785aff" stop-opacity="0"/></radialGradient>"##);
    out.push_str(r##"<linearGradient id="gloss" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#fff" stop-opacity=".17"/><stop offset="1" stop-color="#fff" stop-opacity="0"/></linearGradient>"##);
    for (i, c) in BLADES.iter().enumerate() {
        let l = |v: u8| (v as f32 + (255.0 - v as f32) * 0.12) as u8;
        out.push_str(&format!(
            r##"<linearGradient id="b{i}" gradientUnits="userSpaceOnUse" x1="0" y1="0" x2="{:.3}" y2="{:.3}"><stop offset="0" stop-color="rgb({},{},{})" stop-opacity=".95"/><stop offset="1" stop-color="rgb({},{},{})"/></linearGradient>"##,
            still[i][still[i].len() / 2].0,
            still[i][still[i].len() / 2].1,
            c[0], c[1], c[2], l(c[0]), l(c[1]), l(c[2])
        ));
    }
    out.push_str("</defs>");
    if background {
        let m = 1024.0 * 0.098;
        let mut pb = PathBuilder::new();
        squircle(&mut pb, m, m, 1024.0 - 2.0 * m);
        let path = pb.finish().unwrap();
        let mut d = String::new();
        for seg in path.segments() {
            use tiny_skia::PathSegment::*;
            match seg {
                MoveTo(p) => d.push_str(&format!("M{:.1} {:.1}", p.x, p.y)),
                LineTo(p) => d.push_str(&format!("L{:.1} {:.1}", p.x, p.y)),
                CubicTo(a, b, p) => {
                    d.push_str(&format!("C{:.1} {:.1} {:.1} {:.1} {:.1} {:.1}", a.x, a.y, b.x, b.y, p.x, p.y))
                }
                QuadTo(a, p) => d.push_str(&format!("Q{:.1} {:.1} {:.1} {:.1}", a.x, a.y, p.x, p.y)),
                Close => d.push('Z'),
            }
        }
        out.push_str(&format!(
            r#"<path d="{d}" fill="url(#tile)"/><circle cx="512" cy="512" r="490" fill="url(#glow)"/>"#
        ));
    }
    out.push_str(&format!(r#"<g transform="translate(512 512) scale({radius})" style="isolation:isolate">"#));
    for i in 0..6 {
        out.push_str(&format!(r#"<path fill="url(#b{i})" style="mix-blend-mode:screen" d="{}">"#, blade_d(&still[i])));
        if animated {
            // Start the loop at the icon's phase so the first frame matches the static mark.
            let start = (ICON_PHASE * frames as f32).round() as usize;
            let values: Vec<String> = (0..=frames).map(|f| blade_d(&keys[(start + f) % frames][i])).collect();
            out.push_str(&format!(
                r#"<animate attributeName="d" dur="6s" repeatCount="indefinite" calcMode="spline" keySplines="{}" values="{}"/>"#,
                vec!["0.45 0 0.55 1"; frames].join(";"),
                values.join(";")
            ));
        }
        out.push_str("</path>");
    }
    out.push_str(r##"<circle r="1" fill="none" stroke="#fff" stroke-opacity=".2" stroke-width=".035"/>"##);
    out.push_str(r#"<ellipse cx="0" cy="-.5" rx=".8" ry=".45" fill="url(#gloss)"/>"#);
    out.push_str("</g></svg>");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blades_keep_point_count() {
        let a = blades(0.0);
        let b = blades(0.5);
        assert_eq!(a.len(), 6);
        for (x, y) in a.iter().zip(&b) {
            assert_eq!(x.len(), y.len());
        }
    }

    #[test]
    fn renders_opaque_centre_ring() {
        let px = rgba(128, ICON_PHASE, true);
        assert_eq!(px.len(), 128 * 128 * 4);
        // The tile is opaque in the middle of an edge, transparent in the corner.
        assert_eq!(px[(64 * 128 + 16) * 4 + 3], 255);
        assert_eq!(px[3], 0);
    }
}
