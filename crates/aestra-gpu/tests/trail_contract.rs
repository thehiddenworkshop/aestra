use aestra_compiler::EffectCompiler;
use aestra_core::{
    EffectAsset, EffectPlaybackMode, Emitter, RENDERER_TRAIL, RendererProperties, RendererTypeId,
};
use aestra_gpu::GpuEffectArtifact;
use aestra_runtime::{EffectInstance, RendererCapability};
use std::sync::Arc;

fn fixture() -> EffectAsset {
    let mut effect = EffectAsset::new("Trail contract", 2.0);
    let mut emitter = Emitter::basic_sprite("Trail", 2.0);
    emitter.max_particles = 8;
    emitter.renderers[0].renderer_type = RendererTypeId(RENDERER_TRAIL.into());
    emitter.renderers[0].properties = RendererProperties::Trail {
        width: 1.0,
        sample_interval: 0.025,
        lifetime: 0.5,
        max_points: 32,
        max_trails: 0,
        sampling: aestra_core::TrailSamplingMode::Time,
        sample_distance: 0.1,
        curve_tolerance: 0.01,
        uv_mode: aestra_core::TrailUvMode::Stretch,
        tile_length: 1.0,
        end_cap: aestra_core::TrailEndCap::Flat,
    };
    effect.emitters.push(emitter);
    effect
}

#[test]
fn validates_bounded_history_and_keeps_normal_particle_capacity_separate() {
    let effect = fixture();
    let compiled = Arc::new(EffectCompiler::default().compile(&effect).unwrap());
    assert!(
        compiled
            .requirements
            .renderers
            .contains(&RendererCapability::RibbonParticles)
    );
    let instance = EffectInstance::new(compiled);
    let gpu = GpuEffectArtifact::from_instance(&instance).unwrap();
    assert_eq!(gpu.particles.len(), 8 + 1 + 8 * 32);
    assert_eq!(gpu.total_slots, 8);
    assert_eq!(
        gpu.emitters[0]._turbulence_padding, 0,
        "trail-only emitters must not pay for ribbon linking"
    );
    for invalid in [
        RendererProperties::Trail {
            width: f32::NAN,
            sample_interval: 0.1,
            lifetime: 1.0,
            max_points: 4,
            max_trails: 0,
            sampling: aestra_core::TrailSamplingMode::Time,
            sample_distance: 0.1,
            curve_tolerance: 0.01,
            uv_mode: aestra_core::TrailUvMode::Stretch,
            tile_length: 1.0,
            end_cap: aestra_core::TrailEndCap::Flat,
        },
        RendererProperties::Trail {
            width: 1.0,
            sample_interval: 0.0,
            lifetime: 1.0,
            max_points: 4,
            max_trails: 0,
            sampling: aestra_core::TrailSamplingMode::Time,
            sample_distance: 0.1,
            curve_tolerance: 0.01,
            uv_mode: aestra_core::TrailUvMode::Stretch,
            tile_length: 1.0,
            end_cap: aestra_core::TrailEndCap::Flat,
        },
        RendererProperties::Trail {
            width: 1.0,
            sample_interval: 0.1,
            lifetime: -1.0,
            max_points: 4,
            max_trails: 0,
            sampling: aestra_core::TrailSamplingMode::Time,
            sample_distance: 0.1,
            curve_tolerance: 0.01,
            uv_mode: aestra_core::TrailUvMode::Stretch,
            tile_length: 1.0,
            end_cap: aestra_core::TrailEndCap::Flat,
        },
        RendererProperties::Trail {
            width: 1.0,
            sample_interval: 0.1,
            lifetime: 1.0,
            max_points: 65,
            max_trails: 0,
            sampling: aestra_core::TrailSamplingMode::Time,
            sample_distance: 0.1,
            curve_tolerance: 0.01,
            uv_mode: aestra_core::TrailUvMode::Stretch,
            tile_length: 1.0,
            end_cap: aestra_core::TrailEndCap::Flat,
        },
    ] {
        let mut effect = effect.clone();
        effect.emitters[0].renderers[0].properties = invalid;
        assert!(EffectCompiler::default().compile(&effect).is_err());
    }
    for parents in [257, 800, 1024, 1025, 2053, 8192] {
        let mut supported = effect.clone();
        supported.emitters[0].max_particles = parents;
        let compiled = EffectCompiler::default().compile(&supported).unwrap();
        let gpu =
            GpuEffectArtifact::from_instance(&EffectInstance::new(Arc::new(compiled))).unwrap();
        assert_eq!(gpu.emitters[0].trail_capacity, parents);
        assert_eq!(gpu.particles.len(), (parents + 1 + parents * 32) as usize);
    }
    let mut oversized = effect.clone();
    oversized.emitters[0].max_particles = 32768;
    let compiled = EffectCompiler::default().compile(&oversized).unwrap();
    assert_eq!(
        GpuEffectArtifact::from_instance(&EffectInstance::new(Arc::new(compiled))).err(),
        Some(aestra_gpu::GpuArtifactError::TrailLimit),
    );
    if let RendererProperties::Trail { max_trails, .. } =
        &mut oversized.emitters[0].renderers[0].properties
    {
        *max_trails = u32::MAX;
    }
    let compiled = EffectCompiler::default().compile(&oversized).unwrap();
    assert_eq!(
        GpuEffectArtifact::dynamics_from_instance(&EffectInstance::new(Arc::new(compiled))).err(),
        Some(aestra_gpu::GpuArtifactError::TrailLimit)
    );
    let mut duplicate = effect;
    let mut renderer = duplicate.emitters[0].renderers[0].clone();
    renderer.id = aestra_core::RendererId::new();
    duplicate.emitters[0].renderers.push(renderer);
    assert!(EffectCompiler::default().compile(&duplicate).is_err());
}

