//! Bevy render-world adapter for engine-neutral Aestra GPU artifacts.

mod bounds;
mod geometry_statistics;
mod mesh_inputs;
mod particle_statistics;
mod preparation_timing;
mod render;
mod ribbon_bounds;
mod simulation_timing;
mod trail_checkpoints;
mod trail_compaction;
mod trail_culling;
mod trail_replay;
mod wireframe;

use crate::{
    ActiveBackend, AestraRenderSettings, AestraRuntimeStatus, CompatibilityIssue,
    CompatibilityIssueCode, CompatibilityReport, EffectRenderMode, EffectRuntimeStatus,
    GpuCapabilities, GpuPresentationPrepared, PresentedEffect, ProjectAssetCache,
    capabilities::select_backend,
    material::{MaterialBindingError, MaterialRuntimeBinding},
};
use aestra_core::MaterialId;
use aestra_gpu::material::{CompiledMaterialProgram, MaterialProgramFingerprint};
pub use aestra_gpu::particle_attributes::{
    GpuParticleAttributeSummary, estimate_particle_attributes,
};
use aestra_gpu::particle_attributes::{GpuParticleAttributes, prune_particle_attributes};
use aestra_gpu::shader::{SIMULATION_WESL, SPRITE_RENDER_WESL};
pub use aestra_gpu::{
    GpuArtifactError, GpuCurve, GpuEffectArtifact, GpuEmitter, GpuGlobals, GpuGradient,
    GpuGradientKey, GpuParticle, GpuRenderGlobals, GpuRenderParams, GpuRenderer, MAX_CURVE_KEYS,
    MAX_FLIPBOOK_FRAMES,
};
use aestra_gpu::{
    GpuBlend, GpuSimulationState, WORKGROUP_SIZE, fold_seed,
    indirect_draw_commands_with_statistics, indirect_draw_offset,
};
use aestra_runtime::{RendererPlanKind, SimulationClass};
use bevy::{
    asset::{RenderAssetUsages, io::embedded::EmbeddedAssetRegistry},
    camera::{
        primitives::Aabb,
        visibility::{self, RenderLayers, VisibilityClass},
    },
    ecs::schedule::IntoScheduleConfigs,
    prelude::*,
    render::{
        ExtractSchedule, MainWorld, Render, RenderApp, RenderStartup, RenderSystems,
        diagnostic::RecordDiagnostics,
        extract_component::{ExtractComponent, ExtractComponentPlugin},
        gpu_readback::{Readback, ReadbackComplete},
        render_asset::RenderAssets,
        render_resource::{
            BindGroup, BindGroupEntries, BindGroupLayout, BindGroupLayoutDescriptor,
            BindGroupLayoutEntries, Buffer, BufferInitDescriptor, BufferUsages,
            CachedComputePipelineId, CommandEncoder, ComputePassDescriptor, ComputePipeline,
            ComputePipelineDescriptor, DownlevelFlags, Extent3d, PipelineCache, ShaderStages,
            TextureDimension, TextureFormat,
            binding_types::{storage_buffer, storage_buffer_read_only},
        },
        renderer::{
            RenderAdapter, RenderAdapterInfo, RenderContext, RenderDevice, RenderGraph,
            RenderGraphSystems,
        },
        storage::{GpuShaderBuffer, ShaderBuffer},
        sync_component::SyncComponent,
    },
};
pub use particle_statistics::GpuParticleStatistics;
pub use preparation_timing::GpuPreparationTiming;
pub use simulation_timing::GpuSimulationTiming;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};

pub const WESL_SHADER_PATH: &str = "embedded://aestra_bevy_render/shaders/aestra_simulation.wesl";
pub const WESL_RENDER_SHADER_PATH: &str =
    "embedded://aestra_bevy_render/shaders/aestra_sprite_render.wesl";
pub const WESL_MESH_WIREFRAME_SHADER_PATH: &str =
    "embedded://aestra_bevy_render/shaders/aestra_mesh_wireframe.wesl";
/// The unified stateful simulation module (hybrid roadmap M6): the death_integrate / spawn / present
/// compute pipelines are built from this. Composed from the proven `aestra_gpu` WGSL primitives.
pub const STATEFUL_SIMULATION_SHADER_PATH: &str =
    "embedded://aestra_bevy_render/shaders/aestra_stateful_simulation.wgsl";

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
    /// Shared per-slot scratch (3 words/slot) for ribbon link state, kept off the
    /// particle ABI. A 1-word dummy when the effect has no ribbon renderer.
    aux: Handle<ShaderBuffer>,
    render_globals: Handle<ShaderBuffer>,
    workgroups: u32,
    has_ribbons: bool,
    has_trails: bool,
    ribbon_workgroups: u32,
    total_slots: u32,
    simulation_time: f32,
    history_epoch: u32,
    statistics_token: u32,
    checkpoint_context: Arc<trail_checkpoints::TrailContext>,
    trail_roots: Vec<(u32, u32)>,
    /// Persistent simulation-state sizing for stateful emitters (hybrid roadmap M6); `records == 0`
    /// for a fully analytic effect. The render world allocates its persistent state buffers from
    /// this (see [`StatefulStates`]).
    simulation_state: GpuSimulationState,
    /// One stateful dispatch descriptor per enabled stateful emitter (hybrid roadmap M6), in compiled
    /// emitter order. Non-empty only when *every* enabled emitter is stateful — the case the GPU
    /// stateful path drives end-to-end; empty for analytic effects and (for now) mixed
    /// analytic+stateful effects, which take the analytic path.
    stateful_dispatch: Vec<StatefulDispatch>,
}

/// The parameters the GPU stateful path needs for one emitter (hybrid roadmap M6), sourced from the
/// compiled emitter at prepare time so the render graph does not need the CPU effect. The stateful
/// integrator is the minimal reference model (deterministic launch direction, constant gravity, fixed
/// lifetime), so it reads scalar midpoints of the authored ranges rather than the full analytic
/// feature set.
#[derive(Clone)]
struct StatefulDispatch {
    /// Live-particle capacity — the emitter's `max_particles`, and the persistent state slot count.
    capacity: u32,
    /// This emitter's base index into the alive-indices / indirect draw buffers.
    slot_offset: u32,
    /// This emitter's index for the packed emitter/alive word and its indirect draw command.
    emitter_index: u32,
    /// The effect's enabled-emitter count, locating the statistics telemetry trailer in the indirect
    /// buffer (at `emitter_count * 4`).
    emitter_count: u32,
    /// Mean particles emitted per second; fractional per-tick spawns accumulate across ticks.
    spawn_rate: f32,
    /// Initial launch speed along the deterministic direction.
    speed: f32,
    /// Particle lifetime in seconds.
    lifetime: f32,
    /// Constant acceleration applied to velocity each tick.
    gravity: [f32; 3],
    /// The effect's 64-bit spawn seed.
    seed: u64,
}

#[derive(Component, Clone)]
#[require(Transform, Visibility, VisibilityClass)]
#[component(on_add = visibility::add_visibility_class::<GpuDrawInstance>)]
struct GpuDrawInstance {
    owner: Entity,
    mesh: Option<Handle<Mesh>>,
    wireframe_geometry: Option<Arc<wireframe::WireframeGeometry>>,
    renderers: Handle<ShaderBuffer>,
    particles: Handle<ShaderBuffer>,
    alive: Handle<ShaderBuffer>,
    aux: Handle<ShaderBuffer>,
    indirect: Handle<ShaderBuffer>,
    render_globals: Handle<ShaderBuffer>,
    render_params: Handle<ShaderBuffer>,
    texture: Handle<Image>,
    fallback_texture: Handle<Image>,
    renderer_order: u32,
    emitter_index: u32,
    indirect_offset: u64,
    trail_instances: Option<u32>,
    trail_owners: u32,
    blend: GpuBlend,
    material: MaterialId,
    semantic_material: Option<GpuSemanticMaterialBinding>,
    render_mode: GpuRenderMode,
    mesh_center: Vec3,
}

#[derive(Clone)]
struct GpuSemanticMaterialBinding {
    program: Arc<CompiledMaterialProgram>,
    render_state: aestra_core::material::MaterialRenderState,
    shader: Handle<Shader>,
    multisampled_shader: Handle<Shader>,
    uniforms: Arc<[u8]>,
    textures: Vec<Handle<Image>>,
    fallback_texture: Handle<Image>,
}

#[derive(Clone)]
struct MaterialShaderVariants {
    single_sampled: Handle<Shader>,
    multisampled: Handle<Shader>,
}

#[derive(Resource, Default)]
pub(crate) struct MaterialShaderCache(BTreeMap<MaterialProgramFingerprint, MaterialShaderVariants>);

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
pub struct GpuFallbackTextures {
    pub white: Handle<Image>,
    pub missing: Handle<Image>,
}

type UnpreparedPlayers<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static mut PresentedEffect,
        &'static EffectRuntimeStatus,
        Option<&'static RenderLayers>,
    ),
    (Without<GpuEffectBuffers>, Without<GpuPresentationPrepared>),
>;

type PreparedGpuPlayers<'w, 's> = Query<
    'w,
    's,
    (
        &'static mut PresentedEffect,
        &'static mut GpuEffectBuffers,
        &'static EffectRuntimeStatus,
        Option<&'static RenderLayers>,
        Option<&'static Children>,
    ),
    Without<GpuDrawInstance>,
>;

#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct MaterialPreparationParams<'w> {
    shaders: ResMut<'w, Assets<Shader>>,
    asset_server: Res<'w, AssetServer>,
    texture_cache: ResMut<'w, ProjectAssetCache>,
    fallback_textures: Res<'w, GpuFallbackTextures>,
    shader_cache: ResMut<'w, MaterialShaderCache>,
}

impl MaterialPreparationParams<'_> {
    fn prepare(
        &mut self,
        binding: &MaterialRuntimeBinding,
        effect: &PresentedEffect,
    ) -> Result<GpuSemanticMaterialBinding, MaterialBindingError> {
        let _span = tracing::info_span!("aestra::gpu::material_prepare").entered();
        prepare_semantic_material(
            binding,
            effect,
            &self.asset_server,
            &mut self.texture_cache,
            &self.fallback_textures,
            &mut self.shaders,
            &mut self.shader_cache,
        )
    }
}

#[derive(Resource)]
struct SimulationPipeline {
    layout: BindGroupLayoutDescriptor,
    reset: CachedComputePipelineId,
    simulate: CachedComputePipelineId,
    link_ribbons: CachedComputePipelineId,
    update_trails: CachedComputePipelineId,
}

