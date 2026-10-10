//! Opt-in per-view sprite permutations: page sort + parallel binary-rank merges.
//! Capacity-scaled O(N log² N) comparisons, O(N) storage, no particle readback or live-count cap.
use super::draw_instance::{GpuDrawInstance, GpuRenderMode};
use crate::{AestraRenderSettings, TransparentOrderMode};
use aestra_gpu::{GpuBlend, GpuParticle, GpuRenderGlobals, GpuRenderParams};
use bevy::render::render_resource::{
    ShaderType,
    binding_types::uniform_buffer,
    encase::{StorageBuffer, UniformBuffer},
};
use bevy::render::{diagnostic::RecordDiagnostics, renderer::RenderQueue, view::ExtractedView};
use bevy::{
    app::SubApp,
    prelude::*,
    render::{
        Render, RenderStartup, RenderSystems,
        render_asset::RenderAssets,
        render_resource::{
            BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries,
            BufferInitDescriptor, BufferUsages, CachedComputePipelineId, ComputePassDescriptor,
            ComputePipelineDescriptor, PipelineCache, ShaderStages,
            binding_types::{storage_buffer, storage_buffer_read_only},
        },
        renderer::{RenderContext, RenderDevice, RenderGraph, RenderGraphSystems},
        storage::GpuShaderBuffer,
    },
};
use std::sync::Arc;

