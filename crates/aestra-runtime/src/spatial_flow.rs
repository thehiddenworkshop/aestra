//! A shared acceleration field, independent of particle identity and lifetime.
use aestra_core::{
    noise::{NoiseBasis, NoiseSettings},
    spatial_flow::{FlowMode, FlowSpace, SpatialFlow},
};
pub const IDENTITY: [[f32; 4]; 3] = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
];

/// Curl uses centered differences of a vector potential, not normalized vectors.
/// The scale-dependent step resolves the highest octave; constant Lipschitz
/// normalization bounds every component by Strength without changing divergence.
pub fn sample(position: [f32; 3], time: f32, flow: SpatialFlow) -> Option<[f32; 3]> {
    if !flow.is_valid() || !time.is_finite() || position.iter().any(|v| !v.is_finite()) {
        return None;
    }
    if flow.strength == 0.0 {
        return Some([0.0; 3]);
    }
    let q = std::array::from_fn::<_, 3, _>(|i| {
        (position[i] + flow.offset[i] - flow.evolution[i] * time) * flow.scale
    });
    let settings = NoiseSettings {
        basis: NoiseBasis::Value,
        dimensions: 3,
        scale: 1.0,
        octaves: flow.octaves,
        ..Default::default()
    };
    let potential = |p, channel: u32| {
        crate::noise::sample(
            p,
            &NoiseSettings {
                seed: flow.seed.wrapping_add(channel.wrapping_mul(0x9e3779b9)),
                ..settings
            },
        )
        .map(|n| n * 2.0 - 1.0)
    };
    if flow.mode == FlowMode::Noise {
        return Some([
            potential(q, 0)? * flow.strength,
            potential(q, 1)? * flow.strength,
            potential(q, 2)? * flow.strength,
        ]);
    }
    let highest = (1u32 << (flow.octaves - 1)) as f32;
    let h = 1.0 / (128.0 * highest);
    let derivative = |channel, axis: usize| {
        let mut a = q;
        let mut b = q;
        a[axis] += h;
        b[axis] -= h;
        Some((potential(a, channel)? - potential(b, channel)?) / (2.0 * h))
    };
    let mut weight = 1.0;
    let mut weights = 0.0;
    let mut weighted_frequency = 0.0;
    for octave in 0..flow.octaves {
        weights += weight;
        weighted_frequency += weight * (1u32 << octave) as f32;
        weight *= 0.5;
    }
    let gain = flow.strength / (7.5 * weighted_frequency / weights);
    Some([
        (derivative(2, 1)? - derivative(1, 2)?) * gain,
        (derivative(0, 2)? - derivative(2, 0)?) * gain,
        (derivative(1, 0)? - derivative(0, 1)?) * gain,
    ])
}

pub fn inverse_linear(m: [[f32; 4]; 3]) -> Option<[[f32; 3]; 3]> {
    if m.iter().flatten().any(|v| !v.is_finite()) {
        return None;
    }
    let cross = |a: [f32; 3], b: [f32; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let columns = std::array::from_fn::<_, 3, _>(|c| [m[0][c], m[1][c], m[2][c]]);
    let rows = [
        cross(columns[1], columns[2]),
        cross(columns[2], columns[0]),
        cross(columns[0], columns[1]),
    ];
    let det = columns[0][0] * rows[0][0] + columns[0][1] * rows[0][1] + columns[0][2] * rows[0][2];
    if !det.is_finite() || det == 0.0 {
        return None;
    }
    Some(rows.map(|row| row.map(|v| v / det)))
}
pub fn acceleration(
    position: [f32; 3],
    time: f32,
    flow: SpatialFlow,
    world_from_effect: [[f32; 4]; 3],
) -> Option<[f32; 3]> {
    if flow.space == FlowSpace::Effect {
        return sample(position, time, flow);
    }
    let inverse = inverse_linear(world_from_effect)?;
    let p = world_from_effect
        .map(|r| r[0] * position[0] + r[1] * position[1] + r[2] * position[2] + r[3]);
    let a = sample(p, time, flow)?;
    Some(inverse.map(|r| r[0] * a[0] + r[1] * a[1] + r[2] * a[2]))
}

#[cfg(test)]
#[path = "tests/spatial_flow.rs"]
mod tests;
