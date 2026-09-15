use super::*;
use aestra_runtime::{CompiledEffect, EffectInstance, EffectProfile};
use std::sync::atomic::{AtomicU32, Ordering};

// Unique across owners and rebuilt buffers, not merely across seeks of one player.
static NEXT_CONTEXT: AtomicU32 = AtomicU32::new(1);

/// Latest asynchronous native-GPU live counts and actual draw submissions.
/// No particle records are read back.
/// Counts can lag playback; context edits invalidate them immediately, even before
/// the render-world upload has caught up. GPU timings are not implied by this data.
#[derive(Component, Debug)]
pub struct GpuParticleStatistics {
    effect: Arc<CompiledEffect>,
    epoch: u32,
    revision: u64,
    seed: u64,
    token: u32,
    observation: Option<(f32, Vec<u32>)>,
    pub(super) geometry: Option<super::geometry_statistics::Sample>,
}

impl GpuParticleStatistics {
    pub(super) fn new(instance: &EffectInstance) -> Self {
        Self {
            effect: instance.effect().clone(),
            epoch: instance.history_epoch(),
            revision: instance.history_revision(),
            seed: instance.seed(),
            token: NEXT_CONTEXT.fetch_add(1, Ordering::Relaxed),
            observation: None,
            geometry: None,
        }
    }

    fn matches(&self, instance: &EffectInstance) -> bool {
        Arc::ptr_eq(&self.effect, instance.effect())
            && self.epoch == instance.history_epoch()
            && self.revision == instance.history_revision()
            && self.seed == instance.seed()
    }

    pub(super) fn context_token(&self, instance: &EffectInstance) -> Option<u32> {
        self.matches(instance).then_some(self.token)
    }

    pub(super) fn sync(&mut self, instance: &EffectInstance) -> u32 {
        if !self.matches(instance) {
            *self = Self::new(instance);
        }
        self.token
    }

    /// Simulation time and counts in compiled emitter order, or no valid observation yet.
    pub fn observation(&self, instance: &EffectInstance) -> Option<(f32, &[u32])> {
        if !self.matches(instance) {
            return None;
        }
        self.observation.as_ref().and_then(|(time, counts)| {
            (*time <= instance.time()).then_some((*time, counts.as_slice()))
        })
    }

    pub fn record_profile(&self, instance: &EffectInstance, profile: &mut EffectProfile) -> bool {
        self.observation(instance)
            .is_some_and(|(_, counts)| profile.record_particle_counts(counts))
    }

    /// Updates actual submissions independently from live counts. Missing or stale
    /// frames clear the metrics; indexed vertices count references, not unique vertices.
    pub fn record_geometry_profile(&self, instance: &EffectInstance, profile: &mut EffectProfile) {
        use aestra_runtime::ProfileValue::{Measured, Unavailable};
        profile.submitted_instances = Unavailable;
        profile.submitted_vertices = Unavailable;
        profile.submitted_primitives = Unavailable;
        profile.draw_calls = Unavailable;
        if let Some(sample) = &self.geometry
            && self.context_token(instance) == Some(sample.token)
            && sample.time <= instance.time()
        {
            profile.submitted_instances = Measured(sample.instances);
            profile.submitted_vertices = Measured(sample.vertices);
            profile.submitted_primitives = Measured(sample.primitives);
            profile.draw_calls = Measured(sample.draws);
        }
    }

    fn receive(&mut self, instance: &EffectInstance, words: &[u32]) {
        let count = self.effect.emitters.len();
        if !self.matches(instance) || words.len() != count * 4 + 4 {
            return;
        }
        let trailer = &words[count * 4..];
        let time = f32::from_bits(trailer[3]);
        if trailer[0] != aestra_gpu::PARTICLE_STATISTICS_MAGIC
            || trailer[1] != self.token
            || trailer[2] != self.epoch
            || !time.is_finite()
            || time < 0.0
            || time > instance.time()
            || self
                .observation
                .as_ref()
                .is_some_and(|(previous, _)| time < *previous)
        {
            return;
        }
        let counts: Vec<_> = words[..count * 4]
            .as_chunks::<4>()
            .0
            .iter()
            .map(|command| command[1])
            .collect();
        if counts
            .iter()
            .zip(&self.effect.emitters)
            .any(|(&alive, emitter)| {
                alive > emitter.max_particles || (!emitter.enabled && alive != 0)
            })
        {
            return;
        }
        self.observation = Some((time, counts));
    }
}

#[derive(Component)]
pub(super) struct ParticleStatisticsOwner(pub Entity);

pub(super) fn receive_particle_statistics(
    event: On<ReadbackComplete>,
    owners: Query<&ParticleStatisticsOwner>,
    mut players: Query<(&PresentedEffect, &mut GpuParticleStatistics)>,
) {
    let Ok(owner) = owners.get(event.event_target()) else {
        return;
    };
    let Ok((player, mut statistics)) = players.get_mut(owner.0) else {
        return;
    };
    let words: Vec<u32> = event.to_shader_type();
    statistics.receive(&player.instance, &words);
}

#[cfg(test)]
mod tests {
    use super::*;
    use aestra_compiler::EffectCompiler;
    use aestra_core::{EffectAsset, Emitter};

    fn instance() -> EffectInstance {
        let mut asset = EffectAsset::new("Telemetry", 3.0);
        asset.emitters.push(Emitter::basic_sprite("Particles", 3.0));
        let mut instance =
            EffectInstance::new(Arc::new(EffectCompiler::default().compile(&asset).unwrap()));
        instance.set_playback_time(2.0);
        instance
    }