#[test]
fn independent_pool_capacity_is_serialized_validated_and_profiled() {
    let mut effect = fixture();
    let legacy = effect
        .to_pretty_ron()
        .unwrap()
        .replace("max_trails: 0,", "")
        .replace("sampling: Time,", "")
        .replace("curve_tolerance: 0.01,", "")
        .replace("sample_distance: 0.1,", "");
    let legacy = EffectAsset::from_ron(&legacy).unwrap();
    assert!(matches!(
        legacy.emitters[0].renderers[0].properties,
        RendererProperties::Trail {
            max_trails: 0,
            sampling: aestra_core::TrailSamplingMode::Time,
            sample_distance: 0.1,
            curve_tolerance: 0.01,
            ..
        }
    ));
    let compiled = EffectCompiler::default().compile(&effect).unwrap();
    let mut profile = aestra_runtime::EffectProfile::from_compiled(&compiled);
    let old_memory = profile.buffer_memory_bytes.value().unwrap();
    assert_eq!(profile.trail_capacity.value(), Some(8));
    assert_eq!(
        profile.occupied_trails,
        aestra_runtime::ProfileValue::Unavailable
    );
    if let RendererProperties::Trail { max_trails, .. } =
        &mut effect.emitters[0].renderers[0].properties
    {
        *max_trails = 24;
    }
    let compiled = Arc::new(EffectCompiler::default().compile(&effect).unwrap());
    assert!(!profile.matches_compiled(&compiled));
    profile = aestra_runtime::EffectProfile::from_compiled(&compiled);
    assert_eq!(profile.trail_capacity.value(), Some(24));
    assert_eq!(
        profile.buffer_memory_bytes.value().unwrap() - old_memory,
        16 * (32 * 64 + 8 * 31 + 4)
    );
    profile.record_trail_usage(Some(aestra_runtime::TrailUsage {
        occupied: 20,
        retired: 12,
        evictions: 3,
        truncated: 2,
    }));
    assert_eq!(
        profile.truncated_trails,
        aestra_runtime::ProfileValue::Measured(2)
    );
    assert_eq!(
        profile.retired_trails,
        aestra_runtime::ProfileValue::Measured(12)
    );
    profile.record_trail_usage(None);
    assert_eq!(
        profile.truncated_trails,
        aestra_runtime::ProfileValue::Unavailable
    );
    assert_eq!(
        profile.trail_evictions,
        aestra_runtime::ProfileValue::Unavailable
    );
    let gpu = GpuEffectArtifact::from_instance(&EffectInstance::new(compiled)).unwrap();
    assert_eq!(gpu.total_slots, 8);
    assert_eq!(gpu.particles.len(), 8 + 1 + 24 * 32);
    assert_eq!(gpu.emitters[0].trail_capacity, 24);
    assert_eq!(gpu.renderers[0].playback_mode, 24);
    if let RendererProperties::Trail { max_trails, .. } =
        &mut effect.emitters[0].renderers[0].properties
    {
        *max_trails = 7;
    }
    assert!(EffectCompiler::default().compile(&effect).is_err());
    if let RendererProperties::Trail { max_trails, .. } =
        &mut effect.emitters[0].renderers[0].properties
    {
        *max_trails = 1025;
    }
    let compiled = EffectCompiler::default().compile(&effect).unwrap();
    let large_profile = aestra_runtime::EffectProfile::from_compiled(&compiled);
    assert_eq!(
        large_profile.buffer_memory_bytes.value().unwrap() - old_memory,
        1017 * (32 * 64 + 8 * 31 + 4) + 4 + 4 * (3 * 8 + 2 * 2048 + 1 + 12 * 2)
    );
}

