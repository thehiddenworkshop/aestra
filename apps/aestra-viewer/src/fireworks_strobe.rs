//! Bounded strobe shell authored with a generic periodic material function.
use aestra_bevy::EffectAsset;

pub fn effect() -> EffectAsset {
    EffectAsset::from_ron(include_str!(
        "../../../assets/test/effects/fireworks_strobe.aestra.ron"
    ))
    .expect("checked-in strobe shell must parse")
}

#[cfg(test)]
mod tests {
    use super::*;
    use aestra_bevy::material::{
        MaterialEvaluationDomain, MaterialExpression, MaterialExpressionKind as Expr,
        MaterialFunction, MaterialFunctionRef, MaterialInput, MaterialInstance, MaterialParameter,
        MaterialParameterValue, MaterialProgram, MaterialProgramRef, MaterialValue,
        MaterialValueType,
    };
    use aestra_bevy::{
        EffectId, EventId, EventLink, EventTrigger, ModuleParameters, ParameterId,
        RendererProperties, ScalarRange, Value,
    };
    use aestra_bevy::{MaterialExpressionId, MaterialId, MaterialParameterId, MaterialProgramId};
    use std::collections::BTreeMap;

    const BASE: u128 = 0xa3574a00_0000_4000_8000_0000000fb000;

    fn program() -> MaterialProgram {
        let mut p = MaterialProgram::from_ron(include_str!(
            "../../../assets/test/materials/fireworks_hero_star.aestra.material.ron"
        ))
        .unwrap();
        // Remap the entire existing graph, including its parameter references.
        let old = p.id.as_uuid().as_u128();
        let mut ron = p.to_pretty_ron().unwrap();
        for offset in (0..=90).rev() {
            ron = ron.replace(
                &MaterialExpressionId::from_u128(old + offset).to_string(),
                &MaterialExpressionId::from_u128(BASE + 1000 + offset).to_string(),
            );
        }
        p = MaterialProgram::from_ron(&ron).unwrap();
        p.name = "Fireworks Strobe Hot Core".into();
        let id = |n| MaterialExpressionId::from_u128(BASE + 1200 + n);
        let cycles = MaterialParameterId::from_u128(BASE + 1191);
        let duty = MaterialParameterId::from_u128(BASE + 1192);
        for (parameter, name, default) in [(cycles, "Cycles per life", 18.0), (duty, "Duty", 0.18)]
        {
            p.parameters.push(MaterialParameter {
                id: parameter,
                name: name.into(),
                value_type: MaterialValueType::Float,
                evaluation_domain: MaterialEvaluationDomain::Effect,
                default: Some(MaterialValue::Float(default)),
            });
        }
        let function = MaterialFunction::from_ron(include_str!(
            "../../../assets/test/materials/periodic_gate.aestra.material-function.ron"
        ))
        .unwrap();
        let color = p.outputs.color;
        let alpha = p.outputs.alpha;
        for (n, kind) in [
            (0, Expr::Input(MaterialInput::ParticleNormalizedAge)),
            (1, Expr::Input(MaterialInput::ParticleRandom)),
            (2, Expr::Parameter(cycles)),
            (3, Expr::Parameter(duty)),
            (4, Expr::Multiply(id(0), id(2))),
            (5, Expr::Add(id(4), id(1))),
            (
                6,
                Expr::FunctionCall {
                    function: MaterialFunctionRef::Project(function.id),
                    arguments: BTreeMap::from([
                        (function.inputs[0].id, id(5)),
                        (function.inputs[1].id, id(3)),
                    ]),
                    output: function.outputs[0].id,
                },
            ),
            (7, Expr::Multiply(color, id(6))),
            (8, Expr::Multiply(alpha, id(6))),
        ] {
            p.expressions.push(MaterialExpression { id: id(n), kind });
        }
        p.outputs.color = id(7);
        p.outputs.alpha = id(8);
        p
    }

