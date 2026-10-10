//! Contact sheet of animation frames, for checking the logo motion.
#[allow(dead_code)]
#[path = "../src/logo.rs"]
mod logo;
fn main() {
    let n = 6;
    let s = 160;
    let mut sheet = image::RgbaImage::new(s * n, s);
    for i in 0..n {
        let f = image::RgbaImage::from_raw(s, s, logo::rgba(s, i as f32 / n as f32, true)).unwrap();
        image::imageops::overlay(&mut sheet, &f, (i * s) as i64, 0);
    }
    let out = std::env::args().nth(1).unwrap_or_else(|| "logo-strip.png".into());
    sheet.save(out).unwrap();
}
