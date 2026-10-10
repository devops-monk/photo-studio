//! photo-studio-cli: batch image editing with the Photo Studio engine.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use studio_core::adjust::{self, Adjustment, LevelsChannel};
use studio_core::filters::{Filter, FilterCtx, NoiseDist, RadialMode};
use studio_core::geom::Rect;
use studio_core::transform::{Resample, Rotation};
use studio_core::{Document, Rgba, io};

const HELP: &str = "\
photo-studio-cli — batch image editing with the Photo Studio engine

USAGE:
    photo-studio-cli <input> [operations…] -o <output>
    photo-studio-cli batch <inputs or folders…> --out-dir <dir> [--format <ext>] [operations…]
    photo-studio-cli info <file>…
    photo-studio-cli list

Operations run in the order given:
    --resize <W | W% | WxH>        Resize (keeps aspect ratio unless WxH)
    --rotate <90|180|270|degrees>  Rotate the canvas
    --flip <h|v>                   Flip horizontally or vertically
    --crop <x,y,w,h>               Crop to a rectangle
    --trim                         Crop away transparent borders
    --adjust <name[=value]>        Apply an adjustment (see `list`)
    --filter <name[=value,…]>      Apply a filter (see `list`)
    --flatten                      Merge layers onto white

Options:
    -o, --output <file>            Output file; the extension picks the format
    --out-dir <dir>                Output folder for `batch`
    --format <ext>                 Output format for `batch` (default: same as input)
    --quality <1-100>              JPEG quality (default 90)
    -h, --help                     Show this help
    -V, --version                  Show the version

Examples:
    photo-studio-cli photo.jpg --adjust autotone --filter unsharp=80,1.5 -o photo-fixed.jpg
    photo-studio-cli scan.png --resize 50% --adjust bw --adjust contrast=20 -o scan.webp
    photo-studio-cli batch ./raw --out-dir ./web --format webp --resize 1600
";

const LIST: &str = "\
Adjustments (--adjust):
    brightness=<-100..100>   contrast=<-100..100>   exposure=<-5..5>
    vibrance=<-100..100>     saturation=<-100..100> hue=<-180..180>
    levels=<black,white[,gamma]>                     invert   desaturate
    bw                       sepia                  warm=<0..100>   cool=<0..100>
    posterize=<2..64>        threshold=<0..255>     equalize
    autotone                 autocontrast           autocolor
    shadows=<0..100>         highlights=<0..100>

Filters (--filter):
    blur=<radius>            box=<radius>           motion=<angle,distance>
    radial=<amount>          zoom=<amount>          surface=<radius,threshold>
    tiltshift=<blur>         iris=<blur>            sharpen    sharpenmore
    unsharp=<amount,radius[,threshold]>             noise=<amount>
    median=<radius>          denoise=<strength>     dust=<radius,threshold>
    twirl=<degrees>          wave=<amplitude,wavelength>   ripple=<amount>
    pinch=<amount>           spherize=<amount>      polar
    pixelate=<cell>          crystallize=<cell>     halftone=<radius>
    emboss                   edges                  glow=<width>
    oil=<radius>             solarize               vignette=<amount>
    clouds                   flare
";

#[derive(Debug, Clone)]
enum Op {
    Resize(String),
    Rotate(f32),
    Flip(bool),
    Crop(Rect),
    Trim,
    Adjust(String),
    Filter(String),
    Flatten,
}

fn nums(v: &str) -> Result<Vec<f32>, String> {
    if v.is_empty() {
        return Ok(Vec::new());
    }
    v.split(',').map(|s| s.trim().parse::<f32>().map_err(|_| format!("not a number: `{s}`"))).collect()
}

fn arg(name: &str, v: &[f32], i: usize, default: Option<f32>) -> Result<f32, String> {
    v.get(i).copied().or(default).ok_or_else(|| format!("`{name}` needs a value"))
}

fn split(spec: &str) -> (String, String) {
    match spec.split_once('=') {
        Some((n, v)) => (n.trim().to_ascii_lowercase(), v.trim().to_string()),
        None => (spec.trim().to_ascii_lowercase(), String::new()),
    }
}

