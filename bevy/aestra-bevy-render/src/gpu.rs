//! Bevy render-world adapter for engine-neutral Aestra GPU artifacts.

mod alpha_sort;
// Test-only wgpu-version boundary; playback never maps sort buffers.
#[cfg(test)]
fn alpha_sort_test_readback(buffer: &wgpu::Buffer) -> Vec<u8> {
    buffer.slice(..).get_mapped_range().to_vec()
}
mod bounds;
mod capability_publication;
mod catchup_pacing;
mod clone_extraction;
mod coupled_simulation;
mod draw_commands;
mod draw_instance;
mod draw_preparation;
mod draw_resources;
mod effect_inputs;
mod extension_stages;
mod extraction;
mod extraction_cleanup;
mod extraction_systems;
mod geometry_statistics;
#[path = "gpu/mapped_readback_019.rs"]
mod mapped_readback;
mod material_lighting;
mod mesh_inputs;
mod output_context;
mod paged_trails;
mod particle_light_inputs;
pub mod particle_light_readback;
mod particle_light_transport;
pub mod particle_lights;
mod particle_output_readback;
mod particle_outputs;
mod particle_statistics;
mod physics;
mod pipeline;
mod preparation_context;
mod preparation_timing;
mod render;
mod ribbon_bounds;
#[path = "gpu/scene_depth_019.rs"]
mod scene_depth;
mod shader_composition;
mod simulation_pipeline;
mod simulation_timing;
mod sprite_culling;
mod stage_inputs;
mod stage_output_delivery;
mod stage_runtimes;
mod stateful_simulation;
mod stateful_trails;
mod storage_buffers;
mod storage_encoding;
mod timestamp_transport;
mod trail_checkpoints;
mod trail_compaction;
mod trail_context;
mod trail_culling;
mod trail_replay;
mod view_phases;
mod volume;
mod wireframe;
mod world_sdf;

pub use catchup_pacing::AestraCatchupPacing;
use catchup_pacing::CatchupPacer;
#[cfg(test)]
use catchup_pacing::stateful_catchup_budget;
#[cfg(test)]
use draw_instance::gpu_draw_mesh_center;
use draw_instance::{GpuDrawInstance, GpuRenderMode, GpuSemanticMaterialBinding};
#[cfg(test)]
use effect_inputs::event_link_counter_base;
pub(crate) use effect_inputs::{GpuEffectBuffers, HostEventHistory};
use effect_inputs::{RouteWiring, StatefulAppearance, StatefulDispatch, TickSchedule};

use crate::{
    ActiveBackend, AestraRenderSettings, CompatibilityIssue, CompatibilityIssueCode,
    CompatibilityReport, EffectRenderMode, EffectRuntimeStatus, GpuCapabilities,
    GpuPresentationPrepared, PresentedEffect, ProjectAssetCache, TransparentOrderMode,
    capabilities::select_backend,
    material::{MaterialBindingError, MaterialRuntimeBinding},
};
#[cfg(test)]
use aestra_core::MaterialId;
use aestra_gpu::material::MaterialProgramFingerprint;
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
    GpuBlend, WORKGROUP_SIZE, fold_seed, indirect_draw_commands_with_statistics,
    indirect_draw_offset,
};
#[cfg(test)]
use aestra_runtime::{PlaybackHistoryPolicy, SeekQuality};
use aestra_runtime::{RendererPlanKind, SimulationClass};
use bevy::{
    asset::{RenderAssetUsages, io::embedded::EmbeddedAssetRegistry},
    camera::{
        primitives::Aabb,
        visibility::{self, RenderLayers},
    },
    ecs::schedule::IntoScheduleConfigs,
    prelude::*,
    render::{
        Render, RenderApp, RenderStartup, RenderSystems,
        diagnostic::RecordDiagnostics,
        gpu_readback::{Readback, ReadbackComplete},
        render_asset::RenderAssets,
        render_resource::{
            BindGroup, BindGroupEntries, BindGroupLayout, BindGroupLayoutDescriptor,
            BindGroupLayoutEntries, Buffer, BufferInitDescriptor, BufferUsages,
            CachedComputePipelineId, ComputePassDescriptor, ComputePipeline,
            ComputePipelineDescriptor, Extent3d, PipelineCache, ShaderStages, TextureDimension,
            TextureFormat,
            binding_types::{storage_buffer, storage_buffer_read_only},
        },
        renderer::{RenderContext, RenderDevice, RenderGraph, RenderGraphSystems},
        storage::{GpuShaderBuffer, ShaderBuffer},
    },
};
pub use extension_stages::{
    AestraDebugViews, AestraEffectOutputs, AestraFieldView, AestraOutputEvent, AestraWorldSdf,
    GpuStageProgress, GpuStageTiming,
};
pub use physics::{AestraPhysicsColliders, AestraPhysicsQuery, PhysicsPose};

pub use alpha_sort::{AlphaSortSnapshot, GpuAlphaSortStatistics};
pub use output_context::{EffectOutputContext, ParticleOutputContext};
use particle_output_readback::{GpuArrivalReadback, receive_homing_arrivals};
pub use particle_output_readback::{GpuEventLinkCounts, GpuEventLinkStatistics};
pub use particle_statistics::GpuParticleStatistics;
pub use preparation_timing::GpuPreparationTiming;
pub use simulation_timing::{GpuSimulationFrame, GpuSimulationTiming, GpuSimulationWork};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};

pub const WESL_SHADER_PATH: &str = simulation_pipeline::WESL_SHADER_PATH;
use coupled_simulation::*;
pub(crate) use mapped_readback::with_mapped_range;
pub use render::{WESL_MESH_WIREFRAME_SHADER_PATH, WESL_RENDER_SHADER_PATH};
use simulation_pipeline::{GpuBindGroup, SimulationPipeline, init_pipeline, prepare_bind_groups};
use stateful_simulation::*;
/// The unified stateful simulation module (hybrid roadmap M6): the death_integrate / spawn / present
/// compute pipelines are built from this. Composed from the proven `aestra_gpu` WGSL primitives.
pub const STATEFUL_SIMULATION_SHADER_PATH: &str =
    stateful_simulation::STATEFUL_SIMULATION_SHADER_PATH;

impl StatefulAppearance {
    fn of(emitter: &aestra_gpu::GpuEmitter) -> Self {
        Self {
            size: emitter.size,
            opacity: emitter.opacity,
            color: emitter.color,
            max_scale: emitter.max_scale,
        }
    }

    /// White, opaque and one unit across all life (a gradient without keys is white).
    #[cfg(test)]
    fn plain() -> Self {
        let mut keys = [Vec2::ZERO; aestra_gpu::MAX_CURVE_KEYS];
        keys[0] = Vec2::new(0.0, 1.0);
        let one = aestra_gpu::GpuCurve {
            keys,
            count: 1,
            ..Default::default()
        };
        Self {
            size: one,
            opacity: one,
            color: aestra_gpu::GpuGradient::default(),
            max_scale: 1.0,
        }
    }
}

/// A hash of the event links into and out of emitter `index` (host bindings HB9b).
fn event_signature(effect: &aestra_runtime::CompiledEffect, index: usize) -> u64 {
    effect
        .event_links
        .iter()
        .filter(|link| link.source == index || link.target == index)
        .fold(0xcbf2_9ce4_8422_2325_u64, |hash, link| {
            [
                link.source as u32,
                link.target as u32,
                aestra_runtime::event_trigger_bit(link.trigger),
                link.count,
                link.inherit.to_bits(),
                link.trigger
                    .distance_settings()
                    .map_or(0, |(spacing, _)| spacing.to_bits()),
                link.trigger.samples_per_tick(),
            ]
            .into_iter()
            .fold(hash, |hash, bits| {
                (hash ^ u64::from(bits)).wrapping_mul(0x0000_0100_0000_01b3)
            })
        })
}

/// The spawn placement of an emitter transform, with the rotation normalized for the trig-free kernel.
fn spawn_placement(transform: aestra_core::EmitterTransform) -> aestra_runtime::SpawnPlacement {
    let rotation = Quat::from_array(transform.rotation);
    let rotation = if rotation.length_squared() > 1e-12 {
        rotation.normalize()
    } else {
        Quat::IDENTITY
    };
    aestra_runtime::SpawnPlacement {
        translation: transform.translation,
        rotation: rotation.to_array(),
        scale: transform.scale,
    }
}

#[derive(Clone)]
struct MaterialShaderVariants {
    single_sampled: Handle<Shader>,
    multisampled: Handle<Shader>,
}

#[derive(Resource, Default)]
pub(crate) struct MaterialShaderCache(BTreeMap<MaterialProgramFingerprint, MaterialShaderVariants>);

#[derive(Component)]
pub(crate) struct GpuReadbackOwner(Entity);

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

pub(crate) fn install(app: &mut App) {
    let alpha_statistics = GpuAlphaSortStatistics::default();
    app.insert_resource(alpha_statistics.clone());
    install_shader_assets(app);
    particle_lights::install(app);
    extension_stages::install(app);
    volume::install(app);
    let timing_mailbox = simulation_timing::TimingMailbox::default();
    let preparation_mailboxes = preparation_timing::PreparationMailboxes::default();
    app.insert_resource(preparation_mailboxes.clone())
        .add_systems(PreUpdate, preparation_timing::receive);
    let geometry_mailbox = geometry_statistics::GeometryMailbox::default();
    app.insert_resource(geometry_mailbox.clone())
        .add_systems(PreUpdate, geometry_statistics::receive);
    app.insert_resource(timing_mailbox.clone())
        .add_systems(PreUpdate, simulation_timing::receive_timings);
    app.init_resource::<MaterialShaderCache>()
        .add_systems(Startup, init_fallback_textures)
        .add_systems(Update, update_gpu_inputs.after(prepare_gpu_effects));
    extraction::install(app);
    extraction::install_device_publication(app);
    install_visibility_updates(app);
    let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
        let capabilities = GpuCapabilities::unavailable("Bevy has no render sub-application");
        let requested = app.world().resource::<AestraRenderSettings>().presentation;
        let status = select_backend(requested, &capabilities);
        app.insert_resource(capabilities).insert_resource(status);
        return;
    };
    render_app
        .insert_resource(alpha_statistics)
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
                .in_set(draw_resources::SimulateEffects)
                .after(RenderGraphSystems::Begin)
                .before(RenderGraphSystems::Render),
        );
    render::install(render_app);
    trail_culling::install(render_app);
    trail_compaction::install(render_app);
    alpha_sort::install(render_app);
}