/// The stateful GPU backend's compute pipelines (hybrid roadmap M6), built from the unified
/// `aestra_gpu::stateful_simulation_wgsl` module over a shared six-binding layout: persistent state,
/// the free list and its atomic count, the atomic spawn counter, the per-dispatch params, and the
/// presentation output. Present only for effects with at least one stateful emitter; the per-frame
/// dispatch that consumes these lands in the next increment.
#[derive(Resource)]
struct StatefulSimulationPipeline {
    layout: BindGroupLayoutDescriptor,
    /// Advances each live slot by one fixed tick and frees the ones that died this tick.
    death_integrate: CachedComputePipelineId,
    /// Claims a free slot and a fresh ordinal for each of this tick's new particles.
    spawn: CachedComputePipelineId,
    /// Extracts live persistent state into the 48-byte presentation particle buffer.
    present: CachedComputePipelineId,
}

pub(crate) fn install(app: &mut App) {
    install_shader_assets(app);
    let timing_mailbox = simulation_timing::TimingMailbox::default();
    let preparation_mailboxes = preparation_timing::PreparationMailboxes::default();
    app.insert_resource(preparation_mailboxes.clone())
        .add_systems(PreUpdate, preparation_timing::receive);
    let geometry_mailbox = geometry_statistics::GeometryMailbox::default();
    app.insert_resource(geometry_mailbox.clone())
        .add_systems(PreUpdate, geometry_statistics::receive);
    app.insert_resource(timing_mailbox.clone())
        .add_systems(PreUpdate, simulation_timing::receive_timings);
    app.add_plugins((
        ExtractComponentPlugin::<GpuEffectBuffers>::default(),
        ExtractComponentPlugin::<GpuDrawInstance>::default(),
    ))
    .init_resource::<MaterialShaderCache>()
    .add_systems(Startup, init_fallback_textures)
    .add_systems(Update, update_gpu_inputs.after(prepare_gpu_effects));
    install_visibility_updates(app);
    let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
        let capabilities = GpuCapabilities::unavailable("Bevy has no render sub-application");
        let requested = app.world().resource::<AestraRenderSettings>().presentation;
        let status = select_backend(requested, &capabilities);
        app.insert_resource(capabilities).insert_resource(status);
        return;
    };
    render_app
        .insert_resource(preparation_mailboxes)
        .insert_resource(geometry_mailbox)
        .init_resource::<geometry_statistics::Submissions>()
        .add_systems(
            RenderGraph,
            (
                geometry_statistics::begin
                    .after(RenderGraphSystems::Begin)
                    .before(RenderGraphSystems::Render),
                geometry_statistics::finish
                    .after(RenderGraphSystems::Render)
                    .before(RenderGraphSystems::Submit),
            ),
        )
        .insert_resource(timing_mailbox)
        .add_systems(ExtractSchedule, publish_gpu_capabilities)
        .add_systems(RenderStartup, (init_pipeline, init_stateful_pipeline))
        .init_resource::<StatefulStates>()
        .add_systems(
            Render,
            (
                prepare_bind_groups,
                // Persistent stateful buffers are allocated alongside the analytic bind groups; the
                // dispatch that consumes them lands in the next increment.
                prepare_stateful_states,
            )
                .in_set(RenderSystems::PrepareBindGroups),
        )
        // Run inside the render-graph diagnostics window (after `Begin`) and before
        // the graph draws (`Render`), so the `aestra::gpu::simulate` GPU timestamp
        // span is captured and particles are simulated before they are rendered.
        .add_systems(
            RenderGraph,
            run_simulation
                .after(RenderGraphSystems::Begin)
                .before(RenderGraphSystems::Render),
        );
    render::install(render_app);
    trail_culling::install(render_app);
    trail_compaction::install(render_app);
}

