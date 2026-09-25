//! Frame-loop execution of plugin extension stages (fluid F1, extensible-stages M13b).
//!
//! Every [`PresentedEffect`] whose compiled effect has extension stages (a fluid solver) gets its
//! stages executed in the render world, each on a [`StageTimeline`]:
//!
//! - the main world mirrors the instance's presentation time, seek quality, seed and host-binding
//!   snapshot into [`ExtractedStages`] each frame;
//! - the render world keeps one timeline per stage per effect entity, rebuilt only when the compiled
//!   effect or seed changes; a rebound host object drops the checkpoints (never restored under
//!   another object) while the live state continues;
//! - each frame the render graph advances every timeline to the tick the presentation time asks
//!   for, within the same per-frame catch-up budgets the stateful particle path uses (`Preview`
//!   while scrubbing, `Exact` otherwise), restoring the nearest GPU-resident checkpoint on a backward
//!   seek — the same restart/restore-and-replay semantics, independent of the CPU players' compiled
//!   seek mode;
//! - GPU time is measured per instance with pass timestamps ([`GpuStageTiming`], surfaced as
//!   `EffectProfile::gpu_stage_time_ns`) and spanned as `aestra::gpu::extension_stages` for Bevy's
//!   render diagnostics.
//!
//! Host input during catch-up/replay is the current snapshot, not a recorded history — historical
//! host input is host-bindings HB8.
//!
//! **Debug field slices.** With [`AestraDebugViews::field_slices`] (or an [`AestraFieldView`] on the
//! effect), one grid field a stage declares ([`aestra_runtime::FieldLayout`]) is shown as a slice: a
//! compute pass copies the slice from the field's buffer into a storage texture drawn on an unlit
//! quad, a child of the effect. Scalar fields render as white with alpha = value × gain; vector fields
//! as |xyz| × gain. The grid is placed in the effect's space (the domain-space decision is fluid F2).

use super::*;
use crate::execution::{PassTimestamps, StageExecutor, StageInputs, StageTimeline, TimelinePolicy};
use aestra_compiler::ExtensionRegistry;
use aestra_core::ResourceTypeId;
use aestra_gpu::GpuHostBindings;
use aestra_runtime::{
    CompiledEffect, CompiledExtensionStage, EffectInstance, FieldLayout, ProfileValue,
};
use bevy::render::{renderer::RenderQueue, sync_world::MainEntity, texture::GpuImage};
use simulation_timing::{SimulationTimer, TimingMailbox};
use wgpu::util::DeviceExt;

/// Debug views of extension-stage fields (fluid F1). Off by default; editors and viewers opt in.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct AestraDebugViews {
    /// Show a slice through one grid field of every effect that has one.
    pub field_slices: bool,
}

/// Requests, and tunes, the field slice view of one effect regardless of [`AestraDebugViews`].
#[derive(Component, Debug, Clone)]
pub struct AestraFieldView {
    /// The field to show; `None` picks the first scalar field (else the first field).
    pub resource: Option<ResourceTypeId>,
    /// Multiplies field values before they become colour/alpha.
    pub gain: f32,
    /// Where along the grid's z axis the slice sits, 0–1.
    pub slice: f32,
}

impl Default for AestraFieldView {
    fn default() -> Self {
        Self {
            resource: None,
            gain: 1.0,
            slice: 0.5,
        }
    }
}

/// The latest measured GPU time of an instance's extension stages (fluid F1): every tick simulated in
/// the most recent frame that advanced them, catch-up and replay included.
#[derive(Component, Debug, Default, Clone)]
pub struct GpuStageTiming {
    sample: Option<(f32, u64)>,
}

impl GpuStageTiming {
    /// The measured time, when it was taken at or before the instance's current time.
    pub fn time_ns(&self, instance: &EffectInstance) -> ProfileValue<u64> {
        self.sample
            .filter(|(time, _)| *time <= instance.time())
            .map_or(ProfileValue::Unavailable, |(_, nanoseconds)| {
                ProfileValue::Measured(nanoseconds)
            })
    }
}

/// What the render world needs to run an effect's extension stages this frame.
#[derive(Component, Clone)]
pub(crate) struct ExtractedStages {
    effect: Arc<CompiledEffect>,
    time: f32,
    quality: SeekQuality,
    host: GpuHostBindings,
    seed: u32,
    host_epoch: u64,
    /// The effect's placement: world space into its space (fluid F2).
    world_to_effect: [[f32; 4]; 3],
    view: Option<FieldViewTarget>,
}

impl SyncComponent for ExtractedStages {
    type Target = Self;
}

