//! Same-frame, per-view GPU culling. CPU visibility deliberately stays conservative:
//! no asynchronous readback participates in correctness or stops history simulation.
use super::{GpuDrawInstance, GpuEffectBuffers, GpuParticle, GpuRenderGlobals, GpuRenderer};
use aestra_gpu::GpuTrailCullParams;
use bevy::{
    app::SubApp,
    prelude::*,
    render::{
        Render, RenderStartup, RenderSystems,
        camera::TemporalJitter,
        render_asset::RenderAssets,
        render_resource::{
            BindGroup, BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, Buffer,
            BufferInitDescriptor, BufferUsages, CachedComputePipelineId, ComputePassDescriptor,
            ComputePipelineDescriptor, PipelineCache, ShaderStages,
            binding_types::{storage_buffer, storage_buffer_read_only, uniform_buffer},
            encase::UniformBuffer,
        },
        renderer::{RenderContext, RenderDevice, RenderGraph, RenderGraphSystems, RenderQueue},
        storage::GpuShaderBuffer,
        view::ExtractedView,
    },
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Resource)]
struct TrailCullPipeline {
    layout: BindGroupLayoutDescriptor,
    pipeline: CachedComputePipelineId,
}

struct Entry {
    owner: Entity,
    params: Buffer,
    indirect: Buffer,
    bindings: BindGroup,
    compact_bindings: BindGroup,
}

#[derive(Resource, Default)]
pub(super) struct TrailCulling {
    entries: BTreeMap<(Entity, Entity), Entry>,
    dispatched: bool,
}

impl TrailCulling {
    pub(super) fn indirect(&self, view: Entity, draw: Entity) -> Option<&Buffer> {
        self.dispatched
            .then(|| self.entries.get(&(view, draw)))
            .flatten()
            .map(|entry| &entry.indirect)
    }
}

pub(super) fn install(app: &mut SubApp) {
    app.init_resource::<TrailCulling>()
        .add_systems(RenderStartup, init_pipeline)
        .add_systems(
            Render,
            prepare
                .in_set(RenderSystems::PrepareBindGroups)
                .after(super::trail_compaction::TrailCompactionSystems::Prepare),
        )
        .add_systems(
            RenderGraph,
            cull.after(super::run_simulation)
                .after(super::trail_compaction::TrailCompactionSystems::Compact)
                .before(RenderGraphSystems::Render),
        );
}

fn init_pipeline(mut commands: Commands, assets: Res<AssetServer>, cache: Res<PipelineCache>) {
    let layout = BindGroupLayoutDescriptor::new(
        "aestra trail culling",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::COMPUTE,
            (
                storage_buffer_read_only::<Vec<GpuParticle>>(false),
                storage_buffer_read_only::<Vec<GpuRenderer>>(false),
                uniform_buffer::<GpuTrailCullParams>(false),
                storage_buffer::<Vec<u32>>(false),
                storage_buffer_read_only::<GpuRenderGlobals>(false),
                storage_buffer_read_only::<Vec<u32>>(false),
                storage_buffer_read_only::<Vec<u32>>(false),
            ),
        ),
    );
    let pipeline = cache.queue_compute_pipeline(ComputePipelineDescriptor {
        label: Some("aestra cull world trail history".into()),
        layout: vec![layout.clone()],
        shader: assets.load("embedded://aestra_bevy_render/shaders/aestra_trail_cull.wesl"),
        entry_point: Some("cull_trail".into()),
        ..default()
    });
    commands.insert_resource(TrailCullPipeline { layout, pipeline });
}

