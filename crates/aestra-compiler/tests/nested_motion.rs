use aestra_compiler::EffectCompiler;
use aestra_core::{
    EffectAsset, EffectClip, EffectPlaybackMode, Emitter, EmitterTransform, HostTransformKey,
    HostTransformTrack,
};
use aestra_project::ProjectAssetIndex;
use aestra_runtime::{CompiledEffect, EffectInstance, InheritedHostTransform};
use std::sync::Arc;

fn effect(name: &str, duration: f32, mode: EffectPlaybackMode, axis: usize) -> EffectAsset {
    let mut effect = EffectAsset::new(name, duration);
    effect.playback_mode = mode;
    let mut end = EmitterTransform::default();
    end.translation[axis] = 20.0;
    effect.host_transform_track = Some(HostTransformTrack::from_pose_keys(
        vec![
            HostTransformKey {
                time: 0.0,
                transform: EmitterTransform::default(),
            },
            HostTransformKey {
                time: 20.0,
                transform: end,
            },
        ],
        false,
    ));
    effect
}

fn compiled(effect: &EffectAsset) -> CompiledEffect {
    EffectCompiler::default().compile(effect).unwrap()
}

#[test]
fn child_clock_retains_continuous_time_and_offsets_are_occurrence_stable() {
    for parent_mode in [
        EffectPlaybackMode::Once,
        EffectPlaybackMode::LoopRestart,
        EffectPlaybackMode::LoopContinuous,
    ] {
        let mut parent = effect("Parent", 6.0, parent_mode, 0);
        let mut child = effect("Child", 2.0, EffectPlaybackMode::LoopContinuous, 1);
        let mut clip = EffectClip::new(child.id, 0.5, 5.5);
        clip.source_offset = 0.75;
        parent.effect_clips.push(clip);
        let parent = compiled(&parent);
        let clip = &parent.effect_clips[0];
        for child_mode in [
            EffectPlaybackMode::Once,
            EffectPlaybackMode::LoopRestart,
            EffectPlaybackMode::LoopContinuous,
        ] {
            child.playback_mode = child_mode;
            let child = compiled(&child);
            assert!(clip.map_instance_time(0.25, &parent, &child).is_none());
            assert_eq!(
                clip.map_instance_time(0.5, &parent, &child),
                Some((0.75, -0.25))
            );
            if child_mode == EffectPlaybackMode::Once {
                continue;
            }
            for time in [1.0, 1.25, 3.25, 5.75] {
                let (local, offset) = clip.map_instance_time(time, &parent, &child).unwrap();
                assert_eq!(local + offset, time);
                if child_mode.is_continuous() {
                    assert_eq!(local, time + 0.25);
                    assert_eq!(offset, -0.25);
                } else {
                    assert!(local < 2.0);
                }
            }
            if parent_mode.is_looping() {
                assert!(clip.map_instance_time(6.25, &parent, &child).is_none());
                let (local, offset) = clip.map_instance_time(7.0, &parent, &child).unwrap();
                assert_eq!(local, 1.25);
                assert_eq!(
                    offset,
                    if parent_mode.is_continuous() {
                        5.75
                    } else {
                        -0.25
                    }
                );
            }
        }
    }
}

