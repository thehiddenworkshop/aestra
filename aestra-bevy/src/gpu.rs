//! GPU artifact packing and Bevy render-world compute integration.

mod render;

use aestra_core::{
    BlendMode, EmitterShape, FlipbookPlaybackMode, FlipbookTimeSource, PropertyEvaluationDomain,
    ScalarRange, Vec3Range,
};
use aestra_runtime::{
    CompiledCurve, CompiledGradient, CompiledVec3Curve, EffectInstance, ExecutionPlan, Instruction,
    MaterialColorPlan, RendererPlanKind, RuntimeValue, ScalarSource, VectorSource,
};
use bevy::{
    asset::{RenderAssetUsages, embedded_asset},
    camera::{
        primitives::Aabb,
        visibility::{self, RenderLayers, VisibilityClass},
    },
    ecs::schedule::IntoScheduleConfigs,
    prelude::*,
    render::{
        ExtractSchedule, MainWorld, Render, RenderApp, RenderStartup, RenderSystems,
        extract_component::{ExtractComponent, ExtractComponentPlugin},
        gpu_readback::{Readback, ReadbackComplete},
        render_asset::RenderAssets,
        render_resource::{
            BindGroup, BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries,
            BufferUsages, CachedComputePipelineId, ComputePassDescriptor,
            ComputePipelineDescriptor, DownlevelFlags, Extent3d, PipelineCache, ShaderStages,
            ShaderType, TextureDimension, TextureFormat,
            binding_types::{storage_buffer, storage_buffer_read_only},
        },
        renderer::{RenderAdapter, RenderAdapterInfo, RenderContext, RenderDevice, RenderGraph},
        storage::{GpuShaderBuffer, ShaderBuffer},
        sync_component::SyncComponent,
    },
};
use thiserror::Error;

use crate::{
    ActiveBackend, AestraRuntimeStatus, AestraSettings, EffectPlayer, EffectRenderMode,
    EffectRuntimeStatus, GpuCapabilities, GpuPresentationPrepared, TextureAssetCache,
    capabilities::select_backend,
};

pub const WESL_SHADER_PATH: &str = "embedded://aestra_bevy/shaders/aestra_simulation.wesl";
pub const WESL_RENDER_SHADER_PATH: &str =
    "embedded://aestra_bevy/shaders/aestra_sprite_render.wesl";
