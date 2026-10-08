//! Saved F8.1B graph contracts; native birth/lighting images are gated separately.
use super::*;
use aestra_runtime::{Instruction, ScalarSource};

#[test]
fn cohorts_resolve_shared_lit_material_and_only_death_links_birth_children() {
    for tier in ["high", "medium", "low"] {
        let options =
            Options::parse(["--smoke-cohorts".into(), "--tier".into(), tier.into()]).unwrap();
        assert_eq!(options.history, PlaybackHistoryPolicy::PlaybackOnly);
        let project = options.compile().unwrap();
        let root = &project.root;
        assert!(root.extension_stages.is_empty());
        assert_eq!(root.duration, 18.0);
        assert_eq!(root.emitters.len(), 5);
        assert_eq!(root.emitters[0].max_particles, 96);
        assert_eq!(root.event_links.len(), 4);
        let graph: Vec<_> = root
            .event_links
            .iter()
            .map(|link| {
                assert_eq!(link.trigger, aestra_bevy::EventTrigger::OnDeath);
                (link.source, link.target, link.count)
            })
            .collect();
        assert_eq!(graph, [(3, 0, 32), (3, 1, 32), (4, 0, 32), (4, 2, 32)]);
        assert_eq!(root.emitters[3].start_time, 0.0);
        assert_eq!(root.emitters[4].start_time, 2.0);
        assert_eq!(root.particle_outputs().count(), 2);
        for ((_, route), source) in root.particle_outputs().zip([1, 2]) {
            assert_eq!(route.source, source);
            assert_eq!(route.trigger, aestra_bevy::EventTrigger::OnSpawn);
        }
        for (index, emitter) in root.emitters.iter().enumerate() {
            assert_eq!(root.is_event_target(index), index < 3);
            let Instruction::Emit {
                spawn_rate,
                burst_count,
                ..
            } = &emitter.execution.emitter_update[0]
            else {
                panic!("missing emission");
            };
            assert!(
                matches!(spawn_rate, ScalarSource::Constant(value) if value.constant_value() == Some(&0.0))
            );
            assert_eq!(burst_count.constant_value(), Some(&u32::from(index >= 3)));
            if index >= 3 {
                assert_eq!(emitter.max_particles, 1);
            }
            if index == 1 || index == 2 {
                assert!(
                    emitter.renderers.is_empty(),
                    "only smoke can supply receiver pixels"
                );
            }
            if index >= 3 {
                assert_eq!(emitter.renderers.len(), 1);
                let Instruction::Appearance { opacity, .. } = &emitter.execution.particle_update[1]
                else {
                    panic!("missing launch appearance");
                };
                assert_eq!(
                    opacity.constant_value().unwrap().sample(0.5),
                    0.0,
                    "launch markers cannot supply receiver pixels"
                );
            }
        }
        let demand: u32 = root
            .event_links
            .iter()
            .filter(|link| link.target == 0)
            .map(|link| root.emitters[link.source].max_particles * link.count)
            .sum();
        assert_eq!(demand, 64);
        assert!(demand <= root.emitters[0].max_particles);
        let presented = aestra_bevy::PresentedEffect::new(root.clone());
        let binding = presented
            .material_binding_for_emitter(
                root.emitters[0].renderers[0].material,
                root.emitters[0].source,
            )
            .unwrap();
        assert!(binding.program().requires_scene_lighting());
    }
}

#[test]
fn cohorts_cannot_be_combined_with_another_lab_but_explicit_effect_still_wins() {
    for other in [
        "--smoke-lighting",
        "--particle-smoke-lighting",
        "--smoke-persistence",
    ] {
        for args in [["--smoke-cohorts", other], [other, "--smoke-cohorts"]] {
            assert!(Options::parse(args.into_iter().map(String::from)).is_err());
        }
    }
    let options = Options::parse([
        "--smoke-cohorts".into(),
        "--effect".into(),
        "effects/fireworks_smoke_persistence.aestra.ron".into(),
    ])
    .unwrap();
    assert!(
        options
            .effect_path
            .ends_with("effects/fireworks_smoke_persistence.aestra.ron")
    );
}
