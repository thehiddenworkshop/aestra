//! Main-world GPU particle allocation and live material/input preparation.
//! Shared verbatim by shipping and engine-upgrade qualification.
use super::draw_instance::{
    GpuDrawInstance, GpuRenderMode, GpuSemanticMaterialBinding, HostMotionDraw,
};
pub(super) use super::effect_inputs::StatefulAppearance;
use super::effect_inputs::{GpuEffectBuffers, HostEventHistory, StatefulDispatch};
use super::particle_output_readback::{
    GpuArrivalReadback, GpuEventLinkCounts, GpuEventLinkStatistics, receive_homing_arrivals,
};
use super::particle_statistics::GpuParticleStatistics;
use super::preparation_timing::GpuPreparationTiming;
use super::simulation_timing::GpuSimulationTiming;
use super::stateful_simulation::STATEFUL_TICK_DT;
use super::transform_sync::spawn_placement;
use super::{
    bounds, material_shader, particle_lights, particle_statistics, ribbon_bounds, sprite_culling,
    storage_buffers,
};
use crate::{
    ActiveBackend, CompatibilityIssue, CompatibilityIssueCode, CompatibilityReport,
    EffectRenderMode, EffectRuntimeStatus, GpuCapabilities, GpuPresentationPrepared,
    PresentedEffect, ProjectAssetCache,
    material::{MaterialBindingError, MaterialRuntimeBinding},
};
use aestra_gpu::material::MaterialProgramFingerprint;
use aestra_gpu::particle_attributes::{GpuParticleAttributes, prune_particle_attributes};
use aestra_gpu::{
    GpuArtifactError, GpuBlend, GpuEffectArtifact, GpuGlobals, GpuRenderGlobals, GpuRenderParams,
    GpuRenderer, WORKGROUP_SIZE, fold_seed, indirect_draw_commands_with_statistics,
    indirect_draw_offset,
};
use aestra_runtime::{RendererPlanKind, SimulationClass};
use bevy::{
    asset::RenderAssetUsages,
    camera::{
        primitives::Aabb,
        visibility::{self, RenderLayers},
    },
    log::tracing,
    prelude::*,
    render::{
        gpu_readback::{Readback, ReadbackComplete},
        render_resource::{Extent3d, TextureDimension, TextureFormat},
        storage::ShaderBuffer,
    },
};
use std::{collections::BTreeMap, sync::Arc};

pub(super) fn install(app: &mut App) {
    app.init_resource::<MaterialShaderCache>()
        .add_systems(Startup, init_fallback_textures)
        .add_systems(Update, update_gpu_inputs.after(prepare_gpu_effects));
}

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
    pub(super) fn plain() -> Self {
        let mut keys = aestra_gpu::GpuCurve::default().keys;
        keys[0] = [0.0, 1.0].into();
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

#[derive(Clone)]
pub(super) struct MaterialShaderVariants {
    pub(super) single_sampled: Handle<Shader>,
    pub(super) multisampled: Handle<Shader>,
}

#[derive(Resource, Default)]
pub(crate) struct MaterialShaderCache(BTreeMap<MaterialProgramFingerprint, MaterialShaderVariants>);

#[derive(Component)]
pub(crate) struct GpuReadbackOwner(pub(crate) Entity);

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
    playback_diagnostics: Option<Res<super::playback_diagnostics::GpuPlaybackDiagnostics>>,
) {
    let _span = tracing::info_span!("aestra::gpu::prepare_instance").entered();
    for (entity, mut player, runtime, render_layers) in &mut players {
        let activation_start = playback_diagnostics
            .as_ref()
            .is_some_and(|d| d.profiling_enabled())
            .then(std::time::Instant::now);
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
            half_extents: Vec3A::from_array(artifact.bounds_half_extents.to_array()),
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
            _padding: Default::default(),
            emission_end: player.instance.emission_cutoffs().emission_end(),
            kill_time: player.instance.emission_cutoffs().kill_time(),
            _cutoff_padding: Default::default(),
            world_from_effect: Default::default(),
        }));
        let render_globals = buffers.add(storage_buffers::new(GpuRenderGlobals {
            world_from_effect: Default::default(),
            time: player.simulation_time(),
            seed: fold_seed(player.instance.seed()),
            _padding: Default::default(),
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
                        let mut render_params = GpuRenderParams {
                            renderer_index,
                            alive_offset,
                            _padding: Default::default(),
                            ..Default::default()
                        };
                        *render_params.mesh_from_local.as_mut() = mesh_from_emitter(
                            player.effect().emitters[emitter_index as usize].transform,
                        )
                        .to_cols_array();
                        let render_params = buffers.add(storage_buffers::new(render_params));
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
        if let Some(started) = activation_start {
            commands
                .entity(entity)
                .insert(super::playback_diagnostics::GpuActivationTiming {
                    cpu_preparation_ns: started.elapsed().as_nanos().min(u128::from(u64::MAX))
                        as u64,
                });
        }
    }
}

pub(super) fn apply_semantic_sprite_compatibility(
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

pub(super) fn apply_semantic_sprite_compatibility_to_renderers(
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
                                    half_extents: Vec3::from_array(
                                        motion.position_half_extents.to_array(),
                                    ),
                                    maximum_size: motion.maximum_size
                                        * motion
                                            .linear_from_local
                                            .x_axis
                                            .length()
                                            .max(motion.linear_from_local.y_axis.length())
                                            .max(motion.linear_from_local.z_axis.length()),
                                    world_from_effect: Default::default(),
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
    let shader_variants = prepare_material_shaders(&program, shaders, shader_cache);
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

pub(super) fn prepare_material_shaders(
    program: &aestra_gpu::material::CompiledMaterialProgram,
    shaders: &mut Assets<Shader>,
    shader_cache: &mut MaterialShaderCache,
) -> MaterialShaderVariants {
    shader_cache
        .0
        .entry(program.program_fingerprint)
        .or_insert_with(|| {
            let single_sampled = shaders.add(material_shader::shader(
                material_shader::compose(&program.shader.wgsl, program.requires_scene_lighting()),
                format!(
                    "generated://aestra/material/{}.wgsl",
                    program.program_fingerprint
                ),
            ));
            let multisampled = if program.requires_scene_depth() {
                shaders.add(material_shader::shader(
                    material_shader::compose(
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
        .clone()
}

const fn gpu_blend(blend: aestra_core::BlendMode) -> GpuBlend {
    match blend {
        aestra_core::BlendMode::Alpha => GpuBlend::Alpha,
        aestra_core::BlendMode::Additive => GpuBlend::Additive,
        aestra_core::BlendMode::Multiply => GpuBlend::Multiply,
    }
}

pub use super::field_observations::GpuTrailStatistics;

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
#[path = "tests/preparation.rs"]
mod tests;