fn install_shader_assets(app: &App) {
    let registry = app.world().resource::<EmbeddedAssetRegistry>();
    let shader_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../aestra-gpu/src/shaders");
    registry.insert_asset(
        shader_root.join("aestra_trail_compact.wesl"),
        Path::new("aestra_bevy_render/shaders/aestra_trail_compact.wesl"),
        aestra_gpu::shader::trail_compact_wesl().into_bytes(),
    );
    registry.insert_asset(
        shader_root.join("aestra_trail_cull.wesl"),
        Path::new("aestra_bevy_render/shaders/aestra_trail_cull.wesl"),
        aestra_gpu::shader::trail_cull_wesl().into_bytes(),
    );
    registry.insert_asset(
        shader_root.join("aestra_mesh_wireframe.wesl"),
        Path::new("aestra_bevy_render/shaders/aestra_mesh_wireframe.wesl"),
        aestra_gpu::shader::mesh_wireframe_wesl().into_bytes(),
    );
    registry.insert_asset(
        shader_root.join("aestra_simulation.wesl"),
        Path::new("aestra_bevy_render/shaders/aestra_simulation.wesl"),
        SIMULATION_WESL.as_bytes(),
    );
    registry.insert_asset(
        shader_root.join("aestra_sprite_render.wesl"),
        Path::new("aestra_bevy_render/shaders/aestra_sprite_render.wesl"),
        SPRITE_RENDER_WESL.as_bytes(),
    );
    // The unified stateful simulation module (hybrid roadmap M6), composed from the proven aestra-gpu
    // primitives. Plain WGSL (no WESL composition), so it is embedded directly as .wgsl.
    registry.insert_asset(
        shader_root.join("aestra_stateful_simulation.wgsl"),
        Path::new("aestra_bevy_render/shaders/aestra_stateful_simulation.wgsl"),
        aestra_gpu::stateful_simulation_wgsl().into_bytes(),
    );
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

pub(crate) fn prepare_gpu_effects(
    mut commands: Commands,
    capabilities: Res<GpuCapabilities>,
    mut buffers: ResMut<Assets<ShaderBuffer>>,
    mut material_resources: MaterialPreparationParams,
    mut players: UnpreparedPlayers,
) {
    let _span = tracing::info_span!("aestra::gpu::prepare_instance").entered();
    for (entity, mut player, runtime, render_layers) in &mut players {
        if !matches!(
            runtime.active,
            ActiveBackend::Gpu | ActiveBackend::GpuReadback
        ) {
            continue;
        }
        player.refresh_automatic_material_bindings();
        let artifact_result =
            GpuEffectArtifact::dynamics_from_instance(&player.instance).and_then(|d| {
                if d.storage_records > capabilities.max_particles {
                    return Err(GpuArtifactError::TrailLimit);
                }
                GpuEffectArtifact::from_instance(&player.instance)
            });
        let mut artifact = match artifact_result {
            Ok(artifact) => artifact,
            Err(error) => {
                let message =
                    format!("GPU artifact is unsupported ({error}); using the CPU reference");
                commands.entity(entity).insert(EffectRuntimeStatus {
                    active: ActiveBackend::CpuReference,
                    reason: message.clone(),
                    compatibility: CompatibilityReport::from_issues(
                        runtime.compatibility.target,
                        [CompatibilityIssue::new(
                            CompatibilityIssueCode::BackendRejected,
                            message,
                        )],
                    ),
                });
                continue;
            }
        };
        if artifact.total_slots == 0 {
            commands.entity(entity).insert(GpuPresentationPrepared);
            continue;
        }
        apply_semantic_sprite_compatibility_to_renderers(&mut artifact.renderers, &player);
        let renderer_draws = artifact
            .renderers
            .iter_mut()
            .enumerate()
            .zip(
                player
                    .effect()
                    .emitters
                    .iter()
                    .filter(|emitter| emitter.enabled)
                    .flat_map(|emitter| {
                        emitter
                            .renderers
                            .iter()
                            .map(move |renderer| (emitter, renderer))
                    }),
            )
            .map(|((index, renderer), (emitter, plan))| {
                let material = player.effect().material(plan.material);
                let runtime_binding =
                    player.material_binding_for_emitter(plan.material, emitter.source);
                let semantic_material = runtime_binding
                    .map(|binding| material_resources.prepare(binding, &player))
                    .transpose()
                    .map_err(|error| {
                        warn!(
                            "semantic material {} could not be bound: {error}",
                            plan.material
                        );
                        error
                    })
                    .ok()
                    .flatten();
                let texture = match &plan.kind {
                    RendererPlanKind::Mesh { .. } => None,
                    RendererPlanKind::Ribbon { .. } | RendererPlanKind::Trail { .. } => {
                        material.and_then(|material| material.texture)
                    }
                    RendererPlanKind::Sprite => material.and_then(|material| material.texture),
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
                let overridden = texture
                    .and_then(|asset| player.texture_override(asset))
                    .cloned();
                let (texture, fallback_texture) = overridden
                    .map(|image| (image, material_resources.fallback_textures.missing.clone()))
                    .unwrap_or_else(|| {
                        texture_path.map_or_else(
                            || {
                                (
                                    material_resources.fallback_textures.white.clone(),
                                    material_resources.fallback_textures.white.clone(),
                                )
                            },
                            |path| {
                                (
                                    material_resources
                                        .texture_cache
                                        .load(&material_resources.asset_server, &path),
                                    material_resources.fallback_textures.missing.clone(),
                                )
                            },
                        )
                    });
                (
                    index as u32,
                    renderer.emitter_index,
                    artifact.emitters[renderer.emitter_index as usize].slot_offset,
                    semantic_material.as_ref().map_or_else(
                        || match renderer.blend_mode {
                            mode if mode == GpuBlend::Additive as u32 => GpuBlend::Additive,
                            mode if mode == GpuBlend::Multiply as u32 => GpuBlend::Multiply,
                            _ => GpuBlend::Alpha,
                        },
                        |binding| gpu_blend(binding.render_state.blend),
                    ),
                    texture,
                    fallback_texture,
                    plan.material,
                    semantic_material,
                    match plan.kind {
                        RendererPlanKind::Mesh { asset } => {
                            player.mesh_override(asset).cloned().or_else(|| {
                                player
                                    .effect()
                                    .assets
                                    .iter()
                                    .find(|entry| entry.source == asset)
                                    .map(|entry| {
                                        material_resources.texture_cache.load_mesh(
                                            &material_resources.asset_server,
                                            &entry.path,
                                        )
                                    })
                            })
                        }
                        _ => None,
                    },
                )
            })
            .collect::<Vec<_>>();
        if runtime.active == ActiveBackend::Gpu {
            let requirements = artifact
                .renderers
                .iter()
                .zip(&renderer_draws)
                .map(|(renderer, draw)| {
                    GpuParticleAttributes::for_renderer(
                        renderer,
                        draw.7.as_ref().map(|binding| &binding.program.reflection),
                        player.render_mode() == EffectRenderMode::Wireframe
                            && !draw
                                .7
                                .as_ref()
                                .is_some_and(|binding| binding.program.has_vertex_offset),
                    )
                })
                .collect::<Vec<_>>();
            prune_particle_attributes(
                &mut artifact.emitters,
                &mut artifact.renderers,
                &requirements,
            );
        }
        let bounds = Aabb {
            center: Vec3A::ZERO,
            half_extents: Vec3A::from(artifact.bounds_half_extents),
        };
        // The GPU stateful path (hybrid roadmap M6) drives every enabled stateful emitter end-to-end,
        // one dispatch descriptor each (in compiled emitter order, which is 1:1 with the GpuEmitters).
        // Dynamics come from the compiled GpuEmitter as scalar midpoints of the authored ranges — the
        // minimal reference model. The path only engages when *all* enabled emitters are stateful; a
        // mixed effect keeps the empty list and takes the analytic path (documented follow-up).
        let compiled_emitters = &player.instance.effect().emitters;
        let enabled_emitters = compiled_emitters
            .iter()
            .filter(|emitter| emitter.enabled)
            .count();
        let emitter_count = artifact.emitters.len() as u32;
        let seed = player.instance.seed();
        let stateful_dispatch: Vec<StatefulDispatch> = if artifact.simulation_state.records > 0 {
            let dispatches: Vec<StatefulDispatch> = compiled_emitters
                .iter()
                .enumerate()
                .filter(|(_, compiled)| {
                    compiled.enabled && compiled.simulation_class != SimulationClass::Analytic
                })
                .filter_map(|(index, _)| {
                    artifact
                        .emitters
                        .get(index)
                        .map(|emitter| StatefulDispatch {
                            capacity: emitter.max_particles,
                            slot_offset: emitter.slot_offset,
                            emitter_index: index as u32,
                            emitter_count,
                            spawn_rate: 0.5 * (emitter.spawn_rate.x + emitter.spawn_rate.y),
                            speed: 0.5 * (emitter.speed.x + emitter.speed.y),
                            lifetime: 0.5 * (emitter.lifetime.x + emitter.lifetime.y),
                            gravity: [emitter.gravity.x, emitter.gravity.y, emitter.gravity.z],
                            seed,
                        })
                })
                .collect();
            // Only engage when every enabled emitter is stateful; otherwise fall back to analytic.
            if dispatches.len() == enabled_emitters {
                dispatches
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };
        let indirect_draw_commands = indirect_draw_commands_with_statistics(&artifact.emitters);
        let particle_statistics = GpuParticleStatistics::new(&player.instance);
        let trail_roots = artifact
            .emitters
            .iter()
            .enumerate()
            .filter(|(_, e)| e.trail_points >= 2)
            .map(|(i, e)| (i as u32, e.trail_offset))
            .collect();
        let emitters = buffers.add(ShaderBuffer::from(artifact.emitters));
        let ribbon_renderers = artifact
            .renderers
            .iter()
            .enumerate()
            .filter_map(|(index, r)| (r.renderer_kind == 3).then_some(index as u32))
            .collect::<Vec<_>>();
        let trail_renderers: BTreeMap<_, _> = artifact
            .renderers
            .iter()
            .enumerate()
            .filter(|(_, r)| r.renderer_kind == 4)
            .map(|(index, r)| (index as u32, aestra_gpu::trail_draw_instances(r)))
            .collect();
        let has_trails = !trail_renderers.is_empty();
        let has_ribbons = !ribbon_renderers.is_empty() || has_trails;
        let ribbon_workgroups = player
            .effect()
            .emitters
            .len()
            .div_ceil(WORKGROUP_SIZE as usize) as u32;
        let renderer_owners: Vec<_> = artifact.renderers.iter().map(|r| r.playback_mode).collect();
        let renderers = buffers.add(ShaderBuffer::from(artifact.renderers));
        // Full record count, including the trail-history storage region past
        // total_slots, so aux (indexed by slot) covers trail head/record slots.
        let record_count = artifact.particles.len();
        let particles = buffers.add(ShaderBuffer::from(artifact.particles));
        let alive = buffers.add(ShaderBuffer::from(vec![
            0_u32;
            artifact.total_slots as usize
        ]));
        let dead = buffers.add(ShaderBuffer::from(vec![
            0_u32;
            artifact.total_slots as usize
        ]));
        // Shared per-slot aux scratch (3 words/slot) for ribbon link + trail ring
        // state; a 1-word dummy when the effect draws no ribbons/trails.
        let aux = buffers.add(ShaderBuffer::from(vec![
            0_u32;
            if has_ribbons {
                record_count * 3
            } else {
                1
            }
        ]));
        let counters = buffers.add(ShaderBuffer::from(vec![
            0_u32;
            2 + if has_trails {
                6 * player.effect().emitters.len()
            } else {
                0
            }
        ]));
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
            world_from_effect: Mat4::IDENTITY,
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
                counters: counters.clone(),
                indirect: indirect.clone(),
                globals,
                aux: aux.clone(),
                render_globals: render_globals.clone(),
                workgroups: artifact.total_slots.div_ceil(WORKGROUP_SIZE),
                has_ribbons,
                has_trails,
                simulation_time: player.simulation_time(),
                history_epoch: player.instance.history_epoch(),
                statistics_token: 0,
                checkpoint_context: default(),
                trail_roots,
                ribbon_workgroups,
                total_slots: artifact.total_slots,
                simulation_state: artifact.simulation_state,
                stateful_dispatch,
            },
            GpuPresentationPrepared,
            particle_statistics,
            GpuSimulationTiming::default(),
            GpuPreparationTiming::default(),
        ));
        commands.entity(entity).with_children(|parent| {
            parent
                .spawn((
                    Readback::buffer(indirect.clone()),
                    particle_statistics::ParticleStatisticsOwner(entity),
                ))
                .observe(particle_statistics::receive_particle_statistics);
        });
        if has_trails {
            commands
                .entity(entity)
                .insert(GpuTrailStatistics::default())
                .with_children(|parent| {
                    parent
                        .spawn((
                            Readback::buffer(counters.clone()),
                            GpuTrailReadbackOwner(entity),
                        ))
                        .observe(receive_trail_statistics);
                });
        }
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
                        material,
                        semantic_material,
                        mesh,
                    ) in renderer_draws
                    {
                        let render_params = buffers.add(ShaderBuffer::from(GpuRenderParams {
                            renderer_index,
                            alive_offset,
                            _padding: UVec2::ZERO,
                            mesh_from_local: mesh_from_emitter(
                                player.effect().emitters[emitter_index as usize].transform,
                            ),
                        }));
                        let mesh_bounds = mesh.clone().map(bounds::MeshBoundsSource::new);
                        let mut draw = parent.spawn((
                            HostMotionDraw,
                            GpuDrawInstance {
                                owner: entity,
                                mesh,
                                wireframe_geometry: None,
                                renderers: renderers.clone(),
                                particles: particles.clone(),
                                alive: alive.clone(),
                                aux: aux.clone(),
                                indirect: indirect.clone(),
                                render_globals: render_globals.clone(),
                                render_params,
                                texture,
                                fallback_texture,
                                renderer_order: renderer_index,
                                emitter_index,
                                indirect_offset: indirect_draw_offset(emitter_index),
                                trail_instances: trail_renderers.get(&renderer_index).copied(),
                                trail_owners: renderer_owners[renderer_index as usize],
                                blend,
                                material,
                                semantic_material,
                                render_mode,
                                mesh_center: Vec3::ZERO,
                            },
                            render_layers.cloned().unwrap_or_default(),
                            bounds,
                            Transform::default(),
                            Visibility::Inherited,
                        ));
                        // Geometry may still be loading; enable culling only once its bounds
                        // and the current particle motion have both been resolved.
                        if let Some(mesh_bounds) = mesh_bounds {
                            draw.insert((mesh_bounds, visibility::NoFrustumCulling));
                        }
                        // Enable culling only once motion and the propagated transform resolve.
                        if ribbon_renderers.contains(&renderer_index) {
                            draw.insert((
                                ribbon_bounds::RibbonBoundsSource::default(),
                                visibility::NoFrustumCulling,
                            ));
                        }
                        // World-space history can be far outside current emitter bounds.
                        if trail_renderers.contains_key(&renderer_index) {
                            draw.insert(visibility::NoFrustumCulling);
                        }
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

fn apply_semantic_sprite_compatibility(
    renderer: &mut aestra_gpu::GpuRenderer,
    binding: &MaterialRuntimeBinding,
) {
    if binding.uses_sampled_textures() {
        renderer.textured = 1;
    }
    if let Some(softness) = binding.legacy_sprite_softness() {
        renderer.softness = softness;
    }
}

fn apply_semantic_sprite_compatibility_to_renderers(
    renderers: &mut [GpuRenderer],
    player: &PresentedEffect,
) {
    for (renderer, plan) in renderers.iter_mut().zip(
        player
            .effect()
            .emitters
            .iter()
            .filter(|emitter| emitter.enabled)
            .flat_map(|emitter| {
                emitter
                    .renderers
                    .iter()
                    .map(move |renderer| (emitter, renderer))
            }),
    ) {
        let (emitter, plan) = plan;
        if let Some(binding) = player.material_binding_for_emitter(plan.material, emitter.source) {
            apply_semantic_sprite_compatibility(renderer, binding);
        }
    }
}

fn publish_gpu_capabilities(
    render_device: Res<RenderDevice>,
    adapter: Res<RenderAdapter>,
    adapter_info: Res<RenderAdapterInfo>,
    mut main_world: ResMut<MainWorld>,
) {
    let capabilities = detect_gpu_capabilities(&render_device, &adapter, &adapter_info);
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

type PreparedDraws<'w, 's> = Query<
    'w,
    's,
    (
        &'static mut GpuDrawInstance,
        &'static mut RenderLayers,
        Option<&'static mut bounds::MeshBoundsSource>,
        Option<&'static mut ribbon_bounds::RibbonBoundsSource>,
    ),
    Without<PresentedEffect>,
>;

fn update_gpu_inputs(
    mut buffers: ResMut<Assets<ShaderBuffer>>,
    mut material_resources: MaterialPreparationParams,
    mut players: PreparedGpuPlayers,
    mut draw_instances: PreparedDraws,
) {
    let _span = tracing::info_span!("aestra::gpu::artifact_update").entered();
    for (mut player, mut gpu, runtime, render_layers, children) in &mut players {
        player.refresh_automatic_material_bindings();
        // Only emitter and renderer inputs change per frame; use the dynamics
        // builder so we never reallocate the capacity-sized particle scratch buffer
        // here (its cost scales with capacity, not with what actually changed).
        if let Some(children) = children {
            let render_mode = gpu_render_mode(player.render_mode());
            for child in children.iter() {
                if let Ok((mut draw, mut draw_layers, mesh_bounds, ribbon_bounds)) =
                    draw_instances.get_mut(child)
                {
                    if let Some(mut source) = ribbon_bounds {
                        source.0 = None;
                    }
                    if let Some(mut mesh_bounds) = mesh_bounds {
                        mesh_bounds.motion = None;
                    }
                    draw.render_mode = render_mode;
                    let emitter = player
                        .effect()
                        .emitters
                        .get(draw.emitter_index as usize)
                        .map(|emitter| emitter.source);
                    if let Some(binding) = emitter.and_then(|emitter| {
                        player.material_binding_for_emitter(draw.material, emitter)
                    }) {
                        match material_resources.prepare(binding, &player) {
                            Ok(prepared) => {
                                draw.blend = gpu_blend(binding.render_state().blend);
                                draw.semantic_material = Some(prepared);
                            }
                            Err(error) => warn!(
                                "semantic material {} could not be updated: {error}",
                                draw.material
                            ),
                        }
                    } else {
                        draw.semantic_material = None;
                    }
                    *draw_layers = render_layers.cloned().unwrap_or_default();
                }
            }
        }
        // Use the binding that actually prepared successfully, including retained bindings
        // on preparation failure. Never prune CPU-presentation/readback data.
        if let Ok(mut dynamics) = GpuEffectArtifact::dynamics_from_instance(&player.instance) {
            let ribbon_workgroups = (dynamics.emitters.len() as u32).div_ceil(WORKGROUP_SIZE);
            if gpu.ribbon_workgroups != ribbon_workgroups {
                gpu.ribbon_workgroups = ribbon_workgroups;
            }
            let has_ribbons = dynamics
                .renderers
                .iter()
                .any(|r| matches!(r.renderer_kind, 3 | 4));
            if gpu.has_ribbons != has_ribbons {
                gpu.has_ribbons = has_ribbons;
            }
            if let Some(children) = children {
                for child in children.iter() {
                    if let Ok((draw, _, mesh_bounds, ribbon_bounds)) = draw_instances.get_mut(child)
                    {
                        if let Some(mut source) = ribbon_bounds {
                            source.0 = dynamics
                                .ribbon_bounds
                                .get(draw.emitter_index as usize)
                                .copied();
                        }
                        if let Some(mut mesh_bounds) = mesh_bounds {
                            mesh_bounds.displacement = draw
                                .semantic_material
                                .as_ref()
                                .map_or(Some([0.0; 3]), |binding| {
                                    binding.program.vertex_offset_bounds
                                })
                                .map(Vec3::from_array);
                            mesh_bounds.motion = dynamics
                                .mesh_bounds
                                .get(draw.emitter_index as usize)
                                .copied();
                        }
                    }
                }
            }
            apply_semantic_sprite_compatibility_to_renderers(&mut dynamics.renderers, &player);
            if runtime.active == ActiveBackend::Gpu {
                let mut requirements = vec![GpuParticleAttributes::ALL; dynamics.renderers.len()];
                if let Some(children) = children {
                    for child in children.iter() {
                        if let Ok((draw, _, _, _)) = draw_instances.get(child)
                            && let Some(renderer) =
                                dynamics.renderers.get(draw.renderer_order as usize)
                        {
                            requirements[draw.renderer_order as usize] =
                                GpuParticleAttributes::for_renderer(
                                    renderer,
                                    draw.semantic_material
                                        .as_ref()
                                        .map(|binding| &binding.program.reflection),
                                    draw.render_mode == GpuRenderMode::Wireframe
                                        && !draw.semantic_material.as_ref().is_some_and(
                                            |binding| binding.program.has_vertex_offset,
                                        ),
                                );
                        }
                    }
                }
                prune_particle_attributes(
                    &mut dynamics.emitters,
                    &mut dynamics.renderers,
                    &requirements,
                );
            }
            let _upload = tracing::info_span!("aestra::gpu::buffer_upload").entered();
            if let Some(mut buffer) = buffers.get_mut(&gpu.emitters) {
                buffer.set_data(dynamics.emitters);
            }
            if let Some(mut buffer) = buffers.get_mut(&gpu.renderers) {
                buffer.set_data(dynamics.renderers);
            }
        }
    }
}

fn install_visibility_updates(app: &mut App) {
    app.add_systems(
        PostUpdate,
        sync_host_motion_draw_globals
            .after(bevy::transform::TransformSystems::Propagate)
            .before(bounds::sync_mesh_bounds)
            .before(ribbon_bounds::sync_ribbon_bounds)
            .before(wireframe::prepare_wireframe_geometry)
            .before(sync_gpu_render_transforms),
    );
    app.add_systems(
        PostUpdate,
        sync_host_motion_draw_transforms.before(bevy::transform::TransformSystems::Propagate),
    );
    app.add_systems(
        PostUpdate,
        (
            bounds::sync_mesh_bounds,
            ribbon_bounds::sync_ribbon_bounds,
            sync_gpu_render_transforms,
            wireframe::prepare_wireframe_geometry,
        )
            .after(bevy::transform::TransformSystems::Propagate)
            .before(visibility::VisibilitySystems::CheckVisibility),
    );
    app.add_systems(
        PostUpdate,
        sync_host_motion_replay_culling
            .after(bounds::sync_mesh_bounds)
            .after(ribbon_bounds::sync_ribbon_bounds)
            .before(visibility::VisibilitySystems::CheckVisibility),
    );
}

#[derive(Component)]
struct HostMotionDraw;

#[derive(Component)]
struct HostMotionReplayCulling;

fn sync_host_motion_draw_globals(
    players: Query<(&PresentedEffect, &GlobalTransform), Without<HostMotionDraw>>,
    mut draws: Query<(&ChildOf, &mut GlobalTransform), With<HostMotionDraw>>,
) {
    for (parent, mut global) in &mut draws {
        if let Ok((player, placement)) = players.get(parent.parent()) {
            let matrix = Mat4::from(placement.affine())
                * Mat4::from_cols_array(
                    &player
                        .instance
                        .host_transform_context()
                        .matrix_at(player.simulation_time()),
                );
            global.set_if_neq(GlobalTransform::from(matrix));
        }
    }
}

// During a bounded multi-frame seek the processed pose can lag the requested pose.
// Do not cull mixed sprite/mesh/ribbon draws at the future pose. Trails have their
// own per-camera world-history culling; ordinary bounds resume when motion is removed.
#[allow(clippy::type_complexity)]
fn sync_host_motion_replay_culling(
    mut commands: Commands,
    players: Query<(&PresentedEffect, &GpuEffectBuffers)>,
    draws: Query<(
        Entity,
        &ChildOf,
        &GpuDrawInstance,
        Has<visibility::NoFrustumCulling>,
        Has<HostMotionReplayCulling>,
        Has<ribbon_bounds::RibbonBoundsSource>,
    )>,
) {
    for (entity, parent, draw, uncullable, was_forced, ribbon) in &draws {
        let forced = players.get(parent.parent()).is_ok_and(|(player, gpu)| {
            gpu.has_trails && !player.instance.host_transform_context().is_identity()
        });
        if forced {
            if !uncullable || !was_forced {
                commands
                    .entity(entity)
                    .insert((HostMotionReplayCulling, visibility::NoFrustumCulling));
            }
        } else if was_forced {
            commands.entity(entity).remove::<HostMotionReplayCulling>();
            if draw.mesh.is_none() && !ribbon && draw.trail_instances.is_none() {
                commands
                    .entity(entity)
                    .remove::<visibility::NoFrustumCulling>();
            }
        }
    }
}

// Keep Bevy visibility/sorting transforms aligned with the matrix used by the shader.
// The host's own placement is never overwritten by animation.
fn sync_host_motion_draw_transforms(
    players: Query<&PresentedEffect>,
    mut draws: Query<(&ChildOf, &mut Transform), With<HostMotionDraw>>,
) {
    for (parent, mut transform) in &mut draws {
        if let Ok(player) = players.get(parent.parent()) {
            let desired = Transform::from_matrix(Mat4::from_cols_array(
                &player
                    .instance
                    .host_transform_context()
                    .matrix_at(player.simulation_time()),
            ));
            if *transform != desired {
                *transform = desired;
            }
        }
    }
}

// Rendering and culling must see the same frame's propagated effect transform.
fn sync_gpu_render_transforms(
    mut buffers: ResMut<Assets<ShaderBuffer>>,
    mut players: Query<(
        &PresentedEffect,
        &GlobalTransform,
        &mut GpuEffectBuffers,
        &mut GpuParticleStatistics,
    )>,
) {
    for (player, transform, mut gpu, mut statistics) in &mut players {
        let statistics_token = statistics.sync(&player.instance);
        gpu.statistics_token = statistics_token;
        let placement = Mat4::from(transform.affine());
        let world = placement
            * Mat4::from_cols_array(
                &player
                    .instance
                    .host_transform_context()
                    .matrix_at(player.simulation_time()),
            );
        gpu.simulation_time = player.simulation_time();
        gpu.history_epoch = player.instance.history_epoch();
        if gpu.has_trails {
            let seed = player.instance.seed();
            let revision = player.instance.history_revision();
            let context = player.instance.host_transform_context();
            let motion = (!context.is_identity()).then(|| Arc::new(context));
            let mut key = [0; 22];
            key[..6].copy_from_slice(&[
                seed as u32,
                (seed >> 32) as u32,
                revision as u32,
                (revision >> 32) as u32,
                player.effect().duration.to_bits(),
                u32::from(player.effect().playback_mode.is_continuous()),
            ]);
            key[6..].copy_from_slice(
                &Mat4::from(transform.affine())
                    .to_cols_array()
                    .map(f32::to_bits),
            );
            if let Some(data) = buffers.get(&gpu.emitters).and_then(|b| b.data.as_ref())
                && (gpu.checkpoint_context.key != key
                    || gpu.checkpoint_context.emitters != *data
                    || gpu.checkpoint_context.motion != motion)
            {
                gpu.checkpoint_context = Arc::new(trail_checkpoints::TrailContext {
                    emitters: data.clone(),
                    key,
                    motion,
                });
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
                _padding: UVec2::new(player.instance.history_epoch(), statistics_token),
                world_from_effect: world,
            });
        }
        if let Some(mut buffer) = buffers.get_mut(&gpu.render_globals) {
            buffer.set_data(GpuRenderGlobals {
                world_from_effect: world,
                time: player.simulation_time(),
                seed: fold_seed(player.instance.seed()),
                _padding: Vec2::ZERO,
            });
        }
    }
}

fn prepare_semantic_material(
    binding: &MaterialRuntimeBinding,
    effect: &PresentedEffect,
    asset_server: &AssetServer,
    texture_cache: &mut ProjectAssetCache,
    fallback_textures: &GpuFallbackTextures,
    shaders: &mut Assets<Shader>,
    shader_cache: &mut MaterialShaderCache,
) -> Result<GpuSemanticMaterialBinding, MaterialBindingError> {
    let prepared = binding.prepare()?;
    let program = binding.program().clone();
    let shader_variants = shader_cache
        .0
        .entry(program.program_fingerprint)
        .or_insert_with(|| {
            let single_sampled = shaders.add(Shader::from_wgsl(
                program.shader.wgsl.clone(),
                format!(
                    "generated://aestra/material/{}.wgsl",
                    program.program_fingerprint
                ),
            ));
            let multisampled = if program.requires_scene_depth() {
                shaders.add(Shader::from_wgsl(
                    program.multisampled_shader.wgsl.clone(),
                    format!(
                        "generated://aestra/material/{}_multisampled.wgsl",
                        program.program_fingerprint
                    ),
                ))
            } else {
                single_sampled.clone()
            };
            MaterialShaderVariants {
                single_sampled,
                multisampled,
            }
        })
        .clone();
    let textures = prepared
        .textures
        .into_iter()
        .map(|(_, asset)| {
            effect.texture_override(asset).cloned().unwrap_or_else(|| {
                effect
                    .effect()
                    .assets
                    .iter()
                    .find(|candidate| candidate.source == asset)
                    .map(|asset| texture_cache.load(asset_server, &asset.path))
                    .unwrap_or_else(|| fallback_textures.missing.clone())
            })
        })
        .collect();
    Ok(GpuSemanticMaterialBinding {
        program,
        render_state: binding.render_state(),
        shader: shader_variants.single_sampled,
        multisampled_shader: shader_variants.multisampled,
        uniforms: prepared.uniforms.into(),
        textures,
        fallback_texture: fallback_textures.missing.clone(),
    })
}

const fn gpu_blend(blend: aestra_core::BlendMode) -> GpuBlend {
    match blend {
        aestra_core::BlendMode::Alpha => GpuBlend::Alpha,
        aestra_core::BlendMode::Additive => GpuBlend::Additive,
        aestra_core::BlendMode::Multiply => GpuBlend::Multiply,
    }
}

/// Asynchronously observed owner-pool counters, invalidated by history discontinuities.
#[derive(Component, Debug, Default)]
pub struct GpuTrailStatistics {
    epoch: Option<u32>,
    usage: aestra_runtime::TrailUsage,
}

impl GpuTrailStatistics {
    pub fn usage(
        &self,
        instance: &aestra_runtime::EffectInstance,
    ) -> Option<aestra_runtime::TrailUsage> {
        (self.epoch == Some(instance.history_epoch())).then_some(self.usage)
    }
}

#[derive(Component)]
struct GpuTrailReadbackOwner(Entity);

fn receive_trail_statistics(
    event: On<ReadbackComplete>,
    owners: Query<&GpuTrailReadbackOwner>,
    mut players: Query<(&PresentedEffect, &mut GpuTrailStatistics)>,
) {
    let Ok(owner) = owners.get(event.event_target()) else {
        return;
    };
    let Ok((player, mut statistics)) = players.get_mut(owner.0) else {
        return;
    };
    let words: Vec<u32> = event.to_shader_type();
    let mut usage = aestra_runtime::TrailUsage::default();
    for (index, emitter) in player.effect().emitters.iter().enumerate() {
        if !emitter.enabled
            || !emitter
                .renderers
                .iter()
                .any(|r| matches!(r.kind, aestra_runtime::RendererPlanKind::Trail { .. }))
        {
            continue;
        }
        let Some(stats) = words.get(2 + index * 6..2 + (index + 1) * 6) else {
            return;
        };
        if stats[4] != player.instance.history_epoch() {
            return;
        }
        usage.occupied = usage.occupied.saturating_add(stats[0]);
        usage.retired = usage.retired.saturating_add(stats[1]);
        usage.evictions = usage.evictions.saturating_add(stats[2]);
        usage.truncated = usage.truncated.saturating_add(stats[5]);
    }
    statistics.epoch = Some(player.instance.history_epoch());
    statistics.usage = usage;
}

pub(crate) fn receive_readback(
    event: On<ReadbackComplete>,
    owners: Query<&GpuReadbackOwner>,
    mut players: Query<&mut PresentedEffect>,
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
            .filter(|particle| particle.packed_emitter_alive & 0xffff != 0)
            .map(|particle| aestra_runtime::ParticleSample {
                emitter_index: (particle.packed_emitter_alive >> 16) as usize,
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
        shader,
        entry_point: Some("update_trails".into()),
        ..default()
    });
    commands.insert_resource(SimulationPipeline {
        layout,
        reset,
        simulate,
        link_ribbons,
        update_trails,
    });
}

/// Builds the stateful backend's compute pipelines (hybrid roadmap M6) from the unified
/// `aestra_gpu::stateful_simulation_wgsl` module. The nine-binding layout is shared across the three
/// entry points (each uses a subset). Gated on the same device limits as the analytic pipeline plus
/// the nine-binding requirement; when unavailable the resource is simply absent and stateful effects
/// fall back like any unsupported artifact.
fn init_stateful_pipeline(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    pipeline_cache: Res<PipelineCache>,
    render_device: Res<RenderDevice>,
    adapter: Res<RenderAdapter>,
) {
    const STATEFUL_BINDING_COUNT: u32 = 9;
    let limits = render_device.limits();
    if !adapter
        .get_downlevel_capabilities()
        .flags
        .contains(DownlevelFlags::COMPUTE_SHADERS)
        || limits.max_storage_buffers_per_shader_stage < STATEFUL_BINDING_COUNT
        || limits.max_bindings_per_bind_group < STATEFUL_BINDING_COUNT
        || limits.max_compute_invocations_per_workgroup < WORKGROUP_SIZE
        || limits.max_compute_workgroup_size_x < WORKGROUP_SIZE
    {
        return;
    }
    let layout = BindGroupLayoutDescriptor::new(
        "aestra_gpu_stateful_simulation",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::COMPUTE,
            (
                storage_buffer::<Vec<f32>>(false),           // 0: persistent state
                storage_buffer::<Vec<u32>>(false),           // 1: free list
                storage_buffer::<Vec<u32>>(false),           // 2: free count (atomic)
                storage_buffer::<Vec<u32>>(false),           // 3: spawn counter (atomic)
                storage_buffer_read_only::<Vec<u32>>(false), // 4: params
                storage_buffer::<Vec<GpuParticle>>(false),   // 5: presentation output
                storage_buffer::<Vec<u32>>(false),           // 6: alive indices (compaction)
                storage_buffer::<Vec<u32>>(false),           // 7: indirect draw commands (atomic)
                storage_buffer::<Vec<u32>>(false),           // 8: live counters (atomic)
            ),
        ),
    );
    let shader = asset_server.load(STATEFUL_SIMULATION_SHADER_PATH);
    let pipeline = |label: &'static str, entry: &'static str| {
        pipeline_cache.queue_compute_pipeline(ComputePipelineDescriptor {
            label: Some(label.into()),
            layout: vec![layout.clone()],
            shader: shader.clone(),
            entry_point: Some(entry.into()),
            ..default()
        })
    };
    commands.insert_resource(StatefulSimulationPipeline {
        layout: layout.clone(),
        death_integrate: pipeline("aestra stateful death+integrate", "death_integrate"),
        spawn: pipeline("aestra stateful spawn", "spawn"),
        present: pipeline("aestra stateful present", "present"),
    });
}

fn prepare_bind_groups(
    mut commands: Commands,
    pipeline: Option<Res<SimulationPipeline>>,
    render_device: Res<RenderDevice>,
    pipeline_cache: Res<PipelineCache>,
    buffers: Res<RenderAssets<GpuShaderBuffer>>,
    effects: Query<(Entity, &GpuEffectBuffers)>,
) {
    let _span = tracing::info_span!("aestra::gpu::bind_groups").entered();
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

type TrailHistories = BTreeMap<
    Entity,
    (
        AssetId<ShaderBuffer>,
        Arc<trail_checkpoints::TrailContext>,
        trail_checkpoints::TrailHistory,
    ),
>;

/// The persistent GPU state for one stateful effect (hybrid roadmap M6). Unlike the analytic
/// particle buffer — recomputed from scratch every frame — this survives across frames so per-particle
/// state advances incrementally; it is only reallocated when the effect's capacity changes. The
/// stateful dispatch (next increment) advances `state` with the death loop and extracts it into the
/// presentation buffer.
struct StatefulPersistentState {
    /// Slot capacity these buffers were sized for; a change triggers reallocation.
    records: u32,
    /// `f32` components of persistent state per slot.
    stride: u32,
    /// `records * stride` persistent state floats (position, velocity, age, lifetime, ordinal bits).
    state: Buffer,
    /// Free-slot indices for death/reuse; initialised to every slot free.
    free_list: Buffer,
    /// Atomic count of free slots; initialised to `records`.
    free_count: Buffer,
    /// Atomic spawn ordinal counter; initialised to 0.
    spawn_counter: Buffer,
    /// The last fixed tick the persistent state was advanced to. A target below this is a backward
    /// seek, handled by restart+replay: reallocate to the initial state and replay forward.
    last_tick: u32,
    /// Fractional spawn carry, so a non-integer per-tick spawn rate emits the right long-run count.
    spawn_accumulator: f32,
}

impl StatefulPersistentState {
    /// Allocates and initialises one emitter's persistent buffers for `records` slots: zeroed state, a
    /// full free list (`0..records`), a free count of `records`, and a spawn counter of 0.
    fn allocate(render_device: &RenderDevice, records: u32, stride: u32) -> Self {
        let state_floats = records as usize * stride as usize;
        let state = render_device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some("aestra stateful state"),
            contents: &vec![0_u8; state_floats * std::mem::size_of::<f32>()],
            usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
        });
        let free_list_bytes: Vec<u8> = (0..records).flat_map(u32::to_le_bytes).collect();
        let free_list = render_device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some("aestra stateful free list"),
            contents: &free_list_bytes,
            usage: BufferUsages::STORAGE,
        });
        let free_count = render_device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some("aestra stateful free count"),
            contents: &records.to_le_bytes(),
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
        });
        let spawn_counter = render_device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some("aestra stateful spawn counter"),
            contents: &0_u32.to_le_bytes(),
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
        });
        Self {
            records,
            stride,
            state,
            free_list,
            free_count,
            spawn_counter,
            last_tick: 0,
            spawn_accumulator: 0.0,
        }
    }
}

