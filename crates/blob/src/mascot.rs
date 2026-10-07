//! Coco's motion: what the body and the eyes do over time.
//!
//! The UI feeds it events (pointer, press, drag, mood) and calls `step` once
//! per frame; `frame` then gives the outlines to paint. Nothing here knows
//! about windows or the GPU, so the motion can be tested and drawn anywhere.

use crate::shape::{N, Radii, Shape, angle};
use crate::spring::Spring;
use std::f32::consts::TAU;

/// sRGB, each channel 0..1.
pub type Rgb = [f32; 3];

pub const ORANGE: Rgb = [0.957, 0.353, 0.110];
pub const GRAY: Rgb = [0.478, 0.478, 0.490];
pub const INK: Rgb = [0.086, 0.086, 0.094];

/// Where the body touches the "floor": squash and stretch pivot on this line.
const BASE_Y: f32 = 0.80;
const GRAVITY: f32 = 19.0;
const EYE_CAP_STEPS: usize = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mood {
    /// At rest: speech bubble, blinking and glancing around.
    Idle,
    /// Translating: cloud, eyes turned into dashes that scan left and right.
    Thinking,
    /// A translation just arrived: a hop, then back to `Idle` by itself.
    Happy,
    /// Something went wrong.
    Sorry,
    /// The model is still loading.
    Waking,
}

#[derive(Clone, Copy)]
struct Look {
    shape: Shape,
    color: Rgb,
    eye_w: f32,
    eye_h: f32,
    /// Degrees, clockwise; 0 is an upright pill.
    eye_tilt: f32,
    eye_gap: f32,
    eye_x: f32,
    eye_y: f32,
}

impl Mood {
    fn look(self) -> Look {
        match self {
            Mood::Idle => Look { shape: Shape::Bubble, color: ORANGE, eye_w: 0.17, eye_h: 0.46, eye_tilt: 7.0, eye_gap: 0.46, eye_x: 0.02, eye_y: -0.14 },
            Mood::Thinking => Look { shape: Shape::Cloud, color: ORANGE, eye_w: 0.15, eye_h: 0.34, eye_tilt: -68.0, eye_gap: 0.42, eye_x: 0.04, eye_y: -0.20 },
            Mood::Happy => Look { shape: Shape::Capsule, color: ORANGE, eye_w: 0.25, eye_h: 0.50, eye_tilt: 0.0, eye_gap: 0.58, eye_x: 0.0, eye_y: -0.10 },
            Mood::Sorry => Look { shape: Shape::Drop, color: GRAY, eye_w: 0.15, eye_h: 0.40, eye_tilt: 9.0, eye_gap: 0.40, eye_x: -0.04, eye_y: 0.08 },
            Mood::Waking => Look { shape: Shape::Ball, color: INK, eye_w: 0.12, eye_h: 0.32, eye_tilt: 24.0, eye_gap: 0.40, eye_x: -0.18, eye_y: -0.30 },
        }
    }
}

/// What to paint: closed outlines in Coco's own units, where the body is
/// about 2 wide, x grows to the right and y grows downwards.
#[derive(Clone, Debug, Default)]
pub struct Frame {
    pub body: Vec<[f32; 2]>,
    pub color: Rgb,
    pub eyes: [Vec<[f32; 2]>; 2],
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Cue {
    OpenEyes,
    Blink,
    Jump,
    EndGlance,
}

pub struct Mascot {
    /// Seconds since Coco started. Coco runs for days: a 32-bit float would
    /// stop counting single frames after about three of them.
    clock: f64,
    seed: u32,
    mood: Mood,

    from: Radii,
    to: Radii,
    morph: Spring,
    color_from: Rgb,
    color_to: Rgb,
    color_t: f32,

    /// Vertical scale; the width follows so the body keeps its volume.
    stretch: Spring,
    /// Shear: positive pushes the top to the right.
    lean: Spring,
    hop_y: f32,
    hop_v: f32,
    airborne: bool,
    bounces: u8,
    /// How much the outline ripples, 0..1; decays by itself.
    jelly: f32,
    /// Overall size, 0 to 1: Coco grows out of nothing when it appears.
    pop: Spring,

