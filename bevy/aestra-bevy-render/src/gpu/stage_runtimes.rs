//! Shared production stage allocation, context transitions and checkpoint invalidation.
use super::{
    catchup_pacing::CatchupPacer, stage_inputs::ExtractedStages,
    stage_output_delivery::StageOutputStamp,
};
use crate::execution::{ProgramCache, StageExecutor, StageTimeline, TimelinePolicy};
use aestra_compiler::ExtensionRegistry;
use bevy::{
    prelude::*,
    render::renderer::{RenderDevice, RenderQueue},
};
use std::{collections::BTreeMap, sync::Arc};

fn stages(
    effect: &aestra_runtime::CompiledEffect,
) -> impl Iterator<Item = &aestra_runtime::CompiledExtensionStage> {
    effect.all_extension_stages()
}

/// What one effect's stage timelines were built for: every stage's execution block, the seed and the
/// host-binding size. Content, not the compiled effect's identity — an edit that leaves the stages
/// alone (moving an emitter with the gizmo recompiles every frame) keeps the running simulation.
#[derive(PartialEq)]
pub(super) struct StagesKey {
    blocks: Vec<aestra_runtime::ExecutionBlock>,
    seed: u32,
    host_bytes: u64,
}

impl StagesKey {
    pub(super) fn of(extracted: &ExtractedStages) -> Self {
        Self {
            blocks: stages(&extracted.effect)
                .map(|stage| stage.block.clone())
                .collect(),
            seed: extracted.seed,
            host_bytes: extracted.host.byte_len(),
        }
    }

    pub(super) fn matches(&self, extracted: &ExtractedStages) -> bool {
        self.seed == extracted.seed
            && self.host_bytes == extracted.host.byte_len()
            && self
                .blocks
                .iter()
                .eq(stages(&extracted.effect).map(|stage| &stage.block))
    }

    /// Whether the effect's stages differ from these only in their constants' values (fluid F3): a
    /// domain input edit or drag, which the running timelines take in place.
    pub(super) fn differs_only_in_constants(&self, extracted: &ExtractedStages) -> bool {
        let same_shape = |old: &aestra_runtime::ExecutionBlock,
                          new: &aestra_runtime::ExecutionBlock| {
            old.resources == new.resources
                && old.ops == new.ops
                && old.fields == new.fields
                && old.constants.len() == new.constants.len()
        };
        self.seed == extracted.seed
            && self.host_bytes == extracted.host.byte_len()
            && self.blocks.len() == stages(&extracted.effect).count()
            && self
                .blocks
                .iter()
                .zip(stages(&extracted.effect))
                .all(|(old, new)| same_shape(old, &new.block))
    }
}

/// One effect's stage timelines in the render world.
pub(super) struct EffectStages {
    key: StagesKey,
    context: StageContext,
    host_epoch: u64,
    /// One per stage; `None` when the stage could not be prepared (logged once).
    pub(super) timelines: Vec<Option<StageTimeline>>,
    /// Per stage, the context/tick last read; old callbacks cannot cross a discontinuity.
    pub(super) read_ticks: Vec<Option<StageOutputStamp>>,
}

/// An epoch alone is a seek and can reuse compatible history. A restart at
/// zero, changed simulation context, or a new presentation must rebuild it.
struct StageContext {
    presentation: Arc<()>,
    revision: u64,
    epoch: u32,
}

impl StageContext {
    fn of(extracted: &ExtractedStages) -> Self {
        Self {
            presentation: extracted.output_identity.clone(),
            revision: extracted.history_revision,
            epoch: extracted.history_epoch,
        }
    }

    fn requires_reset(&self, extracted: &ExtractedStages) -> bool {
        !Arc::ptr_eq(&self.presentation, &extracted.output_identity)
            || self.revision != extracted.history_revision
            || (self.epoch != extracted.history_epoch && extracted.history_epoch_start_time == 0.)
    }
}

#[derive(Resource, Default)]
pub(crate) struct StageRuntimes(pub(super) BTreeMap<Entity, EffectStages>);