/// Per-entity persistent state for stateful effects, kept in the render world across frames (the
/// `TrailHistories` pattern). One [`StatefulPersistentState`] per stateful emitter, in the same order
/// as the effect's `stateful_dispatch`. Populated by [`prepare_stateful_states`].
#[derive(Resource, Default)]
struct StatefulStates(BTreeMap<Entity, Vec<StatefulPersistentState>>);

/// Allocates and retains one set of persistent state buffers per stateful emitter, reallocating only
/// when an emitter's capacity changes (or the emitter set changes) and dropping them when the effect
/// stops being stateful or is removed. This is the render-world lifecycle the analytic path does not
/// need (it recomputes every frame); the stateful dispatch reads these buffers.
fn prepare_stateful_states(
    mut states: ResMut<StatefulStates>,
    render_device: Res<RenderDevice>,
    effects: Query<(Entity, &GpuEffectBuffers)>,
) {
    states.0.retain(|entity, _| {
        effects
            .get(*entity)
            .is_ok_and(|(_, effect)| !effect.stateful_dispatch.is_empty())
    });
    for (entity, effect) in &effects {
        if effect.stateful_dispatch.is_empty() {
            continue;
        }
        let stride = effect.simulation_state.stride;
        let current = states.0.get(&entity);
        let matches = current.is_some_and(|states| {
            states.len() == effect.stateful_dispatch.len()
                && states
                    .iter()
                    .zip(&effect.stateful_dispatch)
                    .all(|(state, dispatch)| {
                        state.records == dispatch.capacity && state.stride == stride
                    })
        });
        if !matches {
            let allocated = effect
                .stateful_dispatch
                .iter()
                .map(|dispatch| {
                    StatefulPersistentState::allocate(&render_device, dispatch.capacity, stride)
                })
                .collect();
            states.0.insert(entity, allocated);
        }
    }
}