    fn build() -> EffectAsset {
        let mut shell = super::super::fireworks_hero::effect();
        super::super::fireworks_f5::scaled_launch_headroom(&mut shell);
        shell.id = EffectId::from_u128(BASE);
        shell.name = "Fireworks Strobe".into();
        shell.duration = 7.0;
        shell.emitters.truncate(5);
        shell.emitters[1].max_particles = 192;
        // Fixed three-second lifetime maps 18 cycles to six flashes per second.
        if let ModuleParameters::Initialize {
            lifetime, speed, ..
        } = &mut shell.emitters[1].modules[2].parameters
        {
            *lifetime = ScalarRange::new(3.0, 3.0);
            *speed = ScalarRange::new(18.0, 22.0);
        }
        // No persistent luminous history bridging the deliberate off intervals.
        shell.emitters[1]
            .renderers
            .retain(|r| matches!(r.properties, RendererProperties::Sprite));
        shell.parameters.retain(|p| {
            p.name == "Star radiance"
                || p.name == "Trail radiance"
                || [
                    "Launch shell color",
                    "Main stars color",
                    "Burst flash color",
                    "Launch smoke color",
                    "Burst smoke color",
                ]
                .contains(&p.name.as_str())
        });
        for (i, parameter) in shell.parameters.iter_mut().enumerate() {
            let old = parameter.id;
            parameter.id = ParameterId::from_u128(BASE + 50 + i as u128);
            if parameter.name == "Main stars color" {
                parameter.default = Value::Gradient(aestra_bevy::Gradient::new(vec![
                    aestra_bevy::ColorKey::new(0.0, [1.0, 0.97, 0.85, 1.0]),
                    aestra_bevy::ColorKey::new(0.7, [1.0, 0.6, 0.15, 1.0]),
                    aestra_bevy::ColorKey::new(1.0, [0.12, 0.03, 0.002, 1.0]),
                ]));
            }
            if let Value::Gradient(g) = &mut parameter.default {
                g.id = aestra_bevy::GradientId::from_u128(BASE + 800 + i as u128);
            }
            for emitter in &mut shell.emitters {
                for module in &mut emitter.modules {
                    for binding in module.bindings.values_mut() {
                        if *binding == old {
                            *binding = parameter.id;
                        }
                    }
                }
            }
            for material in &mut shell.material_instances {
                for value in material.values.values_mut() {
                    if *value == MaterialParameterValue::EffectParameter(old) {
                        *value = MaterialParameterValue::EffectParameter(parameter.id);
                    }
                }
            }
        }
        let p = program();
        let material = MaterialId::from_u128(BASE + 940);
        let gain = shell
            .parameters
            .iter()
            .find(|p| p.name == "Star radiance")
            .unwrap()
            .id;
        let mut values = BTreeMap::from([(
            p.parameters[0].id,
            MaterialParameterValue::EffectParameter(gain),
        )]);
        for (i, parameter) in p.parameters.iter().skip(1).enumerate() {
            let id = ParameterId::from_u128(BASE + 70 + i as u128);
            let MaterialValue::Float(default) = parameter.default.as_ref().unwrap() else {
                unreachable!()
            };
            shell.parameters.push(aestra_bevy::EffectParameter {
                id,
                name: format!("Strobe {}", parameter.name.to_lowercase()),
                default: Value::Scalar(*default),
                exposed: true,
            });
            values.insert(parameter.id, MaterialParameterValue::EffectParameter(id));
        }
        shell.material_instances.push(MaterialInstance {
            id: material,
            program: MaterialProgramRef::Project(p.id),
            values,
            render_state: p.render_state_policy.default,
        });
        shell.emitters[1].renderers[0].material = material;
        for (i, emitter) in shell.emitters.iter_mut().enumerate() {
            if i != 3 {
                emitter.duration = shell.duration;
            }
            super::super::fireworks_f0::fix_emitter_ids(emitter, BASE + 100 + i as u128 * 100);
        }
        shell.events.clear();
        for (i, target, count) in [(0, 1, 192), (1, 2, 1), (2, 4, 48)] {
            let mut link = EventLink::new(
                shell.emitters[0].id,
                EventTrigger::OnDeath,
                shell.emitters[target].id,
            );
            link.id = EventId::from_u128(BASE + 850 + i);
            link.count = count;
            link.inherit_velocity = 0.02;
            shell.events.push(link);
        }
        shell.event_outputs.clear();
        shell.particle_outputs.clear();
        for (i, name, trigger) in [
            (0, "launch", EventTrigger::OnSpawn),
            (1, "main_break", EventTrigger::OnDeath),
        ] {
            let mut definition = aestra_bevy::EventDefinition::new(name);
            definition.id = aestra_bevy::EventDefinitionId::from_u128(BASE + 900 + i);
            let mut route =
                aestra_bevy::ParticleOutputRoute::new(shell.emitters[0].id, trigger, definition.id);
            route.id = aestra_bevy::EventRouteId::from_u128(BASE + 910 + i);
            route.aggregation = aestra_bevy::EventAggregation::FirstPerTick;
            shell.event_outputs.push(definition);
            shell.particle_outputs.push(route);
        }
        shell.metadata.insert("status".into(), "F5E bounded per-particle strobe prototype; artistic and finale-scale acceptance pending".into());
        shell.metadata.insert("notes".into(), "192 sprite-only stars, 3 seconds, 18 cycles per life (6 Hz), 18% duty. Stable ParticleRandom phase and generic Periodic Gate WESL function gate RGB and alpha; exact off intervals, no luminous trails or host timers. Unlit smoke. Normalized-age timing means lifetime edits change Hertz. Flashes are appearance, not sound event triggers.".into());
        super::super::fireworks_budgets::author_profiles(&mut shell);
        shell
    }

