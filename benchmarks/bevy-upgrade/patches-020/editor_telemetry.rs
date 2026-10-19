//! Actual renderer observation models and particle readback observer for editor qualification.
pub use crate::event_statistics::GpuEventLinkStatistics;
pub use crate::output_context::AestraOutputEvent;
#[path = "../../../bevy/aestra-bevy-render/src/gpu/geometry_sample.rs"]
mod geometry_sample;
mod draw_resources {
    pub(super) use super::geometry_sample::GeometrySample;
}
#[path = "../../../bevy/aestra-bevy-render/src/gpu/particle_statistics.rs"]
mod particle_statistics;
pub use particle_statistics::GpuParticleStatistics;
#[path = "../../../bevy/aestra-bevy-render/src/gpu/simulation_observation.rs"]
mod simulation_timing;
#[path = "../../../bevy/aestra-bevy-render/src/gpu/timestamp_observation.rs"]
mod timestamp_observation;
pub use simulation_timing::GpuSimulationTiming;
#[path = "../../../bevy/aestra-bevy-render/src/gpu/preparation_observation.rs"]
mod preparation_observation;
pub use preparation_observation::GpuPreparationTiming;
#[path = "../../../bevy/aestra-bevy-render/src/gpu/field_observations.rs"]
mod field_observations;
pub use field_observations::{GpuStageTiming, GpuTrailStatistics};

// Explicit headless observations, not substitutes for the production component types.
// Particle counts pass through the actual production ReadbackComplete observer.
pub(crate) fn particle_observation(
    app: &mut bevy::prelude::App,
    owner: bevy::prelude::Entity,
    time: f32,
    counts: &[u32],
) {
    use bevy::render::gpu_readback::ReadbackComplete;
    let player = app.world().get::<crate::PresentedEffect>(owner).unwrap();
    let stats = GpuParticleStatistics::new(&player.instance);
    let token = stats.context_token(&player.instance).unwrap();
    let epoch = player.instance.history_epoch();
    let mut words: Vec<u32> = counts.iter().flat_map(|count| [0, *count, 0, 0]).collect();
    words.extend([
        aestra_gpu::PARTICLE_STATISTICS_MAGIC,
        token,
        epoch,
        time.to_bits(),
    ]);
    app.world_mut().entity_mut(owner).insert(stats);
    let readback = app
        .world_mut()
        .spawn(particle_statistics::ParticleStatisticsOwner(owner))
        .observe(particle_statistics::receive_particle_statistics)
        .id();
    app.world_mut().trigger(ReadbackComplete {
        entity: readback,
        data: words.into_iter().flat_map(u32::to_le_bytes).collect(),
    });
    app.world_mut().despawn(readback);
}

pub(crate) fn timings(app: &mut bevy::prelude::App, owner: bevy::prelude::Entity, time: f32) {
    let player = app.world().get::<crate::PresentedEffect>(owner).unwrap();
    let stats = app.world().get::<GpuParticleStatistics>(owner).unwrap();
    let token = stats.context_token(&player.instance).unwrap();
    let sample = |nanoseconds| GpuSimulationTiming {
        sample: Some(timestamp_observation::Sample {
            owner,
            token,
            time,
            nanoseconds,
            sequence: 1,
            work: None,
        }),
    };
    app.world_mut().entity_mut(owner).insert((
        sample(17),
        GpuPreparationTiming {
            compaction: sample(5),
            culling: sample(7),
        },
    ));
}

pub(crate) fn geometry(app: &mut bevy::prelude::App, owner: bevy::prelude::Entity, time: f32) {
    let token = {
        let player = app.world().get::<crate::PresentedEffect>(owner).unwrap();
        app.world()
            .get::<GpuParticleStatistics>(owner)
            .unwrap()
            .context_token(&player.instance)
            .unwrap()
    };
    app.world_mut()
        .get_mut::<GpuParticleStatistics>(owner)
        .unwrap()
        .geometry = Some(geometry_sample::GeometrySample {
        owner: Some(owner),
        token,
        time,
        instances: 9,
        vertices: 36,
        primitives: 18,
        draws: 3,
    });
}

pub(crate) fn field_and_trail(
    app: &mut bevy::prelude::App,
    owner: bevy::prelude::Entity,
    time: f32,
) {
    let epoch = app
        .world()
        .get::<crate::PresentedEffect>(owner)
        .unwrap()
        .instance
        .history_epoch();
    app.world_mut().entity_mut(owner).insert((
        GpuStageTiming {
            sample: Some((time, 41)),
        },
        GpuTrailStatistics {
            epoch: Some(epoch),
            usage: aestra_runtime::TrailUsage {
                occupied: 3,
                retired: 1,
                truncated: 2,
                evictions: 4,
            },
        },
    ));
}