    eye_w: Spring,
    eye_h: Spring,
    eye_tilt: Spring,
    eye_gap: Spring,
    eye_x: Spring,
    eye_y: Spring,
    /// 1 = open, 0 = closed.
    open: Spring,
    gaze_x: Spring,
    gaze_y: Spring,

    pointer: Option<[f32; 2]>,
    attention: Option<[f32; 2]>,
    glance: Option<[f32; 2]>,
    pressed: bool,
    drag: Option<[f32; 2]>,
    /// Keeps a slow breath going at rest. Off means Coco is perfectly still
    /// between two idle gestures, so the window needs no redraw.
    pub breathe: bool,

    cues: Vec<(f64, Cue)>,
    next_blink: f64,
    next_glance: f64,
    revert_at: Option<f64>,
    last_hop: f64,
    /// How much of the breath and of the thinking bob shows, 0 to 1: both
    /// fade in and out instead of switching.
    breath: f32,
    think: f32,
}

impl Default for Mascot {
    fn default() -> Self {
        Self::new(Mood::Idle)
    }
}

impl Mascot {
    pub fn new(mood: Mood) -> Self {
        let look = mood.look();
        let radii = *look.shape.radii();
        Self {
            clock: 0.0,
            seed: 0x2545_F491,
            mood,
            from: radii,
            to: radii,
            morph: Spring::new(1.0, 2.6, 0.52),
            color_from: look.color,
            color_to: look.color,
            color_t: 1.0,
            stretch: Spring::new(1.0, 3.4, 0.32),
            lean: Spring::new(0.0, 2.8, 0.45),
            hop_y: 0.0,
            hop_v: 0.0,
            airborne: false,
            bounces: 0,
            jelly: 0.0,
            pop: Spring::new(1.0, 2.8, 0.5),
            eye_w: Spring::new(look.eye_w, 3.2, 0.6),
            eye_h: Spring::new(look.eye_h, 3.2, 0.6),
            eye_tilt: Spring::new(look.eye_tilt, 2.6, 0.55),
            eye_gap: Spring::new(look.eye_gap, 3.0, 0.6),
            eye_x: Spring::new(look.eye_x, 3.0, 0.7),
            eye_y: Spring::new(look.eye_y, 3.0, 0.7),
            open: Spring::new(1.0, 9.0, 0.85),
            gaze_x: Spring::new(0.0, 5.0, 0.85),
            gaze_y: Spring::new(0.0, 5.0, 0.85),
            pointer: None,
            attention: None,
            glance: None,
            pressed: false,
            drag: None,
            breathe: false,
            cues: Vec::new(),
            next_blink: 1.6,
            next_glance: 4.0,
            revert_at: None,
            last_hop: f64::NEG_INFINITY,
            breath: 0.0,
            think: if mood == Mood::Thinking { 1.0 } else { 0.0 },
        }
    }

    pub fn mood(&self) -> Mood {
        self.mood
    }

    /// Changes shape, colour and eyes. The body morphs from wherever it is
    /// now, so a change in the middle of another one stays smooth.
    pub fn set_mood(&mut self, mood: Mood) {
        if mood == self.mood {
            return;
        }
        let look = mood.look();
        self.from = self.radii();
        self.to = *look.shape.radii();
        self.morph.x = 0.0;
        self.morph.target = 1.0;
        self.color_from = self.color();
        self.color_to = look.color;
        self.color_t = 0.0;
        self.eye_w.target = look.eye_w;
        self.eye_h.target = look.eye_h;
        self.eye_tilt.target = look.eye_tilt;
        self.eye_gap.target = look.eye_gap;
        self.eye_x.target = look.eye_x;
        self.eye_y.target = look.eye_y;
        self.jelly = self.jelly.max(0.55);
        self.mood = mood;
        self.revert_at = None;
        self.cues.retain(|(_, cue)| *cue != Cue::Jump);
        if mood == Mood::Happy {
            // A result for every pause in typing must not mean a hop for
            // every pause: one, then a quiet spell.
            if !self.airborne && self.clock - self.last_hop >= 4.0 {
                self.last_hop = self.clock;
                // Anticipation: crouch first, then jump.
                self.stretch.target = 0.84;
                self.cue(0.13, Cue::Jump);
            }
            self.revert_at = Some(self.clock + 1.5);
        }
    }

