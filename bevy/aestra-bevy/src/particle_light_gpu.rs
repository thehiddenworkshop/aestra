//! Opt-in bounded native Bevy GPU-clustered realization. No selected-position
//! readback. Default layers only until layer-aware GPU clustering is certified.
use crate::{
    EffectOutputContext, ParticleLightMode, ParticleLightRealizationSettings, PresentedEffect,
};
use aestra_bevy_render::gpu::particle_lights::{
    AestraParticleLightSettings, GpuSelectedParticleLights, ParticleLightArtifact,
    ParticleLightSelectionSet, ParticleLightSource,
};
use bevy::{
    camera::visibility::RenderLayers,
    light::cluster::GlobalClusterSettings,
    pbr::{GlobalClusterableObjectMeta, GpuClusteredLight},
    prelude::*,
    render::{
        Extract, ExtractSchedule, MainWorld, Render, RenderApp, RenderSystems,
        extract_component::{ExtractComponent, ExtractComponentPlugin},
        extract_resource::{ExtractResource, ExtractResourcePlugin},
        render_resource::*,
        renderer::{RenderContext, RenderDevice, RenderGraph, RenderGraphSystems, RenderQueue},
        view::ExtractedView,
    },
};
use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex},
};

/// Independent host cap and byte budgets. Zero max_lights disables this adapter.
/// buffer bytes include our mapping/token/uniform buffers and the reserved Bevy
/// logical light records, not Bevy's allocation granularity/camera-cluster storage
/// or the separately budgeted selector. This is not a total renderer-memory cap.
#[derive(Resource, Clone, Debug, ExtractResource)]
pub struct ParticleLightGpuSettings {
    pub max_lights: u32,
    pub max_buffer_bytes: u64,
    /// Qualified source metadata, including clip paths and collection overhead.
    pub max_manifest_bytes: usize,
}
impl Default for ParticleLightGpuSettings {
    fn default() -> Self {
        Self {
            max_lights: 96,
            max_buffer_bytes: 1024 * 1024,
            max_manifest_bytes: 1024 * 1024,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParticleLightGpuRejection {
    BufferBudget,
    ManifestBudget,
    InvalidClamps,
    UnsupportedClustering,
    NonDefaultLayers,
    SlotUnavailable,
    PipelineLoading,
    Shader(String),
    Selection(String),
}

/// Last render observation, asynchronously visible to the main world. Capacities
/// are bounds, NOT read-back counts of illuminated particles. No GPU readback is
/// added for statistics. Dispatches and sequence refer to the last dispatched
/// selection; written_capacity is zero during rejection/disable/empty preparation.
#[derive(Clone, Debug, Default)]
pub struct ParticleLightGpuObservation {
    pub dispatches: u64,
    pub sequence: u64,
    pub reserved_slots: u32,
    pub written_capacity: u32,
    pub invalid_sources: usize,
    /// Current metadata bytes plus the logical reserved-light record bound.
    pub buffer_bytes: u64,
    pub rejection: Option<ParticleLightGpuRejection>,
}
#[derive(Resource, Clone, Default, ExtractResource)]
pub struct ParticleLightGpuStatistics(Arc<Mutex<ParticleLightGpuObservation>>);
impl ParticleLightGpuStatistics {
    pub fn snapshot(&self) -> ParticleLightGpuObservation {
        self.0.lock().unwrap().clone()
    }
}

/// Reserved slot ordinal, not a source-particle identity. Do not edit its light,
/// visibility, layers or transform. Slots stay zero-lumen in the main world.
#[derive(Component, Clone, Copy, ExtractComponent)]
pub struct ParticleLightGpuSlot(pub u32);
#[derive(Resource, Default)]
struct Slots(Vec<Entity>);
#[derive(Resource)]
struct NoRenderBridge;
#[derive(Resource, Default)]
struct Authorizations {
    sources: BTreeSet<ParticleLightSource>,
    rejection: Option<ParticleLightGpuRejection>,
}
#[derive(Resource, Clone, ExtractResource)]
struct BridgeShader(Handle<Shader>);
#[derive(Resource)]
struct Pipeline {
    layout: BindGroupLayoutDescriptor,
    id: CachedComputePipelineId,
}
#[derive(ShaderType)]
struct Params {
    bounds: UVec4,
    clamps: Vec4,
}
struct Buffers {
    slots: Buffer,
    tokens: Buffer,
    uniform: UniformBuffer<Params>,
    shape: (u32, usize),
}
#[derive(Resource, Default)]
struct State {
    buffers: Option<Buffers>,
}

pub(super) fn install(app: &mut App) {
    app.init_resource::<ParticleLightGpuSettings>()
        .init_resource::<ParticleLightGpuStatistics>()
        .init_resource::<Slots>()
        .add_systems(
            PostUpdate,
            reserve_slots.before(bevy::transform::TransformSystems::Propagate),
        );
    if app.get_sub_app(RenderApp).is_none() {
        app.insert_resource(NoRenderBridge);
        return;
    }
    let shader = app
        .world_mut()
        .resource_mut::<Assets<Shader>>()
        .add(Shader::from_wgsl(
            include_str!("particle_light_gpu.wgsl"),
            file!(),
        ));
    app.insert_resource(BridgeShader(shader)).add_plugins((
        ExtractComponentPlugin::<ParticleLightGpuSlot>::default(),
        ExtractResourcePlugin::<ParticleLightGpuSettings>::default(),
        ExtractResourcePlugin::<ParticleLightGpuStatistics>::default(),
        ExtractResourcePlugin::<ParticleLightRealizationSettings>::default(),
        ExtractResourcePlugin::<BridgeShader>::default(),
    ));
    app.sub_app_mut(RenderApp)
        .init_resource::<Authorizations>()
        .init_resource::<State>()
        .add_systems(ExtractSchedule, authorize)
        .add_systems(Render, init.in_set(RenderSystems::PrepareResources))
        .add_systems(
            RenderGraph,
            inject
                .after(ParticleLightSelectionSet)
                .before(RenderGraphSystems::Render),
        );
}

fn valid_clamps(settings: &ParticleLightRealizationSettings) -> bool {
    settings.max_lumens.is_finite()
        && settings.max_lumens > 0.0
        && settings.max_range.is_finite()
        && settings.max_range > 0.0
}
fn slot_cap(
    settings: &ParticleLightGpuSettings,
    global: u32,
    clamps: &ParticleLightRealizationSettings,
) -> Result<u32, ParticleLightGpuRejection> {
    if !valid_clamps(clamps) {
        return Err(ParticleLightGpuRejection::InvalidClamps);
    }
    let cap = settings.max_lights.min(global);
    if cap == 0 {
        return Ok(0);
    }
    // One 16-byte mapping and one exact Bevy light record per reserved slot.
    if u64::from(cap) * (16 + GpuClusteredLight::min_size().get()) + Params::min_size().get()
        > settings.max_buffer_bytes
    {
        return Err(ParticleLightGpuRejection::BufferBudget);
    }
    Ok(cap)
}
fn placeholder(range: f32) -> PointLight {
    PointLight {
        intensity: 0.0,
        range,
        shadow_maps_enabled: false,
        contact_shadows_enabled: false,
        ..default()
    }
}
fn reserve_slots(world: &mut World) {
    let settings = world.resource::<ParticleLightGpuSettings>();
    let clamps = world.resource::<ParticleLightRealizationSettings>().clone();
    let mut cap = if *world.resource::<ParticleLightMode>() == ParticleLightMode::SameFrameGpu {
        slot_cap(
            settings,
            world.resource::<AestraParticleLightSettings>().max_lights,
            &clamps,
        )
        .unwrap_or(0) as usize
    } else {
        0
    };
    if world.contains_resource::<NoRenderBridge>() {
        world
            .resource::<ParticleLightGpuStatistics>()
            .0
            .lock()
            .unwrap()
            .rejection = (cap > 0).then_some(ParticleLightGpuRejection::UnsupportedClustering);
        cap = 0;
    }
    if let Some(device) = world.get_resource::<RenderDevice>() {
        let limits = device.limits();
        if limits.max_storage_buffers_per_shader_stage < 5
            || (cap as u64) * GpuClusteredLight::min_size().get()
                > limits
                    .max_storage_buffer_binding_size
                    .min(limits.max_buffer_size)
            || (cap as u64) * 16 > limits.max_buffer_size
            || (cap as u32).div_ceil(64) > limits.max_compute_workgroups_per_dimension
        {
            cap = 0;
        }
    }
    if world
        .get_resource::<GlobalClusterSettings>()
        .is_some_and(|s| !s.supports_storage_buffers || s.gpu_clustering.is_none())
    {
        cap = 0;
    }
    world.resource_scope(|world, mut slots: Mut<Slots>| {
        slots.0.retain(|e| {
            if world.get::<ParticleLightGpuSlot>(*e).is_some()
                && world.get::<PointLight>(*e).is_some()
            {
                true
            } else {
                if world.entities().contains(*e) {
                    world.despawn(*e);
                }
                false
            }
        });
        let retained = cap.min(slots.0.len());
        for entity in slots.0.drain(retained..) {
            world.despawn(entity);
        }
        while slots.0.len() < cap {
            slots.0.push(
                world
                    .spawn((
                        ParticleLightGpuSlot(0),
                        placeholder(clamps.max_range),
                        Transform::IDENTITY,
                    ))
                    .id(),
            );
        }
        for (index, entity) in slots.0.iter().enumerate() {
            if world.get::<ParticleLightGpuSlot>(*entity).unwrap().0 != index as u32 {
                world
                    .entity_mut(*entity)
                    .insert(ParticleLightGpuSlot(index as u32));
            }
            let light = world.get::<PointLight>(*entity).unwrap();
            if light.intensity != 0.0
                || light.range != clamps.max_range
                || light.shadow_maps_enabled
                || light.contact_shadows_enabled
            {
                world
                    .entity_mut(*entity)
                    .insert(placeholder(clamps.max_range));
            }
            if world.get::<Transform>(*entity) != Some(&Transform::IDENTITY) {
                world.entity_mut(*entity).insert(Transform::IDENTITY);
            }
            if world.get::<RenderLayers>(*entity) != Some(&RenderLayers::default()) {
                world.entity_mut(*entity).insert(RenderLayers::default());
            }
            if world.get::<Visibility>(*entity) != Some(&Visibility::Visible) {
                world.entity_mut(*entity).insert(Visibility::Visible);
            }
            // Placeholder position is unrelated to the GPU light. Only GPU
            // clustering may spatially cull it. Avoid dirtying unchanged lights
            // every tick (unnecessary shadow-frustum/extraction work).
            if world
                .get::<bevy::camera::visibility::NoFrustumCulling>(*entity)
                .is_none()
            {
                world
                    .entity_mut(*entity)
                    .insert(bevy::camera::visibility::NoFrustumCulling);
            }
        }
    });
}

// Only qualified source metadata crosses extraction, never source particles or
// their positions. Share the portable adapter's epoch/artifact/context validation.
#[allow(clippy::type_complexity)]
fn authorize(
    mut commands: Commands,
    main: Res<MainWorld>,
    players: Extract<Query<(Entity, &PresentedEffect, Option<&EffectOutputContext>)>>,
    mode: Extract<Res<ParticleLightMode>>,
    settings: Extract<Res<ParticleLightGpuSettings>>,
) {
    let mut result = Authorizations::default();
    let mut bytes = 0usize;
    if **mode == ParticleLightMode::SameFrameGpu
        && settings.max_lights > 0
        && main.resource::<AestraParticleLightSettings>().max_lights > 0
    {
        'owners: for (owner, player, context) in &players {
            if main
                .get::<crate::EffectRuntimeStatus>(owner)
                .is_none_or(|r| r.active != crate::ActiveBackend::Gpu)
            {
                continue;
            }
            for emitter in &player.effect().emitters {
                for output in &emitter.scene_outputs {
                    let aestra_runtime::SceneOutputPlanKind::ParticlePointLight(plan) =
                        &output.kind;
                    if !emitter.enabled || plan.max_lights == 0 {
                        continue;
                    }
                    let path_bytes = context
                        .map_or(0, |c| c.clip_path.len())
                        .checked_mul(std::mem::size_of::<crate::EffectClipId>());
                    let Some(next) = path_bytes
                        .and_then(|n| {
                            n.checked_add(std::mem::size_of::<ParticleLightSource>() + 64)
                        })
                        .and_then(|n| bytes.checked_add(n))
                        .filter(|n| *n <= settings.max_manifest_bytes)
                    else {
                        result.rejection = Some(ParticleLightGpuRejection::ManifestBudget);
                        break 'owners;
                    };
                    let source = ParticleLightSource {
                        root: context.map_or(owner, |c| c.root),
                        root_epoch: context
                            .map_or(player.instance.history_epoch(), |c| c.playback_epoch),
                        clip_path: context.map_or_else(Vec::new, |c| c.clip_path.clone()),
                        owner,
                        owner_epoch: player.instance.history_epoch(),
                        revision: player.instance.history_revision(),
                        effect: player.effect().source,
                        seed: player.instance.seed(),
                        emitter: emitter.source,
                        region: emitter.region,
                        output: output.source,
                        artifact: ParticleLightArtifact(player.effect().clone()),
                    };
                    let Some(layers) = super::particle_lights::source_layers(&main, &source) else {
                        continue;
                    };
                    // Extraction runs after visibility propagation; hide a
                    // source under a hidden transform parent as well as an
                    // explicitly hidden root/owner.
                    if [source.owner, source.root].iter().any(|e| {
                        main.get::<InheritedVisibility>(*e)
                            .is_some_and(|v| !v.get())
                    }) {
                        continue;
                    }
                    if layers != RenderLayers::default() {
                        result.rejection = Some(ParticleLightGpuRejection::NonDefaultLayers);
                        break 'owners;
                    }
                    bytes = next;
                    result.sources.insert(source);
                }
            }
        }
    }
    if result.rejection.is_some() {
        result.sources.clear();
    }
    commands.insert_resource(result);
}

fn init(
    mut commands: Commands,
    mode: Res<ParticleLightMode>,
    shader: Res<BridgeShader>,
    existing: Option<Res<Pipeline>>,
    cache: Res<PipelineCache>,
    clusters: Option<Res<GlobalClusterSettings>>,
    meta: Option<Res<GlobalClusterableObjectMeta>>,
) {
    if *mode != ParticleLightMode::SameFrameGpu
        || existing.is_some()
        || meta.is_none()
        || clusters.is_none_or(|s| !s.supports_storage_buffers || s.gpu_clustering.is_none())
    {
        return;
    }
    let storage = |binding, read_only, size| BindGroupLayoutEntry {
        binding,
        visibility: ShaderStages::COMPUTE,
        ty: BindingType::Buffer {
            ty: BufferBindingType::Storage { read_only },
            has_dynamic_offset: false,
            min_binding_size: std::num::NonZeroU64::new(size),
        },
        count: None,
    };
    let layout = BindGroupLayoutDescriptor::new(
        "aestra bounded clustered lights",
        &[
            storage(0, true, 48),
            storage(1, true, 16),
            storage(2, false, GpuClusteredLight::min_size().get()),
            storage(3, true, 16),
            storage(4, true, 16),
            BindGroupLayoutEntry {
                binding: 5,
                visibility: ShaderStages::COMPUTE,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: Some(Params::min_size()),
                },
                count: None,
            },
        ],
    );
    let id = cache.queue_compute_pipeline(ComputePipelineDescriptor {
        label: Some("aestra bounded clustered lights".into()),
        layout: vec![layout.clone()],
        shader: shader.0.clone(),
        entry_point: Some("inject".into()),
        ..default()
    });
    commands.insert_resource(Pipeline { layout, id });
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn inject(
    mut context: RenderContext,
    (mode, settings, clamps, selection): (
        Res<ParticleLightMode>,
        Res<ParticleLightGpuSettings>,
        Res<ParticleLightRealizationSettings>,
        Res<AestraParticleLightSettings>,
    ),
    selected: Res<GpuSelectedParticleLights>,
    sources: Res<Authorizations>,
    clusters: Option<Res<GlobalClusterSettings>>,
    meta: Option<Res<GlobalClusterableObjectMeta>>,
    slots: Query<(Entity, &ParticleLightGpuSlot)>,
    views: Query<
        Option<&RenderLayers>,
        (
            With<ExtractedView>,
            With<bevy::render::camera::ExtractedCamera>,
        ),
    >,
    pipeline: Option<Res<Pipeline>>,
    cache: Res<PipelineCache>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    mut state: ResMut<State>,
    statistics: Res<ParticleLightGpuStatistics>,
) {
    let mut stats = statistics.0.lock().unwrap();
    stats.rejection = None;
    stats.written_capacity = 0;
    stats.reserved_slots = 0;
    stats.invalid_sources = 0;
    stats.buffer_bytes = 0;
    let result = (|| {
        if *mode != ParticleLightMode::SameFrameGpu {
            return Ok(());
        }
        let cap = slot_cap(&settings, selection.max_lights, &clamps)?;
        stats.reserved_slots = slots.iter().count() as u32;
        stats.buffer_bytes = u64::from(stats.reserved_slots) * GpuClusteredLight::min_size().get();
        if cap == 0 {
            return Ok(());
        }
        let (Some(clusters), Some(meta)) = (&clusters, &meta) else {
            return Err(ParticleLightGpuRejection::UnsupportedClustering);
        };
        let limits = device.limits();
        if !clusters.supports_storage_buffers
            || clusters.gpu_clustering.is_none()
            || limits.max_storage_buffers_per_shader_stage < 5
            || cap.div_ceil(64) > limits.max_compute_workgroups_per_dimension
        {
            return Err(ParticleLightGpuRejection::UnsupportedClustering);
        }
        if views
            .iter()
            .any(|l| l.is_some_and(|l| *l != RenderLayers::default()))
        {
            return Err(ParticleLightGpuRejection::NonDefaultLayers);
        }
        if let Some(error) = &sources.rejection {
            return Err(error.clone());
        }
        let Some(frame) = selected.frame() else {
            return match &selected.rejection {
                Some(e) => Err(ParticleLightGpuRejection::Selection(e.clone())),
                None => Ok(()),
            };
        };
        let token_count = frame.manifest.len().max(1);
        let bytes = u64::from(cap) * (16 + GpuClusteredLight::min_size().get())
            + token_count as u64 * 16
            + Params::min_size().get();
        if bytes > settings.max_buffer_bytes
            || u64::from(cap) * 16
                > limits
                    .max_storage_buffer_binding_size
                    .min(limits.max_buffer_size)
            || token_count as u64 * 16
                > limits
                    .max_storage_buffer_binding_size
                    .min(limits.max_buffer_size)
        {
            return Err(ParticleLightGpuRejection::BufferBudget);
        }
        let mut destinations = vec![UVec4::splat(u32::MAX); cap as usize];
        for (entity, slot) in &slots {
            let Some(destination) = destinations.get_mut(slot.0 as usize) else {
                continue;
            };
            let Some(&index) = meta.entity_to_index.get(&entity) else {
                return Err(ParticleLightGpuRejection::SlotUnavailable);
            };
            if destination.x != u32::MAX {
                return Err(ParticleLightGpuRejection::SlotUnavailable);
            }
            destination.x =
                u32::try_from(index).map_err(|_| ParticleLightGpuRejection::SlotUnavailable)?;
        }
        if destinations.iter().any(|d| d.x == u32::MAX) {
            return Err(ParticleLightGpuRejection::SlotUnavailable);
        }
        let Some(BindingResource::Buffer(target)) = meta.gpu_clustered_lights.binding() else {
            return Err(ParticleLightGpuRejection::SlotUnavailable);
        };
        if !target.buffer.usage().contains(BufferUsages::STORAGE) {
            return Err(ParticleLightGpuRejection::UnsupportedClustering);
        }
        let Some(pipeline) = pipeline else {
            return Err(ParticleLightGpuRejection::PipelineLoading);
        };
        let Some(compiled) = cache.get_compute_pipeline(pipeline.id) else {
            return Err(match cache.get_compute_pipeline_state(pipeline.id) {
                CachedPipelineState::Err(e) => ParticleLightGpuRejection::Shader(e.to_string()),
                _ => ParticleLightGpuRejection::PipelineLoading,
            });
        };
        let mut tokens = vec![UVec4::ZERO; token_count];
        for (index, source) in frame.manifest.iter().enumerate() {
            if sources.sources.contains(source) {
                tokens[index].x = 1;
            } else {
                stats.invalid_sources += 1;
            }
        }
        if state
            .buffers
            .as_ref()
            .is_none_or(|b| b.shape != (cap, token_count))
        {
            let buffer = |size| {
                device.create_buffer(&BufferDescriptor {
                    label: Some("aestra bounded light metadata"),
                    size,
                    usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                })
            };
            state.buffers = Some(Buffers {
                slots: buffer(u64::from(cap) * 16),
                tokens: buffer(token_count as u64 * 16),
                uniform: UniformBuffer::from(Params {
                    bounds: UVec4::ZERO,
                    clamps: Vec4::ZERO,
                }),
                shape: (cap, token_count),
            });
        }
        let buffers = state.buffers.as_mut().unwrap();
        let encoded = |values: &Vec<UVec4>| {
            let mut b = encase::StorageBuffer::new(Vec::new());
            b.write(values).unwrap();
            b.into_inner()
        };
        queue.write_buffer(&buffers.slots, 0, &encoded(&destinations));
        queue.write_buffer(&buffers.tokens, 0, &encoded(&tokens));
        buffers.uniform.set(Params {
            bounds: UVec4::new(cap, frame.selected_capacity, frame.manifest.len() as u32, 0),
            clamps: Vec4::new(clamps.max_lumens, clamps.max_range, 0.0, 0.0),
        });
        buffers.uniform.write_buffer(&device, &queue);
        let group = device.create_bind_group(
            "aestra bounded clustered lights",
            &cache.get_bind_group_layout(&pipeline.layout),
            &BindGroupEntries::sequential((
                frame.records.as_entire_buffer_binding(),
                frame.counters.as_entire_buffer_binding(),
                BindingResource::Buffer(target),
                buffers.slots.as_entire_buffer_binding(),
                buffers.tokens.as_entire_buffer_binding(),
                buffers.uniform.binding().unwrap(),
            )),
        );
        let mut pass = context
            .command_encoder()
            .begin_compute_pass(&ComputePassDescriptor {
                label: Some("aestra same-frame selected lights"),
                timestamp_writes: None,
            });
        pass.set_pipeline(compiled);
        pass.set_bind_group(0, &group, &[]);
        pass.dispatch_workgroups(cap.div_ceil(64), 1, 1);
        stats.dispatches += 1;
        stats.sequence = frame.sequence;
        stats.written_capacity = cap.min(frame.selected_capacity);
        stats.buffer_bytes = bytes;
        Ok(())
    })();
    if let Err(error) = result {
        stats.rejection = Some(error);
    }
    if stats.written_capacity == 0 {
        state.buffers = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn caps_are_configurable_and_invalid_budgets_fail_closed() {
        let mut settings = ParticleLightGpuSettings {
            max_lights: 1024,
            ..default()
        };
        let clamps = ParticleLightRealizationSettings::default();
        assert_eq!(slot_cap(&settings, 512, &clamps), Ok(512));
        settings.max_buffer_bytes = 0;
        assert_eq!(
            slot_cap(&settings, 512, &clamps),
            Err(ParticleLightGpuRejection::BufferBudget)
        );
        assert_eq!(slot_cap(&settings, 0, &clamps), Ok(0));
        assert_eq!(
            slot_cap(
                &ParticleLightGpuSettings::default(),
                1,
                &ParticleLightRealizationSettings {
                    max_lumens: f32::NAN,
                    ..default()
                }
            ),
            Err(ParticleLightGpuRejection::InvalidClamps)
        );
    }
    #[test]
    fn reserved_pool_reuses_shrinks_recovers_and_preserves_host_lights() {
        let mut world = World::new();
        world.insert_resource(ParticleLightMode::SameFrameGpu);
        world.insert_resource(AestraParticleLightSettings {
            max_lights: 4,
            ..default()
        });
        world.insert_resource(ParticleLightGpuSettings {
            max_lights: 3,
            ..default()
        });
        world.init_resource::<ParticleLightRealizationSettings>();
        world.init_resource::<Slots>();
        let host = world
            .spawn(PointLight {
                intensity: 123.0,
                ..default()
            })
            .id();
        reserve_slots(&mut world);
        let original = world.resource::<Slots>().0.clone();
        assert_eq!(original.len(), 3);
        reserve_slots(&mut world);
        assert_eq!(world.resource::<Slots>().0, original);
        world.despawn(original[1]);
        reserve_slots(&mut world);
        assert_eq!(world.resource::<Slots>().0.len(), 3);
        assert!(world.entities().contains(original[0]) && world.entities().contains(original[2]));
        for entity in &world.resource::<Slots>().0 {
            let light = world.get::<PointLight>(*entity).unwrap();
            assert_eq!(light.intensity, 0.0);
            assert!(!light.shadow_maps_enabled && !light.contact_shadows_enabled);
        }
        world.resource_mut::<ParticleLightGpuSettings>().max_lights = 1;
        reserve_slots(&mut world);
        assert_eq!(world.resource::<Slots>().0, vec![original[0]]);
        world.insert_resource(ParticleLightMode::PortableAsync);
        reserve_slots(&mut world);
        assert!(world.resource::<Slots>().0.is_empty());
        assert_eq!(world.get::<PointLight>(host).unwrap().intensity, 123.0);
        world.insert_resource(ParticleLightMode::SameFrameGpu);
        world
            .resource_mut::<ParticleLightGpuSettings>()
            .max_buffer_bytes = 0;
        reserve_slots(&mut world);
        assert!(world.resource::<Slots>().0.is_empty());
        world
            .resource_mut::<ParticleLightGpuSettings>()
            .max_buffer_bytes = 1024 * 1024;
        world.init_resource::<ParticleLightGpuStatistics>();
        world.insert_resource(NoRenderBridge);
        reserve_slots(&mut world);
        assert!(world.resource::<Slots>().0.is_empty());
        assert_eq!(
            world
                .resource::<ParticleLightGpuStatistics>()
                .snapshot()
                .rejection,
            Some(ParticleLightGpuRejection::UnsupportedClustering)
        );
    }
}
