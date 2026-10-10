//! Actual production payloads and candidate adapter, not substitute components.
pub use aestra_runtime::{
    BackendCapabilities, CompatibilityIssueCode, CompatibilityReport, CompatibilityTarget,
    EffectRequirements, RendererCapability,
};
#[path = "../../../bevy/aestra-bevy-render/src/render_settings.rs"]
mod render_settings;
pub use render_settings::{AestraRenderSettings, PresentationMode, TransparentOrderMode};
#[path = "../../../bevy/aestra-bevy-render/src/capabilities.rs"]
mod capabilities;
pub use capabilities::{AestraRuntimeStatus, GpuCapabilities};
#[path = "../../../bevy/aestra-bevy-render/src/gpu/alpha_sort.rs"]
mod alpha_sort;
mod alpha_sort_native;
#[path = "../../../bevy/aestra-bevy-render/src/gpu/capability_publication.rs"]
mod capability_publication;
#[path = "../../../bevy/aestra-bevy-render/src/gpu/catchup_pacing.rs"]
mod catchup_pacing;
#[path = "../../../bevy/aestra-bevy-render/src/gpu/clone_extraction.rs"]
mod clone_extraction;
#[path = "../../../bevy/aestra-bevy-render/src/gpu/draw_commands.rs"]
mod draw_commands;
#[path = "../../../bevy/aestra-bevy-render/src/gpu/draw_instance.rs"]
mod draw_instance;
#[path = "../../../bevy/aestra-bevy-render/src/gpu/draw_preparation.rs"]
mod draw_preparation;
#[path = "../../../bevy/aestra-bevy-render/src/gpu/draw_resources.rs"]
mod draw_resources;
#[path = "../../../bevy/aestra-bevy-render/src/gpu/mapped_readback_020.rs"]
mod mapped_readback;
#[path = "../../../bevy/aestra-bevy-render/src/gpu/preparation_context.rs"]
mod preparation_context;
#[path = "../../../bevy/aestra-bevy-render/src/gpu/timestamp_transport.rs"]
mod timestamp_transport;
#[path = "../../../bevy/aestra-bevy-render/src/gpu/trail_compaction.rs"]
mod trail_compaction;
#[path = "../../../bevy/aestra-bevy-render/src/gpu/trail_culling.rs"]
mod trail_culling;
mod trail_native;
// Match the shipping test's readback boundary, with wgpu 30's fallible map view.
fn alpha_sort_test_readback(buffer: &wgpu::Buffer) -> Vec<u8> {
    buffer.slice(..).get_mapped_range().unwrap().to_vec()
}
mod queue_native;
#[path = "../../../bevy/aestra-bevy-render/src/gpu/render.rs"]
mod render;
use aestra_gpu::GpuBlend;
use draw_instance::{GpuDrawInstance, GpuRenderMode};
mod draw_native;
#[path = "../../../bevy/aestra-bevy-render/src/gpu/effect_inputs.rs"]
mod effect_inputs;
#[path = "../../../bevy/aestra-bevy-render/src/gpu/extraction_020.rs"]
mod extraction;
#[path = "../../../bevy/aestra-bevy-render/src/gpu/extraction_cleanup.rs"]
mod extraction_cleanup;
#[path = "../../../bevy/aestra-bevy-render/src/gpu/extraction_systems.rs"]
mod extraction_systems;
#[path = "../../../bevy/aestra-bevy-render/src/material_layout.rs"]
mod material_layout;
#[path = "../../../bevy/aestra-bevy-render/src/gpu/mesh_inputs.rs"]
mod mesh_inputs;
#[path = "../../../bevy/aestra-bevy-render/src/gpu/particle_light_inputs.rs"]
mod particle_light_inputs;
#[path = "../../../bevy/aestra-bevy-render/src/gpu/particle_light_transport.rs"]
mod particle_light_transport;
#[path = "../../../bevy/aestra-bevy-render/src/gpu/pipeline.rs"]
mod pipeline;
mod pipeline_native;
#[path = "../../../bevy/aestra-bevy-render/src/gpu/scene_depth_020.rs"]
mod scene_depth;
mod shader_support;
mod simulation_native;
#[path = "../../../bevy/aestra-bevy-render/src/gpu/simulation_pipeline.rs"]
mod simulation_pipeline;
#[path = "../../../bevy/aestra-bevy-render/src/gpu/sprite_culling.rs"]
mod sprite_culling;
#[path = "../../../bevy/aestra-bevy-render/src/gpu/stage_inputs.rs"]
mod stage_inputs;
#[path = "../../../bevy/aestra-bevy-render/src/gpu/trail_context.rs"]
mod trail_context;
#[path = "../../../bevy/aestra-bevy-render/src/gpu/view_phases.rs"]
mod view_phases;
#[path = "../../../bevy/aestra-bevy-render/src/gpu/wireframe.rs"]
mod wireframe;
#[path = "../../../bevy/aestra-bevy-render/src/gpu/world_sdf.rs"]
mod world_sdf;

