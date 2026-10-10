//! Bounded, non-blocking GPU simulation timestamps, separate from draw timings.
pub use super::timestamp_transport::GpuSimulationWork;
pub(super) use super::timestamp_transport::{Sample, SimulationTimer, TimingMailbox};
use super::*;
use aestra_runtime::{EffectInstance, ProfileValue};

/// Latest context-valid GPU simulation observation. Includes all replay dispatches
/// for this instance in the sampled frame, but never its draw calls or CPU work.
#[derive(Component, Debug, Default)]
pub struct GpuSimulationTiming {
    pub(super) sample: Option<Sample>,
}

/// One context-valid asynchronous frame result; sequence identifies a timestamp
/// batch and lets consumers avoid counting a retained result more than once.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GpuSimulationFrame {
    pub sequence: u64,
    pub nanoseconds: u64,
    /// Requested playback time in seconds, not proof that paced catch-up reached it.
    pub requested_time: f32,
    pub work: GpuSimulationWork,
}

impl GpuSimulationTiming {
    /// Returns no frame when timestamps or complete-window work metadata are
    /// unavailable. Legacy mixed analytic/stateful partial windows are excluded.
    pub fn frame_sample(
        &self,
        instance: &EffectInstance,
        context: &GpuParticleStatistics,
    ) -> Option<GpuSimulationFrame> {
        let sample = self.sample.as_ref().filter(|sample| {
            context.context_token(instance) == Some(sample.token) && sample.time <= instance.time()
        })?;
        Some(GpuSimulationFrame {
            sequence: sample.sequence,
            nanoseconds: sample.nanoseconds,
            requested_time: sample.time,
            work: sample.work?,
        })
    }

    pub fn time_ns(
        &self,
        instance: &EffectInstance,
        context: &GpuParticleStatistics,
    ) -> ProfileValue<u64> {
        self.sample
            .as_ref()
            .filter(|sample| {
                context.context_token(instance) == Some(sample.token)
                    && sample.time <= instance.time()
            })
            .map_or(ProfileValue::Unavailable, |sample| {
                ProfileValue::Measured(sample.nanoseconds)
            })
    }
}