fn apply_adjust(doc: &mut Document, spec: &str) -> Result<(), String> {
    let (name, val) = split(spec);
    let v = nums(&val)?;
    let img = &mut doc.layers[doc.active].pixels;
    let a = match name.as_str() {
        "brightness" => Adjustment::BrightnessContrast { brightness: arg(&name, &v, 0, None)?, contrast: 0.0 },
        "contrast" => Adjustment::BrightnessContrast { brightness: 0.0, contrast: arg(&name, &v, 0, None)? },
        "exposure" => Adjustment::Exposure { exposure: arg(&name, &v, 0, None)?, offset: 0.0, gamma: 1.0 },
        "vibrance" => Adjustment::Vibrance { vibrance: arg(&name, &v, 0, None)?, saturation: 0.0 },
        "saturation" => Adjustment::HueSaturation {
            hue: 0.0,
            saturation: arg(&name, &v, 0, None)?,
            lightness: 0.0,
            colorize: false,
        },
        "hue" => Adjustment::HueSaturation {
            hue: arg(&name, &v, 0, None)?,
            saturation: 0.0,
            lightness: 0.0,
            colorize: false,
        },
        "levels" => Adjustment::Levels {
            master: LevelsChannel {
                in_black: arg(&name, &v, 0, Some(0.0))? as u8,
                in_white: arg(&name, &v, 1, Some(255.0))? as u8,
                gamma: arg(&name, &v, 2, Some(1.0))?,
                ..Default::default()
            },
            red: Default::default(),
            green: Default::default(),
            blue: Default::default(),
        },
        "invert" => Adjustment::Invert,
        "desaturate" | "grayscale" | "greyscale" => Adjustment::Desaturate,
        "bw" | "blackwhite" => Adjustment::catalog()[7].clone(),
        "sepia" => Adjustment::BlackWhite {
            weights: [40.0, 60.0, 40.0, 60.0, 20.0, 80.0],
            tint: Some(Rgba::rgb(170, 120, 60)),
        },
        "warm" => Adjustment::PhotoFilter {
            color: Rgba::rgb(236, 138, 0),
            density: arg(&name, &v, 0, Some(25.0))?,
            preserve_luminosity: true,
        },
        "cool" => Adjustment::PhotoFilter {
            color: Rgba::rgb(0, 109, 255),
            density: arg(&name, &v, 0, Some(25.0))?,
            preserve_luminosity: true,
        },
        "posterize" => Adjustment::Posterize { levels: arg(&name, &v, 0, Some(4.0))? as u8 },
        "threshold" => Adjustment::Threshold { level: arg(&name, &v, 0, Some(128.0))? as u8 },
        "autotone" => adjust::auto_tone(img, 0.001),
        "autocontrast" => adjust::auto_contrast(img, 0.001),
        "autocolor" => adjust::auto_color(img),
        "equalize" => {
            adjust::equalize(img, None);
            return Ok(());
        }
        "shadows" | "highlights" => {
            let amount = arg(&name, &v, 0, Some(35.0))?;
            let (s, h) = if name == "shadows" { (amount, 0.0) } else { (0.0, amount) };
            adjust::shadows_highlights(img, s, h, 30.0, None);
            return Ok(());
        }
        _ => return Err(format!("unknown adjustment `{name}` (see `photo-studio-cli list`)")),
    };
    adjust::apply(img, &a, None);
    Ok(())
}

