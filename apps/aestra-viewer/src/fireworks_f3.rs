//! Editable F3 shell prototypes. These are not Test A certification workloads.
use aestra_bevy::EffectAsset;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Probe {
    Peony,
    Chrysanthemum,
    Pistil,
    Willow,
}

impl Probe {
    pub const ALL: [Self; 4] = [Self::Peony, Self::Chrysanthemum, Self::Pistil, Self::Willow];

    pub fn name(self) -> &'static str {
        match self {
            Self::Peony => "f3-peony",
            Self::Chrysanthemum => "f3-chrysanthemum",
            Self::Pistil => "f3-pistil",
            Self::Willow => "f3-willow",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|probe| probe.name() == value)
    }
}

pub fn effect(probe: Probe) -> EffectAsset {
    EffectAsset::from_ron(match probe {
        Probe::Peony => include_str!("../../../assets/test/effects/fireworks_peony.aestra.ron"),
        Probe::Chrysanthemum => {
            include_str!("../../../assets/test/effects/fireworks_chrysanthemum.aestra.ron")
        }
        Probe::Pistil => include_str!("../../../assets/test/effects/fireworks_pistil.aestra.ron"),
        Probe::Willow => include_str!("../../../assets/test/effects/fireworks_willow.aestra.ron"),
    })
    .expect("checked-in F3 prototype must parse")
}

#[cfg(test)]
mod authored {
    use super::*;
    use aestra_bevy::material::{
        MaterialDomain, MaterialExpression, MaterialExpressionKind as Expr, MaterialInput,
        MaterialInstance, MaterialOutputs, MaterialProgram, MaterialProgramRef,
        MaterialRenderState, MaterialRenderStatePolicy, MaterialValue, MaterialVectorComponent,
    };
    use aestra_bevy::{
        BlendMode, ColorKey, Curve, CurveKey, EffectId, EffectParameter, EffectPlaybackMode,
        Emitter, EmitterShape, EventId, EventLink, EventTrigger, Gradient, MaterialExpressionId,
        MaterialId, MaterialProgramId, ModuleInstance, ParameterId, RendererInstance,
        RendererProperties, RendererTypeId, ScalarRange, TrailEndCap, TrailSamplingMode,
        TrailUvMode, Value, VelocityDistribution,
    };
    use std::collections::BTreeMap;

    const BASE: u128 = 0xa3574a00_0000_4000_8000_0000000f3000;
    const STAR: MaterialId = MaterialId::from_u128(BASE + 9001);
    const TAIL: MaterialId = MaterialId::from_u128(BASE + 9002);
    const SMOKE: MaterialId = MaterialId::from_u128(BASE + 9003);

    fn curve(keys: &[(f32, f32)]) -> Curve {
        Curve::new(keys.iter().map(|&(t, v)| CurveKey::new(t, v)).collect())
    }

    #[allow(clippy::too_many_arguments)]
    fn emitter(
        name: &str,
        capacity: u32,
        rate: f32,
        burst: u32,
        lifetime: ScalarRange,
        speed: ScalarRange,
        mode: VelocityDistribution,
        angle: f32,
        drag: f32,
        size: &[(f32, f32)],
        opacity: &[(f32, f32)],
        colors: &[(f32, [f32; 4])],
        material: MaterialId,
        base: u128,
    ) -> Emitter {
        let mut emitter = Emitter::basic_sprite(name, 7.0);
        emitter.max_particles = capacity;
        emitter.modules = vec![
            ModuleInstance::emission(rate, burst),
            ModuleInstance::shape(EmitterShape::Point),
            ModuleInstance::initialize_with_distribution(
                lifetime,
                speed,
                mode,
                [0.0, 1.0, 0.0],
                angle,
                ScalarRange::new(0.0, 0.0),
            ),
            ModuleInstance::motion([0.0, -9.81, 0.0], drag, 0.0),
            ModuleInstance::appearance(
                curve(size),
                curve(opacity),
                Gradient::new(colors.iter().map(|&(t, c)| ColorKey::new(t, c)).collect()),
            ),
        ];
        emitter.renderers = vec![RendererInstance::sprite(material)];
        super::super::fireworks_f0::fix_emitter_ids(&mut emitter, base);
        emitter
    }

