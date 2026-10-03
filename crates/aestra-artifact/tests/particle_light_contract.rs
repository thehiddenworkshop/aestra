//! F7C: persisted material-free sinks and deterministic read-only CPU candidates.
use aestra_artifact::{ArtifactError, CURRENT_ARTIFACT_VERSION, decode_effect, encode_effect};
use aestra_compiler::EffectCompiler;
use aestra_core::*;
use aestra_runtime::{
    EffectInstance, ParticleLightColorPlan, ParticleSample, QualityTier, RuntimeValue,
    SceneOutputPlanKind,
};
use std::sync::Arc;

fn source() -> EffectAsset {
    let mut effect = EffectAsset::new("generic luminous embers", 4.0);
    let mut emitter = Emitter::basic_sprite("embers", 4.0);
    emitter.renderers.clear();
    emitter
        .scene_outputs
        .push(SceneOutputInstance::particle_point_light(
            ParticlePointLightProperties::new(2_000.0, 12.0),
        ));
    effect.materials.clear();
    effect.emitters.push(emitter);
    effect
}

fn particle() -> ParticleSample {
    ParticleSample {
        emitter_index: 0,
        particle_index: 42,
        position: [1.0, 2.0, 3.0],
        color: [0.9, 0.2, 0.1, 0.0],
        size: 0.0,
        rotation: 0.0,
        normalized_age: 0.25,
    }
}

fn gradient(rgb: [f32; 3]) -> Value {
    Value::Gradient(Gradient::new(vec![ColorKey::new(
        0.0,
        [rgb[0], rgb[1], rgb[2], 1.0],
    )]))
}

fn properties(effect: &mut EffectAsset) -> &mut ParticlePointLightProperties {
    let SceneOutputProperties::ParticlePointLight(light) =
        &mut effect.emitters[0].scene_outputs[0].properties;
    light
}

#[test]
fn material_free_source_and_v7_artifact_round_trip_preserve_candidates() {
    let source = source();
    source.validate().unwrap();
    let text = source.to_pretty_ron().unwrap();
    let reloaded = EffectAsset::from_ron(&text).unwrap();
    assert_eq!(source, reloaded);
    let compiled = EffectCompiler::default().compile(&reloaded).unwrap();
    assert!(compiled.materials.is_empty());
    assert!(compiled.emitters[0].renderers.is_empty());
    assert!(compiled.point_lights.is_empty());
    assert!(compiled.event_routes.is_empty());
    let decoded = decode_effect(&encode_effect(&compiled).unwrap()).unwrap();
    assert_eq!(compiled, decoded);
    let sample = particle();
    let candidates: Vec<_> = compiled.particle_light_candidates(&sample, &[]).collect();
    assert_eq!(
        candidates,
        decoded
            .particle_light_candidates(&sample, &[])
            .collect::<Vec<_>>()
    );
    let candidate = candidates[0];
    assert_eq!(
        candidate.identity.output,
        source.emitters[0].scene_outputs[0].id
    );
    assert_eq!(candidate.identity.emitter, source.emitters[0].id);
    assert_eq!(
        candidate.identity.region,
        source.emitters[0].implicit_region_id()
    );
    assert_eq!(candidate.identity.particle_index, 42);
    assert_eq!(candidate.position, sample.position);
    assert_eq!(candidate.light.linear_color, [0.9, 0.2, 0.1]);
    assert_eq!(
        candidate.light.intensity_lumens, 1_687.5,
        "not multiplied by size/alpha"
    );
    assert_eq!(candidate.light.range, 12.0);
    assert_eq!(candidate.light.radius, 0.1);

    let mut old = source.clone();
    old.emitters[0].scene_outputs.clear();
    old.emitters[0]
        .renderers
        .push(RendererInstance::sprite(DEFAULT_SPRITE_MATERIAL_ID));
    let mut default_material = EffectAsset::new("default", 1.0);
    old.materials.append(&mut default_material.materials);
    let old_text = old.to_pretty_ron().unwrap();
    assert!(!old_text.contains("scene_outputs"));
    assert!(
        EffectAsset::from_ron(&old_text).unwrap().emitters[0]
            .scene_outputs
            .is_empty()
    );
    assert!(
        EffectCompiler::default().compile(&old).unwrap().emitters[0]
            .scene_outputs
            .is_empty()
    );
}