pub const MAX_CURVE_KEYS: usize = 8;
pub const MAX_FLIPBOOK_FRAMES: usize = 64;
const WORKGROUP_SIZE: u32 = 64;
const INDIRECT_DRAW_WORDS: usize = 4;
const INDIRECT_DRAW_BYTES: u64 = (INDIRECT_DRAW_WORDS * std::mem::size_of::<u32>()) as u64;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum GpuArtifactError {
    #[error("emitter '{0}' has no {1} instruction")]
    MissingInstruction(String, &'static str),
    #[error("{kind} has {actual} keys; the GPU profile supports at most {maximum}")]
    KeyLimit {
        kind: &'static str,
        actual: usize,
        maximum: usize,
    },
    #[error("flipbook '{name}' has {actual} frames; the GPU profile supports at most {maximum}")]
    FlipbookFrameLimit {
        name: String,
        actual: usize,
        maximum: usize,
    },
}

#[derive(Debug, Clone, Copy, Default, ShaderType)]
pub struct GpuCurve {
    pub keys: [Vec2; MAX_CURVE_KEYS],
    pub count: u32,
    pub _padding: Vec3,
}

#[derive(Debug, Clone, Copy, Default, ShaderType)]
pub struct GpuGradientKey {
    pub color: Vec4,
    pub time: f32,
    pub _padding: Vec3,
}

#[derive(Debug, Clone, Copy, Default, ShaderType)]
pub struct GpuGradient {
    pub keys: [GpuGradientKey; MAX_CURVE_KEYS],
    pub count: u32,
    pub _padding: Vec3,
}

#[derive(Debug, Clone, Copy, Default, ShaderType)]
pub struct GpuEmitter {
    pub slot_offset: u32,
    pub max_particles: u32,
    pub burst_count: u32,
    pub shape_kind: u32,
    pub start_time: f32,
    pub duration: f32,
    pub source_offset: f32,
    pub source_duration: f32,
    pub spawn_rate: Vec2,
    pub spawn_rate_source: u32,
    pub seed_index: u32,
    pub spawn_rate_curve: GpuCurve,
    pub shape_radius: f32,
    pub shape_depth: f32,
    pub shape_extent_z: f32,
    pub spread_radians: f32,
    pub drag: Vec2,
    pub drag_source: u32,
    pub _drag_padding: u32,
    pub drag_curve: GpuCurve,
    pub direction: Vec3,
    pub _direction_padding: f32,
    pub lifetime: Vec2,
    pub speed: Vec2,
    pub angular_velocity: Vec2,
    pub _range_padding: Vec2,
    pub gravity: Vec3,
    pub gravity_source: u32,
    pub gravity_max: Vec3,
    pub _gravity_max_padding: f32,
    pub gravity_curves: [GpuCurve; 3],
    pub turbulence: Vec2,
    pub turbulence_source: u32,
    pub _turbulence_padding: u32,
    pub turbulence_curve: GpuCurve,
    pub translation: Vec3,
    pub max_scale: f32,
    pub rotation: Vec4,
    pub scale: Vec3,
    pub _transform_padding: f32,
    pub size: GpuCurve,
    pub opacity: GpuCurve,
    pub color: GpuGradient,
}

/// One authored presentation path for an emitter.
#[derive(Debug, Clone, Copy, ShaderType)]
pub struct GpuRenderer {
    pub emitter_index: u32,
    pub blend_mode: u32,
    pub softness: f32,
    pub textured: u32,
    pub uv_min: Vec2,
    pub uv_max: Vec2,
    pub tint: Vec4,
    pub particle_color: u32,
    pub renderer_kind: u32,
    pub frame_count: u32,
    pub playback_mode: u32,
    pub flipbook_flags: u32,
    pub frame_rate: f32,
    pub _padding: Vec3,
    pub frames: [Vec4; MAX_FLIPBOOK_FRAMES],
}

/// Selects the renderer record used by one indirect draw.
#[derive(Debug, Clone, Copy, Default, ShaderType)]
pub struct GpuRenderParams {
    pub renderer_index: u32,
    pub alive_offset: u32,
    pub _padding: UVec2,
}

#[derive(Debug, Clone, Copy, Default, ShaderType)]
pub struct GpuGlobals {
    pub time: f32,
    pub total_slots: u32,
    pub seed: u32,
    pub emitter_count: u32,
    pub duration: f32,
    pub continuous: u32,
    pub _padding: UVec2,
}

#[derive(Debug, Clone, Copy, Default, ShaderType)]
pub struct GpuRenderGlobals {
    pub world_from_effect: Mat4,
    pub time: f32,
    pub seed: u32,
    pub _padding: Vec2,
}

/// Stable storage/readback ABI shared with `aestra_simulation.wesl`.
#[derive(Debug, Clone, Copy, Default, ShaderType)]
pub struct GpuParticle {
    pub color: Vec4,
    pub position: Vec3,
    pub size: f32,
    pub rotation: f32,
    pub normalized_age: f32,
    pub emitter_index: u32,
    pub alive: u32,
    pub particle_index: u32,
    pub _padding_0: u32,
    pub _padding_1: u32,
    pub _padding_2: u32,
}

#[derive(Debug, Clone)]
pub struct GpuEffectArtifact {
    pub emitters: Vec<GpuEmitter>,
    pub renderers: Vec<GpuRenderer>,
    pub particles: Vec<GpuParticle>,
    pub total_slots: u32,
    pub bounds_half_extents: Vec3,
}

impl GpuEffectArtifact {
    pub fn from_instance(instance: &EffectInstance) -> Result<Self, GpuArtifactError> {
        for flipbook in &instance.effect().flipbooks {
            if flipbook.frames.len() > MAX_FLIPBOOK_FRAMES {
                return Err(GpuArtifactError::FlipbookFrameLimit {
                    name: flipbook.name.clone(),
                    actual: flipbook.frames.len(),
                    maximum: MAX_FLIPBOOK_FRAMES,
                });
            }
        }
        let parameters = instance.parameter_values();
        let mut slot_offset = 0_u32;
        let mut bounds_half_extents = Vec3::splat(0.01);
        let mut emitters = Vec::with_capacity(instance.effect().emitters.len());
        let mut renderers = Vec::new();
        for (emitter_index, emitter) in instance.effect().emitters.iter().enumerate() {
            let (spawn_rate, burst_count) =
                emission(&emitter.execution, parameters).ok_or_else(|| {
                    GpuArtifactError::MissingInstruction(emitter.name.clone(), "Emit")
                })?;
            let shape = shape(&emitter.execution, parameters).ok_or_else(|| {
                GpuArtifactError::MissingInstruction(emitter.name.clone(), "SampleShape")
            })?;
            let init = initialize(&emitter.execution, parameters).ok_or_else(|| {
                GpuArtifactError::MissingInstruction(emitter.name.clone(), "Initialize")
            })?;
            let motion = motion(&emitter.execution, parameters).unwrap_or_default();
            let appearance = appearance(&emitter.execution, parameters).ok_or_else(|| {
                GpuArtifactError::MissingInstruction(emitter.name.clone(), "Appearance")
            })?;
            let (shape_kind, shape_radius, shape_depth, shape_extent_z) = match shape {
                EmitterShape::Point => (0, 0.0, 0.0, 0.0),
                EmitterShape::Circle { radius } => (1, radius, 0.0, 0.0),
                EmitterShape::Ring { radius } => (2, radius, 0.0, 0.0),
                EmitterShape::Sphere { radius } => (3, radius, 0.0, 0.0),
                EmitterShape::Hemisphere { radius } => (4, radius, 0.0, 0.0),
                EmitterShape::Box { half_extents } => {
                    (5, half_extents[0], half_extents[1], half_extents[2])
                }
                EmitterShape::Cylinder { radius, depth } => (6, radius, depth, 0.0),
                EmitterShape::Cone { radius, depth } => (7, radius, depth, 0.0),
            };
            if emitter.enabled {
                renderers.extend(emitter.renderers.iter().map(|renderer| {
                    let material = instance
                        .effect()
                        .material(renderer.material)
                        .expect("compiler guarantees renderer material references");
                    let (tint, particle_color) = match &material.color {
                        MaterialColorPlan::ParticleColor => ([1.0; 4], 1),
                        MaterialColorPlan::Value(value) => (*value.resolve(parameters), 0),
                    };
                    let mut frames = [Vec4::new(0.0, 0.0, 1.0, 1.0); MAX_FLIPBOOK_FRAMES];
                    let (
                        renderer_kind,
                        frame_count,
                        playback_mode,
                        flipbook_flags,
                        frame_rate,
                        texture,
                    ) = match &renderer.kind {
                        RendererPlanKind::Sprite => (0, 1, 0, 0, 0.0, material.texture),
                        RendererPlanKind::Flipbook {
                            flipbook,
                            time_source,
                            playback,
                            random_start,
                        } => {
                            let flipbook = instance
                                .effect()
                                .flipbook(*flipbook)
                                .expect("compiler guarantees flipbook references");
                            for (target, frame) in frames.iter_mut().zip(&flipbook.frames) {
                                *target = Vec4::new(
                                    frame.min[0],
                                    frame.min[1],
                                    frame.max[0],
                                    frame.max[1],
                                );
                            }
                            let mut flags = 0;
                            if *time_source == FlipbookTimeSource::EffectTime {
                                flags |= 1;
                            }
                            if *random_start {
                                flags |= 2;
                            }
                            if flipbook.looping {
                                flags |= 4;
                            }
                            (
                                1,
                                flipbook.frames.len() as u32,
                                match playback {
                                    FlipbookPlaybackMode::Forward => 0,
                                    FlipbookPlaybackMode::Reverse => 1,
                                    FlipbookPlaybackMode::PingPong => 2,
                                },
                                flags,
                                flipbook.frame_rate,
                                Some(flipbook.texture),
                            )
                        }
                    };
                    GpuRenderer {
                        emitter_index: emitter_index as u32,
                        blend_mode: match material.blend {
                            BlendMode::Alpha => GpuBlend::Alpha as u32,
                            BlendMode::Additive => GpuBlend::Additive as u32,
                            BlendMode::Multiply => GpuBlend::Multiply as u32,
                        },
                        softness: *material.softness.resolve(parameters),
                        textured: u32::from(texture.is_some()),
                        uv_min: Vec2::from_array(material.uv.min),
                        uv_max: Vec2::from_array(material.uv.max),
                        tint: Vec4::from_array(tint),
                        particle_color,
                        renderer_kind,
                        frame_count,
                        playback_mode,
                        flipbook_flags,
                        frame_rate,
                        _padding: Vec3::ZERO,
                        frames,
                    }
                }));
            }
            let local_bounds = emitter_bounds(
                shape,
                init.lifetime,
                init.speed,
                maximum_absolute_vector_source(motion.gravity),
                maximum_absolute_scalar_source(motion.turbulence),
                appearance.size,
            );
            bounds_half_extents = bounds_half_extents.max(transformed_emitter_bounds(
                local_bounds,
                emitter.transform.translation,
                emitter.transform.rotation,
                emitter.transform.scale,
            ));
            let rotation = Quat::from_array(emitter.transform.rotation).normalize();
            let scale = Vec3::from_array(emitter.transform.scale);
            let (spawn_rate, spawn_rate_source, spawn_rate_curve) = if emitter.enabled {
                pack_scalar_source(spawn_rate)?
            } else {
                (Vec2::ZERO, 0, GpuCurve::default())
            };
            let (drag, drag_source, drag_curve) = pack_scalar_source(motion.drag)?;
            let (turbulence, turbulence_source, turbulence_curve) =
                pack_scalar_source(motion.turbulence)?;
            let (gravity, gravity_source, gravity_max, gravity_curves) =
                pack_vector_source(motion.gravity)?;
            emitters.push(GpuEmitter {
                slot_offset,
                max_particles: emitter.max_particles,
                burst_count: if emitter.enabled { burst_count } else { 0 },
                shape_kind,
                start_time: emitter.start_time,
                duration: emitter.duration,
                source_offset: emitter.source_offset,
                source_duration: emitter.source_duration,
                spawn_rate,
                spawn_rate_source,
                seed_index: emitter.seed_index,
                spawn_rate_curve,
                shape_radius,
                shape_depth,
                shape_extent_z,
                spread_radians: init.spread_degrees.to_radians(),
                drag,
                drag_source,
                _drag_padding: 0,
                drag_curve,
                direction: Vec3::from_array(init.direction).normalize_or_zero(),
                _direction_padding: 0.0,
                lifetime: Vec2::new(init.lifetime.min, init.lifetime.max),
                speed: Vec2::new(init.speed.min, init.speed.max),
                angular_velocity: Vec2::new(init.angular_velocity.min, init.angular_velocity.max),
                _range_padding: Vec2::ZERO,
                gravity,
                gravity_source,
                gravity_max,
                _gravity_max_padding: 0.0,
                gravity_curves,
                turbulence,
                turbulence_source,
                _turbulence_padding: 0,
                turbulence_curve,
                translation: Vec3::from_array(emitter.transform.translation),
                max_scale: scale.max_element(),
                rotation: Vec4::from_array(rotation.to_array()),
                scale,
                _transform_padding: 0.0,
                size: pack_curve(appearance.size)?,
                opacity: pack_curve(appearance.opacity)?,
                color: pack_gradient(appearance.color)?,
            });
            slot_offset = slot_offset.saturating_add(emitter.max_particles);
        }
        Ok(Self {
            emitters,
            renderers,
            particles: vec![GpuParticle::default(); slot_offset as usize],
            total_slots: slot_offset,
            bounds_half_extents,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
enum GpuBlend {
    Alpha = 0,
    Additive = 1,
    Multiply = 2,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
enum GpuRenderMode {
    #[default]
    Rendered,
    Wireframe,
}

#[derive(Component, Clone, ExtractComponent)]
pub(crate) struct GpuEffectBuffers {
    emitters: Handle<ShaderBuffer>,
    renderers: Handle<ShaderBuffer>,
    particles: Handle<ShaderBuffer>,
    alive: Handle<ShaderBuffer>,
    dead: Handle<ShaderBuffer>,
    counters: Handle<ShaderBuffer>,
    indirect: Handle<ShaderBuffer>,
    globals: Handle<ShaderBuffer>,
    render_globals: Handle<ShaderBuffer>,
    workgroups: u32,
    total_slots: u32,
}

#[derive(Component, Clone)]
#[require(Transform, Visibility, VisibilityClass)]
#[component(on_add = visibility::add_visibility_class::<GpuDrawInstance>)]
struct GpuDrawInstance {
    renderers: Handle<ShaderBuffer>,
    particles: Handle<ShaderBuffer>,
    alive: Handle<ShaderBuffer>,
    indirect: Handle<ShaderBuffer>,
    render_globals: Handle<ShaderBuffer>,
    render_params: Handle<ShaderBuffer>,
    texture: Handle<Image>,
    fallback_texture: Handle<Image>,
    renderer_order: u32,
    indirect_offset: u64,
    blend: GpuBlend,
    render_mode: GpuRenderMode,
    mesh_center: Vec3,
}

impl SyncComponent for GpuDrawInstance {
    type Target = Self;
}

impl ExtractComponent for GpuDrawInstance {
    type QueryData = (
        &'static Self,
        &'static ViewVisibility,
        &'static GlobalTransform,
        &'static Aabb,
    );
    type QueryFilter = ();
    type Out = Self;

    fn extract_component(
        (instance, visibility, transform, bounds): bevy::ecs::query::QueryItem<
            '_,
            '_,
            Self::QueryData,
        >,
    ) -> Option<Self::Out> {
        visibility.get().then(|| {
            let mut extracted = instance.clone();
            extracted.mesh_center = gpu_draw_mesh_center(transform, bounds);
            extracted
        })
    }
}

fn gpu_draw_mesh_center(transform: &GlobalTransform, bounds: &Aabb) -> Vec3 {
    transform.transform_point(Vec3::from(bounds.center))
}

#[derive(Component)]
pub(crate) struct GpuReadbackOwner(Entity);

#[derive(Component)]
struct GpuBindGroup(BindGroup);

#[derive(Resource)]
pub(crate) struct GpuFallbackTextures {
    pub(crate) white: Handle<Image>,
    pub(crate) missing: Handle<Image>,
}

type UnpreparedPlayers<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static EffectPlayer,
        &'static EffectRuntimeStatus,
        Option<&'static RenderLayers>,
    ),
    (Without<GpuEffectBuffers>, Without<GpuPresentationPrepared>),
>;

type PreparedGpuPlayers<'w, 's> = Query<
    'w,
    's,
    (
        &'static EffectPlayer,
        &'static GlobalTransform,
        &'static GpuEffectBuffers,
        Option<&'static RenderLayers>,
        Option<&'static Children>,
    ),
    Without<GpuDrawInstance>,
>;

#[derive(Resource)]
struct SimulationPipeline {
    layout: BindGroupLayoutDescriptor,
    reset: CachedComputePipelineId,
    simulate: CachedComputePipelineId,
}

pub(crate) fn install(app: &mut App) {
    embedded_asset!(app, "shaders/aestra_simulation.wesl");
    embedded_asset!(app, "shaders/aestra_sprite_render.wesl");
    app.add_plugins((
        ExtractComponentPlugin::<GpuEffectBuffers>::default(),
        ExtractComponentPlugin::<GpuDrawInstance>::default(),
    ))
    .add_systems(Startup, init_fallback_textures)
    .add_systems(Update, update_gpu_inputs.after(crate::play_effects));
    let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
        let capabilities = GpuCapabilities::unavailable("Bevy has no render sub-application");
        let requested = app.world().resource::<AestraSettings>().presentation;
        let status = select_backend(requested, &capabilities);
        app.insert_resource(capabilities).insert_resource(status);
        return;
    };
    render_app
        .add_systems(ExtractSchedule, publish_gpu_capabilities)
        .add_systems(RenderStartup, init_pipeline)
        .add_systems(
            Render,
            prepare_bind_groups.in_set(RenderSystems::PrepareBindGroups),
        )
        .add_systems(RenderGraph, run_simulation);
    render::install(render_app);
}

fn init_fallback_textures(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let white = images.add(Image::new_fill(
        Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[255, 255, 255, 255],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    ));
    let missing = images.add(Image::new_fill(
        Extent3d {
            width: 2,
            height: 2,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[
            255, 0, 255, 255, 24, 8, 28, 255, 24, 8, 28, 255, 255, 0, 255, 255,
        ],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    ));
    commands.insert_resource(GpuFallbackTextures { white, missing });
}

pub(crate) fn prepare_gpu_players(
    mut commands: Commands,
    mut buffers: ResMut<Assets<ShaderBuffer>>,
    asset_server: Res<AssetServer>,
    mut texture_cache: ResMut<TextureAssetCache>,
    fallback_textures: Res<GpuFallbackTextures>,
    players: UnpreparedPlayers,
) {
    for (entity, player, runtime, render_layers) in &players {
        if !matches!(
            runtime.active,
            ActiveBackend::Gpu | ActiveBackend::GpuReadback
        ) {
            continue;
        }
        let artifact = match GpuEffectArtifact::from_instance(&player.instance) {
            Ok(artifact) => artifact,
            Err(error) => {
                commands.entity(entity).insert(EffectRuntimeStatus {
                    active: ActiveBackend::CpuReference,
                    reason: format!(
                        "GPU artifact is unsupported ({error}); using the CPU reference"
                    ),
                });
                continue;
            }
        };
        if artifact.total_slots == 0 {
            commands.entity(entity).insert(GpuPresentationPrepared);
            continue;
        }
        let renderer_draws = artifact
            .renderers
            .iter()
            .enumerate()
            .zip(
                player
                    .effect()
                    .emitters
                    .iter()
                    .filter(|emitter| emitter.enabled)
                    .flat_map(|emitter| emitter.renderers.iter()),
            )
            .map(|((index, renderer), plan)| {
                let material = player
                    .effect()
                    .material(plan.material)
                    .expect("compiler guarantees renderer material references");
                let texture = match &plan.kind {
                    RendererPlanKind::Sprite => material.texture,
                    RendererPlanKind::Flipbook { flipbook, .. } => player
                        .effect()
                        .flipbook(*flipbook)
                        .map(|flipbook| flipbook.texture),
                };
                let texture_path = texture.and_then(|texture| {
                    player
                        .effect()
                        .assets
                        .iter()
                        .find(|asset| asset.source == texture)
                        .map(|asset| asset.path.clone())
                });
                let (texture, fallback_texture) = texture_path.map_or_else(
                    || {
                        (
                            fallback_textures.white.clone(),
                            fallback_textures.white.clone(),
                        )
                    },
                    |path| {
                        (
                            texture_cache.load(&asset_server, &path),
                            fallback_textures.missing.clone(),
                        )
                    },
                );
                (
                    index as u32,
                    renderer.emitter_index,
                    artifact.emitters[renderer.emitter_index as usize].slot_offset,
                    match renderer.blend_mode {
                        mode if mode == GpuBlend::Additive as u32 => GpuBlend::Additive,
                        mode if mode == GpuBlend::Multiply as u32 => GpuBlend::Multiply,
                        _ => GpuBlend::Alpha,
                    },
                    texture,
                    fallback_texture,
                )
            })
            .collect::<Vec<_>>();
        let bounds = Aabb {
            center: Vec3A::ZERO,
            half_extents: Vec3A::from(artifact.bounds_half_extents),
        };
        let indirect_draw_commands = indirect_draw_commands(&artifact.emitters);
        let emitters = buffers.add(ShaderBuffer::from(artifact.emitters));
        let renderers = buffers.add(ShaderBuffer::from(artifact.renderers));
        let particles = buffers.add(ShaderBuffer::from(artifact.particles));
        let alive = buffers.add(ShaderBuffer::from(vec![
            0_u32;
            artifact.total_slots as usize
        ]));
        let dead = buffers.add(ShaderBuffer::from(vec![
            0_u32;
            artifact.total_slots as usize
        ]));
        let counters = buffers.add(ShaderBuffer::from(vec![0_u32; 2]));
        let mut indirect_buffer = ShaderBuffer::from(indirect_draw_commands);
        indirect_buffer.buffer_description.usage |= BufferUsages::INDIRECT;
        let indirect = buffers.add(indirect_buffer);
        let globals = buffers.add(ShaderBuffer::from(GpuGlobals {
            time: player.simulation_time(),
            total_slots: artifact.total_slots,
            seed: fold_seed(player.instance.seed()),
            emitter_count: player.effect().emitters.len() as u32,
            duration: player.effect().duration,
            continuous: u32::from(player.effect().playback_mode.is_continuous()),
            _padding: UVec2::ZERO,
        }));
        let render_globals = buffers.add(ShaderBuffer::from(GpuRenderGlobals {
            world_from_effect: Mat4::IDENTITY,
            time: player.simulation_time(),
            seed: fold_seed(player.instance.seed()),
            _padding: Vec2::ZERO,
        }));
        commands.entity(entity).insert((
            GpuEffectBuffers {
                emitters: emitters.clone(),
                renderers: renderers.clone(),
                particles: particles.clone(),
                alive: alive.clone(),
                dead,
                counters,
                indirect: indirect.clone(),
                globals,
                render_globals: render_globals.clone(),
                workgroups: artifact.total_slots.div_ceil(WORKGROUP_SIZE),
                total_slots: artifact.total_slots,
            },
            GpuPresentationPrepared,
        ));
        let render_mode = gpu_render_mode(player.render_mode());
        commands
            .entity(entity)
            .with_children(|parent| match runtime.active {
                ActiveBackend::Gpu => {
                    for (
                        renderer_index,
                        emitter_index,
                        alive_offset,
                        blend,
                        texture,
                        fallback_texture,
                    ) in renderer_draws
                    {
                        let render_params = buffers.add(ShaderBuffer::from(GpuRenderParams {
                            renderer_index,
                            alive_offset,
                            _padding: UVec2::ZERO,
                        }));
                        parent.spawn((
                            GpuDrawInstance {
                                renderers: renderers.clone(),
                                particles: particles.clone(),
                                alive: alive.clone(),
                                indirect: indirect.clone(),
                                render_globals: render_globals.clone(),
                                render_params,
                                texture,
                                fallback_texture,
                                renderer_order: renderer_index,
                                indirect_offset: indirect_draw_offset(emitter_index),
                                blend,
                                render_mode,
                                mesh_center: Vec3::ZERO,
                            },
                            render_layers.cloned().unwrap_or_default(),
                            bounds,
                            Transform::default(),
                            Visibility::Inherited,
                        ));
                    }
                }
                ActiveBackend::GpuReadback => {
                    parent.spawn((
                        Readback::buffer(particles.clone()),
                        GpuReadbackOwner(entity),
                    ));
                }
                ActiveBackend::Pending | ActiveBackend::CpuReference => {
                    unreachable!("non-GPU players do not allocate GPU buffers")
                }
            });
    }
}

fn publish_gpu_capabilities(
    render_device: Res<RenderDevice>,
    adapter: Res<RenderAdapter>,
    adapter_info: Res<RenderAdapterInfo>,
    mut main_world: ResMut<MainWorld>,
) {
    let capabilities = detect_gpu_capabilities(&render_device, &adapter, &adapter_info);
    let requested = main_world.resource::<AestraSettings>().presentation;
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
}

fn detect_gpu_capabilities(
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
    if limits.max_storage_buffers_per_shader_stage < 7 {
        limitations.push(format!(
            "{} storage buffers per shader stage are available; 7 are required",
            limits.max_storage_buffers_per_shader_stage
        ));
    }
    if limits.max_bindings_per_bind_group < 7 {
        limitations.push(format!(
            "{} bindings per group are available; 7 are required",
            limits.max_bindings_per_bind_group
        ));
    }
    if max_particles == 0 {
        limitations.push("storage or dispatch limits allow no particles".into());
    }
    let compute_pipeline_supported = compute_shaders
        && limits.max_compute_invocations_per_workgroup >= WORKGROUP_SIZE
        && limits.max_compute_workgroup_size_x >= WORKGROUP_SIZE
        && limits.max_storage_buffers_per_shader_stage >= 7
        && limits.max_bindings_per_bind_group >= 7
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
        max_buffer_size: limits.max_buffer_size,
        max_compute_workgroups_per_dimension: limits.max_compute_workgroups_per_dimension,
        max_particles,
        limitations,
    }
}

fn update_gpu_inputs(
    mut buffers: ResMut<Assets<ShaderBuffer>>,
    players: PreparedGpuPlayers,
    mut draw_instances: Query<(&mut GpuDrawInstance, &mut RenderLayers), Without<EffectPlayer>>,
) {
    for (player, transform, gpu, render_layers, children) in &players {
        if let Ok(artifact) = GpuEffectArtifact::from_instance(&player.instance) {
            if let Some(mut buffer) = buffers.get_mut(&gpu.emitters) {
                buffer.set_data(artifact.emitters);
            }
            if let Some(mut buffer) = buffers.get_mut(&gpu.renderers) {
                buffer.set_data(artifact.renderers);
            }
        }
        if let Some(mut buffer) = buffers.get_mut(&gpu.globals) {
            buffer.set_data(GpuGlobals {
                time: player.simulation_time(),
                total_slots: gpu.total_slots,
                seed: fold_seed(player.instance.seed()),
                emitter_count: player.effect().emitters.len() as u32,
                duration: player.effect().duration,
                continuous: u32::from(player.effect().playback_mode.is_continuous()),
                _padding: UVec2::ZERO,
            });
        }
        if let Some(mut buffer) = buffers.get_mut(&gpu.render_globals) {
            buffer.set_data(GpuRenderGlobals {
                world_from_effect: Mat4::from(transform.affine()),
                time: player.simulation_time(),
                seed: fold_seed(player.instance.seed()),
                _padding: Vec2::ZERO,
            });
        }
        if let Some(children) = children {
            let render_mode = gpu_render_mode(player.render_mode());
            for child in children.iter() {
                if let Ok((mut draw, mut draw_layers)) = draw_instances.get_mut(child) {
                    draw.render_mode = render_mode;
                    *draw_layers = render_layers.cloned().unwrap_or_default();
                }
            }
        }
    }
}

pub(crate) fn receive_readback(
    event: On<ReadbackComplete>,
    owners: Query<&GpuReadbackOwner>,
    mut players: Query<&mut EffectPlayer>,
) {
    let Ok(owner) = owners.get(event.event_target()) else {
        return;
    };
    let Ok(mut player) = players.get_mut(owner.0) else {
        return;
    };
    let particles: Vec<GpuParticle> = event.to_shader_type();
    player.gpu_samples.clear();
    player.gpu_samples.extend(
        particles
            .into_iter()
            .filter(|particle| particle.alive != 0)
            .map(|particle| aestra_runtime::ParticleSample {
                emitter_index: particle.emitter_index as usize,
                particle_index: particle.particle_index,
                position: particle.position.to_array(),
                size: particle.size,
                rotation: particle.rotation,
                color: particle.color.to_array(),
                normalized_age: particle.normalized_age,
            }),
    );
}

fn init_pipeline(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    pipeline_cache: Res<PipelineCache>,
) {
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
        shader,
        entry_point: Some("simulate".into()),
        ..default()
    });
    commands.insert_resource(SimulationPipeline {
        layout,
        reset,
        simulate,
    });
}

fn prepare_bind_groups(
    mut commands: Commands,
    pipeline: Res<SimulationPipeline>,
    render_device: Res<RenderDevice>,
    pipeline_cache: Res<PipelineCache>,
    buffers: Res<RenderAssets<GpuShaderBuffer>>,
    effects: Query<(Entity, &GpuEffectBuffers)>,
) {
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
            )),
        );
        commands.entity(entity).insert(GpuBindGroup(bind_group));
    }
}