fn parse_filter(spec: &str) -> Result<Filter, String> {
    let (name, val) = split(spec);
    let v = nums(&val)?;
    let a = |i: usize, d: f32| arg(&name, &v, i, Some(d));
    Ok(match name.as_str() {
        "blur" | "gaussian" => Filter::GaussianBlur { radius: a(0, 4.0)? },
        "box" => Filter::BoxBlur { radius: a(0, 4.0)? },
        "motion" => Filter::MotionBlur { angle: a(0, 0.0)?, distance: a(1, 20.0)? },
        "radial" | "spin" => Filter::RadialBlur { amount: a(0, 10.0)?, mode: RadialMode::Spin, cx: 0.5, cy: 0.5 },
        "zoom" => Filter::RadialBlur { amount: a(0, 10.0)?, mode: RadialMode::Zoom, cx: 0.5, cy: 0.5 },
        "surface" => Filter::SurfaceBlur { radius: a(0, 5.0)?, threshold: a(1, 15.0)? },
        "tiltshift" => Filter::TiltShift { focus_y: 0.5, focus_height: 0.2, transition: 0.2, blur: a(0, 12.0)? },
        "iris" => Filter::IrisBlur { cx: 0.5, cy: 0.5, rx: 0.25, ry: 0.25, transition: 0.2, blur: a(0, 15.0)? },
        "sharpen" => Filter::Sharpen,
        "sharpenmore" => Filter::SharpenMore,
        "unsharp" => Filter::UnsharpMask { amount: a(0, 80.0)?, radius: a(1, 1.5)?, threshold: a(2, 0.0)? as u8 },
        "noise" => Filter::AddNoise { amount: a(0, 10.0)?, dist: NoiseDist::Gaussian, monochrome: true, seed: 7 },
        "median" => Filter::Median { radius: a(0, 2.0)? },
        "denoise" => Filter::ReduceNoise { strength: a(0, 6.0)?, preserve_details: 40.0 },
        "dust" => Filter::DustScratches { radius: a(0, 2.0)?, threshold: a(1, 12.0)? as u8 },
        "twirl" => Filter::Twirl { angle: a(0, 90.0)? },
        "wave" => Filter::Wave { amplitude: a(0, 8.0)?, wavelength: a(1, 60.0)?, horizontal: true, vertical: false },
        "ripple" => Filter::Ripple { amount: a(0, 100.0)?, size: 12.0 },
        "pinch" => Filter::Pinch { amount: a(0, 50.0)? },
        "spherize" => Filter::Spherize { amount: a(0, 60.0)? },
        "polar" => Filter::PolarCoordinates { mode: Default::default() },
        "pixelate" | "mosaic" => Filter::Mosaic { cell: a(0, 12.0)? },
        "crystallize" => Filter::Crystallize { cell: a(0, 16.0)?, seed: 3 },
        "halftone" => Filter::ColorHalftone { radius: a(0, 6.0)? },
        "emboss" => Filter::Emboss { angle: 135.0, height: 3.0, amount: a(0, 100.0)? },
        "edges" => Filter::FindEdges,
        "glow" => Filter::GlowingEdges { width: a(0, 2.0)?, brightness: 6.0 },
        "oil" => Filter::OilPaint { radius: a(0, 4.0)?, levels: 20 },
        "solarize" => Filter::Solarize,
        "vignette" => Filter::Vignette { amount: a(0, -60.0)?, midpoint: 50.0, feather: 50.0 },
        "clouds" => Filter::Clouds { seed: 11, scale: a(0, 120.0)? },
        "flare" => Filter::LensFlare { cx: 0.3, cy: 0.3, brightness: a(0, 100.0)? },
        _ => return Err(format!("unknown filter `{name}` (see `photo-studio-cli list`)")),
    })
}

fn parse_resize(spec: &str, w: u32, h: u32) -> Result<(u32, u32), String> {
    let bad = || format!("bad --resize value `{spec}` (use 1600, 50% or 1600x900)");
    if let Some(p) = spec.strip_suffix('%') {
        let p: f32 = p.parse().map_err(|_| bad())?;
        return Ok((((w as f32 * p / 100.0).round() as u32).max(1), ((h as f32 * p / 100.0).round() as u32).max(1)));
    }
    if let Some((a, b)) = spec.split_once(['x', 'X']) {
        return Ok((a.parse().map_err(|_| bad())?, b.parse().map_err(|_| bad())?));
    }
    let nw: u32 = spec.parse().map_err(|_| bad())?;
    Ok((nw.max(1), ((h as f64 * nw as f64 / w as f64).round() as u32).max(1)))
}

fn run_ops(doc: &mut Document, ops: &[Op]) -> Result<(), String> {
    for op in ops {
        // Most operations work on the active (or only) layer; flatten first for layered files.
        match op {
            Op::Resize(s) => {
                let (w, h) = parse_resize(s, doc.width, doc.height)?;
                doc.resize_image(w, h, Resample::Lanczos);
            }
            Op::Rotate(d) => match *d as i32 {
                90 if d.fract() == 0.0 => doc.rotate(Rotation::Cw90),
                180 if d.fract() == 0.0 => doc.rotate(Rotation::Half),
                270 | -90 if d.fract() == 0.0 => doc.rotate(Rotation::Ccw90),
                _ => doc.rotate_arbitrary(*d),
            },
            Op::Flip(h) => doc.flip(*h),
            Op::Crop(r) => doc.crop(*r),
            Op::Trim => doc.trim(),
            Op::Flatten => doc.flatten_image(),
            Op::Adjust(s) => {
                ensure_raster(doc);
                apply_adjust(doc, s)?;
            }
            Op::Filter(s) => {
                ensure_raster(doc);
                let f = parse_filter(s)?;
                let i = doc.active;
                f.apply(&mut doc.layers[i].pixels, None, &FilterCtx::default());
            }
        }
    }
    Ok(())
}

