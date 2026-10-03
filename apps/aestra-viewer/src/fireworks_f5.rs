//! Editable, bounded secondary shell: rocket death -> stars -> secondary sparks.
use aestra_bevy::EffectAsset;

pub fn effect() -> EffectAsset {
    EffectAsset::from_ron(include_str!(
        "../../../assets/test/effects/fireworks_multi_break.aestra.ron"
    ))
    .expect("checked-in multi-break shell must parse")
}

#[cfg(test)]
mod tests {
    use super::*;
    use aestra_bevy::{
        ColorKey, Curve, CurveKey, EffectId, EffectParameter, EventId, EventLink, EventTrigger,
        Gradient, GradientId, ModuleParameters, ParameterId, RendererProperties, ScalarRange,
        Value,
    };

    const BASE: u128 = 0xa3574a00_0000_4000_8000_0000000f8000;

    fn build() -> EffectAsset {
        let mut shell = super::super::fireworks_hero::effect();
        shell.id = EffectId::from_u128(BASE);
        shell.name = "Fireworks Multi-break".into();
        shell.duration = 7.0;
        shell.emitters.truncate(5);
        shell.emitters[1].max_particles = 64;
        if let ModuleParameters::Initialize {
            lifetime, speed, ..
        } = &mut shell.emitters[1].modules[2].parameters
        {
            *lifetime = ScalarRange::new(1.2, 1.6);
            *speed = ScalarRange::new(18.0, 22.0);
        }
        let mut secondary = shell.emitters[1].clone();
        secondary.name = "Secondary sparks".into();
        secondary.max_particles = 512;
        if let ModuleParameters::Initialize {
            lifetime, speed, ..
        } = &mut secondary.modules[2].parameters
        {
            *lifetime = ScalarRange::new(0.45, 0.85);
            *speed = ScalarRange::new(3.0, 6.0);
        }
        if let ModuleParameters::Motion { drag, .. } = &mut secondary.modules[3].parameters {
            *drag = 0.6;
        }
        secondary.modules[4].parameters = ModuleParameters::Appearance {
            size: Curve::new(vec![CurveKey::new(0.0, 0.32), CurveKey::new(1.0, 0.04)]),
            opacity: Curve::new(vec![CurveKey::new(0.0, 1.0), CurveKey::new(1.0, 0.0)]),
            color: Gradient::new(vec![
                ColorKey::new(0.0, [1.0, 0.95, 0.7, 1.0]),
                ColorKey::new(0.3, [1.0, 0.55, 0.12, 1.0]),
                ColorKey::new(1.0, [0.12, 0.015, 0.002, 1.0]),
            ]),
        };
        shell.emitters.push(secondary);
        // Reuse the hero's material programs, but give this effect's local
        // parameters/emitters their own stable identities and color controls.
        shell.parameters.truncate(2);
        for (i, parameter) in shell.parameters.iter_mut().enumerate() {
            let old = parameter.id;
            parameter.id = ParameterId::from_u128(BASE + 50 + i as u128);
            for material in &mut shell.material_instances {
                for value in material.values.values_mut() {
                    if *value == aestra_bevy::material::MaterialParameterValue::EffectParameter(old)
                    {
                        *value = aestra_bevy::material::MaterialParameterValue::EffectParameter(
                            parameter.id,
                        );
                    }
                }
            }
        }
        for (i, emitter) in shell.emitters.iter_mut().enumerate() {
            if i != 3 {
                emitter.duration = shell.duration;
            }
            super::super::fireworks_f0::fix_emitter_ids(emitter, BASE + 100 + i as u128 * 100);
            for module in &mut emitter.modules {
                module.bindings.clear();
            }
            let Value::Gradient(mut color) = emitter.modules[4].parameter_value("color").unwrap()
            else {
                unreachable!()
            };
            color.id = GradientId::from_u128(BASE + 800 + i as u128);
            let id = ParameterId::from_u128(BASE + 60 + i as u128);
            shell.parameters.push(EffectParameter {
                id,
                name: format!("{} color", emitter.name),
                default: Value::Gradient(color),
                exposed: true,
            });
            emitter.modules[4].bindings.insert("color".into(), id);
            for renderer in &mut emitter.renderers {
                if let RendererProperties::Trail {
                    max_trails,
                    lifetime,
                    width,
                    ..
                } = &mut renderer.properties
                {
                    *max_trails = emitter.max_particles;
                    if i == 5 {
                        *lifetime = 0.35;
                        *width = 0.2;
                    }
                }
            }
        }
        shell.events.clear();
        for (i, source, target, count, inherit) in [
            (0, 0, 1, 64, 0.02),
            (1, 0, 2, 1, 0.02),
            (2, 0, 4, 48, 0.02),
            (3, 1, 5, 8, 0.25),
        ] {
            let mut link = EventLink::new(
                shell.emitters[source].id,
                EventTrigger::OnDeath,
                shell.emitters[target].id,
            );
            link.id = EventId::from_u128(BASE + 850 + i);
            link.count = count;
            link.inherit_velocity = inherit;
            shell.events.push(link);
        }
        shell.event_outputs.clear();
        shell.particle_outputs.clear();
        for (i, name, emitter, trigger) in [
            (0, "launch", 0, EventTrigger::OnSpawn),
            (1, "main_break", 0, EventTrigger::OnDeath),
            (2, "secondary_break", 1, EventTrigger::OnDeath),
        ] {
            let mut definition = aestra_bevy::EventDefinition::new(name);
            definition.id = aestra_bevy::EventDefinitionId::from_u128(BASE + 900 + i);
            let mut route = aestra_bevy::ParticleOutputRoute::new(
                shell.emitters[emitter].id,
                trigger,
                definition.id,
            );
            route.id = aestra_bevy::EventRouteId::from_u128(BASE + 910 + i);
            // One representative position/count per tick, not 512 audio calls.
            shell.event_outputs.push(definition);
            shell.particle_outputs.push(route);
        }
        shell.metadata.insert(
            "status".into(),
            "F5 bounded two-generation prototype with generic host cues; artistic acceptance pending".into(),
        );
        shell.metadata.insert("notes".into(), "One rocket death feeds 64 stars; their deaths each feed eight short-lived secondary sparks at the real parent position, with 25% inherited velocity. Reuses hero materials and unlit smoke. Not crackle, strobe or an AAA-density certification.".into());
        shell
    }

