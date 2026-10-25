//! Optional single-full-target-camera diagnostic, before Bevy clears its OIT lists.
use super::*;
use bevy::{
    camera::{MainPassResolutionOverride, NormalizedRenderTarget, RenderTarget},
    core_pipeline::{
        core_3d::main_transparent_pass_3d,
        oit::{
            OitBuffers,
            resolve::{OitResolvePipelineId, node::oit_resolve},
        },
        schedule::{Core3d, Core3dSystems},
    },
    render::{
        render_resource::PipelineCache,
        renderer::{RenderContext, ViewQuery},
    },
    window::PrimaryWindow,
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

/// Latest completed diagnostic, not a current-frame correctness guarantee.
/// Counts include all fragments collected by Bevy OIT, not just Aestra draws.
#[derive(Clone, Copy, Debug)]
pub struct ExperimentalOitDiagnosticSample {
    pub camera: Entity,
    pub sequence: u64,
    pub target_size: UVec2,
    pub sorted_fragment_max_count: u32,
    /// Bevy's u32 attempt counter; workloads approaching 2^32 fragments/frame
    /// are outside this diagnostic's qualification (counter wrap is not detected).
    pub attempted_fragments: u32,
    pub node_capacity: u32,
    pub dropped_fragments: u32,
    /// Count over the exact-sort threshold among *accepted* fragments. A lower
    /// bound when global drops occur; zero never certifies visual correctness.
    pub over_sort_pixels: u32,
    pub invalid_links: u32,
    /// Capped at sorted_fragment_max_count + 1, not an unbounded depth statistic.
    pub capped_max_depth: u32,
}

#[derive(Clone, Debug, PartialEq)]
struct Key {
    camera: Entity,
    size: UVec2,
    target: NormalizedRenderTarget,
    average: u32,
    sorted_max: u32,
    budget: u64,
}

#[derive(Default)]
struct State {
    enabled: bool,
    generation: u64,
    sequence: u64,
    key: Option<Key>,
    sample: Option<ExperimentalOitDiagnosticSample>,
}

/// Explicitly enabled bounded aggregate GPU readback (16 bytes/sample, 3 slots).
/// Disabled by default: no allocation, dispatch or readback. This initial probe
/// supports exactly one OIT camera, full target, without resolution overrides.
/// `None` means disabled/pending/unavailable, never "no overflow". Changing the
/// camera/configuration or rejection invalidates previous samples at PostUpdate;
/// disabling invalidates immediately. Slots remain bounded/resident after use.
#[derive(Resource, Clone, Default)]
pub struct ExperimentalOitDiagnostics(Arc<Mutex<State>>);

impl ExperimentalOitDiagnostics {
    pub fn set_enabled(&self, enabled: bool) {
        let mut state = self.0.lock().unwrap();
        if state.enabled != enabled {
            state.enabled = enabled;
            state.generation += 1;
            state.key = None;
            state.sample = None;
            state.sequence = 0;
        }
    }

    pub fn snapshot(&self) -> Option<ExperimentalOitDiagnosticSample> {
        self.0.lock().unwrap().sample
    }

    fn configure(&self, key: Option<Key>) -> Option<u64> {
        let mut state = self.0.lock().unwrap();
        let key = state.enabled.then_some(key).flatten();
        if state.key != key {
            state.generation += 1;
            state.key = key;
            state.sample = None;
            state.sequence = 0;
        }
        state.key.as_ref().map(|_| state.generation)
    }

    fn current(&self, generation: u64) -> bool {
        let state = self.0.lock().unwrap();
        state.enabled && state.key.is_some() && state.generation == generation
    }

    fn publish(
        &self,
        generation: u64,
        sequence: u64,
        sample: Option<ExperimentalOitDiagnosticSample>,
    ) {
        let mut state = self.0.lock().unwrap();
        if state.enabled
            && state.key.is_some()
            && state.generation == generation
            && sequence > state.sequence
        {
            state.sequence = sequence;
            state.sample = sample;
        }
    }
}

#[derive(Component, Clone, ExtractComponent)]
#[extract_component_sync_target(Self)]
#[extract_app(RenderApp)]
struct Request {
    generation: u64,
    key: Key,
}

pub(super) fn install(app: &mut App) {
    let diagnostics = ExperimentalOitDiagnostics::default();
    app.insert_resource(diagnostics.clone())
        .add_plugins(ExtractComponentPlugin::<Request, RenderApp>::default())
        .add_systems(PostUpdate, synchronize.after(super::approve));
    if let Some(render) = app.get_sub_app_mut(RenderApp) {
        render
            .insert_resource(diagnostics)
            .init_resource::<Transport>()
            .add_systems(
                Core3d,
                measure
                    .after(main_transparent_pass_3d)
                    .before(oit_resolve)
                    .in_set(Core3dSystems::MainPass),
            );
    }
}

#[allow(clippy::type_complexity)]
fn synchronize(
    mut commands: Commands,
    diagnostics: Res<ExperimentalOitDiagnostics>,
    cameras: Query<
        (
            Entity,
            &Camera,
            &RenderTarget,
            &ExperimentalOitSettings,
            Has<OitApproved>,
            Has<MainPassResolutionOverride>,
        ),
        With<Camera3d>,
    >,
    oit_cameras: Query<Entity, With<OrderIndependentTransparencySettings>>,
    requests: Query<Entity, With<Request>>,
    primary: Query<Entity, With<PrimaryWindow>>,
) {
    let mut key = None;
    if oit_cameras.iter().count() == 1 {
        for (entity, camera, target, settings, approved, resolution_override) in &cameras {
            if approved
                && camera.is_active
                && camera.viewport.is_none()
                && !resolution_override
                && let Some(size) = camera.physical_target_size()
                && let Some(target) = target.normalize(primary.single().ok())
            {
                key = Some(Key {
                    camera: entity,
                    size,
                    target,
                    average: settings.fragments_per_pixel_average.to_bits(),
                    sorted_max: settings.sorted_fragment_max_count,
                    budget: settings.max_storage_bytes,
                });
            }
        }
    }
    let generation = diagnostics.configure(key.clone());
    for entity in &requests {
        if generation.is_none() || key.as_ref().is_none_or(|key| key.camera != entity) {
            commands.entity(entity).remove::<Request>();
        }
    }
    if let (Some(generation), Some(key)) = (generation, key) {
        commands
            .entity(key.camera)
            .insert(Request { generation, key });
    }
}

struct Slot {
    output: wgpu::Buffer,
    staging: wgpu::Buffer,
    busy: Arc<AtomicBool>,
}
#[derive(Resource, Default)]
struct Transport {
    pipeline: Option<wgpu::ComputePipeline>,
    slots: Vec<Slot>,
    sequence: u64,
}

fn measure(
    view: ViewQuery<(&Request, &OitResolvePipelineId)>,
    buffers: Option<Res<OitBuffers>>,
    cache: Res<PipelineCache>,
    diagnostics: Res<ExperimentalOitDiagnostics>,
    mut transport: ResMut<Transport>,
    mut context: RenderContext,
) {
    let (request, resolve) = view.into_inner();
    if !diagnostics.current(request.generation) || cache.get_render_pipeline(resolve.0).is_none() {
        return;
    }
    let Some(buffers) = buffers else {
        return;
    };
    let (Some(nodes), Some(heads), Some(counter)) = (
        buffers.nodes.buffer(),
        buffers.heads.buffer(),
        buffers.atomic_counter.buffer(),
    ) else {
        return;
    };
    let pixels = request.key.size.x * request.key.size.y;
    if buffers.heads.capacity() != pixels as usize {
        return;
    }
    let capacity = buffers.nodes.capacity() as u32;
    let device = context.render_device().wgpu_device().clone();
    if transport.pipeline.is_none() {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("aestra OIT aggregate diagnostic"),
            source: wgpu::ShaderSource::Wgsl(include_str!("diagnostics.wgsl").into()),
        });
        transport.pipeline = Some(device.create_compute_pipeline(
            &wgpu::ComputePipelineDescriptor {
                label: Some("aestra OIT aggregate diagnostic"),
                layout: None,
                module: &shader,
                entry_point: Some("main"),
                compilation_options: default(),
                cache: None,
            },
        ));
    }
    let slot_index = transport
        .slots
        .iter()
        .position(|slot| !slot.busy.load(Ordering::Acquire));
    let index = match slot_index {
        Some(index) => index,
        None if transport.slots.len() < 3 => {
            let make = |usage| {
                device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("aestra OIT diagnostic aggregate"),
                    size: 16,
                    usage,
                    mapped_at_creation: false,
                })
            };
            transport.slots.push(Slot {
                output: make(
                    wgpu::BufferUsages::STORAGE
                        | wgpu::BufferUsages::COPY_SRC
                        | wgpu::BufferUsages::COPY_DST,
                ),
                staging: make(wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST),
                busy: Arc::new(AtomicBool::new(false)),
            });
            transport.slots.len() - 1
        }
        _ => return, // Backpressure skips a sample, never allocates another slot.
    };
    transport.sequence += 1;
    let sequence = transport.sequence;
    let slot = &transport.slots[index];
    slot.busy.store(true, Ordering::Release);
    let params = [pixels, capacity, request.key.sorted_max, request.key.size.x];
    let bytes: Vec<u8> = params.into_iter().flat_map(u32::to_le_bytes).collect();
    use wgpu::util::DeviceExt;
    let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("aestra OIT diagnostic limits"),
        contents: &bytes,
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let pipeline = transport.pipeline.as_ref().unwrap();
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("aestra OIT diagnostic inputs"),
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: nodes.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: heads.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: counter.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: uniform.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: slot.output.as_entire_binding(),
            },
        ],
    });
    let encoder = context.command_encoder();
    encoder.clear_buffer(&slot.output, 0, None);
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("aestra OIT bounded diagnostic"),
            ..default()
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.dispatch_workgroups(request.key.size.x.div_ceil(64), request.key.size.y, 1);
    }
    encoder.copy_buffer_to_buffer(&slot.output, 0, &slot.staging, 0, 16);
    let staging = slot.staging.clone();
    let busy = slot.busy.clone();
    let diagnostics = diagnostics.clone();
    let generation = request.generation;
    let camera = request.key.camera;
    let target_size = request.key.size;
    let sorted_fragment_max_count = request.key.sorted_max;
    encoder.map_buffer_on_submit(&slot.staging, wgpu::MapMode::Read, 0..16, move |result| {
        let sample = result.ok().and_then(|_| {
            let mapped = staging.slice(..).get_mapped_range().ok()?;
            let words: Vec<u32> = mapped
                .as_chunks::<4>()
                .0
                .iter()
                .map(|word| u32::from_le_bytes(*word))
                .collect();
            Some(ExperimentalOitDiagnosticSample {
                camera,
                sequence,
                target_size,
                sorted_fragment_max_count,
                node_capacity: capacity,
                attempted_fragments: words[0],
                dropped_fragments: words[0].saturating_sub(capacity),
                over_sort_pixels: words[1],
                invalid_links: words[2],
                capped_max_depth: words[3],
            })
        });
        staging.unmap();
        diagnostics.publish(generation, sequence, sample);
        busy.store(false, Ordering::Release);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shader_validates_and_traversal_is_bounded() {
        let module = naga::front::wgsl::parse_str(include_str!("diagnostics.wgsl")).unwrap();
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap();
    }

    #[test]
    fn late_callbacks_cannot_revive_disabled_or_reconfigured_measurements() {
        let diagnostics = ExperimentalOitDiagnostics::default();
        let key = Key {
            camera: Entity::PLACEHOLDER,
            size: UVec2::new(480, 360),
            target: NormalizedRenderTarget::None {
                width: 480,
                height: 360,
            },
            average: 4.0f32.to_bits(),
            sorted_max: 8,
            budget: 32 << 20,
        };
        assert!(diagnostics.configure(Some(key.clone())).is_none());
        diagnostics.set_enabled(true);
        let generation = diagnostics.configure(Some(key.clone())).unwrap();
        let sample = ExperimentalOitDiagnosticSample {
            camera: key.camera,
            sequence: 2,
            target_size: key.size,
            sorted_fragment_max_count: key.sorted_max,
            attempted_fragments: 100,
            node_capacity: 10,
            dropped_fragments: 90,
            over_sort_pixels: 2,
            invalid_links: 0,
            capped_max_depth: 9,
        };
        diagnostics.publish(generation, 2, Some(sample));
        assert_eq!(diagnostics.snapshot().unwrap().sequence, 2);
        diagnostics.publish(
            generation,
            1,
            Some(ExperimentalOitDiagnosticSample {
                sequence: 1,
                ..sample
            }),
        );
        assert_eq!(diagnostics.snapshot().unwrap().sequence, 2);
        let mut changed = key;
        changed.sorted_max = 1;
        let next = diagnostics.configure(Some(changed)).unwrap();
        assert_ne!(generation, next);
        diagnostics.publish(generation, 3, Some(sample));
        assert!(diagnostics.snapshot().is_none());
        diagnostics.publish(next, 2, Some(sample));
        diagnostics.publish(next, 1, None);
        assert!(
            diagnostics.snapshot().is_some(),
            "late failure cannot clear newer result"
        );
        diagnostics.publish(next, 3, None);
        assert!(
            diagnostics.snapshot().is_none(),
            "current failure is unknown, not healthy"
        );
        diagnostics.publish(next, 2, Some(sample));
        assert!(
            diagnostics.snapshot().is_none(),
            "late success cannot revive failed result"
        );
        diagnostics.configure(None);
        diagnostics.publish(next, 4, Some(sample));
        assert!(
            diagnostics.snapshot().is_none(),
            "removed camera stays unknown"
        );
        diagnostics.set_enabled(false);
        diagnostics.publish(next, 4, Some(sample));
        assert!(diagnostics.snapshot().is_none());
        diagnostics.set_enabled(true);
        assert!(diagnostics.snapshot().is_none());
        diagnostics.configure(None);
        diagnostics.publish(next, 5, Some(sample));
        assert!(diagnostics.snapshot().is_none());
    }
}