    fn words(stats: &GpuParticleStatistics, time: f32, count: u32) -> Vec<u32> {
        vec![
            4,
            count,
            0,
            0,
            aestra_gpu::PARTICLE_STATISTICS_MAGIC,
            stats.token,
            stats.epoch,
            time.to_bits(),
        ]
    }

    #[test]
    fn rejects_stale_contexts_and_out_of_order_observations() {
        let mut instance = instance();
        let mut stats = GpuParticleStatistics::new(&instance);
        assert!(stats.observation(&instance).is_none());
        stats.receive(&instance, &words(&stats, 1.5, 7));
        stats.receive(&instance, &words(&stats, 1.0, 3));
        assert_eq!(stats.observation(&instance), Some((1.5, &[7][..])));
        for change in 0..4 {
            let stale = words(&stats, 1.5, 9);
            match change {
                0 => instance.seek(2.0),
                1 => instance.set_seed(123),
                2 => instance.invalidate_history(),
                _ => instance.restart(),
            }
            assert!(stats.observation(&instance).is_none());
            stats.receive(&instance, &stale);
            stats.sync(&instance);
            stats.receive(&instance, &stale);
            assert!(stats.observation(&instance).is_none());
            stats.receive(&instance, &words(&stats, instance.time(), 5));
            assert_eq!(stats.observation(&instance).unwrap().1, &[5]);
        }
    }

    #[test]
    fn isolates_owners_recompiles_and_rebuilt_buffers() {
        let instance = instance();
        let mut stats = GpuParticleStatistics::new(&instance);
        let stale = words(&stats, 1.0, 7);
        let mut other = GpuParticleStatistics::new(&instance);
        other.receive(&instance, &stale);
        assert!(other.observation(&instance).is_none());
        let mut recompiled = EffectInstance::new(Arc::new((**instance.effect()).clone()));
        recompiled.set_playback_time(2.0);
        stats.receive(&recompiled, &stale);
        assert!(stats.observation(&recompiled).is_none());
        stats.sync(&recompiled);
        stats.receive(&recompiled, &stale);
        assert!(stats.observation(&recompiled).is_none());
    }

    #[test]
    fn rejects_uninitialized_truncated_future_and_invalid_counts() {
        let instance = instance();
        let mut stats = GpuParticleStatistics::new(&instance);
        for data in [
            vec![0; 8],
            vec![0; 4],
            words(&stats, 3.0, 2),
            words(&stats, f32::NAN, 2),
            words(&stats, 1.0, u32::MAX),
        ] {
            stats.receive(&instance, &data);
            assert!(stats.observation(&instance).is_none());
        }
        stats.receive(&instance, &words(&stats, 1.0, 0));
        let mut profile = EffectProfile::from_compiled(instance.effect());
        assert!(stats.record_profile(&instance, &mut profile));
        assert_eq!(
            profile.alive_particles,
            aestra_runtime::ProfileValue::Measured(0)
        );
        assert_eq!(
            profile.gpu_time_ns,
            aestra_runtime::ProfileValue::Unavailable
        );
    }

    #[test]
    fn readbacks_follow_nested_owners_and_project_roots() {
        use aestra_runtime::{ProfileValue, ProjectInstanceProfile, ProjectProfile};
        let mut app = App::new();
        let instance = instance();
        let first_root = app.world_mut().spawn_empty().id();
        let second_root = app.world_mut().spawn_empty().id();
        let mut entries = Vec::new();
        for (root, path, count) in [
            (
                first_root,
                vec![
                    aestra_core::EffectClipId::new(),
                    aestra_core::EffectClipId::new(),
                ],
                3,
            ),
            (first_root, vec![aestra_core::EffectClipId::new()], 5),
            (second_root, vec![aestra_core::EffectClipId::new()], 11),
        ] {
            let stats = GpuParticleStatistics::new(&instance);
            let data = words(&stats, 1.5, count)
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect();
            let mut presented = PresentedEffect::new(instance.effect().clone());
            presented.instance = instance.clone();
            let owner = app
                .world_mut()
                .spawn((ChildOf(root), presented, stats))
                .id();
            let readback = app
                .world_mut()
                .spawn((ChildOf(owner), ParticleStatisticsOwner(owner)))
                .observe(receive_particle_statistics)
                .id();
            app.world_mut().trigger(ReadbackComplete {
                entity: readback,
                data,
            });
            let mut profile = EffectProfile::from_compiled(instance.effect());
            assert!(
                app.world()
                    .get::<GpuParticleStatistics>(owner)
                    .unwrap()
                    .record_profile(&instance, &mut profile)
            );
            entries.push((
                root,
                owner,
                ProjectInstanceProfile {
                    path,
                    effect: instance.effect().source,
                    name: "Nested".into(),
                    profile,
                },
            ));
        }
        for (root, expected) in [(first_root, 8), (second_root, 11)] {
            let mut project = ProjectProfile::default();
            project.update(
                entries
                    .iter()
                    .filter(|(r, _, _)| *r == root)
                    .map(|(_, _, e)| e.clone())
                    .collect(),
            );
            assert_eq!(
                project.total.alive_particles,
                ProfileValue::Measured(expected)
            );
        }
        app.world_mut().entity_mut(first_root).despawn();
        assert!(app.world().get_entity(entries[0].1).is_err());
        assert!(
            app.world()
                .get::<GpuParticleStatistics>(entries[2].1)
                .is_some()
        );
    }
}