/// Pixel operations need one raster layer: merge layered documents first.
fn ensure_raster(doc: &mut Document) {
    let single_raster = doc.layers.len() == 1 && matches!(doc.layers[0].kind, studio_core::LayerKind::Raster);
    if !single_raster {
        doc.merge_visible();
        if doc.layers.len() > 1 || !matches!(doc.layers[doc.active].kind, studio_core::LayerKind::Raster) {
            doc.flatten_image();
        }
    }
}

fn save(doc: &Document, out: &Path, quality: u8) -> Result<(), String> {
    let ext = out.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    let r = if ext == io::PROJECT_EXTENSION {
        io::save_project(doc, out)
    } else {
        io::export(doc, out, &io::ExportOptions { jpeg_quality: quality, ..Default::default() })
    };
    r.map_err(|e| format!("{}: {e}", out.display()))
}

fn info(path: &Path) -> Result<(), String> {
    let doc = io::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let meta = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    println!("{}", path.display());
    println!("  size     {} × {} px ({:.1} MP)", doc.width, doc.height, doc.width as f64 * doc.height as f64 / 1e6);
    println!("  file     {:.1} KB", meta as f64 / 1024.0);
    println!("  layers   {}", doc.layers.len());
    for l in doc.layers.iter().rev() {
        println!(
            "    {} {:<28} {:<10} {:>3.0}% {}",
            if l.visible { "●" } else { "○" },
            l.name,
            l.kind.badge(),
            l.opacity * 100.0,
            l.blend.label()
        );
    }
    let comp = doc.composite();
    let h = studio_core::histogram::Histogram::of(&comp, None);
    println!("  mean     R {:.0}  G {:.0}  B {:.0}  L {:.0}", h.mean(0), h.mean(1), h.mean(2), h.mean(3));
    Ok(())
}

fn collect_inputs(args: &[String]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for a in args {
        let p = PathBuf::from(a);
        if p.is_dir() {
            if let Ok(rd) = std::fs::read_dir(&p) {
                let mut files: Vec<PathBuf> = rd
                    .flatten()
                    .map(|e| e.path())
                    .filter(|f| {
                        f.extension()
                            .and_then(|e| e.to_str())
                            .is_some_and(|e| io::OPEN_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
                    })
                    .collect();
                files.sort();
                out.extend(files);
            }
        } else {
            out.push(p);
        }
    }
    out
}

struct Parsed {
    positional: Vec<String>,
    ops: Vec<Op>,
    output: Option<PathBuf>,
    out_dir: Option<PathBuf>,
    format: Option<String>,
    quality: u8,
}

fn parse(args: &[String]) -> Result<Parsed, String> {
    let mut p =
        Parsed { positional: Vec::new(), ops: Vec::new(), output: None, out_dir: None, format: None, quality: 90 };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut val = |name: &str| it.next().cloned().ok_or_else(|| format!("{name} needs a value"));
        match a.as_str() {
            "-o" | "--output" => p.output = Some(PathBuf::from(val(a)?)),
            "--out-dir" => p.out_dir = Some(PathBuf::from(val(a)?)),
            "--format" => p.format = Some(val(a)?.trim_start_matches('.').to_ascii_lowercase()),
            "--quality" => p.quality = val(a)?.parse::<u8>().map_err(|_| "--quality needs 1-100")?.clamp(1, 100),
            "--resize" => p.ops.push(Op::Resize(val(a)?)),
            "--rotate" => p.ops.push(Op::Rotate(val(a)?.parse().map_err(|_| "--rotate needs degrees")?)),
            "--flip" => {
                let v = val(a)?;
                p.ops.push(Op::Flip(match v.as_str() {
                    "h" | "horizontal" => true,
                    "v" | "vertical" => false,
                    _ => return Err("--flip takes h or v".into()),
                }))
            }
            "--crop" => {
                let v = nums(&val(a)?)?;
                if v.len() != 4 {
                    return Err("--crop takes x,y,w,h".into());
                }
                p.ops.push(Op::Crop(Rect::new(v[0] as i32, v[1] as i32, v[2] as u32, v[3] as u32)));
            }
            "--trim" => p.ops.push(Op::Trim),
            "--flatten" => p.ops.push(Op::Flatten),
            "--adjust" => p.ops.push(Op::Adjust(val(a)?)),
            "--filter" => {
                let v = val(a)?;
                parse_filter(&v)?;
                p.ops.push(Op::Filter(v));
            }
            s if s.starts_with('-') && s.len() > 1 => return Err(format!("unknown option `{s}` (try --help)")),
            _ => p.positional.push(a.clone()),
        }
    }
    Ok(p)
}

