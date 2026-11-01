//! Spatial force packing and the canonical N1 scalar kernel used by its potential.
use aestra_core::spatial_flow::{FlowMode, FlowSpace, SpatialFlow};
pub const FLOW_BASE: usize = crate::STATEFUL_DISTANCE_BASE + 2;
pub const FLOW_WORDS: usize = 13;
const _: () = assert!(FLOW_BASE == 191);
pub fn pack(flow: Option<SpatialFlow>, time: f32, words: &mut [u32]) {
    words[FLOW_BASE..FLOW_BASE + FLOW_WORDS].fill(0);
    if let Some(f) = flow.filter(|f| f.is_valid()) {
        words[FLOW_BASE..FLOW_BASE + FLOW_WORDS].copy_from_slice(&[
            match f.mode {
                FlowMode::Noise => 1,
                FlowMode::Curl => 2,
            },
            u32::from(f.space == FlowSpace::World),
            f.scale.to_bits(),
            f.strength.to_bits(),
            f.seed,
            f.octaves,
            f.evolution[0].to_bits(),
            f.evolution[1].to_bits(),
            f.evolution[2].to_bits(),
            f.offset[0].to_bits(),
            f.offset[1].to_bits(),
            f.offset[2].to_bits(),
            time.to_bits(),
        ]);
    }
}
pub fn wgsl() -> String {
    format!(
        "{}\n{}",
        aestra_compiler::spatial_noise_wesl(),
        include_str!("shaders/aestra_spatial_flow.wgsl")
    )
}