/// Builds, keeps or drops each effect's stage timelines.
pub(super) fn prepare_stage_runtimes(
    mut runtimes: ResMut<StageRuntimes>,
    mut pacer: ResMut<CatchupPacer>,
    // Compiled programs outlive the runtimes: a rebuild after an edit reuses them.
    mut programs_cache: Local<ProgramCache>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    effects: Query<(Entity, &ExtractedStages)>,
) {
    pacer.frame(std::time::Instant::now());
    runtimes.0.retain(|entity, _| effects.contains(*entity));
    for (entity, extracted) in &effects {
        if let Some(existing) = runtimes.0.get_mut(&entity) {
            for timeline in existing.timelines.iter_mut().flatten() {
                timeline.set_history_policy(extracted.history_policy);
            }
            if !existing.context.requires_reset(extracted) {
                existing.context = StageContext::of(extracted);
                if existing.host_epoch != extracted.host_epoch {
                    for timeline in existing.timelines.iter_mut().flatten() {
                        timeline.invalidate_checkpoints();
                    }
                    existing.host_epoch = extracted.host_epoch;
                }
            }
        }
        if let Some(existing) = runtimes.0.get_mut(&entity)
            && !existing.context.requires_reset(extracted)
            && existing.key.matches(extracted)
        {
            continue;
        }
        // Only constants changed (an input edit, a source dragged): keep simulating, with the new values.
        if let Some(existing) = runtimes.0.get_mut(&entity)
            && !existing.context.requires_reset(extracted)
            && existing.key.differs_only_in_constants(extracted)
        {
            let blocks: Vec<_> = stages(&extracted.effect)
                .map(|stage| stage.block.clone())
                .collect();
            for (timeline, block) in existing.timelines.iter_mut().zip(&blocks) {
                if let Some(timeline) = timeline
                    && let Err(error) = timeline.set_constants(&queue, &block.constants)
                {
                    warn!("extension stage constants could not be updated: {error}");
                }
            }
            existing.key.blocks = blocks;
            continue;
        }
        // A rebuilt domain replays from tick 0, and its ticks may cost anything now.
        pacer.restart();
        // The linked programs, including any extension linked since the last build.
        let programs = ExtensionRegistry::linked().programs;
        let wgpu_queue: &wgpu::Queue = &queue;
        let timelines = stages(&extracted.effect)
            .map(|stage| {
                StageExecutor::with_cache(
                    device.wgpu_device(),
                    wgpu_queue,
                    &stage.block,
                    &programs,
                    extracted.host.byte_len(),
                    &mut programs_cache,
                )
                .map(|executor| {
                    let mut timeline =
                        StageTimeline::new(executor, TimelinePolicy::default(), extracted.seed);
                    timeline.set_history_policy(extracted.history_policy);
                    timeline
                })
                .map_err(|error| {
                    warn!(
                        "extension stage '{}' ({}) cannot run on this backend: {error}",
                        stage.name,
                        stage.stage_type.as_str()
                    );
                })
                .ok()
            })
            .collect();
        runtimes.0.insert(
            entity,
            EffectStages {
                key: StagesKey::of(extracted),
                context: StageContext::of(extracted),
                host_epoch: extracted.host_epoch,
                read_ticks: vec![None; stages(&extracted.effect).count()],
                timelines,
            },
        );
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_context_distinguishes_seek_from_restart_and_replacement() {
        let effect = Arc::new(
            aestra_compiler::EffectCompiler::default()
                .compile(&aestra_core::EffectAsset::new("Context", 5.))
                .unwrap(),
        );
        let mut presented = crate::PresentedEffect::new(effect);
        let inputs = |presented: &crate::PresentedEffect| {
            ExtractedStages::from_presented(
                presented,
                aestra_runtime::IDENTITY_AFFINE,
                None,
                Vec::new(),
                None,
            )
        };
        let context = StageContext::of(&inputs(&presented));
        assert!(!context.requires_reset(&inputs(&presented)));
        presented.instance.seek(2.);
        assert!(
            !context.requires_reset(&inputs(&presented)),
            "compatible seek retains history"
        );
        presented.instance.restart();
        presented.instance.set_playback_time(3.);
        assert!(
            context.requires_reset(&inputs(&presented)),
            "reset even when the new playhead passed the old tick"
        );
        let context = StageContext::of(&inputs(&presented));
        presented.instance.invalidate_history();
        assert!(context.requires_reset(&inputs(&presented)));
        let context = StageContext::of(&inputs(&presented));
        presented = crate::PresentedEffect::new(presented.effect().clone());
        assert!(context.requires_reset(&inputs(&presented)));
    }
}