#[test]
fn gradient_only_parameter_reads_are_retained_and_live_overrides_fail_closed() {
    for exposed in [false, true] {
        let mut source = source();
        let parameter = EffectParameter {
            id: ParameterId::new(),
            name: "ember light color".into(),
            default: gradient([0.2, 0.4, 0.6]),
            exposed,
        };
        properties(&mut source).color_source =
            ParticleLightColorSource::GradientParameter(parameter.id);
        source.parameters.push(parameter);
        let compiled = EffectCompiler::default().compile(&source).unwrap();
        let compiled = decode_effect(&encode_effect(&compiled).unwrap()).unwrap();
        assert_eq!(compiled.parameters.len(), usize::from(exposed));
        let SceneOutputPlanKind::ParticlePointLight(plan) =
            &compiled.emitters[0].scene_outputs[0].kind;
        assert_eq!(
            matches!(plan.color, ParticleLightColorPlan::GradientParameter(_)),
            exposed
        );
        let mut values: Vec<_> = compiled
            .parameters
            .iter()
            .map(|p| p.default.clone())
            .collect();
        let sample = particle();
        assert_eq!(
            compiled
                .particle_light_candidates(&sample, &values)
                .next()
                .unwrap()
                .light
                .linear_color,
            [0.2, 0.4, 0.6]
        );
        if exposed {
            values[0] = RuntimeValue::compile(&gradient([0.8, 0.1, 0.5])).unwrap();
            assert_eq!(
                compiled
                    .particle_light_candidates(&sample, &values)
                    .next()
                    .unwrap()
                    .light
                    .linear_color,
                [0.8, 0.1, 0.5]
            );
            values[0] = RuntimeValue::Scalar(1.0);
            assert_eq!(
                compiled.particle_light_candidates(&sample, &values).count(),
                0
            );
            values[0] = RuntimeValue::compile(&gradient([2.0, 0.1, 0.5])).unwrap();
            assert_eq!(
                compiled.particle_light_candidates(&sample, &values).count(),
                0
            );
            assert_eq!(compiled.particle_light_candidates(&sample, &[]).count(), 0);
            values[0] = RuntimeValue::compile(&Value::Gradient(Gradient::new(Vec::new()))).unwrap();
            assert_eq!(
                compiled.particle_light_candidates(&sample, &values).count(),
                0
            );
        }
    }
}

#[test]
fn particle_age_gradient_and_curve_output_units_are_resolved() {
    let mut source = source();
    let parameter = EffectParameter {
        id: ParameterId::new(),
        name: "age color".into(),
        exposed: true,
        default: Value::Gradient(Gradient::new(vec![
            ColorKey::new(0.0, [1.0, 0.0, 0.0, 1.0]),
            ColorKey::new(1.0, [0.0, 0.0, 1.0, 1.0]),
        ])),
    };
    properties(&mut source).color_source =
        ParticleLightColorSource::GradientParameter(parameter.id);
    properties(&mut source).intensity_curve.keys[0].value = 1.0;
    properties(&mut source).intensity_curve.output_range = Some(ScalarRange::new(0.0, 2_000.0));
    source.parameters.push(parameter);
    let compiled = EffectCompiler::default().compile(&source).unwrap();
    let values: Vec<_> = compiled
        .parameters
        .iter()
        .map(|p| p.default.clone())
        .collect();
    let sample = particle();
    let candidate = compiled
        .particle_light_candidates(&sample, &values)
        .next()
        .unwrap();
    assert_eq!(candidate.light.linear_color, [0.75, 0.0, 0.25]);
    assert_eq!(candidate.light.intensity_lumens, 1_687.5);
    assert_eq!(
        decode_effect(&encode_effect(&compiled).unwrap()).unwrap(),
        compiled
    );
}