pub(super) use super::draw_resources::PrepareAlphaSort as Prepare;
#[derive(Resource)]
struct Pipeline {
    layout: BindGroupLayoutDescriptor,
    passes: [CachedComputePipelineId; 3],
}
#[derive(ShaderType)]
struct Params {
    view_from_world: Mat4,
    range: UVec4,
}
pub(super) use super::draw_resources::AlphaSort;
use super::draw_resources::AlphaSortEntry as Entry;
/// CPU ownership/work metadata only; no GPU particle or sorted-index readback.
#[derive(Resource, Clone, Default)]
pub struct GpuAlphaSortStatistics(Arc<std::sync::Mutex<AlphaSortSnapshot>>);
#[derive(Clone, Copy, Debug, Default)]
pub struct AlphaSortSnapshot {
    /// Visible view/draw pairs, not live particles.
    pub pairs: usize,
    pub owned_buffer_bytes: u64,
    pub allocated_buffers_this_frame: usize,
    pub dispatched: bool,
}
impl GpuAlphaSortStatistics {
    pub fn snapshot(&self) -> AlphaSortSnapshot {
        *self.0.lock().unwrap()
    }
}
pub(super) fn install(app: &mut SubApp) {
    app.init_resource::<AlphaSort>()
        .add_systems(RenderStartup, init)
        .add_systems(
            Render,
            prepare
                .in_set(RenderSystems::PrepareBindGroups)
                .in_set(Prepare),
        )
        .add_systems(
            RenderGraph,
            sort.in_set(super::draw_resources::SortAlpha)
                .after(super::draw_resources::SimulateEffects)
                .after(RenderGraphSystems::Begin)
                .before(RenderGraphSystems::Render),
        );
}
fn init(mut commands: Commands, assets: Res<AssetServer>, cache: Res<PipelineCache>) {
    let layout = BindGroupLayoutDescriptor::new(
        "aestra alpha sprite sort",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::COMPUTE,
            (
                storage_buffer_read_only::<Vec<GpuParticle>>(false),
                storage_buffer_read_only::<Vec<u32>>(false),
                storage_buffer_read_only::<Vec<u32>>(false),
                storage_buffer_read_only::<GpuRenderGlobals>(false),
                uniform_buffer::<Params>(false),
                storage_buffer_read_only::<Vec<UVec4>>(false),
                storage_buffer::<Vec<UVec4>>(false),
                storage_buffer::<Vec<u32>>(false),
            ),
        ),
    );
    let shader = assets.load("embedded://aestra_bevy_render/shaders/alpha_sort.wgsl");
    let passes = ["classify", "merge", "finish"].map(|name| {
        cache.queue_compute_pipeline(ComputePipelineDescriptor {
            label: Some(format!("aestra alpha {name}").into()),
            layout: vec![layout.clone()],
            shader: shader.clone(),
            entry_point: Some(name.into()),
            ..default()
        })
    });
    commands.insert_resource(Pipeline { layout, passes });
}
fn stages(capacity: u32) -> Option<(u32, Vec<u32>)> {
    let count = capacity.max(256).checked_next_power_of_two()?;
    let mut widths = vec![0];
    let mut width = 256;
    while width < count {
        widths.push(width);
        width *= 2;
    }
    widths.push(count); // finish reads the last complete run
    Some((count, widths))
}
fn eligible(draw: &GpuDrawInstance) -> bool {
    draw.renderer_kind <= 1
        && draw.blend == GpuBlend::Alpha
        && draw.render_mode == GpuRenderMode::Rendered
        && draw.sort_range.y > 0
}
#[allow(clippy::too_many_arguments)]
fn prepare(
    settings: Res<AestraRenderSettings>,
    draws: Query<(Entity, &GpuDrawInstance)>,
    views: Query<
        (
            Entity,
            &ExtractedView,
            &bevy::render::view::RenderVisibleEntities,
        ),
        With<Camera3d>,
    >,
    buffers: Res<RenderAssets<GpuShaderBuffer>>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    cache: Res<PipelineCache>,
    pipeline: Res<Pipeline>,
    mut state: ResMut<AlphaSort>,
    statistics: Res<GpuAlphaSortStatistics>,
) {
    state.dispatched = false;
    let mut retained = std::collections::BTreeSet::new();
    let mut allocations = 0;
    if settings.transparent_order == TransparentOrderMode::DepthBackToFront {
        for (view_entity, view, visible) in &views {
            for (draw_entity, _) in super::render::visible_gpu_draws(visible) {
                let Ok((_, draw)) = draws.get(draw_entity) else {
                    continue;
                };
                if !eligible(draw) {
                    continue;
                }
                let Some((count, widths)) = stages(draw.sort_range.y) else {
                    continue;
                };
                let bytes = u64::from(count) * 16;
                // Respect device limits rather than truncate/sort only a prefix. Unsupported requests fail closed.
                if bytes > device.limits().max_storage_buffer_binding_size
                    || bytes > device.limits().max_buffer_size
                {
                    panic!("alpha sprite sort exceeds device storage limits");
                }
                let (Some(particles), Some(alive), Some(indirect), Some(globals)) = (
                    buffers.get(&draw.particles),
                    buffers.get(&draw.alive),
                    buffers.get(&draw.indirect),
                    buffers.get(&draw.render_globals),
                ) else {
                    continue;
                };
                let key = (view_entity, draw_entity);
                if state.entries.get(&key).is_some_and(|e| e.count != count) {
                    state.entries.remove(&key);
                }
                if let std::collections::btree_map::Entry::Vacant(slot) = state.entries.entry(key) {
                    allocations += widths.len() + 4;
                    let allocate = |label, size| {
                        device.create_buffer(&bevy::render::render_resource::BufferDescriptor {
                            label: Some(label),
                            size,
                            usage: BufferUsages::STORAGE,
                            mapped_at_creation: false,
                        })
                    };
                    let runs = [
                        allocate("aestra alpha sort A", bytes),
                        allocate("aestra alpha sort B", bytes),
                    ];
                    let indices = allocate("aestra alpha sorted indices", u64::from(count) * 4);
                    let uniforms = widths
                        .iter()
                        .map(|_| {
                            device.create_buffer(&bevy::render::render_resource::BufferDescriptor {
                                label: Some("aestra alpha sort parameters"),
                                size: 80,
                                usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
                                mapped_at_creation: false,
                            })
                        })
                        .collect::<Vec<_>>();
                    let mut encoded = StorageBuffer::new(Vec::new());
                    encoded
                        .write(&GpuRenderParams {
                            renderer_index: draw.renderer_order,
                            alive_offset: 0,
                            ..default()
                        })
                        .unwrap();
                    let render_params = device.create_buffer_with_data(&BufferInitDescriptor {
                        label: Some("aestra alpha vertex parameters"),
                        contents: encoded.as_ref(),
                        usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
                    });
                    slot.insert(Entry {
                        count,
                        runs,
                        indices,
                        render_params,
                        uniforms,
                        bindings: vec![],
                    });
                }
                let entry = state.entries.get_mut(&key).unwrap();
                // Draw/source buffers may be replaced on restart or material rebinding: refresh handles.
                entry.bindings.clear();
                let mut vertex_params = StorageBuffer::new(Vec::new());
                vertex_params
                    .write(&GpuRenderParams {
                        renderer_index: draw.renderer_order,
                        ..default()
                    })
                    .unwrap();
                queue.write_buffer(&entry.render_params, 0, vertex_params.as_ref());
                for (stage, width) in widths.iter().enumerate() {
                    let params = Params {
                        view_from_world: view.world_from_view.to_matrix().inverse(),
                        range: UVec4::new(
                            draw.sort_range.x,
                            draw.sort_range.y,
                            draw.emitter_index,
                            *width,
                        ),
                    };
                    let mut encoded = UniformBuffer::new(Vec::new());
                    encoded.write(&params).unwrap();
                    queue.write_buffer(&entry.uniforms[stage], 0, encoded.as_ref());
                    let source = stage % 2;
                    let destination = source ^ 1;
                    entry.bindings.push(device.create_bind_group(
                        Some("aestra alpha sprite sort"),
                        &cache.get_bind_group_layout(&pipeline.layout),
                        &BindGroupEntries::sequential((
                            particles.buffer.as_entire_buffer_binding(),
                            alive.buffer.as_entire_buffer_binding(),
                            indirect.buffer.as_entire_buffer_binding(),
                            globals.buffer.as_entire_buffer_binding(),
                            entry.uniforms[stage].as_entire_buffer_binding(),
                            entry.runs[source].as_entire_buffer_binding(),
                            entry.runs[destination].as_entire_buffer_binding(),
                            entry.indices.as_entire_buffer_binding(),
                        )),
                    ));
                }
                retained.insert(key);
            }
        }
    }
    state.entries.retain(|key, _| retained.contains(key));
    *statistics.0.lock().unwrap() = AlphaSortSnapshot {
        pairs: state.entries.len(),
        owned_buffer_bytes: state
            .entries
            .values()
            .map(|e| {
                e.runs
                    .iter()
                    .chain([&e.indices, &e.render_params])
                    .chain(e.uniforms.iter())
                    .map(|buffer| buffer.size())
                    .sum::<u64>()
            })
            .sum(),
        allocated_buffers_this_frame: allocations,
        dispatched: false,
    };
}
fn sort(
    mut context: RenderContext,
    cache: Res<PipelineCache>,
    pipeline: Res<Pipeline>,
    mut state: ResMut<AlphaSort>,
    statistics: Res<GpuAlphaSortStatistics>,
) {
    let [Some(classify), Some(merge), Some(finish)] =
        pipeline.passes.map(|id| cache.get_compute_pipeline(id))
    else {
        return;
    };
    if state.entries.is_empty() {
        return;
    }
    let diagnostics = context.diagnostic_recorder();
    let diagnostics = diagnostics.as_deref();
    let span = diagnostics.time_span(context.command_encoder(), "aestra::gpu::alpha_sort");
    for entry in state.entries.values() {
        for (stage, binding) in entry.bindings.iter().enumerate() {
            let last = stage + 1 == entry.bindings.len();
            let mut pass = context
                .command_encoder()
                .begin_compute_pass(&ComputePassDescriptor {
                    label: Some("aestra alpha sprite sort"),
                    timestamp_writes: None,
                });
            pass.set_pipeline(if stage == 0 {
                classify
            } else if last {
                finish
            } else {
                merge
            });
            pass.set_bind_group(0, binding, &[]);
            // Flatten dispatch to stay below device dimension limits for large pools.
            let groups = entry.count.div_ceil(if stage == 0 { 256 } else { 64 });
            pass.dispatch_workgroups(groups.min(65535), groups.div_ceil(65535), 1);
        }
    }
    span.end(context.command_encoder());
    state.dispatched = true;
    statistics.0.lock().unwrap().dispatched = true;
}
#[cfg(test)]
mod tests {
    use super::*;
    include!("alpha_sort_tests.rs");
    #[test]
    fn plan_has_no_capture_limit_and_merge_parity_is_total() {
        for capacity in [1, 48, 256, 257, 4096, 4097, 65537, 1_000_000] {
            let (count, widths) = stages(capacity).unwrap();
            assert!(count >= capacity && count.is_power_of_two());
            assert_eq!(widths[0], 0);
            assert_eq!(*widths.last().unwrap(), count);
            for pair in widths[1..].windows(2) {
                assert_eq!(pair[1], pair[0] * 2);
            }
        }
        assert!(stages(u32::MAX).is_none());
    }
    #[test]
    fn shader_validates_all_entrypoints() {
        let module = naga::front::wgsl::parse_str(include_str!("alpha_sort.wgsl")).unwrap();
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::default(),
        )
        .validate(&module)
        .unwrap();
        assert_eq!(module.entry_points.len(), 3);
    }
}
