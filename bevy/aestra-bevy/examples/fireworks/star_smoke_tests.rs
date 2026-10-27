use super::*;
#[path = "star_smoke_authoring.rs"]
mod authoring;

#[test]
fn saved_star_path_candidate_preserves_stars_cues_and_has_no_origin_emission() {
    let options = Options::parse(["--smoke-art-show".into()]).unwrap();
    for (name, expected) in authoring::author(&options.project_root).unwrap() {
        let saved = EffectAsset::from_ron(
            &std::fs::read_to_string(options.project_root.join("effects").join(&name)).unwrap(),
        )
        .unwrap();
        assert_eq!(saved, expected, "regenerate saved candidate {name}");
        assert_eq!(
            saved,
            EffectAsset::from_ron(&saved.to_pretty_ron().unwrap()).unwrap()
        );
        if name.starts_with("fireworks_show") {
            continue;
        }
        let control_name = name.replace("star_path_smoke", "wispy_smoke");
        let control = EffectAsset::from_ron(
            &std::fs::read_to_string(options.project_root.join("effects").join(control_name))
                .unwrap(),
        )
        .unwrap();
        assert_eq!(control.events, saved.events[..control.events.len()]);
        assert_eq!(control.point_lights, saved.point_lights);
        assert_eq!(control.particle_outputs, saved.particle_outputs);
        assert_eq!(control.parameters, saved.parameters);
        for emitter in &control.emitters {
            if emitter.name != "Burst smoke" {
                assert_eq!(
                    emitter,
                    saved.emitters.iter().find(|e| e.id == emitter.id).unwrap()
                );
            }
        }
        let smoke = saved
            .emitters
            .iter()
            .find(|e| e.name == "Star path smoke")
            .unwrap();
        let route = saved.events.iter().find(|l| l.target == smoke.id).unwrap();
        assert_eq!(
            saved
                .emitters
                .iter()
                .find(|e| e.id == route.source)
                .unwrap()
                .name,
            "Main stars"
        );
        assert!(route.trigger.distance_settings().is_some());
        assert_eq!((route.count, route.inherit_velocity), (1, 0.0));
        assert!(smoke.modules.iter().all(|m| match &m.parameters {
            aestra_core::ModuleParameters::Emission {
                spawn_rate,
                burst_count,
            } => *spawn_rate == 0.0 && *burst_count == 0,
            aestra_core::ModuleParameters::Shape { shape } =>
                *shape == aestra_core::EmitterShape::Point,
            aestra_core::ModuleParameters::Initialize { speed, .. } => speed.max < 0.1,
            _ => true,
        }));
        assert!(
            control
                .emitters
                .iter()
                .flat_map(|e| &e.renderers)
                .all(|r| r.material != smoke.renderers[0].material),
            "star-only density control needs a distinct material instance"
        );
    }
}

#[test]
fn star_path_candidate_compiles_through_public_host_at_every_tier() {
    for tier in ["high", "medium", "low"] {
        let options = Options::parse([
            "--smoke-art-show".into(),
            "--effect".into(),
            "effects/fireworks_show_star_path_smoke.aestra.ron".into(),
            "--tier".into(),
            tier.into(),
        ])
        .unwrap();
        let project = options.compile().unwrap();
        assert_eq!(project.root.effect_clips.len(), 13);
        assert_eq!(project.dependencies.len(), 4);
        crate::oit::audit(&project).unwrap();
        for child in project.dependencies.values() {
            let stars = child
                .emitters
                .iter()
                .position(|e| e.name == "Main stars")
                .unwrap();
            let smoke = child
                .emitters
                .iter()
                .position(|e| e.name == "Star path smoke")
                .unwrap();
            assert!(child.event_links.iter().any(|l| l.source == stars
                && l.target == smoke
                && l.trigger.distance_settings().is_some()));
            assert!(
                child.emitters[smoke].max_particles >= child.emitters[stars].max_particles * 32
            );
        }
    }
}