#[test]
fn quality_caps_disabled_outputs_and_invalid_samples_are_presentation_only() {
    let mut source = source();
    properties(&mut source)
        .max_lights_by_quality
        .insert("low".into(), 0);
    for (tier, expected) in [
        (QualityTier::high(), 16),
        (QualityTier::medium(), 8),
        (QualityTier::low(), 0),
    ] {
        let compiled = EffectCompiler::default()
            .with_tier(tier)
            .compile(&source)
            .unwrap();
        let SceneOutputPlanKind::ParticlePointLight(plan) =
            &compiled.emitters[0].scene_outputs[0].kind;
        assert_eq!(plan.max_lights, expected);
        assert_eq!(
            compiled.particle_light_candidates(&particle(), &[]).count(),
            usize::from(expected != 0)
        );
    }
    let compiled = EffectCompiler::default().compile(&source).unwrap();
    for case in 0..7 {
        let mut sample = particle();
        match case {
            0 => sample.emitter_index = usize::MAX,
            1 => sample.normalized_age = f32::NAN,
            2 => sample.normalized_age = -0.1,
            3 => sample.normalized_age = 1.0,
            4 => sample.position[1] = f32::INFINITY,
            5 => sample.color[0] = f32::NAN,
            6 => sample.color[0] = 2.0,
            _ => unreachable!(),
        }
        assert_eq!(
            compiled.particle_light_candidates(&sample, &[]).count(),
            0,
            "case {case}"
        );
    }
    source.emitters[0].scene_outputs[0].enabled = false;
    assert!(
        EffectCompiler::default().compile(&source).is_err(),
        "not a valid sole sink when disabled"
    );
    source.emitters[0].enabled = false;
    source.emitters[0].scene_outputs[0].enabled = true;
    assert!(
        EffectCompiler::default().compile(&source).unwrap().emitters[0]
            .scene_outputs
            .is_empty()
    );
}

#[test]
fn invalid_authored_scene_outputs_reject_with_actionable_paths() {
    for case in 0..12 {
        let mut source = source();
        match case {
            0 => source.emitters[0].scene_outputs[0].id = SceneOutputId::from_u128(0),
            1 => source.emitters[0].scene_outputs[0].output_type.0 = "unknown".into(),
            2 => properties(&mut source).radius = -1.0,
            3 => properties(&mut source).intensity_curve.keys[0].value = -1.0,
            4 => properties(&mut source).range_curve.keys[0].value = 0.0,
            5 => properties(&mut source).intensity_curve.keys.clear(),
            6 => {
                properties(&mut source).color_source = ParticleLightColorSource::Constant([2.0; 3])
            }
            7 => {
                properties(&mut source).color_source =
                    ParticleLightColorSource::GradientParameter(ParameterId::new())
            }
            8 => properties(&mut source)
                .max_lights_by_quality
                .remove("high")
                .map(|_| ())
                .unwrap(),
            9 => properties(&mut source)
                .max_lights_by_quality
                .insert(" padded ".into(), 1)
                .map(|_| ())
                .unwrap_or(()),
            10 => {
                let output = source.emitters[0].scene_outputs[0].clone();
                source.emitters[0].scene_outputs.push(output);
            }
            11 => {
                let id = properties(&mut source).range_curve.id;
                properties(&mut source).intensity_curve.id = id;
            }
            _ => unreachable!(),
        }
        let report = source.validation_report();
        assert!(!report.is_valid(), "case {case}");
        assert!(
            report
                .diagnostics
                .iter()
                .any(|d| d.path.contains("scene_outputs")),
            "case {case}: {report:?}"
        );
        assert!(EffectCompiler::default().compile(&source).is_err());
    }
}