    /// Pointer position in Coco's units (0,0 is the body centre), or `None`
    /// when it is away. The eyes follow it, and Coco perks up when the
    /// pointer is on its body.
    pub fn set_pointer(&mut self, pointer: Option<[f32; 2]>) {
        self.pointer = pointer;
    }

    /// A direction to keep looking at when nothing else asks for attention,
    /// for example towards the open translator panel.
    pub fn set_attention(&mut self, direction: Option<[f32; 2]>) {
        self.attention = direction;
    }

    pub fn press(&mut self) {
        self.pressed = true;
    }

    pub fn release(&mut self) {
        if self.pressed {
            self.pressed = false;
            self.jelly = self.jelly.max(0.5);
        }
    }

    /// Call while the window is being dragged, with its speed in units per
    /// second: the body trails behind and the eyes look where it goes.
    pub fn drag(&mut self, velocity: [f32; 2]) {
        self.drag = Some(velocity);
    }

    pub fn drop_it(&mut self) {
        self.pressed = false;
        if self.drag.take().is_some() {
            self.jelly = self.jelly.max(0.8);
        }
    }

    /// Plays the entrance: grows from nothing with a bounce.
    pub fn enter(&mut self) {
        self.pop.x = 0.0;
        self.pop.v = 0.0;
        self.jelly = self.jelly.max(0.6);
    }

    /// A little wobble with a blink, to acknowledge something.
    pub fn nudge(&mut self) {
        self.jelly = self.jelly.max(0.7);
        self.stretch.v += 1.6;
        self.cue(0.0, Cue::Blink);
    }

    /// True when nothing moves and nothing will until the next idle gesture:
    /// the UI can stop drawing and sleep for `next_wake` seconds.
    pub fn is_resting(&self) -> bool {
        !self.breathe
            && matches!(self.mood, Mood::Idle | Mood::Sorry)
            && !self.pressed
            && self.drag.is_none()
            && self.cues.iter().all(|(at, _)| *at > self.clock)
            && self.breath < 0.01
            && self.think < 0.01
            && !self.airborne
            && self.jelly < 0.004
            && self.color_t >= 1.0
            && self.springs().iter().all(|s| s.settled())
    }

    /// Seconds until the next blink or glance.
    pub fn next_wake(&self) -> f32 {
        let mut at = self.next_blink.min(self.next_glance);
        if let Some(revert) = self.revert_at {
            at = at.min(revert);
        }
        for (cue_at, _) in &self.cues {
            at = at.min(*cue_at);
        }
        (at - self.clock).max(0.0) as f32
    }

    /// Advances time by `dt` seconds. A long `dt` (after a sleep) only moves
    /// the clock; the springs never take a step larger than a frame.
    pub fn step(&mut self, dt: f32) {
        self.clock += f64::from(dt.max(0.0));
        self.run_schedule();
        self.aim();

        // After a rest `dt` is the whole rest: play one frame of it, so that
        // the gesture that woke Coco starts gently and not three frames in.
        let mut left = if dt > 0.05 { 1.0 / 60.0 } else { dt.max(0.0) };
        while left > 0.0 {
            let h = left.min(1.0 / 240.0);
            left -= h;
            self.integrate(h);
        }
    }