fn run_simulation(
    mut render_context: RenderContext,
    pipeline_cache: Res<PipelineCache>,
    pipeline: Res<SimulationPipeline>,
    effects: Query<(&GpuEffectBuffers, &GpuBindGroup)>,
) {
    let (Some(reset), Some(simulate)) = (
        pipeline_cache.get_compute_pipeline(pipeline.reset),
        pipeline_cache.get_compute_pipeline(pipeline.simulate),
    ) else {
        return;
    };
    for (effect, bind_group) in &effects {
        let mut pass =
            render_context
                .command_encoder()
                .begin_compute_pass(&ComputePassDescriptor {
                    label: Some("aestra simulation"),
                    ..default()
                });
        pass.set_bind_group(0, &bind_group.0, &[]);
        pass.set_pipeline(reset);
        pass.dispatch_workgroups(1, 1, 1);
        pass.set_pipeline(simulate);
        pass.dispatch_workgroups(effect.workgroups, 1, 1);
    }
}

fn fold_seed(seed: u64) -> u32 {
    seed as u32 ^ (seed >> 32) as u32
}

fn gpu_render_mode(mode: EffectRenderMode) -> GpuRenderMode {
    match mode {
        EffectRenderMode::Rendered => GpuRenderMode::Rendered,
        EffectRenderMode::Wireframe => GpuRenderMode::Wireframe,
    }
}

