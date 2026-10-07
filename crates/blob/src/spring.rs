//! A damped spring: the value chases its target and, when the damping is
//! below 1, overshoots a little before it settles. All of Coco's motion is
//! made of these, which is why nothing starts or stops abruptly.

use std::f32::consts::TAU;

#[derive(Clone, Copy, Debug)]
pub struct Spring {
    pub x: f32,
    pub v: f32,
    pub target: f32,
    stiffness: f32,
    friction: f32,
}

impl Spring {
    /// `hz` is how fast it oscillates; `damping` 1.0 means no overshoot,
    /// lower values bounce more.
    pub fn new(x: f32, hz: f32, damping: f32) -> Self {
        let w = TAU * hz;
        Self { x, v: 0.0, target: x, stiffness: w * w, friction: 2.0 * damping * w }
    }

    pub fn step(&mut self, dt: f32) {
        self.v += (self.stiffness * (self.target - self.x) - self.friction * self.v) * dt;
        self.x += self.v * dt;
    }

    pub fn settled(&self) -> bool {
        (self.target - self.x).abs() < 0.002 && self.v.abs() < 0.02
    }

    pub fn snap(&mut self) {
        self.x = self.target;
        self.v = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bouncy_spring_overshoots_then_settles_on_its_target() {
        let mut s = Spring::new(0.0, 3.0, 0.4);
        s.target = 1.0;
        let mut peak = 0.0f32;
        for _ in 0..(240 * 3) {
            s.step(1.0 / 240.0);
            peak = peak.max(s.x);
        }
        assert!(peak > 1.05, "no overshoot: {peak}");
        assert!(s.settled(), "still moving: {s:?}");
    }
}
