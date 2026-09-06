//! Stable per-renderer compaction, shared by every view and rebuilt after simulation/replay.
use super::*;
use bevy::render::render_resource::{Buffer, encase::UniformBuffer};

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(super) enum TrailCompactionSystems {
    Prepare,
    Compact,
}

#[derive(Resource)]
struct Pipeline {
    layout: BindGroupLayoutDescriptor,
    passes: [CachedComputePipelineId; 3],
}

pub(super) struct Entry {
    owner: Entity,
    pub output: Buffer,
    pub fallback: Buffer,
    pub render_params: Buffer,
    scratch: Buffer,
    params: Buffer,
    bindings: BindGroup,
    count: u32,
    owners: u32,
    renderer: u32,
}

#[derive(Resource, Default)]
pub(super) struct TrailCompaction {
    pub entries: BTreeMap<Entity, Entry>,
    pub dispatched: bool,
}

impl TrailCompaction {
    pub fn output(&self, draw: Entity) -> Option<&Buffer> {
        self.dispatched
            .then(|| self.entries.get(&draw))
            .flatten()
            .map(|e| &e.output)
    }
}

pub(super) fn install(app: &mut SubApp) {
    app.init_resource::<TrailCompaction>()
        .add_systems(RenderStartup, init)
        .add_systems(
            Render,
            prepare
                .in_set(RenderSystems::PrepareBindGroups)
                .in_set(TrailCompactionSystems::Prepare),
        )
        .add_systems(
            RenderGraph,
            compact
                .after(super::run_simulation)
                .in_set(TrailCompactionSystems::Compact)
                .before(RenderGraphSystems::Render),
        );
}

fn init(mut commands: Commands, assets: Res<AssetServer>, cache: Res<PipelineCache>) {
    let layout = BindGroupLayoutDescriptor::new(
        "aestra trail compaction",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::COMPUTE,
            (
                storage_buffer_read_only::<Vec<GpuParticle>>(false),
                storage_buffer_read_only::<Vec<GpuRenderer>>(false),
                storage_buffer_read_only::<GpuRenderGlobals>(false),
                storage_buffer_read_only::<Vec<u32>>(false),
                bevy::render::render_resource::binding_types::uniform_buffer::<UVec4>(false),
                storage_buffer::<Vec<u32>>(false),
                storage_buffer::<Vec<u32>>(false),
            ),
        ),
    );
    let passes = ["classify_trail", "prefix_trail", "scatter_trail"].map(|entry| {
        cache.queue_compute_pipeline(ComputePipelineDescriptor {
            label: Some(format!("aestra {entry}").into()),
            layout: vec![layout.clone()],
            shader: assets.load("embedded://aestra_bevy_render/shaders/aestra_trail_compact.wesl"),
            entry_point: Some(entry.into()),
            ..default()
        })
    });
    commands.insert_resource(Pipeline { layout, passes });
}

