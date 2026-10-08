//! Saved candidate contracts; the default show and its four source assets stay unchanged.
use super::*;
use aestra_bevy::{EffectPlaybackMode, EffectPlayer, ModuleParameters, PresentedEffect};

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
