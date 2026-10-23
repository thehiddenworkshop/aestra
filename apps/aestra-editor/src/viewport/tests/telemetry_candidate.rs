//! 0.20 observation integration with real component models and readback decoding.
use super::telemetry::tests::Fixture;
use crate::gpu;
use aestra_bevy_render::{ActiveBackend, PresentedEffect};
use aestra_runtime::ProfileValue::{Measured as M, Unavailable as U};

#[test]
fn validated_particle_counts_and_geometry_reach_the_actual_profiler() {
    let mut f = Fixture::new(Some(ActiveBackend::Gpu));
    let owner = f.root.unwrap();
    gpu::particle_observation(&mut f.app, owner, 0.5, &[2, 3, 4, 5]);
    gpu::geometry(&mut f.app, owner, 0.5);
    f.tick();
    assert_eq!(f.profile().alive_particles, M(14));
    assert_eq!(f.profile().submitted_instances, M(9));
    assert_eq!(f.profile().submitted_vertices, M(36));
    assert_eq!(f.profile().submitted_primitives, M(18));
    assert_eq!(f.profile().draw_calls, M(3));
    assert_eq!(f.profile().cpu_time_ns, U);
    assert_eq!(f.profile().gpu_time_ns, U);
}

#[test]
fn wrong_sized_or_future_particle_readbacks_do_not_become_live_counts() {
    for (time, counts) in [(0.5, vec![7]), (2.0, vec![1, 2, 3, 4])] {
        let mut f = Fixture::new(Some(ActiveBackend::Gpu));
        let owner = f.root.unwrap();
        gpu::particle_observation(&mut f.app, owner, time, &counts);
        f.tick();
        assert_eq!(f.profile().alive_particles, U);
    }
}

#[test]
fn context_changes_invalidate_counts_geometry_and_timestamps_before_ingestion() {
    for change in 0..4 {
        let mut f = Fixture::new(Some(ActiveBackend::Gpu));
        let owner = f.root.unwrap();
        gpu::particle_observation(&mut f.app, owner, 0.5, &[2, 3, 4, 5]);
        gpu::geometry(&mut f.app, owner, 0.5);
        gpu::timings(&mut f.app, owner, 0.5);
        f.tick();
        assert_eq!(f.profile().alive_particles, M(14));
        assert_eq!(f.profile().gpu_simulation_time_ns, M(17));
        let mut player = f.app.world_mut().get_mut::<PresentedEffect>(owner).unwrap();
        match change {
            0 => player.instance.seek(1.0),
            1 => player.instance.set_seed(12345),
            2 => player.instance.invalidate_history(),
            _ => player.instance.restart(),
        }
        f.tick();
        assert_eq!(f.profile().alive_particles, U);
        assert_eq!(f.profile().submitted_vertices, U);
        assert_eq!(f.profile().gpu_simulation_time_ns, U);
    }
}

#[test]
fn measured_simulation_windows_do_not_invent_draw_or_cpu_time() {
    let mut f = Fixture::new(Some(ActiveBackend::Gpu));
    let owner = f.root.unwrap();
    gpu::particle_observation(&mut f.app, owner, 0.5, &[0, 0, 0, 0]);
    gpu::timings(&mut f.app, owner, 0.5);
    f.tick();
    assert_eq!(f.profile().alive_particles, M(0));
    assert_eq!(f.profile().gpu_simulation_time_ns, M(17));
    // This fixture has no trail work; its preparation contribution is known zero.
    assert_eq!(f.profile().gpu_trail_compaction_time_ns, M(0));
    assert_eq!(f.profile().gpu_trail_culling_time_ns, M(0));
    assert_eq!(f.profile().gpu_time_ns, U);
    assert_eq!(f.profile().cpu_time_ns, U);
}

#[test]
fn future_geometry_and_timestamps_are_not_current_measurements() {
    let mut f = Fixture::new(Some(ActiveBackend::Gpu));
    let owner = f.root.unwrap();
    gpu::particle_observation(&mut f.app, owner, 0.5, &[1, 2, 3, 4]);
    gpu::geometry(&mut f.app, owner, 2.0);
    gpu::timings(&mut f.app, owner, 2.0);
    f.tick();
    assert_eq!(f.profile().alive_particles, M(10));
    assert_eq!(f.profile().submitted_vertices, U);
    assert_eq!(f.profile().gpu_simulation_time_ns, U);
}