fn indirect_draw_commands(emitters: &[GpuEmitter]) -> Vec<u32> {
    emitters.iter().flat_map(|_| [6, 0, 0, 0]).collect()
}

const fn indirect_draw_offset(emitter_index: u32) -> u64 {
    emitter_index as u64 * INDIRECT_DRAW_BYTES
}

fn emitter_bounds(
    shape: EmitterShape,
    lifetime: ScalarRange,
    speed: ScalarRange,
    gravity: [f32; 3],
    turbulence: f32,
    size: &CompiledCurve,
) -> Vec3 {
    let shape_extents = match shape {
        EmitterShape::Point => Vec3::ZERO,
        EmitterShape::Circle { radius } | EmitterShape::Ring { radius } => {
            Vec3::new(radius.abs(), radius.abs(), 0.0)
        }
        EmitterShape::Sphere { radius } | EmitterShape::Hemisphere { radius } => {
            Vec3::splat(radius.abs())
        }
        EmitterShape::Box { half_extents } => Vec3::from_array(half_extents).abs(),
        EmitterShape::Cylinder { radius, depth } => {
            Vec3::new(radius.abs(), depth.abs() * 0.5, radius.abs())
        }
        EmitterShape::Cone { radius, depth } => Vec3::new(radius.abs(), depth.abs(), radius.abs()),
    };
    let lifetime = lifetime.min.abs().max(lifetime.max.abs());
    let speed = speed.min.abs().max(speed.max.abs());
    let travel = speed * lifetime;
    let size = size
        .first()
        .map_or(0.0, |(_, value)| value.abs())
        .max(size.last_value().abs())
        .max(
            size.segments()
                .iter()
                .map(|segment| segment.start_value.abs().max(segment.end_value.abs()))
                .fold(0.0, f32::max),
        )
        * 0.5;
    shape_extents
        + Vec3::new(
            travel + turbulence.abs() + gravity[0].abs() * lifetime * lifetime * 0.5 + size,
            travel + turbulence.abs() + gravity[1].abs() * lifetime * lifetime * 0.5 + size,
            travel + turbulence.abs() + gravity[2].abs() * lifetime * lifetime * 0.5 + size,
        )
}