fn prepare(
    draws: Query<(Entity, &GpuDrawInstance)>,
    buffers: Res<RenderAssets<GpuShaderBuffer>>,
    device: Res<RenderDevice>,
    cache: Res<PipelineCache>,
    pipeline: Res<Pipeline>,
    mut state: ResMut<TrailCompaction>,
) {
    state.dispatched = false;
    let mut retained = std::collections::BTreeSet::new();
    for (entity, draw) in &draws {
        let Some(count) = draw.trail_instances else {
            continue;
        };
        let owners = draw.trail_owners;
        if owners == 0 || count == 0 {
            continue;
        }
        let (Some(particles), Some(renderers), Some(globals), Some(aux)) = (
            buffers.get(&draw.particles),
            buffers.get(&draw.renderers),
            buffers.get(&draw.render_globals),
            buffers.get(&draw.aux),
        ) else {
            continue;
        };
        if state.entries.get(&entity).is_some_and(|e| {
            e.count != count || e.owners != owners || e.renderer != draw.renderer_order
        }) {
            state.entries.remove(&entity);
        }
        let (output, fallback, scratch, params, render_params) = if let Some(e) =
            state.entries.get(&entity)
        {
            (
                e.output.clone(),
                e.fallback.clone(),
                e.scratch.clone(),
                e.params.clone(),
                e.render_params.clone(),
            )
        } else {
            let output = device.create_buffer(&bevy::render::render_resource::BufferDescriptor {
                label: Some("aestra compact trail commands and indices"),
                size: u64::from(count + 4) * 4,
                usage: BufferUsages::STORAGE | BufferUsages::INDIRECT | BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            });
            let fallback = device.create_buffer_with_data(&BufferInitDescriptor {
                label: Some("aestra full trail range"),
                contents: &[4u32, count, 0, 0]
                    .into_iter()
                    .flat_map(u32::to_le_bytes)
                    .collect::<Vec<_>>(),
                usage: BufferUsages::STORAGE,
            });
            let scratch = device.create_buffer(&bevy::render::render_resource::BufferDescriptor {
                label: Some("aestra trail local ranks and owner offsets"),
                size: u64::from(count + owners) * 4,
                usage: BufferUsages::STORAGE,
                mapped_at_creation: false,
            });
            let mut encoded = UniformBuffer::new(Vec::new());
            encoded
                .write(&UVec4::new(
                    draw.renderer_order,
                    count,
                    owners,
                    count / owners,
                ))
                .unwrap();
            let params = device.create_buffer_with_data(&BufferInitDescriptor {
                label: Some("aestra trail compaction parameters"),
                contents: encoded.as_ref(),
                usage: BufferUsages::UNIFORM,
            });
            let mut encoded = bevy::render::render_resource::encase::StorageBuffer::new(Vec::new());
            encoded
                .write(&GpuRenderParams {
                    renderer_index: draw.renderer_order,
                    _padding: UVec2::new(1, 0),
                    ..default()
                })
                .unwrap();
            let render_params = device.create_buffer_with_data(&BufferInitDescriptor {
                label: Some("aestra compact trail vertex parameters"),
                contents: encoded.as_ref(),
                usage: BufferUsages::STORAGE,
            });
            (output, fallback, scratch, params, render_params)
        };
        let bindings = device.create_bind_group(
            Some("aestra trail compaction"),
            &cache.get_bind_group_layout(&pipeline.layout),
            &BindGroupEntries::sequential((
                particles.buffer.as_entire_buffer_binding(),
                renderers.buffer.as_entire_buffer_binding(),
                globals.buffer.as_entire_buffer_binding(),
                aux.buffer.as_entire_buffer_binding(),
                params.as_entire_buffer_binding(),
                scratch.as_entire_buffer_binding(),
                output.as_entire_buffer_binding(),
            )),
        );
        state.entries.insert(
            entity,
            Entry {
                owner: draw.owner,
                output,
                fallback,
                scratch,
                params,
                render_params,
                bindings,
                count,
                owners,
                renderer: draw.renderer_order,
            },
        );
        retained.insert(entity);
    }
    state.entries.retain(|entity, _| retained.contains(entity));
}

fn compact(
    mut context: RenderContext,
    cache: Res<PipelineCache>,
    pipeline: Res<Pipeline>,
    mut state: ResMut<TrailCompaction>,
    timing: super::preparation_timing::TimingContext,
    mut timer: Local<super::simulation_timing::SimulationTimer>,
) {
    let [Some(classify), Some(prefix), Some(scatter)] =
        pipeline.passes.map(|id| cache.get_compute_pipeline(id))
    else {
        return;
    };
    let mut batch = timing.begin(&mut timer);
    let mut by_owner: BTreeMap<Entity, Vec<&Entry>> = BTreeMap::new();
    for entry in state.entries.values() {
        by_owner.entry(entry.owner).or_default().push(entry);
    }
    for (owner, entries) in by_owner {
        let index = batch.as_mut().and_then(|b| timing.owner(b, owner));
        for (stage, pipeline) in [classify, prefix, scatter].into_iter().enumerate() {
            // Separate passes provide storage visibility; each owner has one complete timing window.
            let mut pass = context
                .command_encoder()
                .begin_compute_pass(&ComputePassDescriptor {
                    label: Some("aestra compact trail segments"),
                    timestamp_writes: index
                        .and_then(|i| batch.as_ref()?.writes(i, stage == 0, stage == 2)),
                });
            pass.set_pipeline(pipeline);
            for entry in &entries {
                pass.set_bind_group(0, &entry.bindings, &[]);
                pass.dispatch_workgroups(
                    match stage {
                        0 => entry.owners.div_ceil(64),
                        1 => 1,
                        _ => entry.count.div_ceil(64),
                    },
                    1,
                    1,
                );
            }
        }
    }
    if let Some(batch) = batch {
        batch.finish(
            context.command_encoder(),
            timing.mailboxes.compaction.clone(),
        );
    }
    state.dispatched = true;
}