#[test]
fn distance_sampling_validates_and_lowers_without_increasing_storage() {
    let mut effect = fixture();
    let expected = 8 + 1 + 8 * 32;
    for distance in [0.001, 0.25, 100.0, 0.0, -1.0, f32::NAN, f32::INFINITY] {
        if let RendererProperties::Trail {
            sampling,
            sample_distance,
            ..
        } = &mut effect.emitters[0].renderers[0].properties
        {
            *sampling = aestra_core::TrailSamplingMode::Distance;
            *sample_distance = distance;
        }
        let compiled = EffectCompiler::default().compile(&effect);
        if distance.is_finite() && distance >= 0.001 {
            let gpu =
                GpuEffectArtifact::from_instance(&EffectInstance::new(Arc::new(compiled.unwrap())))
                    .unwrap();
            assert_eq!(gpu.emitters[0].trail_sampling, 1);
            assert_eq!(gpu.emitters[0].trail_distance, distance);
            assert_eq!(gpu.particles.len(), expected);
        } else {
            assert!(compiled.is_err());
        }
    }
}

#[test]
fn adaptive_sampling_round_trips_validates_tolerance_and_reuses_emitter_padding() {
    for tolerance in [0.001, 0.05, 10.0, 0.0, -1.0, f32::NAN, f32::INFINITY] {
        let mut effect = fixture();
        if let RendererProperties::Trail {
            sampling,
            curve_tolerance,
            sample_distance,
            ..
        } = &mut effect.emitters[0].renderers[0].properties
        {
            *sampling = aestra_core::TrailSamplingMode::Adaptive;
            *curve_tolerance = tolerance;
            *sample_distance = 2.0;
        }
        let compiled = EffectCompiler::default().compile(&effect);
        if tolerance.is_finite() && tolerance >= 0.001 {
            assert_eq!(
                EffectAsset::from_ron(&effect.to_pretty_ron().unwrap()).unwrap(),
                effect
            );
            let gpu =
                GpuEffectArtifact::from_instance(&EffectInstance::new(Arc::new(compiled.unwrap())))
                    .unwrap();
            assert_eq!(gpu.emitters[0].trail_sampling, 2);
            assert_eq!(gpu.emitters[0].trail_tolerance, tolerance);
            assert_eq!(gpu.emitters[0].trail_distance, 2.0);
            assert_eq!(gpu.particles.len(), 8 + 1 + 8 * 32);
        } else {
            assert!(compiled.is_err());
        }
    }
}

