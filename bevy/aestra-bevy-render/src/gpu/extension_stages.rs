//! Frame-loop execution of plugin extension stages (fluid F1, extensible-stages M13b).
//!
//! Every [`PresentedEffect`] whose compiled effect has extension stages (a fluid solver) gets its
//! stages executed in the render world, each on a [`crate::execution::StageTimeline`]:
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

pub use super::output_context::AestraOutputEvent;
pub(crate) use super::stage_inputs::ExtractedStages;
use super::stage_inputs::FieldViewTarget;
pub use super::stage_output_delivery::AestraEffectOutputs;
use super::stage_output_delivery::{
    StageOutputIdentity, StageOutputMailbox, StageOutputStamp, encode_stage_outputs,
    raise_finished_events, receive_stage_outputs,
};
pub(crate) use super::stage_runtimes::StageRuntimes;
#[cfg(test)]
use super::stage_runtimes::StagesKey;
use super::stage_runtimes::prepare_stage_runtimes;
pub use super::world_sdf::AestraWorldSdf;
use super::*;
#[cfg(test)]
use crate::execution::TimelinePolicy;
use crate::execution::{DomainSpawnPipeline, FieldFollowPipeline, PassTimestamps};
#[cfg(test)]
use aestra_compiler::ExtensionRegistry;
use aestra_core::ResourceTypeId;
#[cfg(test)]
use aestra_gpu::GpuHostBindings;
use aestra_gpu::GpuWorldSdf;
use aestra_runtime::{
    CompiledEffect, CompiledExtensionStage, EffectInstance, FieldLayout, ProfileValue,
};
use bevy::render::{renderer::RenderQueue, sync_world::MainEntity, texture::GpuImage};
use simulation_timing::{SimulationTimer, TimingMailbox};
use std::{collections::HashMap, sync::Mutex};
use wgpu::util::DeviceExt;

/// Latest render-world confirmation that all extension stages reached the requested time.
/// Unlike particle counts this also works for volume-only effects with no emitters.
#[derive(Component, Default, Debug)]
pub struct GpuStageProgress {
    sample: Option<StageProgressSample>,
}

#[derive(Clone, Debug)]
struct StageProgressSample {
    effect: Arc<CompiledEffect>,
    seed: u32,
    epoch: u32,
    revision: u64,
    time: f32,
}

impl GpuStageProgress {
    pub fn is_ready(&self, instance: &EffectInstance) -> bool {
        self.sample.as_ref().is_some_and(|sample| {
            Arc::ptr_eq(&sample.effect, instance.effect())
                && sample.seed == instance.seed() as u32
                && sample.epoch == instance.history_epoch()
                && sample.revision == instance.history_revision()
                && (sample.time - instance.time()).abs() < 0.0001
        })
    }
}

fn progress_target_tick(time: f32, tick_dt: f32, coupled: bool) -> u32 {
    if coupled {
        // Match particles, binding traces and output suppression at exact clock boundaries.
        aestra_runtime::trace_tick(time).min(u64::from(u32::MAX)) as u32
    } else {
        (time.max(0.0) / tick_dt + 1e-3).floor() as u32
    }
}

#[derive(Resource, Default, Clone)]
struct StageProgressMailbox(Arc<Mutex<HashMap<Entity, Option<StageProgressSample>>>>);

fn receive_stage_progress(
    mailbox: Res<StageProgressMailbox>,
    mut commands: Commands,
    players: Query<&PresentedEffect>,
) {
    if let Ok(mut samples) = mailbox.0.lock() {
        for (entity, sample) in samples.drain() {
            if players.contains(entity) {
                commands.entity(entity).insert(GpuStageProgress { sample });
            }
        }
    }
}

/// Debug views of extension-stage fields (fluid F1). Off by default; editors and viewers opt in.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct AestraDebugViews {
    /// Show a slice through one grid field of every effect that has one.
    pub field_slices: bool,
}

/// The host's world SDF on the device for stateful particles' `World` colliders (host bindings
/// HB10): uploaded again only on a new revision; a header saying "no world" until the host sets one.
#[derive(Resource)]
pub(crate) struct ParticleWorldBuffer {
    revision: Option<u64>,
    pub(crate) buffer: Buffer,
}

