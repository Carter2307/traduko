//! Traduko's body and eyes: shapes, springs and motion, with no UI dependency.

mod mascot;
mod shape;
mod spring;

pub use mascot::{Frame, GRAY, INK, Mascot, Mood, ORANGE, Rgb};
pub use shape::{N, Radii, Shape};
pub use spring::Spring;

/// Half the side of the square that holds every pose, in Traduko's units: the
/// body is about 1 unit from centre to edge and needs room to hop and stretch.
pub const CANVAS: f32 = 1.45;
