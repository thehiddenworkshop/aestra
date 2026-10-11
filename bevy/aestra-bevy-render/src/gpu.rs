//! Bevy render-world adapter for engine-neutral Aestra GPU artifacts.

mod alpha_sort;
// Test-only wgpu-version boundary; playback never maps sort buffers.
#[cfg(test)]
fn alpha_sort_test_readback(buffer: &wgpu::Buffer) -> Vec<u8> {
    buffer.slice(..).get_mapped_range().to_vec()
}
mod bounds;
#[cfg(test)]
#[path = "gpu/bounds_test_types_019.rs"]
mod bounds_test_types;
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
#[path = "gpu/material_shader_019.rs"]
mod material_shader;
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
mod preparation;
mod preparation_context;
mod preparation_timing;
mod presentation_readiness;
mod render;
mod ribbon_bounds;
#[path = "gpu/scene_depth_019.rs"]
mod scene_depth;
mod shader_composition;
mod simulation_graph;
mod simulation_pipeline;
mod simulation_timing;
mod sprite_culling;
mod stage_coupling;
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
mod transform_sync;
mod view_phases;
mod visibility_updates;
mod volume;
#[path = "gpu/volume_shader_019.rs"]
mod volume_shader;
mod wireframe;
mod world_sdf;

pub use catchup_pacing::AestraCatchupPacing;
#[cfg(test)]
use catchup_pacing::stateful_catchup_budget;
#[cfg(test)]
use draw_instance::gpu_draw_mesh_center;
use draw_instance::{GpuDrawInstance, GpuRenderMode};
pub(crate) use effect_inputs::GpuEffectBuffers;
#[cfg(test)]
use effect_inputs::HostEventHistory;
#[cfg(test)]
use effect_inputs::RouteWiring;
#[cfg(test)]
use effect_inputs::StatefulDispatch;
#[cfg(test)]
use effect_inputs::TickSchedule;
#[cfg(test)]
use effect_inputs::event_link_counter_base;
use preparation::GpuReadbackOwner;
pub(crate) use preparation::prepare_gpu_effects;
pub use preparation::{GpuFallbackTextures, GpuTrailStatistics};
#[cfg(test)]
use preparation::{StatefulAppearance, apply_semantic_sprite_compatibility_to_renderers};

use crate::{AestraRenderSettings, GpuCapabilities, PresentedEffect, capabilities::select_backend};
#[cfg(test)]
use aestra_core::MaterialId;
#[cfg(test)]
use aestra_gpu::GpuBlend;
pub use aestra_gpu::particle_attributes::{
    GpuParticleAttributeSummary, estimate_particle_attributes,
};
use aestra_gpu::shader::{SIMULATION_WESL, SPRITE_RENDER_WESL};
pub use aestra_gpu::{
    GpuArtifactError, GpuCurve, GpuEffectArtifact, GpuEmitter, GpuGlobals, GpuGradient,
    GpuGradientKey, GpuParticle, GpuRenderGlobals, GpuRenderParams, GpuRenderer, MAX_CURVE_KEYS,
    MAX_FLIPBOOK_FRAMES,
};
#[cfg(test)]
use aestra_gpu::{fold_seed, indirect_draw_commands_with_statistics, indirect_draw_offset};
#[cfg(test)]
use aestra_runtime::{PlaybackHistoryPolicy, SeekQuality};
#[cfg(test)]
use std::collections::BTreeMap;

#[cfg(test)]
use bevy::render::render_resource::{
    BindGroup, BindGroupEntries, BindGroupLayout, BindGroupLayoutEntries, Buffer,
    BufferInitDescriptor, BufferUsages, ShaderStages,
    binding_types::{storage_buffer, storage_buffer_read_only},
};
use bevy::{
    asset::io::embedded::EmbeddedAssetRegistry,
    ecs::schedule::IntoScheduleConfigs,
    prelude::*,
    render::{
        Render, RenderApp, RenderStartup, RenderSystems,
        gpu_readback::ReadbackComplete,
        render_asset::RenderAssets,
        renderer::{RenderContext, RenderDevice, RenderGraph, RenderGraphSystems},
        storage::GpuShaderBuffer,
    },
};
#[cfg(test)]
use bevy::{camera::primitives::Aabb, render::storage::ShaderBuffer};
pub use extension_stages::{
    AestraDebugViews, AestraEffectOutputs, AestraFieldView, AestraOutputEvent, AestraWorldSdf,
    GpuStageProgress, GpuStageTiming,
};
pub use physics::{AestraPhysicsColliders, AestraPhysicsQuery, PhysicsPose};

pub use alpha_sort::{AlphaSortSnapshot, GpuAlphaSortStatistics};
pub use output_context::{EffectOutputContext, ParticleOutputContext};
pub use particle_output_readback::{GpuEventLinkCounts, GpuEventLinkStatistics};
pub use particle_statistics::GpuParticleStatistics;
pub use preparation_timing::GpuPreparationTiming;
pub use simulation_timing::{GpuSimulationFrame, GpuSimulationTiming, GpuSimulationWork};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

pub const WESL_SHADER_PATH: &str = simulation_pipeline::WESL_SHADER_PATH;
#[cfg(test)]
use bevy::render::render_resource::ComputePipeline;
#[cfg(test)]
use coupled_simulation::*;
pub(crate) use mapped_readback::with_mapped_range;
pub use render::{WESL_MESH_WIREFRAME_SHADER_PATH, WESL_RENDER_SHADER_PATH};
use simulation_pipeline::{init_pipeline, prepare_bind_groups};
use stateful_simulation::*;
/// The unified stateful simulation module (hybrid roadmap M6): the death_integrate / spawn / present
/// compute pipelines are built from this. Composed from the proven `aestra_gpu` WGSL primitives.
pub const STATEFUL_SIMULATION_SHADER_PATH: &str =
    stateful_simulation::STATEFUL_SIMULATION_SHADER_PATH;

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
    preparation::install(app);
    extraction::install(app);
    extraction::install_device_publication(app);
    visibility_updates::install(app);
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

use simulation_graph::run_simulation;

#[cfg(test)]
mod tests {
    use super::*;

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