    #[test]
    fn multi_break_is_resolvable_and_its_two_generations_have_room_and_trail_history() {
        let shell = effect();
        assert_eq!(
            shell.to_pretty_ron().unwrap(),
            build().to_pretty_ron().unwrap()
        );
        let index = aestra_project::ProjectAssetIndex::scan(super::super::viewer_asset_root(None));
        let project = index.resolve_effect_project(&shell).unwrap();
        let compiled = aestra_bevy::EffectCompiler::default()
            .compile_resolved_project(&project)
            .unwrap();
        let instance =
            aestra_bevy::EffectInstance::with_seed(compiled.root, super::super::fireworks_f0::SEED)
                .with_history_policy(aestra_bevy::PlaybackHistoryPolicy::PlaybackOnly);
        let artifact = aestra_gpu::GpuEffectArtifact::from_instance(&instance).unwrap();
        assert_eq!(artifact.emitters.len(), 6);
        assert_eq!(shell.events.len(), 4);
        assert_eq!(shell.particle_outputs.len(), 3);
        assert!(
            shell
                .particle_outputs
                .iter()
                .all(|route| route.aggregation == aestra_bevy::EventAggregation::FirstPerTick)
        );
        let link = &shell.events[3];
        assert_eq!(link.source, shell.emitters[1].id);
        assert_eq!(link.target, shell.emitters[5].id);
        assert_eq!(link.trigger, EventTrigger::OnDeath);
        assert_eq!(link.inherit_velocity, 0.25);
        assert_eq!(
            shell.emitters[1].max_particles * link.count,
            shell.emitters[5].max_particles
        );
        for &i in &[1, 2, 4, 5] {
            assert!(matches!(
                shell.emitters[i].modules[0].parameters,
                ModuleParameters::Emission {
                    spawn_rate: 0.0,
                    burst_count: 0
                }
            ));
        }
        for emitter in &shell.emitters {
            for renderer in &emitter.renderers {
                if let RendererProperties::Trail {
                    max_trails,
                    lifetime,
                    sample_interval,
                    max_points,
                    ..
                } = renderer.properties
                {
                    assert_eq!(max_trails, emitter.max_particles);
                    assert!(lifetime / sample_interval + 2.0 <= max_points as f32);
                }
            }
        }
    }

    #[test]
    fn multi_break_cli_compiles_with_playback_only_and_records_fixed_bench_ticks() {
        let config = super::super::ViewerConfig::from_iter(
            [
                "--fireworks-f0",
                "--fireworks-f0-probe",
                "f5-multi-break",
                "--history",
                "playback-only",
                "--gpu-bench",
                "unused.json",
            ]
            .into_iter()
            .map(str::to_owned),
        )
        .unwrap();
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
        assert_eq!(
            config.probe_bench_step().unwrap(),
            std::time::Duration::from_secs_f64(1.0 / 60.0)
        );
        let presentation = serde_json::to_value(
            super::super::gpu_bench::BenchPresentation::from_config(&config),
        )
        .unwrap();
        assert!(
            presentation["fixed_simulation_step_seconds"]
                .as_f64()
                .unwrap()
                > 0.0
        );
        let mut interactive = config;
        interactive.gpu_bench = None;
        assert!(interactive.probe_bench_step().is_none());
    }

    #[test]
    #[ignore = "prints fixture source for a reviewed apply_patch update"]
    fn export_multi_break_fixture() {
        println!(
            "F5_FIXTURE={}",
            serde_json::to_string(&build().to_pretty_ron().unwrap()).unwrap()
        );
    }
}
