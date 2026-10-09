//! Saved candidate contracts; the default show and its four source assets stay unchanged.
use super::*;
use aestra_bevy::{EffectPlaybackMode, EffectPlayer, ModuleParameters, PresentedEffect};

#[test]
fn smoke_art_draft_preserves_baseline_routes_and_changes_only_smoke() {
    let art = Options::parse(["--smoke-art-show".into()]).unwrap();
    assert!(art.smoke_art_show && !art.smoke_lighting);
    assert!(
        art.effect_path
            .ends_with("effects/fireworks_show_wispy_smoke.aestra.ron")
    );
    for other in [
        "--persistent-smoke-show",
        "--smoke-lighting",
        "--particle-smoke-lighting",
        "--smoke-persistence",
        "--smoke-cohorts",
    ] {
        for args in [["--smoke-art-show", other], [other, "--smoke-art-show"]] {
            assert!(Options::parse(args.into_iter().map(String::from)).is_err());
        }
    }
    let read =
        |path: PathBuf| EffectAsset::from_ron(&std::fs::read_to_string(path).unwrap()).unwrap();
    let baseline = read(
        art.project_root
            .join("effects/fireworks_show_persistent_smoke.aestra.ron"),
    );
    let draft = read(art.effect_path.clone());
    assert_eq!(baseline.duration, draft.duration);
    assert_eq!(baseline.effect_clips.len(), draft.effect_clips.len());
    for (a, b) in baseline.effect_clips.iter().zip(&draft.effect_clips) {
        assert_eq!(
            (a.id, a.start_time, a.duration, a.transform, a.seed),
            (b.id, b.start_time, b.duration, b.transform, b.seed)
        );
        assert_eq!(a.parameter_overrides, b.parameter_overrides);
    }
    for family in ["multi_break", "crackle", "crossette", "strobe"] {
        let a = read(art.project_root.join(format!(
            "effects/fireworks_{family}_persistent_smoke.aestra.ron"
        )));
        let b = read(
            art.project_root
                .join(format!("effects/fireworks_{family}_wispy_smoke.aestra.ron")),
        );
        assert_eq!(a.events, b.events[1..]);
        let wake = &b.events[0];
        assert_eq!(
            wake.trigger,
            aestra_core::EventTrigger::OnDistance {
                spacing: 1.0,
                max_per_tick: 8
            }
        );
        assert_eq!((wake.count, wake.inherit_velocity), (1, 0.0));
        assert_eq!(
            wake.source,
            b.emitters
                .iter()
                .find(|emitter| emitter.name == "Launch shell")
                .unwrap()
                .id
        );
        assert_eq!(
            wake.target,
            b.emitters
                .iter()
                .find(|emitter| emitter.name == "Launch smoke")
                .unwrap()
                .id
        );
        assert_eq!(a.particle_outputs, b.particle_outputs);
        assert_eq!(a.point_lights, b.point_lights);
        assert_eq!(a.parameters, b.parameters);
        assert_eq!(a.particle_budgets, b.particle_budgets);
        assert_eq!(a.emitters.len(), b.emitters.len());
        for (old, new) in a.emitters.iter().zip(&b.emitters) {
            assert_eq!(old.max_particles, new.max_particles);
            if old.name == "Launch smoke" {
                assert_eq!(new.duration, 18.0);
                assert!(new.modules.iter().any(|m| matches!(m.parameters, ModuleParameters::Initialize { lifetime, speed, .. } if lifetime.min == 8.0 && lifetime.max == 10.0 && speed.max == 0.15)));
                assert!(new.modules.iter().any(|m| matches!(m.parameters, ModuleParameters::Motion { gravity, drag, .. } if gravity == [0.18, 0.035, 0.02] && drag == 0.3)));
            } else if old.name != "Burst smoke" {
                assert_eq!(old, new);
            } else {
                assert!(new.modules.iter().any(|m| matches!(m.parameters, ModuleParameters::Shape { shape: aestra_bevy::EmitterShape::Sphere { radius } } if radius == 9.0)));
                assert!(new.modules.iter().any(|m| matches!(m.parameters, ModuleParameters::Initialize { lifetime, speed, .. } if lifetime.min == 12.0 && lifetime.max == 14.0 && speed.min == 2.5 && speed.max == 5.0)));
                let (size, opacity) = new
                    .modules
                    .iter()
                    .find_map(|m| match &m.parameters {
                        ModuleParameters::Appearance { size, opacity, .. } => Some((size, opacity)),
                        _ => None,
                    })
                    .unwrap();
                assert_eq!(
                    size.keys.iter().map(|k| k.value).collect::<Vec<_>>(),
                    [2.0, 7.5, 12.0]
                );
                assert!(
                    opacity
                        .keys
                        .iter()
                        .any(|k| k.time == 0.7 && k.value == 0.18)
                );
                assert_eq!(opacity.keys.last().unwrap().value, 0.0);
            }
        }
    }
}

#[test]
fn smoke_art_draft_compiles_shared_wisps_at_all_tiers() {
    for tier in ["high", "medium", "low"] {
        let options =
            Options::parse(["--smoke-art-show".into(), "--tier".into(), tier.into()]).unwrap();
        let project = options.compile().unwrap();
        assert_eq!(project.dependencies.len(), 4);
        assert_eq!(project.root.effect_clips.len(), 13);
        assert_eq!(project.root.duration, 37.0);
        assert!(!options.lighting_policy().particle.enabled);
        for child in project.dependencies.values() {
            let presented = PresentedEffect::new(child.clone());
            let mut wisps = 0;
            for emitter in &child.emitters {
                for renderer in &emitter.renderers {
                    let binding = presented
                        .material_binding_for_emitter(renderer.material, emitter.source)
                        .unwrap();
                    wisps += usize::from(binding.program().requires_scene_lighting());
                }
            }
            assert_eq!(wisps, 2);
        }
    }
}

