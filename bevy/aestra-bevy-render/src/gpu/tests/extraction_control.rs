//! Settings, pacing and backend-publication policy shared by both engines.
use super::super::{
    capability_publication::publish_detected_capabilities,
    catchup_pacing::{AestraCatchupPacing, CatchupPacer},
};
use super::test_app;
use crate::{
    AestraRenderSettings, AestraRuntimeStatus, GpuCapabilities, PresentationMode,
    TransparentOrderMode, capabilities::ActiveBackend,
};
use aestra_runtime::SeekQuality;
use bevy::{
    prelude::*,
    render::{Render, RenderApp, RenderSystems},
};
use std::time::{Duration, Instant};

#[derive(Resource, Default)]
struct PreparedSettings(Option<(PresentationMode, u32, TransparentOrderMode, u32)>);

#[test]
fn render_settings_update_and_restore_before_prepare_without_changing_host_values() {
    let mut app = test_app();
    app.sub_app_mut(RenderApp)
        .init_resource::<PreparedSettings>()
        .add_systems(
            Render,
            (|settings: Res<AestraRenderSettings>,
              pacing: Res<CatchupPacer>,
              mut observed: ResMut<PreparedSettings>| {
                observed.0 = Some((
                    settings.presentation,
                    settings.max_gpu_particles,
                    settings.transparent_order,
                    pacing.budget(SeekQuality::Exact),
                ));
            })
            .in_set(RenderSystems::Prepare),
        );
    let default = AestraRenderSettings::default();
    assert_eq!(default.presentation, PresentationMode::Auto);
    assert_eq!(
        default.max_gpu_particles,
        crate::capabilities::DEFAULT_GPU_PARTICLE_BUDGET
    );
    assert_eq!(default.transparent_order, TransparentOrderMode::Fast);
    app.insert_resource(default);
    for (mode, budget, order) in [
        (PresentationMode::Auto, 262_144, TransparentOrderMode::Fast),
        (
            PresentationMode::Gpu,
            128,
            TransparentOrderMode::DepthBackToFront,
        ),
        (
            PresentationMode::CpuReference,
            0,
            TransparentOrderMode::StableCapture,
        ),
        (
            PresentationMode::GpuReadback,
            42,
            TransparentOrderMode::Fast,
        ),
    ] {
        app.insert_resource(AestraRenderSettings {
            presentation: mode,
            max_gpu_particles: budget,
            transparent_order: order,
        });
        app.update();
        let expected = Some((mode, budget, order, 4));
        assert_eq!(
            app.sub_app(RenderApp)
                .world()
                .resource::<PreparedSettings>()
                .0,
            expected
        );
        let host = app.world().resource::<AestraRenderSettings>();
        assert_eq!(
            (
                host.presentation,
                host.max_gpu_particles,
                host.transparent_order
            ),
            (mode, budget, order)
        );
        app.sub_app_mut(RenderApp)
            .world_mut()
            .remove_resource::<AestraRenderSettings>();
        app.update();
        assert_eq!(
            app.sub_app(RenderApp)
                .world()
                .resource::<PreparedSettings>()
                .0,
            expected
        );
    }
}

