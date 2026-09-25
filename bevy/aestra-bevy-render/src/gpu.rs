//! Bevy render-world adapter for engine-neutral Aestra GPU artifacts.

mod bounds;
mod extension_stages;
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
use aestra_runtime::{RendererPlanKind, SeekQuality, SimulationClass};
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
pub use extension_stages::{AestraDebugViews, AestraFieldView, GpuStageTiming};
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
    /// The requested fidelity of stateful seeking this frame (hybrid roadmap M12): `Preview` bounds the
    /// per-frame reconstruction while the user scrubs; `Exact` (the default) reconstructs the
    /// authoritative state. Sourced from the player each frame.
    seek_quality: SeekQuality,
    history_epoch: u32,
    statistics_token: u32,
    checkpoint_context: Arc<trail_checkpoints::TrailContext>,
    trail_roots: Vec<(u32, u32)>,
    /// Persistent simulation-state sizing for stateful emitters (hybrid roadmap M6); `records == 0`
    /// for a fully analytic effect. The render world allocates its persistent state buffers from
    /// this (see [`StatefulStates`]).
    simulation_state: GpuSimulationState,
    /// One stateful dispatch descriptor per enabled stateful emitter (hybrid roadmap M6), in compiled
    /// emitter order. Empty for a fully analytic effect.
    stateful_dispatch: Vec<StatefulDispatch>,
    /// True when *every* enabled emitter is stateful, so the effect skips the analytic reset+simulate
    /// entirely. False for a mixed analytic+stateful effect, where the analytic path runs first (its
    /// `simulate` skips the stateful emitters' slots) and the stateful dispatches fill them after,
    /// reusing the shared counter/telemetry the analytic reset already wrote.
    stateful_only: bool,
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
    /// Per-particle launch speed range `(min, max)`.
    speed: (f32, f32),
    /// Per-particle lifetime range `(min, max)` in seconds.
    lifetime: (f32, f32),
    /// Base launch direction; the per-particle direction is `normalize(direction + spread * random)`.
    direction: [f32; 3],
    /// Cone spread factor mapped from the authored spread angle (`0` = straight along `direction`).
    spread: f32,
    /// Linear velocity damping per second (`v -= drag * v * dt`).
    drag: f32,
    /// Value-noise turbulence strength.
    turbulence: f32,
    /// Spawn shape: 0 = point, 1 = sphere (radius), 2 = box (half extents).
    shape_kind: u32,
    /// Sphere radius (when `shape_kind == 1`).
    shape_radius: f32,
    /// Box half extents (when `shape_kind == 2`).
    shape_half_extents: [f32; 3],
    /// Constant acceleration applied to velocity each tick.
    gravity: [f32; 3],
    /// The effect's 64-bit spawn seed.
    seed: u64,
    /// Collision primitives resolved after each tick (hybrid roadmap M10), capped at `MAX_COLLIDERS`
    /// when packed into the params buffer. Empty for emitters without a collision module.
    colliders: Vec<aestra_core::Collider>,
    /// The domain field these particles follow (fluid F2b); they then advance in lockstep with it.
    field_follow: Option<aestra_runtime::CompiledFieldFollow>,
}

