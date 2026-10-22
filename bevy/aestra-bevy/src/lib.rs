//! Bevy integration and reference playback runtime for compiled Aestra effects.
//!
//! This crate is the **canonical client** for Aestra. If you are embedding
//! Aestra in an application — or writing an integration for another engine such
//! as `aestra-godot` or `aestra-unity` — this is the one place to look: the ECS
//! plumbing differs per host, but the pipeline below is identical everywhere.
//!
//! # The client pipeline
//!
//! 1. Load an authored [`EffectAsset`].
//! 2. Resolve it against a project asset root (child clips + material programs)
//!    with `aestra_project::ProjectAssetIndex`, producing a resolved project.
//! 3. Compile the resolved project into a [`CompiledEffectProject`] with
//!    [`EffectCompiler`]. In a shipping game this is done offline.
//! 4. Spawn an [`EffectPlayer`] for the artifact and add [`AestraPlugin`]. The
//!    plugin owns simulation advance, choreography dispatch, and rendering —
//!    hosts never touch the render backend (`aestra-bevy-render`) directly.
//!
//! Everything a host drives at runtime — play/pause, [`EffectPlayer::seek`],
//! [`EffectPlayer::set_parameter`], choreography events — goes through the
//! `EffectPlayer` handle. Live objects an effect follows (a target, a weapon tip) are bound
//! with [`AestraBindings`] on the player entity; see `examples/live_target_binding.rs`.
//!
//! # Minimal host
//!
//! ```rust,no_run
//! use aestra_bevy::AestraPlugin;
//! use bevy::prelude::*;
//!
//! let mut app = App::new();
//! app.add_plugins((DefaultPlugins, AestraPlugin));
//! // Spawn an `EffectPlayer::from_project(..)` in a startup system, then `app.run()`.
//! ```
//!
//! See `examples/minimal_player.rs` for the full, runnable load-and-play path
//! (`cargo run -p aestra-bevy --example minimal_player`). For a demanding
//! real-world host, `apps/aestra-viewer` drives the same API with capture,
//! diagnostics, and GPU benchmarking layered on top.
mod bindings;
#[cfg(test)]
mod choreography_tests;
mod input_events;
mod lighting_quality;
mod lights;
#[path = "particle_light_boundary_020.rs"]
mod particle_light_boundary;
mod particle_light_gpu;
mod particle_lights;
use particle_lights as particle_light_realization;
mod playback;
#[cfg(test)]
use playback::dispatch_choreography_events;
#[cfg(test)]
use playback::sync_player_presentations;
pub use playback::{AestraChoreographyEvent, AestraSet, EffectPlayer};
mod project;
mod project_profile;
mod world;
pub use aestra_runtime::{PointLightPulse, PointLightSample, TransientPointLight};
pub use bindings::{
    AestraBindingRecorder, AestraBindingTrace, AestraBindings, AestraLinearVelocity, binding_frame,
    spatial_snapshot,
};
pub use input_events::AestraEventInput;
pub use lighting_quality::{LightQualityBudget, LightingQualityError, LightingQualityPolicy};
pub use lights::{
    AestraLightOutput, AestraTransientLightPlugin, MAX_TRANSIENT_LIGHTS, TransientLightKey,
    TransientLightProxy, TransientLightSettings, TransientLightStatistics,
};
pub use particle_light_gpu::{
    ParticleLightGpuObservation, ParticleLightGpuRejection, ParticleLightGpuSettings,
    ParticleLightGpuSlot, ParticleLightGpuStatistics,
};
pub use particle_lights::{
    AestraParticleLightPlugin, AestraParticleLightSettings, ParticleLightMode, ParticleLightProxy,
    ParticleLightReadbackSettings, ParticleLightRealizationSettings, ParticleLightStatistics,
};
pub use project::EffectClipInstance;
pub use project_profile::ProjectProfiler;
pub use world::sdf_from_meshes;