    #[test]
    #[ignore = "prints fixture sources for a reviewed apply_patch update"]
    fn export_strobe_fixture() {
        println!(
            "F5_STROBE={}",
            serde_json::to_string(&build().to_pretty_ron().unwrap()).unwrap()
        );
        println!(
            "F5_STROBE_MATERIAL={}",
            serde_json::to_string(&program().to_pretty_ron().unwrap()).unwrap()
        );
    }

    #[test]
    fn strobe_assets_resolve_and_preserve_the_authored_gate_and_live_cohort() {
        let shell = effect();
        assert_eq!(
            shell.to_pretty_ron().unwrap(),
            build().to_pretty_ron().unwrap()
        );
        let index = aestra_project::ProjectAssetIndex::scan(super::super::viewer_asset_root(None));
        let project = index.resolve_effect_project(&shell).unwrap();
        assert_eq!(
            project.material_programs[&MaterialProgramId::from_u128(BASE + 1000)].normalized(),
            program().normalized()
        );
        let compiled = aestra_bevy::EffectCompiler::default()
            .compile_resolved_project(&project)
            .unwrap();
        let instance =
            aestra_bevy::EffectInstance::with_seed(compiled.root, super::super::fireworks_f0::SEED)
                .with_history_policy(aestra_bevy::PlaybackHistoryPolicy::PlaybackOnly);
        assert_eq!(
            aestra_gpu::GpuEffectArtifact::from_instance(&instance)
                .unwrap()
                .emitters
                .len(),
            5
        );
        // Runtime rendering consumes compiler-expanded custom calls, without
        // needing the author's function library a second time.
        for program in &instance.effect().material_programs {
            aestra_bevy::compile_material_program(program).unwrap();
        }
        assert_eq!(shell.emitters[1].renderers.len(), 1);
        assert!(matches!(
            shell.emitters[1].renderers[0].properties,
            RendererProperties::Sprite
        ));
        assert_eq!(shell.events[0].count, 192);
        assert_eq!(
            shell.emitters.iter().map(|e| e.max_particles).sum::<u32>(),
            306
        );
    }

    #[test]
    fn strobe_cli_keeps_interactive_wall_clock_and_benchmark_fixed_ticks() {
        let mut config = super::super::ViewerConfig::from_iter(
            [
                "--fireworks-f0",
                "--fireworks-f0-probe",
                "f5-strobe",
                "--backend",
                "gpu",
                "--semantic-materials",
                "--history",
                "playback-only",
            ]
            .into_iter()
            .map(str::to_owned),
        )
        .unwrap();
        assert_eq!(
            super::super::prepare_viewer(&config)
                .unwrap_or_else(|e| panic!("{}", e.message))
                .compiled
                .name,
            "Fireworks Strobe"
        );
        assert!(config.probe_bench_step().is_none());
        config.gpu_bench = Some("unused.json".into());
        assert!(config.probe_bench_step().is_some());
    }
}