impl StatefulDispatch {
    /// A hash of the emitter's seed, slot placement, and dynamics. A change means a different
    /// simulation, so the persistent state and its checkpoints are invalidated (hybrid roadmap M7).
    fn fingerprint(&self) -> u64 {
        let mut hash = self.seed;
        for bits in [
            self.capacity,
            self.slot_offset,
            self.emitter_index,
            self.spawn_rate.to_bits(),
            self.speed.0.to_bits(),
            self.speed.1.to_bits(),
            self.lifetime.0.to_bits(),
            self.lifetime.1.to_bits(),
            self.direction[0].to_bits(),
            self.direction[1].to_bits(),
            self.direction[2].to_bits(),
            self.spread.to_bits(),
            self.drag.to_bits(),
            self.turbulence.to_bits(),
            self.shape_kind,
            self.shape_radius.to_bits(),
            self.shape_half_extents[0].to_bits(),
            self.shape_half_extents[1].to_bits(),
            self.shape_half_extents[2].to_bits(),
            self.gravity[0].to_bits(),
            self.gravity[1].to_bits(),
            self.gravity[2].to_bits(),
            self.colliders.len() as u32,
        ] {
            hash = (hash ^ u64::from(bits)).wrapping_mul(0x0000_0100_0000_01b3);
        }
        // Following a field changes the simulation (fluid F2b).
        if let Some(follow) = &self.field_follow {
            for bits in [
                follow.stage as u32,
                follow.strength.to_bits(),
                follow.field.dims[0],
                follow.field.cell_size.to_bits(),
                follow.field.origin[1].to_bits(),
            ] {
                hash = (hash ^ u64::from(bits)).wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
        // Colliders change the simulation, so fold each one's shape and response into the fingerprint
        // (hybrid roadmap M10): editing a collider invalidates the persistent state and checkpoints.
        for collider in &self.colliders {
            let (kind, a, b) = collider_geometry(collider);
            for bits in [
                kind,
                a[0].to_bits(),
                a[1].to_bits(),
                a[2].to_bits(),
                b[0].to_bits(),
                b[1].to_bits(),
                b[2].to_bits(),
                collider.restitution.to_bits(),
                collider.friction.to_bits(),
                u32::from(collider.kill),
            ] {
                hash = (hash ^ u64::from(bits)).wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
        hash
    }
}

/// Decodes a collider into the `(kind, a, b)` param packing shared by the GPU kernel and CPU reference
/// (hybrid roadmap M10): `kind` 0 = plane (`a` = unit normal, `b.x` = distance), 1 = sphere (`a` =
/// center, `b.x` = radius), 2 = box (`a` = min, `b` = max).
fn collider_geometry(collider: &aestra_core::Collider) -> (u32, [f32; 3], [f32; 3]) {
    match collider.shape {
        aestra_core::ColliderShape::Plane { normal, distance } => (0, normal, [distance, 0.0, 0.0]),
        aestra_core::ColliderShape::Sphere { center, radius } => (1, center, [radius, 0.0, 0.0]),
        aestra_core::ColliderShape::Aabb { min, max } => (2, min, max),
    }
}

/// Packs the emitter's colliders into the params buffer's collider block (count word at index 26, then
/// up to `MAX_COLLIDERS` 10-word records from index 27), mirroring `aestra_gpu::STATEFUL_COLLISION_WGSL`
/// and the CPU reference's `resolve_colliders`.
fn pack_colliders(colliders: &[aestra_core::Collider], words: &mut [u32]) {
    let count = colliders.len().min(aestra_runtime::MAX_COLLIDERS);
    words[26] = count as u32;
    for (index, collider) in colliders.iter().take(count).enumerate() {
        let base = 27 + index * 10;
        let (kind, a, b) = collider_geometry(collider);
        words[base] = kind;
        words[base + 1] = a[0].to_bits();
        words[base + 2] = a[1].to_bits();
        words[base + 3] = a[2].to_bits();
        words[base + 4] = b[0].to_bits();
        words[base + 5] = b[1].to_bits();
        words[base + 6] = b[2].to_bits();
        words[base + 7] = collider.restitution.to_bits();
        words[base + 8] = collider.friction.to_bits();
        words[base + 9] = u32::from(collider.kill);
    }
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
    extension_stages::install(app);
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
        // The GPU stateful path (hybrid roadmap M6) drives every enabled stateful emitter, one dispatch
        // descriptor each (in compiled emitter order, which is 1:1 with the GpuEmitters). Dynamics come
        // from the compiled GpuEmitter as scalar midpoints of the authored ranges — the minimal
        // reference model. `stateful_only` records whether the whole effect is stateful (skip the
        // analytic path) or mixed (run analytic first, then fill the stateful emitters' slots).
        let compiled_emitters = &player.instance.effect().emitters;
        let enabled_emitters = compiled_emitters
            .iter()
            .filter(|emitter| emitter.enabled)
            .count();
        let emitter_count = artifact.emitters.len() as u32;
        let seed = player.instance.seed();
        // Collision input provider boundary (hybrid roadmap M11): this GPU backend supplies only
        // authored colliders (it resolves them on-GPU, needing no engine scene data). If the effect's
        // collision inputs need a source this backend cannot provide, refuse the stateful path
        // explicitly — a warning and no dispatches — rather than silently mis-simulating.
        let collision_inputs = player.instance.effect().collision_inputs();
        let collision_supported = match collision_inputs
            .resolve_against(&aestra_core::CollisionBackendCapabilities::authored_only())
        {
            Ok(()) => true,
            Err(error) => {
                warn!(
                    "skipping stateful simulation for effect {:?}: {error}",
                    player.instance.effect().name
                );
                false
            }
        };
        let stateful_dispatch: Vec<StatefulDispatch> =
            if artifact.simulation_state.records > 0 && collision_supported {
                compiled_emitters
                    .iter()
                    .enumerate()
                    .filter(|(_, compiled)| {
                        compiled.enabled && compiled.simulation_class != SimulationClass::Analytic
                    })
                    .filter_map(|(index, compiled)| {
                        artifact
                            .emitters
                            .get(index)
                            .map(|emitter| StatefulDispatch {
                                capacity: emitter.max_particles,
                                slot_offset: emitter.slot_offset,
                                emitter_index: index as u32,
                                emitter_count,
                                spawn_rate: 0.5 * (emitter.spawn_rate.x + emitter.spawn_rate.y),
                                speed: (emitter.speed.x, emitter.speed.y),
                                lifetime: (emitter.lifetime.x, emitter.lifetime.y),
                                direction: [
                                    emitter.direction.x,
                                    emitter.direction.y,
                                    emitter.direction.z,
                                ],
                                // Map the authored spread half-angle to the cone factor: 0 rad -> straight,
                                // ~90 deg -> factor 1, blending in more of the random unit vector.
                                spread: emitter.spread_radians / std::f32::consts::FRAC_PI_2,
                                drag: 0.5 * (emitter.drag.x + emitter.drag.y),
                                turbulence: 0.5 * (emitter.turbulence.x + emitter.turbulence.y),
                                // Map the analytic shape encoding to the stateful one (sphere/box/point).
                                shape_kind: match emitter.shape_kind {
                                    3 => 1, // Sphere
                                    5 => 2, // Box
                                    _ => 0, // Point (and shapes the stateful path does not model yet)
                                },
                                shape_radius: emitter.shape_radius,
                                shape_half_extents: [
                                    emitter.shape_radius,
                                    emitter.shape_depth,
                                    emitter.shape_extent_z,
                                ],
                                gravity: [emitter.gravity.x, emitter.gravity.y, emitter.gravity.z],
                                seed,
                                colliders: compiled.colliders.clone(),
                                field_follow: compiled.field_follow.clone(),
                            })
                    })
                    .collect()
            } else {
                Vec::new()
            };
        let stateful_only =
            !stateful_dispatch.is_empty() && stateful_dispatch.len() == enabled_emitters;
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
                seek_quality: player.seek_quality(),
                history_epoch: player.instance.history_epoch(),
                statistics_token: 0,
                checkpoint_context: default(),
                trail_roots,
                ribbon_workgroups,
                total_slots: artifact.total_slots,
                simulation_state: artifact.simulation_state,
                stateful_dispatch,
                stateful_only,
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
        gpu.seek_quality = player.seek_quality();
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

/// One GPU-resident snapshot of a stateful emitter's full persistent state at a fixed tick (hybrid
/// roadmap M7): copies of all four buffers plus the CPU-side spawn accumulator. A backward seek
/// restores the nearest snapshot at or before the target and replays forward the short remainder,
/// instead of replaying from tick 0.
struct StatefulCheckpoint {
    tick: u32,
    state: Buffer,
    free_list: Buffer,
    free_count: Buffer,
    spawn_counter: Buffer,
    spawn_accumulator: f32,
}

/// The persistent GPU state for one stateful effect (hybrid roadmap M6/M7). Unlike the analytic
/// particle buffer — recomputed from scratch every frame — this survives across frames so per-particle
/// state advances incrementally, and it carries a store of GPU-resident checkpoints for cheap backward
/// seek. Reallocated (which drops the checkpoints) when the emitter's capacity or dynamics fingerprint
/// changes.
struct StatefulPersistentState {
    /// Slot capacity these buffers were sized for; a change triggers reallocation.
    records: u32,
    /// `f32` components of persistent state per slot.
    stride: u32,
    /// Identity of the emitter's dynamics + seed; a change invalidates the state and its checkpoints.
    fingerprint: u64,
    /// `records * stride` persistent state floats (position, velocity, age, lifetime, ordinal bits).
    state: Buffer,
    /// Free-slot indices for death/reuse; initialised to every slot free.
    free_list: Buffer,
    /// Atomic count of free slots; initialised to `records`.
    free_count: Buffer,
    /// Atomic spawn ordinal counter; initialised to 0.
    spawn_counter: Buffer,
    /// The last fixed tick the persistent state was advanced to. A target below this is a backward seek.
    last_tick: u32,
    /// Fractional spawn carry, so a non-integer per-tick spawn rate emits the right long-run count.
    spawn_accumulator: f32,
    /// GPU-resident checkpoints, ascending by tick (hybrid roadmap M7).
    checkpoints: Vec<StatefulCheckpoint>,
}

/// Fixed tick cadence between checkpoints (~1/3 s at 60 Hz).
const STATEFUL_CHECKPOINT_CADENCE: u32 = 20;
/// Checkpoint count budget per emitter; exceeding it coarsens the store (drops every other), doubling
/// the effective cadence and keeping memory bounded with full-timeline coverage.
const MAX_STATEFUL_CHECKPOINTS: usize = 64;

impl StatefulPersistentState {
    /// Allocates and initialises one emitter's persistent buffers for `records` slots: zeroed state, a
    /// full free list (`0..records`), a free count of `records`, and a spawn counter of 0. All four
    /// buffers are copy source+dest so they can be snapshot to / restored from a checkpoint.
    fn allocate(render_device: &RenderDevice, records: u32, stride: u32, fingerprint: u64) -> Self {
        let (state, free_list, free_count, spawn_counter) =
            Self::fresh_buffers(render_device, records, stride);
        Self {
            records,
            stride,
            fingerprint,
            state,
            free_list,
            free_count,
            spawn_counter,
            last_tick: 0,
            spawn_accumulator: 0.0,
            checkpoints: Vec::new(),
        }
    }

    /// Creates the four persistent buffers initialised to tick 0.
    fn fresh_buffers(
        render_device: &RenderDevice,
        records: u32,
        stride: u32,
    ) -> (Buffer, Buffer, Buffer, Buffer) {
        const COPYABLE: BufferUsages = BufferUsages::STORAGE
            .union(BufferUsages::COPY_SRC)
            .union(BufferUsages::COPY_DST);
        let state_floats = records as usize * stride as usize;
        let state = render_device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some("aestra stateful state"),
            contents: &vec![0_u8; state_floats * std::mem::size_of::<f32>()],
            usage: COPYABLE,
        });
        let free_list_bytes: Vec<u8> = (0..records).flat_map(u32::to_le_bytes).collect();
        let free_list = render_device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some("aestra stateful free list"),
            contents: &free_list_bytes,
            usage: COPYABLE,
        });
        let free_count = render_device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some("aestra stateful free count"),
            contents: &records.to_le_bytes(),
            usage: COPYABLE,
        });
        let spawn_counter = render_device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some("aestra stateful spawn counter"),
            contents: &0_u32.to_le_bytes(),
            usage: COPYABLE,
        });
        (state, free_list, free_count, spawn_counter)
    }

    /// Re-initialises the four buffers to tick 0 (keeping the checkpoint store), for a backward seek
    /// before the earliest checkpoint.
    fn reset_to_zero(&mut self, render_device: &RenderDevice) {
        let (state, free_list, free_count, spawn_counter) =
            Self::fresh_buffers(render_device, self.records, self.stride);
        self.state = state;
        self.free_list = free_list;
        self.free_count = free_count;
        self.spawn_counter = spawn_counter;
        self.last_tick = 0;
        self.spawn_accumulator = 0.0;
    }

    /// Captures a GPU-resident checkpoint of the current state at `tick` (copying all four buffers
    /// GPU→GPU), unless one already exists at that tick. Coarsens the store when it exceeds the budget.
    fn capture(&mut self, render_device: &RenderDevice, encoder: &mut CommandEncoder, tick: u32) {
        if self
            .checkpoints
            .iter()
            .any(|checkpoint| checkpoint.tick == tick)
        {
            return;
        }
        let (state, free_list, free_count, spawn_counter) =
            Self::fresh_buffers(render_device, self.records, self.stride);
        encoder.copy_buffer_to_buffer(&self.state, 0, &state, 0, self.state.size());
        encoder.copy_buffer_to_buffer(&self.free_list, 0, &free_list, 0, self.free_list.size());
        encoder.copy_buffer_to_buffer(&self.free_count, 0, &free_count, 0, self.free_count.size());
        encoder.copy_buffer_to_buffer(
            &self.spawn_counter,
            0,
            &spawn_counter,
            0,
            self.spawn_counter.size(),
        );
        let checkpoint = StatefulCheckpoint {
            tick,
            state,
            free_list,
            free_count,
            spawn_counter,
            spawn_accumulator: self.spawn_accumulator,
        };
        // Insert keeping the store ascending by tick.
        let position = self
            .checkpoints
            .partition_point(|existing| existing.tick < tick);
        self.checkpoints.insert(position, checkpoint);
        if self.checkpoints.len() > MAX_STATEFUL_CHECKPOINTS {
            retain_every_other(&mut self.checkpoints);
        }
    }

    /// Restores the checkpoint captured exactly at `tick` (fluid F2b's joint seek), setting the last
    /// tick and spawn carry. False when this store has no checkpoint there.
    fn restore_at(&mut self, encoder: &mut CommandEncoder, tick: u32) -> bool {
        if !self
            .checkpoints
            .iter()
            .any(|checkpoint| checkpoint.tick == tick)
        {
            return false;
        }
        if let Some((at, accumulator)) = self.restore_nearest(encoder, tick) {
            self.last_tick = at;
            self.spawn_accumulator = accumulator;
        }
        true
    }

    /// Restores the nearest checkpoint at or before `target` (copying its four buffers back GPU→GPU),
    /// returning its `(tick, spawn_accumulator)`, or `None` when no checkpoint is at or before `target`.
    fn restore_nearest(&self, encoder: &mut CommandEncoder, target: u32) -> Option<(u32, f32)> {
        // The store is ascending by tick; the nearest at or before `target` is just before the first
        // one past it.
        let index = self
            .checkpoints
            .partition_point(|checkpoint| checkpoint.tick <= target)
            .checked_sub(1)?;
        let checkpoint = &self.checkpoints[index];
        encoder.copy_buffer_to_buffer(&checkpoint.state, 0, &self.state, 0, self.state.size());
        encoder.copy_buffer_to_buffer(
            &checkpoint.free_list,
            0,
            &self.free_list,
            0,
            self.free_list.size(),
        );
        encoder.copy_buffer_to_buffer(
            &checkpoint.free_count,
            0,
            &self.free_count,
            0,
            self.free_count.size(),
        );
        encoder.copy_buffer_to_buffer(
            &checkpoint.spawn_counter,
            0,
            &self.spawn_counter,
            0,
            self.spawn_counter.size(),
        );
        Some((checkpoint.tick, checkpoint.spawn_accumulator))
    }
}