impl ExtractComponent for ExtractedStages {
    type QueryData = &'static Self;
    type QueryFilter = ();
    type Out = Self;

    fn extract_component(
        stages: bevy::ecs::query::QueryItem<'_, '_, Self::QueryData>,
    ) -> Option<Self::Out> {
        Some(stages.clone())
    }
}

/// A field slice the render world copies into a view image.
#[derive(Clone)]
struct FieldViewTarget {
    image: AssetId<Image>,
    stage: usize,
    layout: FieldLayout,
    slice: u32,
    gain: f32,
}

/// Main-world state of an effect's field view: its quad (whose material keeps the image alive) and
/// what the render world copies into the image.
#[derive(Component)]
struct FieldViewState {
    quad: Entity,
    target: FieldViewTarget,
}

/// Marks the quad a field view draws on.
#[derive(Component)]
struct FieldViewQuad;

#[derive(Resource, Default, Clone)]
struct StageTimingMailbox(TimingMailbox);

/// The effect's extension stages — its own domains, then its enabled emitters' — in a stable order.
fn stages(effect: &CompiledEffect) -> impl Iterator<Item = &CompiledExtensionStage> {
    effect.all_extension_stages()
}

pub(super) fn install(app: &mut App) {
    let mailbox = StageTimingMailbox::default();
    app.init_resource::<AestraDebugViews>()
        .insert_resource(mailbox.clone())
        .add_plugins(ExtractComponentPlugin::<ExtractedStages>::default())
        .add_systems(PreUpdate, receive_stage_timings)
        .add_systems(
            Update,
            (sync_field_views, sync_stage_inputs)
                .chain()
                .in_set(crate::AestraRenderSet::Prepare),
        );
    let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
        return;
    };
    render_app
        .insert_resource(mailbox)
        .init_resource::<StageRuntimes>()
        .add_systems(RenderStartup, init_field_slice_pipeline)
        .add_systems(
            Render,
            prepare_stage_runtimes.in_set(RenderSystems::PrepareResources),
        )
        .add_systems(
            RenderGraph,
            run_extension_stages
                .after(RenderGraphSystems::Begin)
                .before(RenderGraphSystems::Render),
        );
}

/// A presented effect, its field view, and which stage components it already carries.
type StageInputQuery<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static PresentedEffect,
        Option<&'static GlobalTransform>,
        Option<&'static FieldViewState>,
        Has<ExtractedStages>,
        Has<GpuStageTiming>,
    ),
>;

/// The 3×4 affine taking world space into the space `transform` places (rows).
fn world_to_local(transform: &GlobalTransform) -> [[f32; 4]; 3] {
    let inverse = transform.affine().inverse();
    let (x, y, z, t) = (
        inverse.matrix3.x_axis,
        inverse.matrix3.y_axis,
        inverse.matrix3.z_axis,
        inverse.translation,
    );
    [
        [x.x, y.x, z.x, t.x],
        [x.y, y.y, z.y, t.y],
        [x.z, y.z, z.z, t.z],
    ]
}

/// Mirrors each presented effect's stage inputs for extraction (or removes them).
fn sync_stage_inputs(mut commands: Commands, effects: StageInputQuery) {
    for (entity, presented, transform, view, extracted, timed) in &effects {
        let effect = presented.effect();
        if stages(effect).next().is_none() {
            if extracted {
                commands.entity(entity).remove::<ExtractedStages>();
            }
            continue;
        }
        let instance = &presented.instance;
        let mut entity = commands.entity(entity);
        entity.insert(ExtractedStages {
            effect: Arc::clone(effect),
            time: instance.time(),
            quality: presented.seek_quality(),
            host: GpuHostBindings::from_instance(instance),
            seed: instance.seed() as u32,
            host_epoch: instance.host_input_epoch(),
            world_to_effect: transform.map_or(aestra_runtime::IDENTITY_AFFINE, world_to_local),
            view: view.map(|view| view.target.clone()),
        });
        if !timed {
            entity.insert(GpuStageTiming::default());
        }
    }
}

/// Picks the field an effect's view shows: the requested one, else the first scalar, else the first.
fn pick_field(
    effect: &CompiledEffect,
    requested: Option<&ResourceTypeId>,
) -> Option<(usize, FieldLayout)> {
    let fields: Vec<(usize, &FieldLayout)> = stages(effect)
        .enumerate()
        .flat_map(|(stage, compiled)| compiled.block.fields.iter().map(move |f| (stage, f)))
        .collect();
    let chosen = match requested {
        Some(resource) => fields.iter().find(|(_, field)| &field.resource == resource),
        None => fields
            .iter()
            .find(|(_, field)| field.components == 1)
            .or(fields.first()),
    };
    chosen.map(|(stage, field)| (*stage, (*field).clone()))
}