fn prepare_particle_world(
    mut commands: Commands,
    device: Res<RenderDevice>,
    world: Option<Res<AestraWorldSdf>>,
    current: Option<Res<ParticleWorldBuffer>>,
) {
    let packed = world.as_deref().and_then(AestraWorldSdf::packed);
    let revision = packed.map(|packed| packed.revision);
    if current
        .as_ref()
        .is_some_and(|current| current.revision == revision)
    {
        return;
    }
    let absent = GpuWorldSdf::absent();
    let bytes = packed.unwrap_or(&absent).to_bytes();
    let buffer =
        device.create_buffer_with_data(&bevy::render::render_resource::BufferInitDescriptor {
            label: Some("aestra particle world"),
            contents: &bytes,
            usage: bevy::render::render_resource::BufferUsages::STORAGE,
        });
    commands.insert_resource(ParticleWorldBuffer { revision, buffer });
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

/// The Follow Field (fluid F2b), Spawn From Domain (fluid F10) and event gather (host bindings HB9b)
/// pipelines the stateful path uses for emitters advancing in lockstep.
#[derive(Resource)]
pub(crate) struct FieldFollow(
    FieldFollowPipeline,
    DomainSpawnPipeline,
    crate::execution::EventGatherPipeline,
);

fn init_field_follow(mut commands: Commands, device: Res<RenderDevice>) {
    let device = device.wgpu_device();
    commands.insert_resource(FieldFollow(
        FieldFollowPipeline::new(device),
        DomainSpawnPipeline::new(device),
        crate::execution::EventGatherPipeline::new(device),
    ));
}

/// The lockstep coupling for an effect with particle event links but no coupled domains (host
/// bindings HB9b): its emitters advance tick by tick together, with no domain.
pub(super) fn link_coupling(follower: Option<&FieldFollow>) -> Option<super::Coupling<'_>> {
    let follower = follower?;
    Some(super::Coupling {
        domains: &mut [],
        inputs: crate::execution::StageInputs::default(),
        follower: &follower.0,
        spawner: &follower.1,
        gatherer: &follower.2,
    })
}

/// The coupling for an effect whose stateful emitters follow its domains (fluid F2b), when it has one
/// and its domains are prepared.
pub(super) fn coupling<'a>(
    runtimes: &'a mut StageRuntimes,
    entity: Entity,
    extracted: Option<&'a ExtractedStages>,
    follower: Option<&'a FieldFollow>,
) -> Option<super::Coupling<'a>> {
    let extracted = extracted.filter(|extracted| extracted.coupled)?;
    let runtime = runtimes.0.get_mut(&entity)?;
    let follower = follower?;
    Some(super::Coupling {
        domains: &mut runtime.timelines,
        inputs: extracted.inputs(),
        follower: &follower.0,
        spawner: &follower.1,
        gatherer: &follower.2,
    })
}