pub use aestra_bevy_render::material::{
    MaterialBindingContext, MaterialRuntimeBinding, compile_material_program,
};
pub use aestra_bevy_render::preview;
pub use aestra_bevy_render::sampling::{SpriteSampling, TrailRasterSampling};
pub use aestra_bevy_render::{
    ActiveBackend, AestraRenderPlugin, AestraRenderSet, AestraRenderSettings as AestraSettings,
    AestraRuntimeStatus, BackendCapabilities, CompatibilityIssue, CompatibilityIssueCode,
    CompatibilityReport, CompatibilityTarget, DEFAULT_GPU_PARTICLE_BUDGET, EffectRenderMode,
    EffectRequirements, EffectRuntimeStatus, GpuCapabilities, PresentationMode, PresentedEffect,
    RendererCapability, TransparentOrderMode, gpu,
};
// The host's world and what effects report back to gameplay (fluid F11, host bindings HB9/HB10).
pub use aestra_bevy_render::gpu::{
    AestraEffectOutputs, AestraOutputEvent, AestraPhysicsColliders, AestraPhysicsQuery,
    AestraWorldSdf, EffectOutputContext, ParticleOutputContext, PhysicsPose,
};
pub use aestra_compiler::{
    AestraExtension, CompileError, EffectCompiler, ExtensionManifest, ExtensionRegistry,
    ModuleRegistry, ProjectCompileError, link_extension,
};
pub use aestra_core::*;
pub use aestra_runtime::{
    BindingError, BindingFrame, BindingSlot, BindingSnapshot, BindingState, BindingStatus,
    HostRequirements, SpatialBindingSnapshot,
};
pub use aestra_runtime::{BindingRecorder, BindingTrace, trace_tick};
pub use aestra_runtime::{
    CheckpointBackendId, CheckpointContext, CheckpointPolicy, CheckpointStore, ClockAdvance,
    CompiledEffect, CompiledEffectProject, DEFAULT_PLAYBACK_TICK_RATE, DispatchedChoreographyEvent,
    EffectInstance, EffectProfile, EmitterProfile, ParameterError, ParticleSample,
    PlaybackCheckpoint, PlaybackClock, PlaybackDriver, PlaybackHistoryPolicy, ProfileValue,
    ProfileValueSource, ProjectChoreographyEvent, ProjectInstanceProfile, ProjectProfile,
    RendererPlanKind, RuntimeValue, SeekOrigin, SeekPlan, SeekQuality, SimulationSeekMode,
};
pub use aestra_runtime::{
    EVENT_FINISHED, EVENT_IMPACT, EVENT_TARGET_ACQUIRED, EVENT_TARGET_LOST, EffectOutputEvent,
    EventOrigin, QualityTier, SdfVolume, StageOutputValue,
};
pub use aestra_runtime::{EventInputError, HostInputEvent, INPUT_RESTART, neutral_payload};
pub use aestra_runtime::{MAX_PHYSICS_PROXIES, PhysicsProxy, PhysicsScene};

#[cfg(test)]
use bevy::prelude::{App, Update};
#[cfg(test)]
use std::sync::Arc;