/// Halves a store by retaining every other entry (index 0, 2, 4, …), doubling the effective cadence
/// while keeping the first and (for an odd length) last, so full-timeline coverage survives.
fn retain_every_other<T>(items: &mut Vec<T>) {
    let mut keep = false;
    items.retain(|_| {
        keep = !keep;
        keep
    });
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
        // Reallocate (dropping the checkpoints) when the emitter set, a capacity, or a dynamics
        // fingerprint changes — any of which means a different simulation.
        let matches = current.is_some_and(|states| {
            states.len() == effect.stateful_dispatch.len()
                && states
                    .iter()
                    .zip(&effect.stateful_dispatch)
                    .all(|(state, dispatch)| {
                        state.records == dispatch.capacity
                            && state.stride == stride
                            && state.fingerprint == dispatch.fingerprint()
                    })
        });
        if !matches {
            let allocated = effect
                .stateful_dispatch
                .iter()
                .map(|dispatch| {
                    StatefulPersistentState::allocate(
                        &render_device,
                        dispatch.capacity,
                        stride,
                        dispatch.fingerprint(),
                    )
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
/// Per-frame fixed-tick catch-up budget for an *exact* stateful seek: large, so a settled cursor
/// converges to the authoritative state in a few frames, but still bounded so one frame cannot stall
/// the GPU on a huge jump (the remainder continues on later frames).
const STATEFUL_MAX_CATCHUP_TICKS: u32 = 300;

/// Per-frame catch-up budget for a *preview* seek (hybrid roadmap M12): tight, so rapid scrubbing stays
/// responsive. The reconstruction is temporally bounded — the presented state is an *exact* earlier
/// tick when the budget cannot reach the target, never a values-approximate one — and a preview is
/// never authoritative, so an exact pass on cursor-release replays the remainder to the target.
const STATEFUL_PREVIEW_CATCHUP_TICKS: u32 = 24;

/// The per-frame catch-up budget for a stateful seek at the requested quality (hybrid roadmap M12).
fn stateful_catchup_budget(quality: SeekQuality) -> u32 {
    match quality {
        SeekQuality::Preview => STATEFUL_PREVIEW_CATCHUP_TICKS,
        SeekQuality::Exact => STATEFUL_MAX_CATCHUP_TICKS,
    }
}

/// The stateful params words for one dispatch (`aestra_gpu::STATEFUL_SIMULATION_PARAM_WORDS`):
/// `spawn_per_tick` varies across advance ticks and `subtick` is the presentation-interpolation time
/// `present` uses.
fn stateful_params_bytes(
    dispatch: &StatefulDispatch,
    spawn_per_tick: u32,
    subtick: f32,
) -> Vec<u8> {
    let mut words = vec![0u32; aestra_gpu::STATEFUL_SIMULATION_PARAM_WORDS];
    words[..26].copy_from_slice(&[
        dispatch.capacity,
        spawn_per_tick,
        dispatch.seed as u32,
        (dispatch.seed >> 32) as u32,
        dispatch.speed.0.to_bits(),
        dispatch.speed.1.to_bits(),
        dispatch.lifetime.0.to_bits(),
        dispatch.lifetime.1.to_bits(),
        STATEFUL_TICK_DT.to_bits(),
        dispatch.gravity[0].to_bits(),
        dispatch.gravity[1].to_bits(),
        dispatch.gravity[2].to_bits(),
        dispatch.direction[0].to_bits(),
        dispatch.direction[1].to_bits(),
        dispatch.direction[2].to_bits(),
        dispatch.spread.to_bits(),
        dispatch.drag.to_bits(),
        dispatch.emitter_index,
        dispatch.slot_offset,
        dispatch.turbulence.to_bits(),
        dispatch.shape_kind,
        dispatch.shape_radius.to_bits(),
        dispatch.shape_half_extents[0].to_bits(),
        dispatch.shape_half_extents[1].to_bits(),
        dispatch.shape_half_extents[2].to_bits(),
        subtick.to_bits(),
    ]);
    // Collider block (hybrid roadmap M10): a count word at 26, then up to MAX_COLLIDERS 10-word
    // records from 27 (see aestra_gpu::STATEFUL_COLLISION_WGSL).
    pack_colliders(&dispatch.colliders, &mut words);
    words.into_iter().flat_map(u32::to_le_bytes).collect()
}

/// The effect-wide render buffers a stateful emitter presents into.
struct StatefulRenderBuffers<'a> {
    particles: &'a Buffer,
    alive: &'a Buffer,
    indirect: &'a Buffer,
    counters: &'a Buffer,
}

/// A bind group over one emitter's persistent buffers, `params`, and the effect's render buffers.
fn stateful_bind_group(
    device: &RenderDevice,
    layout: &BindGroupLayout,
    persistent: &StatefulPersistentState,
    params: &Buffer,
    render: &StatefulRenderBuffers<'_>,
) -> BindGroup {
    device.create_bind_group(
        Some("aestra_gpu_stateful"),
        layout,
        &BindGroupEntries::sequential((
            persistent.state.as_entire_buffer_binding(),
            persistent.free_list.as_entire_buffer_binding(),
            persistent.free_count.as_entire_buffer_binding(),
            persistent.spawn_counter.as_entire_buffer_binding(),
            params.as_entire_buffer_binding(),
            render.particles.as_entire_buffer_binding(),
            render.alive.as_entire_buffer_binding(),
            render.indirect.as_entire_buffer_binding(),
            render.counters.as_entire_buffer_binding(),
        )),
    )
}

/// One tick's params (with this tick's spawn count, advancing the fractional spawn carry) and its bind
/// group.
fn stateful_tick_group(
    device: &RenderDevice,
    layout: &BindGroupLayout,
    persistent: &mut StatefulPersistentState,
    dispatch: &StatefulDispatch,
    render: &StatefulRenderBuffers<'_>,
) -> BindGroup {
    persistent.spawn_accumulator += dispatch.spawn_rate * STATEFUL_TICK_DT;
    let spawn_count = persistent.spawn_accumulator.floor();
    persistent.spawn_accumulator -= spawn_count;
    let spawn_count = (spawn_count as u32).min(dispatch.capacity);
    let params = device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("aestra stateful tick params"),
        contents: &stateful_params_bytes(dispatch, spawn_count, 0.0),
        usage: BufferUsages::STORAGE,
    });
    stateful_bind_group(device, layout, persistent, &params, render)
}

/// Resets one emitter's indirect instance count, then presents + compacts its live slots into the
/// effect-wide alive/indirect/counters buffers the render path draws. The vertex count (word 0 of the
/// draw command) is preserved; only the instance count (word 1) is zeroed so the compaction rebuilds
/// it. Presentation interpolation (hybrid roadmap M8) extrapolates by the sub-tick time — how far past
/// the last simulated tick the requested time is, bounded to one tick in case the fixed-tick advance
/// lags the presentation time.
#[allow(clippy::too_many_arguments)]
fn present_stateful_emitter(
    device: &RenderDevice,
    encoder: &mut CommandEncoder,
    present: &ComputePipeline,
    layout: &BindGroupLayout,
    persistent: &StatefulPersistentState,
    dispatch: &StatefulDispatch,
    render: &StatefulRenderBuffers<'_>,
    simulation_time: f32,
) {
    let instance_count_offset = u64::from(dispatch.emitter_index * 4 + 1) * 4;
    encoder.clear_buffer(render.indirect, instance_count_offset, Some(4));
    let subtick = (simulation_time - persistent.last_tick as f32 * STATEFUL_TICK_DT)
        .clamp(0.0, STATEFUL_TICK_DT);
    let params = device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("aestra stateful present params"),
        contents: &stateful_params_bytes(dispatch, 0, subtick),
        usage: BufferUsages::STORAGE,
    });
    let group = stateful_bind_group(device, layout, persistent, &params, render);
    let mut pass = encoder.begin_compute_pass(&ComputePassDescriptor {
        label: Some("aestra stateful present"),
        timestamp_writes: None,
    });
    pass.set_bind_group(0, &group, &[]);
    pass.set_pipeline(present);
    pass.dispatch_workgroups(dispatch.capacity.div_ceil(WORKGROUP_SIZE), 1, 1);
}