#[allow(clippy::too_many_arguments)]
fn prepare(
    views: Query<(Entity, &ExtractedView, Option<&TemporalJitter>)>,
    draws: Query<(Entity, &GpuDrawInstance)>,
    effects: Query<&GpuEffectBuffers>,
    buffers: Res<RenderAssets<GpuShaderBuffer>>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    pipeline: Res<TrailCullPipeline>,
    cache: Res<PipelineCache>,
    mut culling: ResMut<TrailCulling>,
    compaction: Res<super::trail_compaction::TrailCompaction>,
) {
    culling.dispatched = false;
    let mut retained = BTreeSet::new();
    let epochs: BTreeMap<_, _> = effects
        .iter()
        .map(|effect| (effect.render_globals.id(), effect.history_epoch))
        .collect();
    for (view_entity, view, jitter) in &views {
        // Until we consume the exact jittered view uniform, opt out for TAA views.
        if jitter.is_some() {
            continue;
        }
        let matrix = view
            .clip_from_world
            .unwrap_or_else(|| view.clip_from_view * view.world_from_view.to_matrix().inverse());
        if !matrix.is_finite() || !matrix.determinant().is_finite() || matrix.determinant() == 0.0 {
            continue;
        }
        for (draw_entity, draw) in &draws {
            let Some(instance_count) = draw.trail_instances else {
                continue;
            };
            let Some(compact) = compaction.entries.get(&draw_entity) else {
                continue;
            };
            // Future vertex displacement cannot be bounded by trail positions alone.
            if draw
                .semantic_material
                .as_ref()
                .is_some_and(|binding| binding.program.has_vertex_offset)
            {
                continue;
            }
            let (Some(particles), Some(renderers), Some(globals), Some(aux), Some(&epoch)) = (
                buffers.get(&draw.particles),
                buffers.get(&draw.renderers),
                buffers.get(&draw.render_globals),
                buffers.get(&draw.aux),
                epochs.get(&draw.render_globals.id()),
            ) else {
                continue;
            };
            let params = GpuTrailCullParams {
                clip_from_world: matrix,
                renderer_index: draw.renderer_order,
                instance_count,
                epoch,
                _padding: 0,
            };
            let mut encoded = UniformBuffer::new(Vec::new());
            encoded.write(&params).expect("trail culling uniform ABI");
            let key = (view_entity, draw_entity);
            let (params_buffer, indirect) = if let Some(entry) = culling.entries.get(&key) {
                queue.write_buffer(&entry.params, 0, encoded.as_ref());
                (entry.params.clone(), entry.indirect.clone())
            } else {
                (
                    device.create_buffer_with_data(&BufferInitDescriptor {
                        label: Some("aestra trail culling view"),
                        contents: encoded.as_ref(),
                        usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
                    }),
                    device.create_buffer_with_data(&BufferInitDescriptor {
                        label: Some("aestra culled trail draw"),
                        contents: &[0; 16],
                        usage: BufferUsages::STORAGE
                            | BufferUsages::INDIRECT
                            | BufferUsages::COPY_SRC,
                    }),
                )
            };
            let bindings = device.create_bind_group(
                Some("aestra trail culling"),
                &cache.get_bind_group_layout(&pipeline.layout),
                &BindGroupEntries::sequential((
                    particles.buffer.as_entire_buffer_binding(),
                    renderers.buffer.as_entire_buffer_binding(),
                    params_buffer.as_entire_buffer_binding(),
                    indirect.as_entire_buffer_binding(),
                    globals.buffer.as_entire_buffer_binding(),
                    aux.buffer.as_entire_buffer_binding(),
                    compact.fallback.as_entire_buffer_binding(),
                )),
            );
            let compact_bindings = device.create_bind_group(
                Some("aestra compact trail culling"),
                &cache.get_bind_group_layout(&pipeline.layout),
                &BindGroupEntries::sequential((
                    particles.buffer.as_entire_buffer_binding(),
                    renderers.buffer.as_entire_buffer_binding(),
                    params_buffer.as_entire_buffer_binding(),
                    indirect.as_entire_buffer_binding(),
                    globals.buffer.as_entire_buffer_binding(),
                    aux.buffer.as_entire_buffer_binding(),
                    compact.output.as_entire_buffer_binding(),
                )),
            );
            culling.entries.insert(
                key,
                Entry {
                    owner: draw.owner,
                    params: params_buffer,
                    indirect,
                    bindings,
                    compact_bindings,
                },
            );
            retained.insert(key);
        }
    }
    // Removed draws/cameras, invalid matrices and missing buffers must not reuse old decisions.
    culling.entries.retain(|key, _| retained.contains(key));
}

fn cull(
    mut context: RenderContext,
    cache: Res<PipelineCache>,
    pipeline: Res<TrailCullPipeline>,
    mut culling: ResMut<TrailCulling>,
    compaction: Res<super::trail_compaction::TrailCompaction>,
    timing: super::preparation_timing::TimingContext,
    mut timer: Local<super::simulation_timing::SimulationTimer>,
) {
    let Some(pipeline) = cache.get_compute_pipeline(pipeline.pipeline) else {
        return;
    };
    let mut batch = timing.begin(&mut timer);
    let mut by_owner: BTreeMap<Entity, Vec<&Entry>> = BTreeMap::new();
    for entry in culling.entries.values() {
        by_owner.entry(entry.owner).or_default().push(entry);
    }
    for (owner, entries) in by_owner {
        let index = batch.as_mut().and_then(|b| timing.owner(b, owner));
        let mut pass = context
            .command_encoder()
            .begin_compute_pass(&ComputePassDescriptor {
                label: Some("aestra trail visibility"),
                timestamp_writes: index.and_then(|i| batch.as_ref()?.writes(i, true, true)),
            });
        pass.set_pipeline(pipeline);
        for entry in entries {
            pass.set_bind_group(
                0,
                if compaction.dispatched {
                    &entry.compact_bindings
                } else {
                    &entry.bindings
                },
                &[],
            );
            pass.dispatch_workgroups(1, 1, 1);
        }
    }
    if let Some(batch) = batch {
        batch.finish(context.command_encoder(), timing.mailboxes.culling.clone());
    }
    culling.dispatched = true;
}
