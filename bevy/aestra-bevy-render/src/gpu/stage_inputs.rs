//! Actual domain/field presentation extraction metadata.
use aestra_gpu::{GpuHostBindings, GpuWorldSdf};
use aestra_runtime::{CompiledEffect, FieldLayout, PlaybackHistoryPolicy, SeekQuality};
use bevy::{asset::AssetId, prelude::*};
use std::sync::Arc;

/// What the render world needs to run an effect's extension stages this frame.
#[derive(Component, Clone)]
pub(crate) struct ExtractedStages {
    pub(super) effect: Arc<CompiledEffect>,
    pub(super) output_identity: Arc<()>,
    pub(super) time: f32,
    pub(super) quality: SeekQuality,
    pub(super) history_policy: PlaybackHistoryPolicy,
    pub(super) host: GpuHostBindings,
    pub(super) seed: u32,
    pub(super) history_epoch: u32,
    pub(super) history_epoch_start_time: f32,
    pub(super) history_revision: u64,
    pub(super) host_epoch: u64,
    /// The effect's placement: world space into its space (fluid F2).
    pub(super) world_to_effect: [[f32; 4]; 3],
    /// Stateful emitters follow a domain field (fluid F2b): the stateful path advances the domains in
    /// lockstep with them, so this system must not advance them on its own.
    pub(super) coupled: bool,
    pub(super) view: Option<FieldViewTarget>,
    /// Fields copied into volume textures after the stages advance (fluid F3).
    pub(super) volumes: Vec<VolumeFieldTarget>,
    /// The host's world SDF (fluid F11), when it supplies one.
    pub(super) world: Option<GpuWorldSdf>,
}

impl ExtractedStages {
    pub(super) fn from_presented(
        presented: &crate::PresentedEffect,
        world_to_effect: [[f32; 4]; 3],
        view: Option<FieldViewTarget>,
        volumes: Vec<VolumeFieldTarget>,
        world: Option<GpuWorldSdf>,
    ) -> Self {
        let instance = &presented.instance;
        let effect = presented.effect();
        Self {
            effect: effect.clone(),
            output_identity: presented.output_identity.clone(),
            time: instance.time(),
            quality: presented.seek_quality(),
            history_policy: presented.history_policy(),
            host: GpuHostBindings::from_instance(instance),
            seed: instance.seed() as u32,
            history_epoch: instance.history_epoch(),
            history_epoch_start_time: instance.history_epoch_start_time(),
            history_revision: instance.history_revision(),
            host_epoch: instance.host_input_epoch(),
            world_to_effect,
            coupled: effect
                .emitters
                .iter()
                .any(|emitter| emitter.enabled && emitter.coupled()),
            view,
            volumes,
            world,
        }
    }

    pub(super) fn inputs(&self) -> crate::execution::StageInputs<'_> {
        crate::execution::StageInputs {
            host_bindings: Some(&self.host),
            world_to_effect: self.world_to_effect,
            world_sdf: self.world.as_ref(),
        }
    }
}

/// A field slice the render world copies into a view image.
#[derive(Clone)]
pub(super) struct FieldViewTarget {
    pub(super) image: AssetId<Image>,
    pub(super) stage: usize,
    pub(super) layout: FieldLayout,
    pub(super) slice: u32,
    pub(super) gain: f32,
}

/// A field the render world copies into a volume texture each frame.
#[derive(Clone)]
pub(super) struct VolumeFieldTarget {
    pub stage: usize,
    pub layout: FieldLayout,
    pub image: AssetId<Image>,
    /// A bricked field's table image (fluid F7).
    pub table: Option<AssetId<Image>>,
}