    fn run_schedule(&mut self) {
        let now = self.clock;
        let mut due = Vec::new();
        self.cues.retain(|&(at, cue)| {
            if at <= now {
                due.push(cue);
                false
            } else {
                true
            }
        });
        for cue in due {
            match cue {
                Cue::Blink => {
                    self.open.target = 0.0;
                    self.cue(0.10, Cue::OpenEyes);
                }
                Cue::OpenEyes => self.open.target = 1.0,
                Cue::Jump => {
                    self.airborne = true;
                    self.bounces = 0;
                    self.hop_v = -3.5;
                    self.stretch.target = 1.06;
                }
                Cue::EndGlance => self.glance = None,
            }
        }

        if matches!(self.revert_at, Some(at) if at <= now) {
            self.set_mood(Mood::Idle);
        }

        let calm = matches!(self.mood, Mood::Idle | Mood::Sorry | Mood::Waking);
        if now >= self.next_blink {
            if self.mood != Mood::Thinking {
                self.cue(0.0, Cue::Blink);
                if self.random() < 0.18 {
                    self.cue(0.26, Cue::Blink);
                }
            }
            self.next_blink = now + f64::from(2.6 + 3.8 * self.random());
        }
        if now >= self.next_glance {
            if calm && self.pointer.is_none() && self.drag.is_none() {
                let a = self.random() * TAU;
                self.glance = Some([a.cos() * 0.9, a.sin() * 0.45]);
                let hold = 0.7 + 0.9 * self.random();
                self.cue(hold, Cue::EndGlance);
                // A character often blinks as its eyes move.
                if self.random() < 0.4 {
                    self.cue(0.0, Cue::Blink);
                    self.next_blink = self.next_blink.max(now + 1.5);
                }
            }
            self.next_glance = now + f64::from(5.0 + 6.0 * self.random());
        }
    }

    /// Sets the spring targets from what is going on right now.
    fn aim(&mut self) {
        // The pointer is on the body, not merely close to it.
        let touched = self.drag.is_none() && self.pointer.is_some_and(|p| p[0].hypot(p[1]) < 1.3);

        // Where the eyes look, by priority.
        let mut gaze = [0.0, 0.0];
        if let Some(v) = self.drag {
            let speed = (v[0] * v[0] + v[1] * v[1]).sqrt();
            if speed > 0.2 {
                let k = (speed / 6.0).min(1.0) / speed;
                gaze = [v[0] * k, v[1] * k];
            }
        } else if self.mood == Mood::Thinking {
            gaze = [0.85 * self.turn(1.0 / 1.4).sin(), -0.12];
        } else if let Some(p) = self.pointer {
            let d = (p[0] * p[0] + p[1] * p[1]).sqrt().max(1e-3);
            // Full interest within 4 units, none from 6.
            let interest = ((6.0 - d) / 2.0).clamp(0.0, 1.0);
            let k = (d / 1.6).min(1.0) * interest / d;
            gaze = [p[0] * k, p[1] * k];
        } else if let Some(g) = self.glance {
            gaze = g;
        } else if let Some(a) = self.attention {
            gaze = a;
        }
        self.gaze_x.target = gaze[0];
        self.gaze_y.target = gaze[1];

        // Squash and stretch.
        if !self.airborne && !self.cues.iter().any(|(_, cue)| *cue == Cue::Jump) {
            self.stretch.target = if self.pressed && self.drag.is_none() {
                0.90
            } else if let Some(v) = self.drag {
                1.0 + (v[1].abs() * 0.005).min(0.09) - (v[0].abs() * 0.0025).min(0.05)
            } else if touched {
                1.035
            } else {
                1.0
            };
        }
        self.lean.target = match self.drag {
            // The base is held by the pointer, so the top trails behind.
            Some(v) => (-v[0] * 0.012).clamp(-0.22, 0.22),
            None => 0.0,
        };

        // Wide eyes under the pointer.
        let look = self.mood.look();
        self.eye_h.target = look.eye_h * if touched { 1.10 } else { 1.0 };
    }

    fn integrate(&mut self, h: f32) {
        self.morph.step(h);
        self.pop.step(h);
        self.stretch.step(h);
        self.lean.step(h);
        self.eye_w.step(h);
        self.eye_h.step(h);
        self.eye_tilt.step(h);
        self.eye_gap.step(h);
        self.eye_x.step(h);
        self.eye_y.step(h);
        self.open.step(h);
        self.gaze_x.step(h);
        self.gaze_y.step(h);
        self.color_t = (self.color_t + h / 0.32).min(1.0);
        let ease = 1.0 - (-h / 0.25).exp();
        self.breath += (f32::from(u8::from(self.breathe)) - self.breath) * ease;
        self.think += (f32::from(u8::from(self.mood == Mood::Thinking)) - self.think) * ease;

        let floor = if self.mood == Mood::Thinking { 0.42 } else { 0.0 };
        self.jelly = floor + (self.jelly - floor) * (-h / 0.42).exp();

        if self.airborne {
            self.hop_v += GRAVITY * h;
            self.hop_y += self.hop_v * h;
            if self.hop_y >= 0.0 && self.hop_v > 0.0 {
                // Landing: squash on impact, then one smaller bounce.
                self.hop_y = 0.0;
                self.stretch.v -= 1.6 * self.hop_v;
                self.jelly = self.jelly.max(0.75);
                if self.bounces == 0 {
                    self.bounces = 1;
                    self.hop_v = -1.9;
                    self.stretch.target = 1.0;
                } else {
                    self.airborne = false;
                    self.hop_v = 0.0;
                    self.stretch.target = 1.0;
                }
            }
        }
    }