/// Creates, updates or removes each effect's field-view quad.
#[allow(clippy::type_complexity)]
fn sync_field_views(
    mut commands: Commands,
    debug: Res<AestraDebugViews>,
    effects: Query<(
        Entity,
        &PresentedEffect,
        Option<&AestraFieldView>,
        Option<&mut FieldViewState>,
        Option<&RenderLayers>,
    )>,
    cameras_3d: Query<(), With<Camera3d>>,
    mut assets: (
        Option<ResMut<Assets<Image>>>,
        Option<ResMut<Assets<Mesh>>>,
        Option<ResMut<Assets<StandardMaterial>>>,
    ),
) {
    for (entity, presented, request, state, layers) in effects {
        let wanted = (debug.field_slices || request.is_some())
            .then(|| {
                pick_field(
                    presented.effect(),
                    request.and_then(|r| r.resource.as_ref()),
                )
            })
            .flatten();
        let settings = request.cloned().unwrap_or_default();
        let Some((stage, layout)) = wanted else {
            if let Some(state) = state {
                commands.entity(state.quad).despawn();
                commands.entity(entity).remove::<FieldViewState>();
            }
            continue;
        };
        let slice = ((settings.slice.clamp(0.0, 1.0) * layout.dims[2] as f32) as u32)
            .min(layout.dims[2] - 1);
        if let Some(mut state) = state {
            if state.target.stage == stage && state.target.layout == layout {
                state.target.slice = slice;
                state.target.gain = settings.gain;
                continue;
            }
            commands.entity(state.quad).despawn();
            commands.entity(entity).remove::<FieldViewState>();
        }
        let Some(images) = assets.0.as_mut() else {
            continue; // no image assets: nothing can present a view
        };
        let mut image = Image::new_fill(
            Extent3d {
                width: layout.dims[0],
                height: layout.dims[1],
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            &[0, 0, 0, 0],
            TextureFormat::Rgba8Unorm,
            RenderAssetUsages::RENDER_WORLD,
        );
        image.texture_descriptor.usage |=
            bevy::render::render_resource::TextureUsages::STORAGE_BINDING;
        let image = images.add(image);
        let size = Vec2::new(
            layout.dims[0] as f32 * layout.cell_size,
            layout.dims[1] as f32 * layout.cell_size,
        );
        let origin = Vec3::from(layout.origin);
        let center = origin
            + Vec3::new(
                size.x * 0.5,
                size.y * 0.5,
                (slice as f32 + 0.5) * layout.cell_size,
            );
        let transform = Transform::from_translation(center);
        // A 3-D preview draws an unlit quad; a 2-D view (no 3-D camera) draws a sprite.
        let quad = match (!cameras_3d.is_empty(), assets.1.as_mut(), assets.2.as_mut()) {
            (true, Some(meshes), Some(materials)) => commands
                .spawn((
                    FieldViewQuad,
                    Mesh3d(meshes.add(Rectangle::from_size(size))),
                    MeshMaterial3d(materials.add(StandardMaterial {
                        base_color_texture: Some(image.clone()),
                        unlit: true,
                        alpha_mode: AlphaMode::Blend,
                        double_sided: true,
                        cull_mode: None,
                        ..default()
                    })),
                    transform,
                    ChildOf(entity),
                ))
                .id(),
            _ => commands
                .spawn((
                    FieldViewQuad,
                    Sprite {
                        image: image.clone(),
                        custom_size: Some(size),
                        ..default()
                    },
                    transform,
                    ChildOf(entity),
                ))
                .id(),
        };
        if let Some(layers) = layers {
            commands.entity(quad).insert(layers.clone());
        }
        commands.entity(entity).insert(FieldViewState {
            quad,
            target: FieldViewTarget {
                image: image.id(),
                stage,
                layout,
                slice,
                gain: settings.gain,
            },
        });
    }
}

fn receive_stage_timings(
    mailbox: Res<StageTimingMailbox>,
    mut timings: Query<(&PresentedEffect, &mut GpuStageTiming)>,
) {
    let Some(samples) = mailbox.0.take() else {
        return;
    };
    // Stages only work while they advance: a frame with no ticks (paused, caught up) publishes no
    // sample, and the latest real measurement stays — `time_ns` still drops it once a backward seek
    // puts it after the playhead.
    for sample in samples {
        if let Ok((presented, mut timing)) = timings.get_mut(sample.owner)
            && sample.time.is_finite()
            && sample.time <= presented.instance.time()
        {
            timing.sample = Some((sample.time, sample.nanoseconds));
        }
    }
}

/// One effect's stage timelines in the render world.
struct EffectStages {
    /// Identity of the compiled effect and seed the timelines were built for.
    key: (usize, u32),
    host_epoch: u64,
    /// One per stage; `None` when the stage could not be prepared (logged once).
    timelines: Vec<Option<StageTimeline>>,
}

#[derive(Resource, Default)]
struct StageRuntimes(BTreeMap<Entity, EffectStages>);

/// Builds, keeps or drops each effect's stage timelines.
fn prepare_stage_runtimes(
    mut runtimes: ResMut<StageRuntimes>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    effects: Query<(Entity, &ExtractedStages)>,
) {
    runtimes.0.retain(|entity, _| effects.contains(*entity));
    for (entity, extracted) in &effects {
        let key = (Arc::as_ptr(&extracted.effect) as usize, extracted.seed);
        if let Some(existing) = runtimes.0.get_mut(&entity)
            && existing.key == key
        {
            if existing.host_epoch != extracted.host_epoch {
                // Recorded against another host object: never restore it under this one.
                for timeline in existing.timelines.iter_mut().flatten() {
                    timeline.invalidate_checkpoints();
                }
                existing.host_epoch = extracted.host_epoch;
            }
            continue;
        }
        // The linked programs, including any extension linked since the last build.
        let programs = ExtensionRegistry::linked().programs;
        let wgpu_queue: &wgpu::Queue = &queue;
        let timelines = stages(&extracted.effect)
            .map(|stage| {
                StageExecutor::new(
                    device.wgpu_device(),
                    wgpu_queue,
                    &stage.block,
                    &programs,
                    extracted.host.byte_len(),
                )
                .map(|executor| {
                    StageTimeline::new(executor, TimelinePolicy::default(), extracted.seed)
                })
                .map_err(|error| {
                    warn!(
                        "extension stage '{}' ({}) cannot run on this backend: {error}",
                        stage.name,
                        stage.stage_type.as_str()
                    );
                })
                .ok()
            })
            .collect();
        runtimes.0.insert(
            entity,
            EffectStages {
                key,
                host_epoch: extracted.host_epoch,
                timelines,
            },
        );
    }
}

/// Advances every effect's stage timelines to its presentation time, then refreshes field views.
#[allow(clippy::too_many_arguments)]
fn run_extension_stages(
    mut render_context: RenderContext,
    mut runtimes: ResMut<StageRuntimes>,
    effects: Query<(Entity, &MainEntity, &ExtractedStages)>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    mailbox: Res<StageTimingMailbox>,
    mut timer: Local<SimulationTimer>,
    slice_pipeline: Option<Res<FieldSlicePipeline>>,
    images: Res<RenderAssets<GpuImage>>,
) {
    if effects.is_empty() {
        return;
    }
    let _span = tracing::info_span!("aestra::gpu::extension_stages").entered();
    let diagnostics = render_context.diagnostic_recorder();
    let diagnostics = diagnostics.as_deref();
    let gpu_span = diagnostics.time_span(
        render_context.command_encoder(),
        "aestra::gpu::extension_stages",
    );
    let mut batch = timer.begin(&device, queue.get_timestamp_period());
    let wgpu_device = device.wgpu_device();
    for (entity, main_entity, extracted) in &effects {
        let Some(runtime) = runtimes.0.get_mut(&entity) else {
            continue;
        };
        let budget = stateful_catchup_budget(extracted.quality);
        let pending: Vec<usize> = runtime
            .timelines
            .iter()
            .enumerate()
            .filter_map(|(index, timeline)| {
                let timeline = timeline.as_ref()?;
                (timeline.tick_for_time(extracted.time) != timeline.last_tick()).then_some(index)
            })
            .collect();
        let timing = (!pending.is_empty())
            .then(|| {
                batch
                    .as_mut()?
                    .instance(main_entity.id(), 0, extracted.time)
            })
            .flatten();
        for (position, &index) in pending.iter().enumerate() {
            let Some(timeline) = runtime.timelines[index].as_mut() else {
                continue;
            };
            let stamps = batch
                .as_ref()
                .zip(timing)
                .map(|(batch, slot)| PassTimestamps {
                    query_set: batch.query_set(),
                    begin: (position == 0).then_some(slot),
                    end: (position + 1 == pending.len()).then_some(slot + 1),
                });
            let target = timeline.tick_for_time(extracted.time);
            if let Err(error) = timeline.advance_to(
                wgpu_device,
                render_context.command_encoder(),
                target,
                budget,
                StageInputs {
                    host_bindings: Some(&extracted.host),
                    world_to_effect: extracted.world_to_effect,
                },
                stamps,
            ) {
                warn!("extension stage stopped: {error}");
                runtime.timelines[index] = None;
            }
        }
        // The debug field slice.
        if let (Some(view), Some(pipeline)) = (&extracted.view, &slice_pipeline)
            && let Some(Some(timeline)) = runtime.timelines.get(view.stage)
            && let Some(field) = timeline.executor().buffer(view.layout.resource.as_str())
            && let Some(image) = images.get(view.image)
        {
            pipeline.encode(
                wgpu_device,
                render_context.command_encoder(),
                field,
                &image.texture_view,
                view,
            );
        }
    }
    gpu_span.end(render_context.command_encoder());
    if let Some(batch) = batch {
        batch.finish(&mut render_context, mailbox.0.clone());
    }
}

/// Copies one z-slice of a grid field into an `rgba8unorm` storage texture.
const FIELD_SLICE_WGSL: &str = r#"
struct SliceParams {
    dims_x: u32,
    dims_y: u32,
    components: u32,
    slice: u32,
    gain: f32,
}

@group(0) @binding(0) var<storage, read> field: array<f32>;
@group(0) @binding(1) var<storage, read> params: SliceParams;
@group(0) @binding(2) var slice_image: texture_storage_2d<rgba8unorm, write>;

@compute @workgroup_size(8, 8)
fn slice_field(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= params.dims_x || id.y >= params.dims_y) { return; }
    let stride = select(params.components, 4u, params.components == 3u);
    let base = ((params.slice * params.dims_y + id.y) * params.dims_x + id.x) * stride;
    var color: vec4<f32>;
    if (params.components == 1u) {
        color = vec4<f32>(1.0, 1.0, 1.0, clamp(field[base] * params.gain, 0.0, 1.0));
    } else {
        let v = vec3<f32>(field[base], field[base + 1u], field[base + 2u]) * params.gain;
        color = vec4<f32>(clamp(abs(v), vec3<f32>(0.0), vec3<f32>(1.0)), clamp(length(v), 0.0, 1.0));
    }
    // Texture rows run downward; the grid's y runs upward.
    textureStore(slice_image, vec2<i32>(i32(id.x), i32(params.dims_y - 1u - id.y)), color);
}
"#;