fn transformed_emitter_bounds(
    local: Vec3,
    translation: [f32; 3],
    rotation: [f32; 4],
    scale: [f32; 3],
) -> Vec3 {
    let scaled = local * Vec3::from_array(scale).abs();
    let rotation = Quat::from_array(rotation).normalize();
    let rotated = (rotation * Vec3::X * scaled.x).abs()
        + (rotation * Vec3::Y * scaled.y).abs()
        + (rotation * Vec3::Z * scaled.z).abs();
    Vec3::from_array(translation).abs() + rotated
}

fn pack_curve(curve: &CompiledCurve) -> Result<GpuCurve, GpuArtifactError> {
    let mut points = Vec::new();
    if let Some(first) = curve.first() {
        points.push(first);
        points.extend(
            curve
                .segments()
                .iter()
                .map(|segment| (segment.end_time, segment.end_value)),
        );
    }
    if points.len() > MAX_CURVE_KEYS {
        return Err(GpuArtifactError::KeyLimit {
            kind: "curve",
            actual: points.len(),
            maximum: MAX_CURVE_KEYS,
        });
    }
    let mut packed = GpuCurve {
        count: points.len() as u32,
        ..default()
    };
    for (target, (time, value)) in packed.keys.iter_mut().zip(points) {
        *target = Vec2::new(time, value);
    }
    Ok(packed)
}

fn pack_gradient(gradient: &CompiledGradient) -> Result<GpuGradient, GpuArtifactError> {
    let mut points = Vec::new();
    if let Some(first) = gradient.first() {
        points.push(first);
        points.extend(
            gradient
                .segments()
                .iter()
                .map(|segment| (segment.end_time, segment.end_color)),
        );
    }
    if points.len() > MAX_CURVE_KEYS {
        return Err(GpuArtifactError::KeyLimit {
            kind: "gradient",
            actual: points.len(),
            maximum: MAX_CURVE_KEYS,
        });
    }
    let mut packed = GpuGradient {
        count: points.len() as u32,
        ..default()
    };
    for (target, (time, color)) in packed.keys.iter_mut().zip(points) {
        target.time = time;
        target.color = Vec4::from_array(color);
    }
    Ok(packed)
}

#[derive(Clone, Copy)]
enum ResolvedGpuScalarSource<'a> {
    Constant(f32),
    RandomRange(ScalarRange),
    Curve(&'a CompiledCurve, PropertyEvaluationDomain),
}

#[derive(Clone, Copy)]
enum ResolvedGpuVectorSource<'a> {
    Constant([f32; 3]),
    RandomRange(Vec3Range),
    Curve(&'a CompiledVec3Curve, PropertyEvaluationDomain),
}

fn resolve_gpu_vector_source<'a>(
    source: &'a VectorSource,
    values: &'a [RuntimeValue],
) -> ResolvedGpuVectorSource<'a> {
    match source {
        VectorSource::Constant(value) => ResolvedGpuVectorSource::Constant(*value.resolve(values)),
        VectorSource::RandomRange(value) => {
            ResolvedGpuVectorSource::RandomRange(*value.resolve(values))
        }
        VectorSource::Curve { value, domain } => {
            ResolvedGpuVectorSource::Curve(value.resolve(values), *domain)
        }
    }
}

fn maximum_absolute_curve(curve: &CompiledCurve) -> f32 {
    curve
        .first()
        .map_or(0.0, |(_, value)| value.abs())
        .max(curve.last_value().abs())
        .max(
            curve
                .segments()
                .iter()
                .map(|segment| segment.start_value.abs().max(segment.end_value.abs()))
                .fold(0.0, f32::max),
        )
}

fn maximum_absolute_vector_source(source: ResolvedGpuVectorSource<'_>) -> [f32; 3] {
    match source {
        ResolvedGpuVectorSource::Constant(value) => value.map(f32::abs),
        ResolvedGpuVectorSource::RandomRange(range) => {
            std::array::from_fn(|axis| range.min[axis].abs().max(range.max[axis].abs()))
        }
        ResolvedGpuVectorSource::Curve(curve, _) => {
            std::array::from_fn(|axis| maximum_absolute_curve(&curve.curves[axis]))
        }
    }
}