#[test]
fn project_samples_compose_parent_clip_and_child_motion_without_changing_particles() {
    let dir = tempfile::tempdir().unwrap();
    let mut leaf = effect("Leaf", 2.0, EffectPlaybackMode::LoopContinuous, 2);
    leaf.emitters.push(Emitter::basic_sprite("Particle", 2.0));
    leaf.save_ron(dir.path().join("leaf.aestra.ron")).unwrap();
    let mut parent = effect("Parent", 4.0, EffectPlaybackMode::LoopContinuous, 1);
    let mut child_clip = EffectClip::new(leaf.id, 0.25, 3.75);
    child_clip.source_offset = 0.5;
    child_clip.transform.translation = [0.0, 5.0, 0.0];
    parent.effect_clips.push(child_clip);
    parent
        .save_ron(dir.path().join("parent.aestra.ron"))
        .unwrap();
    let mut root = effect("Root", 6.0, EffectPlaybackMode::LoopContinuous, 0);
    let mut clip = EffectClip::new(parent.id, 0.5, 5.5);
    clip.source_offset = 0.75;
    clip.transform.translation = [10.0, 0.0, 0.0];
    root.effect_clips.push(clip);
    let project = EffectCompiler::default()
        .compile_project(&root, &ProjectAssetIndex::scan(dir.path()))
        .unwrap();
    for time in [1.0, 3.0, 4.25, 7.0, 1.0] {
        let mut samples = Vec::new();
        project.evaluate(time, 23, &mut samples);
        assert!(!samples.is_empty(), "no nested samples at {time}");
        let phase = time % 6.0;
        let parent_time = phase + 0.25;
        let leaf_time = parent_time % 4.0 + 0.25;
        let parent_clip = &project.root.effect_clips[0];
        let parent_effect = &project.dependencies[&parent.id];
        let leaf_clip = &parent_effect.effect_clips[0];
        let parent_seed = parent_clip.seed.resolve(23, parent_clip.source_clip);
        let leaf_seed = leaf_clip.seed.resolve(parent_seed, leaf_clip.source_clip);
        let mut local = Vec::new();
        aestra_runtime::evaluate(
            &project.dependencies[&leaf.id],
            leaf_time,
            leaf_seed,
            &mut local,
        );
        assert_eq!(
            samples.iter().map(|s| &s.particle).collect::<Vec<_>>(),
            local.iter().collect::<Vec<_>>()
        );
        for sample in &samples {
            assert_eq!(sample.instance_path.len(), 2);
            let m = sample.world_from_effect;
            assert!((m[12] - (10.0 + time)).abs() < 1e-5);
            assert!((m[13] - (5.0 + parent_time)).abs() < 1e-5);
            assert!((m[14] - leaf_time).abs() < 1e-5);
        }
    }
}

#[test]
fn inherited_edits_invalidate_history_but_equivalent_contexts_and_seeks_do_not() {
    let parent = compiled(&effect(
        "Parent",
        4.0,
        EffectPlaybackMode::LoopContinuous,
        0,
    ));
    let mut instance = EffectInstance::new(Arc::new(parent.clone()));
    let chain = InheritedHostTransform::default().for_child(
        parent.host_transform_track.clone(),
        EmitterTransform::default(),
        0.25,
    );
    instance.set_inherited_host_transform(Arc::new(chain.clone()));
    let revision = instance.history_revision();
    instance.set_inherited_host_transform(Arc::new(chain));
    instance.seek(1.0);
    instance.seek(0.5);
    assert_eq!(instance.history_revision(), revision);
    let mut revision = revision;
    for (motion, placement, offset) in [
        (
            parent.host_transform_track.clone(),
            EmitterTransform::default(),
            4.25,
        ),
        (
            parent.host_transform_track.clone(),
            EmitterTransform {
                translation: [1.0, 0.0, 0.0],
                ..Default::default()
            },
            4.25,
        ),
        (None, EmitterTransform::default(), 4.25),
    ] {
        instance.set_inherited_host_transform(Arc::new(
            InheritedHostTransform::default().for_child(motion, placement, offset),
        ));
        revision += 1;
        assert_eq!(instance.history_revision(), revision);
    }
}

#[test]
fn nested_moving_trail_lab_compiles_and_evaluates_after_parent_loops() {
    let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/test");
    let root =
        EffectAsset::load_ron(assets.join("effects/nested_moving_trail_lab.aestra.ron")).unwrap();
    let project = EffectCompiler::default()
        .compile_project(&root, &ProjectAssetIndex::scan(&assets))
        .unwrap();
    assert_eq!(project.dependencies.len(), 2);
    for t in [1.0, 3.0, 5.0, 7.0, 9.0, 1.0] {
        let mut samples = Vec::new();
        project.evaluate(t, 23, &mut samples);
        assert!(!samples.is_empty(), "no trail heads at {t}");
        assert!(samples.iter().all(
            |s| s.instance_path.len() == 2 && s.world_from_effect.iter().all(|v| v.is_finite())
        ));
    }
}
