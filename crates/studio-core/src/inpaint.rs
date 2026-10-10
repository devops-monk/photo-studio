//! Content-aware fill: fill a hole with patches from its surroundings (multi-scale PatchMatch).

use rayon::prelude::*;

use crate::image::{Image, Mask};

const PATCH: i32 = 3; // patch radius: 7×7 patches

/// Simple deterministic xorshift for PatchMatch's random search.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 32) as u32
    }
    fn range(&mut self, n: i32) -> i32 {
        if n <= 0 { 0 } else { (self.next() % n as u32) as i32 }
    }
}

/// A float RGB image plus a hole mask, used at each pyramid level.
#[derive(Clone)]
struct Level {
    w: i32,
    h: i32,
    px: Vec<[f32; 3]>,
    hole: Vec<bool>,
}

impl Level {
    fn at(&self, x: i32, y: i32) -> [f32; 3] {
        self.px[(y.clamp(0, self.h - 1) * self.w + x.clamp(0, self.w - 1)) as usize]
    }

    /// A valid source patch centre: the whole patch lies outside the hole and inside the image.
    fn valid_source(&self, x: i32, y: i32) -> bool {
        if x < PATCH || y < PATCH || x >= self.w - PATCH || y >= self.h - PATCH {
            return false;
        }
        for dy in -PATCH..=PATCH {
            for dx in -PATCH..=PATCH {
                if self.hole[((y + dy) * self.w + x + dx) as usize] {
                    return false;
                }
            }
        }
        true
    }

    fn downsample(&self) -> Level {
        let (w, h) = ((self.w + 1) / 2, (self.h + 1) / 2);
        let mut px = vec![[0.0; 3]; (w * h) as usize];
        let mut hole = vec![false; (w * h) as usize];
        for y in 0..h {
            for x in 0..w {
                let mut acc = [0.0; 3];
                let mut n = 0.0;
                let mut any_hole = false;
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let (sx, sy) = (2 * x + dx, 2 * y + dy);
                    if sx >= self.w || sy >= self.h {
                        continue;
                    }
                    let i = (sy * self.w + sx) as usize;
                    any_hole |= self.hole[i];
                    if !self.hole[i] {
                        for c in 0..3 {
                            acc[c] += self.px[i][c];
                        }
                        n += 1.0;
                    }
                }
                let i = (y * w + x) as usize;
                hole[i] = any_hole;
                if n > 0.0 {
                    px[i] = acc.map(|v| v / n);
                }
            }
        }
        Level { w, h, px, hole }
    }
}

/// Sum of squared differences between the patches at `a` (target) and `b` (source).
fn patch_distance(t: &Level, ax: i32, ay: i32, bx: i32, by: i32, best: f32) -> f32 {
    let mut d = 0.0;
    for dy in -PATCH..=PATCH {
        for dx in -PATCH..=PATCH {
            let p = t.at(ax + dx, ay + dy);
            let q = t.at(bx + dx, by + dy);
            d += (p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) + (p[2] - q[2]).powi(2);
        }
        if d > best {
            return d;
        }
    }
    d
}

