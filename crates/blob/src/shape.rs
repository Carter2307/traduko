//! Body shapes. Each shape is stored as its outline radius at `N` evenly
//! spaced angles around the centre, so any two shapes can be blended point
//! by point: that is what makes the morph between them smooth.

use std::f32::consts::TAU;
use std::sync::OnceLock;

/// Number of points on the body outline.
pub const N: usize = 128;

/// Outline radius at each of the `N` angles. Angle 0 points right and the
/// angle grows clockwise on screen (y goes down).
pub type Radii = [f32; N];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    /// Speech bubble with a small tail: Traduko at rest.
    Bubble,
    Cloud,
    Capsule,
    Drop,
    Ball,
    Triangle,
    Clover,
}

impl Shape {
    pub const ALL: [Shape; 7] = [
        Shape::Bubble,
        Shape::Cloud,
        Shape::Capsule,
        Shape::Drop,
        Shape::Ball,
        Shape::Triangle,
        Shape::Clover,
    ];

    pub fn radii(self) -> &'static Radii {
        static CACHE: OnceLock<[Radii; 7]> = OnceLock::new();
        &CACHE.get_or_init(|| Shape::ALL.map(build))[self as usize]
    }
}

pub fn angle(i: usize) -> f32 {
    i as f32 / N as f32 * TAU
}

fn circle(x: f32, y: f32, cx: f32, cy: f32, r: f32) -> bool {
    (x - cx) * (x - cx) + (y - cy) * (y - cy) <= r * r
}

fn superellipse(x: f32, y: f32, cy: f32, a: f32, b: f32, n: f32) -> bool {
    (x / a).abs().powf(n) + ((y - cy) / b).abs().powf(n) <= 1.0
}

/// A circle that slides from `a` to `b` while its radius goes from `ra` to `rb`.
fn taper(x: f32, y: f32, a: (f32, f32), ra: f32, b: (f32, f32), rb: f32) -> bool {
    const STEPS: usize = 24;
    (0..=STEPS).any(|i| {
        let s = i as f32 / STEPS as f32;
        circle(x, y, a.0 + (b.0 - a.0) * s, a.1 + (b.1 - a.1) * s, ra + (rb - ra) * s)
    })
}

fn circles(x: f32, y: f32, list: &[(f32, f32, f32)]) -> bool {
    list.iter().any(|&(cx, cy, r)| circle(x, y, cx, cy, r))
}

fn inside(shape: Shape, x: f32, y: f32) -> bool {
    match shape {
        Shape::Bubble => {
            superellipse(x, y, -0.10, 1.0, 0.80, 2.9)
                || taper(x, y, (-0.40, 0.38), 0.32, (-0.74, 0.96), 0.09)
        }
        Shape::Cloud => circles(
            x,
            y,
            &[
                (0.0, 0.08, 0.66),
                (-0.52, 0.14, 0.46),
                (0.54, 0.20, 0.44),
                (-0.20, -0.36, 0.48),
                (0.34, -0.30, 0.46),
                (-0.22, 0.44, 0.42),
                (0.28, 0.48, 0.40),
            ],
        ),
        Shape::Capsule => superellipse(x, y, 0.0, 1.0, 0.70, 4.2),
        Shape::Drop => taper(x, y, (-0.02, 0.16), 0.80, (0.20, -0.86), 0.12),
        Shape::Ball => circle(x, y, 0.0, 0.0, 0.90),
        Shape::Triangle => {
            let (a, b, c) = ((0.0, -0.64), (0.70, 0.52), (-0.70, 0.52));
            taper(x, y, a, 0.28, b, 0.28) || taper(x, y, b, 0.28, c, 0.28) || taper(x, y, c, 0.28, a, 0.28)
        }
        Shape::Clover => circles(
            x,
            y,
            &[(-0.30, -0.44, 0.50), (0.44, -0.26, 0.50), (0.28, 0.46, 0.50), (-0.46, 0.28, 0.50), (0.0, 0.0, 0.5)],
        ),
    }
}

/// Finds the outline by walking each ray from far away towards the centre,
/// then smooths the result so that creases and tips come out rounded.
fn build(shape: Shape) -> Radii {
    const FAR: f32 = 1.5;
    const STEP: f32 = 0.01;
    let mut raw = [0.0f32; N];
    for (i, r) in raw.iter_mut().enumerate() {
        let (sin, cos) = angle(i).sin_cos();
        let mut t = FAR;
        while t > 0.0 && !inside(shape, cos * t, sin * t) {
            t -= STEP;
        }
        let (mut lo, mut hi) = (t.max(0.0), (t + STEP).min(FAR));
        for _ in 0..10 {
            let mid = (lo + hi) * 0.5;
            if inside(shape, cos * mid, sin * mid) { lo = mid } else { hi = mid }
        }
        *r = lo;
    }
    const KERNEL: [f32; 7] = [0.03, 0.11, 0.22, 0.28, 0.22, 0.11, 0.03];
    let mut out = [0.0f32; N];
    for (i, r) in out.iter_mut().enumerate() {
        *r = KERNEL.iter().enumerate().map(|(k, w)| w * raw[(i + N + k - 3) % N]).sum();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_shape_is_a_closed_outline_that_fits_the_canvas() {
        for shape in Shape::ALL {
            let radii = shape.radii();
            let (min, max) = radii.iter().fold((f32::MAX, 0.0f32), |(lo, hi), r| (lo.min(*r), hi.max(*r)));
            assert!(min > 0.35, "{shape:?} is too thin somewhere: {min}");
            assert!(max < 1.25, "{shape:?} is too large: {max}");
        }
    }

    #[test]
    fn the_outline_has_no_jumps() {
        for shape in Shape::ALL {
            let radii = shape.radii();
            for i in 0..N {
                let step = (radii[i] - radii[(i + 1) % N]).abs();
                assert!(step < 0.12, "{shape:?} jumps by {step} at point {i}");
            }
        }
    }
}