    fn springs(&self) -> [&Spring; 13] {
        [
            &self.pop, &self.morph, &self.stretch, &self.lean, &self.eye_w, &self.eye_h, &self.eye_tilt,
            &self.eye_gap, &self.eye_x, &self.eye_y, &self.open, &self.gaze_x, &self.gaze_y,
        ]
    }

    /// The phase, in radians, of something that turns `hz` times a second.
    /// Taken on the 64-bit clock, so it stays smooth after days of running.
    fn turn(&self, hz: f64) -> f32 {
        (self.clock * hz).fract() as f32 * TAU
    }

    fn cue(&mut self, delay: f32, cue: Cue) {
        self.cues.push((self.clock + f64::from(delay), cue));
    }

    fn random(&mut self) -> f32 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        (self.seed >> 8) as f32 / (1u32 << 24) as f32
    }

    fn radii(&self) -> Radii {
        let m = self.morph.x;
        std::array::from_fn(|i| self.from[i] + (self.to[i] - self.from[i]) * m)
    }

    pub fn color(&self) -> Rgb {
        let t = self.color_t * self.color_t * (3.0 - 2.0 * self.color_t);
        std::array::from_fn(|i| self.color_from[i] + (self.color_to[i] - self.color_from[i]) * t)
    }

    /// Squash, lean and hop, applied to the body and to the eyes alike so
    /// that the face stays stuck on the body.
    fn place(&self, p: [f32; 2]) -> [f32; 2] {
        let mut sy = self.stretch.x;
        if matches!(self.mood, Mood::Idle | Mood::Sorry | Mood::Waking) {
            sy += 0.016 * self.breath * self.turn(1.0 / 3.6).sin();
        }
        sy += 0.022 * self.think * self.turn(1.16).sin();
        let sy = sy.clamp(0.6, 1.5);
        let sx = sy.powf(-0.7);
        let lean = self.lean.x + 0.03 * self.gaze_x.x;

        let grow = self.pop.x.max(0.0);
        let dy = (p[1] - BASE_Y) * sy * grow;
        let x = p[0] * sx * grow - lean * dy;
        [x, BASE_Y + dy + self.hop_y]
    }

    pub fn frame(&self) -> Frame {
        let radii = self.radii();
        let ripple = self.jelly;
        let waves = [self.turn(0.84), self.turn(1.13), self.turn(1.5)];
        let body = (0..N)
            .map(|i| {
                let a = angle(i);
                let wave = 0.030 * (2.0 * a + waves[0]).sin()
                    + 0.022 * (3.0 * a - waves[1] + 1.3).sin()
                    + 0.012 * (5.0 * a + waves[2] + 0.4).sin();
                let r = radii[i] * (1.0 + ripple * wave);
                self.place([r * a.cos(), r * a.sin()])
            })
            .collect();

        let open = self.open.x.clamp(0.0, 1.0);
        let w = self.eye_w.x * (1.0 + 0.22 * (1.0 - open));
        // A closed eye is a short dash, not a dot.
        let h = (self.eye_w.x * 0.52) + (self.eye_h.x - self.eye_w.x * 0.52) * open;
        let cx = self.eye_x.x + 0.13 * self.gaze_x.x;
        let cy = self.eye_y.x + 0.10 * self.gaze_y.x;
        let tilt = self.eye_tilt.x.to_radians() + 0.10 * self.gaze_x.x * open;
        let eyes = [-0.5f32, 0.5].map(|side| {
            pill(w.max(0.02), h.max(0.02))
                .into_iter()
                .map(|[x, y]| {
                    let (s, c) = tilt.sin_cos();
                    self.place([cx + side * self.eye_gap.x + x * c - y * s, cy + x * s + y * c])
                })
                .collect()
        });

        Frame { body, color: self.color(), eyes }
    }
}

