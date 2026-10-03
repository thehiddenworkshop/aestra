//! Editable bounded secondary shells: multi-break, crackle and four-arm crossette.
use aestra_bevy::EffectAsset;

pub fn effect() -> EffectAsset {
    EffectAsset::from_ron(include_str!(
        "../../../assets/test/effects/fireworks_multi_break.aestra.ron"
    ))
    .expect("checked-in multi-break shell must parse")
}

pub fn crackle_effect() -> EffectAsset {
    EffectAsset::from_ron(include_str!(
        "../../../assets/test/effects/fireworks_crackle.aestra.ron"
    ))
    .expect("checked-in crackle shell must parse")
}

pub fn crossette_effect() -> EffectAsset {
    EffectAsset::from_ron(include_str!(
        "../../../assets/test/effects/fireworks_crossette.aestra.ron"
    ))
    .expect("checked-in crossette shell must parse")
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
        build_variant(false)
    }

    fn build_crossette() -> EffectAsset {
        let base = BASE + 0x2000;
        let mut shell = build();
        shell.particle_budgets.clear();
        shell.id = EffectId::from_u128(base);
        shell.name = "Fireworks Crossette".into();
        shell.emitters[1].max_particles = 32;
        if let ModuleParameters::Initialize { lifetime, .. } =
            &mut shell.emitters[1].modules[2].parameters
        {
            *lifetime = ScalarRange::new(0.85, 0.95);
        }
        shell.emitters.truncate(5);
        // Each link requests ONE arm per actual parent death. Four spherical
        // samples cannot guarantee a cross; four constant directions can.
        // This plane is effect-local XY, not aligned to each parent's heading.
        let template = build().emitters[5].clone();
        for (name, direction) in [
            ("Crossette NE", [1.0, 1.0, 0.0]),
            ("Crossette NW", [-1.0, 1.0, 0.0]),
            ("Crossette SW", [-1.0, -1.0, 0.0]),
            ("Crossette SE", [1.0, -1.0, 0.0]),
        ] {
            let mut arm = template.clone();
            arm.name = name.into();
            arm.max_particles = 32;
            if let ModuleParameters::Initialize {
                lifetime,
                speed,
                direction: axis,
                spread_degrees,
                velocity_distribution,
                ..
            } = &mut arm.modules[2].parameters
            {
                *lifetime = ScalarRange::new(0.75, 0.75);
                *speed = ScalarRange::new(10.0, 10.0);
                *axis = direction;
                *spread_degrees = 0.0;
                *velocity_distribution = aestra_bevy::VelocityDistribution::Constant;
            }
            if let ModuleParameters::Motion { drag, .. } = &mut arm.modules[3].parameters {
                *drag = 0.25;
            }
            shell.emitters.push(arm);
        }
        // New stable identities, with one shared exposed arm-color control.
        for (i, parameter) in shell.parameters.iter_mut().enumerate() {
            let old = parameter.id;
            parameter.id = ParameterId::from_u128(base + 50 + i as u128);
            if let Value::Gradient(color) = &mut parameter.default {
                color.id = GradientId::from_u128(base + 800 + i as u128);
            }
            if parameter.name == "Secondary sparks color" {
                parameter.name = "Crossette arms color".into();
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
            super::super::fireworks_f0::fix_emitter_ids(emitter, base + 0x10000 + i as u128 * 100);
            for renderer in &mut emitter.renderers {
                if let RendererProperties::Trail {
                    max_trails,
                    lifetime,
                    width,
                    ..
                } = &mut renderer.properties
                {
                    *max_trails = emitter.max_particles;
                    if i >= 5 {
                        *lifetime = 0.5;
                        *width = 0.18;
                    }
                }
            }
        }
        shell.events.clear();
        for (i, source, target, count, inherit) in [
            (0, 0, 1, 32, 0.02),
            (1, 0, 2, 1, 0.02),
            (2, 0, 4, 48, 0.02),
            (3, 1, 5, 1, 0.3),
            (4, 1, 6, 1, 0.3),
            (5, 1, 7, 1, 0.3),
            (6, 1, 8, 1, 0.3),
        ] {
            let mut link = EventLink::new(
                shell.emitters[source].id,
                EventTrigger::OnDeath,
                shell.emitters[target].id,
            );
            link.id = EventId::from_u128(base + 850 + i);
            link.count = count;
            link.inherit_velocity = inherit;
            shell.events.push(link);
        }
        for (i, output) in shell.event_outputs.iter_mut().enumerate() {
            output.id = aestra_bevy::EventDefinitionId::from_u128(base + 900 + i as u128);
            if i == 2 {
                output.name = "crossette_split".into();
            }
            let route = &mut shell.particle_outputs[i];
            route.id = aestra_bevy::EventRouteId::from_u128(base + 910 + i as u128);
            route.source = shell.emitters[if i == 2 { 1 } else { 0 }].id;
            route.output = output.id;
        }
        shell.metadata.insert("status".into(), "F5D bounded four-arm crossette prototype; artistic and finale-scale acceptance pending".into());
        shell.metadata.insert("notes".into(), "One rocket feeds 32 stars. Each real star death feeds one child in each of four constant XY diagonal directions, sharing its birth position and 30% inherited velocity (128 arms total). Fixed effect-local plane, not parent-oriented; no random four-sample substitute or host timer. Reuses HDR materials, cooling trails and unlit smoke.".into());
        super::super::fireworks_budgets::author_profiles(&mut shell);
        shell
    }

    fn build_variant(crackle: bool) -> EffectAsset {
        let base = if crackle { BASE + 0x1000 } else { BASE };
        let mut shell = super::super::fireworks_hero::effect();
        shell.id = EffectId::from_u128(base);
        shell.name = if crackle {
            "Fireworks Crackle"
        } else {
            "Fireworks Multi-break"
        }
        .into();
        shell.duration = 7.0;
        shell.emitters.truncate(5);
        let parents = if crackle { 96 } else { 64 };
        shell.emitters[1].max_particles = parents;
        if let ModuleParameters::Initialize {
            lifetime, speed, ..
        } = &mut shell.emitters[1].modules[2].parameters
        {
            *lifetime = if crackle {
                ScalarRange::new(0.75, 1.05)
            } else {
                ScalarRange::new(1.2, 1.6)
            };
            *speed = ScalarRange::new(18.0, 22.0);
        }
        let mut secondary = shell.emitters[1].clone();
        secondary.name = if crackle {
            "Crackle sparks"
        } else {
            "Secondary sparks"
        }
        .into();
        secondary.max_particles = parents * if crackle { 12 } else { 8 };
        if let ModuleParameters::Initialize {
            lifetime, speed, ..
        } = &mut secondary.modules[2].parameters
        {
            *lifetime = if crackle {
                ScalarRange::new(0.08, 0.20)
            } else {
                ScalarRange::new(0.45, 0.85)
            };
            *speed = if crackle {
                ScalarRange::new(4.0, 8.0)
            } else {
                ScalarRange::new(3.0, 6.0)
            };
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
        if crackle {
            // A dark, slowly drifting burning carrier adds a real seeded delay
            // before each impulsive pop. No age trigger or independent timer.
            let mut carrier = secondary.clone();
            carrier.name = "Burning carriers".into();
            carrier.max_particles = parents;
            if let ModuleParameters::Initialize {
                lifetime, speed, ..
            } = &mut carrier.modules[2].parameters
            {
                *lifetime = ScalarRange::new(0.12, 0.35);
                *speed = ScalarRange::new(0.8, 1.8);
            }
            carrier.modules[4].parameters = ModuleParameters::Appearance {
                size: Curve::new(vec![CurveKey::new(0.0, 0.10), CurveKey::new(1.0, 0.03)]),
                opacity: Curve::new(vec![CurveKey::new(0.0, 0.25), CurveKey::new(1.0, 0.05)]),
                color: Gradient::new(vec![
                    ColorKey::new(0.0, [0.5, 0.1, 0.005, 1.0]),
                    ColorKey::new(1.0, [0.1, 0.01, 0.001, 1.0]),
                ]),
            };
            carrier
                .renderers
                .retain(|r| matches!(r.properties, RendererProperties::Sprite));
            secondary
                .renderers
                .retain(|r| matches!(r.properties, RendererProperties::Sprite));
            // Bright, brief cooling, no long secondary trails masquerading as crackle.
            secondary.modules[4].parameters = ModuleParameters::Appearance {
                size: Curve::new(vec![
                    CurveKey::new(0.0, 0.48),
                    CurveKey::new(0.15, 0.28),
                    CurveKey::new(1.0, 0.02),
                ]),
                opacity: Curve::new(vec![
                    CurveKey::new(0.0, 1.0),
                    CurveKey::new(0.2, 0.9),
                    CurveKey::new(1.0, 0.0),
                ]),
                color: Gradient::new(vec![
                    ColorKey::new(0.0, [1.0, 0.98, 0.8, 1.0]),
                    ColorKey::new(0.25, [1.0, 0.5, 0.03, 1.0]),
                    ColorKey::new(1.0, [0.12, 0.01, 0.001, 1.0]),
                ]),
            };
            shell.emitters.push(carrier);
        }
        shell.emitters.push(secondary);
        // Reuse the hero's material programs, but give this effect's local
        // parameters/emitters their own stable identities and color controls.
        shell.parameters.truncate(2);
        for (i, parameter) in shell.parameters.iter_mut().enumerate() {
            let old = parameter.id;
            parameter.id = ParameterId::from_u128(base + 50 + i as u128);
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
            super::super::fireworks_f0::fix_emitter_ids(emitter, base + 100 + i as u128 * 100);
            for module in &mut emitter.modules {
                module.bindings.clear();
            }
            let Value::Gradient(mut color) = emitter.modules[4].parameter_value("color").unwrap()
            else {
                unreachable!()
            };
            color.id = GradientId::from_u128(base + 800 + i as u128);
            let id = ParameterId::from_u128(base + 60 + i as u128);
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
        let mut links = vec![
            (0, 0, 1, parents, 0.02),
            (1, 0, 2, 1, 0.02),
            (2, 0, 4, 48, 0.02),
            (
                3,
                1,
                5,
                if crackle { 1 } else { 8 },
                if crackle { 0.85 } else { 0.25 },
            ),
        ];
        if crackle {
            links.push((4, 5, 6, 12, 0.15));
        }
        for (i, source, target, count, inherit) in links {
            let mut link = EventLink::new(
                shell.emitters[source].id,
                EventTrigger::OnDeath,
                shell.emitters[target].id,
            );
            link.id = EventId::from_u128(base + 850 + i);
            link.count = count;
            link.inherit_velocity = inherit;
            shell.events.push(link);
        }
        shell.event_outputs.clear();
        shell.particle_outputs.clear();
        for (i, name, emitter, trigger) in [
            (0, "launch", 0, EventTrigger::OnSpawn),
            (1, "main_break", 0, EventTrigger::OnDeath),
            (
                2,
                if crackle {
                    "crackle"
                } else {
                    "secondary_break"
                },
                if crackle { 5 } else { 1 },
                EventTrigger::OnDeath,
            ),
        ] {
            let mut definition = aestra_bevy::EventDefinition::new(name);
            definition.id = aestra_bevy::EventDefinitionId::from_u128(base + 900 + i);
            let mut route = aestra_bevy::ParticleOutputRoute::new(
                shell.emitters[emitter].id,
                trigger,
                definition.id,
            );
            route.id = aestra_bevy::EventRouteId::from_u128(base + 910 + i);
            // One representative position/count per tick, not 512 audio calls.
            shell.event_outputs.push(definition);
            shell.particle_outputs.push(route);
        }
        shell.metadata.insert(
            "status".into(),
            "F5 bounded two-generation prototype with generic host cues; artistic acceptance pending".into(),
        );
        shell.metadata.insert("notes".into(), "One rocket death feeds 64 stars; their deaths each feed eight short-lived secondary sparks at the real parent position, with 25% inherited velocity. Reuses hero materials and unlit smoke. Not crackle, strobe or an AAA-density certification.".into());
        if crackle {
            shell.metadata.insert("status".into(), "F5C bounded delayed-carrier crackle prototype; artistic and finale-scale acceptance pending".into());
            shell.metadata.insert("notes".into(), "One rocket feeds 96 stars; their deaths spawn one burning carrier each. Seeded carrier lifetimes delay 12 brief sprite-only sparks per real death (1152 total). Reuses HDR hero materials and unlit smoke; no audio timer or new trigger. Not AAA-density certification.".into());
        }
        super::super::fireworks_budgets::author_profiles(&mut shell);
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
    fn crackle_is_an_editable_three_generation_chain_with_bounded_impulsive_sparks() {
        let shell = crackle_effect();
        assert_eq!(
            shell.to_pretty_ron().unwrap(),
            build_variant(true).to_pretty_ron().unwrap()
        );
        let index = aestra_project::ProjectAssetIndex::scan(super::super::viewer_asset_root(None));
        let project = index.resolve_effect_project(&shell).unwrap();
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
            7
        );
        assert_eq!(
            shell.emitters.iter().map(|e| e.max_particles).sum::<u32>(),
            1458
        );
        assert_eq!(shell.events.len(), 5);
        for (link, source, target, count) in
            [(&shell.events[3], 1, 5, 1), (&shell.events[4], 5, 6, 12)]
        {
            assert_eq!(
                (link.source, link.target, link.count, link.trigger),
                (
                    shell.emitters[source].id,
                    shell.emitters[target].id,
                    count,
                    EventTrigger::OnDeath
                )
            );
        }
        for &i in &[1, 2, 4, 5, 6] {
            assert!(matches!(
                shell.emitters[i].modules[0].parameters,
                ModuleParameters::Emission {
                    spawn_rate: 0.0,
                    burst_count: 0
                }
            ));
        }
        for &i in &[5, 6] {
            assert_eq!(shell.emitters[i].renderers.len(), 1);
            assert!(matches!(
                shell.emitters[i].renderers[0].properties,
                RendererProperties::Sprite
            ));
        }
        assert!(matches!(shell.emitters[5].modules[2].parameters,
            ModuleParameters::Initialize { lifetime, .. } if lifetime.min == 0.12 && lifetime.max == 0.35));
        assert!(matches!(shell.emitters[6].modules[2].parameters,
            ModuleParameters::Initialize { lifetime, .. } if lifetime.max <= 0.20));
        let cue = shell
            .particle_outputs
            .iter()
            .find(|r| r.source == shell.emitters[5].id)
            .unwrap();
        assert_eq!(cue.trigger, EventTrigger::OnDeath);
        assert!(
            shell
                .event_outputs
                .iter()
                .any(|o| o.id == cue.output && o.name == "crackle")
        );
    }

    #[test]
    fn crackle_cli_compiles_live_playback_and_accepts_the_host_cue_check() {
        let config = super::super::ViewerConfig::from_iter(
            [
                "--fireworks-f0",
                "--fireworks-f0-probe",
                "f5-crackle",
                "--backend",
                "gpu",
                "--history",
                "playback-only",
                "--fireworks-cue-check",
                "unused-crackle-cues.json",
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
            "Fireworks Crackle"
        );
        let mut benchmark = config;
        benchmark.fireworks_cue_check = None;
        assert!(benchmark.probe_bench_step().is_none());
        benchmark.gpu_bench = Some("unused.json".into());
        assert!(benchmark.probe_bench_step().is_some());
    }

    #[test]
    #[ignore = "prints fixture source for a reviewed apply_patch update"]
    fn export_multi_break_fixture() {
        println!(
            "F5_FIXTURE={}",
            serde_json::to_string(&build().to_pretty_ron().unwrap()).unwrap()
        );
    }

    #[test]
    fn crossette_has_exactly_four_deterministic_arms_per_parent_and_compiles() {
        let shell = crossette_effect();
        assert_eq!(
            shell.to_pretty_ron().unwrap(),
            build_crossette().to_pretty_ron().unwrap()
        );
        let index = aestra_project::ProjectAssetIndex::scan(super::super::viewer_asset_root(None));
        let project = index.resolve_effect_project(&shell).unwrap();
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
            9
        );
        assert_eq!(
            shell.emitters.iter().map(|e| e.max_particles).sum::<u32>(),
            274
        );
        assert_eq!(shell.events.len(), 7);
        for (i, direction) in [
            [1.0, 1.0, 0.0],
            [-1.0, 1.0, 0.0],
            [-1.0, -1.0, 0.0],
            [1.0, -1.0, 0.0],
        ]
        .into_iter()
        .enumerate()
        {
            let arm = &shell.emitters[5 + i];
            let link = &shell.events[3 + i];
            assert_eq!(
                (
                    link.source,
                    link.target,
                    link.count,
                    link.trigger,
                    link.inherit_velocity
                ),
                (shell.emitters[1].id, arm.id, 1, EventTrigger::OnDeath, 0.3)
            );
            assert!(
                matches!(arm.modules[2].parameters, ModuleParameters::Initialize {
                direction: axis, speed, lifetime, spread_degrees: 0.0,
                velocity_distribution: aestra_bevy::VelocityDistribution::Constant, ..
            } if axis == direction && speed.min == 10.0 && speed.max == 10.0 && lifetime.min == 0.75 && lifetime.max == 0.75)
            );
        }
        for &i in &[1, 2, 4, 5, 6, 7, 8] {
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
        assert_eq!(shell.event_outputs[2].name, "crossette_split");
        assert_eq!(shell.particle_outputs[2].source, shell.emitters[1].id);
    }

    #[test]
    fn crossette_cli_supports_live_playback_capture_and_host_check() {
        let config = super::super::ViewerConfig::from_iter(
            [
                "--fireworks-f0",
                "--fireworks-f0-probe",
                "f5-crossette",
                "--backend",
                "gpu",
                "--history",
                "playback-only",
                "--fireworks-cue-check",
                "unused-crossette-cues.json",
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
            "Fireworks Crossette"
        );
        let mut benchmark = config;
        benchmark.fireworks_cue_check = None;
        assert!(benchmark.probe_bench_step().is_none());
        benchmark.gpu_bench = Some("unused.json".into());
        assert!(benchmark.probe_bench_step().is_some());
    }

    #[test]
    #[ignore = "prints fixture source for a reviewed apply_patch update"]
    fn export_crackle_fixture() {
        println!(
            "F5_CRACKLE={}",
            serde_json::to_string(&build_variant(true).to_pretty_ron().unwrap()).unwrap()
        );
    }

    #[test]
    #[ignore = "prints fixture source for a reviewed apply_patch update"]
    fn export_crossette_fixture() {
        println!(
            "F5_CROSSETTE={}",
            serde_json::to_string(&build_crossette().to_pretty_ron().unwrap()).unwrap()
        );
    }
}
