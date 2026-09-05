//! Bevy render-world adapter for engine-neutral Aestra GPU artifacts.

mod bounds;
mod mesh_inputs;
mod render;
mod ribbon_bounds;
mod wireframe;

use crate::{
    ActiveBackend, AestraRenderSettings, AestraRuntimeStatus, CompatibilityIssue,
    CompatibilityIssueCode, CompatibilityReport, EffectRenderMode, EffectRuntimeStatus,
    GpuCapabilities, GpuPresentationPrepared, PresentedEffect, TextureAssetCache,
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
    GpuBlend, WORKGROUP_SIZE, fold_seed, indirect_draw_commands, indirect_draw_offset,
};
use aestra_runtime::RendererPlanKind;
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
            BindGroup, BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries,
            BufferUsages, CachedComputePipelineId, ComputePassDescriptor,
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
    has_ribbons: bool,
    ribbon_workgroups: u32,
    total_slots: u32,
}

#[derive(Component, Clone)]
#[require(Transform, Visibility, VisibilityClass)]
#[component(on_add = visibility::add_visibility_class::<GpuDrawInstance>)]
struct GpuDrawInstance {
    mesh: Option<Handle<Mesh>>,
    wireframe_geometry: Option<Arc<wireframe::WireframeGeometry>>,
    renderers: Handle<ShaderBuffer>,
    particles: Handle<ShaderBuffer>,
    alive: Handle<ShaderBuffer>,
    indirect: Handle<ShaderBuffer>,
    render_globals: Handle<ShaderBuffer>,
    render_params: Handle<ShaderBuffer>,
    texture: Handle<Image>,
    fallback_texture: Handle<Image>,
    renderer_order: u32,
    emitter_index: u32,
    indirect_offset: u64,
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
    texture_cache: ResMut<'w, TextureAssetCache>,
    fallback_textures: Res<'w, GpuFallbackTextures>,
    shader_cache: ResMut<'w, MaterialShaderCache>,
}

impl MaterialPreparationParams<'_> {
    fn prepare(
        &mut self,
        binding: &MaterialRuntimeBinding,
        effect: &aestra_runtime::CompiledEffect,
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
}

pub(crate) fn install(app: &mut App) {
    install_shader_assets(app);
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
        .add_systems(ExtractSchedule, publish_gpu_capabilities)
        .add_systems(RenderStartup, init_pipeline)
        .add_systems(
            Render,
            prepare_bind_groups.in_set(RenderSystems::PrepareBindGroups),
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
}

fn install_shader_assets(app: &App) {
    let registry = app.world().resource::<EmbeddedAssetRegistry>();
    let shader_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../aestra-gpu/src/shaders");
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
        let mut artifact = match GpuEffectArtifact::from_instance(&player.instance) {
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
                    .map(|binding| material_resources.prepare(binding, player.effect()))
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
                    RendererPlanKind::Ribbon { .. } => {
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
                let (texture, fallback_texture) = texture_path.map_or_else(
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
                );
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
                        RendererPlanKind::Mesh { asset } => player
                            .effect()
                            .assets
                            .iter()
                            .find(|entry| entry.source == asset)
                            .map(|entry| {
                                material_resources
                                    .asset_server
                                    .load::<Mesh>(entry.path.clone())
                            }),
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
        let indirect_draw_commands = indirect_draw_commands(&artifact.emitters);
        let emitters = buffers.add(ShaderBuffer::from(artifact.emitters));
        let ribbon_renderers = artifact
            .renderers
            .iter()
            .enumerate()
            .filter_map(|(index, r)| (r.renderer_kind == 3).then_some(index as u32))
            .collect::<Vec<_>>();
        let has_ribbons = !ribbon_renderers.is_empty();
        let ribbon_workgroups = player
            .effect()
            .emitters
            .len()
            .div_ceil(WORKGROUP_SIZE as usize) as u32;
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
                has_ribbons,
                ribbon_workgroups,
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
                            GpuDrawInstance {
                                mesh,
                                wireframe_geometry: None,
                                renderers: renderers.clone(),
                                particles: particles.clone(),
                                alive: alive.clone(),
                                indirect: indirect.clone(),
                                render_globals: render_globals.clone(),
                                render_params,
                                texture,
                                fallback_texture,
                                renderer_order: renderer_index,
                                emitter_index,
                                indirect_offset: indirect_draw_offset(emitter_index),
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
                        match material_resources.prepare(binding, player.effect()) {
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
            let has_ribbons = dynamics.renderers.iter().any(|r| r.renderer_kind == 3);
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
        (
            bounds::sync_mesh_bounds,
            ribbon_bounds::sync_ribbon_bounds,
            sync_gpu_render_transforms,
            wireframe::prepare_wireframe_geometry,
        )
            .after(bevy::transform::TransformSystems::Propagate)
            .before(visibility::VisibilitySystems::CheckVisibility),
    );
}

// Rendering and culling must see the same frame's propagated effect transform.
fn sync_gpu_render_transforms(
    mut buffers: ResMut<Assets<ShaderBuffer>>,
    players: Query<(&PresentedEffect, &GlobalTransform, &GpuEffectBuffers)>,
) {
    for (player, transform, gpu) in &players {
        if let Some(mut buffer) = buffers.get_mut(&gpu.render_globals) {
            buffer.set_data(GpuRenderGlobals {
                world_from_effect: Mat4::from(transform.affine()),
                time: player.simulation_time(),
                seed: fold_seed(player.instance.seed()),
                _padding: Vec2::ZERO,
            });
        }
    }
}

fn prepare_semantic_material(
    binding: &MaterialRuntimeBinding,
    effect: &aestra_runtime::CompiledEffect,
    asset_server: &AssetServer,
    texture_cache: &mut TextureAssetCache,
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
            effect
                .assets
                .iter()
                .find(|candidate| candidate.source == asset)
                .map(|asset| texture_cache.load(asset_server, &asset.path))
                .unwrap_or_else(|| fallback_textures.missing.clone())
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
        shader: shader.clone(),
        entry_point: Some("simulate".into()),
        ..default()
    });
    let link_ribbons = pipeline_cache.queue_compute_pipeline(ComputePipelineDescriptor {
        label: Some("aestra link ribbons".into()),
        layout: vec![layout.clone()],
        shader,
        entry_point: Some("link_ribbons".into()),
        ..default()
    });
    commands.insert_resource(SimulationPipeline {
        layout,
        reset,
        simulate,
        link_ribbons,
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
    let _span = tracing::info_span!("aestra::gpu::bind_groups").entered();
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
    mesh_draws: Query<(&GpuDrawInstance, &render::PreparedMeshDraw)>,
    buffers: Res<RenderAssets<GpuShaderBuffer>>,
) {
    let _span = tracing::info_span!("aestra::gpu::simulate").entered();
    let link_ribbons = pipeline_cache.get_compute_pipeline(pipeline.link_ribbons);
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
    for (effect, bind_group) in &effects {
        if effect.has_ribbons && link_ribbons.is_none() {
            continue;
        }
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
        if effect.has_ribbons
            && let Some(link_ribbons) = link_ribbons
        {
            pass.set_pipeline(link_ribbons);
            pass.dispatch_workgroups(effect.ribbon_workgroups, 1, 1);
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
    use aestra_runtime::EffectInstance;
    use std::sync::Arc;

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
                    render_globals: handle.clone(),
                    workgroups: 1,
                    has_ribbons: true,
                    ribbon_workgroups: 1,
                    total_slots: 1,
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
            "../../../assets/effects/plasma_burst.aestra.ron"
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