#[test]
fn persistent_show_is_opt_in_and_cannot_be_combined_with_labs() {
    let normal = Options::parse([]).unwrap();
    assert!(!normal.persistent_smoke_show);
    assert!(
        normal
            .effect_path
            .ends_with("effects/fireworks_show.aestra.ron")
    );
    for lab in [
        "--smoke-lighting",
        "--particle-smoke-lighting",
        "--smoke-persistence",
        "--smoke-cohorts",
    ] {
        for args in [
            ["--persistent-smoke-show", lab],
            [lab, "--persistent-smoke-show"],
        ] {
            assert!(Options::parse(args.into_iter().map(String::from)).is_err());
        }
    }
    let explicit = Options::parse([
        "--persistent-smoke-show".into(),
        "--effect".into(),
        "effects/fireworks_show.aestra.ron".into(),
    ])
    .unwrap();
    assert!(
        explicit
            .effect_path
            .ends_with("effects/fireworks_show.aestra.ron")
    );
}

#[test]
fn persistent_show_keeps_authored_launches_and_preserves_smoke_tail_windows() {
    let normal = Options::parse([]).unwrap();
    let candidate = Options::parse(["--persistent-smoke-show".into()]).unwrap();
    let read =
        |path: &PathBuf| EffectAsset::from_ron(&std::fs::read_to_string(path).unwrap()).unwrap();
    let original = read(&normal.effect_path);
    let saved = read(&candidate.effect_path);
    assert_eq!(saved.duration, 37.0);
    assert_eq!(saved.playback_mode, EffectPlaybackMode::Once);
    assert!(saved.emitters.is_empty());
    assert_ne!(saved.id, original.id);
    assert_eq!(saved.effect_clips.len(), 13);
    for (a, b) in saved.effect_clips.iter().zip(&original.effect_clips) {
        assert_eq!(a.id, b.id);
        assert_ne!(a.source, b.source);
        assert_eq!(a.start_time, b.start_time);
        assert_eq!(a.source_offset, b.source_offset);
        assert_eq!(a.transform, b.transform);
        assert_eq!(a.seed, b.seed);
        assert_eq!(a.parameter_overrides, b.parameter_overrides);
        assert_eq!(a.duration, 18.0);
        assert!(a.start_time + a.duration <= saved.duration - 2.0);
    }
    assert_eq!(
        read(&candidate.effect_path),
        EffectAsset::from_ron(&saved.to_pretty_ron().unwrap()).unwrap()
    );
    for family in ["multi_break", "crackle", "crossette", "strobe"] {
        let original = read(
            &normal
                .project_root
                .join(format!("effects/fireworks_{family}.aestra.ron")),
        );
        let variant = read(&normal.project_root.join(format!(
            "effects/fireworks_{family}_persistent_smoke.aestra.ron"
        )));
        assert_ne!(original.id, variant.id);
        assert_eq!(original.events, variant.events);
        assert_eq!(original.particle_outputs, variant.particle_outputs);
        assert_eq!(original.point_lights, variant.point_lights);
        assert_eq!(original.parameters, variant.parameters);
        for (a, b) in original.emitters.iter().zip(&variant.emitters) {
            assert_eq!(a.id, b.id);
            assert_eq!(a.max_particles, b.max_particles);
            if a.name != "Burst smoke" {
                assert_eq!(
                    a, b,
                    "unchanged non-burst-smoke emitter: {family}/{}",
                    a.name
                );
            } else {
                assert_eq!(b.duration, 18.0);
                assert!(b.modules.iter().any(|module| matches!(module.parameters, ModuleParameters::Initialize { lifetime, .. } if lifetime.min == 12.0 && lifetime.max == 14.0)));
            }
        }
    }
}

#[test]
fn all_tiers_compile_candidate_with_shared_lit_material_and_public_playback_only() {
    for tier in ["high", "medium", "low"] {
        let options = Options::parse([
            "--persistent-smoke-show".into(),
            "--tier".into(),
            tier.into(),
        ])
        .unwrap();
        assert!(
            !options.smoke_lighting,
            "full audience camera/environment, not lab scale"
        );
        let policy = options.lighting_policy();
        assert!(
            !policy.particle.enabled,
            "no invented selected-star outputs"
        );
        assert_eq!(
            policy.representative.max_lights,
            LightingQualityPolicy::preset(tier)
                .unwrap()
                .representative
                .max_lights
        );
        let project = options.compile().unwrap();
        assert_eq!(project.dependencies.len(), 4);
        assert_eq!(project.root.duration, 37.0);
        for shell in project.dependencies.values() {
            let presented = PresentedEffect::new(shell.clone());
            assert_eq!(shell.duration, 18.0);
            assert_eq!(shell.point_lights.len(), 1);
            let mut lit = 0;
            for emitter in &shell.emitters {
                for renderer in &emitter.renderers {
                    let binding = presented
                        .material_binding_for_emitter(renderer.material, emitter.source)
                        .unwrap();
                    lit += usize::from(binding.program().requires_scene_lighting());
                }
            }
            assert_eq!(
                lit, 2,
                "launch and burst smoke share the lit billow program"
            );
        }
        assert!(
            project.instances(30.0, SHOW_SEED).len() > 1,
            "late smoke owners survive original show endpoint"
        );
        assert_eq!(project.instances(36.0, SHOW_SEED).len(), 1);
        let player = EffectPlayer::from_project(project).with_history_policy(options.history);
        assert_eq!(player.history_policy(), PlaybackHistoryPolicy::PlaybackOnly);
    }
}