#[test]
fn duplication_rekeys_owned_outputs_and_curves_but_keeps_parameter_references() {
    let mut source = source();
    let id = ParameterId::new();
    properties(&mut source).color_source = ParticleLightColorSource::GradientParameter(id);
    let original = source.emitters[0].clone();
    let mut copy = original.clone();
    copy.regenerate_ids();
    assert_ne!(copy.id, original.id);
    assert_ne!(copy.scene_outputs[0].id, original.scene_outputs[0].id);
    let SceneOutputProperties::ParticlePointLight(before) = &original.scene_outputs[0].properties;
    let SceneOutputProperties::ParticlePointLight(after) = &copy.scene_outputs[0].properties;
    assert_ne!(before.intensity_curve.id, after.intensity_curve.id);
    assert_ne!(before.range_curve.id, after.range_curve.id);
    assert_eq!(before.color_source, after.color_source);
}

#[test]
fn reordered_samples_and_regions_have_stable_distinct_candidate_identity() {
    let mut source = source();
    source.emitters[0].regions = vec![
        EmitterRegion::new(0.0, 0.0, 1.0),
        EmitterRegion::new(2.0, 0.0, 1.0),
    ];
    let compiled = EffectCompiler::default().compile(&source).unwrap();
    let a = particle();
    let mut b = a;
    b.particle_index += 1;
    let candidates = |samples: &[ParticleSample]| {
        let mut result: Vec<_> = samples
            .iter()
            .flat_map(|p| compiled.particle_light_candidates(p, &[]))
            .collect();
        result.sort_by_key(|p| p.identity);
        result
    };
    assert_eq!(candidates(&[a, b]), candidates(&[b, a]));
    b = a;
    b.emitter_index = 1;
    let pair = candidates(&[a, b]);
    assert_eq!(pair.len(), 2);
    assert_ne!(pair[0].identity.region, pair[1].identity.region);
}

#[test]
fn thousands_of_candidates_remain_an_unselected_reference_stream() {
    let mut source = source();
    properties(&mut source).color_source = ParticleLightColorSource::Constant([1.0, 0.5, 0.1]);
    properties(&mut source).priority = 7;
    let compiled = EffectCompiler::default().compile(&source).unwrap();
    let samples: Vec<_> = (0..4096)
        .map(|index| ParticleSample {
            particle_index: index,
            color: [f32::NAN; 4],
            ..particle()
        })
        .collect();
    let candidates: Vec<_> = samples
        .iter()
        .flat_map(|p| compiled.particle_light_candidates(p, &[]))
        .collect();
    assert_eq!(
        candidates.len(),
        4096,
        "reference evaluation is not admission/top-K"
    );
    assert!(
        candidates
            .iter()
            .all(|p| p.priority == 7 && p.light.linear_color == [1.0, 0.5, 0.1])
    );
    let SceneOutputPlanKind::ParticlePointLight(plan) = &compiled.emitters[0].scene_outputs[0].kind;
    assert_eq!(
        plan.max_lights, 16,
        "future selection must enforce this and the host cap"
    );
    let mut custom = QualityTier::high();
    custom.name = "custom".into();
    let custom = EffectCompiler::default()
        .with_tier(custom)
        .compile(&source)
        .unwrap();
    assert_eq!(
        custom.emitters[0].scene_outputs, compiled.emitters[0].scene_outputs,
        "unlisted tiers use authored high cap"
    );
}