/// The canonical fixed simulation tick, matching `aestra_runtime::StatefulSimulation::TICK_DT`.
const STATEFUL_TICK_DT: f32 = 1.0 / 60.0;
/// Cap on fixed ticks advanced in a single frame, so a large seek or a first frame far into the
/// timeline cannot stall the GPU; the simulation catches up over subsequent frames.
const STATEFUL_MAX_CATCHUP_TICKS: u32 = 300;

/// Encodes one stateful *emitter's* per-frame GPU work (hybrid roadmap M6): advance its persistent
/// state from its last tick to the tick for `simulation_time` (death loop + spawn per tick), then
/// reset its indirect instance count and run present, which extracts presentation and compacts its
/// live slots into the effect-wide alive/indirect/counters buffers the render path draws. A backward
/// seek is handled by restart+replay — the state is reset to tick 0 and replayed forward (the derived
/// seek mode). The caller clears the shared live counter once before the emitter loop and stamps the
/// statistics telemetry once after it. Every kernel here is conformance-proven on real GPU.
#[allow(clippy::too_many_arguments)]
fn dispatch_stateful_effect(
    device: &RenderDevice,
    encoder: &mut CommandEncoder,
    death_integrate: &ComputePipeline,
    spawn: &ComputePipeline,
    present: &ComputePipeline,
    layout: &BindGroupLayout,
    persistent: &mut StatefulPersistentState,
    dispatch: &StatefulDispatch,
    particles: &Buffer,
    alive: &Buffer,
    indirect: &Buffer,
    counters: &Buffer,
    simulation_time: f32,
) {
    let target_tick = (simulation_time.max(0.0) / STATEFUL_TICK_DT) as u32;
    // Backward seek: restart from the initial state and replay forward (never integrate in reverse).
    if target_tick < persistent.last_tick {
        *persistent =
            StatefulPersistentState::allocate(device, persistent.records, persistent.stride);
    }
    let capacity = dispatch.capacity;
    let workgroups = capacity.div_ceil(WORKGROUP_SIZE);
    let ticks = (target_tick - persistent.last_tick).min(STATEFUL_MAX_CATCHUP_TICKS);

    // params = [capacity, spawn_per_tick, seed_lo, seed_hi, speed, lifetime, dt, gx, gy, gz,
    //           emitter_index, slot_offset]; only spawn_per_tick varies across ticks.
    let params_bytes = |spawn_per_tick: u32| -> Vec<u8> {
        [
            capacity,
            spawn_per_tick,
            dispatch.seed as u32,
            (dispatch.seed >> 32) as u32,
            dispatch.speed.to_bits(),
            dispatch.lifetime.to_bits(),
            STATEFUL_TICK_DT.to_bits(),
            dispatch.gravity[0].to_bits(),
            dispatch.gravity[1].to_bits(),
            dispatch.gravity[2].to_bits(),
            dispatch.emitter_index,
            dispatch.slot_offset,
        ]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect()
    };
    let bind_group = |params: &Buffer| {
        device.create_bind_group(
            Some("aestra_gpu_stateful"),
            layout,
            &BindGroupEntries::sequential((
                persistent.state.as_entire_buffer_binding(),
                persistent.free_list.as_entire_buffer_binding(),
                persistent.free_count.as_entire_buffer_binding(),
                persistent.spawn_counter.as_entire_buffer_binding(),
                params.as_entire_buffer_binding(),
                particles.as_entire_buffer_binding(),
                alive.as_entire_buffer_binding(),
                indirect.as_entire_buffer_binding(),
                counters.as_entire_buffer_binding(),
            )),
        )
    };

    // Per-tick params + bind groups (the spawn count varies with the fractional accumulator). One pass
    // keeps the persistent state coherent across ticks (WebGPU orders dispatches within a pass).
    let mut tick_resources = Vec::with_capacity(ticks as usize);
    for _ in 0..ticks {
        persistent.spawn_accumulator += dispatch.spawn_rate * STATEFUL_TICK_DT;
        let spawn_count = persistent.spawn_accumulator.floor();
        persistent.spawn_accumulator -= spawn_count;
        let spawn_count = (spawn_count as u32).min(capacity);
        let params = device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some("aestra stateful tick params"),
            contents: &params_bytes(spawn_count),
            usage: BufferUsages::STORAGE,
        });
        let group = bind_group(&params);
        tick_resources.push((params, group));
    }
    if !tick_resources.is_empty() {
        let mut pass = encoder.begin_compute_pass(&ComputePassDescriptor {
            label: Some("aestra stateful advance"),
            timestamp_writes: None,
        });
        for (_, group) in &tick_resources {
            pass.set_bind_group(0, group, &[]);
            pass.set_pipeline(death_integrate);
            pass.dispatch_workgroups(workgroups, 1, 1);
            pass.set_pipeline(spawn);
            pass.dispatch_workgroups(workgroups, 1, 1);
        }
    }
    persistent.last_tick += ticks;

    // Reset this emitter's indirect instance count, then present + compact. The vertex count (word 0
    // of the draw command) is preserved; only the instance count (word 1) is zeroed so the compaction
    // rebuilds it. The shared live counter is cleared once by the caller before the emitter loop.
    let instance_count_offset = u64::from(dispatch.emitter_index * 4 + 1) * 4;
    encoder.clear_buffer(indirect, instance_count_offset, Some(4));
    let present_params = device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("aestra stateful present params"),
        contents: &params_bytes(0),
        usage: BufferUsages::STORAGE,
    });
    let present_group = bind_group(&present_params);
    {
        let mut pass = encoder.begin_compute_pass(&ComputePassDescriptor {
            label: Some("aestra stateful present"),
            timestamp_writes: None,
        });
        pass.set_bind_group(0, &present_group, &[]);
        pass.set_pipeline(present);
        pass.dispatch_workgroups(workgroups, 1, 1);
    }
}

