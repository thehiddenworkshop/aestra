//! Reference-driven, editable hero shell. No special fireworks runtime behavior.
use aestra_bevy::EffectAsset;
use bevy::prelude::{Transform, Vec3};

pub fn effect() -> EffectAsset {
    EffectAsset::from_ron(include_str!(
        "../../../assets/test/effects/fireworks_reference_hero.aestra.ron"
    ))
    .expect("checked-in reference hero must parse")
}

pub fn camera(preset: super::FireworksCamera) -> Transform {
    // Frame the elevated burst and its later droop, not the old F0 detail
    // crop. These are whole-shell review views, not macro head close-ups.
    let (eye, target) = match preset {
        super::FireworksCamera::Close => (Vec3::new(0.0, 25.0, 160.0), Vec3::new(0.0, 18.0, 0.0)),
        super::FireworksCamera::Audience => {
            (Vec3::new(0.0, 35.0, 200.0), Vec3::new(0.0, 20.0, 0.0))
        }
        super::FireworksCamera::Wide => (Vec3::new(0.0, 45.0, 250.0), Vec3::new(0.0, 25.0, 0.0)),
    };
    Transform::from_translation(eye).looking_at(target, Vec3::Y)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aestra_bevy::material::{
        MaterialExpression, MaterialExpressionKind as Expr, MaterialInput, MaterialProgram,
        MaterialProgramRef, MaterialValue, MaterialVectorComponent,
    };
    use aestra_bevy::{
        ColorKey, Curve, CurveKey, EffectId, EffectParameter, EventId, EventLink, EventTrigger,
        Gradient, MaterialExpressionId, MaterialId, MaterialParameterId, MaterialProgramId,
        ModuleParameters, ParameterId, RendererProperties, ScalarRange, TrailSamplingMode, Value,
    };
    use std::collections::BTreeMap;

    const BASE: u128 = 0xa3574a00_0000_4000_8000_0000000f7000;

    fn curve(keys: &[(f32, f32)]) -> Curve {
        Curve::new(keys.iter().map(|&(t, v)| CurveKey::new(t, v)).collect())
    }

    fn appearance(
        emitter: &mut aestra_bevy::Emitter,
        size: &[(f32, f32)],
        opacity: &[(f32, f32)],
        colors: &[(f32, [f32; 4])],
    ) {
        emitter.modules[4].parameters = ModuleParameters::Appearance {
            size: curve(size),
            opacity: curve(opacity),
            color: Gradient::new(colors.iter().map(|&(t, c)| ColorKey::new(t, c)).collect()),
        };
    }

    fn dynamics(
        emitter: &mut aestra_bevy::Emitter,
        life: (f32, f32),
        speed_range: (f32, f32),
        drag_value: f32,
    ) {
        if let ModuleParameters::Initialize {
            lifetime, speed, ..
        } = &mut emitter.modules[2].parameters
        {
            *lifetime = ScalarRange::new(life.0, life.1);
            *speed = ScalarRange::new(speed_range.0, speed_range.1);
        }
        if let ModuleParameters::Motion { drag, .. } = &mut emitter.modules[3].parameters {
            *drag = drag_value;
        }
    }

    fn tail(emitter: &mut aestra_bevy::Emitter, width_value: f32, life: f32) {
        if let RendererProperties::Trail {
            width,
            lifetime,
            sampling,
            sample_interval,
            max_trails,
            ..
        } = &mut emitter.renderers[1].properties
        {
            *width = width_value;
            *lifetime = life;
            *sampling = TrailSamplingMode::Time;
            *sample_interval = 1.0 / 60.0;
            *max_trails = emitter.max_particles;
        }
    }

    fn build() -> EffectAsset {
        let mut source =
            super::super::fireworks_f3::effect(super::super::fireworks_f3::Probe::Chrysanthemum);
        source.id = EffectId::from_u128(BASE);
        source.name = "Fireworks Reference Hero".into();
        source.duration = 8.0;
        source.parameters.clear();
        source.events.clear();
        for emitter in &mut source.emitters {
            for module in &mut emitter.modules {
                module.bindings.clear();
            }
        }
        let pink = [
            (0.0, [1.0, 0.78, 0.48, 1.0]),
            (0.16, [1.0, 0.09, 0.28, 1.0]),
            (0.7, [0.6, 0.015, 0.06, 1.0]),
            (1.0, [0.12, 0.004, 0.008, 1.0]),
        ];
        let gold = [
            (0.0, [1.0, 0.95, 0.7, 1.0]),
            (0.35, [1.0, 0.55, 0.12, 1.0]),
            (0.8, [0.45, 0.08, 0.005, 1.0]),
            (1.0, [0.08, 0.01, 0.001, 1.0]),
        ];
        // Main, inner and free embers are separate bounded authored layers;
        // they all originate at one real rocket death, not a timed fake burst.
        source.emitters[1].max_particles = 384;
        dynamics(&mut source.emitters[1], (2.6, 3.5), (20.0, 24.0), 0.45);
        appearance(
            &mut source.emitters[1],
            &[(0.0, 0.65), (0.65, 0.4), (1.0, 0.08)],
            &[(0.0, 1.0), (0.65, 0.9), (1.0, 0.0)],
            &pink,
        );
        tail(&mut source.emitters[1], 0.35, 0.7);
        let mut inner = source.emitters[1].clone();
        inner.name = "Gold inner stars".into();
        inner.max_particles = 96;
        dynamics(&mut inner, (2.8, 4.0), (12.0, 18.0), 0.5);
        appearance(
            &mut inner,
            &[(0.0, 0.5), (0.7, 0.25), (1.0, 0.04)],
            &[(0.0, 1.0), (0.65, 0.8), (1.0, 0.0)],
            &gold,
        );
        tail(&mut inner, 0.3, 0.85);
        let mut embers = inner.clone();
        embers.name = "Free cooling embers".into();
        embers.max_particles = 128;
        embers.renderers.truncate(1);
        dynamics(&mut embers, (1.8, 4.0), (14.0, 25.0), 0.3);
        appearance(
            &mut embers,
            &[(0.0, 0.24), (0.5, 0.18), (1.0, 0.02)],
            &[(0.0, 0.75), (0.7, 0.5), (1.0, 0.0)],
            &gold,
        );
        source.emitters.extend([inner, embers]);
        // Small flash, not the baseline's large white disk.
        dynamics(&mut source.emitters[2], (0.09, 0.09), (0.0, 0.0), 0.0);
        appearance(
            &mut source.emitters[2],
            &[(0.0, 2.5), (1.0, 0.05)],
            &[(0.0, 1.0), (1.0, 0.0)],
            &gold,
        );
        // Explicitly unlit particle approximation: no claim of burst-driven
        // lighting, trajectory attachment or volumetric smoke integration.
        let smoke = [
            (0.0, [0.3, 0.22, 0.27, 1.0]),
            (0.25, [0.16, 0.13, 0.15, 1.0]),
            (1.0, [0.055, 0.06, 0.07, 1.0]),
        ];
        appearance(
            &mut source.emitters[3],
            &[(0.0, 0.7), (0.4, 2.5), (1.0, 5.0)],
            &[(0.0, 0.0), (0.2, 0.24), (1.0, 0.0)],
            &smoke,
        );
        source.emitters[4].max_particles = 48;
        dynamics(&mut source.emitters[4], (3.0, 4.5), (3.0, 7.0), 0.8);
        appearance(
            &mut source.emitters[4],
            &[(0.0, 1.0), (0.35, 4.5), (1.0, 9.0)],
            &[(0.0, 0.0), (0.12, 0.2), (1.0, 0.0)],
            &smoke,
        );
        for (i, material) in source.material_instances.iter_mut().enumerate() {
            let old = material.id;
            material.id = MaterialId::from_u128(BASE + 900 + i as u128);
            if i < 2 {
                material.program = MaterialProgramRef::Project(MaterialProgramId::from_u128(
                    BASE + 1000 + i as u128 * 100,
                ));
                material.values = BTreeMap::from([(
                    MaterialParameterId::from_u128(BASE + 1090 + i as u128 * 100),
                    aestra_bevy::material::MaterialParameterValue::EffectParameter(
                        ParameterId::from_u128(BASE + 50 + i as u128),
                    ),
                )]);
            }
            for emitter in &mut source.emitters {
                for renderer in &mut emitter.renderers {
                    if renderer.material == old {
                        renderer.material = material.id;
                    }
                }
            }
        }
        for (i, name, gain) in [(0, "Star radiance", 12.0), (1, "Trail radiance", 6.0)] {
            source.parameters.push(EffectParameter {
                id: ParameterId::from_u128(BASE + 50 + i),
                name: name.into(),
                default: Value::Scalar(gain),
                exposed: true,
            });
        }
        for (i, emitter) in source.emitters.iter_mut().enumerate() {
            if i != 3 {
                emitter.duration = source.duration;
            }
            super::super::fireworks_f0::fix_emitter_ids(emitter, BASE + 100 + i as u128 * 100);
            // Expose appearance gradients without losing their authored decay.
            let mut gradient = match emitter.modules[4].parameter_value("color").unwrap() {
                Value::Gradient(g) => g,
                _ => unreachable!(),
            };
            gradient.id = aestra_bevy::GradientId::from_u128(BASE + 800 + i as u128);
            let id = ParameterId::from_u128(BASE + 60 + i as u128);
            source.parameters.push(EffectParameter {
                id,
                name: format!("{} color", emitter.name),
                default: Value::Gradient(gradient),
                exposed: true,
            });
            emitter.modules[4].bindings.insert("color".into(), id);
        }
        for (i, target, count) in [(0, 1, 384), (1, 2, 1), (2, 4, 48), (3, 5, 96), (4, 6, 128)] {
            let mut link = EventLink::new(
                source.emitters[0].id,
                EventTrigger::OnDeath,
                source.emitters[target].id,
            );
            link.id = EventId::from_u128(BASE + 850 + i);
            link.count = count;
            link.inherit_velocity = 0.02;
            source.events.push(link);
        }
        source.metadata.insert(
            "status".into(),
            "F4M reference-driven hero prototype; artistic acceptance pending".into(),
        );
        source.metadata.insert("notes".into(), "Cannes-inspired pink/gold layers with white cores and longitudinal tail coverage. Unlit smoke approximation, not illumination. No exposure accumulation, secondary-event chains or AAA-density certification.".into());
        source
    }

    fn programs() -> Vec<MaterialProgram> {
        [
            include_str!("../../../assets/test/materials/fireworks_star.aestra.material.ron"),
            include_str!("../../../assets/test/materials/fireworks_trail.aestra.material.ron"),
        ]
        .into_iter()
        .enumerate()
        .map(|(index, source)| {
            let mut program = MaterialProgram::from_ron(source).unwrap();
            let old_base = program.id.as_uuid().as_u128();
            let base = BASE + 1000 + index as u128 * 100;
            // Remap only the UUIDs belonging to these deterministic
            // baseline programs (including all DAG refs).
            let ron = program.to_pretty_ron().unwrap();
            let mut remapped = ron;
            for offset in (0..=90).rev() {
                remapped = remapped.replace(
                    &MaterialExpressionId::from_u128(old_base + offset).to_string(),
                    &MaterialExpressionId::from_u128(base + offset).to_string(),
                );
            }
            program = MaterialProgram::from_ron(&remapped).unwrap();
            let id = |n| MaterialExpressionId::from_u128(base + n);
            let mut extra = Vec::new();
            if index == 0 {
                program.name = "Fireworks Hero Hot Core".into();
                extra.extend([
                    (5, Expr::Constant(MaterialValue::Float(1.0))),
                    (18, Expr::Constant(MaterialValue::Float(0.16))),
                    (19, Expr::Constant(MaterialValue::Float(0.14))),
                    (
                        20,
                        Expr::RadialMask {
                            uv: id(3),
                            center: id(6),
                            radius: id(18),
                            softness: id(19),
                            invert: id(9),
                        },
                    ),
                    (
                        21,
                        Expr::Constant(MaterialValue::ColorSrgb([1.0, 0.97, 0.9, 1.0])),
                    ),
                    (22, Expr::Input(MaterialInput::ParticleNormalizedAge)),
                    (23, Expr::Subtract(id(5), id(22))),
                    (24, Expr::Multiply(id(20), id(23))),
                    (
                        25,
                        Expr::Lerp {
                            start: id(1),
                            end: id(21),
                            factor: id(24),
                        },
                    ),
                ]);
                program
                    .expressions
                    .iter_mut()
                    .find(|e| e.id == id(17))
                    .unwrap()
                    .kind = Expr::Multiply(id(25), id(16));
            } else {
                program.name = "Fireworks Hero Tapered Tail".into();
                // Stretch U is oldest=0, live head=1. Existing geometry also
                // narrows with sample age; this fades the oldest end smoothly.
                extra.extend([
                    (
                        18,
                        Expr::ExtractComponent {
                            value: id(3),
                            component: MaterialVectorComponent::X,
                        },
                    ),
                    (19, Expr::Constant(MaterialValue::Float(0.75))),
                    (
                        20,
                        Expr::Smoothstep {
                            edge_min: id(4),
                            edge_max: id(19),
                            value: id(18),
                        },
                    ),
                    (21, Expr::Multiply(id(13), id(20))),
                ]);
                program.outputs.alpha = id(21);
            }
            program.expressions.extend(
                extra
                    .into_iter()
                    .map(|(n, kind)| MaterialExpression { id: id(n), kind }),
            );
            program
        })
        .collect()
    }

    #[test]
    fn hero_is_a_normal_resolvable_project_asset_with_bounded_death_cohorts() {
        let source = effect();
        assert_eq!(
            source.to_pretty_ron().unwrap(),
            build().to_pretty_ron().unwrap()
        );
        let index = aestra_project::ProjectAssetIndex::scan(super::super::viewer_asset_root(None));
        let project = index.resolve_effect_project(&source).unwrap();
        aestra_bevy::EffectCompiler::default()
            .compile_resolved_project(&project)
            .unwrap();
        assert_eq!(source.emitters.len(), 7);
        assert_eq!(source.events.iter().map(|e| e.count).sum::<u32>(), 657);
        for link in &source.events {
            assert_eq!(link.source, source.emitters[0].id);
            assert_eq!(link.trigger, EventTrigger::OnDeath);
            let target = source
                .emitters
                .iter()
                .find(|e| e.id == link.target)
                .unwrap();
            assert_eq!(link.count, target.max_particles);
            assert!(matches!(
                target.modules[0].parameters,
                ModuleParameters::Emission {
                    spawn_rate: 0.0,
                    burst_count: 0
                }
            ));
        }
        for emitter in &source.emitters {
            for renderer in &emitter.renderers {
                if let RendererProperties::Trail {
                    lifetime,
                    sample_interval,
                    max_points,
                    max_trails,
                    ..
                } = renderer.properties
                {
                    assert!(lifetime / sample_interval + 2.0 <= max_points as f32);
                    assert_eq!(max_trails, emitter.max_particles);
                }
            }
        }
        for expected in programs() {
            assert_eq!(
                expected.normalized(),
                project.material_programs[&expected.id].normalized()
            );
            aestra_bevy::compile_material_program(&expected).unwrap();
        }
    }

    #[test]
    fn hero_materials_keep_radiance_out_of_coverage_and_use_age_and_tail_uv() {
        for (index, program) in programs().into_iter().enumerate() {
            let dependencies = |root| {
                let mut pending = vec![root];
                let mut kinds = Vec::new();
                while let Some(id) = pending.pop() {
                    let expression = program.expressions.iter().find(|e| e.id == id).unwrap();
                    pending.extend(expression.kind.dependencies());
                    kinds.push(&expression.kind);
                }
                kinds
            };
            assert!(
                dependencies(program.outputs.color)
                    .iter()
                    .any(|kind| matches!(kind, Expr::Parameter(_)))
            );
            assert!(
                !dependencies(program.outputs.alpha)
                    .iter()
                    .any(|kind| matches!(kind, Expr::Parameter(_)))
            );
            if index == 0 {
                assert!(
                    dependencies(program.outputs.color)
                        .iter()
                        .any(|kind| matches!(
                            kind,
                            Expr::Input(MaterialInput::ParticleNormalizedAge)
                        ))
                );
                assert!(
                    dependencies(program.outputs.color)
                        .iter()
                        .any(|kind| matches!(kind, Expr::RadialMask { .. }))
                );
            } else {
                assert!(
                    dependencies(program.outputs.alpha)
                        .iter()
                        .any(|kind| matches!(
                            kind,
                            Expr::ExtractComponent {
                                component: MaterialVectorComponent::X,
                                ..
                            }
                        ))
                );
                assert!(
                    dependencies(program.outputs.alpha)
                        .iter()
                        .any(|kind| matches!(kind, Expr::Input(MaterialInput::RibbonUv)))
                );
            }
        }
    }

    #[test]
    fn hero_cli_preserves_baseline_cameras_and_uses_playback_only_when_requested() {
        let config = super::super::ViewerConfig::from_iter(
            [
                "--fireworks-f0",
                "--fireworks-f0-probe",
                "f4-reference-hero",
                "--history",
                "playback-only",
            ]
            .into_iter()
            .map(str::to_owned),
        )
        .unwrap();
        assert_eq!(
            config.fireworks_probe,
            Some(super::super::FireworksProbe::ReferenceHero)
        );
        assert_eq!(
            config.history_policy,
            aestra_bevy::PlaybackHistoryPolicy::PlaybackOnly
        );
        assert_eq!(
            super::super::prepare_viewer(&config)
                .unwrap_or_else(|e| panic!("{}", e.message))
                .compiled
                .name,
            effect().name
        );
        let old = super::super::FireworksCamera::Close.transform();
        let new = camera(super::super::FireworksCamera::Close);
        assert_ne!(old, new);
        assert!(
            new.forward()
                .dot((Vec3::new(0.0, 18.0, 0.0) - new.translation).normalize())
                > 0.999
        );
    }

    #[test]
    #[ignore = "prints fixture source for a reviewed apply_patch update"]
    fn export_hero_fixture_sources() {
        let mut files = BTreeMap::from([(
            "assets/test/effects/fireworks_reference_hero.aestra.ron".to_owned(),
            build().to_pretty_ron().unwrap(),
        )]);
        for (program, name) in programs().into_iter().zip(["star", "trail"]) {
            files.insert(
                format!("assets/test/materials/fireworks_hero_{name}.aestra.material.ron"),
                program.to_pretty_ron().unwrap(),
            );
        }
        println!("HERO_FIXTURES={}", serde_json::to_string(&files).unwrap());
    }
}