/// Encodes one stateful *emitter's* per-frame GPU work (hybrid roadmap M6/M7): advance its persistent
/// state from its last tick to the tick for `simulation_time` (death loop + spawn per tick), capturing
/// GPU-resident checkpoints at a fixed cadence, then present. A backward seek restores the nearest
/// checkpoint at or before the target and replays only the remainder forward (the derived
/// restart+replay seek mode; never a reverse integration). The caller clears the shared live counter
/// once before the emitter loop and stamps the statistics telemetry once after it. Every kernel here is
/// conformance-proven on real GPU.
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
    render: &StatefulRenderBuffers<'_>,
    simulation_time: f32,
    seek_quality: SeekQuality,
) {
    let target_tick = (simulation_time.max(0.0) / STATEFUL_TICK_DT) as u32;
    if target_tick < persistent.last_tick {
        match persistent.restore_nearest(encoder, target_tick) {
            Some((tick, accumulator)) => {
                persistent.last_tick = tick;
                persistent.spawn_accumulator = accumulator;
            }
            None => persistent.reset_to_zero(device),
        }
    }
    let workgroups = dispatch.capacity.div_ceil(WORKGROUP_SIZE);
    // Advance in cadence-aligned segments (one compute pass each), capturing a GPU-resident checkpoint
    // at each cadence boundary reached. Bounded per frame so a large jump cannot stall the GPU.
    let mut remaining =
        (target_tick - persistent.last_tick).min(stateful_catchup_budget(seek_quality));
    while remaining > 0 {
        let to_boundary =
            STATEFUL_CHECKPOINT_CADENCE - (persistent.last_tick % STATEFUL_CHECKPOINT_CADENCE);
        let segment = remaining.min(to_boundary);
        let groups: Vec<BindGroup> = (0..segment)
            .map(|_| stateful_tick_group(device, layout, persistent, dispatch, render))
            .collect();
        {
            let mut pass = encoder.begin_compute_pass(&ComputePassDescriptor {
                label: Some("aestra stateful advance"),
                timestamp_writes: None,
            });
            for group in &groups {
                pass.set_bind_group(0, group, &[]);
                pass.set_pipeline(death_integrate);
                pass.dispatch_workgroups(workgroups, 1, 1);
                pass.set_pipeline(spawn);
                pass.dispatch_workgroups(workgroups, 1, 1);
            }
        }
        persistent.last_tick += segment;
        remaining -= segment;
        if persistent
            .last_tick
            .is_multiple_of(STATEFUL_CHECKPOINT_CADENCE)
        {
            persistent.capture(device, encoder, persistent.last_tick);
        }
    }
    present_stateful_emitter(
        device,
        encoder,
        present,
        layout,
        persistent,
        dispatch,
        render,
        simulation_time,
    );
}

