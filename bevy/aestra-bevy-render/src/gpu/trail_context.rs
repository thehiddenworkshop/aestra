//! Shared identity of the simulation whose GPU trail checkpoints are valid.
#[derive(Default, PartialEq)]
pub(super) struct TrailContext {
    pub emitters: Vec<u8>,
    // Full seed, explicit context revision, playback mode/duration, world transform.
    pub key: [u32; 22],
    pub motion: Option<std::sync::Arc<aestra_runtime::HostTransformContext>>,
}