fn pack_vector_source(
    source: ResolvedGpuVectorSource<'_>,
) -> Result<(Vec3, u32, Vec3, [GpuCurve; 3]), GpuArtifactError> {
    match source {
        ResolvedGpuVectorSource::Constant(value) => Ok((
            Vec3::from_array(value),
            0,
            Vec3::from_array(value),
            [GpuCurve::default(); 3],
        )),
        ResolvedGpuVectorSource::RandomRange(range) => Ok((
            Vec3::from_array(range.min),
            1,
            Vec3::from_array(range.max),
            [GpuCurve::default(); 3],
        )),
        ResolvedGpuVectorSource::Curve(curve, domain) => Ok((
            Vec3::ZERO,
            match domain {
                PropertyEvaluationDomain::EmitterTime => 2,
                PropertyEvaluationDomain::ParticleLife => 3,
            },
            Vec3::ZERO,
            [
                pack_curve(&curve.curves[0])?,
                pack_curve(&curve.curves[1])?,
                pack_curve(&curve.curves[2])?,
            ],
        )),
    }
}

fn resolve_gpu_scalar_source<'a>(
    source: &'a ScalarSource,
    values: &'a [RuntimeValue],
) -> ResolvedGpuScalarSource<'a> {
    match source {
        ScalarSource::Constant(value) => ResolvedGpuScalarSource::Constant(*value.resolve(values)),
        ScalarSource::RandomRange(value) => {
            ResolvedGpuScalarSource::RandomRange(*value.resolve(values))
        }
        ScalarSource::Curve { value, domain } => {
            ResolvedGpuScalarSource::Curve(value.resolve(values), *domain)
        }
    }
}

fn maximum_absolute_scalar_source(source: ResolvedGpuScalarSource<'_>) -> f32 {
    match source {
        ResolvedGpuScalarSource::Constant(value) => value.abs(),
        ResolvedGpuScalarSource::RandomRange(range) => range.min.abs().max(range.max.abs()),
        ResolvedGpuScalarSource::Curve(curve, _) => maximum_absolute_curve(curve),
    }
}

fn pack_scalar_source(
    source: ResolvedGpuScalarSource<'_>,
) -> Result<(Vec2, u32, GpuCurve), GpuArtifactError> {
    match source {
        ResolvedGpuScalarSource::Constant(value) => {
            Ok((Vec2::splat(value), 0, GpuCurve::default()))
        }
        ResolvedGpuScalarSource::RandomRange(range) => {
            Ok((Vec2::new(range.min, range.max), 1, GpuCurve::default()))
        }
        ResolvedGpuScalarSource::Curve(curve, PropertyEvaluationDomain::EmitterTime) => {
            Ok((Vec2::ZERO, 2, pack_curve(curve)?))
        }
        ResolvedGpuScalarSource::Curve(curve, PropertyEvaluationDomain::ParticleLife) => {
            Ok((Vec2::ZERO, 3, pack_curve(curve)?))
        }
    }
}

fn emission<'a>(
    plan: &'a ExecutionPlan,
    values: &'a [RuntimeValue],
) -> Option<(ResolvedGpuScalarSource<'a>, u32)> {
    plan.emitter_update
        .iter()
        .find_map(|instruction| match instruction {
            Instruction::Emit {
                spawn_rate,
                burst_count,
                ..
            } => {
                let spawn_rate = resolve_gpu_scalar_source(spawn_rate, values);
                Some((spawn_rate, *burst_count.resolve(values)))
            }
            _ => None,
        })
}

fn shape(plan: &ExecutionPlan, values: &[RuntimeValue]) -> Option<EmitterShape> {
    plan.particle_spawn
        .iter()
        .find_map(|instruction| match instruction {
            Instruction::SampleShape { shape, .. } => Some(*shape.resolve(values)),
            _ => None,
        })
}

struct GpuInitialize {
    lifetime: ScalarRange,
    speed: ScalarRange,
    direction: [f32; 3],
    spread_degrees: f32,
    angular_velocity: ScalarRange,
}

fn initialize(plan: &ExecutionPlan, values: &[RuntimeValue]) -> Option<GpuInitialize> {
    plan.particle_spawn
        .iter()
        .find_map(|instruction| match instruction {
            Instruction::Initialize {
                lifetime,
                speed,
                direction,
                spread_degrees,
                angular_velocity,
                ..
            } => Some(GpuInitialize {
                lifetime: *lifetime.resolve(values),
                speed: *speed.resolve(values),
                direction: *direction.resolve(values),
                spread_degrees: *spread_degrees.resolve(values),
                angular_velocity: *angular_velocity.resolve(values),
            }),
            _ => None,
        })
}

struct GpuMotion<'a> {
    gravity: ResolvedGpuVectorSource<'a>,
    drag: ResolvedGpuScalarSource<'a>,
    turbulence: ResolvedGpuScalarSource<'a>,
}

impl Default for GpuMotion<'_> {
    fn default() -> Self {
        Self {
            gravity: ResolvedGpuVectorSource::Constant([0.0; 3]),
            drag: ResolvedGpuScalarSource::Constant(0.0),
            turbulence: ResolvedGpuScalarSource::Constant(0.0),
        }
    }
}

fn motion<'a>(plan: &'a ExecutionPlan, values: &'a [RuntimeValue]) -> Option<GpuMotion<'a>> {
    plan.particle_update
        .iter()
        .find_map(|instruction| match instruction {
            Instruction::Motion {
                gravity,
                drag,
                turbulence,
                ..
            } => Some(GpuMotion {
                gravity: resolve_gpu_vector_source(gravity, values),
                drag: resolve_gpu_scalar_source(drag, values),
                turbulence: resolve_gpu_scalar_source(turbulence, values),
            }),
            _ => None,
        })
}

struct GpuAppearance<'a> {
    size: &'a CompiledCurve,
    opacity: &'a CompiledCurve,
    color: &'a CompiledGradient,
}

