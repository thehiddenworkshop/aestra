use super::*;
#[path = "wind_smoke_authoring.rs"]
mod authoring;

#[test]
fn wind_haze_variant_preserves_path_births_budgets_and_non_smoke_authoring() {
    let options = Options::parse(["--smoke-art-show".into()]).unwrap();
    for (name, expected) in authoring::author(&options.project_root).unwrap() {
        let saved = EffectAsset::from_ron(
            &std::fs::read_to_string(options.project_root.join("effects").join(&name)).unwrap(),
        )
        .unwrap();
        assert_eq!(saved, expected, "regenerate {name}");
        let control = EffectAsset::from_ron(
            &std::fs::read_to_string(
                options
                    .project_root
                    .join("effects")
                    .join(name.replace("wind_haze_smoke", "star_path_smoke")),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(saved.duration, control.duration);
        assert_eq!(saved.events, control.events);
        assert_eq!(saved.particle_outputs, control.particle_outputs);
        assert_eq!(saved.point_lights, control.point_lights);
        assert_eq!(saved.parameters, control.parameters);
        assert_eq!(saved.particle_budgets, control.particle_budgets);
        for emitter in &control.emitters {
            let variant = saved.emitters.iter().find(|e| e.id == emitter.id).unwrap();
            if emitter.name != "Star path smoke" {
                assert_eq!(variant, emitter);
            } else {
                assert_eq!(variant.max_particles, emitter.max_particles);
                assert_eq!(variant.renderers, emitter.renderers);
                assert_eq!(variant.duration, emitter.duration);
            }
        }
        assert_eq!(saved.effect_clips.len(), control.effect_clips.len());
        for (a, b) in saved.effect_clips.iter().zip(&control.effect_clips) {
            let mut restored = a.clone();
            restored.source.id = b.source.id;
            assert_eq!(&restored, b, "choreography is retained");
        }
    }
}

#[test]
fn wind_haze_material_and_saved_show_compile_at_all_tiers() {
    for tier in ["high", "medium", "low"] {
        let options = Options::parse([
            "--smoke-art-show".into(),
            "--effect".into(),
            "effects/fireworks_show_wind_haze_smoke.aestra.ron".into(),
            "--tier".into(),
            tier.into(),
        ])
        .unwrap();
        let project = options.compile().unwrap();
        assert_eq!(project.root.effect_clips.len(), 13);
        assert_eq!(project.dependencies.len(), 4);
        crate::oit::audit(&project).unwrap();
    }
}