#[test]
fn baked_plans_reject_old_versions_bad_slots_envelopes_and_duplicate_ids() {
    let compiled = EffectCompiler::default().compile(&source()).unwrap();
    let text = String::from_utf8(encode_effect(&compiled).unwrap()).unwrap();
    for version in [5, 6] {
        assert!(matches!(
            decode_effect(
                text.replacen(
                    &format!("format_version:{CURRENT_ARTIFACT_VERSION}"),
                    &format!("format_version:{version}"),
                    1
                )
                .as_bytes()
            ),
            Err(ArtifactError::UnsupportedVersion { .. })
        ));
    }
    for case in 0..5 {
        let mut broken = compiled.clone();
        let SceneOutputPlanKind::ParticlePointLight(light) =
            &mut broken.emitters[0].scene_outputs[0].kind;
        match case {
            0 => {
                light.color =
                    ParticleLightColorPlan::GradientParameter(aestra_runtime::ParameterSlot(999))
            }
            1 => light.color = ParticleLightColorPlan::Constant([2.0; 3]),
            2 => light.radius = -1.0,
            3 => broken.emitters[0].scene_outputs[0].source = SceneOutputId::from_u128(0),
            4 => {
                let output = broken.emitters[0].scene_outputs[0].clone();
                broken.emitters[0].scene_outputs.push(output);
            }
            _ => unreachable!(),
        }
        assert!(
            matches!(
                decode_effect(&encode_effect(&broken).unwrap()),
                Err(ArtifactError::InvalidData { .. })
            ),
            "case {case}"
        );
    }
}

#[test]
fn reference_candidates_do_not_change_particles_history_or_events() {
    let mut source = source();
    source.materials = EffectAsset::new("materials", 1.0).materials;
    source.emitters[0]
        .renderers
        .push(RendererInstance::sprite(DEFAULT_SPRITE_MATERIAL_ID));
    source.choreography_events.push(ChoreographyEvent::new(
        "host notification",
        0.5,
        ChoreographyEventPayload::GameplayNotify {
            topic: "embers ready".into(),
        },
    ));
    let output = EventDefinition::new("ember birth");
    source.particle_outputs.push(ParticleOutputRoute::new(
        source.emitters[0].id,
        EventTrigger::OnSpawn,
        output.id,
    ));
    source.event_outputs.push(output);
    let compiled = EffectCompiler::default().compile(&source).unwrap();
    let mut plain_source = source.clone();
    plain_source.emitters[0].scene_outputs.clear();
    let plain = EffectCompiler::default().compile(&plain_source).unwrap();
    assert_eq!(compiled.emitters[0].execution, plain.emitters[0].execution);
    assert_eq!(compiled.emitters[0].stages, plain.emitters[0].stages);
    assert_eq!(compiled.particle_layout, plain.particle_layout);
    assert_eq!(compiled.requirements, plain.requirements);
    assert_eq!(compiled.event_routes, plain.event_routes);
    let mut with_lights = EffectInstance::with_seed(Arc::new(compiled), 77);
    let mut without_lights = EffectInstance::with_seed(Arc::new(plain), 77);
    for _ in 0..120 {
        let mut lit_events = Vec::new();
        let mut unlit_events = Vec::new();
        with_lights.advance_with_choreography_events(1.0 / 60.0, &mut lit_events);
        without_lights.advance_with_choreography_events(1.0 / 60.0, &mut unlit_events);
        assert_eq!(lit_events, unlit_events);
        let mut lit = Vec::new();
        let mut unlit = Vec::new();
        with_lights.evaluate(&mut lit);
        without_lights.evaluate(&mut unlit);
        assert_eq!(lit, unlit);
        let revision = with_lights.history_revision();
        let epoch = with_lights.history_epoch();
        for particle in &lit {
            assert_eq!(
                with_lights
                    .effect()
                    .particle_light_candidates(particle, with_lights.parameter_values())
                    .count(),
                1
            );
        }
        assert_eq!(with_lights.history_revision(), revision);
        assert_eq!(with_lights.history_epoch(), epoch);
        let mut after = Vec::new();
        with_lights.evaluate(&mut after);
        assert_eq!(lit, after);
    }
}