fn appearance<'a>(
    plan: &'a ExecutionPlan,
    values: &'a [RuntimeValue],
) -> Option<GpuAppearance<'a>> {
    plan.particle_update
        .iter()
        .find_map(|instruction| match instruction {
            Instruction::Appearance {
                size,
                opacity,
                color,
                ..
            } => Some(GpuAppearance {
                size: size.resolve(values),
                opacity: opacity.resolve(values),
                color: color.resolve(values),
            }),
            _ => None,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use aestra_compiler::EffectCompiler;
    use aestra_core::{
        AssetDefinition, BlendMode, Curve, CurveKey, EffectAsset, Emitter, MODULE_EMISSION,
        MODULE_MOTION, MaterialDefinition, MaterialInput, MaterialProperties,
        PropertyEvaluationDomain, PropertySource, PropertySourceValue, RendererInstance, UvRect,
        Value, Vec3Curve,
    };
    use std::sync::Arc;

    #[test]
    fn extracted_draw_center_uses_the_world_space_bounds_center() {
        let transform = GlobalTransform::from(
            Transform::from_translation(Vec3::new(10.0, 20.0, 30.0)).with_scale(Vec3::splat(2.0)),
        );
        let bounds = Aabb {
            center: Vec3A::new(1.0, 2.0, 3.0),
            half_extents: Vec3A::ONE,
        };

        assert_eq!(
            gpu_draw_mesh_center(&transform, &bounds),
            Vec3::new(12.0, 24.0, 36.0)
        );
    }

    #[test]
    fn artifact_capacity_matches_authored_bounds() {
        let mut effect = EffectAsset::new("GPU", 2.0);
        let mut first = Emitter::basic_sprite("First", 2.0);
        first.max_particles = 17;
        let mut second = Emitter::basic_sprite("Second", 2.0);
        second.max_particles = 23;
        effect.emitters.extend([first, second]);
        let compiled = Arc::new(EffectCompiler::default().compile(&effect).unwrap());
        let artifact = GpuEffectArtifact::from_instance(&EffectInstance::new(compiled)).unwrap();
        assert_eq!(artifact.total_slots, 40);
        assert_eq!(artifact.emitters[0].slot_offset, 0);
        assert_eq!(artifact.emitters[1].slot_offset, 17);
        assert_eq!(artifact.particles.len(), 40);
    }

    #[test]
    fn indirect_draw_commands_are_isolated_per_emitter() {
        let mut effect = EffectAsset::new("GPU indirect ranges", 2.0);
        let mut first = Emitter::basic_sprite("First", 2.0);
        first.max_particles = 17;
        let mut second = Emitter::basic_sprite("Second", 2.0);
        second.max_particles = 23;
        effect.emitters.extend([first, second]);
        let compiled = Arc::new(EffectCompiler::default().compile(&effect).unwrap());
        let artifact = GpuEffectArtifact::from_instance(&EffectInstance::new(compiled)).unwrap();

        assert_eq!(
            indirect_draw_commands(&artifact.emitters),
            vec![6, 0, 0, 0, 6, 0, 0, 0]
        );
        assert_eq!(indirect_draw_offset(0), 0);
        assert_eq!(indirect_draw_offset(1), INDIRECT_DRAW_BYTES);
        assert_eq!(artifact.emitters[0].slot_offset, 0);
        assert_eq!(artifact.emitters[1].slot_offset, 17);
        assert_eq!(artifact.renderers[0].emitter_index, 0);
        assert_eq!(artifact.renderers[1].emitter_index, 1);
    }

    #[test]
    fn artifact_packs_native_3d_shape_motion_and_bounds() {
        let mut effect = EffectAsset::new("3D GPU", 2.0);
        let mut emitter = Emitter::basic_sprite("Volume", 2.0);
        let shape = emitter
            .modules
            .iter_mut()
            .find(|module| {
                matches!(
                    &module.parameters,
                    aestra_core::ModuleParameters::Shape { .. }
                )
            })
            .unwrap();
        shape.parameters = aestra_core::ModuleParameters::Shape {
            shape: EmitterShape::Box {
                half_extents: [2.0, 3.0, 4.0],
            },
        };
        let initialize = emitter
            .modules
            .iter_mut()
            .find(|module| {
                matches!(
                    &module.parameters,
                    aestra_core::ModuleParameters::Initialize { .. }
                )
            })
            .unwrap();
        if let aestra_core::ModuleParameters::Initialize { direction, .. } =
            &mut initialize.parameters
        {
            *direction = [1.0, 2.0, 3.0];
        }
        let motion = emitter
            .modules
            .iter_mut()
            .find(|module| {
                matches!(
                    &module.parameters,
                    aestra_core::ModuleParameters::Motion { .. }
                )
            })
            .unwrap();
        if let aestra_core::ModuleParameters::Motion { gravity, .. } = &mut motion.parameters {
            *gravity = [4.0, 5.0, 6.0];
        }
        effect.emitters.push(emitter);

        let compiled = Arc::new(EffectCompiler::default().compile(&effect).unwrap());
        let artifact = GpuEffectArtifact::from_instance(&EffectInstance::new(compiled)).unwrap();
        let emitter = artifact.emitters[0];

        assert_eq!(emitter.shape_kind, 5);
        assert_eq!(emitter.shape_radius, 2.0);
        assert_eq!(emitter.shape_depth, 3.0);
        assert_eq!(emitter.shape_extent_z, 4.0);
        assert_eq!(emitter.gravity, Vec3::new(4.0, 5.0, 6.0));
        assert!((emitter.direction.length() - 1.0).abs() < 0.0001);
        assert!(artifact.bounds_half_extents.z > 4.0);
    }

    #[test]
    fn artifact_packs_emitter_transform_and_expands_bounds() {
        let mut effect = EffectAsset::new("Transformed GPU", 2.0);
        let mut emitter = Emitter::basic_sprite("Emitter", 2.0);
        emitter.transform.translation = [100.0, 20.0, -30.0];
        emitter.transform.rotation = [
            0.0,
            0.0,
            std::f32::consts::FRAC_1_SQRT_2,
            std::f32::consts::FRAC_1_SQRT_2,
        ];
        emitter.transform.scale = [2.0, 3.0, 4.0];
        effect.emitters.push(emitter);

        let compiled = Arc::new(EffectCompiler::default().compile(&effect).unwrap());
        let artifact = GpuEffectArtifact::from_instance(&EffectInstance::new(compiled)).unwrap();
        let emitter = artifact.emitters[0];

        assert_eq!(emitter.translation, Vec3::new(100.0, 20.0, -30.0));
        assert_eq!(emitter.scale, Vec3::new(2.0, 3.0, 4.0));
        assert_eq!(emitter.max_scale, 4.0);
        assert!(artifact.bounds_half_extents.x > 100.0);
        assert!(artifact.bounds_half_extents.z > 30.0);
    }

    #[test]
    fn artifact_preserves_every_enabled_renderer() {
        let mut effect = EffectAsset::new("GPU renderers", 2.0);
        let texture = AssetDefinition::texture("Spark", "textures/spark.png");
        let texture_id = texture.id;
        let mut first = Emitter::basic_sprite("First", 2.0);
        effect.materials[0].properties = MaterialProperties::Sprite {
            softness: MaterialInput::Constant(0.5),
            color: aestra_core::SpriteColorSource::ParticleColor,
            texture: Some(texture_id),
            uv: UvRect {
                min: [0.25, 0.0],
                max: [0.75, 1.0],
            },
        };
        let mut alpha = MaterialDefinition::sprite("Alpha", BlendMode::Alpha, 0.65);
        let MaterialProperties::Sprite { color, .. } = &mut alpha.properties;
        *color =
            aestra_core::SpriteColorSource::Value(MaterialInput::Constant([0.25, 0.5, 0.75, 1.0]));
        let alpha_id = alpha.id;
        let multiply = MaterialDefinition::sprite("Multiply", BlendMode::Multiply, 0.8);
        let multiply_id = multiply.id;
        effect.materials.extend([alpha, multiply]);
        first.renderers.push(RendererInstance::sprite(alpha_id));
        first.renderers.push(RendererInstance::sprite(multiply_id));
        let mut disabled = Emitter::basic_sprite("Disabled", 2.0);
        disabled.enabled = false;
        effect.assets.push(texture);
        effect.emitters.extend([first, disabled]);

        let compiled = Arc::new(EffectCompiler::default().compile(&effect).unwrap());
        let artifact = GpuEffectArtifact::from_instance(&EffectInstance::new(compiled)).unwrap();

        assert_eq!(artifact.renderers.len(), 3);
        assert_eq!(artifact.renderers[0].emitter_index, 0);
        assert_eq!(artifact.renderers[0].blend_mode, GpuBlend::Additive as u32);
        assert_eq!(artifact.renderers[0].textured, 1);
        assert_eq!(artifact.renderers[0].uv_min, Vec2::new(0.25, 0.0));
        assert_eq!(artifact.renderers[0].uv_max, Vec2::new(0.75, 1.0));
        assert_eq!(artifact.renderers[1].emitter_index, 0);
        assert_eq!(artifact.renderers[1].blend_mode, GpuBlend::Alpha as u32);
        assert_eq!(artifact.renderers[1].softness, 0.65);
        assert_eq!(artifact.renderers[1].tint, Vec4::new(0.25, 0.5, 0.75, 1.0));
        assert_eq!(artifact.renderers[1].particle_color, 0);
        assert_eq!(artifact.renderers[2].emitter_index, 0);
        assert_eq!(artifact.renderers[2].blend_mode, GpuBlend::Multiply as u32);
        assert_eq!(artifact.renderers[2].softness, 0.8);
    }

    #[test]
    fn artifact_packs_explicit_flipbook_frames_for_wesl() {
        let effect =
            EffectAsset::from_ron(include_str!("../../assets/effects/plasma_burst.aestra.ron"))
                .unwrap();
        let compiled = Arc::new(EffectCompiler::default().compile(&effect).unwrap());
        let artifact = GpuEffectArtifact::from_instance(&EffectInstance::new(compiled)).unwrap();
        let renderer = &artifact.renderers[0];
        assert_eq!(renderer.renderer_kind, 1);
        assert_eq!(renderer.frame_count, 4);
        assert_eq!(renderer.frame_rate, 8.0);
        assert_eq!(renderer.frames[0], Vec4::new(0.0, 0.0, 0.5, 0.5));
        assert_ne!(renderer.flipbook_flags & 2, 0);
        assert_ne!(renderer.flipbook_flags & 4, 0);
    }

    #[test]
    fn gpu_hash_seed_fold_matches_cpu_contract() {
        let seed = 0x1234_5678_9abc_def0;
        assert_eq!(fold_seed(seed), 0x8888_8888);
    }

    #[test]
    fn authored_wesl_compiles_and_validates() {
        let compiler = wesl::Wesl::new(concat!(env!("CARGO_MANIFEST_DIR"), "/src/shaders"));
        let module = "package::aestra_simulation".parse().unwrap();
        let output = compiler.compile(&module).unwrap().to_string();
        assert!(output.contains("fn simulate"));
        assert!(output.contains("fn reset"));
        assert!(output.contains("emitter_index * 4u"));
        assert!(output.contains("emitter.slot_offset + compact_index"));
    }

    #[test]
    fn artifact_packs_spawn_rate_curve_sources_for_wesl() {
        let mut effect = EffectAsset::new("GPU curve rate", 2.0);
        effect
            .emitters
            .push(Emitter::basic_sprite("Emitter", effect.duration));
        let emission = effect.emitters[0]
            .modules
            .iter_mut()
            .find(|module| module.module_type.0 == MODULE_EMISSION)
            .unwrap();
        let source = PropertySource::Curve(PropertyEvaluationDomain::EmitterTime);
        emission
            .property_sources
            .insert("spawn_rate".into(), source);
        emission.property_source_values.insert(
            "spawn_rate".into(),
            vec![PropertySourceValue::new(
                source,
                Value::Curve(Curve::normalized(
                    vec![CurveKey::new(0.0, 0.0), CurveKey::new(1.0, 1.0)],
                    ScalarRange::new(2.0, 20.0),
                )),
            )],
        );

        let compiled = Arc::new(EffectCompiler::default().compile(&effect).unwrap());
        let artifact = GpuEffectArtifact::from_instance(&EffectInstance::new(compiled)).unwrap();
        let emitter = artifact.emitters[0];

        assert_eq!(emitter.spawn_rate_source, 2);
        assert_eq!(emitter.spawn_rate_curve.count, 2);
        assert_eq!(emitter.spawn_rate_curve.keys[0], Vec2::new(0.0, 2.0));
        assert_eq!(emitter.spawn_rate_curve.keys[1], Vec2::new(1.0, 20.0));
    }

    #[test]
    fn artifact_packs_motion_curve_and_random_sources_for_wesl() {
        let mut effect = EffectAsset::new("GPU motion sources", 2.0);
        effect
            .emitters
            .push(Emitter::basic_sprite("Curve", effect.duration));
        effect
            .emitters
            .push(Emitter::basic_sprite("Random", effect.duration));

        let motion = effect.emitters[0]
            .modules
            .iter_mut()
            .find(|module| module.module_type.0 == MODULE_MOTION)
            .unwrap();
        let curve_source = PropertySource::Curve(PropertyEvaluationDomain::ParticleLife);
        motion.property_sources.insert("drag".into(), curve_source);
        motion.property_source_values.insert(
            "drag".into(),
            vec![PropertySourceValue::new(
                curve_source,
                Value::Curve(Curve::normalized(
                    vec![CurveKey::new(0.0, 0.0), CurveKey::new(1.0, 1.0)],
                    ScalarRange::new(0.0, 4.0),
                )),
            )],
        );
        motion
            .property_sources
            .insert("turbulence".into(), PropertySource::RandomRange);
        motion.property_source_values.insert(
            "turbulence".into(),
            vec![PropertySourceValue::new(
                PropertySource::RandomRange,
                Value::Range(ScalarRange::new(1.0, 5.0)),
            )],
        );
        motion
            .property_sources
            .insert("gravity".into(), curve_source);
        motion.property_source_values.insert(
            "gravity".into(),
            vec![PropertySourceValue::new(
                curve_source,
                Value::Vec3Curve(Vec3Curve::constant([2.0, -4.0, 6.0])),
            )],
        );

        let motion = effect.emitters[1]
            .modules
            .iter_mut()
            .find(|module| module.module_type.0 == MODULE_MOTION)
            .unwrap();
        motion
            .property_sources
            .insert("drag".into(), PropertySource::RandomRange);
        motion.property_source_values.insert(
            "drag".into(),
            vec![PropertySourceValue::new(
                PropertySource::RandomRange,
                Value::Range(ScalarRange::new(0.25, 1.5)),
            )],
        );
        motion
            .property_sources
            .insert("turbulence".into(), curve_source);
        motion.property_source_values.insert(
            "turbulence".into(),
            vec![PropertySourceValue::new(
                curve_source,
                Value::Curve(Curve::normalized(
                    vec![CurveKey::new(0.0, 0.0), CurveKey::new(1.0, 1.0)],
                    ScalarRange::new(0.0, 6.0),
                )),
            )],
        );
        motion
            .property_sources
            .insert("gravity".into(), PropertySource::RandomRange);
        motion.property_source_values.insert(
            "gravity".into(),
            vec![PropertySourceValue::new(
                PropertySource::RandomRange,
                Value::Vec3Range(Vec3Range::new([-3.0, -6.0, 1.0], [4.0, 2.0, 9.0])),
            )],
        );

        let compiled = Arc::new(EffectCompiler::default().compile(&effect).unwrap());
        let artifact = GpuEffectArtifact::from_instance(&EffectInstance::new(compiled)).unwrap();
        let curve = artifact.emitters[0];
        let random = artifact.emitters[1];

        assert_eq!(curve.drag_source, 3);
        assert_eq!(curve.drag_curve.count, 2);
        assert_eq!(curve.drag_curve.keys[0], Vec2::new(0.0, 0.0));
        assert_eq!(curve.drag_curve.keys[1], Vec2::new(1.0, 4.0));
        assert_eq!(curve.turbulence_source, 1);
        assert_eq!(curve.turbulence, Vec2::new(1.0, 5.0));
        assert_eq!(curve.gravity_source, 3);
        assert_eq!(curve.gravity_curves[0].keys[0], Vec2::new(0.0, 2.0));
        assert_eq!(curve.gravity_curves[1].keys[1], Vec2::new(1.0, -4.0));
        assert_eq!(curve.gravity_curves[2].keys[0], Vec2::new(0.0, 6.0));
        assert_eq!(random.drag_source, 1);
        assert_eq!(random.drag, Vec2::new(0.25, 1.5));
        assert_eq!(random.turbulence_source, 3);
        assert_eq!(random.turbulence_curve.count, 2);
        assert_eq!(random.turbulence_curve.keys[0], Vec2::new(0.0, 0.0));
        assert_eq!(random.turbulence_curve.keys[1], Vec2::new(1.0, 6.0));
        assert_eq!(random.gravity_source, 1);
        assert_eq!(random.gravity, Vec3::new(-3.0, -6.0, 1.0));
        assert_eq!(random.gravity_max, Vec3::new(4.0, 2.0, 9.0));
    }

    #[test]
    fn sprite_render_wesl_compiles_and_validates() {
        let source = include_str!("shaders/aestra_sprite_render.wesl").to_owned();
        let module: wesl::ModulePath = "package::aestra_sprite_render".parse().unwrap();
        let mut resolver = wesl::VirtualResolver::new();
        resolver.add_module(module.clone(), source.into());
        let compiler = wesl::Wesl::new("").set_custom_resolver(resolver);
        let output = compiler.compile(&module).unwrap().to_string();
        assert!(output.contains("fn fragment_alpha"));
        assert!(output.contains("fn fragment_additive"));
        assert!(output.contains("fn fragment_multiply"));
        assert!(output.contains("renderer_index"));
        assert!(output.contains("alive_offset"));
    }
}