/// Stamps the particle-statistics telemetry trailer the analytic reset writes, so the live-count
/// readback accepts a stateful frame: `[MAGIC, context token, history epoch, time]` at the indirect
/// buffer's telemetry offset (`emitter_count * 4`). Called once per effect after every emitter has
/// presented; the per-emitter alive counts were rebuilt by each present's compaction.
fn stamp_stateful_statistics(
    device: &RenderDevice,
    encoder: &mut CommandEncoder,
    indirect: &Buffer,
    emitter_count: u32,
    statistics_token: u32,
    history_epoch: u32,
    simulation_time: f32,
) {
    let telemetry: [u32; 4] = [
        aestra_gpu::PARTICLE_STATISTICS_MAGIC,
        statistics_token,
        history_epoch,
        simulation_time.to_bits(),
    ];
    let telemetry_src = device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("aestra stateful statistics telemetry"),
        contents: &telemetry
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<u8>>(),
        usage: BufferUsages::COPY_SRC,
    });
    let telemetry_offset = u64::from(emitter_count * 4) * 4;
    encoder.copy_buffer_to_buffer(&telemetry_src, 0, indirect, telemetry_offset, 16);
}

fn run_simulation(
    mut render_context: RenderContext,
    pipeline_cache: Res<PipelineCache>,
    pipeline: Option<Res<SimulationPipeline>>,
    effects: Query<(
        Entity,
        &bevy::render::sync_world::MainEntity,
        &GpuEffectBuffers,
        &GpuBindGroup,
    )>,
    mesh_draws: Query<(&GpuDrawInstance, &render::PreparedMeshDraw)>,
    gpu_resources: (
        Res<RenderAssets<GpuShaderBuffer>>,
        Res<RenderDevice>,
        Res<bevy::render::renderer::RenderQueue>,
        Res<simulation_timing::TimingMailbox>,
    ),
    state: (
        Local<TrailHistories>,
        Local<simulation_timing::SimulationTimer>,
        Option<Res<StatefulSimulationPipeline>>,
        ResMut<StatefulStates>,
    ),
) {
    let _span = tracing::info_span!("aestra::gpu::simulate").entered();
    let Some(pipeline) = pipeline else {
        return;
    };
    let (buffers, render_device, queue, timing_mailbox) = gpu_resources;
    let (mut histories, mut timer, stateful_pipeline, mut stateful_states) = state;
    // Resolve the stateful compute pipelines once (present only when the device supports the path and
    // the pipelines have finished compiling). The stateful branch below drives one enabled stateful
    // emitter end-to-end; other effects take the analytic path unchanged.
    let stateful = stateful_pipeline.as_ref().and_then(|sp| {
        Some((
            sp,
            pipeline_cache.get_compute_pipeline(sp.death_integrate)?,
            pipeline_cache.get_compute_pipeline(sp.spawn)?,
            pipeline_cache.get_compute_pipeline(sp.present)?,
        ))
    });
    let link_ribbons = pipeline_cache.get_compute_pipeline(pipeline.link_ribbons);
    let update_trails = pipeline_cache.get_compute_pipeline(pipeline.update_trails);
    let (Some(reset), Some(simulate)) = (
        pipeline_cache.get_compute_pipeline(pipeline.reset),
        pipeline_cache.get_compute_pipeline(pipeline.simulate),
    ) else {
        return;
    };
    // GPU timestamp span around the whole simulation. This is a no-op unless the
    // host app added `RenderDiagnosticsPlugin`; on Vulkan/DX12 it records real GPU
    // elapsed time, surfaced through the diagnostics store and Tracy.
    let diagnostics = render_context.diagnostic_recorder();
    let diagnostics = diagnostics.as_deref();
    let gpu_span = diagnostics.time_span(render_context.command_encoder(), "aestra::gpu::simulate");
    let mut timing_batch = timer.begin(&render_device, queue.get_timestamp_period());
    histories.retain(|entity, _| effects.get(*entity).is_ok_and(|(_, _, e, _)| e.has_trails));
    let mut allocated: u64 = histories.values().map(|h| h.2.checkpoints.bytes()).sum();
    for (entity, main_entity, effect, bind_group) in &effects {
        // Stateful effects (hybrid roadmap M6) run their own persistent path instead of the analytic
        // reset+simulate, feeding the same alive/indirect/particles buffers the render path draws. One
        // set of persistent buffers per stateful emitter; the shared live counter and statistics
        // telemetry are reset/stamped once around the emitter loop.
        if !effect.stateful_dispatch.is_empty() {
            if let (Some((sp, death_integrate, spawn, present)), Some(persistent_states)) =
                (&stateful, stateful_states.0.get_mut(&entity))
            {
                let render_buffers = [
                    &effect.particles,
                    &effect.alive,
                    &effect.indirect,
                    &effect.counters,
                ]
                .map(|handle| buffers.get(handle).map(|buffer| &buffer.buffer));
                if let [Some(particles), Some(alive), Some(indirect), Some(counters)] =
                    render_buffers
                    && persistent_states.len() == effect.stateful_dispatch.len()
                {
                    let layout = pipeline_cache.get_bind_group_layout(&sp.layout);
                    // Clear the shared live counter once, before any emitter's present bumps it.
                    render_context
                        .command_encoder()
                        .clear_buffer(counters, 0, Some(4));
                    for (dispatch, persistent) in effect
                        .stateful_dispatch
                        .iter()
                        .zip(persistent_states.iter_mut())
                    {
                        dispatch_stateful_effect(
                            &render_device,
                            render_context.command_encoder(),
                            death_integrate,
                            spawn,
                            present,
                            &layout,
                            persistent,
                            dispatch,
                            particles,
                            alive,
                            indirect,
                            counters,
                            effect.simulation_time,
                        );
                    }
                    // Stamp the statistics telemetry once, after every emitter has presented.
                    stamp_stateful_statistics(
                        &render_device,
                        render_context.command_encoder(),
                        indirect,
                        effect.stateful_dispatch[0].emitter_count,
                        effect.statistics_token,
                        effect.history_epoch,
                        effect.simulation_time,
                    );
                }
            }
            continue;
        }
        if (effect.has_ribbons && link_ribbons.is_none())
            || (effect.has_trails && update_trails.is_none())
        {
            continue;
        }
        let replay = if effect.has_trails {
            let Some(globals) = buffers.get(&effect.globals) else {
                continue;
            };
            let Some(render_globals) = buffers.get(&effect.render_globals) else {
                continue;
            };
            let handles = [
                &effect.particles,
                &effect.alive,
                &effect.dead,
                &effect.counters,
                &effect.indirect,
                &effect.aux,
            ];
            let Some(state) = handles
                .iter()
                .map(|h| buffers.get(*h).map(|b| &b.buffer))
                .collect::<Option<Vec<_>>>()
            else {
                continue;
            };
            let history = histories.entry(entity).or_insert_with(|| {
                (
                    effect.particles.id(),
                    effect.checkpoint_context.clone(),
                    default(),
                )
            });
            if history.0 != effect.particles.id() {
                allocated -= history.2.checkpoints.bytes();
                *history = (
                    effect.particles.id(),
                    effect.checkpoint_context.clone(),
                    default(),
                );
            } else if history.1 != effect.checkpoint_context {
                allocated -= history.2.checkpoints.bytes();
                history.2.checkpoints = default();
                if history.1.motion.is_some() || effect.checkpoint_context.motion.is_some() {
                    history.2.replay = default();
                    // Placement edits while paused at time zero need an explicit reset too.
                    for &(_, root) in &effect.trail_roots {
                        render_context.command_encoder().clear_buffer(
                            state[0],
                            root as u64 * 48 + 40,
                            Some(4),
                        );
                    }
                } else {
                    history.2.replay.context_changed();
                }
                history.1 = effect.checkpoint_context.clone();
            }
            let previous = history.2.checkpoints.bytes();
            history.2.sync_buffers(&state);
            allocated = allocated - previous + history.2.checkpoints.bytes();
            if effect.checkpoint_context.motion.is_some() {
                history.2.replay.prepare_tracked(effect.simulation_time);
            }
            if history
                .2
                .replay
                .needs_restore(effect.history_epoch, effect.simulation_time)
                && let Some(time) = history.2.checkpoints.restore(
                    render_context.command_encoder(),
                    &state,
                    effect.simulation_time,
                )
            {
                trail_checkpoints::rebase_epoch(
                    render_context.command_encoder(),
                    &globals.buffer,
                    state[5],
                    state[3],
                    &effect.trail_roots,
                );
                history.2.replay.restore(effect.history_epoch, time);
            }
            let times = history
                .2
                .replay
                .observations(effect.history_epoch, effect.simulation_time);
            // A queue.write_buffer loop would expose only the final time to all
            // dispatches. Encoder copies make each observation visible in order.
            let placement = Mat4::from_cols_array(&std::array::from_fn(|i| {
                f32::from_bits(effect.checkpoint_context.key[6 + i])
            }));
            let bytes = crate::host_transform::observation_bytes(
                &times,
                placement,
                effect.checkpoint_context.motion.as_deref(),
            );
            let times_buffer = render_device.create_buffer_with_data(&BufferInitDescriptor {
                label: Some("aestra trail replay times and host transforms"),
                contents: &bytes,
                usage: BufferUsages::COPY_SRC,
            });
            // GpuRenderGlobals starts with a mat4, followed by time. During a
            // multi-frame replay, draw the processed time rather than expiring
            // all intermediate history against the eventual seek target.
            render_context.command_encoder().copy_buffer_to_buffer(
                &times_buffer,
                (times.len() as u64 - 1) * 68,
                &render_globals.buffer,
                64,
                4,
            );
            render_context.command_encoder().copy_buffer_to_buffer(
                &times_buffer,
                (times.len() as u64 - 1) * 68 + 4,
                &render_globals.buffer,
                0,
                64,
            );
            Some((times_buffer, times, globals, state))
        } else {
            None
        };
        let observation_count = replay.as_ref().map_or(1, |(_, times, _, _)| times.len());
        let observed_time = replay
            .as_ref()
            .map_or(effect.simulation_time, |(_, times, _, _)| {
                *times.last().unwrap()
            });
        let timing_index = timing_batch.as_mut().and_then(|batch| {
            batch.instance(main_entity.id(), effect.statistics_token, observed_time)
        });
        for observation in 0..observation_count {
            if let Some((times, _, globals, _)) = &replay {
                render_context.command_encoder().copy_buffer_to_buffer(
                    times,
                    observation as u64 * 68,
                    &globals.buffer,
                    0,
                    4,
                );
                render_context.command_encoder().copy_buffer_to_buffer(
                    times,
                    observation as u64 * 68 + 4,
                    &globals.buffer,
                    32,
                    64,
                );
            }
            let mut pass =
                render_context
                    .command_encoder()
                    .begin_compute_pass(&ComputePassDescriptor {
                        label: Some("aestra simulation"),
                        timestamp_writes: timing_batch.as_ref().zip(timing_index).and_then(
                            |(batch, index)| {
                                batch.writes(
                                    index,
                                    observation == 0,
                                    observation + 1 == observation_count,
                                )
                            },
                        ),
                    });
            pass.set_bind_group(0, &bind_group.0, &[]);
            pass.set_pipeline(reset);
            pass.dispatch_workgroups(1, 1, 1);
            pass.set_pipeline(simulate);
            pass.dispatch_workgroups(effect.workgroups, 1, 1);
            if effect.has_ribbons
                && let Some(link_ribbons) = link_ribbons
            {
                pass.set_pipeline(link_ribbons);
                pass.dispatch_workgroups(effect.ribbon_workgroups, 1, 1);
            }
            if effect.has_trails
                && let Some(update_trails) = update_trails
            {
                pass.set_pipeline(update_trails);
                pass.dispatch_workgroups(effect.ribbon_workgroups, 1, 1);
            }
            drop(pass);
            if let Some((_, times, _, state)) = &replay {
                let history = &mut histories.get_mut(&entity).unwrap().2;
                let time = times[observation];
                if history.replay.should_capture(time) {
                    let previous = history.checkpoints.bytes();
                    history.checkpoints.capture(
                        &render_device,
                        render_context.command_encoder(),
                        state,
                        time,
                        trail_checkpoints::MEMORY_LIMIT.saturating_sub(allocated),
                    );
                    allocated = allocated - previous + history.checkpoints.bytes();
                }
            }
        }
    }
    // Copy only instance counts after simulation; mesh commands retain their own geometry ranges.
    // This avoids CPU readback and preserves the existing per-emitter simulation ABI.
    for (draw, mesh) in &mesh_draws {
        if let Some(indirect) = buffers.get(&draw.indirect) {
            render_context.command_encoder().copy_buffer_to_buffer(
                &indirect.buffer,
                draw.indirect_offset + 4,
                &mesh.indirect,
                4,
                4,
            );
        }
    }
    gpu_span.end(render_context.command_encoder());
    if let Some(batch) = timing_batch {
        batch.finish(&mut render_context, timing_mailbox.clone());
    }
}