pub(super) fn receive_timings(
    mailbox: Res<TimingMailbox>,
    mut players: Query<(
        &PresentedEffect,
        &GpuParticleStatistics,
        &mut GpuSimulationTiming,
    )>,
) {
    let Some(samples) = mailbox.take() else {
        return;
    };
    // Failed maps and instances omitted by the bounded query budget become unknown,
    // rather than displaying measurements from an older active set.
    for (_, _, mut timing) in &mut players {
        timing.sample = None;
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
            timing.sample = Some(sample);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpu_timestamp_batches_resolve_and_recycle_without_unbounded_allocation() {
        super::super::timestamp_transport::tests::
            gpu_timestamp_batches_resolve_and_recycle_without_unbounded_allocation(false);
    }

    fn instance() -> EffectInstance {
        let mut effect = aestra_core::EffectAsset::new("Timed", 3.0);
        effect
            .emitters
            .push(aestra_core::Emitter::basic_sprite("Particles", 3.0));
        let mut instance = EffectInstance::new(Arc::new(
            aestra_compiler::EffectCompiler::default()
                .compile(&effect)
                .unwrap(),
        ));
        instance.set_playback_time(2.0);
        instance
    }

    #[test]
    fn missing_or_partial_work_metadata_does_not_invent_complete_frame_results() {
        let instance = instance();
        let context = GpuParticleStatistics::new(&instance);
        let mut timing = GpuSimulationTiming::default();
        assert!(timing.frame_sample(&instance, &context).is_none());
        timing.sample = Some(Sample {
            owner: Entity::PLACEHOLDER,
            token: context.context_token(&instance).unwrap(),
            time: 1.0,
            nanoseconds: 123,
            sequence: 1,
            work: None,
        });
        assert_eq!(
            timing.time_ns(&instance, &context),
            ProfileValue::Measured(123)
        );
        assert!(timing.frame_sample(&instance, &context).is_none());
    }

    #[test]
    fn timing_is_invalidated_before_render_upload_after_all_context_changes() {
        let mut instance = instance();
        let mut context = GpuParticleStatistics::new(&instance);
        for change in 0..5 {
            let old_token = context.sync(&instance);
            let timing = GpuSimulationTiming {
                sample: Some(Sample {
                    owner: Entity::PLACEHOLDER,
                    token: old_token,
                    time: 0.0,
                    nanoseconds: 123,
                    sequence: 1,
                    work: Some(GpuSimulationWork::default()),
                }),
            };
            assert_eq!(
                timing.time_ns(&instance, &context),
                ProfileValue::Measured(123)
            );
            assert_eq!(
                timing.frame_sample(&instance, &context).unwrap().sequence,
                1
            );
            match change {
                0 => instance.seek(2.0),
                1 => instance.restart(),
                2 => instance.set_seed(77),
                3 => instance.invalidate_history(),
                _ => instance = EffectInstance::new(Arc::new((**instance.effect()).clone())),
            }
            assert!(timing.frame_sample(&instance, &context).is_none());
            assert_eq!(
                timing.time_ns(&instance, &context),
                ProfileValue::Unavailable
            );
            context.sync(&instance);
            assert_eq!(
                timing.time_ns(&instance, &context),
                ProfileValue::Unavailable
            );
        }
    }

    #[test]
    fn frame_results_isolate_repeated_sources_and_discard_expired_owners() {
        use aestra_runtime::{EffectProfile, ProjectInstanceProfile, ProjectProfile};
        let mut app = App::new();
        let mailbox = TimingMailbox::default();
        app.insert_resource(mailbox.clone())
            .add_systems(PreUpdate, receive_timings);
        let instance = instance();
        let mut owners = Vec::new();
        let mut samples = Vec::new();
        for ns in [10, 20, 90] {
            let context = GpuParticleStatistics::new(&instance);
            let token = context.context_token(&instance).unwrap();
            let mut presented = PresentedEffect::new(instance.effect().clone());
            presented.instance = instance.clone();
            let owner = app
                .world_mut()
                .spawn((presented, context, GpuSimulationTiming::default()))
                .id();
            owners.push(owner);
            samples.push(Sample {
                owner,
                token,
                time: 1.5,
                nanoseconds: ns,
                sequence: 1,
                work: Some(GpuSimulationWork {
                    fixed_ticks: Some(1),
                    trail_observations: 1,
                    trail_workgroups: ns,
                    checkpoint_capture_bytes: Some(0),
                }),
            });
        }
        mailbox.publish(1, samples.clone());
        app.update();
        for (owner, groups) in owners.iter().zip([10, 20, 90]) {
            let context = app.world().get::<GpuParticleStatistics>(*owner).unwrap();
            let timing = app.world().get::<GpuSimulationTiming>(*owner).unwrap();
            let frame = timing.frame_sample(&instance, context).unwrap();
            assert_eq!(frame.sequence, 1);
            assert_eq!(frame.requested_time, 1.5);
            assert_eq!(frame.work.trail_workgroups, groups);
        }
        let entries = || {
            owners
                .iter()
                .map(|owner| {
                    let context = app.world().get::<GpuParticleStatistics>(*owner).unwrap();
                    let timing = app.world().get::<GpuSimulationTiming>(*owner).unwrap();
                    let mut profile = EffectProfile::from_compiled(instance.effect());
                    profile.gpu_simulation_time_ns = timing.time_ns(&instance, context);
                    ProjectInstanceProfile {
                        path: vec![
                            aestra_core::EffectClipId::new(),
                            aestra_core::EffectClipId::new(),
                        ],
                        effect: instance.effect().source,
                        name: "Nested repeated source".into(),
                        profile,
                    }
                })
                .collect::<Vec<_>>()
        };
        let entries = entries();
        let mut first_root = ProjectProfile::default();
        first_root.update(entries[..2].to_vec());
        let mut second_root = ProjectProfile::default();
        second_root.update(entries[2..].to_vec());
        assert_eq!(
            first_root.total.gpu_simulation_time_ns,
            ProfileValue::Measured(30)
        );
        assert_eq!(
            second_root.total.gpu_simulation_time_ns,
            ProfileValue::Measured(90)
        );
        assert_eq!(first_root.total.gpu_time_ns, ProfileValue::Unavailable);
        app.world_mut().entity_mut(owners[0]).despawn();
        app.world_mut()
            .get_mut::<PresentedEffect>(owners[1])
            .unwrap()
            .instance
            .seek(2.0);
        mailbox.publish(2, samples);
        app.update();
        assert!(
            app.world()
                .get::<GpuSimulationTiming>(owners[1])
                .unwrap()
                .sample
                .is_none()
        );
        assert_eq!(
            app.world()
                .get::<GpuSimulationTiming>(owners[2])
                .unwrap()
                .sample
                .as_ref()
                .unwrap()
                .nanoseconds,
            90
        );
        // A failed map or a skipped query budget cannot retain old measurements.
        mailbox.publish(3, Vec::new());
        app.update();
        assert!(
            app.world()
                .get::<GpuSimulationTiming>(owners[2])
                .unwrap()
                .sample
                .is_none()
        );
    }
}
