//! Actual module-palette state and lifecycle stage identities.
use aestra_core::StageKind;
use bevy::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StackStage {
    EmitterSpawn,
    EmitterUpdate,
    ParticleSpawn,
    ParticleUpdate,
    Render,
}

impl StackStage {
    /// The fixed lifecycle stages shown as stack sections, in execution order (§28.3, §31.2). Empty
    /// sections are still shown so the stack has a stable structure. Authored simulation stages render
    /// as their own sections between `ParticleUpdate` and `Render`; they are not part of this list.
    pub(super) const LIFECYCLE: [Self; 4] = [
        Self::EmitterSpawn,
        Self::EmitterUpdate,
        Self::ParticleSpawn,
        Self::ParticleUpdate,
    ];

    pub(super) fn title(self) -> &'static str {
        match self {
            Self::EmitterSpawn => "EMITTER SPAWN",
            Self::EmitterUpdate => "EMITTER UPDATE",
            Self::ParticleSpawn => "PARTICLE SPAWN",
            Self::ParticleUpdate => "PARTICLE UPDATE",
            Self::Render => "RENDER",
        }
    }

    pub(super) fn semantic(self) -> Option<StageKind> {
        match self {
            Self::EmitterSpawn => Some(StageKind::EmitterSpawn),
            Self::EmitterUpdate => Some(StageKind::EmitterUpdate),
            Self::ParticleSpawn => Some(StageKind::ParticleSpawn),
            Self::ParticleUpdate => Some(StageKind::ParticleUpdate),
            Self::Render => None,
        }
    }
}

#[derive(Resource)]
pub(crate) struct ModulePaletteState {
    pub(crate) open: bool,
    pub(crate) stage: StackStage,
    pub(crate) query: String,
}

impl Default for ModulePaletteState {
    fn default() -> Self {
        Self {
            open: false,
            stage: StackStage::EmitterUpdate,
            query: String::new(),
        }
    }
}
