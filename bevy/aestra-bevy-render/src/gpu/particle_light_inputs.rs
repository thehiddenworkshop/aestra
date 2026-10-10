//! Actual particle-light extraction metadata and stable source identity.
use aestra_core::{EffectClipId, EffectId, EmitterId, EmitterRegionId, SceneOutputId};
use bevy::prelude::*;
use std::sync::Arc;

/// Global selected-light cap across every GPU presentation in this render app,
/// independent from each output's authored quality cap. Zero disables all light
/// jobs and releases their scratch. A separate host memory budget rejects an
/// over-budget frame explicitly; no hard-coded emitter/output count limit.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub struct AestraParticleLightSettings {
    pub max_lights: u32,
    pub max_scratch_bytes: u64,
}

/// Explicit host realization choice. Unsupported same-frame configurations
/// fail closed; never silently fall back to delayed positional readback.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParticleLightMode {
    #[default]
    PortableAsync,
    SameFrameGpu,
}
impl Default for AestraParticleLightSettings {
    fn default() -> Self {
        Self {
            max_lights: 0,
            max_scratch_bytes: 64 * 1024 * 1024,
        }
    }
}

/// Canonical occurrence identity. Tokens are frame-local manifest indices, not
/// persistent light identities. Consumers must retain the matching frame manifest
/// with any async selected-set copy and validate owner/root epochs before use.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ParticleLightSource {
    pub root: Entity,
    pub root_epoch: u32,
    pub clip_path: Vec<EffectClipId>,
    pub owner: Entity,
    pub owner_epoch: u32,
    pub revision: u64,
    pub effect: EffectId,
    pub seed: u64,
    pub emitter: EmitterId,
    pub region: EmitterRegionId,
    pub output: SceneOutputId,
    /// Keep the originating compiled artifact alive through async consumption.
    /// An in-place replacement with the same authored IDs/seed/epoch is not the
    /// same presentation. Pointer reuse cannot occur while this handle is held.
    pub artifact: ParticleLightArtifact,
}

#[derive(Clone)]
pub struct ParticleLightArtifact(pub Arc<aestra_runtime::CompiledEffect>);
impl ParticleLightArtifact {
    pub fn matches(&self, effect: &Arc<aestra_runtime::CompiledEffect>) -> bool {
        Arc::ptr_eq(&self.0, effect)
    }
    fn address(&self) -> usize {
        Arc::as_ptr(&self.0) as usize
    }
}
impl std::fmt::Debug for ParticleLightArtifact {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("ParticleLightArtifact")
            .field(&self.address())
            .finish()
    }
}
impl PartialEq for ParticleLightArtifact {
    fn eq(&self, other: &Self) -> bool {
        self.matches(&other.0)
    }
}
impl Eq for ParticleLightArtifact {}
impl PartialOrd for ParticleLightArtifact {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for ParticleLightArtifact {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.address().cmp(&other.address())
    }
}

#[derive(Clone)]
pub(super) struct Input {
    pub(super) source: ParticleLightSource,
    pub(super) emitter_index: u32,
    pub(super) offset: u32,
    pub(super) count: u32,
    pub(super) plan: aestra_runtime::ParticlePointLightPlan,
    pub(super) parameters: Arc<[aestra_runtime::RuntimeValue]>,
}
#[derive(Component, Clone, Default)]
pub(super) struct Inputs(pub(super) Vec<Input>);