/// Main-world state of an effect's field view: its quad (whose material keeps the image alive) and
/// what the render world copies into the image.
#[derive(Component)]
pub(super) struct FieldViewState {
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
    let progress = StageProgressMailbox::default();
    let outputs = StageOutputMailbox::default();
    app.init_resource::<AestraDebugViews>()
        .init_resource::<super::AestraCatchupPacing>()
        .insert_resource(mailbox.clone())
        .insert_resource(progress.clone())
        .insert_resource(outputs.clone())
        .add_message::<AestraOutputEvent>()
        .add_systems(PreUpdate, receive_stage_outputs)
        .add_systems(PostUpdate, raise_finished_events)
        .add_systems(PreUpdate, receive_stage_timings)
        .add_systems(PreUpdate, receive_stage_progress)
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
        .insert_resource(progress)
        .insert_resource(outputs)
        .init_resource::<StageRuntimes>()
        .add_systems(
            RenderStartup,
            (init_field_slice_pipeline, init_field_follow),
        )
        .add_systems(
            Render,
            (prepare_stage_runtimes, prepare_particle_world)
                .in_set(RenderSystems::PrepareResources),
        )
        .add_systems(
            RenderGraph,
            // After the particle simulation, which advances coupled domains itself (fluid F2b), so
            // field views show every domain's final state for the frame.
            run_extension_stages
                .after(super::run_simulation)
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
        Option<&'static super::volume::VolumeViews>,
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
pub(super) fn sync_stage_inputs(
    mut commands: Commands,
    effects: StageInputQuery,
    world: Option<Res<AestraWorldSdf>>,
) {
    let world: Option<&GpuWorldSdf> = world.as_ref().and_then(|world| world.packed());
    for (entity, presented, transform, view, volumes, extracted, timed) in &effects {
        let effect = presented.effect();
        if stages(effect).next().is_none() {
            if extracted {
                commands.entity(entity).remove::<ExtractedStages>();
            }
            continue;
        }
        let mut entity = commands.entity(entity);
        entity.insert(ExtractedStages::from_presented(
            presented,
            transform.map_or(aestra_runtime::IDENTITY_AFFINE, world_to_local),
            view.map(|view| view.target.clone()),
            volumes
                .map(super::volume::VolumeViews::targets)
                .unwrap_or_default(),
            world.cloned(),
        ));
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
        Option<&super::volume::VolumeViews>,
    )>,
    cameras_3d: Query<(), With<Camera3d>>,
    mut assets: (
        Option<ResMut<Assets<Image>>>,
        Option<ResMut<Assets<Mesh>>>,
        Option<ResMut<Assets<StandardMaterial>>>,
    ),
) {
    for (entity, presented, request, state, layers, volumes) in effects {
        // An effect drawn as a volume shows slices only on request.
        let automatic = debug.field_slices && !volumes.is_some_and(|views| views.draws_any());
        let wanted = (automatic || request.is_some())
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

/// Advances every effect's stage timelines to its presentation time, then refreshes field views.
#[allow(clippy::too_many_arguments)]
fn run_extension_stages(
    mut render_context: RenderContext,
    mut runtimes: ResMut<StageRuntimes>,
    effects: Query<(Entity, &MainEntity, &ExtractedStages)>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    mailbox: Res<StageTimingMailbox>,
    mut pacer: ResMut<super::CatchupPacer>,
    progress: Res<StageProgressMailbox>,
    outputs: Res<StageOutputMailbox>,
    mut timer: Local<SimulationTimer>,
    slice_pipeline: Option<Res<FieldSlicePipeline>>,
    volume_pipeline: Option<Res<super::volume::FieldVolume>>,
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
        // Paced by frame time: a replay after a rebuild or a seek spreads over frames.
        let budget = pacer.budget(extracted.quality);
        // Coupled domains advance in lockstep with their particles, in the stateful path (fluid F2b).
        let pending: Vec<usize> = if extracted.coupled {
            Vec::new()
        } else {
            runtime
                .timelines
                .iter()
                .enumerate()
                .filter_map(|(index, timeline)| {
                    let timeline = timeline.as_ref()?;
                    (timeline.tick_for_time(extracted.time) != timeline.last_tick())
                        .then_some(index)
                })
                .collect()
        };
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
            match timeline.advance_to(
                wgpu_device,
                render_context.command_encoder(),
                target,
                budget,
                extracted.inputs(),
                stamps,
            ) {
                Ok(report) => pacer.spent(report.ticks, budget),
                Err(error) => {
                    warn!("extension stage stopped: {error}");
                    runtime.timelines[index] = None;
                }
            }
        }
        let caught_up = runtime.timelines.iter().all(|timeline| {
            timeline.as_ref().is_some_and(|timeline| {
                timeline.last_tick()
                    == progress_target_tick(
                        extracted.time,
                        timeline.policy().tick_dt,
                        extracted.coupled,
                    )
            })
        });
        if let Ok(mut samples) = progress.0.lock() {
            samples.insert(
                main_entity.id(),
                caught_up.then(|| StageProgressSample {
                    effect: extracted.effect.clone(),
                    seed: extracted.seed,
                    epoch: extracted.history_epoch,
                    revision: extracted.history_revision,
                    time: extracted.time,
                }),
            );
        }
        // Outputs (fluid F11): read back once the stage moved — this frame, or in the coupled
        // particle pass before it — and zeroed for the next frame's ticks.
        for (index, timeline) in runtime.timelines.iter().enumerate() {
            let Some(timeline) = timeline else {
                continue;
            };
            let stamp = StageOutputStamp {
                owner: main_entity.id(),
                stage: index,
                tick: u64::from(timeline.last_tick()),
                identity: StageOutputIdentity::of_extracted(extracted),
            };
            if runtime.read_ticks[index].as_ref() == Some(&stamp) {
                continue;
            }
            if encode_stage_outputs(
                timeline.executor(),
                wgpu_device,
                render_context.command_encoder(),
                &outputs,
                stamp.clone(),
            ) {
                runtime.read_ticks[index] = Some(stamp);
            }
        }
        // The debug field slice.
        if let (Some(view), Some(pipeline)) = (&extracted.view, &slice_pipeline)
            && let Some(Some(timeline)) = runtime.timelines.get(view.stage)
            && let Some(field) = timeline.executor().field_buffers(&view.layout)
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
        // Volume textures (fluid F3), from every stage's final state for the frame.
        if let Some(pipeline) = &volume_pipeline {
            for target in &extracted.volumes {
                if let Some(Some(timeline)) = runtime.timelines.get(target.stage)
                    && let Some(field) = timeline.executor().field_buffers(&target.layout)
                    && let Some(image) = images.get(target.image)
                {
                    let table = target
                        .table
                        .and_then(|table| images.get(table))
                        .map(|table| &table.texture_view);
                    pipeline.0.encode(
                        wgpu_device,
                        render_context.command_encoder(),
                        field,
                        &image.texture_view,
                        &target.layout,
                        table.map(|view| &**view),
                    );
                }
            }
        }
    }
    gpu_span.end(render_context.command_encoder());
    if let Some(batch) = batch {
        batch.finish(&mut render_context, mailbox.0.clone());
    }
}

/// Copies one z-slice of a grid field into an `rgba8unorm` storage texture; a bricked field (fluid
/// F7) through its brick table (binding 3; any buffer when every cell is stored), empty where its
/// bricks are not stored.
fn field_slice_wgsl() -> String {
    format!(
        "{FIELD_SLICE_WGSL}{}",
        aestra_gpu::brick_cell_wgsl("slice_brick_cell", "slice_table")
    )
}

const FIELD_SLICE_WGSL: &str = r#"
struct SliceParams {
    dims_x: u32,
    dims_y: u32,
    components: u32,
    slice: u32,
    gain: f32,
    dims_z: u32,
    brick_edge: u32,
    table_word: u32,
}

@group(0) @binding(0) var<storage, read> field: array<f32>;
@group(0) @binding(1) var<storage, read> params: SliceParams;
@group(0) @binding(2) var slice_image: texture_storage_2d<rgba8unorm, write>;
@group(0) @binding(3) var<storage, read> slice_table: array<u32>;

@compute @workgroup_size(8, 8)
fn slice_field(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= params.dims_x || id.y >= params.dims_y) { return; }
    let stride = select(params.components, 4u, params.components == 3u);
    var element = (params.slice * params.dims_y + id.y) * params.dims_x + id.x;
    let texel = vec2<i32>(i32(id.x), i32(params.dims_y - 1u - id.y));
    if (params.brick_edge != 0u) {
        let dims = vec3<u32>(params.dims_x, params.dims_y, params.dims_z);
        element = slice_brick_cell(vec3<u32>(id.xy, params.slice), dims, params.brick_edge, params.table_word);
        if (element == 0xffffffffu) {
            textureStore(slice_image, texel, vec4<f32>(0.0));
            return;
        }
    }
    let base = element * stride;
    var color: vec4<f32>;
    if (params.components == 1u) {
        color = vec4<f32>(1.0, 1.0, 1.0, clamp(field[base] * params.gain, 0.0, 1.0));
    } else {
        let v = vec3<f32>(field[base], field[base + 1u], field[base + 2u]) * params.gain;
        color = vec4<f32>(clamp(abs(v), vec3<f32>(0.0), vec3<f32>(1.0)), clamp(length(v), 0.0, 1.0));
    }
    // Texture rows run downward; the grid's y runs upward.
    textureStore(slice_image, texel, color);
}
"#;

#[derive(Resource)]
struct FieldSlicePipeline(wgpu::ComputePipeline);

fn init_field_slice_pipeline(mut commands: Commands, device: Res<RenderDevice>) {
    let device = device.wgpu_device();
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("aestra field slice"),
        source: wgpu::ShaderSource::Wgsl(field_slice_wgsl().into()),
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
        field: crate::execution::FieldBuffers<'_>,
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
            layout.dims[2],
            layout.bricks.as_ref().map_or(0, |bricks| bricks.edge),
            layout.bricks.as_ref().map_or(0, |bricks| bricks.table_word),
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
                    resource: field.field.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: params.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(image),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: field.table.unwrap_or(field.field).as_entire_binding(),
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
        let module = naga::front::wgsl::parse_str(&field_slice_wgsl()).expect("parses");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::default(),
        )
        .validate(&module)
        .expect("the field slice shader validates");
    }

    fn fire_with(module: &str, input: &str, value: aestra_core::Value) -> ExtractedStages {
        let mut registry = ExtensionRegistry::builtin();
        registry.install(&aestra_fluid::FluidExtension).unwrap();
        let mut effect = aestra_fluid::fire_effect(&registry);
        let found = effect.simulation_stages[0]
            .modules
            .iter_mut()
            .find(|candidate| candidate.module_type.0 == module)
            .unwrap();
        let aestra_core::ModuleParameters::Custom(values) = &mut found.parameters else {
            unreachable!("plugin modules carry a generic payload");
        };
        values.insert(input.into(), value);
        let compiled = aestra_compiler::EffectCompiler::with_extensions(registry)
            .compile(&effect)
            .unwrap();
        let instance = EffectInstance::new(Arc::new(compiled));
        ExtractedStages {
            host: GpuHostBindings::from_instance(&instance),
            output_identity: Arc::new(()),
            effect: Arc::clone(instance.effect()),
            time: 0.0,
            quality: SeekQuality::Exact,
            history_policy: PlaybackHistoryPolicy::default(),
            seed: 7,
            history_epoch: 0,
            history_epoch_start_time: 0.,
            history_revision: 0,
            host_epoch: 0,
            world_to_effect: aestra_runtime::IDENTITY_AFFINE,
            coupled: false,
            view: None,
            volumes: Vec::new(),
            world: None,
        }
    }

    #[test]
    fn a_moved_source_updates_the_running_stages_but_a_new_grid_rebuilds_them() {
        use aestra_core::Value;
        let at = |position| {
            fire_with(
                aestra_fluid::MODULE_DENSITY_SOURCE,
                "position",
                Value::Vec3(position),
            )
        };
        let key = StagesKey::of(&at([0.0, 15.0, 0.0]));
        let moved = at([20.0, 10.0, 0.0]);
        assert!(!key.matches(&moved));
        assert!(key.differs_only_in_constants(&moved), "taken in place");
        let regridded = fire_with(aestra_fluid::MODULE_GRID, "resolution", Value::U32(24));
        assert!(
            !key.differs_only_in_constants(&regridded),
            "new resources and fields rebuild the stages"
        );
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

    #[test]
    fn stage_progress_requires_the_current_simulation_context_and_time() {
        let extracted = fire_with(
            aestra_fluid::MODULE_GRID,
            "resolution",
            aestra_core::Value::U32(16),
        );
        let mut instance = EffectInstance::new(extracted.effect.clone());
        instance.set_seed(7);
        instance.seek(1.5);
        let sample = || StageProgressSample {
            effect: instance.effect().clone(),
            seed: instance.seed() as u32,
            epoch: instance.history_epoch(),
            revision: instance.history_revision(),
            time: instance.time(),
        };
        assert!(!GpuStageProgress::default().is_ready(&instance));
        let ready = GpuStageProgress {
            sample: Some(sample()),
        };
        assert!(ready.is_ready(&instance));
        instance.set_playback_time(2.0);
        assert!(!ready.is_ready(&instance));
        instance.seek(1.5);
        assert!(!ready.is_ready(&instance));
        let ready = GpuStageProgress {
            sample: Some(StageProgressSample {
                effect: instance.effect().clone(),
                seed: instance.seed() as u32,
                epoch: instance.history_epoch(),
                revision: instance.history_revision(),
                time: instance.time(),
            }),
        };
        assert!(ready.is_ready(&instance));
        instance.set_seed(8);
        assert!(!ready.is_ready(&instance));
    }

    #[test]
    fn stage_progress_matches_coupled_clock_boundaries_and_independent_ticks() {
        let dt = TimelinePolicy::default().tick_dt;
        assert_eq!(progress_target_tick(2.0, dt, false), 120);
        assert_eq!(progress_target_tick(2.0, dt, true), 120);
        assert_eq!(progress_target_tick(18.0, dt, true), 1080);
        assert_eq!(progress_target_tick(18.0_f32.next_down(), dt, true), 1079);
        assert_eq!(progress_target_tick(2.001, dt, false), 120);
        assert_eq!(progress_target_tick(2.001, dt, true), 120);
    }
}