mod host_plugin;
pub use host_plugin::{AestraPlugin, EffectProfiler};
#[cfg(test)]
use host_plugin::{prepare_effect_profiles, record_presented_profile};
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_motion_reaches_presentation_and_replacement_resets_history() {
        for mode in [
            EffectPlaybackMode::Once,
            EffectPlaybackMode::LoopRestart,
            EffectPlaybackMode::LoopContinuous,
        ] {
            let mut effect = EffectAsset::new("Motion", 2.0);
            effect.playback_mode = mode;
            effect.emitters.push(Emitter::basic_sprite("Sparks", 2.0));
            effect.host_transform_track = Some(aestra_core::HostTransformTrack::from_pose_keys(
                vec![
                    aestra_core::HostTransformKey {
                        time: 0.0,
                        transform: Default::default(),
                    },
                    aestra_core::HostTransformKey {
                        time: 4.0,
                        transform: aestra_core::EmitterTransform {
                            translation: [40.0, 0.0, 0.0],
                            ..Default::default()
                        },
                    },
                ],
                false,
            ));
            let mut player = EffectPlayer::new(&effect);
            player.seek_simulation_time(3.0);
            let expected_time = player.simulation_time();
            assert_eq!(
                player
                    .instance()
                    .host_transform_at(expected_time)
                    .translation[0],
                expected_time * 10.0
            );
            let mut app = App::new();
            app.add_systems(Update, sync_player_presentations);
            let entity = app
                .world_mut()
                .spawn((PresentedEffect::new(player.effect().clone()), player))
                .id();
            app.update();
            let presented = app.world().get::<PresentedEffect>(entity).unwrap();
            assert_eq!(presented.simulation_time(), expected_time);
            assert!(presented.instance.host_transform_track().is_some());
            let revision = presented.instance.history_revision();
            app.world_mut()
                .get_mut::<EffectPlayer>(entity)
                .unwrap()
                .set_host_transform_track(None);
            app.update();
            let presented = app.world().get::<PresentedEffect>(entity).unwrap();
            assert_eq!(presented.instance.history_revision(), revision + 1);
            assert!(presented.instance.host_transform_track().is_none());
            let inherited = Arc::new(aestra_runtime::InheritedHostTransform::default().for_child(
                None,
                aestra_core::EmitterTransform {
                    translation: [5.0, 2.0, 1.0],
                    ..Default::default()
                },
                0.25,
            ));
            app.world_mut()
                .get_mut::<EffectPlayer>(entity)
                .unwrap()
                .set_inherited_host_transform(inherited.clone());
            app.update();
            let presented = app.world().get::<PresentedEffect>(entity).unwrap();
            assert_eq!(presented.instance.history_revision(), revision + 2);
            assert_eq!(
                presented.instance.host_transform_context().inherited,
                inherited
            );
        }
    }

    #[test]
    fn player_compiles_authored_effect() {
        let mut effect = EffectAsset::new("Test", 2.0);
        effect.emitters.push(Emitter::basic_sprite("Sparks", 2.0));
        let player = EffectPlayer::try_new(&effect).unwrap();
        assert_eq!(player.effect().source, effect.id);
        assert_eq!(player.effect().emitters.len(), 1);
    }

    #[test]
    fn player_accepts_a_reloaded_compiled_artifact() {
        let mut effect = EffectAsset::new("Artifact playback", 2.0);
        effect.emitters.push(Emitter::basic_sprite("Sparks", 2.0));
        let compiled = EffectCompiler::default().compile(&effect).unwrap();
        let bytes = aestra_artifact::encode_effect(&compiled).unwrap();
        let reloaded = aestra_artifact::decode_effect(&bytes).unwrap();
        let mut player = EffectPlayer::from_compiled(Arc::new(reloaded));

        player.seek(0.75);
        let mut samples = Vec::new();
        player.instance().evaluate(&mut samples);
        assert_eq!(player.effect().source, effect.id);
        assert!(!samples.is_empty());
    }

    #[test]
    fn player_render_mode_can_switch_to_wireframe() {
        let mut effect = EffectAsset::new("Wireframe", 2.0);
        effect.emitters.push(Emitter::basic_sprite("Emitter", 2.0));
        let mut player = EffectPlayer::new(&effect);

        assert_eq!(player.render_mode(), EffectRenderMode::Rendered);
        player.set_render_mode(EffectRenderMode::Wireframe);
        assert_eq!(player.render_mode(), EffectRenderMode::Wireframe);
    }

    #[test]
    fn automatic_presentation_is_the_default() {
        assert_eq!(
            AestraSettings::default().presentation,
            PresentationMode::Auto
        );
    }

    #[test]
    fn players_receive_a_machine_readable_profile() {
        let mut effect = EffectAsset::new("Profiled", 2.0);
        let mut emitter = Emitter::basic_sprite("Emitter", 2.0);
        let material = MaterialDefinition::sprite("Alpha", BlendMode::Alpha, 0.2);
        let material_id = material.id;
        effect.materials.push(material);
        emitter
            .renderers
            .push(RendererInstance::sprite(material_id));
        effect.emitters.push(emitter);
        let mut app = App::new();
        app.insert_resource(GpuCapabilities::default())
            .add_systems(Update, prepare_effect_profiles);
        let entity = app
            .world_mut()
            .spawn((
                EffectPlayer::new(&effect),
                EffectRuntimeStatus {
                    active: ActiveBackend::CpuReference,
                    reason: "CPU reference requested".into(),
                    compatibility: CompatibilityReport::compatible(
                        CompatibilityTarget::CpuReference,
                    ),
                },
            ))
            .id();

        app.update();

        let profile = &app.world().get::<EffectProfiler>(entity).unwrap().0;
        assert_eq!(profile.emitter_count, ProfileValue::Measured(1));
        assert_eq!(profile.gpu_time_ns, ProfileValue::Unavailable);
        assert_eq!(profile.submitted_instances, ProfileValue::Unavailable);
        assert!(matches!(
            profile.buffer_memory_bytes,
            ProfileValue::Estimated(bytes) if bytes > 0
        ));
        assert_eq!(profile.draw_calls, ProfileValue::Estimated(2));
        assert!(profile.platform_warnings.is_empty());
    }

    #[test]
    fn native_gpu_profile_does_not_report_empty_readback_as_measured_zero() {
        let mut effect = EffectAsset::new("Native GPU profile", 2.0);
        effect.emitters.push(Emitter::basic_sprite("Emitter", 2.0));
        let compiled = EffectCompiler::default().compile(&effect).unwrap();
        let mut profile = EffectProfile::from_compiled(&compiled);

        record_presented_profile(&mut profile, &compiled, &[], None, ActiveBackend::Gpu);

        assert_eq!(profile.alive_particles, ProfileValue::Unavailable);
        assert_eq!(profile.submitted_instances, ProfileValue::Unavailable);
        assert_eq!(profile.peak_particles, ProfileValue::Unavailable);
        assert!(profile.record_particle_counts(&[7]));
        record_presented_profile(&mut profile, &compiled, &[], None, ActiveBackend::Gpu);
        assert_eq!(profile.alive_particles, ProfileValue::Unavailable);
        assert_eq!(profile.emitters[0].alive_particles, 0);
        assert_eq!(profile.emitters[0].peak_particles, 7);
    }

    #[test]
    fn player_forwards_runtime_parameter_overrides() {
        let mut effect = EffectAsset::new("Parameterized", 2.0);
        let parameter = EffectParameter {
            id: ParameterId::new(),
            name: "Spawn Rate".into(),
            default: Value::Scalar(4.0),
            exposed: true,
        };
        let parameter_id = parameter.id;
        let mut emitter = Emitter::basic_sprite("Emitter", 2.0);
        emitter
            .modules
            .iter_mut()
            .find(|module| module.module_type.0 == MODULE_EMISSION)
            .unwrap()
            .bindings
            .insert("spawn_rate".into(), parameter_id);
        effect.parameters.push(parameter);
        effect.emitters.push(emitter);

        let mut player = EffectPlayer::try_new(&effect).unwrap();
        player
            .set_parameter(parameter_id, Value::Scalar(40.0))
            .unwrap();
        assert!(matches!(
            player.instance().parameter(parameter_id),
            Some(RuntimeValue::Scalar(40.0))
        ));
    }

    #[test]
    fn analytic_forward_clock_does_not_invalidate_history() {
        for mode in [EffectPlaybackMode::Once, EffectPlaybackMode::LoopContinuous] {
            for policy in [
                PlaybackHistoryPolicy::PlaybackOnly,
                PlaybackHistoryPolicy::ReplayEnabled,
            ] {
                let mut effect = EffectAsset::new("Forward history", 30.0);
                effect.playback_mode = mode;
                effect.emitters.push(Emitter::basic_sprite("Emitter", 30.0));
                let mut player = EffectPlayer::new(&effect).with_history_policy(policy);
                assert_eq!(player.seek_mode(), SimulationSeekMode::StatelessDirect);
                let epoch = player.instance().history_epoch();
                for frame in 1..=1800 {
                    player.advance_clock(1.0 / 60.0);
                    assert_eq!(
                        player.instance().history_epoch(),
                        epoch,
                        "forward frame {frame}, {mode:?}, {policy:?}"
                    );
                    assert_eq!(player.simulation_time(), frame as f32 / 60.0);
                }
            }
        }
    }

    #[test]
    fn fixed_step_players_match_across_render_delta_sequences() {
        let mut effect = EffectAsset::new("Deterministic", 2.0);
        effect.emitters.push(Emitter::basic_sprite("Emitter", 2.0));
        let mut fine = EffectPlayer::new(&effect);
        let mut coarse = EffectPlayer::new(&effect);
        fine.set_seed(42);
        coarse.set_seed(42);

        for _ in 0..120 {
            fine.advance_clock(1.0 / 120.0);
        }
        for _ in 0..10 {
            coarse.advance_clock(0.1);
        }

        assert_eq!(fine.frame(), 60);
        assert_eq!(fine.frame(), coarse.frame());
        assert_eq!(fine.elapsed(), coarse.elapsed());
        let mut fine_samples = Vec::new();
        let mut coarse_samples = Vec::new();
        fine.instance().evaluate(&mut fine_samples);
        coarse.instance().evaluate(&mut coarse_samples);
        assert_eq!(fine_samples, coarse_samples);
    }

    #[test]
    fn players_surface_choreography_events_crossed_by_fixed_step_playback() {
        let mut effect = EffectAsset::new("Choreography", 2.0);
        effect.choreography_events = vec![
            ChoreographyEvent::new(
                "Begin",
                0.0,
                ChoreographyEventPayload::GameplayNotify {
                    topic: "begin".into(),
                },
            ),
            ChoreographyEvent::new(
                "Impact",
                0.5,
                ChoreographyEventPayload::PlaySound {
                    cue: "impact".into(),
                },
            ),
        ];
        let mut player = EffectPlayer::new(&effect);

        player.advance_clock(0.75);
        assert_eq!(
            player
                .drain_choreography_events()
                .map(|event| event.name)
                .collect::<Vec<_>>(),
            ["Begin", "Impact"]
        );
    }

    #[test]
    fn frame_controls_pause_and_reproduce_exact_time() {
        let mut effect = EffectAsset::new("Frame Controls", 2.0);
        effect.emitters.push(Emitter::basic_sprite("Emitter", 2.0));
        let mut player = EffectPlayer::new(&effect);
        player.seek_frame(30);
        assert_eq!(player.elapsed(), 0.5);
        player.step_forward();
        assert_eq!(player.frame(), 31);
        assert!(!player.playing);
        let checkpoint = player.checkpoint();
        player.step_back();
        assert_eq!(player.frame(), 30);
        player.restore_checkpoint(checkpoint);
        assert_eq!(player.frame(), 31);
        player.restart();
        assert_eq!(player.frame(), 0);
        assert_eq!(player.elapsed(), 0.0);
    }

    #[test]
    fn snapshotless_stateful_player_restarts_and_replays_backward_seeks() {
        let mut effect = EffectAsset::new("Stateful Fallback", 2.0);
        effect.emitters.push(Emitter::basic_sprite("Emitter", 2.0));
        let mut compiled = EffectCompiler::default().compile(&effect).unwrap();
        compiled.seek_mode = SimulationSeekMode::RestartReplay;
        let mut player = EffectPlayer::from_compiled(Arc::new(compiled));

        player.seek_frame(60);
        assert_eq!(player.elapsed(), 1.0);
        player.seek_frame(30);
        assert_eq!(player.frame(), 30);
        assert_eq!(player.elapsed(), 0.5);
    }

    #[test]
    fn seeks_restore_simulation_but_never_a_past_delivery_epoch() {
        for mode in [
            SimulationSeekMode::StatelessDirect,
            SimulationSeekMode::RestartReplay,
            SimulationSeekMode::CheckpointRestore,
        ] {
            let mut effect = EffectAsset::new("Cue epochs", 2.0);
            effect.emitters.push(Emitter::basic_sprite("Emitter", 2.0));
            let mut compiled = EffectCompiler::default().compile(&effect).unwrap();
            compiled.seek_mode = mode;
            let mut player = EffectPlayer::from_compiled(Arc::new(compiled));
            player.enable_scrub_cache(aestra_runtime::CheckpointPolicy {
                cadence_frames: 1,
                ..Default::default()
            });
            player.advance_clock(1.0);
            let epoch = player.instance().history_epoch();
            player.seek_frame(30);
            assert!(player.instance().history_epoch() > epoch, "{mode:?}");
            assert!((player.instance().history_epoch_start_time() - 0.5).abs() < 0.00001);
            let epoch = player.instance().history_epoch();
            player.seek_frame(30);
            assert!(player.instance().history_epoch() > epoch, "{mode:?}");
            assert!((player.instance().history_epoch_start_time() - 0.5).abs() < 0.00001);
            player.seek_simulation_time(0.75);
            assert!((player.instance().history_epoch_start_time() - 0.75).abs() < 0.00001);
            player.restart();
            assert_eq!(player.instance().history_epoch_start_time(), 0.0);
        }
    }
}
