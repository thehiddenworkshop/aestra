//! Fixture wiring for the actual shipping simulation system, not a substitute dispatcher.
use crate::{
    draw_resources::SimulateEffects,
    presentation_readiness::PresentedOwners,
    simulation_pipeline::{init_pipeline, prepare_bind_groups},
    stage_coupling::{init_field_follow, prepare_particle_world},
    stage_runtimes::StageRuntimes,
    timestamp_transport::TimingMailbox,
};
use bevy::{
    asset::io::embedded::EmbeddedAssetRegistry,
    prelude::*,
    render::{
        Render, RenderApp, RenderStartup, RenderSystems,
        renderer::{RenderGraph, RenderGraphSystems},
    },
};

#[derive(Resource, Default)]
pub(super) struct TimingEvidence {
    pub samples: usize,
    pub max_ticks: u32,
    pub replay_allowed: bool,
    pub max_capture_bytes: u64,
    pub max_trail_observations: u32,
    pub windows: Vec<(f32, crate::timestamp_transport::GpuSimulationWork)>,
}

fn observe_timings(mailbox: Res<TimingMailbox>, mut evidence: ResMut<TimingEvidence>) {
    if let Some(samples) = mailbox.take() {
        for sample in samples {
            assert!(sample.time.is_finite());
            assert!(sample.nanoseconds > 0);
            let work = sample
                .work
                .expect("outer graph publishes a complete stateful window");
            if !evidence.replay_allowed {
                assert!(work.checkpoint_capture_bytes.is_none_or(|bytes| bytes == 0));
            }
            evidence.samples += 1;
            evidence.max_ticks = evidence.max_ticks.max(work.fixed_ticks.unwrap_or(0));
            evidence.max_capture_bytes = evidence
                .max_capture_bytes
                .max(work.checkpoint_capture_bytes.unwrap_or(0));
            evidence.max_trail_observations =
                evidence.max_trail_observations.max(work.trail_observations);
            evidence.windows.push((sample.time, work));
        }
    }
}

pub(super) fn install(app: &mut App) {
    install_graph(app, true);
}

pub(super) fn install_with_stage_installer(app: &mut App) {
    install_graph(app, false);
}

fn install_graph(app: &mut App, install_stage_coupling: bool) {
    let path = "aestra_bevy_render/shaders/aestra_simulation.wesl";
    app.world()
        .resource::<EmbeddedAssetRegistry>()
        .insert_asset(
            std::path::PathBuf::from(path),
            std::path::Path::new(path),
            aestra_gpu::shader::SIMULATION_WESL.as_bytes(),
        );
    app.sub_app_mut(RenderApp)
        .init_resource::<StageRuntimes>()
        .init_resource::<TimingMailbox>()
        .init_resource::<TimingEvidence>()
        .insert_resource(PresentedOwners {
            enabled: true,
            ..default()
        })
        .add_systems(RenderStartup, init_pipeline)
        .add_systems(
            Render,
            prepare_bind_groups.in_set(RenderSystems::PrepareBindGroups),
        )
        .add_systems(
            RenderGraph,
            crate::simulation_graph::run_simulation
                .in_set(SimulateEffects)
                .after(RenderGraphSystems::Begin)
                .before(RenderGraphSystems::Render),
        )
        .add_systems(Render, observe_timings.in_set(RenderSystems::Cleanup));
    if install_stage_coupling {
        app.sub_app_mut(RenderApp)
            .add_systems(RenderStartup, init_field_follow)
            .add_systems(
                Render,
                prepare_particle_world.in_set(RenderSystems::PrepareBindGroups),
            );
    }
}

pub(super) fn assert_timings(app: &App) {
    let evidence = app.sub_app(RenderApp).world().resource::<TimingEvidence>();
    assert!(
        evidence.samples > 0,
        "real outer-graph timestamp callbacks must finish"
    );
    assert!(
        evidence.max_ticks
            <= crate::catchup_pacing::stateful_catchup_budget(aestra_runtime::SeekQuality::Preview),
        "bounded preview tick budget"
    );
}