    fn trail(emitter: &mut Emitter, width: f32, lifetime: f32, capacity: u32) {
        emitter.renderers.push(RendererInstance {
            id: aestra_bevy::RendererId::from_u128(emitter.id.as_uuid().as_u128() + 61),
            renderer_type: RendererTypeId::new(aestra_bevy::RENDERER_TRAIL),
            enabled: true,
            material: TAIL,
            properties: RendererProperties::Trail {
                width,
                sample_interval: 1.0 / 60.0,
                lifetime,
                max_points: 64,
                max_trails: capacity,
                sampling: TrailSamplingMode::Distance,
                sample_distance: 0.25,
                curve_tolerance: 0.01,
                uv_mode: TrailUvMode::Stretch,
                tile_length: 1.0,
                end_cap: TrailEndCap::Flat,
            },
            label: Some("Cooling spark history".into()),
            schema_version: None,
        });
    }

    pub fn build(probe: Probe) -> EffectAsset {
        let base = BASE + 1000 * (1 + probe as u128);
        let willow = probe == Probe::Willow;
        let duration = if willow { 9.0 } else { 7.0 };
        let mut effect = EffectAsset::new(
            format!(
                "Fireworks {} Prototype",
                match probe {
                    Probe::Peony => "Peony",
                    Probe::Chrysanthemum => "Chrysanthemum",
                    Probe::Pistil => "Pistil",
                    Probe::Willow => "Willow",
                }
            ),
            duration,
        );
        effect.id = EffectId::from_u128(base);
        effect.playback_mode = EffectPlaybackMode::Once;
        effect.materials.clear();
        for (index, id) in [STAR, TAIL, SMOKE].into_iter().enumerate() {
            effect.material_instances.push(MaterialInstance {
                id,
                program: MaterialProgramRef::Project(MaterialProgramId::from_u128(
                    BASE + 8000 + index as u128 * 100,
                )),
                values: BTreeMap::new(),
                render_state: MaterialRenderState {
                    blend: if id == SMOKE {
                        BlendMode::Alpha
                    } else {
                        BlendMode::Additive
                    },
                    ..MaterialRenderState::additive_sprite()
                },
            });
        }
        let warm = [
            (0.0, [1.0, 0.95, 0.7, 1.0]),
            (0.3, [1.0, 0.6, 0.15, 1.0]),
            (1.0, [0.55, 0.08, 0.01, 1.0]),
        ];
        let star_color = match probe {
            Probe::Chrysanthemum | Probe::Willow => warm,
            Probe::Peony => [
                (0.0, [1.0, 0.85, 0.8, 1.0]),
                (0.25, [0.85, 0.04, 0.09, 1.0]),
                (1.0, [0.3, 0.01, 0.02, 1.0]),
            ],
            Probe::Pistil => [
                (0.0, [0.85, 0.95, 1.0, 1.0]),
                (0.25, [0.08, 0.3, 1.0, 1.0]),
                (1.0, [0.015, 0.06, 0.3, 1.0]),
            ],
        };
        let mut launch = emitter(
            "Launch shell",
            1,
            0.0,
            1,
            ScalarRange::new(1.3, 1.3),
            ScalarRange::new(42.0, 42.0),
            VelocityDistribution::Constant,
            0.0,
            0.08,
            &[(0.0, 0.5), (0.85, 0.35), (1.0, 0.05)],
            &[(0.0, 1.0), (1.0, 0.7)],
            &warm,
            STAR,
            base + 100,
        );
        trail(&mut launch, 0.8, 0.35, 1);
        let mut stars = emitter(
            "Main stars",
            256,
            0.0,
            0,
            if willow {
                ScalarRange::new(4.2, 5.0)
            } else {
                ScalarRange::new(2.4, 3.1)
            },
            if willow {
                ScalarRange::new(14.0, 18.0)
            } else {
                ScalarRange::new(18.0, 22.0)
            },
            VelocityDistribution::Sphere,
            0.0,
            if willow { 0.75 } else { 0.55 },
            &[(0.0, 0.55), (0.7, 0.35), (1.0, 0.03)],
            &[(0.0, 1.0), (0.55, 0.85), (1.0, 0.0)],
            &star_color,
            STAR,
            base + 200,
        );
        if probe == Probe::Chrysanthemum {
            trail(&mut stars, 0.5, 0.8, 256);
        }
        if willow {
            trail(&mut stars, 0.5, 1.4, 256);
            // Bound long drooping histories by time, not distance: 64 points cover
            // more than the 1.4-second visible tail, including one boundary sample.
            if let RendererProperties::Trail {
                sampling,
                sample_interval,
                ..
            } = &mut stars.renderers[1].properties
            {
                *sampling = TrailSamplingMode::Time;
                *sample_interval = 1.0 / 30.0;
            }
            if let aestra_bevy::ModuleParameters::Motion { gravity, .. } =
                &mut stars.modules[3].parameters
            {
                *gravity = [0.0, -7.0, 0.0];
            }
        }
        let flash = emitter(
            "Burst flash",
            1,
            0.0,
            0,
            ScalarRange::new(0.14, 0.14),
            ScalarRange::new(0.0, 0.0),
            VelocityDistribution::Constant,
            0.0,
            0.0,
            &[(0.0, 9.0), (1.0, 0.2)],
            &[(0.0, 0.9), (1.0, 0.0)],
            &[(0.0, [1.0, 0.9, 0.7, 1.0]), (1.0, [1.0, 0.4, 0.1, 1.0])],
            STAR,
            base + 300,
        );
        let smoke_colors = [
            (0.0, [0.1, 0.12, 0.15, 1.0]),
            (1.0, [0.055, 0.065, 0.08, 1.0]),
        ];
        let mut launch_smoke = emitter(
            "Launch smoke",
            64,
            24.0,
            0,
            ScalarRange::new(1.1, 1.5),
            ScalarRange::new(30.0, 36.0),
            VelocityDistribution::Cone,
            4.0,
            0.8,
            &[(0.0, 0.6), (1.0, 4.0)],
            &[(0.0, 0.0), (0.15, 0.18), (1.0, 0.0)],
            &smoke_colors,
            SMOKE,
            base + 400,
        );
        // An authored short plume, not attachment to live rocket positions.
        launch_smoke.duration = 1.3;
        let mut burst_smoke = emitter(
            "Burst smoke",
            24,
            0.0,
            0,
            ScalarRange::new(3.0, 3.8),
            ScalarRange::new(2.0, 4.0),
            VelocityDistribution::Sphere,
            0.0,
            1.2,
            &[(0.0, 1.0), (0.4, 5.0), (1.0, 8.0)],
            &[(0.0, 0.0), (0.15, 0.12), (1.0, 0.0)],
            &smoke_colors,
            SMOKE,
            base + 500,
        );
        if let aestra_bevy::ModuleParameters::Motion { gravity, .. } =
            &mut burst_smoke.modules[3].parameters
        {
            *gravity = [0.5, 0.25, 0.0];
        }
        for (index, target, count, inherit) in [
            (0, stars.id, 256, 0.02),
            (1, flash.id, 1, 0.0),
            (2, burst_smoke.id, 24, 0.04),
        ] {
            let mut link = EventLink::new(launch.id, EventTrigger::OnDeath, target);
            link.id = EventId::from_u128(base + 700 + index);
            link.count = count;
            link.inherit_velocity = inherit;
            effect.events.push(link);
        }
        // Keep live host controls generic: no shell-specific runtime types.
        for (index, name, default, module, input) in [
            (
                0,
                "Launch speed",
                Value::Range(ScalarRange::new(42.0, 42.0)),
                &mut launch.modules[2],
                "speed",
            ),
            (
                1,
                "Star speed",
                Value::Range(if willow {
                    ScalarRange::new(14.0, 18.0)
                } else {
                    ScalarRange::new(18.0, 22.0)
                }),
                &mut stars.modules[2],
                "speed",
            ),
        ] {
            let id = ParameterId::from_u128(base + 50 + index);
            effect.parameters.push(EffectParameter {
                id,
                name: name.into(),
                default,
                exposed: true,
            });
            module.bindings.insert(input.into(), id);
        }
        effect.emitters = vec![launch, stars, flash, launch_smoke, burst_smoke];
        if probe == Probe::Pistil {
            let mut inner = emitter(
                "Pistil stars",
                96,
                0.0,
                0,
                ScalarRange::new(2.1, 2.7),
                ScalarRange::new(8.0, 10.0),
                VelocityDistribution::Sphere,
                0.0,
                0.55,
                &[(0.0, 0.55), (0.7, 0.35), (1.0, 0.03)],
                &[(0.0, 1.0), (0.55, 0.85), (1.0, 0.0)],
                &warm,
                STAR,
                base + 600,
            );
            let id = ParameterId::from_u128(base + 52);
            effect.parameters.push(EffectParameter {
                id,
                name: "Pistil speed".into(),
                default: Value::Range(ScalarRange::new(8.0, 10.0)),
                exposed: true,
            });
            inner.modules[2].bindings.insert("speed".into(), id);
            let mut link = EventLink::new(effect.emitters[0].id, EventTrigger::OnDeath, inner.id);
            link.id = EventId::from_u128(base + 703);
            link.count = 96;
            link.inherit_velocity = 0.02;
            effect.events.push(link);
            effect.emitters.push(inner);
        }
        // Include the long Willow decay and retired trail history in the root
        // playback window; keep the launch plume's intentionally short window.
        for emitter in &mut effect.emitters {
            if emitter.name != "Launch smoke" {
                emitter.duration = duration;
            }
        }
        effect.metadata.insert(
            "status".into(),
            "F3 visual prototype; not Test A performance certification".into(),
        );
        effect.metadata.insert(
            "preview_seed".into(),
            super::super::fireworks_f0::SEED.to_string(),
        );
        effect.metadata.insert("notes".into(), "Generic death links drive stars/flash/smoke. Launch plume is an independent authored approximation. HDR/bloom, volumetric lighting and production scale remain later gates.".into());
        effect
    }

