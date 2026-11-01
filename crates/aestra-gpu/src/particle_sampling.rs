//! Launch-only settings. No additional per-particle storage or GPU bindings.
use aestra_core::particle_sampling::ParticleSampling;
use glam::UVec4;
pub const SAMPLING_BASE: usize = crate::spatial_flow::FLOW_BASE + crate::spatial_flow::FLOW_WORDS;
pub const SAMPLING_WORDS: usize = 5;
pub fn configuration(settings: Option<ParticleSampling>) -> (UVec4, u32) {
    settings.map_or((UVec4::ZERO, 0), |s| {
        (
            UVec4::new(
                s.positions as u32,
                s.directions as u32,
                s.sequence_count,
                s.jitter.to_bits(),
            ),
            s.seed,
        )
    })
}
pub fn pack(settings: Option<ParticleSampling>, words: &mut [u32]) {
    let (config, seed) = configuration(settings);
    words[SAMPLING_BASE..SAMPLING_BASE + 4].copy_from_slice(&config.to_array());
    words[SAMPLING_BASE + 4] = seed;
}