fn real_main(args: Vec<String>) -> Result<(), String> {
    if args.is_empty() || args.iter().any(|a| a == "-h" || a == "--help") {
        print!("{HELP}");
        return Ok(());
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("photo-studio-cli {}", studio_core::VERSION);
        return Ok(());
    }
    match args[0].as_str() {
        "list" => {
            print!("{LIST}");
            Ok(())
        }
        "info" => {
            for f in collect_inputs(&args[1..]) {
                info(&f)?;
            }
            Ok(())
        }
        "batch" => {
            let p = parse(&args[1..])?;
            let dir = p.out_dir.ok_or("batch needs --out-dir")?;
            std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            let inputs = collect_inputs(&p.positional);
            if inputs.is_empty() {
                return Err("no input images found".into());
            }
            let mut failed = 0;
            for f in &inputs {
                let stem = f.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "image".into());
                let ext = p
                    .format
                    .clone()
                    .or_else(|| f.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()))
                    .unwrap_or_else(|| "png".into());
                let out = dir.join(format!("{stem}.{ext}"));
                let result = io::open(f)
                    .map_err(|e| e.to_string())
                    .and_then(|mut d| run_ops(&mut d, &p.ops).map(|_| d))
                    .and_then(|d| save(&d, &out, p.quality));
                match result {
                    Ok(()) => println!("✓ {} → {}", f.display(), out.display()),
                    Err(e) => {
                        failed += 1;
                        eprintln!("✗ {}: {e}", f.display());
                    }
                }
            }
            println!("{} of {} done", inputs.len() - failed, inputs.len());
            if failed > 0 { Err(format!("{failed} file(s) failed")) } else { Ok(()) }
        }
        _ => {
            let p = parse(&args)?;
            let [input] = p.positional.as_slice() else {
                return Err("give exactly one input file (or use `batch`)".into());
            };
            let out = p.output.ok_or("missing -o <output>")?;
            let mut doc = io::open(Path::new(input)).map_err(|e| format!("{input}: {e}"))?;
            run_ops(&mut doc, &p.ops)?;
            save(&doc, &out, p.quality)?;
            println!("{} × {} → {}", doc.width, doc.height, out.display());
            Ok(())
        }
    }
}

fn main() -> ExitCode {
    match real_main(std::env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }

    #[test]
    fn parses_operations_in_order() {
        let p =
            parse(&args("in.png --resize 50% --adjust brightness=10 --filter blur=2 -o out.jpg --quality 80")).unwrap();
        assert_eq!(p.positional, vec!["in.png"]);
        assert_eq!(p.ops.len(), 3);
        assert!(matches!(&p.ops[0], Op::Resize(s) if s == "50%"));
        assert_eq!(p.output, Some(PathBuf::from("out.jpg")));
        assert_eq!(p.quality, 80);
    }

    #[test]
    fn rejects_unknown_things() {
        assert!(parse(&args("in.png --filter nope")).is_err());
        assert!(parse(&args("in.png --bogus")).is_err());
    }

    #[test]
    fn resize_specs() {
        assert_eq!(parse_resize("50%", 200, 100).unwrap(), (100, 50));
        assert_eq!(parse_resize("100", 200, 100).unwrap(), (100, 50));
        assert_eq!(parse_resize("30x40", 200, 100).unwrap(), (30, 40));
        assert!(parse_resize("abc", 1, 1).is_err());
    }

    #[test]
    fn end_to_end_conversion() {
        let dir = std::env::temp_dir().join(format!("psc-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let input = dir.join("in.png");
        let img = studio_core::Image::filled(40, 20, Rgba::rgb(200, 100, 50));
        std::fs::write(&input, io::encode(&img, "png", &Default::default()).unwrap()).unwrap();
        let out = dir.join("out.jpg");
        real_main(vec![
            input.display().to_string(),
            "--resize".into(),
            "20".into(),
            "--adjust".into(),
            "invert".into(),
            "--filter".into(),
            "blur=1".into(),
            "-o".into(),
            out.display().to_string(),
        ])
        .unwrap();
        let back = io::read_image(&out).unwrap();
        assert_eq!(back.size(), (20, 10));
        assert!(back.get(10, 5).b() > 150, "inverted blue channel");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn every_listed_filter_parses() {
        for name in ["blur=2", "motion=10,5", "unsharp=80,1.5", "pixelate=8", "vignette=-40", "polar", "edges"] {
            parse_filter(name).unwrap_or_else(|e| panic!("{name}: {e}"));
        }
    }
}
