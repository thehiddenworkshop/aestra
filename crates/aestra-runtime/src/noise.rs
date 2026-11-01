//! Deterministic v1 lattice fields. Shader twins live with the compiler's built-in
//! material functions; no random state, wall clock, texture or Bevy dependency.
use aestra_core::noise::{MAX_NOISE_OCTAVES, NOISE_PERIOD, NoiseBasis, NoiseSettings};

fn hash(cell: [u32; 3], seed: u32) -> u32 {
    let mut h = seed
        ^ cell[0].wrapping_mul(0x9e3779b9)
        ^ cell[1].wrapping_mul(0x85ebca6b)
        ^ cell[2].wrapping_mul(0xc2b2ae35);
    h = (h ^ (h >> 16)).wrapping_mul(0x7feb352d);
    h = (h ^ (h >> 15)).wrapping_mul(0x846ca68b);
    h ^ (h >> 16)
}
fn fade(t: f32) -> f32 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}
fn gradient(h: u32, d: [f32; 3], dimensions: u8) -> f32 {
    if dimensions == 2 {
        const G: [[f32; 2]; 8] = [
            [1.0, 0.0],
            [-1.0, 0.0],
            [0.0, 1.0],
            [0.0, -1.0],
            [
                std::f32::consts::FRAC_1_SQRT_2,
                std::f32::consts::FRAC_1_SQRT_2,
            ],
            [
                -std::f32::consts::FRAC_1_SQRT_2,
                std::f32::consts::FRAC_1_SQRT_2,
            ],
            [
                std::f32::consts::FRAC_1_SQRT_2,
                -std::f32::consts::FRAC_1_SQRT_2,
            ],
            [
                -std::f32::consts::FRAC_1_SQRT_2,
                -std::f32::consts::FRAC_1_SQRT_2,
            ],
        ];
        let g = G[(h & 7) as usize];
        return g[0] * d[0] + g[1] * d[1];
    }
    let h = h & 15;
    let u = if h < 8 { d[0] } else { d[1] };
    let v = if h < 4 {
        d[1]
    } else if h == 12 || h == 14 {
        d[0]
    } else {
        d[2]
    };
    (if h & 1 == 0 { u } else { -u }) + (if h & 2 == 0 { v } else { -v })
}
fn basis(position: [f32; 3], settings: &NoiseSettings, seed: u32) -> f32 {
    let mut p = position;
    if settings.dimensions == 2 {
        p[2] = 0.0;
    }
    let cell = p.map(|v| v.floor());
    let f = std::array::from_fn::<_, 3, _>(|i| p[i] - cell[i]);
    let u = f.map(fade);
    let mut sum = 0.0;
    for corner in 0..(1u32 << settings.dimensions) {
        let offset = [corner & 1, (corner >> 1) & 1, (corner >> 2) & 1];
        let c = std::array::from_fn(|i| {
            let v = cell[i] + offset[i] as f32;
            (v - (v / NOISE_PERIOD).floor() * NOISE_PERIOD) as u32
        });
        let h = hash(c, seed);
        let value = match settings.basis {
            NoiseBasis::Value => (h >> 8) as f32 / 16_777_216.0,
            NoiseBasis::Gradient => gradient(
                h,
                std::array::from_fn(|i| f[i] - offset[i] as f32),
                settings.dimensions,
            ),
        };
        let mut weight = 1.0;
        for axis in 0..usize::from(settings.dimensions) {
            weight *= if offset[axis] == 0 {
                1.0 - u[axis]
            } else {
                u[axis]
            };
        }
        sum += value * weight;
    }
    match settings.basis {
        NoiseBasis::Value => sum,
        NoiseBasis::Gradient => (0.5 + 0.5 * sum).clamp(0.0, 1.0),
    }
}

/// Returns unsigned noise times amplitude. Invalid/non-finite inputs are rejected.
/// Octaves (1..8), lacunarity (1..4), gain (0..1), and scale (0..4096) have
/// identical runtime/shader clamps. Filtering tends toward the field mean 0.5.
pub fn sample(position: [f32; 3], s: &NoiseSettings) -> Option<f32> {
    if !(2..=3).contains(&s.dimensions)
        || position
            .into_iter()
            .chain(s.offset)
            .chain([s.scale, s.amplitude, s.lacunarity, s.gain, s.footprint])
            .any(|v| !v.is_finite())
    {
        return None;
    }
    let mut p = std::array::from_fn(|i| (position[i] + s.offset[i]) * s.scale.clamp(0.0, 4096.0));
    if p.iter().any(|v| !v.is_finite()) {
        return None;
    }
    let mut footprint = s.footprint.max(0.0) * s.scale.clamp(0.0, 4096.0);
    let mut weight = 1.0;
    let mut total = 0.0;
    let mut weights = 0.0;
    for octave in 0..s.octaves.clamp(1, MAX_NOISE_OCTAVES) {
        let t = ((footprint - 0.25) / 0.5).clamp(0.0, 1.0);
        let visible = 1.0 - t * t * (3.0 - 2.0 * t);
        let value = if visible > 0.0 {
            basis(p, s, s.seed.wrapping_add(octave.wrapping_mul(0x9e3779b9)))
        } else {
            0.5
        };
        total += (0.5 + (value - 0.5) * visible) * weight;
        weights += weight;
        let lacunarity = s.lacunarity.clamp(1.0, 4.0);
        p = p.map(|v| v * lacunarity);
        footprint *= lacunarity;
        weight *= s.gain.clamp(0.0, 1.0);
    }
    let result = total / weights * s.amplitude.max(0.0);
    result.is_finite().then_some(result)
}

#[cfg(test)]
#[path = "tests/noise.rs"]
mod tests;