#[test]
fn uv_modes_round_trip_validate_and_use_existing_renderer_lanes() {
    for mode in [
        aestra_core::TrailUvMode::Stretch,
        aestra_core::TrailUvMode::Tile,
    ] {
        for length in [0.001, 8.0, 0.0, -1.0, f32::NAN, f32::INFINITY] {
            let mut effect = fixture();
            if let RendererProperties::Trail {
                uv_mode,
                tile_length,
                ..
            } = &mut effect.emitters[0].renderers[0].properties
            {
                *uv_mode = mode;
                *tile_length = length;
            }
            let compiled = EffectCompiler::default().compile(&effect);
            if !length.is_finite() || length < 0.001 {
                assert!(compiled.is_err());
                continue;
            }
            let restored = EffectAsset::from_ron(&effect.to_pretty_ron().unwrap()).unwrap();
            assert_eq!(effect, restored);
            let mut compiled = compiled.unwrap();
            let gpu =
                GpuEffectArtifact::from_instance(&EffectInstance::new(Arc::new(compiled.clone())))
                    .unwrap();
            assert_eq!(
                gpu.renderers[0].flipbook_flags,
                u32::from(mode == aestra_core::TrailUvMode::Tile)
            );
            assert_eq!(gpu.renderers[0].frames[0].x, length);
            assert_eq!(gpu.particles.len(), 8 + 1 + 8 * 32);
            if let aestra_runtime::RendererPlanKind::Trail { tile_length, .. } =
                &mut compiled.emitters[0].renderers[0].kind
            {
                *tile_length = f32::NAN;
            }
            assert!(
                GpuEffectArtifact::from_instance(&EffectInstance::new(Arc::new(compiled))).is_err()
            );
        }
    }
    let legacy = fixture()
        .to_pretty_ron()
        .unwrap()
        .replace("uv_mode: Stretch,", "")
        .replace("tile_length: 1.0,", "")
        .replace("end_cap: Flat,", "");
    let restored = EffectAsset::from_ron(&legacy).unwrap();
    assert!(matches!(
        restored.emitters[0].renderers[0].properties,
        RendererProperties::Trail {
            uv_mode: aestra_core::TrailUvMode::Stretch,
            tile_length: 1.0,
            end_cap: aestra_core::TrailEndCap::Flat,
            ..
        }
    ));
}

#[test]
fn history_epoch_distinguishes_clock_updates_from_explicit_seeks_and_restarts() {
    let mut effect = fixture();
    effect.playback_mode = EffectPlaybackMode::LoopContinuous;
    let mut instance = EffectInstance::new(Arc::new(
        EffectCompiler::default().compile(&effect).unwrap(),
    ));
    let initial = instance.history_epoch();
    instance.set_playback_time(1.5);
    instance.advance(1.0);
    instance.advance_with_choreography_events(2.0, &mut Vec::new());
    assert_eq!(
        instance.history_epoch(),
        initial,
        "continuous wraps preserve history"
    );
    instance.seek(4.75);
    assert_ne!(instance.history_epoch(), initial);
    let after_seek = instance.history_epoch();
    instance.set_seed(0);
    assert_eq!(instance.history_epoch(), after_seek);
    instance.set_seed(1);
    assert_ne!(instance.history_epoch(), after_seek);
    let seeded = instance.history_epoch();
    instance.restart();
    assert_ne!(instance.history_epoch(), seeded);
    effect.playback_mode = EffectPlaybackMode::LoopRestart;
    let mut instance = EffectInstance::new(Arc::new(
        EffectCompiler::default().compile(&effect).unwrap(),
    ));
    let initial = instance.history_epoch();
    instance.advance_with_choreography_events(4.0, &mut Vec::new());
    assert_ne!(
        instance.history_epoch(),
        initial,
        "whole-cycle advances must also reset"
    );
}

#[test]
fn end_caps_round_trip_and_add_draw_primitives_without_spending_history() {
    for cap in [
        aestra_core::TrailEndCap::Flat,
        aestra_core::TrailEndCap::Rounded,
    ] {
        for mode in [
            aestra_core::TrailUvMode::Stretch,
            aestra_core::TrailUvMode::Tile,
        ] {
            let mut effect = fixture();
            if let RendererProperties::Trail {
                end_cap, uv_mode, ..
            } = &mut effect.emitters[0].renderers[0].properties
            {
                *end_cap = cap;
                *uv_mode = mode;
            }
            let decoded = EffectAsset::from_ron(&effect.to_pretty_ron().unwrap()).unwrap();
            assert_eq!(decoded, effect);
            let compiled = EffectCompiler::default().compile(&decoded).unwrap();
            let gpu =
                GpuEffectArtifact::from_instance(&EffectInstance::new(Arc::new(compiled))).unwrap();
            let r = &gpu.renderers[0];
            assert_eq!(
                r.flipbook_flags & 1,
                u32::from(mode == aestra_core::TrailUvMode::Tile)
            );
            assert_eq!(
                r.flipbook_flags & 2 != 0,
                cap == aestra_core::TrailEndCap::Rounded
            );
            assert_eq!(gpu.particles.len(), 8 + 1 + 8 * 32);
            let caps = if cap == aestra_core::TrailEndCap::Rounded {
                16
            } else {
                0
            };
            assert_eq!(aestra_gpu::trail_draw_instances(r), 8 * (31 + caps));
        }
    }
}
