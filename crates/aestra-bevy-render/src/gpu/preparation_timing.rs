//! Separate per-owner render-preparation windows. No CPU waits or particle readbacks.
use super::simulation_timing::{GpuSimulationTiming, SimulationTimer, TimingBatch, TimingMailbox};
use super::*;
use aestra_runtime::{EffectInstance, EffectProfile, ProfileValue};
use bevy::ecs::system::SystemParam;
use bevy::render::{renderer::RenderQueue, sync_world::MainEntity};

/// GPU trail compaction and per-view culling, excluding simulation and drawing.
#[derive(Component, Debug, Default)]
pub struct GpuPreparationTiming {
    compaction: GpuSimulationTiming,
    culling: GpuSimulationTiming,
}

impl GpuPreparationTiming {
    pub fn record_profile(
        &self,
        instance: &EffectInstance,
        context: &GpuParticleStatistics,
        profile: &mut EffectProfile,
    ) {
        if profile.trail_capacity.value() == Some(0) {
            profile.gpu_trail_compaction_time_ns = ProfileValue::Measured(0);
            profile.gpu_trail_culling_time_ns = ProfileValue::Measured(0);
        } else {
            profile.gpu_trail_compaction_time_ns = self.compaction.time_ns(instance, context);
            profile.gpu_trail_culling_time_ns = self.culling.time_ns(instance, context);
        }
    }
}

#[derive(Resource, Default, Clone)]
pub(super) struct PreparationMailboxes {
    pub compaction: TimingMailbox,
    pub culling: TimingMailbox,
}

pub(super) fn receive(
    mailboxes: Res<PreparationMailboxes>,
    mut players: Query<(
        &PresentedEffect,
        &GpuParticleStatistics,
        &mut GpuPreparationTiming,
    )>,
) {
    for (compaction, mailbox) in [(true, &mailboxes.compaction), (false, &mailboxes.culling)] {
        let Some(samples) = mailbox.take() else {
            continue;
        };
        for (_, _, mut timing) in &mut players {
            if compaction {
                timing.compaction.sample = None;
            } else {
                timing.culling.sample = None;
            }
        }
        for sample in samples {
            let Ok((player, context, mut timing)) = players.get_mut(sample.owner) else {
                continue;
            };
            if context.context_token(&player.instance) == Some(sample.token)
                && sample.time.is_finite()
                && sample.time >= 0.0
                && sample.time <= player.instance.time()
            {
                if compaction {
                    timing.compaction.sample = Some(sample);
                } else {
                    timing.culling.sample = Some(sample);
                }
            }
        }
    }
}

#[derive(SystemParam)]
pub(super) struct TimingContext<'w, 's> {
    device: Res<'w, RenderDevice>,
    queue: Res<'w, RenderQueue>,
    pub mailboxes: Res<'w, PreparationMailboxes>,
    effects: Query<'w, 's, (&'static MainEntity, &'static GpuEffectBuffers)>,
}

impl TimingContext<'_, '_> {
    pub fn begin(&self, timer: &mut SimulationTimer) -> Option<TimingBatch> {
        timer.begin(&self.device, self.queue.get_timestamp_period())
    }

    pub fn owner(&self, batch: &mut TimingBatch, owner: Entity) -> Option<u32> {
        let (_, effect) = self.effects.iter().find(|(main, _)| main.id() == owner)?;
        batch.instance(owner, effect.statistics_token, effect.simulation_time)
    }
}

#[cfg(test)]
mod tests {
    use super::super::simulation_timing::Sample;
    use super::*;
    use ProfileValue::{Measured as M, Unavailable as U};

    #[test]
    fn independent_stage_snapshots_preserve_owners_and_invalidate_stale_contexts() {
        let mut asset = aestra_core::EffectAsset::new("Timing", 3.0);
        asset
            .emitters
            .push(aestra_core::Emitter::basic_sprite("Particles", 3.0));
        let effect = Arc::new(
            aestra_compiler::EffectCompiler::default()
                .compile(&asset)
                .unwrap(),
        );
        let mut app = App::new();
        let mailboxes = PreparationMailboxes::default();
        app.insert_resource(mailboxes.clone())
            .add_systems(PreUpdate, receive);
        let mut owners = Vec::new();
        let mut samples = Vec::new();
        for ns in [11, 29] {
            let mut presented = PresentedEffect::new(effect.clone());
            presented.instance.set_playback_time(2.0);
            let context = GpuParticleStatistics::new(&presented.instance);
            let token = context.context_token(&presented.instance).unwrap();
            let owner = app
                .world_mut()
                .spawn((presented, context, GpuPreparationTiming::default()))
                .id();
            owners.push(owner);
            samples.push(Sample {
                owner,
                token,
                time: 1.5,
                nanoseconds: ns,
            });
        }
        let read = |app: &App, owner| {
            let player = app.world().get::<PresentedEffect>(owner).unwrap();
            let context = app.world().get::<GpuParticleStatistics>(owner).unwrap();
            let mut profile = EffectProfile::from_compiled(&effect);
            profile.trail_capacity = M(64);
            app.world()
                .get::<GpuPreparationTiming>(owner)
                .unwrap()
                .record_profile(&player.instance, context, &mut profile);
            (
                profile.gpu_trail_compaction_time_ns,
                profile.gpu_trail_culling_time_ns,
            )
        };
        assert_eq!(read(&app, owners[0]), (U, U));
        mailboxes.compaction.publish(2, samples.clone());
        mailboxes.culling.publish(
            1,
            vec![Sample {
                nanoseconds: 5,
                ..samples[1].clone()
            }],
        );
        app.update();
        assert_eq!(read(&app, owners[0]), (M(11), U));
        assert_eq!(read(&app, owners[1]), (M(29), M(5)));
        // A late old frame cannot overwrite a newer observation.
        mailboxes.compaction.publish(1, Vec::new());
        app.update();
        assert_eq!(read(&app, owners[1]), (M(29), M(5)));
        // Empty/failed/budget-omitted snapshots clear just their own stage.
        mailboxes.compaction.publish(3, Vec::new());
        app.update();
        assert_eq!(read(&app, owners[1]), (U, M(5)));
        app.world_mut()
            .get_mut::<PresentedEffect>(owners[1])
            .unwrap()
            .instance
            .seek(2.0);
        assert_eq!(read(&app, owners[1]), (U, U));
        app.world_mut().entity_mut(owners[0]).despawn();
        mailboxes.compaction.publish(4, samples.clone());
        mailboxes.culling.publish(2, samples);
        app.update();
        assert_eq!(read(&app, owners[1]), (U, U));
    }

    #[test]
    fn non_trail_gpu_instances_contribute_known_zero_without_timestamp_support() {
        let mut asset = aestra_core::EffectAsset::new("Sprite", 1.0);
        asset
            .emitters
            .push(aestra_core::Emitter::basic_sprite("Sprite", 1.0));
        let instance = EffectInstance::new(Arc::new(
            aestra_compiler::EffectCompiler::default()
                .compile(&asset)
                .unwrap(),
        ));
        let context = GpuParticleStatistics::new(&instance);
        let mut profile = EffectProfile::from_compiled(instance.effect());
        GpuPreparationTiming::default().record_profile(&instance, &context, &mut profile);
        assert_eq!(profile.gpu_trail_compaction_time_ns, M(0));
        assert_eq!(profile.gpu_trail_culling_time_ns, M(0));
        assert_eq!(profile.gpu_time_ns, U);
    }
}