/// A capsule centred on the origin, as a closed outline.
fn pill(w: f32, h: f32) -> Vec<[f32; 2]> {
    let r = w.min(h) * 0.5;
    let (half_w, half_h) = (w * 0.5 - r, h * 0.5 - r);
    let mut points = Vec::with_capacity(4 * EYE_CAP_STEPS + 4);
    for corner in 0..4 {
        let (cx, cy) = match corner {
            0 => (half_w, half_h),
            1 => (-half_w, half_h),
            2 => (-half_w, -half_h),
            _ => (half_w, -half_h),
        };
        for step in 0..=EYE_CAP_STEPS {
            let a = (corner as f32 + step as f32 / EYE_CAP_STEPS as f32) * TAU / 4.0;
            points.push([cx + r * a.cos(), cy + r * a.sin()]);
        }
    }
    points
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(m: &mut Mascot, seconds: f32) {
        for _ in 0..(seconds * 60.0) as usize {
            m.step(1.0 / 60.0);
        }
    }

    fn eye_height(m: &Mascot) -> f32 {
        let eye = &m.frame().eyes[0];
        let (lo, hi) = eye.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| (lo.min(p[1]), hi.max(p[1])));
        hi - lo
    }

    #[test]
    fn it_comes_to_rest_and_says_when_to_wake() {
        let mut m = Mascot::default();
        m.step(1.0 / 60.0);
        assert!(m.is_resting());
        assert!(m.next_wake() > 0.5);
    }

    #[test]
    fn the_entrance_grows_from_nothing_past_full_size_and_settles() {
        let mut m = Mascot::default();
        m.enter();
        let width = |m: &Mascot| {
            let body = m.frame().body;
            body.iter().fold(0.0f32, |w, p| w.max(p[0])) - body.iter().fold(0.0f32, |w, p| w.min(p[0]))
        };
        assert!(width(&m) < 0.01);
        let mut widest = 0.0f32;
        for _ in 0..180 {
            m.step(1.0 / 60.0);
            widest = widest.max(width(&m));
        }
        let rest = width(&Mascot::default());
        assert!(widest > rest * 1.03, "no overshoot: {widest} against {rest}");
        assert!((width(&m) - rest).abs() < 0.03);
    }

    #[test]
    fn it_looks_at_a_near_pointer_then_rests_while_the_pointer_is_still() {
        let mut m = Mascot::default();
        m.set_pointer(Some([4.0, -2.0]));
        run(&mut m, 1.5);
        assert!(m.gaze_x.x > 0.5 && m.gaze_y.x < -0.2, "not looking: {} {}", m.gaze_x.x, m.gaze_y.x);
        assert!(m.is_resting(), "a still pointer must not keep the animation running");
        m.set_pointer(None);
        m.step(1.0 / 60.0);
        assert!(!m.is_resting(), "the eyes must travel back");
    }

    #[test]
    fn after_days_of_running_it_still_blinks_and_rests() {
        let mut m = Mascot::default();
        m.clock = 80.0 * 3600.0;
        m.next_blink = m.clock + 0.5;
        m.next_glance = m.clock + 100.0;
        let open = eye_height(&m);
        let mut smallest = open;
        // A 120 Hz display: the frame time a 32-bit clock could not add any more.
        for _ in 0..(120 * 3) {
            m.step(1.0 / 120.0);
            smallest = smallest.min(eye_height(&m));
        }
        assert!(smallest < open * 0.5, "no blink: {smallest} of {open}");
        assert!((eye_height(&m) - open).abs() < 0.02, "the eyes stayed shut");
        assert!(m.is_resting(), "it never came back to rest");
    }

    #[test]
    fn a_press_released_as_the_drag_starts_does_not_stay_pressed() {
        let mut m = Mascot::default();
        m.press();
        m.drop_it();
        run(&mut m, 2.0);
        assert!((m.stretch.x - 1.0).abs() < 0.01, "still squashed: {}", m.stretch.x);
        assert!(m.is_resting());
    }

    #[test]
    fn results_in_quick_succession_give_one_hop() {
        let mut m = Mascot::default();
        let mut hops = 0;
        for _ in 0..3 {
            m.set_mood(Mood::Thinking);
            run(&mut m, 0.4);
            m.set_mood(Mood::Happy);
            let mut was_airborne = false;
            for _ in 0..48 {
                m.step(1.0 / 60.0);
                was_airborne |= m.airborne;
            }
            hops += u32::from(was_airborne);
        }
        assert_eq!(hops, 1, "one hop for three results inside four seconds");
    }

    #[test]
    fn it_rests_while_it_holds_a_glance_and_wakes_to_end_it() {
        let mut m = Mascot::default();
        m.next_blink = 100.0;
        m.next_glance = 0.1;
        run(&mut m, 0.6);
        assert!(m.glance.is_some(), "no glance started");
        assert!(m.is_resting(), "holding a glance must not keep the frames coming");
        assert!(m.next_wake() < 1.2, "nothing set to end the glance");
    }

    #[test]
    fn it_blinks_by_itself() {
        let mut m = Mascot::default();
        let open = eye_height(&m);
        let mut smallest = open;
        for _ in 0..(60 * 8) {
            m.step(1.0 / 60.0);
            smallest = smallest.min(eye_height(&m));
        }
        assert!(smallest < open * 0.5, "eyes never closed: {smallest} of {open}");
        run(&mut m, 1.0);
    }

    #[test]
    fn a_long_sleep_does_not_break_the_springs() {
        let mut m = Mascot::default();
        m.step(30.0);
        run(&mut m, 1.5);
        for p in m.frame().body {
            assert!(p[0].abs() < 1.5 && p[1].abs() < 1.5, "body flew away: {p:?}");
        }
    }

    #[test]
    fn happy_hops_then_returns_to_idle() {
        let mut m = Mascot::default();
        m.set_mood(Mood::Thinking);
        run(&mut m, 1.0);
        assert!(!m.is_resting(), "thinking must keep moving");
        m.set_mood(Mood::Happy);
        let mut top = 0.0f32;
        for _ in 0..(60 * 1) {
            m.step(1.0 / 60.0);
            top = top.min(m.hop_y);
        }
        assert!(top < -0.2, "no hop: {top}");
        run(&mut m, 4.0);
        assert_eq!(m.mood(), Mood::Idle);
        assert!(m.hop_y == 0.0 && !m.airborne);
    }

    #[test]
    fn a_press_squashes_and_a_release_springs_back() {
        let mut m = Mascot::default();
        m.press();
        run(&mut m, 0.5);
        assert!(m.stretch.x < 0.93);
        m.release();
        let mut peak = 0.0f32;
        for _ in 0..90 {
            m.step(1.0 / 60.0);
            peak = peak.max(m.stretch.x);
        }
        assert!(peak > 1.02, "no overshoot after release: {peak}");
    }

    #[test]
    fn every_frame_stays_inside_the_window_margin() {
        let mut m = Mascot::default();
        let mut worst = 0.0f32;
        let script: [(f32, fn(&mut Mascot)); 6] = [
            (0.0, |m| m.set_pointer(Some([0.5, -0.4]))),
            (0.6, |m| m.press()),
            (0.9, |m| m.release()),
            (1.2, |m| m.set_mood(Mood::Thinking)),
            (2.6, |m| m.set_mood(Mood::Happy)),
            (5.0, |m| m.set_mood(Mood::Sorry)),
        ];
        let mut next = 0;
        for i in 0..(60 * 7) {
            let t = i as f32 / 60.0;
            while next < script.len() && script[next].0 <= t {
                script[next].1(&mut m);
                next += 1;
            }
            m.step(1.0 / 60.0);
            for p in m.frame().body {
                worst = worst.max(p[0].abs()).max(p[1].abs());
            }
        }
        assert!(worst < 1.42, "the body reaches {worst}, outside the canvas");
    }
}
