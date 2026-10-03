//! F6A ordinary reusable clip composition, with no fireworks-specific runtime policy.
use aestra_bevy::EffectAsset;
use bevy::prelude::*;

pub const BENCH_FRAMES: usize = 1680; // 120 warm-up + 1680 measured = 30 seconds at 60 Hz.

pub fn effect() -> EffectAsset {
    EffectAsset::from_ron(include_str!(
        "../../../assets/test/effects/fireworks_show.aestra.ron"
    ))
    .expect("checked-in reusable fireworks show must parse")
}

pub fn camera(view: crate::FireworksCamera) -> Transform {
    let (eye, target) = match view {
        crate::FireworksCamera::Close => (Vec3::new(0.0, 42.0, 145.0), Vec3::new(0.0, 24.0, 0.0)),
        crate::FireworksCamera::Audience => {
            (Vec3::new(0.0, 48.0, 180.0), Vec3::new(0.0, 28.0, 0.0))
        }
        crate::FireworksCamera::Wide => (Vec3::new(0.0, 60.0, 235.0), Vec3::new(0.0, 26.0, 0.0)),
    };
    Transform::from_translation(eye).looking_at(target, Vec3::Y)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aestra_bevy::{
        EffectCompiler, EffectPlaybackMode, PlaybackHistoryPolicy, QualityTier, Value,
    };
    use std::{collections::BTreeSet, sync::Arc};

    #[test]
    fn show_is_saved_reusable_and_has_no_host_timers_or_preroll() {
        let source = effect();
        assert_eq!(source.duration, 26.0);
        assert_eq!(source.playback_mode, EffectPlaybackMode::Once);
        assert!(source.emitters.is_empty());
        assert_eq!(source.effect_clips.len(), 13);
        assert_eq!(
            source
                .effect_clips
                .iter()
                .map(|c| c.source.id)
                .collect::<BTreeSet<_>>()
                .len(),
            4
        );
        assert_eq!(
            source
                .effect_clips
                .iter()
                .map(|c| c.id)
                .collect::<BTreeSet<_>>()
                .len(),
            13
        );
        assert!(
            source
                .effect_clips
                .iter()
                .all(|c| c.source_offset == 0.0 && c.duration == 7.0 && c.transform.is_valid())
        );
        assert_eq!(
            source
                .effect_clips
                .iter()
                .map(|c| c.start_time + c.duration)
                .fold(0.0_f32, f32::max),
            24.0
        );
        assert_eq!(
            EffectAsset::from_ron(&source.to_pretty_ron().unwrap()).unwrap(),
            source
        );
    }

    #[test]
    fn each_tier_resolves_shared_sources_and_preserves_independent_seeds_and_overrides() {
        let source = effect();
        let index = aestra_project::ProjectAssetIndex::scan(crate::viewer_asset_root(None));
        let resolved = index.resolve_effect_project(&source).unwrap();
        let mut previous_capacity = u64::MAX;
        for (tier, expected_peak) in QualityTier::presets().into_iter().zip([4460, 1980, 964]) {
            let project = EffectCompiler::default()
                .with_tier(tier)
                .compile_resolved_project(&resolved)
                .unwrap();
            assert_eq!(project.dependencies.len(), 4);
            assert_eq!(project.root.max_particles, 0);
            // Distance sampling uses transformed world travel, not unscaled
            // emitter speed. Lock headroom for the show's largest clip scale.
            for child in resolved.dependencies.values() {
                let rocket = &child.emitters[0];
                let speed = rocket
                    .modules
                    .iter()
                    .find_map(|module| {
                        if let aestra_bevy::ModuleParameters::Initialize { speed, .. } =
                            module.parameters
                        {
                            Some(speed.max)
                        } else {
                            None
                        }
                    })
                    .unwrap();
                for renderer in &rocket.renderers {
                    if let aestra_bevy::RendererProperties::Trail {
                        max_points,
                        lifetime,
                        sample_distance,
                        ..
                    } = renderer.properties
                    {
                        assert!(
                            max_points as f32
                                >= (1.2 * speed * lifetime / sample_distance).ceil() + 2.0
                        );
                    }
                }
            }
            let mut paths = BTreeSet::new();
            let mut seeds = BTreeSet::new();
            let mut peak_count = 0;
            let mut peak_capacity = 0;
            // Includes exact start/end boundaries and the silent cleanup tail.
            for frame in 0..=1800 {
                let time = frame as f32 / 60.0;
                let instances = project.instances(time, crate::fireworks_f0::SEED);
                let mut capacity = 0;
                for scheduled in instances.iter().filter(|i| !i.path.is_empty()) {
                    assert_eq!(scheduled.path.len(), 1);
                    paths.insert(scheduled.path.clone());
                    seeds.insert(scheduled.seed);
                    assert!(Arc::ptr_eq(
                        &scheduled.effect,
                        project.dependencies.get(&scheduled.effect.source).unwrap()
                    ));
                    let clip = source
                        .effect_clips
                        .iter()
                        .find(|c| c.id == scheduled.path[0])
                        .unwrap();
                    assert_eq!(
                        scheduled.seed,
                        clip.seed.resolve(crate::fireworks_f0::SEED, clip.id)
                    );
                    assert_ne!(scheduled.seed, clip.seed.resolve(42, clip.id));
                    assert!((scheduled.time - (time - clip.start_time)).abs() < 0.001);
                    assert_eq!(scheduled.parameter_overrides.len(), 1);
                    let (parameter, value) = clip.parameter_overrides.iter().next().unwrap();
                    let packed = &scheduled.parameter_overrides[0];
                    assert_eq!(packed.source, *parameter);
                    assert_eq!(packed.slot, scheduled.effect.parameter_slots[parameter]);
                    assert_eq!(
                        packed.value,
                        aestra_bevy::RuntimeValue::compile(value).unwrap()
                    );
                    assert!(matches!(
                        clip.parameter_overrides.values().next(),
                        Some(Value::Gradient(_))
                    ));
                    capacity += scheduled.effect.max_particles as u64;
                }
                peak_count = peak_count.max(instances.len() - 1);
                peak_capacity = peak_capacity.max(capacity);
                if time > 24.0 {
                    assert_eq!(instances.len(), 1);
                }
            }
            assert_eq!(paths.len(), 13);
            assert_eq!(seeds.len(), 13);
            assert_eq!(peak_count, 6); // Active clip pools, not six simultaneous bursts.
            assert_eq!(peak_capacity, expected_peak);
            assert!(peak_capacity < previous_capacity);
            previous_capacity = peak_capacity;
            for child in project.dependencies.values() {
                let instance = aestra_bevy::EffectInstance::with_seed(child.clone(), 42)
                    .with_history_policy(PlaybackHistoryPolicy::PlaybackOnly);
                aestra_gpu::GpuEffectArtifact::from_instance(&instance).unwrap();
            }
        }
    }

    #[test]
    fn cli_uses_a_full_forward_show_window_only_for_benchmarking() {
        let mut config = crate::ViewerConfig::from_iter(
            [
                "--fireworks-f0",
                "--fireworks-f0-probe",
                "f6-show",
                "--semantic-materials",
                "--backend",
                "gpu",
                "--history",
                "playback-only",
            ]
            .into_iter()
            .map(str::to_owned),
        )
        .unwrap();
        let prepared = crate::prepare_viewer(&config).unwrap_or_else(|e| panic!("{}", e.message));
        assert_eq!(prepared.compiled.name, "Fireworks Show");
        assert_eq!(prepared.project.dependencies.len(), 4);
        assert!(config.probe_bench_step().is_none());
        config.gpu_bench = Some("unused.json".into());
        assert!((config.probe_bench_step().unwrap().as_secs_f64() - 1.0 / 60.0).abs() < 1e-8);
        assert!(
            BENCH_FRAMES + crate::gpu_bench::DEFAULT_GPU_BENCH_WARMUP
                > prepared.compiled.duration as usize * 60
        );
    }
}
