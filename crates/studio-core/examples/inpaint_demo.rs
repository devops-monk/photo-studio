//! `cargo run --release -p studio-core --example inpaint_demo -- in.jpg x y w h out.png`
//! Fills the rectangle with content-aware fill and writes a before/after strip.
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let img = studio_core::io::read_image(std::path::Path::new(&a[0])).unwrap();
    let n = |i: usize| a[i].parse::<i32>().unwrap();
    let r = studio_core::Rect::new(n(1), n(2), n(3) as u32, n(4) as u32);
    let hole = studio_core::select::ellipse_mask(
        img.width(),
        img.height(),
        (r.x as f32, r.y as f32),
        (r.right() as f32, r.bottom() as f32),
    );
    let t = std::time::Instant::now();
    let out = studio_core::inpaint::content_aware_fill(&img, &hole);
    eprintln!("filled in {:?}", t.elapsed());
    let ctx = r.inflate(r.w as i32);
    let mut strip = studio_core::Image::new(ctx.w * 2 + 8, ctx.h);
    strip.blit(&img.crop(ctx), 0, 0);
    strip.blit(&out.crop(ctx), ctx.w as i32 + 8, 0);
    std::fs::write(&a[5], studio_core::io::encode(&strip, "png", &Default::default()).unwrap()).unwrap();
}