/// What couples an effect's stateful emitters to its domains (fluid F2b): the domains' timelines
/// (indexed like `CompiledEffect::all_extension_stages`), the host inputs they tick with, and the
/// Follow Field pipeline.
pub(super) struct Coupling<'a> {
    pub domains: &'a mut [Option<crate::execution::StageTimeline>],
    pub inputs: crate::execution::StageInputs<'a>,
    pub follower: &'a crate::execution::FieldFollowPipeline,
}

/// The latest tick at or before `target` that every store holds a checkpoint for.
fn joint_checkpoint_tick(
    persistent_states: &[StatefulPersistentState],
    domains: &[Option<crate::execution::StageTimeline>],
    target: u32,
) -> Option<u32> {
    let first = persistent_states.first()?;
    first
        .checkpoints
        .iter()
        .rev()
        .map(|checkpoint| checkpoint.tick)
        .filter(|tick| *tick <= target)
        .find(|tick| {
            persistent_states
                .iter()
                .all(|state| state.checkpoints.iter().any(|c| c.tick == *tick))
                && domains
                    .iter()
                    .flatten()
                    .all(|domain| domain.checkpoint_ticks().contains(tick))
        })
}

/// Advances an effect whose stateful emitters follow a domain's field (fluid F2b) — every store in
/// lockstep, tick by tick: each tick advances the domains one tick, then every stateful emitter one
/// tick (death loop + spawn), then pulls the following emitters toward their fields. All stores
/// checkpoint at the same cadence; a backward seek (or stores that fell out of step, e.g. a rebuilt
/// domain) restores every store at the latest tick they all hold — else resets them all to tick 0 —
/// and replays, so scrubbing reproduces the uninterrupted run. Then every emitter presents.
#[allow(clippy::too_many_arguments)]
fn run_coupled_stateful(
    device: &RenderDevice,
    encoder: &mut CommandEncoder,
    pipelines: (&ComputePipeline, &ComputePipeline, &ComputePipeline),
    layout: &BindGroupLayout,
    persistent_states: &mut [StatefulPersistentState],
    dispatches: &[StatefulDispatch],
    coupling: Coupling<'_>,
    render: &StatefulRenderBuffers<'_>,
    simulation_time: f32,
    seek_quality: SeekQuality,
) {
    let (death_integrate, spawn, present) = pipelines;
    let domains = coupling.domains;
    let target = (simulation_time.max(0.0) / STATEFUL_TICK_DT) as u32;
    let last = persistent_states.first().map_or(0, |state| state.last_tick);
    let in_step = persistent_states
        .iter()
        .all(|state| state.last_tick == last)
        && domains
            .iter()
            .flatten()
            .all(|domain| domain.last_tick() == last);
    if !in_step || target < last {
        match joint_checkpoint_tick(persistent_states, domains, target.min(last)) {
            Some(tick) => {
                for state in persistent_states.iter_mut() {
                    state.restore_at(encoder, tick);
                }
                for domain in domains.iter_mut().flatten() {
                    domain.restore_to(encoder, tick);
                }
            }
            None => {
                for state in persistent_states.iter_mut() {
                    state.reset_to_zero(device);
                }
                for domain in domains.iter_mut().flatten() {
                    domain.restore_to(encoder, 0);
                }
            }
        }
    }
    let now = persistent_states.first().map_or(0, |state| state.last_tick);
    let ticks = target
        .saturating_sub(now)
        .min(stateful_catchup_budget(seek_quality));
    for _ in 0..ticks {
        let next = persistent_states[0].last_tick + 1;
        for domain in domains.iter_mut().flatten() {
            if let Err(error) = domain.advance_to(
                device.wgpu_device(),
                encoder,
                next,
                1,
                coupling.inputs,
                None,
            ) {
                warn!("coupled domain stopped: {error}");
            }
        }
        for (dispatch, persistent) in dispatches.iter().zip(persistent_states.iter_mut()) {
            let group = stateful_tick_group(device, layout, persistent, dispatch, render);
            {
                let workgroups = dispatch.capacity.div_ceil(WORKGROUP_SIZE);
                let mut pass = encoder.begin_compute_pass(&ComputePassDescriptor {
                    label: Some("aestra coupled stateful tick"),
                    timestamp_writes: None,
                });
                pass.set_bind_group(0, &group, &[]);
                pass.set_pipeline(death_integrate);
                pass.dispatch_workgroups(workgroups, 1, 1);
                pass.set_pipeline(spawn);
                pass.dispatch_workgroups(workgroups, 1, 1);
            }
            if let Some(follow) = &dispatch.field_follow
                && let Some(Some(domain)) = domains.get(follow.stage)
                && let Some(field) = domain.executor().buffer(follow.field.resource.as_str())
            {
                coupling.follower.encode(
                    device.wgpu_device(),
                    encoder,
                    &persistent.state,
                    dispatch.capacity,
                    field,
                    follow,
                    STATEFUL_TICK_DT,
                );
            }
            persistent.last_tick += 1;
            if persistent
                .last_tick
                .is_multiple_of(STATEFUL_CHECKPOINT_CADENCE)
            {
                persistent.capture(device, encoder, persistent.last_tick);
            }
        }
    }
    for (dispatch, persistent) in dispatches.iter().zip(persistent_states.iter()) {
        present_stateful_emitter(
            device,
            encoder,
            present,
            layout,
            persistent,
            dispatch,
            render,
            simulation_time,
        );
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

/// Runs the per-emitter stateful dispatches for one effect. When `owns_shared_reset` is true (a fully
/// stateful effect, with no analytic reset), it clears the shared live counter before the emitter loop
/// and stamps the statistics telemetry after it; when false (a mixed effect), the analytic reset
/// already did both, so it only runs the emitters — their presents add to the counter the analytic
/// `simulate` already contributed to.
#[allow(clippy::too_many_arguments)]
fn run_stateful_dispatches(
    device: &RenderDevice,
    encoder: &mut CommandEncoder,
    pipelines: (&ComputePipeline, &ComputePipeline, &ComputePipeline),
    layout: &BindGroupLayout,
    persistent_states: &mut [StatefulPersistentState],
    dispatches: &[StatefulDispatch],
    render: &StatefulRenderBuffers<'_>,
    coupling: Option<Coupling<'_>>,
    simulation_time: f32,
    seek_quality: SeekQuality,
    statistics_token: u32,
    history_epoch: u32,
    owns_shared_reset: bool,
) {
    if owns_shared_reset {
        // Clear the shared live counter once, before any emitter's present bumps it.
        encoder.clear_buffer(render.counters, 0, Some(4));
    }
    // Emitters following a domain's field advance in lockstep with it (fluid F2b).
    let coupled = dispatches
        .iter()
        .any(|dispatch| dispatch.field_follow.is_some());
    match coupling.filter(|_| coupled) {
        Some(coupling) => run_coupled_stateful(
            device,
            encoder,
            pipelines,
            layout,
            persistent_states,
            dispatches,
            coupling,
            render,
            simulation_time,
            seek_quality,
        ),
        None => {
            for (dispatch, persistent) in dispatches.iter().zip(persistent_states.iter_mut()) {
                dispatch_stateful_effect(
                    device,
                    encoder,
                    pipelines.0,
                    pipelines.1,
                    pipelines.2,
                    layout,
                    persistent,
                    dispatch,
                    render,
                    simulation_time,
                    seek_quality,
                );
            }
        }
    }
    if owns_shared_reset && let Some(first) = dispatches.first() {
        stamp_stateful_statistics(
            device,
            encoder,
            render.indirect,
            first.emitter_count,
            statistics_token,
            history_epoch,
            simulation_time,
        );
    }
}

/// What the simulation system keeps across frames: trail histories, the timestamp timer, stateful
/// pipelines and states, and the domains coupled emitters follow (fluid F2b), with their pipeline.
type SimulationState<'w, 's> = (
    Local<'s, TrailHistories>,
    Local<'s, simulation_timing::SimulationTimer>,
    Option<Res<'w, StatefulSimulationPipeline>>,
    ResMut<'w, StatefulStates>,
    ResMut<'w, extension_stages::StageRuntimes>,
    Option<Res<'w, extension_stages::FieldFollow>>,
);

fn run_simulation(
    mut render_context: RenderContext,
    pipeline_cache: Res<PipelineCache>,
    pipeline: Option<Res<SimulationPipeline>>,
    effects: Query<(
        Entity,
        &bevy::render::sync_world::MainEntity,
        &GpuEffectBuffers,
        &GpuBindGroup,
        Option<&extension_stages::ExtractedStages>,
    )>,
    mesh_draws: Query<(&GpuDrawInstance, &render::PreparedMeshDraw)>,
    gpu_resources: (
        Res<RenderAssets<GpuShaderBuffer>>,
        Res<RenderDevice>,
        Res<bevy::render::renderer::RenderQueue>,
        Res<simulation_timing::TimingMailbox>,
    ),
    state: SimulationState,
) {
    let _span = tracing::info_span!("aestra::gpu::simulate").entered();
    let Some(pipeline) = pipeline else {
        return;
    };
    let (buffers, render_device, queue, timing_mailbox) = gpu_resources;
    let (
        mut histories,
        mut timer,
        stateful_pipeline,
        mut stateful_states,
        mut stage_runtimes,
        follower,
    ) = state;
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
    histories.retain(|entity, _| {
        effects
            .get(*entity)
            .is_ok_and(|(_, _, e, _, _)| e.has_trails)
    });
    let mut allocated: u64 = histories.values().map(|h| h.2.checkpoints.bytes()).sum();
    for (entity, main_entity, effect, bind_group, extracted_stages) in &effects {
        // A fully stateful effect (hybrid roadmap M6) skips the analytic reset+simulate entirely and
        // runs only its persistent path, which owns the shared counter reset and statistics telemetry.
        // A mixed effect falls through to the analytic path and runs its stateful emitters afterward
        // (at the end of this loop body), where the analytic reset has already prepared the buffers.
        if effect.stateful_only
            && let (Some((sp, death_integrate, spawn, present)), Some(persistent_states)) =
                (&stateful, stateful_states.0.get_mut(&entity))
        {
            let render_buffers = [
                &effect.particles,
                &effect.alive,
                &effect.indirect,
                &effect.counters,
            ]
            .map(|handle| buffers.get(handle).map(|buffer| &buffer.buffer));
            if let [Some(particles), Some(alive), Some(indirect), Some(counters)] = render_buffers
                && persistent_states.len() == effect.stateful_dispatch.len()
            {
                let layout = pipeline_cache.get_bind_group_layout(&sp.layout);
                run_stateful_dispatches(
                    &render_device,
                    render_context.command_encoder(),
                    (death_integrate, spawn, present),
                    &layout,
                    persistent_states,
                    &effect.stateful_dispatch,
                    &StatefulRenderBuffers {
                        particles,
                        alive,
                        indirect,
                        counters,
                    },
                    extension_stages::coupling(
                        &mut stage_runtimes,
                        entity,
                        extracted_stages,
                        follower.as_deref(),
                    ),
                    effect.simulation_time,
                    effect.seek_quality,
                    effect.statistics_token,
                    effect.history_epoch,
                    true,
                );
            }
        }
        if effect.stateful_only {
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
        // Mixed effect (some analytic, some stateful emitters): the analytic reset+simulate above ran
        // (its `simulate` skipped the stateful emitters' slots), so now fill those slots with the
        // stateful path. The analytic reset already cleared the shared counter and stamped telemetry,
        // so these dispatches must not repeat that (`owns_shared_reset = false`). Pure-stateful effects
        // took the branch at the top of the loop and never reach here; pure-analytic effects have an
        // empty dispatch list, so this is a no-op for them.
        if !effect.stateful_dispatch.is_empty()
            && let (Some((sp, death_integrate, spawn, present)), Some(persistent_states)) =
                (&stateful, stateful_states.0.get_mut(&entity))
        {
            let render_buffers = [
                &effect.particles,
                &effect.alive,
                &effect.indirect,
                &effect.counters,
            ]
            .map(|handle| buffers.get(handle).map(|buffer| &buffer.buffer));
            if let [Some(particles), Some(alive), Some(indirect), Some(counters)] = render_buffers
                && persistent_states.len() == effect.stateful_dispatch.len()
            {
                let layout = pipeline_cache.get_bind_group_layout(&sp.layout);
                run_stateful_dispatches(
                    &render_device,
                    render_context.command_encoder(),
                    (death_integrate, spawn, present),
                    &layout,
                    persistent_states,
                    &effect.stateful_dispatch,
                    &StatefulRenderBuffers {
                        particles,
                        alive,
                        indirect,
                        counters,
                    },
                    extension_stages::coupling(
                        &mut stage_runtimes,
                        entity,
                        extracted_stages,
                        follower.as_deref(),
                    ),
                    effect.simulation_time,
                    effect.seek_quality,
                    effect.statistics_token,
                    effect.history_epoch,
                    false,
                );
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

    #[test]
    fn preview_seek_is_bounded_per_frame_and_exact_converges_to_the_target() {
        // Hybrid roadmap M12: preview bounds per-frame reconstruction more tightly than exact, so
        // rapid scrubbing stays responsive; both budgets are finite, so no single frame can stall the
        // GPU on a huge jump; and iterating the bounded catch-up converges *exactly* to the target tick
        // (the state is a valid earlier tick until it arrives — never values-approximate).
        assert!(
            stateful_catchup_budget(SeekQuality::Preview)
                < stateful_catchup_budget(SeekQuality::Exact),
            "preview reconstructs fewer ticks per frame than exact"
        );

        let target = 1000_u32;
        let mut frames_by_quality = Vec::new();
        for quality in [SeekQuality::Preview, SeekQuality::Exact] {
            let budget = stateful_catchup_budget(quality);
            let mut last_tick = 0_u32;
            let mut frames = 0_u32;
            while last_tick < target {
                // The exact per-frame advance the dispatch computes: min(remaining, budget).
                let step = (target - last_tick).min(budget);
                assert!(step <= budget, "each frame advances at most the budget");
                last_tick += step;
                frames += 1;
                assert!(frames < 10_000, "the seek must terminate");
            }
            assert_eq!(
                last_tick, target,
                "the {quality:?} seek converges exactly to the target tick"
            );
            frames_by_quality.push(frames);
        }
        assert!(
            frames_by_quality[0] > frames_by_quality[1],
            "preview reaches a large target over more (cheaper) frames than exact"
        );

        // Preview is never authoritative; exact is (once it reaches the target).
        assert!(!SeekQuality::Preview.is_authoritative());
        assert!(SeekQuality::Exact.is_authoritative());
    }

    #[test]
    fn coarsening_a_checkpoint_store_halves_it_keeping_full_coverage() {
        // retain_every_other backs the checkpoint-budget coarsening: it keeps indices 0, 2, 4, …,
        // doubling the spacing while preserving the first and last entries (for an odd length).
        let mut ticks: Vec<u32> = (0..=12).map(|k| k * 20).collect(); // 13 entries: 0,20,…,240
        retain_every_other(&mut ticks);
        assert_eq!(ticks, vec![0, 40, 80, 120, 160, 200, 240]);
        // Idempotent shape: coarsening again keeps halving and still spans the timeline.
        retain_every_other(&mut ticks);
        assert_eq!(ticks, vec![0, 80, 160, 240]);

        let mut even = vec![1, 2, 3, 4];
        retain_every_other(&mut even);
        assert_eq!(even, vec![1, 3], "even length keeps the first of each pair");
    }

    #[test]
    fn a_dynamics_change_changes_the_fingerprint() {
        let base = StatefulDispatch {
            capacity: 128,
            slot_offset: 0,
            emitter_index: 0,
            emitter_count: 1,
            spawn_rate: 24.0,
            speed: (10.0, 14.0),
            lifetime: (1.0, 1.5),
            direction: [0.0, 1.0, 0.0],
            spread: 0.4,
            drag: 0.5,
            turbulence: 4.0,
            shape_kind: 1,
            shape_radius: 3.0,
            shape_half_extents: [0.0; 3],
            gravity: [0.0, -9.81, 0.0],
            seed: 42,
            colliders: Vec::new(),
            field_follow: None,
        };
        assert_eq!(
            base.fingerprint(),
            base.clone().fingerprint(),
            "stable for equal dynamics"
        );
        let mut changed = base.clone();
        changed.gravity[1] = -12.0;
        assert_ne!(
            base.fingerprint(),
            changed.fingerprint(),
            "gravity change invalidates"
        );
        let mut reseeded = base.clone();
        reseeded.seed = 43;
        assert_ne!(
            base.fingerprint(),
            reseeded.fingerprint(),
            "seed change invalidates"
        );
        // A collider change invalidates the persistent state and checkpoints (hybrid roadmap M10).
        let mut with_collider = base.clone();
        with_collider.colliders.push(aestra_core::Collider {
            shape: aestra_core::ColliderShape::Plane {
                normal: [0.0, 1.0, 0.0],
                distance: 0.0,
            },
            restitution: 0.5,
            friction: 0.2,
            kill: false,
        });
        assert_ne!(
            base.fingerprint(),
            with_collider.fingerprint(),
            "adding a collider invalidates"
        );
        let mut bouncier = with_collider.clone();
        bouncier.colliders[0].restitution = 0.9;
        assert_ne!(
            with_collider.fingerprint(),
            bouncier.fingerprint(),
            "changing collider response invalidates"
        );
    }

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
                    seek_quality: SeekQuality::Exact,
                    history_epoch: 0,
                    statistics_token: 0,
                    checkpoint_context: default(),
                    trail_roots: vec![],
                    simulation_state: default(),
                    stateful_dispatch: Vec::new(),
                    stateful_only: false,
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

/// Fluid F2b: stateful emitters following a domain's field advance in lockstep with it through the
/// production coupled loop ([`run_coupled_stateful`]), on a real GPU — and scrubbing back and forth
/// reproduces the uninterrupted run bit for bit.
#[cfg(test)]
mod coupled_tests {
    use super::*;
    use crate::execution::{FieldFollowPipeline, StageExecutor, StageInputs, StageTimeline};
    use aestra_extension::ExtensionRegistry;
    use bevy::render::render_resource::{PipelineLayoutDescriptor, RawComputePipelineDescriptor};
    use bevy::render::renderer::WgpuWrapper;

    const CAPACITY: u32 = 256;
    const STRIDE: u32 = 9;

    struct Scene {
        device: RenderDevice,
        queue: wgpu::Queue,
        layout: BindGroupLayout,
        pipelines: [ComputePipeline; 3],
        follower: FieldFollowPipeline,
        states: Vec<StatefulPersistentState>,
        dispatches: Vec<StatefulDispatch>,
        domains: Vec<Option<StageTimeline>>,
        render: [Buffer; 4],
    }

    fn scene(follow: bool) -> Option<Scene> {
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = wgpu::Backends::PRIMARY;
        let instance = wgpu::Instance::new(descriptor);
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: None,
        }))
        .ok()?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_limits: adapter.limits(),
            ..Default::default()
        }))
        .ok()?;
        let device = RenderDevice::new(WgpuWrapper::new(device));

        // The fluid domain, lowered by the real plugin and run on a StageTimeline.
        let mut registry = ExtensionRegistry::builtin();
        registry.install(&aestra_fluid::FluidExtension).unwrap();
        let effect = aestra_compiler::EffectCompiler::with_extensions(registry.clone())
            .compile(&aestra_fluid::smoke_effect(&registry))
            .unwrap();
        let block = &effect.extension_stages[0].block;
        let executor =
            StageExecutor::new(device.wgpu_device(), &queue, block, &registry.programs, 4).unwrap();
        let domains = vec![Some(StageTimeline::new(executor, Default::default(), 7))];
        let field = block
            .field(&aestra_core::ResourceTypeId::new(
                aestra_fluid::RESOURCE_VELOCITY,
            ))
            .unwrap()
            .clone();

        // The production stateful program and its explicit 9-binding layout.
        let layout = device.create_bind_group_layout(
            "coupled test stateful",
            &BindGroupLayoutEntries::sequential(
                ShaderStages::COMPUTE,
                (
                    storage_buffer::<Vec<f32>>(false),
                    storage_buffer::<Vec<u32>>(false),
                    storage_buffer::<Vec<u32>>(false),
                    storage_buffer::<Vec<u32>>(false),
                    storage_buffer_read_only::<Vec<u32>>(false),
                    storage_buffer::<Vec<GpuParticle>>(false),
                    storage_buffer::<Vec<u32>>(false),
                    storage_buffer::<Vec<u32>>(false),
                    storage_buffer::<Vec<u32>>(false),
                ),
            ),
        );
        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let module = device
            .wgpu_device()
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: None,
                source: wgpu::ShaderSource::Wgsl(aestra_gpu::stateful_simulation_wgsl().into()),
            });
        let pipeline = |entry: &str| {
            device.create_compute_pipeline(&RawComputePipelineDescriptor {
                label: None,
                layout: Some(&pipeline_layout),
                module: &module,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        let pipelines = [
            pipeline("death_integrate"),
            pipeline("spawn"),
            pipeline("present"),
        ];

        // Two emitters sharing the one domain: light smoke puffs and heavier embers.
        let dispatch = |index: u32, gravity: f32, strength: f32| StatefulDispatch {
            capacity: CAPACITY,
            slot_offset: index * CAPACITY,
            emitter_index: index,
            emitter_count: 2,
            spawn_rate: 90.0,
            speed: (0.0, 4.0),
            lifetime: (3.0, 4.0),
            direction: [0.0, 1.0, 0.0],
            spread: 0.6,
            drag: 0.0,
            turbulence: 0.0,
            shape_kind: 1,
            shape_radius: 8.0,
            shape_half_extents: [0.0; 3],
            gravity: [0.0, gravity, 0.0],
            seed: 42 + u64::from(index),
            colliders: Vec::new(),
            field_follow: follow.then(|| aestra_runtime::CompiledFieldFollow {
                stage: 0,
                field: field.clone(),
                strength,
            }),
        };
        let dispatches = vec![dispatch(0, 0.0, 8.0), dispatch(1, -20.0, 3.0)];
        let states = dispatches
            .iter()
            .map(|d| {
                StatefulPersistentState::allocate(&device, d.capacity, STRIDE, d.fingerprint())
            })
            .collect();
        let buffer = |bytes: u64| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: bytes,
                usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let render = [
            buffer(u64::from(2 * CAPACITY) * 48),
            buffer(u64::from(2 * CAPACITY) * 4),
            buffer(64),
            buffer(16),
        ];
        let follower = FieldFollowPipeline::new(device.wgpu_device());
        Some(Scene {
            device,
            queue,
            layout,
            pipelines,
            follower,
            states,
            dispatches,
            domains,
            render,
        })
    }

    impl Scene {
        /// One frame at `tick` (mid-tick, so the target is exactly that tick).
        fn frame(&mut self, tick: u32) {
            let time = (tick as f32 + 0.5) * STATEFUL_TICK_DT;
            let mut encoder = self.device.create_command_encoder(&Default::default());
            let host = aestra_gpu::GpuHostBindings { words: vec![0] };
            let [particles, alive, indirect, counters] = &self.render;
            run_coupled_stateful(
                &self.device,
                &mut encoder,
                (&self.pipelines[0], &self.pipelines[1], &self.pipelines[2]),
                &self.layout,
                &mut self.states,
                &self.dispatches,
                Coupling {
                    domains: &mut self.domains,
                    inputs: StageInputs {
                        host_bindings: Some(&host),
                        ..Default::default()
                    },
                    follower: &self.follower,
                },
                &StatefulRenderBuffers {
                    particles,
                    alive,
                    indirect,
                    counters,
                },
                time,
                SeekQuality::Exact,
            );
            self.queue.submit([encoder.finish()]);
        }

        /// Every emitter's persistent state and the domain's velocity, bit for bit.
        fn state(&self) -> Vec<Vec<u8>> {
            let mut buffers: Vec<&wgpu::Buffer> = self.states.iter().map(|s| &*s.state).collect();
            let domain = self.domains[0].as_ref().unwrap();
            buffers.push(
                domain
                    .executor()
                    .buffer(aestra_fluid::RESOURCE_VELOCITY)
                    .unwrap(),
            );
            buffers
                .into_iter()
                .map(|buffer| read_back(&self.device, &self.queue, buffer))
                .collect()
        }
    }

    fn read_back(device: &RenderDevice, queue: &wgpu::Queue, buffer: &wgpu::Buffer) -> Vec<u8> {
        let readback = device.wgpu_device().create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: buffer.size(),
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = device
            .wgpu_device()
            .create_command_encoder(&Default::default());
        encoder.copy_buffer_to_buffer(buffer, 0, &readback, 0, buffer.size());
        queue.submit([encoder.finish()]);
        let slice = readback.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        device
            .wgpu_device()
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(std::time::Duration::from_secs(60)),
            })
            .unwrap();
        let bytes = slice.get_mapped_range().to_vec();
        readback.unmap();
        bytes
    }

    fn require(scene: Option<Scene>) -> Option<Scene> {
        if scene.is_none() {
            assert!(
                std::env::var_os("AESTRA_REQUIRE_GPU_CONFORMANCE").is_none(),
                "AESTRA_REQUIRE_GPU_CONFORMANCE is set but no GPU is available"
            );
            eprintln!("skipping coupled conformance: no GPU");
        }
        scene
    }

    #[test]
    fn coupled_particles_scrub_back_and_forth_to_the_uninterrupted_state() {
        let Some(mut uninterrupted) = require(scene(true)) else {
            return;
        };
        uninterrupted.frame(90);
        let at_90 = uninterrupted.state();

        let mut scrubbed = scene(true).unwrap();
        scrubbed.frame(90);
        scrubbed.frame(35); // joint restore at tick 20, replay 15
        assert_eq!(scrubbed.states[0].last_tick, 35);
        assert_eq!(scrubbed.domains[0].as_ref().unwrap().last_tick(), 35);
        scrubbed.frame(7); // before the first checkpoint: everything resets to tick 0
        scrubbed.frame(90);
        assert_eq!(scrubbed.state(), at_90, "every store, bit for bit");

        // Following the field is what moved them: uncoupled particles end elsewhere.
        let mut uncoupled = scene(false).unwrap();
        uncoupled.frame(90);
        let uncoupled = uncoupled.state();
        assert_ne!(uncoupled[0], at_90[0], "the smoke puffs follow the plume");
        assert_ne!(uncoupled[1], at_90[1], "the embers too");
    }
}