#[test]
#[ignore = "Requires a native Vulkan GPU; run explicitly and serially"]
fn native_020_device_capabilities_publish_through_real_extract_schedule() {
    use bevy::{
        prelude::*,
        render::{
            RenderApp,
            renderer::{RenderAdapter, RenderAdapterInfo, RenderDevice},
        },
    };
    use capabilities::ActiveBackend;
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    descriptor.backends = wgpu::Backends::VULKAN;
    let instance = wgpu::Instance::new(descriptor);
    let adapter = pollster::block_on(instance.request_adapter(&Default::default()))
        .expect("native Vulkan GPU required; no qualification skip");
    let info = adapter.get_info();
    assert_ne!(
        info.device_type,
        wgpu::DeviceType::Cpu,
        "hardware GPU required"
    );
    println!("Native capability publication qualification: {info:?}");
    let (device, _queue) = pollster::block_on(adapter.request_device(&Default::default())).unwrap();
    let device = RenderDevice::new(device);
    let adapter = RenderAdapter::new(adapter);
    let info = RenderAdapterInfo::new(info);
    let expected = capability_publication::detect_gpu_capabilities(&device, &adapter, &info);
    assert!(expected.detected);
    assert!(
        expected.compute_pipeline_supported,
        "qualification adapter must support Aestra compute"
    );
    assert!(
        expected.native_render_supported,
        "qualification adapter must support native drawing"
    );
    assert_eq!(expected.adapter_name, info.name);
    assert_eq!(expected.max_buffer_size, device.limits().max_buffer_size);
    let mut app = extraction::test_app();
    app.init_resource::<AestraRenderSettings>()
        .init_resource::<AestraRuntimeStatus>()
        .init_resource::<GpuCapabilities>();
    app.sub_app_mut(RenderApp)
        .insert_resource(device)
        .insert_resource(adapter)
        .insert_resource(info);
    extraction::install_device_publication(&mut app);
    app.update();
    assert_eq!(app.world().resource::<GpuCapabilities>(), &expected);
    assert_eq!(
        app.world().resource::<AestraRuntimeStatus>().active,
        ActiveBackend::Gpu
    );
    let caps_tick = app
        .world()
        .get_resource_ref::<GpuCapabilities>()
        .unwrap()
        .last_changed();
    let status_tick = app
        .world()
        .get_resource_ref::<AestraRuntimeStatus>()
        .unwrap()
        .last_changed();
    app.update();
    assert_eq!(
        app.world()
            .get_resource_ref::<GpuCapabilities>()
            .unwrap()
            .last_changed(),
        caps_tick
    );
    assert_eq!(
        app.world()
            .get_resource_ref::<AestraRuntimeStatus>()
            .unwrap()
            .last_changed(),
        status_tick
    );
    for (mode, expected_backend) in [
        (PresentationMode::CpuReference, ActiveBackend::CpuReference),
        (PresentationMode::GpuReadback, ActiveBackend::GpuReadback),
        (PresentationMode::Gpu, ActiveBackend::Gpu),
    ] {
        app.world_mut()
            .resource_mut::<AestraRenderSettings>()
            .presentation = mode;
        app.update();
        assert_eq!(
            app.world().resource::<AestraRuntimeStatus>().requested,
            mode
        );
        assert_eq!(
            app.world().resource::<AestraRuntimeStatus>().active,
            expected_backend
        );
        assert_eq!(
            app.sub_app(RenderApp)
                .world()
                .resource::<AestraRenderSettings>()
                .presentation,
            mode
        );
        assert_eq!(app.world().resource::<GpuCapabilities>(), &expected);
    }
}
