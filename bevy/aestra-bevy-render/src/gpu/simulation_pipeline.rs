//! Shared analytic pipeline setup and particle dispatch. No replay scheduler or readback.
use super::effect_inputs::GpuEffectBuffers;
use aestra_gpu::{GpuEmitter, GpuGlobals, GpuParticle, WORKGROUP_SIZE};
use bevy::{
    prelude::*,
    render::{
        render_asset::RenderAssets,
        render_resource::{
            BindGroup, BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries,
            CachedComputePipelineId, CommandEncoder, ComputePassDescriptor, ComputePipeline,
            ComputePipelineDescriptor, DownlevelFlags, PipelineCache, ShaderStages,
            binding_types::{storage_buffer, storage_buffer_read_only},
        },
        renderer::{RenderAdapter, RenderDevice},
        storage::GpuShaderBuffer,
    },
};

pub(super) const WESL_SHADER_PATH: &str =
    "embedded://aestra_bevy_render/shaders/aestra_simulation.wesl";

#[derive(Component)]
pub(super) struct GpuBindGroup(pub(super) BindGroup);

#[derive(Resource)]
pub(super) struct SimulationPipeline {
    pub(super) layout: BindGroupLayoutDescriptor,
    pub(super) reset: CachedComputePipelineId,
    pub(super) simulate: CachedComputePipelineId,
    pub(super) link_ribbons: CachedComputePipelineId,
    pub(super) update_trails: CachedComputePipelineId,
    pub(super) paged_trails: [CachedComputePipelineId; 9],
}

pub(super) fn init_pipeline(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    pipeline_cache: Res<PipelineCache>,
    render_device: Res<RenderDevice>,
    adapter: Res<RenderAdapter>,
) {
    let limits = render_device.limits();
    if !adapter
        .get_downlevel_capabilities()
        .flags
        .contains(DownlevelFlags::COMPUTE_SHADERS)
        || limits.max_storage_buffers_per_shader_stage
            < aestra_gpu::SIMULATION_STORAGE_BINDING_COUNT
        || limits.max_bindings_per_bind_group < aestra_gpu::SIMULATION_STORAGE_BINDING_COUNT
        || limits.max_compute_invocations_per_workgroup < WORKGROUP_SIZE
        || limits.max_compute_workgroup_size_x < WORKGROUP_SIZE
    {
        return;
    }
    let layout = BindGroupLayoutDescriptor::new(
        "aestra_gpu_simulation",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::COMPUTE,
            (
                storage_buffer_read_only::<Vec<GpuEmitter>>(false),
                storage_buffer::<Vec<GpuParticle>>(false),
                storage_buffer::<Vec<u32>>(false),
                storage_buffer::<Vec<u32>>(false),
                storage_buffer::<Vec<u32>>(false),
                storage_buffer::<Vec<u32>>(false),
                storage_buffer_read_only::<GpuGlobals>(false),
                storage_buffer::<Vec<u32>>(false),
            ),
        ),
    );
    let shader = asset_server.load(WESL_SHADER_PATH);
    let reset = pipeline_cache.queue_compute_pipeline(ComputePipelineDescriptor {
        label: Some("aestra reset counters".into()),
        layout: vec![layout.clone()],
        shader: shader.clone(),
        entry_point: Some("reset".into()),
        ..default()
    });
    let simulate = pipeline_cache.queue_compute_pipeline(ComputePipelineDescriptor {
        label: Some("aestra simulate particles".into()),
        layout: vec![layout.clone()],
        shader: shader.clone(),
        entry_point: Some("simulate".into()),
        ..default()
    });
    let link_ribbons = pipeline_cache.queue_compute_pipeline(ComputePipelineDescriptor {
        label: Some("aestra link ribbons".into()),
        layout: vec![layout.clone()],
        shader: shader.clone(),
        entry_point: Some("link_ribbons".into()),
        ..default()
    });
    let update_trails = pipeline_cache.queue_compute_pipeline(ComputePipelineDescriptor {
        label: Some("aestra record trail history".into()),
        layout: vec![layout.clone()],
        shader: shader.clone(),
        entry_point: Some("update_trails".into()),
        ..default()
    });
    let paged_trails = aestra_gpu::PAGED_TRAIL_ENTRY_POINTS.map(|entry| {
        pipeline_cache.queue_compute_pipeline(ComputePipelineDescriptor {
            label: Some(format!("aestra {entry}").into()),
            layout: vec![layout.clone()],
            shader: shader.clone(),
            entry_point: Some(entry.into()),
            ..default()
        })
    });
    commands.insert_resource(SimulationPipeline {
        layout,
        reset,
        simulate,
        link_ribbons,
        update_trails,
        paged_trails,
    });
}