fn install_shader_assets(app: &App) {
    let registry = app.world().resource::<EmbeddedAssetRegistry>();
    registry.insert_asset(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/gpu/alpha_sort.wgsl"),
        Path::new("aestra_bevy_render/shaders/alpha_sort.wgsl"),
        include_str!("gpu/alpha_sort.wgsl").as_bytes(),
    );
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

#[allow(clippy::too_many_arguments)]
pub(crate) fn prepare_gpu_effects(
    mut commands: Commands,
    sampling: Option<Res<crate::sampling::SpriteSampling>>,
    trail_sampling: Option<Res<crate::sampling::TrailRasterSampling>>,
    light_settings: Res<particle_lights::AestraParticleLightSettings>,
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
            GpuEffectArtifact::dynamics_from_instance(&player.instance).and_then(|mut d| {
                let plan =
                    aestra_gpu::TrailScratchPlan::configure(&mut d.emitters, d.storage_records)?;
                if d.storage_records > capabilities.max_particles {
                    return Err(GpuArtifactError::TrailLimit);
                }
                if !plan.fits(
                    d.storage_records,
                    d.emitters.len() as u32,
                    capabilities.max_storage_buffer_binding_size,
                    capabilities.max_buffer_size,
                    capabilities.max_compute_workgroups_per_dimension,
                ) {
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
            aestra_gpu::particle_attributes::retain_particle_light_attributes(
                &mut artifact.emitters,
                player.effect(),
                light_settings.max_lights != 0,
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
        // Collision input provider boundary (hybrid roadmap M11): this GPU backend supplies authored
        // colliders (resolved on-GPU) and the host's world SDF (`AestraWorldSdf`, host bindings HB10)
        // for `World` colliders. If the effect's
        // collision inputs need a source this backend cannot provide, refuse the stateful path
        // explicitly — a warning and no dispatches — rather than silently mis-simulating.
        let collision_inputs = player.instance.effect().collision_inputs();
        let collision_supported = match collision_inputs.resolve_against(
            &aestra_core::CollisionBackendCapabilities::new([
                aestra_core::CollisionInputSource::AuthoredColliders,
                aestra_core::CollisionInputSource::SignedDistanceField,
                aestra_core::CollisionInputSource::EnginePhysicsQuery,
            ]),
        ) {
            Ok(()) => true,
            Err(error) => {
                warn!(
                    "skipping stateful simulation for effect {:?}: {error}",
                    player.instance.effect().name
                );
                false
            }
        };
        let mut stateful_dispatch: Vec<StatefulDispatch> =
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
                                // A sub-emitter spawns only from its event links (host bindings HB9b).
                                spawn_rate: if player.effect().is_event_target(index) {
                                    0.0
                                } else {
                                    0.5 * (emitter.spawn_rate.x + emitter.spawn_rate.y)
                                },
                                burst_count: if player.effect().is_event_target(index) {
                                    0
                                } else {
                                    emitter.burst_count
                                },
                                burst_tick: (emitter.start_time / STATEFUL_TICK_DT).ceil().max(1.0)
                                    as u32
                                    - 1,
                                speed: (emitter.speed.x, emitter.speed.y),
                                lifetime: (emitter.lifetime.x, emitter.lifetime.y),
                                direction: [
                                    emitter.direction.x,
                                    emitter.direction.y,
                                    emitter.direction.z,
                                ],
                                // Preserve the legacy spread factor; explicit modes use degrees.
                                velocity_distribution: emitter.velocity_distribution,
                                spread: if emitter.velocity_distribution == 0 {
                                    emitter.spread_radians / std::f32::consts::FRAC_PI_2
                                } else {
                                    emitter.spread_radians.to_degrees()
                                },
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
                                domain_spawn: compiled.domain_spawn.clone(),
                                homing: compiled.homing.clone(),
                                homing_target: None,
                                homing_tracker: aestra_runtime::HomingTracker::default(),
                                attachment: compiled.attachment.clone(),
                                arrival_word: None,
                                homing_world_target: None,
                                event_mask: player.effect().event_mask(index),
                                distance_emission: player.effect().distance_emission(index),
                                event_signature: event_signature(player.effect(), index),
                                overflow_word: None,
                                schedule: None,
                                world_from_effect: aestra_runtime::IDENTITY_AFFINE,
                                world_revision: 0,
                                cutoffs: aestra_runtime::EmissionCutoffs::NONE,
                                placement: spawn_placement(compiled.transform),
                                appearance: StatefulAppearance::of(emitter),
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
        let trail_plan = aestra_gpu::TrailScratchPlan::configure(
            &mut artifact.emitters,
            artifact.particles.len() as u32,
        )
        .expect("validated trail plan");
        let light_pools = particle_lights::Pools(
            artifact
                .emitters
                .iter()
                .map(|emitter| (emitter.slot_offset, emitter.max_particles))
                .collect(),
        );
        let sort_capacities: Vec<_> = artifact.emitters.iter().map(|e| e.max_particles).collect();
        let emitters = buffers.add(storage_buffers::new(artifact.emitters));
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
        let has_ribbons = !ribbon_renderers.is_empty();
        let ribbon_workgroups = player
            .effect()
            .emitters
            .len()
            .div_ceil(WORKGROUP_SIZE as usize) as u32;
        let trail_workgroups = player.effect().emitters.len() as u32;
        sampling
            .as_deref()
            .copied()
            .unwrap_or_default()
            .apply(&mut artifact.renderers);
        trail_sampling
            .as_deref()
            .copied()
            .unwrap_or_default()
            .apply(&mut artifact.renderers);
        let renderer_kinds: Vec<_> = artifact.renderers.iter().map(|r| r.renderer_kind).collect();
        let renderer_owners: Vec<_> = artifact.renderers.iter().map(|r| r.playback_mode).collect();
        let renderers = buffers.add(storage_buffers::new(artifact.renderers));
        // Full record count, including the trail-history storage region past
        // total_slots, so aux (indexed by slot) covers trail head/record slots.
        let particles = buffers.add(storage_buffers::new(artifact.particles));
        let alive = buffers.add(storage_buffers::new(vec![
            0_u32;
            artifact.total_slots as usize
        ]));
        let dead = buffers.add(storage_buffers::new(vec![
            0_u32;
            artifact.total_slots as usize
        ]));
        // Shared per-slot aux scratch (3 words/slot) for ribbon link + trail ring
        // state; a 1-word dummy when the effect draws no ribbons/trails.
        let aux = buffers.add(storage_buffers::new(vec![
            0_u32;
            if has_ribbons || has_trails {
                trail_plan.aux_words as usize
            } else {
                1
            }
        ]));
        // Words 0..2 count live particles, then the trails' statistics, then each homing emitter's
        // arrivals (host bindings HB9), cumulative: the host raises `impact` on each increase.
        let arrivals_base = 2 + if has_trails {
            6 * player.effect().emitters.len() as u32
        } else {
            0
        };
        let mut arrival_words = 0;
        for dispatch in stateful_dispatch
            .iter_mut()
            .filter(|dispatch| dispatch.homing.is_some())
        {
            dispatch.arrival_word = Some(arrivals_base + arrival_words);
            // The count, then the tick it was counted up to.
            arrival_words += 2;
        }
        // Then each event-reporting emitter's source overflow count (host bindings HB9b).
        for dispatch in stateful_dispatch
            .iter_mut()
            .filter(|dispatch| dispatch.event_mask != 0)
        {
            dispatch.overflow_word = Some(arrivals_base + arrival_words);
            arrival_words += 1;
        }
        // Captured child demand, list drops and destination accepts per link.
        arrival_words += 3 * player.effect().event_links.len() as u32;
        // Then each particle output route's ring of tick records (event system E3).
        let particle_outputs: Vec<(aestra_runtime::CompiledParticleOutput, u32)> = player
            .effect()
            .particle_outputs()
            .map(|(_, route)| {
                let ring = arrivals_base + arrival_words;
                arrival_words += aestra_gpu::PARTICLE_OUTPUT_RING_WORDS;
                (route.clone(), ring)
            })
            .collect();
        let counters = buffers.add(storage_buffers::new(vec![
            0_u32;
            (arrivals_base + arrival_words)
                as usize
        ]));
        let indirect = buffers.add(storage_buffers::indirect(indirect_draw_commands));
        let globals = buffers.add(storage_buffers::new(GpuGlobals {
            time: player.simulation_time(),
            total_slots: artifact.total_slots,
            seed: fold_seed(player.instance.seed()),
            emitter_count: player.effect().emitters.len() as u32,
            duration: player.effect().duration,
            continuous: u32::from(player.effect().playback_mode.is_continuous()),
            _padding: UVec2::ZERO,
            emission_end: player.instance.emission_cutoffs().emission_end(),
            kill_time: player.instance.emission_cutoffs().kill_time(),
            _cutoff_padding: Vec2::ZERO,
            world_from_effect: Mat4::IDENTITY,
        }));
        let render_globals = buffers.add(storage_buffers::new(GpuRenderGlobals {
            world_from_effect: Mat4::IDENTITY,
            time: player.simulation_time(),
            seed: fold_seed(player.instance.seed()),
            _padding: Vec2::ZERO,
        }));
        commands.entity(entity).insert((
            light_pools,
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
                history_policy: player.history_policy(),
                history_epoch: player.instance.history_epoch(),
                statistics_token: 0,
                checkpoint_context: default(),
                trail_roots,
                ribbon_workgroups,
                trail_workgroups,
                trail_plan,
                total_slots: artifact.total_slots,
                simulation_state: artifact.simulation_state,
                stateful_dispatch,
                stateful_only,
                event_links: player.effect().event_links.clone(),
                routed: !player.effect().event_routes.is_empty(),
                particle_outputs,
                output_suppress_through: aestra_runtime::trace_tick(
                    player.instance.history_epoch_start_time(),
                ),
                host_events: Arc::new(HostEventHistory::of(&player.instance)),
                physics: aestra_gpu::pack_physics_scene(&Default::default()).into(),
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
        if arrival_words > 0 {
            commands.entity(entity).insert(GpuEventLinkStatistics {
                dropped: vec![0; player.effect().event_links.len()],
                links: vec![GpuEventLinkCounts::default(); player.effect().event_links.len()],
                source_overflow: 0,
                readback_samples: 0,
            });
            commands.entity(entity).with_children(|parent| {
                parent
                    .spawn((
                        Readback::buffer(counters.clone()),
                        GpuArrivalReadback {
                            effect: entity,
                            seen: BTreeMap::new(),
                            particle_delivery: default(),
                        },
                    ))
                    .observe(receive_homing_arrivals);
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
                        let render_params = buffers.add(storage_buffers::new(GpuRenderParams {
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
                                sort_range: UVec2::new(
                                    alive_offset,
                                    sort_capacities[emitter_index as usize],
                                ),
                                renderer_kind: renderer_kinds[renderer_index as usize],
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
                                sampled_sprite_cull: None,
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
    sampling: Option<Res<crate::sampling::SpriteSampling>>,
    trail_sampling: Option<Res<crate::sampling::TrailRasterSampling>>,
    light_settings: Res<particle_lights::AestraParticleLightSettings>,
    mut buffers: ResMut<Assets<ShaderBuffer>>,
    mut material_resources: MaterialPreparationParams,
    mut players: PreparedGpuPlayers,
    mut draw_instances: PreparedDraws,
) {
    let _span = tracing::info_span!("aestra::gpu::artifact_update").entered();
    for (mut player, mut gpu, runtime, render_layers, children) in &mut players {
        player.refresh_automatic_material_bindings();
        // A transform-only edit (a gizmo drag) swaps the effect in place: re-place future spawns.
        // Attached emitters are placed from their binding instead (see `sync_gpu_render_transforms`).
        for index in 0..gpu.stateful_dispatch.len() {
            if gpu.stateful_dispatch[index].attachment.is_some() {
                continue;
            }
            let emitter = gpu.stateful_dispatch[index].emitter_index as usize;
            if let Some(emitter) = player.effect().emitters.get(emitter) {
                let placement = spawn_placement(emitter.transform);
                if gpu.stateful_dispatch[index].placement != placement {
                    gpu.stateful_dispatch[index].placement = placement;
                }
            }
        }
        // Only emitter and renderer inputs change per frame; use the dynamics
        // builder so we never reallocate the capacity-sized particle scratch buffer
        // here (its cost scales with capacity, not with what actually changed).
        if let Some(children) = children {
            let render_mode = gpu_render_mode(player.render_mode());
            for child in children.iter() {
                if let Ok((mut draw, mut draw_layers, mesh_bounds, ribbon_bounds)) =
                    draw_instances.get_mut(child)
                {
                    // Failed/unsupported dynamic preparation must never retain stale cull bounds.
                    draw.sampled_sprite_cull = None;
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
            gpu.trail_workgroups = dynamics.emitters.len() as u32;
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
                    if let Ok((mut draw, _, mesh_bounds, ribbon_bounds)) =
                        draw_instances.get_mut(child)
                    {
                        let minimum_pixels = sampling
                            .as_deref()
                            .copied()
                            .unwrap_or_default()
                            .normalized()
                            .minimum_pixels;
                        // Event/stateful/attached histories cannot be bounded by the analytic
                        // spawn envelope. Vertex displacement likewise keeps the safe fallback.
                        if dynamics.simulation_state.records == 0
                            && minimum_pixels > 0.0
                            && draw.renderer_kind == 0
                            && draw.blend == GpuBlend::Additive
                            && !draw
                                .semantic_material
                                .as_ref()
                                .is_some_and(|binding| binding.program.has_vertex_offset)
                        {
                            draw.sampled_sprite_cull = dynamics
                                .mesh_bounds
                                .get(draw.emitter_index as usize)
                                .map(|motion| sprite_culling::Bounds {
                                    half_extents: motion.position_half_extents,
                                    maximum_size: motion.maximum_size
                                        * motion
                                            .linear_from_local
                                            .x_axis
                                            .length()
                                            .max(motion.linear_from_local.y_axis.length())
                                            .max(motion.linear_from_local.z_axis.length()),
                                    world_from_effect: Mat4::IDENTITY,
                                    minimum_pixels,
                                });
                        }
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
                aestra_gpu::particle_attributes::retain_particle_light_attributes(
                    &mut dynamics.emitters,
                    player.effect(),
                    light_settings.max_lights != 0,
                );
            }
            let _upload = tracing::info_span!("aestra::gpu::buffer_upload").entered();
            sampling
                .as_deref()
                .copied()
                .unwrap_or_default()
                .apply(&mut dynamics.renderers);
            trail_sampling
                .as_deref()
                .copied()
                .unwrap_or_default()
                .apply(&mut dynamics.renderers);
            if let Some(mut buffer) = buffers.get_mut(&gpu.emitters) {
                storage_buffers::update(&mut buffer, dynamics.emitters);
            }
            if let Some(mut buffer) = buffers.get_mut(&gpu.renderers) {
                storage_buffers::update(&mut buffer, dynamics.renderers);
            }
        }
    }
}

fn install_visibility_updates(app: &mut App) {
    // `sync_gpu_render_transforms` raises homing target events (host bindings HB9).
    app.add_message::<AestraOutputEvent>();
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
    sampling: Option<Res<crate::sampling::SpriteSampling>>,
    players: Query<(&PresentedEffect, &GpuEffectBuffers)>,
    mut draws: Query<(
        Entity,
        &ChildOf,
        &mut GpuDrawInstance,
        Has<visibility::NoFrustumCulling>,
        Has<HostMotionReplayCulling>,
        Has<ribbon_bounds::RibbonBoundsSource>,
    )>,
) {
    let minimum_pixels = sampling
        .as_deref()
        .copied()
        .unwrap_or_default()
        .normalized()
        .minimum_pixels;
    for (entity, parent, mut draw, uncullable, was_forced, ribbon) in &mut draws {
        // A view-dependent pixel floor can expand a sprite beyond world-space AABBs.
        // Bypass unpadded main-world frustum culling; safe analytic draws receive a
        // padded per-view queue check instead. Unknown bounds still use GPU clip rejection.
        let sampled_sprite =
            minimum_pixels > 0.0 && draw.renderer_kind == 0 && draw.blend == GpuBlend::Additive;
        let replaying_host = players.get(parent.parent()).is_ok_and(|(player, gpu)| {
            gpu.has_trails && !player.instance.host_transform_context().is_identity()
        });
        if replaying_host {
            draw.sampled_sprite_cull = None;
        }
        let forced = sampled_sprite || replaying_host;
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

/// What [`sync_gpu_render_transforms`] reads and updates per effect.
type RenderTransformSync = (
    Entity,
    &'static PresentedEffect,
    &'static GlobalTransform,
    &'static mut GpuEffectBuffers,
    &'static mut GpuParticleStatistics,
    Option<&'static AestraPhysicsColliders>,
);

// Rendering and culling must see the same frame's propagated effect transform.
fn sync_gpu_render_transforms(
    mut buffers: ResMut<Assets<ShaderBuffer>>,
    mut players: Query<RenderTransformSync>,
    mut events: MessageWriter<AestraOutputEvent>,
    world_sdf: Option<Res<AestraWorldSdf>>,
) {
    for (entity, player, transform, mut gpu, mut statistics, physics) in &mut players {
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
        gpu.history_policy = player.history_policy();
        // `World` and `Physics` colliders (host bindings HB10): where the effect sits in the host's
        // world, which world (a new one restarts their history), and this frame's physics colliders.
        let world_revision = world_sdf.as_deref().map_or(0, AestraWorldSdf::revision);
        let mut reads_physics = false;
        // The host's input events (event system E2–E3), rebuilt only when they change.
        if gpu.host_events.events != HostEventHistory::keys(&player.instance) {
            gpu.host_events = Arc::new(HostEventHistory::of(&player.instance));
        }
        let cutoffs = player.instance.emission_cutoffs();
        for dispatch in &mut gpu.stateful_dispatch {
            dispatch.cutoffs = cutoffs;
            let (mut world_colliders, mut physics_colliders) = (false, false);
            for collider in &dispatch.colliders {
                match collider.shape {
                    aestra_core::ColliderShape::World { .. } => world_colliders = true,
                    aestra_core::ColliderShape::Physics { .. } => physics_colliders = true,
                    _ => {}
                }
            }
            if world_colliders || physics_colliders {
                dispatch.world_from_effect = std::array::from_fn(|row| {
                    let r = world.row(row);
                    [r.x, r.y, r.z, r.w]
                });
            }
            if world_colliders {
                dispatch.world_revision = world_revision;
            }
            reads_physics |= physics_colliders;
        }
        if reads_physics {
            gpu.physics = match physics {
                Some(physics) => aestra_gpu::pack_physics_scene(&physics.0).into(),
                None => aestra_gpu::pack_physics_scene(&Default::default()).into(),
            };
        }
        // Homing targets (host bindings HB7) and attached emitters' placements (HB7b): this frame's,
        // from the bindings, into effect space. An attachment without a pose keeps its last placement.
        if gpu
            .stateful_dispatch
            .iter()
            .any(|dispatch| dispatch.homing.is_some() || dispatch.attachment.is_some())
        {
            let effect_from_world = world.inverse();
            let rows: [[f32; 4]; 3] = std::array::from_fn(|row| {
                let r = effect_from_world.row(row);
                [r.x, r.y, r.z, r.w]
            });
            let effect = player.instance.effect();
            let emitters = &effect.emitters;
            // Under a binding trace (host bindings HB8), each tick's own recorded input — computed
            // once per trace and placement, so replays after a seek steer and place exactly as the
            // uninterrupted run did.
            let trace = player.instance.binding_trace();
            let key = trace.map(|trace| {
                rows.iter().flatten().fold(trace.identity(), |hash, value| {
                    (hash ^ u64::from(value.to_bits())).wrapping_mul(0x0000_0100_0000_01b3)
                })
            });
            for dispatch in &mut gpu.stateful_dispatch {
                match (trace, key) {
                    (Some(trace), Some(key))
                        if dispatch.schedule.as_ref().map(|schedule| schedule.key) != Some(key) =>
                    {
                        let ticks = trace.len() as u64;
                        let authored = emitters
                            .get(dispatch.emitter_index as usize)
                            .map(|emitter| emitter.transform)
                            .unwrap_or_default();
                        dispatch.schedule = Some(Arc::new(TickSchedule {
                            key,
                            homing: dispatch
                                .homing
                                .as_ref()
                                .map(|homing| homing.schedule(effect, trace, ticks, rows))
                                .unwrap_or_default(),
                            placement: dispatch
                                .attachment
                                .as_ref()
                                .map(|attachment| {
                                    attachment
                                        .schedule(effect, trace, ticks, rows, authored)
                                        .into_iter()
                                        .map(spawn_placement)
                                        .collect()
                                })
                                .unwrap_or_default(),
                        }));
                    }
                    (None, _) => dispatch.schedule = None,
                    _ => {}
                }
                if let Some(homing) = &dispatch.homing {
                    let input = homing.resolve(&player.instance, rows);
                    dispatch.homing_target =
                        dispatch.homing_tracker.resolve(homing.config.lost, input);
                    let in_world = |target: aestra_runtime::HomingTarget| {
                        world
                            .transform_point3(Vec3::from_array(target.position))
                            .to_array()
                    };
                    dispatch.homing_world_target = dispatch.homing_target.map(in_world);
                    // The target appearing or vanishing (host bindings HB9), where it was.
                    if let Some(change) = dispatch.homing_tracker.take_change() {
                        let (kind, target) = match change {
                            aestra_runtime::TargetChange::Lost(target) => {
                                (aestra_runtime::EVENT_TARGET_LOST, target)
                            }
                            aestra_runtime::TargetChange::Acquired(target) => {
                                (aestra_runtime::EVENT_TARGET_ACQUIRED, target)
                            }
                        };
                        events.write(AestraOutputEvent::root(
                            entity,
                            aestra_runtime::EffectOutputEvent::new(
                                kind,
                                aestra_runtime::EventOrigin::Emitter(
                                    dispatch.emitter_index as usize,
                                ),
                                "homing",
                                in_world(target).to_vec(),
                                0.0,
                                aestra_runtime::trace_tick(player.instance.time()),
                            ),
                        ));
                    }
                }
                if let Some(attachment) = &dispatch.attachment
                    && dispatch.schedule.is_none()
                    && let Some(emitter) = emitters.get(dispatch.emitter_index as usize)
                    && let Some(transform) =
                        attachment.resolve(&player.instance, rows, emitter.transform)
                {
                    dispatch.placement = spawn_placement(transform);
                }
            }
        }
        gpu.history_epoch = player.instance.history_epoch();
        gpu.output_suppress_through =
            aestra_runtime::trace_tick(player.instance.history_epoch_start_time());
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
            if let Some(data) = buffers.get(&gpu.emitters).and_then(storage_buffers::bytes)
                && (gpu.checkpoint_context.key != key
                    || gpu.checkpoint_context.emitters != data
                    || gpu.checkpoint_context.motion != motion)
            {
                gpu.checkpoint_context = Arc::new(trail_context::TrailContext {
                    emitters: data.to_vec(),
                    key,
                    motion,
                });
            }
        }
        if let Some(mut buffer) = buffers.get_mut(&gpu.globals) {
            storage_buffers::update(
                &mut buffer,
                GpuGlobals {
                    time: player.simulation_time(),
                    total_slots: gpu.total_slots,
                    seed: fold_seed(player.instance.seed()),
                    emitter_count: player.effect().emitters.len() as u32,
                    duration: player.effect().duration,
                    continuous: u32::from(player.effect().playback_mode.is_continuous()),
                    _padding: UVec2::new(player.instance.history_epoch(), statistics_token),
                    emission_end: player.instance.emission_cutoffs().emission_end(),
                    kill_time: player.instance.emission_cutoffs().kill_time(),
                    _cutoff_padding: Vec2::ZERO,
                    world_from_effect: world,
                },
            );
        }
        if let Some(mut buffer) = buffers.get_mut(&gpu.render_globals) {
            storage_buffers::update(
                &mut buffer,
                GpuRenderGlobals {
                    world_from_effect: world,
                    time: player.simulation_time(),
                    seed: fold_seed(player.instance.seed()),
                    _padding: Vec2::ZERO,
                },
            );
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
                material_lighting::compose(&program.shader.wgsl, program.requires_scene_lighting()),
                format!(
                    "generated://aestra/material/{}.wgsl",
                    program.program_fingerprint
                ),
            ));
            let multisampled = if program.requires_scene_depth() {
                shaders.add(Shader::from_wgsl(
                    material_lighting::compose(
                        &program.multisampled_shader.wgsl,
                        program.requires_scene_lighting(),
                    ),
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

type TrailHistories = BTreeMap<
    Entity,
    (
        AssetId<ShaderBuffer>,
        Arc<trail_context::TrailContext>,
        trail_checkpoints::TrailHistory,
    ),
>;

/// What the simulation system keeps across frames: trail histories, the timestamp timer, stateful
/// pipelines and states, and the domains coupled emitters follow (fluid F2b), with their pipeline.
type SimulationState<'w, 's> = (
    Local<'s, TrailHistories>,
    Local<'s, stateful_trails::Histories>,
    Local<'s, simulation_timing::SimulationTimer>,
    Option<Res<'w, StatefulSimulationPipeline>>,
    ResMut<'w, StatefulStates>,
    ResMut<'w, extension_stages::StageRuntimes>,
    Option<Res<'w, extension_stages::FieldFollow>>,
    Option<ResMut<'w, CatchupPacer>>,
    Option<Res<'w, extension_stages::ParticleWorldBuffer>>,
);

type SimulationGpuResources<'w> = (
    Res<'w, RenderAssets<GpuShaderBuffer>>,
    Res<'w, RenderDevice>,
    Res<'w, bevy::render::renderer::RenderQueue>,
    Res<'w, simulation_timing::TimingMailbox>,
    Res<'w, AestraRenderSettings>,
);

#[allow(clippy::too_many_arguments)]
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
    gpu_resources: SimulationGpuResources,
    state: SimulationState,
    mut light_readiness: ResMut<particle_lights::PresentedOwners>,
) {
    light_readiness.owners.clear();
    let _span = tracing::info_span!("aestra::gpu::simulate").entered();
    let Some(pipeline) = pipeline else {
        return;
    };
    let (buffers, render_device, queue, timing_mailbox, render_settings) = gpu_resources;
    let (
        mut histories,
        mut stateful_histories,
        mut timer,
        stateful_pipeline,
        mut stateful_states,
        mut stage_runtimes,
        follower,
        mut pacer,
        particle_world,
    ) = state;
    let Some(particle_world) = particle_world else {
        return;
    };
    // Resolve the stateful compute pipelines once (present only when the device supports the path and
    // the pipelines have finished compiling). The stateful branch below drives one enabled stateful
    // emitter end-to-end; other effects take the analytic path unchanged.
    let stateful = stateful_pipeline.as_ref().and_then(|sp| {
        let order_present = match render_settings.transparent_order {
            TransparentOrderMode::Fast | TransparentOrderMode::DepthBackToFront => None,
            TransparentOrderMode::StableCapture => {
                Some(pipeline_cache.get_compute_pipeline(sp.order_present)?)
            }
        };
        Some((
            sp,
            pipeline_cache.get_compute_pipeline(sp.death_integrate)?,
            pipeline_cache.get_compute_pipeline(sp.spawn)?,
            pipeline_cache.get_compute_pipeline(sp.present)?,
            order_present,
        ))
    });
    let link_ribbons = pipeline_cache.get_compute_pipeline(pipeline.link_ribbons);
    let update_trails = pipeline_cache.get_compute_pipeline(pipeline.update_trails);
    let paged_pipelines: Option<[&ComputePipeline; 9]> = pipeline
        .paged_trails
        .map(|id| pipeline_cache.get_compute_pipeline(id))
        .into_iter()
        .collect::<Option<Vec<_>>>()
        .and_then(|pipelines| pipelines.try_into().ok());
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
            .is_ok_and(|(_, _, e, _, _)| e.has_trails && e.stateful_dispatch.is_empty())
    });
    stateful_histories.retain(|entity, _| {
        effects
            .get(*entity)
            .is_ok_and(|(_, _, e, _, _)| e.has_trails && !e.stateful_dispatch.is_empty())
    });
    let mut allocated: u64 = histories
        .values()
        .map(|h| h.2.checkpoints.bytes())
        .sum::<u64>()
        + stateful_histories
            .values()
            .map(|h| h.1.checkpoints.bytes())
            .sum::<u64>();
    for (entity, main_entity, effect, bind_group, extracted_stages) in &effects {
        let mut fully_presented = effect.stateful_dispatch.is_empty();
        let paged = if effect.trail_plan.paged() {
            let Some(pipelines) = paged_pipelines else {
                continue;
            };
            Some(paged_trails::Dispatch::new(
                &render_device,
                effect.trail_plan,
                pipelines,
                effect.trail_workgroups,
            ))
        } else {
            None
        };
        // Histories shared with stateful emitters advance in the lockstep path:
        // reset/present analytic heads, present stateful heads, then record trails
        // after each tick's event births. Other mixed effects retain the old path.
        let stateful_trails = effect.has_trails && !effect.stateful_dispatch.is_empty();
        if (effect.stateful_only || stateful_trails)
            && let (
                Some((sp, death_integrate, spawn, present, order_present)),
                Some(persistent_states),
            ) = (&stateful, stateful_states.0.get_mut(&entity))
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
                let mut observer = if stateful_trails {
                    let Some(update) = update_trails else {
                        continue;
                    };
                    if effect.has_ribbons && link_ribbons.is_none() {
                        continue;
                    }
                    let (Some(globals), Some(render_globals)) = (
                        buffers.get(&effect.globals),
                        buffers.get(&effect.render_globals),
                    ) else {
                        continue;
                    };
                    let (Some(dead), Some(aux)) =
                        (buffers.get(&effect.dead), buffers.get(&effect.aux))
                    else {
                        continue;
                    };
                    let history = stateful_histories
                        .entry(entity)
                        .or_insert_with(|| (effect.particles.id(), default()));
                    allocated -= history.1.checkpoints.bytes();
                    if history.0 != effect.particles.id() {
                        *history = (effect.particles.id(), default());
                    }
                    history.1.sync(effect, persistent_states);
                    Some(stateful_trails::Observer {
                        history: &mut history.1,
                        effect,
                        group: &bind_group.0,
                        reset,
                        simulate,
                        update,
                        paged: paged.as_ref(),
                        ribbons: link_ribbons,
                        globals: &globals.buffer,
                        render_globals: &render_globals.buffer,
                        buffers: [
                            particles,
                            alive,
                            &dead.buffer,
                            counters,
                            indirect,
                            &aux.buffer,
                        ],
                        memory_budget: trail_checkpoints::MEMORY_LIMIT.saturating_sub(allocated),
                        diagnostics,
                        observations: 0,
                    })
                } else {
                    None
                };
                let layout = pipeline_cache.get_bind_group_layout(&sp.layout);
                let physics_buffer = physics_scene_buffer(&render_device, &effect.physics);
                let timing_index = timing_batch.as_mut().and_then(|batch| {
                    batch.instance(
                        main_entity.id(),
                        effect.statistics_token,
                        effect.simulation_time,
                    )
                });
                if let Some((batch, index)) = timing_batch.as_ref().zip(timing_index) {
                    drop(render_context.command_encoder().begin_compute_pass(
                        &ComputePassDescriptor {
                            label: Some("aestra stateful timing begin"),
                            timestamp_writes: batch.writes(index, true, false),
                        },
                    ));
                }
                let stateful_work = run_stateful_dispatches(
                    &render_device,
                    render_context.command_encoder(),
                    (death_integrate, spawn, present, *order_present),
                    &layout,
                    persistent_states,
                    &effect.stateful_dispatch,
                    &effect.event_links,
                    effect.routed.then_some(RouteWiring {
                        bursts: &effect.host_events.bursts,
                        outputs: &effect.particle_outputs,
                        output_epoch: effect.history_epoch,
                        output_suppress_through: effect.output_suppress_through,
                    }),
                    &StatefulRenderBuffers {
                        particles,
                        alive,
                        indirect,
                        counters,
                        world: &particle_world.buffer,
                        physics: &physics_buffer,
                    },
                    extension_stages::coupling(
                        &mut stage_runtimes,
                        entity,
                        extracted_stages,
                        follower.as_deref(),
                    )
                    .or_else(|| {
                        (!effect.event_links.is_empty() || effect.routed || stateful_trails)
                            .then(|| extension_stages::link_coupling(follower.as_deref()))
                            .flatten()
                    }),
                    effect.simulation_time,
                    effect.seek_quality,
                    effect.statistics_token,
                    effect.history_epoch,
                    true,
                    pacer.as_deref_mut(),
                    observer
                        .as_mut()
                        .map(|observer| observer as &mut dyn CoupledHistory),
                );
                if stateful_work.is_some() && light_readiness.enabled {
                    light_readiness.owners.insert(entity);
                }
                if let Some((batch, index)) = timing_batch.as_mut().zip(timing_index) {
                    let trail_observations = observer.as_ref().map_or(0, |o| o.observations);
                    batch.work(
                        index,
                        GpuSimulationWork {
                            fixed_ticks: stateful_work.map(|work| work.0),
                            checkpoint_capture_bytes: stateful_work.map(|work| work.1),
                            trail_observations,
                            trail_workgroups: u64::from(trail_observations)
                                * (u64::from(effect.trail_workgroups)
                                    + paged.as_ref().map_or(0, |p| p.workgroups())),
                        },
                    );
                }
                if let Some(observer) = observer {
                    allocated += observer.history.checkpoints.bytes();
                }
                if let Some((batch, index)) = timing_batch.as_ref().zip(timing_index) {
                    drop(render_context.command_encoder().begin_compute_pass(
                        &ComputePassDescriptor {
                            label: Some("aestra stateful timing end"),
                            timestamp_writes: batch.writes(index, false, true),
                        },
                    ));
                }
            }
        }
        if effect.stateful_only || stateful_trails {
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
            if !effect.history_policy.captures_checkpoints() {
                allocated -= history.2.checkpoints.bytes();
                history.2.checkpoints = default();
            }
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
        // The legacy mixed analytic/stateful timer excludes the later stateful
        // dispatches. Do not advertise its partial window as a complete frame.
        if effect.stateful_dispatch.is_empty()
            && let Some((batch, index)) = timing_batch.as_mut().zip(timing_index)
        {
            let trail_observations = if effect.has_trails {
                observation_count as u32
            } else {
                0
            };
            batch.work(
                index,
                GpuSimulationWork {
                    fixed_ticks: None,
                    checkpoint_capture_bytes: None,
                    trail_observations,
                    trail_workgroups: u64::from(trail_observations)
                        * (u64::from(effect.trail_workgroups)
                            + paged.as_ref().map_or(0, |p| p.workgroups())),
                },
            );
        }
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
            let particle_span = effect.has_trails.then(|| {
                diagnostics.time_span(
                    render_context.command_encoder(),
                    "aestra::gpu::trail_particles",
                )
            });
            simulation_pipeline::record_particles(
                render_context.command_encoder(),
                bind_group,
                effect,
                reset,
                simulate,
                link_ribbons,
                timing_batch
                    .as_ref()
                    .zip(timing_index)
                    .and_then(|(batch, index)| {
                        batch.writes(
                            index,
                            observation == 0,
                            observation + 1 == observation_count && !effect.has_trails,
                        )
                    }),
            );
            if let Some(span) = particle_span {
                span.end(render_context.command_encoder());
            }
            if effect.has_trails
                && let Some(update_trails) = update_trails
                && !simulation_pipeline::record_trails(
                    render_context.command_encoder(),
                    bind_group,
                    effect,
                    simulation_pipeline::TrailPipelines {
                        update: update_trails,
                        link_ribbons,
                        paged: paged.as_ref(),
                    },
                    &buffers,
                    simulation_pipeline::TrailTimestamps {
                        history: timing_batch.as_ref().zip(timing_index).and_then(
                            |(batch, index)| {
                                batch.writes(
                                    index,
                                    false,
                                    observation + 1 == observation_count && paged.is_none(),
                                )
                            },
                        ),
                        paged_end: timing_batch.as_ref().zip(timing_index).and_then(
                            |(batch, index)| {
                                batch.writes(index, false, observation + 1 == observation_count)
                            },
                        ),
                    },
                    diagnostics,
                )
            {
                continue;
            }
            if let Some((_, times, _, state)) = &replay {
                let history = &mut histories.get_mut(&entity).unwrap().2;
                let time = times[observation];
                if effect.history_policy.captures_checkpoints()
                    && history.replay.should_capture(time)
                {
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
            && let (
                Some((sp, death_integrate, spawn, present, order_present)),
                Some(persistent_states),
            ) = (&stateful, stateful_states.0.get_mut(&entity))
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
                let physics_buffer = physics_scene_buffer(&render_device, &effect.physics);
                let work = run_stateful_dispatches(
                    &render_device,
                    render_context.command_encoder(),
                    (death_integrate, spawn, present, *order_present),
                    &layout,
                    persistent_states,
                    &effect.stateful_dispatch,
                    &effect.event_links,
                    effect.routed.then_some(RouteWiring {
                        bursts: &effect.host_events.bursts,
                        outputs: &effect.particle_outputs,
                        output_epoch: effect.history_epoch,
                        output_suppress_through: effect.output_suppress_through,
                    }),
                    &StatefulRenderBuffers {
                        particles,
                        alive,
                        indirect,
                        counters,
                        world: &particle_world.buffer,
                        physics: &physics_buffer,
                    },
                    extension_stages::coupling(
                        &mut stage_runtimes,
                        entity,
                        extracted_stages,
                        follower.as_deref(),
                    )
                    .or_else(|| {
                        (!effect.event_links.is_empty() || effect.routed)
                            .then(|| extension_stages::link_coupling(follower.as_deref()))
                            .flatten()
                    }),
                    effect.simulation_time,
                    effect.seek_quality,
                    effect.statistics_token,
                    effect.history_epoch,
                    false,
                    pacer.as_deref_mut(),
                    None,
                );
                fully_presented = work.is_some();
            }
        }
        if fully_presented && light_readiness.enabled {
            light_readiness.owners.insert(entity);
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
    fn sprite_sampling_culling_override_is_opt_in_and_reversible() {
        let mut app = App::new();
        app.init_resource::<crate::sampling::SpriteSampling>()
            .add_systems(Update, sync_host_motion_replay_culling);
        let parent = app.world_mut().spawn_empty().id();
        let mut draws = Vec::new();
        for (kind, blend) in [
            (0, GpuBlend::Additive),
            (0, GpuBlend::Alpha),
            (1, GpuBlend::Additive),
            (2, GpuBlend::Additive),
            (3, GpuBlend::Additive),
            (4, GpuBlend::Additive),
        ] {
            draws.push(
                app.world_mut()
                    .spawn((
                        ChildOf(parent),
                        GpuDrawInstance {
                            sort_range: UVec2::ZERO,
                            renderer_kind: kind,
                            owner: parent,
                            mesh: None,
                            wireframe_geometry: None,
                            renderers: default(),
                            particles: default(),
                            alive: default(),
                            aux: default(),
                            indirect: default(),
                            render_globals: default(),
                            render_params: default(),
                            texture: default(),
                            fallback_texture: default(),
                            renderer_order: 0,
                            emitter_index: 0,
                            indirect_offset: 0,
                            blend,
                            material: aestra_core::MaterialId::new(),
                            semantic_material: None,
                            render_mode: GpuRenderMode::Rendered,
                            mesh_center: Vec3::ZERO,
                            sampled_sprite_cull: None,
                            trail_owners: 0,
                            trail_instances: None,
                        },
                    ))
                    .id(),
            );
        }
        for minimum in [0.0, 2.0, 0.0, f32::NAN] {
            app.world_mut()
                .resource_mut::<crate::sampling::SpriteSampling>()
                .minimum_pixels = minimum;
            app.update();
            for (index, entity) in draws.iter().enumerate() {
                assert_eq!(
                    app.world()
                        .entity(*entity)
                        .contains::<visibility::NoFrustumCulling>(),
                    index == 0 && minimum == 2.0
                );
            }
        }
    }

    #[test]
    fn event_link_statistics_record_admission_and_separate_both_drop_causes() {
        let mut statistics = GpuEventLinkStatistics {
            dropped: vec![0],
            links: vec![GpuEventLinkCounts::default()],
            ..default()
        };
        statistics.record_link(0, 800, 0, 800);
        statistics.record_link(0, 800, 100, 600);
        assert_eq!(
            statistics.links[0],
            GpuEventLinkCounts {
                captured_demand: 1600,
                expansion_omitted: 100,
                accepted: 1400,
                destination_rejected: 100,
            }
        );
        assert_eq!(statistics.dropped, [200]);
        statistics.record_link(0, 0, 0, 0);
        assert_eq!(statistics.dropped, [200]);
    }

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
            burst_count: 0,
            burst_tick: 0,
            speed: (10.0, 14.0),
            lifetime: (1.0, 1.5),
            direction: [0.0, 1.0, 0.0],
            spread: 0.4,
            velocity_distribution: aestra_core::VelocityDistribution::LegacyCone as u32,
            drag: 0.5,
            turbulence: 4.0,
            shape_kind: 1,
            shape_radius: 3.0,
            shape_half_extents: [0.0; 3],
            gravity: [0.0, -9.81, 0.0],
            seed: 42,
            colliders: Vec::new(),
            field_follow: None,
            domain_spawn: None,
            homing: None,
            homing_target: None,
            homing_tracker: aestra_runtime::HomingTracker::default(),
            attachment: None,
            arrival_word: None,
            homing_world_target: None,
            event_mask: 0,
            distance_emission: None,
            event_signature: 0,
            overflow_word: None,
            schedule: None,
            world_from_effect: aestra_runtime::IDENTITY_AFFINE,
            world_revision: 0,
            cutoffs: aestra_runtime::EmissionCutoffs::NONE,
            placement: aestra_runtime::SpawnPlacement::IDENTITY,
            appearance: StatefulAppearance::plain(),
        };
        assert_eq!(
            base.fingerprint(),
            base.clone().fingerprint(),
            "stable for equal dynamics"
        );
        let mut changed = base.clone();
        changed.burst_count = 1;
        assert_ne!(
            base.fingerprint(),
            changed.fingerprint(),
            "burst edits invalidate history"
        );
        let mut changed = base.clone();
        changed.burst_tick = 30;
        assert_ne!(
            base.fingerprint(),
            changed.fingerprint(),
            "burst timing edits invalidate history"
        );
        let mut changed = base.clone();
        changed.velocity_distribution = aestra_core::VelocityDistribution::Ring as u32;
        assert_ne!(
            base.fingerprint(),
            changed.fingerprint(),
            "distribution change invalidates history"
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
                    trail_workgroups: 1,
                    trail_plan: default(),
                    total_slots: 1,
                    simulation_time: 0.0,
                    seek_quality: SeekQuality::Exact,
                    history_policy: PlaybackHistoryPolicy::default(),
                    history_epoch: 0,
                    statistics_token: 0,
                    checkpoint_context: default(),
                    trail_roots: vec![],
                    simulation_state: default(),
                    stateful_dispatch: Vec::new(),
                    stateful_only: false,
                    event_links: Vec::new(),
                    routed: false,
                    particle_outputs: Vec::new(),
                    output_suppress_through: 0,
                    host_events: Default::default(),
                    physics: aestra_gpu::pack_physics_scene(&Default::default()).into(),
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
            let bytes = storage_buffers::bytes(buffers.get(&handle).unwrap()).unwrap();
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
            let buffer = app
                .world()
                .resource::<Assets<ShaderBuffer>>()
                .get(&handle)
                .unwrap();
            let bytes = storage_buffers::bytes(buffer).unwrap();
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
    use crate::execution::{
        DomainSpawnPipeline, FieldFollowPipeline, StageExecutor, StageInputs, StageTimeline,
    };
    use aestra_extension::ExtensionRegistry;
    use bevy::render::render_resource::{PipelineLayoutDescriptor, RawComputePipelineDescriptor};
    use bevy::render::renderer::WgpuWrapper;

    const CAPACITY: u32 = 256;
    const STRIDE: u32 = aestra_gpu::STATEFUL_STATE_STRIDE;
    const TICKS_PER_SUBMISSION: u32 = 8;

    include!("gpu/stateful_trails_tests.rs");

    struct Scene {
        device: RenderDevice,
        queue: wgpu::Queue,
        layout: BindGroupLayout,
        pipelines: [ComputePipeline; 4],
        follower: FieldFollowPipeline,
        spawner: DomainSpawnPipeline,
        gatherer: crate::execution::EventGatherPipeline,
        /// A world SDF saying no world is supplied (host bindings HB10).
        no_world: Buffer,
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
        // Hosted Windows runners can expose WARP even when no hardware GPU is available.
        // These long replay checks belong on the native-GPU job, not a software adapter.
        if adapter.get_info().device_type == wgpu::DeviceType::Cpu {
            return None;
        }
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_limits: adapter.limits(),
            ..Default::default()
        }))
        .ok()?;
        let device = RenderDevice::new(WgpuWrapper::new(device));

        // The fluid domain, lowered by the real plugin and run on a StageTimeline.
        let mut registry = ExtensionRegistry::builtin();
        registry.install(&aestra_fluid::FluidExtension).unwrap();
        let mut source = aestra_fluid::smoke_effect(&registry);
        let grid = source.simulation_stages[0]
            .modules
            .iter_mut()
            .find(|module| module.module_type.0 == aestra_fluid::MODULE_GRID)
            .unwrap();
        let aestra_core::ModuleParameters::Custom(values) = &mut grid.parameters else {
            panic!("fluid grid parameters should be custom");
        };
        // Keep the same physical domain, with fewer cells: this checks replay/coupling,
        // not solver accuracy, and does not need the showcase's full resolution.
        values.insert("resolution".into(), aestra_core::Value::U32(16));
        values.insert("cell_size".into(), aestra_core::Value::Scalar(6.0));
        let effect = aestra_compiler::EffectCompiler::with_extensions(registry.clone())
            .compile(&source)
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

        // The production stateful program and its explicit 10-binding layout.
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
                    storage_buffer::<Vec<u32>>(false),
                    storage_buffer_read_only::<Vec<u32>>(false),
                    storage_buffer_read_only::<Vec<u32>>(false),
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
            pipeline("order_present"),
        ];

        // Two emitters sharing the one domain: light smoke puffs and heavier embers.
        let dispatch = |index: u32, gravity: f32, strength: f32| StatefulDispatch {
            capacity: CAPACITY,
            slot_offset: index * CAPACITY,
            emitter_index: index,
            emitter_count: 2,
            spawn_rate: 90.0,
            burst_count: 0,
            burst_tick: 0,
            speed: (0.0, 4.0),
            lifetime: (3.0, 4.0),
            direction: [0.0, 1.0, 0.0],
            spread: 0.6,
            velocity_distribution: aestra_core::VelocityDistribution::LegacyCone as u32,
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
            domain_spawn: None,
            homing: None,
            homing_target: None,
            homing_tracker: aestra_runtime::HomingTracker::default(),
            attachment: None,
            arrival_word: None,
            homing_world_target: None,
            event_mask: 0,
            distance_emission: None,
            event_signature: 0,
            overflow_word: None,
            schedule: None,
            world_from_effect: aestra_runtime::IDENTITY_AFFINE,
            world_revision: 0,
            cutoffs: aestra_runtime::EmissionCutoffs::NONE,
            placement: aestra_runtime::SpawnPlacement::IDENTITY,
            appearance: StatefulAppearance::plain(),
        };
        let dispatches = vec![dispatch(0, 0.0, 8.0), dispatch(1, -20.0, 3.0)];
        let states = dispatches
            .iter()
            .map(|d| {
                StatefulPersistentState::allocate(
                    &device,
                    d.capacity,
                    STRIDE,
                    d.fingerprint(),
                    false,
                )
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
        let spawner = DomainSpawnPipeline::new(device.wgpu_device());
        let gatherer = crate::execution::EventGatherPipeline::new(device.wgpu_device());
        let no_world = device.create_buffer_with_data(&BufferInitDescriptor {
            label: None,
            contents: &aestra_gpu::GpuWorldSdf::absent().to_bytes(),
            usage: BufferUsages::STORAGE,
        });
        Some(Scene {
            no_world,
            device,
            queue,
            layout,
            pipelines,
            follower,
            spawner,
            gatherer,
            states,
            dispatches,
            domains,
            render,
        })
    }

    fn chained_event_scene() -> Option<(Scene, [aestra_runtime::CompiledEventLink; 2])> {
        let mut scene = scene(false)?;
        scene.domains.clear();
        let template = scene.dispatches[0].clone();
        let rockets = StatefulDispatch {
            capacity: 64,
            slot_offset: 0,
            emitter_index: 0,
            spawn_rate: 3.0,
            speed: (38.0, 46.0),
            lifetime: (1.1, 1.5),
            spread: 0.1,
            velocity_distribution: aestra_core::VelocityDistribution::LegacyCone as u32,
            gravity: [0.0, -25.0, 0.0],
            shape_kind: 0,
            event_mask: 2,
            ..template.clone()
        };
        let burst = StatefulDispatch {
            capacity: 4096,
            slot_offset: 64,
            emitter_index: 1,
            spawn_rate: 0.0,
            speed: (10.0, 18.0),
            lifetime: (2.2, 2.8),
            spread: std::f32::consts::PI,
            velocity_distribution: aestra_core::VelocityDistribution::LegacyCone as u32,
            gravity: [0.0, -20.0, 0.0],
            event_mask: 4,
            colliders: vec![aestra_core::Collider {
                shape: aestra_core::ColliderShape::Plane {
                    normal: [0.0, 1.0, 0.0],
                    distance: 0.0,
                },
                restitution: 0.3,
                friction: 0.5,
                kill: false,
            }],
            ..template.clone()
        };
        let glints = StatefulDispatch {
            capacity: 2048,
            slot_offset: 4160,
            emitter_index: 2,
            spawn_rate: 0.0,
            lifetime: (0.3, 0.6),
            ..template
        };
        scene.dispatches = vec![rockets, burst, glints];
        let buffer = |bytes: u64| {
            scene.device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: bytes,
                usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        scene.render = [buffer(6208 * 48), buffer(6208 * 4), buffer(64), buffer(16)];
        scene.states = scene
            .dispatches
            .iter()
            .map(|d| {
                StatefulPersistentState::allocate(
                    &scene.device,
                    d.capacity,
                    STRIDE,
                    d.fingerprint(),
                    d.event_mask != 0,
                )
            })
            .collect();
        let links = [
            aestra_runtime::CompiledEventLink {
                source: 0,
                trigger: aestra_core::EventTrigger::OnDeath,
                target: 1,
                count: 48,
                inherit: 0.2,
            },
            aestra_runtime::CompiledEventLink {
                source: 1,
                trigger: aestra_core::EventTrigger::OnCollision,
                target: 2,
                count: 2,
                inherit: 0.0,
            },
        ];
        Some((scene, links))
    }

    fn advance_chained_event_scene(
        scene: &mut Scene,
        links: &[aestra_runtime::CompiledEventLink],
        tick: u32,
    ) {
        advance_event_scene(scene, links, &RouteWiring::default(), tick);
    }

    /// Advances the scene's emitters in the production lockstep loop, with its event links and the
    /// bursts of input routes (event system E3), until every emitter reaches `tick`.
    fn advance_event_scene(
        scene: &mut Scene,
        links: &[aestra_runtime::CompiledEventLink],
        routes: &RouteWiring<'_>,
        tick: u32,
    ) {
        // A backward seek may restore an earlier checkpoint and need several bounded catch-up
        // submissions. Wait until all emitters reach the requested frame before observing state.
        loop {
            let mut encoder = scene.device.create_command_encoder(&Default::default());
            let [particles, alive, indirect, counters] = &scene.render;
            run_coupled_stateful(
                &scene.device,
                &mut encoder,
                (
                    &scene.pipelines[0],
                    &scene.pipelines[1],
                    &scene.pipelines[2],
                    Some(&scene.pipelines[3]),
                ),
                &scene.layout,
                &mut scene.states,
                &scene.dispatches,
                Coupling {
                    domains: &mut [],
                    inputs: StageInputs::default(),
                    follower: &scene.follower,
                    spawner: &scene.spawner,
                    gatherer: &scene.gatherer,
                },
                links,
                routes,
                &StatefulRenderBuffers {
                    particles,
                    alive,
                    indirect,
                    counters,
                    world: &scene.no_world,
                    physics: &scene.no_world,
                },
                (tick as f32 + 0.5) * STATEFUL_TICK_DT,
                4,
                None,
            );
            scene.queue.submit([encoder.finish()]);
            if scene.states.iter().all(|state| state.last_tick == tick) {
                break;
            }
        }
    }

    /// Host bindings HB9b: the production lockstep loop turns a rocket's death into its burst's
    /// particles — one rocket every few ticks, each death 48 sparks.
    #[test]
    fn event_links_spawn_sub_emitters_in_the_production_lockstep_loop() {
        let Some((mut scene, links)) = chained_event_scene() else {
            return;
        };
        for tick in 1..=240 {
            advance_chained_event_scene(&mut scene, &links, tick);
        }
        let staging = scene.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 8,
            usage: BufferUsages::COPY_DST | BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = scene.device.create_command_encoder(&Default::default());
        encoder.copy_buffer_to_buffer(&scene.states[0].spawn_counter, 0, &staging, 0, 4);
        encoder.copy_buffer_to_buffer(&scene.states[1].spawn_counter, 0, &staging, 4, 4);
        scene.queue.submit([encoder.finish()]);
        staging.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        scene
            .device
            .wgpu_device()
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(std::time::Duration::from_secs(60)),
            })
            .unwrap();
        let words: Vec<u32> = staging
            .slice(..)
            .get_mapped_range()
            .as_chunks::<4>()
            .0
            .iter()
            .map(|bytes| u32::from_le_bytes(*bytes))
            .collect();
        eprintln!("rockets spawned {}, sparks spawned {}", words[0], words[1]);
        assert!(words[0] >= 10, "{words:?}");
        assert!(words[1] >= 48 * 5, "every rocket death bursts: {words:?}");
    }

    #[derive(Debug, PartialEq, Eq)]
    struct ChainedEventSnapshot {
        particles: Vec<Vec<[u32; 10]>>,
        events: Vec<Vec<[u32; 8]>>,
        spawn_counts: Vec<u32>,
        draw_ordinals: Vec<Vec<u32>>,
    }

    fn chained_event_snapshot(scene: &Scene) -> ChainedEventSnapshot {
        let words = |buffer: &Buffer| {
            read_back(&scene.device, &scene.queue, buffer)
                .as_chunks::<4>()
                .0
                .iter()
                .map(|bytes| u32::from_le_bytes(*bytes))
                .collect::<Vec<_>>()
        };
        let particles = scene
            .states
            .iter()
            .map(|emitter| {
                let state = words(&emitter.state);
                let mut live: Vec<_> = state
                    .as_chunks::<10>()
                    .0
                    .iter()
                    .filter(|record| {
                        let age = f32::from_bits(record[6]);
                        let lifetime = f32::from_bits(record[7]);
                        lifetime > 0.0 && age < lifetime
                    })
                    .copied()
                    .collect();
                live.sort_unstable_by_key(|record| record[8]);
                live
            })
            .collect();
        let events = scene
            .states
            .iter()
            .map(|emitter| {
                let source = words(&emitter.events);
                let count = source[0].min(aestra_runtime::PARTICLE_EVENT_CAPACITY) as usize;
                let mut records: Vec<_> = source[4..]
                    .as_chunks::<8>()
                    .0
                    .iter()
                    .take(count)
                    .copied()
                    .collect();
                records.sort_unstable_by_key(|record| (record[1], record[0]));
                records
            })
            .collect();
        let spawn_counts = scene
            .states
            .iter()
            .map(|emitter| words(&emitter.spawn_counter)[0])
            .collect();
        let presentation = words(&scene.render[0]);
        let alive_indices = words(&scene.render[1]);
        let indirect = words(&scene.render[2]);
        let draw_ordinals = scene
            .dispatches
            .iter()
            .map(|dispatch| {
                let count = indirect[dispatch.emitter_index as usize * 4 + 1] as usize;
                let offset = dispatch.slot_offset as usize;
                let ordinals: Vec<_> = alive_indices[offset..offset + count]
                    .iter()
                    .map(|index| presentation[*index as usize * 12 + 11])
                    .collect();
                if count <= 4096 {
                    assert!(
                        ordinals.is_sorted(),
                        "stateful transparent order is unstable"
                    );
                }
                ordinals
            })
            .collect();
        ChainedEventSnapshot {
            particles,
            events,
            spawn_counts,
            draw_ordinals,
        }
    }

    #[test]
    fn a_changed_host_input_history_differs_from_its_first_changed_tick() {
        let history = [(30, 1), (70, 2)];
        assert_eq!(HostEventHistory::divergence(&history, &history), None);
        assert_eq!(
            HostEventHistory::divergence(&history, &[(30, 1), (70, 2), (90, 3)]),
            Some(90),
            "an event appended"
        );
        assert_eq!(
            HostEventHistory::divergence(&history, &[(30, 1), (70, 5)]),
            Some(70),
            "a payload changed"
        );
        assert_eq!(
            HostEventHistory::divergence(&history, &[(30, 1), (50, 4)]),
            Some(50),
            "an event recorded earlier, after a backward seek"
        );
        assert_eq!(HostEventHistory::divergence(&history, &[]), Some(30));
    }

    /// Event system E3: a host's detonations spawn their bursts in the production lockstep loop,
    /// after the tick's event links; a history changed after the fact — detonations recorded once the
    /// run was past them — replays from where it differs, to exactly the state of a run that had them
    /// all along, and so does a backward seek.
    #[test]
    fn input_bursts_spawn_in_the_lockstep_loop_and_a_changed_history_replays_exactly() {
        let Some((mut fresh, links)) = chained_event_scene() else {
            return;
        };
        let (mut late, _) = chained_event_scene().expect("a second GPU scene should initialize");
        let burst = |tick, positions: &[[f32; 3]]| aestra_runtime::InputSpawnBurst {
            tick,
            route: 0,
            target: 1,
            count: 24,
            events: positions
                .iter()
                .enumerate()
                .map(|(ordinal, position)| aestra_runtime::ParticleEvent {
                    ordinal: ordinal as u64,
                    position: *position,
                    velocity: [0.0; 3],
                })
                .collect(),
        };
        let bursts = [
            burst(30, &[[4.0, 12.0, -2.0]]),
            burst(70, &[[-6.0, 9.0, 1.0], [0.0, 20.0, 0.0]]),
        ];
        let wired = RouteWiring {
            bursts: &bursts,
            outputs: &[],
            ..Default::default()
        };
        let history = [(30, 1), (70, 2)];
        for state in &mut fresh.states {
            state.set_host_events(&history);
        }
        // Just past the first detonation (the rockets' links fire later): its particles, born at the
        // detonation point once the tick advanced.
        advance_event_scene(&mut fresh, &links, &wired, 31);
        let first = chained_event_snapshot(&fresh);
        assert_eq!(first.spawn_counts[1], 24, "{:?}", first.spawn_counts);
        let point = [4.0_f32, 12.0, -2.0].map(f32::to_bits);
        assert!(
            first.particles[1]
                .iter()
                .all(|record| record[..3] == point && f32::from_bits(record[6]) == 0.0),
            "{:?}",
            first.particles[1]
        );
        advance_event_scene(&mut fresh, &links, &wired, 150);
        let expected = chained_event_snapshot(&fresh);
        assert!(
            expected.spawn_counts[1] > 24 * 3,
            "the rockets' links fired too"
        );

        // The same run without the detonations, which the host then records back in time.
        advance_event_scene(&mut late, &links, &RouteWiring::default(), 150);
        assert_ne!(chained_event_snapshot(&late), expected);
        for state in &mut late.states {
            state.set_host_events(&history);
            assert_eq!(state.rewind, Some(30));
            assert!(
                state
                    .checkpoints
                    .iter()
                    .all(|checkpoint| checkpoint.tick <= 30)
            );
        }
        advance_event_scene(&mut late, &links, &wired, 150);
        assert_eq!(chained_event_snapshot(&late), expected);
        // A backward seek replays the recorded detonations as well.
        advance_event_scene(&mut late, &links, &wired, 50);
        advance_event_scene(&mut late, &links, &wired, 150);
        assert_eq!(chained_event_snapshot(&late), expected);
    }

    #[test]
    fn external_birth_outputs_reach_the_host_tick_without_pause_or_replay_duplicates() {
        let Some((mut scene, mut links)) = chained_event_scene() else {
            assert!(std::env::var_os("AESTRA_REQUIRE_GPU_CONFORMANCE").is_none());
            return;
        };
        scene.dispatches[0].spawn_rate = 0.0;
        scene.dispatches[0].burst_count = 1;
        scene.dispatches[0].burst_tick = 0;
        scene.dispatches[0].lifetime = (0.05, 0.05);
        for dispatch in &mut scene.dispatches[1..] {
            dispatch.event_mask = 1;
            dispatch.speed = (0.0, 0.0);
            dispatch.lifetime = (10.0, 10.0);
            dispatch.colliders.clear();
        }
        scene.states = scene
            .dispatches
            .iter()
            .map(|dispatch| {
                StatefulPersistentState::allocate(
                    &scene.device,
                    dispatch.capacity,
                    STRIDE,
                    dispatch.fingerprint(),
                    true,
                )
            })
            .collect();
        let ring = aestra_gpu::PARTICLE_OUTPUT_RING_WORDS;
        scene.render[3] = scene.device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some("external birth output regression rings"),
            contents: &vec![0; (32 + 2 * ring) as usize * 4],
            usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
        });
        links[0].count = 12;
        let links = &links[..1];
        let outputs: Vec<_> = [1, 2]
            .into_iter()
            .enumerate()
            .map(|(index, source)| {
                (
                    aestra_runtime::CompiledParticleOutput {
                        output: format!("birth_{source}"),
                        source,
                        trigger: aestra_core::EventTrigger::OnSpawn,
                        aggregation: aestra_core::EventAggregation::EachEvent { limit: 3 },
                    },
                    32 + index as u32 * ring,
                )
            })
            .collect();
        let bursts: Vec<_> = [(1, 3), (6, 5)]
            .into_iter()
            .map(|(tick, count)| aestra_runtime::InputSpawnBurst {
                tick,
                route: 0,
                target: 2,
                count,
                events: vec![aestra_runtime::ParticleEvent {
                    ordinal: 0,
                    position: [4.0, 12.0, -2.0],
                    velocity: [0.0; 3],
                }],
            })
            .collect();
        let wiring = RouteWiring {
            bursts: &bursts,
            outputs: &outputs,
            output_epoch: 7,
            ..Default::default()
        };
        for tick in 1..=8 {
            advance_event_scene(&mut scene, links, &wiring, tick);
        }
        let records = |scene: &Scene| {
            read_back(&scene.device, &scene.queue, &scene.render[3])
                .as_chunks::<4>()
                .0
                .iter()
                .map(|word| u32::from_le_bytes(*word))
                .collect::<Vec<_>>()
        };
        let initial = records(&scene);
        let decode = |words: &[u32], base: usize| {
            words[base..base + ring as usize]
                .as_chunks::<{ aestra_gpu::PARTICLE_OUTPUT_SLOT_WORDS as usize }>()
                .0
                .iter()
                .filter_map(|slot| aestra_gpu::read_particle_output_slot(slot))
                .filter(|record| record.count > 0)
                .collect::<Vec<_>>()
        };
        let linked = decode(&initial, 32);
        assert_eq!(linked.len(), 1);
        assert_eq!(
            (linked[0].tick, linked[0].epoch, linked[0].count),
            (4, 7, 12)
        );
        assert_eq!(linked[0].first.len(), 3);
        let inputs = decode(&initial, (32 + ring) as usize);
        assert_eq!(
            inputs
                .iter()
                .map(|r| (r.tick, r.epoch, r.count))
                .collect::<Vec<_>>(),
            [(2, 7, 3), (7, 7, 5)]
        );
        assert!(
            inputs
                .iter()
                .flat_map(|r| &r.first)
                .all(|(_, position)| *position == [4.0, 12.0, -2.0])
        );
        let expected = chained_event_snapshot(&scene);
        advance_event_scene(&mut scene, links, &wiring, 8);
        assert_eq!(
            &records(&scene)[32..],
            &initial[32..],
            "pause cannot export twice"
        );
        let replay = RouteWiring {
            output_epoch: 8,
            output_suppress_through: 8,
            ..wiring
        };
        advance_event_scene(&mut scene, links, &replay, 2);
        for tick in 3..=8 {
            advance_event_scene(&mut scene, links, &replay, tick);
        }
        assert_eq!(chained_event_snapshot(&scene), expected);
        assert_eq!(
            &records(&scene)[32..],
            &initial[32..],
            "seek/replay cannot export old births"
        );
        for state in &mut scene.states {
            state.reset_to_zero(&scene.device);
        }
        let restarted = RouteWiring {
            output_epoch: 9,
            ..wiring
        };
        for tick in 1..=8 {
            advance_event_scene(&mut scene, links, &restarted, tick);
        }
        let fresh = records(&scene);
        assert_eq!(decode(&fresh, 32)[0].epoch, 9);
        assert_eq!(decode(&fresh, (32 + ring) as usize)[0].epoch, 9);
        assert_eq!(chained_event_snapshot(&scene), expected);
    }

    #[test]
    fn event_born_particles_present_changed_cooling_gradients_without_changing_state() {
        let Some((mut scene, links)) = chained_event_scene() else {
            assert!(
                std::env::var_os("AESTRA_REQUIRE_GPU_CONFORMANCE").is_none(),
                "stateful appearance regression requires a hardware GPU"
            );
            return;
        };
        scene.dispatches[0].spawn_rate = 0.0;
        scene.dispatches[0].burst_count = 1;
        scene.dispatches[0].lifetime = (0.25, 0.25);
        advance_chained_event_scene(&mut scene, &links, 80);
        let baseline = chained_event_snapshot(&scene);
        assert_eq!(baseline.spawn_counts[..2], [1, 48]);
        assert_eq!(baseline.particles[1].len(), 48);
        let original_presentation = read_back(&scene.device, &scene.queue, &scene.render[0]);
        let fingerprint = scene.dispatches[1].fingerprint();
        let gradient = aestra_core::Gradient::new(vec![
            aestra_core::ColorKey::new(0.0, [0.1, 1.0, 0.2, 1.0]),
            aestra_core::ColorKey::new(0.6, [0.02, 0.5, 0.1, 1.0]),
            aestra_core::ColorKey::new(1.0, [0.0, 0.08, 0.02, 1.0]),
        ]);
        let mut packed = GpuGradient {
            count: gradient.keys.len() as u32,
            ..Default::default()
        };
        for (key, source) in packed.keys.iter_mut().zip(&gradient.keys) {
            key.color = Vec4::from_array(source.color);
            key.time = source.time;
        }
        scene.dispatches[1].appearance.color = packed;
        assert_eq!(scene.dispatches[1].fingerprint(), fingerprint);
        // Re-present the current tick, not a new simulation or an extra burst.
        advance_chained_event_scene(&mut scene, &links, 80);
        assert_eq!(chained_event_snapshot(&scene), baseline);
        let words: Vec<_> = read_back(&scene.device, &scene.queue, &scene.render[0])
            .as_chunks::<4>()
            .0
            .iter()
            .map(|bytes| u32::from_le_bytes(*bytes))
            .collect();
        let particles: Vec<_> = words
            .as_chunks::<12>()
            .0
            .iter()
            .filter(|particle| particle[10] >> 16 == 1 && particle[10] & 0xffff != 0)
            .collect();
        assert_eq!(particles.len(), 48);
        let expected = aestra_runtime::CompiledGradient::compile(&gradient);
        for particle in particles {
            let color = expected.sample(f32::from_bits(particle[9]));
            for (actual, expected) in particle[..4].iter().zip(color) {
                assert!((f32::from_bits(*actual) - expected).abs() < 1e-5);
            }
        }
        scene.dispatches[1].appearance.color = GpuGradient::default();
        advance_chained_event_scene(&mut scene, &links, 80);
        assert_eq!(chained_event_snapshot(&scene), baseline);
        assert_eq!(
            read_back(&scene.device, &scene.queue, &scene.render[0]),
            original_presentation
        );
    }

    #[test]
    fn authored_one_shot_burst_launches_once_and_drives_death_links_after_seek() {
        for burst_tick in [0, 30] {
            let Some((mut scene, links)) = chained_event_scene() else {
                assert!(
                    std::env::var_os("AESTRA_REQUIRE_GPU_CONFORMANCE").is_none(),
                    "authored burst regression requires a hardware GPU"
                );
                return;
            };
            scene.dispatches[0].spawn_rate = 0.0;
            scene.dispatches[0].burst_count = 1;
            scene.dispatches[0].burst_tick = burst_tick;
            scene.dispatches[0].lifetime = (0.25, 0.25);
            advance_chained_event_scene(&mut scene, &links, burst_tick + 1);
            assert_eq!(chained_event_snapshot(&scene).spawn_counts[0], 1);
            advance_chained_event_scene(&mut scene, &links, 80);
            let expected = chained_event_snapshot(&scene);
            assert_eq!(
                expected.spawn_counts[0], 1,
                "no continuous emission or repeated burst"
            );
            assert_eq!(
                expected.spawn_counts[1], 48,
                "one real death drives exactly one child cohort"
            );
            advance_chained_event_scene(&mut scene, &links, burst_tick);
            advance_chained_event_scene(&mut scene, &links, 80);
            assert_eq!(
                chained_event_snapshot(&scene),
                expected,
                "replay neither drops nor doubles the launch"
            );

            scene.dispatches[0].cutoffs.stop_tick = Some(0);
            for state in &mut scene.states {
                state.reset_to_zero(&scene.device);
            }
            advance_chained_event_scene(&mut scene, &links, 80);
            assert_eq!(
                chained_event_snapshot(&scene).spawn_counts[..2],
                [0, 0],
                "stop suppresses authored birth and its child links"
            );
        }
    }

    #[test]
    fn chained_fireworks_match_across_fresh_runs_and_backward_seek() {
        let Some((mut first, links)) = chained_event_scene() else {
            assert!(
                std::env::var_os("AESTRA_REQUIRE_GPU_CONFORMANCE").is_none(),
                "fireworks replay conformance requires a hardware GPU"
            );
            eprintln!("skipping fireworks replay conformance: no hardware GPU");
            return;
        };
        let (mut second, _) = chained_event_scene().expect("a second GPU scene should initialize");
        let mut reference = Vec::new();
        for tick in 1..=300 {
            advance_chained_event_scene(&mut first, &links, tick);
            advance_chained_event_scene(&mut second, &links, tick);
            if [120, 180, 240, 300].contains(&tick) {
                let expected = chained_event_snapshot(&first);
                assert_eq!(
                    chained_event_snapshot(&second),
                    expected,
                    "fresh GPU runs diverged at frame {tick}"
                );
                reference.push((tick, expected));
            }
        }
        assert!(reference[3].1.spawn_counts[1] >= 48 * 5);
        assert!(reference[3].1.spawn_counts[2] > 0);

        advance_chained_event_scene(&mut first, &links, 90);
        for tick in 91..=300 {
            advance_chained_event_scene(&mut first, &links, tick);
            if let Some((_, expected)) = reference.iter().find(|(frame, _)| *frame == tick) {
                assert_eq!(
                    &chained_event_snapshot(&first),
                    expected,
                    "checkpoint restore and replay diverged at frame {tick}"
                );
            }
        }
    }

    #[test]
    fn distance_smoke_checkpoint_restores_path_remainders_and_children() {
        let Some((mut scene, mut links)) = chained_event_scene() else {
            assert!(std::env::var_os("AESTRA_REQUIRE_GPU_CONFORMANCE").is_none());
            return;
        };
        let trigger = aestra_core::EventTrigger::OnDistance {
            spacing: 0.3,
            max_per_tick: 8,
        };
        scene.dispatches[0].event_mask = aestra_runtime::event_trigger_bit(trigger);
        scene.dispatches[0].distance_emission = trigger.distance_settings();
        scene.dispatches[1].speed = (0.0, 0.0);
        scene.dispatches[1].gravity = [0.0; 3];
        scene.dispatches[1].colliders.clear();
        scene.dispatches[1].event_mask = 0;
        links[0].trigger = trigger;
        links[0].count = 1;
        links[0].inherit = 0.0;
        let links = &links[..1];
        let mut reference = Vec::new();
        for tick in 1..=240 {
            advance_chained_event_scene(&mut scene, links, tick);
            if [120, 180].contains(&tick) {
                reference.push((tick, chained_event_snapshot(&scene)));
            }
        }
        assert!(reference[1].1.spawn_counts[1] > 100);
        advance_chained_event_scene(&mut scene, links, 90);
        for tick in 91..=180 {
            advance_chained_event_scene(&mut scene, links, tick);
            if let Some((_, expected)) = reference.iter().find(|(frame, _)| *frame == tick) {
                assert_eq!(
                    &chained_event_snapshot(&scene),
                    expected,
                    "distance checkpoint diverged at tick {tick}"
                );
            }
        }
    }

    /// Host bindings HB8: with a binding trace, every tick steers toward its own recorded target, so
    /// scrubbing back and replaying reproduces the uninterrupted run bit for bit; with live input,
    /// the replay uses the present target and the past changes.
    #[test]
    fn a_traced_homing_target_replays_exactly_after_a_backward_seek() {
        let Some(mut scene) = scene(false) else {
            return;
        };
        scene.domains.clear();
        let circle = |tick: u32| {
            let angle = tick as f32 * 0.03;
            Some(aestra_runtime::HomingTarget {
                position: [6.0 * angle.cos(), 4.0, 6.0 * angle.sin()],
                velocity: [0.0; 3],
            })
        };
        let homing = aestra_runtime::CompiledHoming {
            config: aestra_runtime::HomingConfig {
                speed: 20.0,
                acceleration: 40.0,
                turn_rate: 6.0,
                arrival_radius: 0.3,
                lost: aestra_runtime::HomingLostPolicy::KeepLastPosition,
            },
            target: [0.0; 3],
            target_velocity: [0.0; 3],
            target_source: None,
            velocity_source: None,
        };
        let template = scene.dispatches[0].clone();
        let traced = |schedule: bool| StatefulDispatch {
            spawn_rate: 60.0,
            lifetime: (2.0, 2.5),
            homing: Some(homing.clone()),
            homing_target: circle(0),
            schedule: schedule.then(|| {
                Arc::new(TickSchedule {
                    key: 7,
                    homing: (0..240).map(circle).collect(),
                    placement: Vec::new(),
                })
            }),
            ..template.clone()
        };
        // Live particles by spawn ordinal, bit for bit (slot assignment follows thread timing).
        let mut run = |dispatch: StatefulDispatch, scrub: bool| -> Vec<[u32; 10]> {
            scene.dispatches = vec![dispatch];
            scene.states = vec![StatefulPersistentState::allocate(
                &scene.device,
                scene.dispatches[0].capacity,
                STRIDE,
                scene.dispatches[0].fingerprint(),
                false,
            )];
            let frame = |scene: &mut Scene, tick: u32| loop {
                // Live input: the frame's target is the host's present one.
                scene.dispatches[0].homing_target = circle(tick);
                let mut encoder = scene.device.create_command_encoder(&Default::default());
                let [particles, alive, indirect, counters] = &scene.render;
                run_coupled_stateful(
                    &scene.device,
                    &mut encoder,
                    (
                        &scene.pipelines[0],
                        &scene.pipelines[1],
                        &scene.pipelines[2],
                        Some(&scene.pipelines[3]),
                    ),
                    &scene.layout,
                    &mut scene.states,
                    &scene.dispatches,
                    Coupling {
                        domains: &mut [],
                        inputs: StageInputs::default(),
                        follower: &scene.follower,
                        spawner: &scene.spawner,
                        gatherer: &scene.gatherer,
                    },
                    &[],
                    &RouteWiring::default(),
                    &StatefulRenderBuffers {
                        particles,
                        alive,
                        indirect,
                        counters,
                        world: &scene.no_world,
                        physics: &scene.no_world,
                    },
                    (tick as f32 + 0.5) * STATEFUL_TICK_DT,
                    TICKS_PER_SUBMISSION,
                    None,
                );
                scene.queue.submit([encoder.finish()]);
                if scene.states[0].last_tick == tick {
                    break;
                }
            };
            for tick in 1..=180 {
                frame(&mut scene, tick);
            }
            if scrub {
                frame(&mut scene, 60);
                frame(&mut scene, 180);
            }
            let size = scene.states[0].state.size();
            let staging = scene.device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size,
                usage: BufferUsages::COPY_DST | BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            let mut encoder = scene.device.create_command_encoder(&Default::default());
            encoder.copy_buffer_to_buffer(&scene.states[0].state, 0, &staging, 0, size);
            scene.queue.submit([encoder.finish()]);
            staging.slice(..).map_async(wgpu::MapMode::Read, |_| {});
            scene
                .device
                .wgpu_device()
                .poll(wgpu::PollType::Wait {
                    submission_index: None,
                    timeout: Some(std::time::Duration::from_secs(60)),
                })
                .unwrap();
            let words: Vec<u32> = staging
                .slice(..)
                .get_mapped_range()
                .as_chunks::<4>()
                .0
                .iter()
                .map(|bytes| u32::from_le_bytes(*bytes))
                .collect();
            let mut live: Vec<[u32; 10]> = words
                .as_chunks::<10>()
                .0
                .iter()
                .filter(|record| {
                    let (age, lifetime) = (f32::from_bits(record[6]), f32::from_bits(record[7]));
                    lifetime > 0.0 && age < lifetime
                })
                .copied()
                .collect();
            live.sort_by_key(|record| record[8]);
            live
        };
        let straight = run(traced(true), false);
        let scrubbed = run(traced(true), true);
        assert!(straight.len() > 50, "{} live", straight.len());
        assert!(
            straight == scrubbed,
            "a traced target replays the uninterrupted run exactly"
        );
        let live_straight = run(traced(false), false);
        let live_scrubbed = run(traced(false), true);
        assert!(
            live_straight != live_scrubbed,
            "live input replays the past with the present target: not exact, as reported"
        );
    }

    impl Scene {
        /// One frame at `tick` (mid-tick, so the target is exactly that tick).
        fn frame(&mut self, tick: u32) {
            let time = (tick as f32 + 0.5) * STATEFUL_TICK_DT;
            loop {
                let mut encoder = self.device.create_command_encoder(&Default::default());
                let host = aestra_gpu::GpuHostBindings { words: vec![0] };
                let [particles, alive, indirect, counters] = &self.render;
                run_coupled_stateful(
                    &self.device,
                    &mut encoder,
                    (
                        &self.pipelines[0],
                        &self.pipelines[1],
                        &self.pipelines[2],
                        Some(&self.pipelines[3]),
                    ),
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
                        spawner: &self.spawner,
                        gatherer: &self.gatherer,
                    },
                    &[],
                    &RouteWiring::default(),
                    &StatefulRenderBuffers {
                        particles,
                        alive,
                        indirect,
                        counters,
                        world: &self.no_world,
                        physics: &self.no_world,
                    },
                    time,
                    TICKS_PER_SUBMISSION,
                    None,
                );
                self.queue.submit([encoder.finish()]);
                // Bound each native command buffer and drain it before encoding more replay work.
                // In particular, a seek must not queue an entire fluid replay in one submission.
                self.device
                    .wgpu_device()
                    .poll(wgpu::PollType::Wait {
                        submission_index: None,
                        timeout: Some(std::time::Duration::from_secs(120)),
                    })
                    .expect("coupled simulation submission should complete");
                if self.states.iter().all(|state| state.last_tick == tick) {
                    break;
                }
            }
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
        let (sender, receiver) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        device
            .wgpu_device()
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(std::time::Duration::from_secs(120)),
            })
            .expect("coupled simulation readback submission should complete");
        receiver
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("coupled simulation mapping callback should run")
            .expect("coupled simulation buffer should map successfully");
        let bytes = slice.get_mapped_range().to_vec();
        readback.unmap();
        bytes
    }

    #[test]
    fn playback_history_policy_preserves_a_domain_and_resumes_future_captures() {
        let Some(mut scene) = require(scene(false)) else {
            return;
        };
        let domain = scene.domains[0].as_mut().unwrap();
        let advance = |domain: &mut StageTimeline, target| {
            let mut encoder = scene.device.create_command_encoder(&Default::default());
            let report = domain
                .advance_to(
                    scene.device.wgpu_device(),
                    &mut encoder,
                    target,
                    60,
                    StageInputs::default(),
                    None,
                )
                .unwrap();
            scene.queue.submit([encoder.finish()]);
            report
        };
        let velocity = |domain: &StageTimeline| {
            read_back(
                &scene.device,
                &scene.queue,
                domain
                    .executor()
                    .buffer(aestra_fluid::RESOURCE_VELOCITY)
                    .unwrap(),
            )
        };
        advance(domain, 20);
        let before = velocity(domain);
        assert!(domain.checkpoint_bytes() > 0);
        domain.set_history_policy(PlaybackHistoryPolicy::PlaybackOnly);
        assert_eq!(domain.last_tick(), 20);
        assert_eq!(domain.checkpoint_bytes(), 0);
        assert_eq!(advance(domain, 20).ticks, 0);
        assert_eq!(velocity(domain), before);
        advance(domain, 40);
        assert_eq!(domain.checkpoint_bytes(), 0);
        assert_eq!(advance(domain, 20).restored_from, Some(0));
        assert_eq!(velocity(domain), before);
        domain.set_history_policy(PlaybackHistoryPolicy::ReplayEnabled);
        assert_eq!(domain.last_tick(), 20);
        advance(domain, 40);
        assert_eq!(domain.checkpoint_ticks(), vec![40]);
        assert!(domain.checkpoint_bytes() > 0);
    }

    fn require(scene: Option<Scene>) -> Option<Scene> {
        if scene.is_none() {
            assert!(
                std::env::var_os("AESTRA_REQUIRE_GPU_CONFORMANCE").is_none(),
                "AESTRA_REQUIRE_GPU_CONFORMANCE is set but no hardware GPU is available"
            );
            eprintln!("skipping coupled conformance: no hardware GPU");
        }
        scene
    }

    #[test]
    #[ignore = "requires a hardware GPU; exercised by the native GPU visual workflow"]
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

    fn place(scene: &mut Scene, placement: aestra_runtime::SpawnPlacement) {
        for (state, dispatch) in scene.states.iter_mut().zip(&mut scene.dispatches) {
            dispatch.placement = placement;
            state.set_placement(placement);
        }
    }

    #[test]
    #[ignore = "requires a hardware GPU; exercised by the native GPU visual workflow"]
    fn moving_an_emitter_keeps_the_run_live_and_a_seek_replays_the_new_placement() {
        let Some(mut moved) = require(scene(true)) else {
            return;
        };
        let placement = aestra_runtime::SpawnPlacement {
            translation: [12.0, 3.0, -4.0],
            rotation: Quat::from_rotation_z(0.8).to_array(),
            scale: [1.5; 3],
        };
        moved.frame(60);
        let before = moved.state();
        place(&mut moved, placement);
        moved.frame(61);
        // A gizmo drag re-places the spawns without restarting anything: one more tick.
        assert_eq!(moved.states[0].last_tick, 61);
        assert_eq!(moved.domains[0].as_ref().unwrap().last_tick(), 61);
        moved.frame(90);
        assert!(
            moved
                .states
                .iter()
                .all(|state| state.checkpoints.is_empty()),
            "a history mixing two placements is never checkpointed"
        );
        // Seeking back replays from tick 0 under the one placement.
        moved.frame(30);
        moved.frame(90);

        let mut placed = scene(true).unwrap();
        place(&mut placed, placement);
        placed.frame(90);
        let placed = placed.state();
        assert_eq!(
            moved.state(),
            placed,
            "the replay equals a run placed from the start"
        );
        assert_ne!(placed[0], before[0]);

        let mut unplaced = scene(true).unwrap();
        unplaced.frame(90);
        assert_ne!(
            unplaced.state()[0],
            placed[0],
            "the placement moved the spawns"
        );
    }
}
