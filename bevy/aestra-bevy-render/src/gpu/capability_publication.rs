//! Concrete device discovery and render-to-main backend publication.
use super::extraction::MainWorld;
use crate::{
    AestraRenderSettings, AestraRuntimeStatus, GpuCapabilities, capabilities::select_backend,
};
use aestra_gpu::{GpuParticle, WORKGROUP_SIZE};
use bevy::{
    prelude::*,
    render::{
        render_resource::DownlevelFlags,
        renderer::{RenderAdapter, RenderAdapterInfo, RenderDevice},
    },
};

pub(super) fn publish_gpu_capabilities(
    render_device: Res<RenderDevice>,
    adapter: Res<RenderAdapter>,
    adapter_info: Res<RenderAdapterInfo>,
    mut main_world: ResMut<MainWorld>,
) {
    let capabilities = detect_gpu_capabilities(&render_device, &adapter, &adapter_info);
    publish_detected_capabilities(&mut main_world, capabilities);
}

/// Publish only a changed decision; unchanged frames must not dirty host resources.
pub(super) fn publish_detected_capabilities(
    main_world: &mut World,
    capabilities: GpuCapabilities,
) -> bool {
    let requested = main_world.resource::<AestraRenderSettings>().presentation;
    let status = select_backend(requested, &capabilities);
    let changed = main_world.resource::<AestraRuntimeStatus>() != &status
        || main_world.resource::<GpuCapabilities>() != &capabilities;
    if changed {
        info!(
            "Aestra backend: {} on {} ({}); {}",
            status.active, capabilities.adapter_name, capabilities.backend, status.reason
        );
        main_world.insert_resource(capabilities);
        main_world.insert_resource(status);
    }
    changed
}

pub(super) fn detect_gpu_capabilities(
    render_device: &RenderDevice,
    adapter: &RenderAdapter,
    adapter_info: &RenderAdapterInfo,
) -> GpuCapabilities {
    let limits = render_device.limits();
    let flags = adapter.get_downlevel_capabilities().flags;
    let compute_shaders = flags.contains(DownlevelFlags::COMPUTE_SHADERS);
    let indirect_execution = flags.contains(DownlevelFlags::INDIRECT_EXECUTION);
    let vertex_storage = flags.contains(DownlevelFlags::VERTEX_STORAGE);
    let binding_capacity = limits
        .max_storage_buffer_binding_size
        .min(limits.max_buffer_size)
        / std::mem::size_of::<GpuParticle>() as u64;
    let dispatch_capacity =
        u64::from(limits.max_compute_workgroups_per_dimension) * u64::from(WORKGROUP_SIZE);
    let max_particles = binding_capacity
        .min(dispatch_capacity)
        .min(u64::from(u32::MAX)) as u32;

    let mut limitations = Vec::new();
    if !compute_shaders {
        limitations.push("compute shaders are unavailable".into());
    }
    if limits.max_compute_invocations_per_workgroup < WORKGROUP_SIZE
        || limits.max_compute_workgroup_size_x < WORKGROUP_SIZE
    {
        limitations.push(format!(
            "compute workgroups cannot run {WORKGROUP_SIZE} invocations"
        ));
    }
    if limits.max_storage_buffers_per_shader_stage < aestra_gpu::SIMULATION_STORAGE_BINDING_COUNT {
        limitations.push(format!(
            "{} storage buffers per shader stage are available; {} are required",
            limits.max_storage_buffers_per_shader_stage,
            aestra_gpu::SIMULATION_STORAGE_BINDING_COUNT
        ));
    }
    if limits.max_bindings_per_bind_group < aestra_gpu::SIMULATION_STORAGE_BINDING_COUNT {
        limitations.push(format!(
            "{} bindings per group are available; {} are required",
            limits.max_bindings_per_bind_group,
            aestra_gpu::SIMULATION_STORAGE_BINDING_COUNT
        ));
    }
    if max_particles == 0 {
        limitations.push("storage or dispatch limits allow no particles".into());
    }
    let compute_pipeline_supported = compute_shaders
        && limits.max_compute_invocations_per_workgroup >= WORKGROUP_SIZE
        && limits.max_compute_workgroup_size_x >= WORKGROUP_SIZE
        && limits.max_storage_buffers_per_shader_stage
            >= aestra_gpu::SIMULATION_STORAGE_BINDING_COUNT
        && limits.max_bindings_per_bind_group >= aestra_gpu::SIMULATION_STORAGE_BINDING_COUNT
        && max_particles > 0;
    if !indirect_execution {
        limitations.push("indirect execution is unavailable".into());
    }
    if !vertex_storage {
        limitations.push("vertex-stage storage buffers are unavailable".into());
    }
    if limits.max_bind_groups < 2 {
        limitations.push(format!(
            "{} bind group is available; native rendering requires 2",
            limits.max_bind_groups
        ));
    }
    let native_render_supported = compute_pipeline_supported
        && indirect_execution
        && vertex_storage
        && limits.max_bind_groups >= 2;

    GpuCapabilities {
        detected: true,
        adapter_name: adapter_info.name.clone(),
        backend: format!("{:?}", adapter_info.backend),
        device_type: format!("{:?}", adapter_info.device_type),
        driver: if adapter_info.driver.is_empty() {
            "unknown".into()
        } else {
            adapter_info.driver.clone()
        },
        compute_shaders,
        indirect_execution,
        vertex_storage,
        compute_pipeline_supported,
        native_render_supported,
        max_bind_groups: limits.max_bind_groups,
        max_bindings_per_bind_group: limits.max_bindings_per_bind_group,
        max_storage_buffers_per_shader_stage: limits.max_storage_buffers_per_shader_stage,
        max_storage_buffer_binding_size: limits.max_storage_buffer_binding_size,
        max_sampled_textures_per_shader_stage: limits.max_sampled_textures_per_shader_stage,
        max_samplers_per_shader_stage: limits.max_samplers_per_shader_stage,
        max_uniform_buffer_binding_size: limits.max_uniform_buffer_binding_size,
        max_buffer_size: limits.max_buffer_size,
        max_compute_workgroups_per_dimension: limits.max_compute_workgroups_per_dimension,
        max_compute_invocations_per_workgroup: limits.max_compute_invocations_per_workgroup,
        max_compute_workgroup_size_x: limits.max_compute_workgroup_size_x,
        max_particles,
        limitations,
    }
}