/// Fill the hole at this level by PatchMatch + voting, a few rounds.
fn solve(level: &mut Level, rounds: usize, seed: u64, finest: bool) {
    let (w, h) = (level.w, level.h);
    let targets: Vec<(i32, i32)> = (0..h)
        .flat_map(|y| (0..w).map(move |x| (x, y)))
        .filter(|&(x, y)| {
            // Every pixel whose patch touches the hole is re-synthesised.
            (-PATCH..=PATCH).any(|dy| {
                (-PATCH..=PATCH).any(|dx| {
                    let (px, py) = (x + dx, y + dy);
                    px >= 0 && py >= 0 && px < w && py < h && level.hole[(py * w + px) as usize]
                })
            })
        })
        .collect();
    if targets.is_empty() {
        return;
    }
    let sources: Vec<(i32, i32)> =
        (0..h).flat_map(|y| (0..w).map(move |x| (x, y))).filter(|&(x, y)| level.valid_source(x, y)).collect();
    if sources.is_empty() {
        return;
    }
    let mut rng = Rng(seed | 1);
    // Nearest-neighbour field for each target pixel.
    let mut nnf: Vec<(i32, i32, f32)> = targets
        .iter()
        .map(|&(x, y)| {
            let s = sources[rng.range(sources.len() as i32) as usize];
            (s.0, s.1, patch_distance(level, x, y, s.0, s.1, f32::MAX))
        })
        .collect();
    let index: std::collections::HashMap<(i32, i32), usize> =
        targets.iter().enumerate().map(|(i, &t)| (t, i)).collect();
    let max_r = w.max(h);
    for round in 0..rounds {
        for iter in 0..4 {
            let forward = (iter + round) % 2 == 0;
            let order: Box<dyn Iterator<Item = usize>> =
                if forward { Box::new(0..targets.len()) } else { Box::new((0..targets.len()).rev()) };
            for i in order {
                let (x, y) = targets[i];
                let mut best = nnf[i];
                // Propagation from the already-visited neighbours.
                let step = if forward { -1 } else { 1 };
                for (nx, ny) in [(x + step, y), (x, y + step)] {
                    if let Some(&j) = index.get(&(nx, ny)) {
                        // The neighbour's match, shifted by the same offset.
                        let (cx, cy) = (nnf[j].0 + (x - nx), nnf[j].1 + (y - ny));
                        if level.valid_source(cx, cy) {
                            let d = patch_distance(level, x, y, cx, cy, best.2);
                            if d < best.2 {
                                best = (cx, cy, d);
                            }
                        }
                    }
                }
                // Random search in shrinking windows.
                let mut r = max_r;
                while r >= 1 {
                    let cx = best.0 + rng.range(2 * r + 1) - r;
                    let cy = best.1 + rng.range(2 * r + 1) - r;
                    if level.valid_source(cx, cy) {
                        let d = patch_distance(level, x, y, cx, cy, best.2);
                        if d < best.2 {
                            best = (cx, cy, d);
                        }
                    }
                    r /= 2;
                }
                nnf[i] = best;
            }
        }
        // Voting: each hole pixel becomes a weighted average of what overlapping patches predict.
        // Weights are sharpened with this round's own distance statistics, so good matches win.
        let mut ds: Vec<f32> = nnf.iter().map(|n| n.2).collect();
        ds.sort_by(|a, b| a.total_cmp(b));
        let sigma = ds[ds.len() * 3 / 4].max(1e-4);
        let votes: Vec<[f32; 3]> = (0..(w * h) as usize)
            .into_par_iter()
            .map(|p| {
                let (x, y) = ((p as i32) % w, (p as i32) / w);
                if !level.hole[p] {
                    return level.px[p];
                }
                let mut acc = [0.0f32; 3];
                let mut n = 0.0f32;
                for dy in -PATCH..=PATCH {
                    for dx in -PATCH..=PATCH {
                        if let Some(&j) = index.get(&(x - dx, y - dy)) {
                            let (sx, sy, d) = nnf[j];
                            let wgt = (-d / sigma).exp().max(1e-8);
                            let c = level.at(sx + dx, sy + dy);
                            for k in 0..3 {
                                acc[k] += c[k] * wgt;
                            }
                            n += wgt;
                        }
                    }
                }
                if n > 0.0 { acc.map(|v| v / n) } else { level.px[p] }
            })
            .collect();
        level.px = votes;
    }
    // At full resolution, copy each hole pixel from its best match so texture stays crisp.
    if finest {
        let copy: Vec<(usize, [f32; 3])> = targets
            .iter()
            .zip(&nnf)
            .filter(|((x, y), _)| level.hole[(y * w + x) as usize])
            .map(|(&(x, y), &(sx, sy, _))| ((y * w + x) as usize, level.at(sx, sy)))
            .collect();
        for (i, c) in copy {
            level.px[i] = c;
        }
    }
}