#[derive(Resource)]
struct FieldSlicePipeline(wgpu::ComputePipeline);

fn init_field_slice_pipeline(mut commands: Commands, device: Res<RenderDevice>) {
    let device = device.wgpu_device();
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("aestra field slice"),
        source: wgpu::ShaderSource::Wgsl(FIELD_SLICE_WGSL.into()),
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("aestra field slice"),
        layout: None,
        module: &module,
        entry_point: Some("slice_field"),
        compilation_options: Default::default(),
        cache: None,
    });
    commands.insert_resource(FieldSlicePipeline(pipeline));
}

impl FieldSlicePipeline {
    fn encode(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        field: &wgpu::Buffer,
        image: &wgpu::TextureView,
        view: &FieldViewTarget,
    ) {
        let layout = &view.layout;
        let params: [u32; 8] = [
            layout.dims[0],
            layout.dims[1],
            layout.components,
            view.slice,
            view.gain.to_bits(),
            0,
            0,
            0,
        ];
        let params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("aestra field slice params"),
            contents: &params
                .iter()
                .flat_map(|word| word.to_le_bytes())
                .collect::<Vec<u8>>(),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("aestra field slice"),
            layout: &self.0.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: field.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: params.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(image),
                },
            ],
        });
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("aestra field slice"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.0);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.dispatch_workgroups(layout.dims[0].div_ceil(8), layout.dims[1].div_ceil(8), 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_field_slice_shader_validates() {
        let module = naga::front::wgsl::parse_str(FIELD_SLICE_WGSL).expect("parses");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::default(),
        )
        .validate(&module)
        .expect("the field slice shader validates");
    }

    #[test]
    fn stage_timing_is_only_reported_for_the_present_or_past() {
        let effect = aestra_compiler::EffectCompiler::default()
            .compile(&aestra_core::EffectAsset::new("Timed", 3.0))
            .unwrap();
        let mut instance = EffectInstance::new(Arc::new(effect));
        instance.set_playback_time(1.0);
        let timing = GpuStageTiming {
            sample: Some((0.5, 1200)),
        };
        assert_eq!(timing.time_ns(&instance), ProfileValue::Measured(1200));
        instance.set_playback_time(0.25);
        assert_eq!(
            timing.time_ns(&instance),
            ProfileValue::Unavailable,
            "a sample from after a backward seek is not current"
        );
    }
}