    pub fn programs() -> Vec<MaterialProgram> {
        (0..3)
            .map(|index| {
                let base = BASE + 8000 + index * 100;
                let id = |n| MaterialExpressionId::from_u128(base + n);
                let ribbon = index == 1;
                let mut program = MaterialProgram::additive_sprite(
                    [
                        "Fireworks Star Core",
                        "Fireworks Cooling Trail",
                        "Fireworks Particle Smoke",
                    ][index as usize],
                );
                program.id = MaterialProgramId::from_u128(base);
                program.domain = if ribbon {
                    MaterialDomain::Ribbon
                } else {
                    MaterialDomain::Sprite
                };
                let state = MaterialRenderState {
                    blend: if index == 2 {
                        BlendMode::Alpha
                    } else {
                        BlendMode::Additive
                    },
                    ..MaterialRenderState::additive_sprite()
                };
                program.render_state_policy = MaterialRenderStatePolicy::fixed(state);
                let mut expressions = vec![
                    (1, Expr::Input(MaterialInput::ParticleColor)),
                    (2, Expr::Input(MaterialInput::ParticleOpacity)),
                    (
                        3,
                        Expr::Input(if ribbon {
                            MaterialInput::RibbonUv
                        } else {
                            MaterialInput::Uv0
                        }),
                    ),
                    (4, Expr::Constant(MaterialValue::Float(0.0))),
                    (5, Expr::Constant(MaterialValue::Float(1.0))),
                ];
                if ribbon {
                    expressions.extend([
                        (
                            6,
                            Expr::ExtractComponent {
                                value: id(3),
                                component: MaterialVectorComponent::Y,
                            },
                        ),
                        (7, Expr::Constant(MaterialValue::Float(0.5))),
                        (8, Expr::Subtract(id(6), id(7))),
                        (9, Expr::Multiply(id(8), id(8))),
                        (10, Expr::Constant(MaterialValue::Float(0.25))),
                        (
                            11,
                            Expr::Smoothstep {
                                edge_min: id(4),
                                edge_max: id(10),
                                value: id(9),
                            },
                        ),
                        (12, Expr::Subtract(id(5), id(11))),
                    ]);
                } else {
                    expressions.extend([
                        (6, Expr::Constant(MaterialValue::Vec2([0.5, 0.5]))),
                        (7, Expr::Constant(MaterialValue::Float(0.45))),
                        (
                            8,
                            Expr::Constant(MaterialValue::Float(if index == 2 {
                                0.5
                            } else {
                                0.2
                            })),
                        ),
                        (9, Expr::Constant(MaterialValue::Bool(false))),
                        (
                            12,
                            Expr::RadialMask {
                                uv: id(3),
                                center: id(6),
                                radius: id(7),
                                softness: id(8),
                                invert: id(9),
                            },
                        ),
                    ]);
                }
                expressions.push((13, Expr::Multiply(id(2), id(12))));
                program.expressions = expressions
                    .into_iter()
                    .map(|(n, kind)| MaterialExpression { id: id(n), kind })
                    .collect();
                program.outputs = MaterialOutputs {
                    color: id(1),
                    alpha: id(13),
                    vertex_offset: None,
                };
                program
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f3_prototypes_compile_with_single_real_death_bursts_and_semantic_materials() {
        let programs: std::collections::BTreeMap<_, _> = authored::programs()
            .into_iter()
            .map(|p| (p.id, p))
            .collect();
        for probe in Probe::ALL {
            let effect = effect(probe);
            assert_eq!(
                effect.to_pretty_ron().unwrap(),
                authored::build(probe).to_pretty_ron().unwrap()
            );
            let compiled = aestra_bevy::EffectCompiler::default()
                .compile_with_material_programs(&effect, &programs)
                .unwrap();
            assert_eq!(
                compiled.emitters.len(),
                if probe == Probe::Pistil { 6 } else { 5 }
            );
            assert_eq!(
                effect.events.len(),
                if probe == Probe::Pistil { 4 } else { 3 }
            );
            assert_eq!(effect.events[0].count, 256);
            assert!(
                effect
                    .events
                    .iter()
                    .all(|e| e.source == effect.emitters[0].id
                        && e.trigger == aestra_bevy::EventTrigger::OnDeath)
            );
            // Every event cohort has its own bounded target pool, and none of
            // those targets can independently emit a duplicate cohort.
            for link in &effect.events {
                let target = effect
                    .emitters
                    .iter()
                    .find(|e| e.id == link.target)
                    .unwrap();
                assert!(link.count <= target.max_particles);
                assert!(matches!(
                    target.modules[0].parameters,
                    aestra_bevy::ModuleParameters::Emission {
                        spawn_rate: 0.0,
                        burst_count: 0
                    }
                ));
            }
            for program in programs.values() {
                aestra_bevy::compile_material_program(program).unwrap();
            }
        }
    }

    #[test]
    fn pistil_layers_share_a_death_origin_but_keep_distinct_speeds_and_colors() {
        use aestra_bevy::{ModuleParameters, Value, VelocityDistribution};
        let effect = effect(Probe::Pistil);
        let outer = &effect.emitters[1];
        let inner = &effect.emitters[5];
        let mut speeds = Vec::new();
        for emitter in [outer, inner] {
            let ModuleParameters::Initialize {
                speed,
                velocity_distribution,
                ..
            } = emitter.modules[2].parameters
            else {
                panic!("star initialization missing")
            };
            assert_eq!(velocity_distribution, VelocityDistribution::Sphere);
            speeds.push(speed);
            let parameter = effect
                .parameters
                .iter()
                .find(|p| p.id == emitter.modules[2].bindings["speed"])
                .unwrap();
            assert!(parameter.exposed);
            assert_eq!(parameter.default, Value::Range(speed));
        }
        assert!(speeds[1].max < speeds[0].min);
        assert_eq!(effect.events[0].source, effect.events[3].source);
        assert_eq!(
            effect.events[0].inherit_velocity,
            effect.events[3].inherit_velocity
        );
        assert_eq!(effect.events[3].count, 96);
        let ModuleParameters::Appearance {
            color: outer_color, ..
        } = &outer.modules[4].parameters
        else {
            panic!("outer appearance missing")
        };
        let ModuleParameters::Appearance {
            color: inner_color, ..
        } = &inner.modules[4].parameters
        else {
            panic!("inner appearance missing")
        };
        assert_ne!(outer_color.keys, inner_color.keys);
    }

    #[test]
    fn willow_history_and_playback_window_cover_the_full_decay_with_bounded_storage() {
        use aestra_bevy::{ModuleParameters, RendererProperties, TrailSamplingMode};
        let effect = effect(Probe::Willow);
        let launch = &effect.emitters[0];
        let stars = &effect.emitters[1];
        let ModuleParameters::Initialize {
            lifetime: launch_life,
            ..
        } = launch.modules[2].parameters
        else {
            panic!("launch initialization missing")
        };
        let ModuleParameters::Initialize {
            lifetime: star_life,
            ..
        } = stars.modules[2].parameters
        else {
            panic!("star initialization missing")
        };
        let RendererProperties::Trail {
            sampling,
            sample_interval,
            lifetime,
            max_points,
            max_trails,
            ..
        } = stars.renderers[1].properties
        else {
            panic!("Willow trail missing")
        };
        assert_eq!(sampling, TrailSamplingMode::Time);
        assert_eq!(max_trails, stars.max_particles);
        assert_eq!(max_points, 64);
        assert!(sample_interval > 0.0);
        assert!((max_points - 1) as f32 * sample_interval > lifetime);
        assert!(star_life.min > 4.0 && lifetime > 1.0);
        // Include a fixed-tick margin for the last death and retained history.
        assert!(launch_life.max + star_life.max + lifetime + 1.0 / 60.0 < effect.duration);
        assert_eq!(stars.duration, effect.duration);
        assert_eq!(effect.emitters[3].duration, 1.3);
        assert!(
            matches!(stars.modules[3].parameters, ModuleParameters::Motion { gravity, drag, .. } if gravity[1] < 0.0 && drag > 0.0)
        );
    }

    #[test]
    fn f3_project_assets_resolve_without_viewer_injected_materials() {
        let index = aestra_project::ProjectAssetIndex::scan(super::super::viewer_asset_root(None));
        for probe in Probe::ALL {
            let source = effect(probe);
            let resolved = index.resolve_effect_project(&source).unwrap();
            assert_eq!(resolved.material_programs.len(), 3);
            let compiled = aestra_bevy::EffectCompiler::default()
                .compile_resolved_project(&resolved)
                .unwrap();
            assert_eq!(
                compiled.root.emitters.len(),
                if probe == Probe::Pistil { 6 } else { 5 }
            );
            for expected in authored::programs() {
                assert_eq!(
                    expected.normalized(),
                    resolved.material_programs[&expected.id].normalized()
                );
            }
        }
    }

    #[test]
    fn f3_cli_uses_editable_assets_with_playback_only_history_when_requested() {
        for probe in Probe::ALL {
            let config = super::super::ViewerConfig::from_iter(
                [
                    "--fireworks-f0",
                    "--fireworks-f0-probe",
                    probe.name(),
                    "--history",
                    "playback-only",
                ]
                .into_iter()
                .map(str::to_owned),
            )
            .unwrap();
            assert_eq!(
                config.fireworks_probe,
                Some(super::super::FireworksProbe::Shell(probe))
            );
            assert_eq!(
                config.history_policy,
                aestra_bevy::PlaybackHistoryPolicy::PlaybackOnly
            );
            assert_eq!(
                super::super::prepare_viewer(&config)
                    .unwrap_or_else(|error| panic!("{}", error.message))
                    .compiled
                    .name,
                effect(probe).name
            );
        }
    }

    #[test]
    #[ignore = "prints fixture source for a reviewed apply_patch update"]
    fn export_f3_fixture_sources() {
        let mut files = std::collections::BTreeMap::new();
        for probe in Probe::ALL {
            files.insert(
                format!(
                    "assets/test/effects/fireworks_{}.aestra.ron",
                    probe.name().trim_start_matches("f3-")
                ),
                authored::build(probe).to_pretty_ron().unwrap(),
            );
        }
        for (program, name) in authored::programs()
            .into_iter()
            .zip(["star", "trail", "smoke"])
        {
            files.insert(
                format!("assets/test/materials/fireworks_{name}.aestra.material.ron"),
                program.to_pretty_ron().unwrap(),
            );
        }
        println!("F3_FIXTURES={}", serde_json::to_string(&files).unwrap());
    }
}