#[test]
fn cpu_reference_backend_does_not_consume_gpu_simulation_observations() {
    let mut f = Fixture::new(Some(ActiveBackend::CpuReference));
    let owner = f.root.unwrap();
    gpu::particle_observation(&mut f.app, owner, 0.5, &[1, 2, 3, 4]);
    gpu::timings(&mut f.app, owner, 0.5);
    f.tick();
    assert_eq!(f.profile().gpu_simulation_time_ns, U);
    assert_eq!(f.profile().gpu_trail_compaction_time_ns, U);
}

#[test]
fn extension_stage_timings_are_independent_of_particle_backend_and_reject_future_samples() {
    aestra_fluid::link();
    let registry = aestra_compiler::ExtensionRegistry::linked();
    for backend in [ActiveBackend::Gpu, ActiveBackend::CpuReference] {
        let session = crate::EditorSession::from_test_effect(aestra_fluid::fire_effect(&registry));
        let mut f = Fixture::from_session(session, Some(backend));
        let owner = f.root.unwrap();
        gpu::field_and_trail(&mut f.app, owner, 0.5);
        f.tick();
        assert_eq!(f.profile().gpu_stage_time_ns, M(41));
        gpu::field_and_trail(&mut f.app, owner, 2.0);
        f.tick();
        assert_eq!(f.profile().gpu_stage_time_ns, U);
    }
}

#[test]
fn trail_usage_and_preparation_timings_expire_under_their_original_context_rules() {
    let mut effect = crate::test_support::effect_with_timing_slack();
    let renderer = &mut effect.emitters[0].renderers[0];
    renderer.renderer_type = aestra_core::RendererTypeId(aestra_core::RENDERER_TRAIL.into());
    renderer.properties = aestra_core::RendererProperties::Trail {
        width: 0.1,
        sample_interval: 1.0 / 60.0,
        lifetime: 1.0,
        max_points: 16,
        max_trails: 0,
        sampling: Default::default(),
        sample_distance: 0.1,
        curve_tolerance: 0.1,
        uv_mode: Default::default(),
        tile_length: 1.0,
        end_cap: Default::default(),
    };
    let mut f = Fixture::from_session(
        crate::EditorSession::from_test_effect(effect),
        Some(ActiveBackend::Gpu),
    );
    let owner = f.root.unwrap();
    gpu::particle_observation(&mut f.app, owner, 0.5, &[2, 3, 4, 5]);
    gpu::timings(&mut f.app, owner, 0.5);
    gpu::field_and_trail(&mut f.app, owner, 0.5);
    f.tick();
    assert_eq!(f.profile().occupied_trails, M(3));
    assert_eq!(f.profile().retired_trails, M(1));
    assert_eq!(f.profile().truncated_trails, M(2));
    assert_eq!(f.profile().trail_evictions, M(4));
    assert_eq!(f.profile().gpu_trail_compaction_time_ns, M(5));
    assert_eq!(f.profile().gpu_trail_culling_time_ns, M(7));
    f.app
        .world_mut()
        .get_mut::<PresentedEffect>(owner)
        .unwrap()
        .instance
        .seek(1.0);
    f.tick();
    assert_eq!(f.profile().occupied_trails, U);
    assert_eq!(f.profile().gpu_trail_compaction_time_ns, U);
    assert_eq!(f.profile().gpu_trail_culling_time_ns, U);
}

#[test]
fn gpu_readback_keeps_sample_counts_and_consumes_only_context_valid_geometry_and_time() {
    let mut f = Fixture::new(Some(ActiveBackend::GpuReadback));
    let owner = f.root.unwrap();
    let mut samples = Vec::new();
    f.app
        .world_mut()
        .resource_mut::<crate::EditorSession>()
        .evaluate_preview(&mut samples);
    let expected = samples.len() as u32;
    assert!(expected > 0);
    f.app
        .world_mut()
        .get_mut::<PresentedEffect>(owner)
        .unwrap()
        .restore_gpu_samples(samples);
    gpu::particle_observation(&mut f.app, owner, 0.5, &[0, 0, 0, 0]);
    gpu::geometry(&mut f.app, owner, 0.5);
    gpu::timings(&mut f.app, owner, 0.5);
    f.tick();
    assert_eq!(f.profile().alive_particles, M(expected));
    assert_eq!(f.profile().submitted_instances, M(9));
    assert_eq!(f.profile().gpu_simulation_time_ns, M(17));
    assert_eq!(f.profile().cpu_time_ns, U);
}