#[test]
fn optional_pacing_removal_restores_default_without_resetting_adaptive_history() {
    let mut app = test_app();
    assert!(AestraCatchupPacing::default().paced);
    let start = Instant::now();
    let adaptive = {
        let mut pacer = app
            .sub_app_mut(RenderApp)
            .world_mut()
            .resource_mut::<CatchupPacer>();
        pacer.frame(start);
        let budget = pacer.budget(SeekQuality::Exact);
        pacer.spent(budget, budget);
        pacer.frame(start + Duration::from_millis(10));
        pacer.budget(SeekQuality::Exact)
    };
    assert!(adaptive > 4);
    app.update(); // Absence means paced, retaining learned budget.
    assert_eq!(
        app.sub_app(RenderApp)
            .world()
            .resource::<CatchupPacer>()
            .budget(SeekQuality::Exact),
        adaptive
    );
    app.insert_resource(AestraCatchupPacing { paced: false });
    app.update();
    let pacer = app.sub_app(RenderApp).world().resource::<CatchupPacer>();
    assert_eq!(pacer.budget(SeekQuality::Exact), 300);
    assert_eq!(pacer.budget(SeekQuality::Preview), 24);
    app.world_mut().remove_resource::<AestraCatchupPacing>();
    app.update();
    assert_eq!(
        app.sub_app(RenderApp)
            .world()
            .resource::<CatchupPacer>()
            .budget(SeekQuality::Exact),
        adaptive
    );
    app.insert_resource(AestraCatchupPacing { paced: false });
    app.update();
    app.world_mut().resource_mut::<AestraCatchupPacing>().paced = true;
    app.update();
    assert_eq!(
        app.sub_app(RenderApp)
            .world()
            .resource::<CatchupPacer>()
            .budget(SeekQuality::Exact),
        adaptive
    );
}

fn supported() -> GpuCapabilities {
    GpuCapabilities {
        detected: true,
        compute_shaders: true,
        compute_pipeline_supported: true,
        indirect_execution: true,
        vertex_storage: true,
        native_render_supported: true,
        max_bind_groups: 2,
        max_bindings_per_bind_group: 8,
        max_storage_buffers_per_shader_stage: 8,
        max_compute_invocations_per_workgroup: 64,
        max_compute_workgroup_size_x: 64,
        max_particles: 1000,
        ..Default::default()
    }
}

#[test]
fn capability_publication_reacts_to_modes_and_device_changes_but_not_unchanged_frames() {
    let mut world = World::new();
    world.init_resource::<AestraRenderSettings>();
    world.init_resource::<AestraRuntimeStatus>();
    world.init_resource::<GpuCapabilities>();
    assert_eq!(
        world.resource::<AestraRuntimeStatus>().active,
        ActiveBackend::Pending
    );
    assert!(publish_detected_capabilities(&mut world, supported()));
    assert_eq!(
        world.resource::<AestraRuntimeStatus>().active,
        ActiveBackend::Gpu
    );
    world.clear_trackers();
    assert!(!publish_detected_capabilities(&mut world, supported()));
    assert!(
        !world
            .get_resource_ref::<GpuCapabilities>()
            .unwrap()
            .is_changed()
    );
    assert!(
        !world
            .get_resource_ref::<AestraRuntimeStatus>()
            .unwrap()
            .is_changed()
    );
    for (mode, expected) in [
        (PresentationMode::CpuReference, ActiveBackend::CpuReference),
        (PresentationMode::GpuReadback, ActiveBackend::GpuReadback),
        (PresentationMode::Gpu, ActiveBackend::Gpu),
        (PresentationMode::Auto, ActiveBackend::Gpu),
    ] {
        world.resource_mut::<AestraRenderSettings>().presentation = mode;
        assert!(publish_detected_capabilities(&mut world, supported()));
        assert_eq!(world.resource::<AestraRuntimeStatus>().active, expected);
        assert_eq!(world.resource::<AestraRuntimeStatus>().requested, mode);
        assert_eq!(world.resource::<GpuCapabilities>(), &supported());
    }
    let unavailable = GpuCapabilities::unavailable("no device");
    assert!(publish_detected_capabilities(
        &mut world,
        unavailable.clone()
    ));
    assert_eq!(
        world.resource::<AestraRuntimeStatus>().active,
        ActiveBackend::CpuReference
    );
    assert_eq!(world.resource::<GpuCapabilities>(), &unavailable);
    assert!(publish_detected_capabilities(&mut world, supported()));
    assert_eq!(
        world.resource::<AestraRuntimeStatus>().active,
        ActiveBackend::Gpu
    );
}
