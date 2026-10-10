//! Colour gradients, shared by the Gradient tool, gradient fill layers and Gradient Map.

use serde::{Deserialize, Serialize};

use crate::color::Rgba;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct GradientStop {
    /// 0..1 along the gradient.
    pub pos: f32,
    pub color: Rgba,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Gradient {
    pub stops: Vec<GradientStop>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum GradientShape {
    #[default]
    Linear,
    Radial,
    Angle,
    Reflected,
    Diamond,
}

impl GradientShape {
    pub const ALL: [GradientShape; 5] = [Self::Linear, Self::Radial, Self::Angle, Self::Reflected, Self::Diamond];

    pub fn label(&self) -> &'static str {
        match self {
            Self::Linear => "Linear",
            Self::Radial => "Radial",
            Self::Angle => "Angle",
            Self::Reflected => "Reflected",
            Self::Diamond => "Diamond",
        }
    }

    /// Position along the gradient (0..1) of point `p` for a drag from `a` to `b`.
    pub fn t_at(&self, a: (f32, f32), b: (f32, f32), p: (f32, f32)) -> f32 {
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let len2 = (dx * dx + dy * dy).max(1e-6);
        let len = len2.sqrt();
        let (px, py) = (p.0 - a.0, p.1 - a.1);
        match self {
            Self::Linear => ((px * dx + py * dy) / len2).clamp(0.0, 1.0),
            Self::Reflected => ((px * dx + py * dy) / len2).abs().clamp(0.0, 1.0),
            Self::Radial => ((px * px + py * py).sqrt() / len).clamp(0.0, 1.0),
            Self::Diamond => {
                // Distance measured along the drag axis and its perpendicular (L∞ norm).
                let u = (px * dx + py * dy) / len;
                let v = (-px * dy + py * dx) / len;
                (u.abs().max(v.abs()) / len).clamp(0.0, 1.0)
            }
            Self::Angle => {
                let base = dy.atan2(dx);
                let ang = py.atan2(px) - base;
                (ang.rem_euclid(std::f32::consts::TAU)) / std::f32::consts::TAU
            }
        }
    }
}

impl Gradient {
    pub fn two(a: Rgba, b: Rgba) -> Self {
        Gradient { stops: vec![GradientStop { pos: 0.0, color: a }, GradientStop { pos: 1.0, color: b }] }
    }

    pub fn reversed(&self) -> Self {
        let mut stops: Vec<_> = self.stops.iter().map(|s| GradientStop { pos: 1.0 - s.pos, color: s.color }).collect();
        stops.reverse();
        Gradient { stops }
    }

    /// Ready-made gradients offered in the UI.
    pub fn presets(fg: Rgba, bg: Rgba) -> Vec<(&'static str, Gradient)> {
        let mut fade = fg;
        fade.0[3] = 0;
        let stops =
            |v: &[(f32, Rgba)]| Gradient { stops: v.iter().map(|&(pos, color)| GradientStop { pos, color }).collect() };
        vec![
            ("Foreground to Background", Gradient::two(fg, bg)),
            ("Foreground to Transparent", Gradient::two(fg, fade)),
            ("Black, White", Gradient::two(Rgba::BLACK, Rgba::WHITE)),
            (
                "Spectrum",
                stops(&[
                    (0.0, Rgba::rgb(255, 0, 0)),
                    (0.17, Rgba::rgb(255, 255, 0)),
                    (0.33, Rgba::rgb(0, 255, 0)),
                    (0.5, Rgba::rgb(0, 255, 255)),
                    (0.67, Rgba::rgb(0, 0, 255)),
                    (0.83, Rgba::rgb(255, 0, 255)),
                    (1.0, Rgba::rgb(255, 0, 0)),
                ]),
            ),
            (
                "Sunset",
                stops(&[(0.0, Rgba::rgb(40, 20, 90)), (0.5, Rgba::rgb(230, 70, 90)), (1.0, Rgba::rgb(255, 200, 90))]),
            ),
            (
                "Ocean",
                stops(&[(0.0, Rgba::rgb(5, 30, 70)), (0.6, Rgba::rgb(0, 140, 200)), (1.0, Rgba::rgb(170, 240, 255))]),
            ),
            (
                "Copper",
                stops(&[(0.0, Rgba::rgb(60, 25, 10)), (0.5, Rgba::rgb(200, 110, 60)), (1.0, Rgba::rgb(255, 220, 180))]),
            ),
        ]
    }

    pub fn sample(&self, t: f32) -> Rgba {
        let t = t.clamp(0.0, 1.0);
        match self.stops.len() {
            0 => return Rgba::TRANSPARENT,
            1 => return self.stops[0].color,
            _ => {}
        }
        let first = &self.stops[0];
        if t <= first.pos {
            return first.color;
        }
        for w in self.stops.windows(2) {
            if t <= w[1].pos {
                let span = (w[1].pos - w[0].pos).max(1e-6);
                return Rgba::lerp(w[0].color, w[1].color, (t - w[0].pos) / span);
            }
        }
        self.stops[self.stops.len() - 1].color
    }

    /// 256-entry lookup table, used by Gradient Map.
    pub fn lut(&self) -> Vec<Rgba> {
        (0..256).map(|i| self.sample(i as f32 / 255.0)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samples_between_stops() {
        let g = Gradient::two(Rgba::BLACK, Rgba::WHITE);
        assert_eq!(g.sample(0.0), Rgba::BLACK);
        assert_eq!(g.sample(1.0), Rgba::WHITE);
        assert_eq!(g.sample(0.5), Rgba::rgb(128, 128, 128));
    }

    #[test]
    fn shapes_map_endpoints() {
        let (a, b) = ((0.0, 0.0), (10.0, 0.0));
        assert_eq!(GradientShape::Linear.t_at(a, b, (10.0, 5.0)), 1.0);
        assert_eq!(GradientShape::Reflected.t_at(a, b, (-5.0, 0.0)), 0.5);
        assert_eq!(GradientShape::Radial.t_at(a, b, (0.0, 5.0)), 0.5);
    }
}