pub(super) fn prepare_bind_groups(
    mut commands: Commands,
    pipeline: Option<Res<SimulationPipeline>>,
    render_device: Res<RenderDevice>,
    pipeline_cache: Res<PipelineCache>,
    buffers: Res<RenderAssets<GpuShaderBuffer>>,
    effects: Query<(Entity, &GpuEffectBuffers)>,
) {
    let _span = bevy::log::info_span!("aestra::gpu::bind_groups").entered();
    let Some(pipeline) = pipeline else {
        return;
    };
    for (entity, effect) in &effects {
        let Some(emitters) = buffers.get(&effect.emitters) else {
            continue;
        };
        let Some(particles) = buffers.get(&effect.particles) else {
            continue;
        };
        let Some(alive) = buffers.get(&effect.alive) else {
            continue;
        };
        let Some(dead) = buffers.get(&effect.dead) else {
            continue;
        };
        let Some(counters) = buffers.get(&effect.counters) else {
            continue;
        };
        let Some(indirect) = buffers.get(&effect.indirect) else {
            continue;
        };
        let Some(globals) = buffers.get(&effect.globals) else {
            continue;
        };
        let Some(aux) = buffers.get(&effect.aux) else {
            continue;
        };
        let bind_group = render_device.create_bind_group(
            Some("aestra_gpu_simulation"),
            &pipeline_cache.get_bind_group_layout(&pipeline.layout),
            &BindGroupEntries::sequential((
                emitters.buffer.as_entire_buffer_binding(),
                particles.buffer.as_entire_buffer_binding(),
                alive.buffer.as_entire_buffer_binding(),
                dead.buffer.as_entire_buffer_binding(),
                counters.buffer.as_entire_buffer_binding(),
                indirect.buffer.as_entire_buffer_binding(),
                globals.buffer.as_entire_buffer_binding(),
                aux.buffer.as_entire_buffer_binding(),
            )),
        );
        commands.entity(entity).insert(GpuBindGroup(bind_group));
    }
}

/// Record the production reset/simulate/ribbon portion of one observation.
/// Trail history and stateful scheduling remain with the caller, in their existing order.
pub(super) fn record_particles(
    encoder: &mut CommandEncoder,
    bind_group: &GpuBindGroup,
    effect: &GpuEffectBuffers,
    reset: &ComputePipeline,
    simulate: &ComputePipeline,
    link_ribbons: Option<&ComputePipeline>,
    timestamp_writes: Option<wgpu::ComputePassTimestampWrites<'_>>,
) {
    let mut pass = encoder.begin_compute_pass(&ComputePassDescriptor {
        label: Some("aestra simulation"),
        timestamp_writes,
    });
    pass.set_bind_group(0, &bind_group.0, &[]);
    pass.set_pipeline(reset);
    pass.dispatch_workgroups(1, 1, 1);
    pass.set_pipeline(simulate);
    pass.dispatch_workgroups(effect.workgroups, 1, 1);
    if effect.has_ribbons
        && !effect.has_trails
        && let Some(link_ribbons) = link_ribbons
    {
        pass.set_pipeline(link_ribbons);
        pass.dispatch_workgroups(effect.ribbon_workgroups, 1, 1);
    }
}
