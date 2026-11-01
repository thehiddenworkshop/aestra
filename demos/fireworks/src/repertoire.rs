//! Host-owned variety cues. Particle shape, breaks and cooling live in saved effects.
use bevy::prelude::Vec3;

pub const FAMILIES: [&str; 8] = [
    "point_star",
    "gold_tail",
    "ring",
    "palm",
    "willow",
    "crossette",
    "crackle",
    "strobe",
];

#[derive(Clone, Copy)]
pub struct Cue {
    pub at: f32,
    pub family: usize,
    pub origin: Vec3,
    pub velocity: Vec3,
    pub fuse: f32,
}

const fn cue(at: f32, family: usize, lane: f32, fan: f32, height: f32, fuse: f32) -> Cue {
    Cue {
        at,
        family,
        origin: Vec3::new(lane * 24.0, 0.0, -8.0),
        velocity: Vec3::new(fan, height, 0.0),
        fuse,
    }
}

/// Three low fan palms, paired rings, a round peony, branching/secondary breaks,
/// a high willow pause, then a mixed-height closing phrase. Never an owner cap.
pub const CUES: [Cue; 22] = [
    cue(0.0, 3, -1.0, -13.0, 36.0, 1.05),
    cue(0.18, 3, 0.0, 0.0, 38.0, 1.1),
    cue(0.36, 3, 1.0, 13.0, 36.0, 1.05),
    cue(2.4, 2, -0.7, -3.0, 46.0, 1.4),
    cue(2.8, 2, 0.7, 3.0, 48.0, 1.45),
    cue(4.2, 0, 0.0, 0.0, 52.0, 1.55),
    cue(5.4, 5, -1.0, -4.0, 40.0, 1.2),
    cue(5.65, 5, 0.0, 0.0, 44.0, 1.35),
    cue(5.9, 5, 1.0, 4.0, 40.0, 1.2),
    cue(7.6, 7, -0.8, -2.0, 49.0, 1.5),
    cue(8.0, 7, 0.8, 2.0, 46.0, 1.4),
    cue(9.4, 6, -0.6, -3.0, 43.0, 1.3),
    cue(9.8, 6, 0.6, 3.0, 46.0, 1.4),
    cue(11.2, 4, -0.45, -2.0, 48.0, 1.7),
    cue(11.65, 4, 0.45, 2.0, 50.0, 1.75),
    cue(14.8, 3, -1.0, -14.0, 36.0, 1.0),
    cue(15.0, 2, -0.6, -6.0, 45.0, 1.4),
    cue(15.2, 6, 0.0, 0.0, 42.0, 1.25),
    cue(15.4, 1, 0.0, 0.0, 48.0, 1.55),
    cue(15.6, 5, 0.6, 6.0, 45.0, 1.4),
    cue(15.8, 7, 0.8, 3.0, 49.0, 1.5),
    cue(16.0, 3, 1.0, 14.0, 36.0, 1.0),
];

pub fn launch(serial: u64) -> Cue {
    CUES[serial as usize % CUES.len()]
}