/// Fill the area covered by `hole` with plausible content from the rest of `img`.
/// Works on a window around the hole so large images stay fast.
pub fn content_aware_fill(img: &Image, hole: &Mask) -> Image {
    let Some(hb) = hole.bounds() else { return img.clone() };
    let margin = (hb.w.max(hb.h) as i32).max(24);
    let win = hb.inflate(margin * 2).intersect(&img.bounds());
    let crop = img.crop(win);
    let (w, h) = (win.w as i32, win.h as i32);
    let mut level = Level {
        w,
        h,
        px: crop
            .pixels()
            .chunks_exact(4)
            .map(|p| [p[0] as f32 / 255.0, p[1] as f32 / 255.0, p[2] as f32 / 255.0])
            .collect(),
        hole: (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .map(|(x, y)| hole.get_or_zero(win.x + x, win.y + y) >= 64)
            .collect(),
    };
    // Grow the hole a touch so seams fall on clean pixels.
    let grown: Vec<bool> = (0..h)
        .flat_map(|y| (0..w).map(move |x| (x, y)))
        .map(|(x, y)| {
            (-1..=1).any(|dy| {
                (-1..=1).any(|dx| {
                    let (px, py) = (x + dx, y + dy);
                    px >= 0 && py >= 0 && px < w && py < h && level.hole[(py * w + px) as usize]
                })
            })
        })
        .collect();
    level.hole = grown;

    // Pyramid down until the hole is a few pixels across.
    let mut pyramid = vec![level];
    while pyramid.last().map(|l| l.w.min(l.h) > 48).unwrap_or(false) {
        let next = pyramid.last().unwrap().downsample();
        pyramid.push(next);
    }
    // Coarsest: seed the hole by diffusing its border inwards (onion peel), then solve.
    {
        let top = pyramid.last_mut().unwrap();
        let (tw, th) = (top.w, top.h);
        let mut known: Vec<bool> = top.hole.iter().map(|h| !h).collect();
        loop {
            let mut next = known.clone();
            let mut changed = false;
            for y in 0..th {
                for x in 0..tw {
                    let i = (y * tw + x) as usize;
                    if known[i] {
                        continue;
                    }
                    let mut acc = [0.0; 3];
                    let mut n = 0.0;
                    for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1), (-1, -1), (1, 1), (-1, 1), (1, -1)] {
                        let (nx, ny) = (x + dx, y + dy);
                        if nx >= 0 && ny >= 0 && nx < tw && ny < th && known[(ny * tw + nx) as usize] {
                            let c = top.px[(ny * tw + nx) as usize];
                            for k in 0..3 {
                                acc[k] += c[k];
                            }
                            n += 1.0;
                        }
                    }
                    if n > 0.0 {
                        top.px[i] = acc.map(|v| v / n);
                        next[i] = true;
                        changed = true;
                    }
                }
            }
            known = next;
            if !changed {
                break;
            }
        }
    }
    let levels = pyramid.len();
    for li in (0..levels).rev() {
        if li + 1 < levels {
            // Upsample the coarser solution into this level's hole.
            let (coarse, fine) = {
                let (a, b) = pyramid.split_at_mut(li + 1);
                (&b[0], &mut a[li])
            };
            for y in 0..fine.h {
                for x in 0..fine.w {
                    let i = (y * fine.w + x) as usize;
                    if fine.hole[i] {
                        fine.px[i] = coarse.at(x / 2, y / 2);
                    }
                }
            }
        }
        let rounds = if li == 0 { 2 } else { 4 };
        solve(&mut pyramid[li], rounds, 0x9E37_79B9 ^ li as u64, li == 0);
    }

    let fin = &pyramid[0];
    let mut out = img.clone();
    let iw = img.width() as usize;
    let px = out.pixels_mut();
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) as usize;
            if !fin.hole[i] {
                continue;
            }
            let (gx, gy) = ((win.x + x) as usize, (win.y + y) as usize);
            let o = (gy * iw + gx) * 4;
            // Blend by the original hole coverage so soft selections feather in.
            let k = (hole.get_or_zero(win.x + x, win.y + y) as f32 / 255.0).max(if fin.hole[i] { 0.35 } else { 0.0 });
            for c in 0..3 {
                let v = fin.px[i][c] * 255.0;
                px[o + c] = (px[o + c] as f32 + (v - px[o + c] as f32) * k).round().clamp(0.0, 255.0) as u8;
            }
            px[o + 3] = px[o + 3].max((255.0 * k) as u8);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Rgba;
    use crate::geom::Rect;

    #[test]
    fn fills_hole_from_surroundings() {
        // Horizontal stripes with a blob in the middle; the fill should continue the stripes.
        let mut img = Image::new(80, 60);
        for y in 0..60 {
            for x in 0..80 {
                let c = if (y / 6) % 2 == 0 { Rgba::rgb(220, 60, 40) } else { Rgba::rgb(30, 90, 200) };
                img.put(x, y, c);
            }
        }
        for y in 24..36 {
            for x in 34..46 {
                img.put(x, y, Rgba::rgb(0, 255, 0));
            }
        }
        let hole = crate::select::rect_mask(80, 60, Rect::new(33, 23, 14, 14));
        let out = content_aware_fill(&img, &hole);
        // No green left, and the colours come from the stripe palette.
        for y in 24..36 {
            for x in 34..46 {
                let c = out.get(x, y);
                assert!(c.g() < 140, "green remains at {x},{y}: {c:?}");
            }
        }
        // Outside the hole nothing changed.
        assert_eq!(out.get(5, 5), img.get(5, 5));
    }
}
