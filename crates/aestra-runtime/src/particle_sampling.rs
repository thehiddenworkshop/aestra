//! Bounded seeded launch layouts. No rejection, sorting, allocation or clock.
use crate::{add3, cross3, scale3};
use aestra_core::{
    EmitterShape, VelocityDistribution,
    particle_sampling::{ParticleSampling, SamplingMode},
};

#[derive(Debug, Clone, Copy)]
pub struct SamplingCohort {
    pub index: u32,
    pub count: u32,
    /// First destination birth ordinal of this cohort, not a persistent owner ID.
    pub first: u32,
}
impl SamplingCohort {
    pub fn sequence(ordinal: u32, count: u32) -> Self {
        let count = count.max(1);
        let index = ordinal % count;
        Self {
            index,
            count,
            first: ordinal - index,
        }
    }
}
fn hash(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846ca68b);
    x ^ (x >> 16)
}
fn unit(x: u32) -> f32 {
    (hash(x) >> 8) as f32 * (1.0 / 16777216.0)
}
fn fraction(x: u32) -> f32 {
    (x >> 8) as f32 * (1.0 / 16777216.0)
}
fn key(settings: ParticleSampling, seed: u32, cohort: SamplingCohort, stream: u32) -> u32 {
    settings.seed ^ seed ^ cohort.first.wrapping_mul(0x9e3779b9) ^ stream.wrapping_mul(0x85ebca6b)
}
/// Position and direction streams are independent. Only layout settings affect
/// these streams; speed and lifetime retain the initializer's original streams.
pub fn coordinates(
    settings: ParticleSampling,
    mode: SamplingMode,
    seed: u32,
    cohort: SamplingCohort,
    stream: u32,
) -> [f32; 3] {
    let k = key(settings, seed, cohort, stream);
    let i = cohort.index;
    let random = std::array::from_fn(|axis| {
        unit(k ^ i.wrapping_mul(0x27d4eb2d) ^ (axis as u32 + 1).wrapping_mul(0x165667b1))
    });
    if mode != SamplingMode::Stratified {
        return random;
    }
    let n = cohort.count.max(1) as f32;
    let jitter = settings.jitter;
    [
        ((i as f32 + 0.5 + (random[0] - 0.5) * jitter * 0.98) / n).min(0.99999994),
        (fraction(i.wrapping_mul(0x9e3779b9).wrapping_add(hash(k)))
            + (random[1] - 0.5) * jitter / n.sqrt())
        .rem_euclid(1.0),
        (fraction(
            i.wrapping_mul(0xbb67ae85)
                .wrapping_add(hash(k ^ 0xa511e9b3)),
        ) + (random[2] - 0.5) * jitter / n.sqrt())
        .rem_euclid(1.0),
    ]
}
fn rotate_sphere(v: [f32; 3], k: u32) -> [f32; 3] {
    let a = unit(k ^ 0x1234567);
    let b = unit(k ^ 0x7654321) * std::f32::consts::TAU;
    let c = unit(k ^ 0xabcdef) * std::f32::consts::TAU;
    let q = [
        (1.0 - a).sqrt() * b.sin(),
        (1.0 - a).sqrt() * b.cos(),
        a.sqrt() * c.sin(),
        a.sqrt() * c.cos(),
    ];
    let t = scale3(cross3([q[0], q[1], q[2]], v), 2.0);
    add3(add3(v, scale3(t, q[3])), cross3([q[0], q[1], q[2]], t))
}
pub fn direction(
    settings: ParticleSampling,
    seed: u32,
    cohort: SamplingCohort,
    mode: VelocityDistribution,
    axis: [f32; 3],
    spread: f32,
) -> [f32; 3] {
    let [u, v, _] = coordinates(settings, settings.directions, seed, cohort, 1);
    let result = crate::sample_velocity_distribution(mode, axis, spread, u, v);
    if mode == VelocityDistribution::Sphere {
        rotate_sphere(result, key(settings, seed, cohort, 1))
    } else {
        result
    }
}
pub fn position(
    settings: ParticleSampling,
    seed: u32,
    cohort: SamplingCohort,
    shape: EmitterShape,
) -> [f32; 3] {
    let [u, v, w] = coordinates(settings, settings.positions, seed, cohort, 2);
    let phi = v * std::f32::consts::TAU;
    match shape {
        EmitterShape::Point => [0.0; 3],
        EmitterShape::Circle { radius } => [
            radius * u.sqrt() * phi.cos(),
            radius * u.sqrt() * phi.sin(),
            0.0,
        ],
        EmitterShape::Ring { radius } => [radius * phi.cos(), radius * phi.sin(), 0.0],
        EmitterShape::Sphere { radius } | EmitterShape::Hemisphere { radius } => {
            let y = if matches!(shape, EmitterShape::Sphere { .. }) {
                1.0 - 2.0 * u
            } else {
                u
            };
            let r = (1.0 - y * y).max(0.0).sqrt();
            let d = [r * phi.cos(), y, r * phi.sin()];
            let d = if matches!(shape, EmitterShape::Sphere { .. }) {
                rotate_sphere(d, key(settings, seed, cohort, 2))
            } else {
                d
            };
            scale3(d, radius * w.powf(1.0 / 3.0))
        }
        EmitterShape::Box { half_extents } => [
            (2.0 * u - 1.0) * half_extents[0],
            (2.0 * v - 1.0) * half_extents[1],
            (2.0 * w - 1.0) * half_extents[2],
        ],
        EmitterShape::Cylinder { radius, depth } => [
            radius * u.sqrt() * phi.cos(),
            (w - 0.5) * depth,
            radius * u.sqrt() * phi.sin(),
        ],
        EmitterShape::Cone { radius, depth } => {
            let height = w.powf(1.0 / 3.0);
            let r = radius * height * u.sqrt();
            [r * phi.cos(), height * depth, r * phi.sin()]
        }
    }
}