fn mesh_from_emitter(transform: aestra_core::EmitterTransform) -> Mat4 {
    let scale = Vec3::from_array(transform.scale);
    Mat4::from_scale_rotation_translation(
        scale / scale.abs().max_element().max(0.000001),
        Quat::from_array(transform.rotation),
        Vec3::ZERO,
    )
}

fn gpu_render_mode(mode: EffectRenderMode) -> GpuRenderMode {
    match mode {
        EffectRenderMode::Rendered => GpuRenderMode::Rendered,
        EffectRenderMode::Wireframe => GpuRenderMode::Wireframe,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aestra_compiler::EffectCompiler;
    use aestra_core::material::{
        LEGACY_SPRITE_SOFTNESS_PARAMETER, MaterialEvaluationDomain, MaterialExpression,
        MaterialExpressionKind, MaterialInstance, MaterialParameter, MaterialParameterValue,
        MaterialProgram, MaterialProgramRef, MaterialRenderState, MaterialValue, MaterialValueType,
    };
    use aestra_core::{
        AssetDefinition, BlendMode, Curve, CurveKey, EffectAsset, Emitter, EmitterShape,
        MODULE_EMISSION, MODULE_MOTION, MaterialDefinition, MaterialExpressionId, MaterialInput,
        MaterialParameterId, MaterialProperties, PropertyEvaluationDomain, PropertySource,
        PropertySourceValue, RendererInstance, ScalarRange, UvRect, Value, Vec3Curve, Vec3Range,
    };
    use aestra_gpu::INDIRECT_DRAW_BYTES;
    use aestra_gpu::indirect_draw_commands;
    use aestra_runtime::EffectInstance;
    use std::sync::Arc;

    #[test]
    fn trail_statistics_are_unavailable_until_readback_and_after_history_reset() {
        let mut effect = EffectAsset::new("Telemetry", 2.0);
        effect.emitters.push(Emitter::basic_sprite("Emitter", 2.0));
        let mut instance = EffectInstance::new(Arc::new(
            EffectCompiler::default().compile(&effect).unwrap(),
        ));
        assert_eq!(GpuTrailStatistics::default().usage(&instance), None);
        let stats = GpuTrailStatistics {
            epoch: Some(instance.history_epoch()),
            usage: aestra_runtime::TrailUsage {
                occupied: 5,
                retired: 2,
                evictions: 7,
                truncated: 3,
            },
        };
        assert_eq!(stats.usage(&instance).unwrap().occupied, 5);
        instance.set_playback_time(0.5);
        assert!(stats.usage(&instance).is_some());
        instance.seek(1.0);
        assert!(stats.usage(&instance).is_none());
    }

    #[test]
    fn rendering_and_ribbon_culling_receive_the_same_propagated_transform() {
        let mut effect = EffectAsset::new("Bounds upload", 2.0);
        effect.emitters.push(Emitter::basic_sprite("Emitter", 2.0));
        let compiled = Arc::new(EffectCompiler::default().compile(&effect).unwrap());
        let mut app = App::new();
        app.add_plugins(bevy::transform::TransformPlugin)
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<ShaderBuffer>>();
        install_visibility_updates(&mut app);
        let handle = app
            .world_mut()
            .resource_mut::<Assets<ShaderBuffer>>()
            .add(ShaderBuffer::default());
        let parent = app.world_mut().spawn(Transform::IDENTITY).id();
        let player = app
            .world_mut()
            .spawn((
                ChildOf(parent),
                Transform::IDENTITY,
                GpuParticleStatistics::new(&EffectInstance::new(compiled.clone())),
                PresentedEffect::new(compiled),
                GpuEffectBuffers {
                    emitters: default(),
                    renderers: default(),
                    particles: default(),
                    alive: default(),
                    dead: default(),
                    counters: default(),
                    indirect: default(),
                    globals: default(),
                    aux: default(),
                    render_globals: handle.clone(),
                    workgroups: 1,
                    has_ribbons: true,
                    has_trails: false,
                    ribbon_workgroups: 1,
                    total_slots: 1,
                    simulation_time: 0.0,
                    history_epoch: 0,
                    statistics_token: 0,
                    checkpoint_context: default(),
                    trail_roots: vec![],
                    simulation_state: default(),
                    stateful_dispatch: Vec::new(),
                },
            ))
            .id();
        let model = aestra_gpu::ribbon_bounds::RibbonParticleBounds {
            position_half_extents: Vec3::new(1.0, 5.0, 2.0),
            maximum_half_width: 3.0,
        };
        let draw = app
            .world_mut()
            .spawn((
                ChildOf(player),
                Transform::IDENTITY,
                Aabb::default(),
                ribbon_bounds::RibbonBoundsSource(Some(model)),
                HostMotionDraw,
                visibility::NoFrustumCulling,
            ))
            .id();
        for transform in [
            Transform::IDENTITY,
            Transform::from_xyz(123.0, -42.0, 70.0)
                .with_rotation(Quat::from_rotation_y(0.7))
                .with_scale(Vec3::new(-0.1, 3.0, 2.0)),
            Transform::from_xyz(-500.0, 20.0, 0.0),
        ] {
            *app.world_mut().get_mut::<Transform>(parent).unwrap() = transform;
            app.update();
            let expected = app.world().get::<GlobalTransform>(player).unwrap().affine();
            let buffers = app.world().resource::<Assets<ShaderBuffer>>();
            let bytes = buffers.get(&handle).unwrap().data.as_ref().unwrap();
            let actual: [f32; 16] = std::array::from_fn(|i| {
                f32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap())
            });
            assert_eq!(
                Mat4::from_cols_array(&actual),
                Mat4::from(expected),
                "same-frame render upload"
            );
            assert_eq!(
                app.world().get::<GlobalTransform>(draw).unwrap().affine(),
                expected
            );
            let aabb = app.world().get::<Aabb>(draw).unwrap();
            assert_eq!(
                Vec3::from(aabb.half_extents),
                model.half_extents(Mat3::from(expected.matrix3)).unwrap()
            );
            assert!(
                !app.world()
                    .entity(draw)
                    .contains::<visibility::NoFrustumCulling>()
            );
        }
        let track = aestra_runtime::CompiledHostTransformTrack::new(
            aestra_core::HostTransformTrack::from_pose_keys(
                vec![
                    aestra_core::HostTransformKey {
                        time: 0.0,
                        transform: default(),
                    },
                    aestra_core::HostTransformKey {
                        time: 1.0,
                        transform: aestra_core::EmitterTransform {
                            translation: [20.0, 30.0, 10.0],
                            rotation: Quat::from_rotation_y(0.8).to_array(),
                            scale: [0.5, 2.0, 1.5],
                        },
                    },
                ],
                false,
            ),
        )
        .unwrap();
        app.world_mut()
            .get_mut::<PresentedEffect>(player)
            .unwrap()
            .instance
            .set_host_transform_track(Some(Arc::new(track.clone())));
        let clip = aestra_core::EmitterTransform {
            rotation: Quat::from_rotation_z(0.7).to_array(),
            scale: [2.0, 0.5, 1.0],
            ..default()
        };
        app.world_mut()
            .get_mut::<PresentedEffect>(player)
            .unwrap()
            .instance
            .set_inherited_host_transform(Arc::new(
                aestra_runtime::InheritedHostTransform::default().for_child(
                    Some(Arc::new(track.clone())),
                    clip,
                    0.25,
                ),
            ));
        for time in [0.0, 0.5, 1.0, 0.25, 0.25] {
            app.world_mut()
                .get_mut::<PresentedEffect>(player)
                .unwrap()
                .instance
                .seek(time);
            app.update();
            let placement =
                Mat4::from(app.world().get::<GlobalTransform>(player).unwrap().affine());
            let pose = app
                .world()
                .get::<PresentedEffect>(player)
                .unwrap()
                .instance
                .host_transform_at(time);
            let expected = placement
                * crate::host_transform::matrix(track.sample(time + 0.25))
                * crate::host_transform::matrix(clip)
                * crate::host_transform::matrix(pose);
            let actual = Mat4::from(app.world().get::<GlobalTransform>(draw).unwrap().affine());
            assert!(actual.abs_diff_eq(expected, 1e-5));
            let bytes = app
                .world()
                .resource::<Assets<ShaderBuffer>>()
                .get(&handle)
                .unwrap()
                .data
                .as_ref()
                .unwrap();
            let uploaded = Mat4::from_cols_array(&std::array::from_fn(|i| {
                f32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap())
            }));
            assert!(
                uploaded.abs_diff_eq(actual, 1e-5),
                "GPU and Bevy must use the same frame's pose"
            );
        }
    }

    #[test]
    fn semantic_material_instance_reaches_the_gpu_draw_artifact_without_legacy_material_data() {
        let softness = MaterialParameterId::from_u128(0xA501);
        let mut program = MaterialProgram::additive_sprite("Deterministic additive flame");
        program.parameters.push(MaterialParameter {
            id: softness,
            name: LEGACY_SPRITE_SOFTNESS_PARAMETER.into(),
            value_type: MaterialValueType::Float,
            evaluation_domain: MaterialEvaluationDomain::Instance,
            default: Some(MaterialValue::Float(1.0)),
        });
        program.expressions.push(MaterialExpression {
            id: MaterialExpressionId::from_u128(0xA502),
            kind: MaterialExpressionKind::Parameter(softness),
        });
        let material = MaterialId::from_u128(0xA500);
        let instance = MaterialInstance {
            id: material,
            program: MaterialProgramRef::Project(program.id),
            values: BTreeMap::from([(
                softness,
                MaterialParameterValue::Constant(MaterialValue::Float(0.08)),
            )]),
            render_state: MaterialRenderState::additive_sprite(),
        };
        let mut effect = EffectAsset::new("Semantic flame fixture", 2.0);
        let mut emitter = Emitter::basic_sprite("Flame", effect.duration);
        let emitter_id = emitter.id;
        emitter.renderers[0].material = material;
        effect.material_instances.push(instance.clone());
        effect.emitters.push(emitter);
        let programs = BTreeMap::from([(program.id, program)]);
        let compiled_effect = Arc::new(
            EffectCompiler::default()
                .compile_with_material_programs(&effect, &programs)
                .unwrap(),
        );
        assert!(compiled_effect.material(material).is_none());

        let presented = PresentedEffect::new(compiled_effect);

        let mut artifact = GpuEffectArtifact::from_instance(&presented.instance).unwrap();
        assert_eq!(artifact.renderers.len(), 1);
        assert_eq!(artifact.renderers[0].blend_mode, GpuBlend::Additive as u32);
        assert_eq!(artifact.renderers[0].softness, 1.0);
        assert!(presented.material_binding(material).is_some());
        assert!(
            presented
                .material_binding_for_emitter(material, emitter_id)
                .is_some()
        );
        apply_semantic_sprite_compatibility_to_renderers(&mut artifact.renderers, &presented);
        assert_eq!(artifact.renderers[0].softness, 0.08);
    }

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
        let effect = EffectAsset::from_ron(include_str!(
            "../../../assets/test/effects/plasma_burst.aestra.ron"
        ))
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
}
