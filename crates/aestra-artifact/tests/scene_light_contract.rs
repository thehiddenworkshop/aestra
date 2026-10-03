//! Authored lights survive source/artifact reload and resolve without a live child entity.
use aestra_artifact::{ArtifactError, decode_effect, encode_effect};
use aestra_compiler::EffectCompiler;
use aestra_core::*;
use aestra_project::ResolvedEffectProject;
use aestra_runtime::{
    CompiledEffectProject, EffectOutputEvent, EventOrigin, LightBindingError, ParameterSlot,
    RuntimeValue,
};
use std::{collections::BTreeMap, sync::Arc};

fn source() -> EffectAsset {
    let mut asset = EffectAsset::new("generic scene light", 4.0);
    let emitter = Emitter::basic_sprite("source", 4.0);
    let output = EventDefinition::new("arbitrary_flash");
    let route = ParticleOutputRoute::new(emitter.id, EventTrigger::OnDeath, output.id);
    let parameter = EffectParameter {
        id: ParameterId::new(),
        name: "light color".into(),
        default: gradient([1.0, 0.2, 0.1]),
        exposed: true,
    };
    let mut binding = PointLightBinding::new(
        route.id,
        PointLightPulse::flash([1.0; 3], 500_000.0, 80.0, 0.6),
    );
    binding.color_parameter = Some(LightColorParameter {
        parameter: parameter.id,
        normalized_age: 0.5,
    });
    asset.emitters.push(emitter);
    asset.event_outputs.push(output);
    asset.particle_outputs.push(route);
    asset.parameters.push(parameter);
    asset.point_lights.push(binding);
    asset
}
fn event() -> EffectOutputEvent {
    EffectOutputEvent::new(
        "arbitrary_flash",
        EventOrigin::Emitter(0),
        "",
        vec![0.0; 3],
        4096.0,
        80,
    )
}
fn gradient(rgb: [f32; 3]) -> Value {
    Value::Gradient(Gradient::new(vec![ColorKey::new(
        0.0,
        [rgb[0], rgb[1], rgb[2], 1.0],
    )]))
}

#[test]
fn source_and_artifact_round_trip_preserve_the_complete_binding() {
    let asset = source();
    let reloaded = EffectAsset::from_ron(&asset.to_pretty_ron().unwrap()).unwrap();
    assert_eq!(asset.point_lights, reloaded.point_lights);
    let compiled = EffectCompiler::default().compile(&reloaded).unwrap();
    let decoded = decode_effect(&encode_effect(&compiled).unwrap()).unwrap();
    assert_eq!(decoded.point_lights, compiled.point_lights);
    let parameters: Vec<_> = decoded
        .parameters
        .iter()
        .map(|p| p.default.clone())
        .collect();
    assert_eq!(
        decoded.point_light_for_output(&event(), |s| parameters.get(s.0)),
        compiled.point_light_for_output(&event(), |s| parameters.get(s.0))
    );
    let mut live = parameters;
    let slot = compiled.point_lights[0].color_parameter.unwrap().0;
    live[slot.0] = RuntimeValue::compile(&gradient([0.1, 0.8, 0.3])).unwrap();
    let (_, pulse) = decoded
        .point_light_for_output(&event(), |s| live.get(s.0))
        .unwrap()
        .unwrap();
    assert_eq!(pulse.linear_color, [0.1, 0.8, 0.3]);
    assert_eq!(
        pulse.intensity_lumens.sample(0.0),
        500_000.0,
        "magnitude is not light intensity"
    );
    live[slot.0] = RuntimeValue::compile(&gradient([2.0, 0.0, 0.0])).unwrap();
    assert_eq!(
        decoded.point_light_for_output(&event(), |s| live.get(s.0)),
        Err(LightBindingError::InvalidColor)
    );
    live[slot.0] = RuntimeValue::Scalar(1.0);
    assert_eq!(
        decoded.point_light_for_output(&event(), |s| live.get(s.0)),
        Err(LightBindingError::InvalidColor)
    );

    let text = String::from_utf8(encode_effect(&compiled).unwrap()).unwrap();
    assert!(matches!(
        decode_effect(
            text.replacen(
                &format!(
                    "format_version:{}",
                    aestra_artifact::CURRENT_ARTIFACT_VERSION
                ),
                "format_version:5",
                1
            )
            .as_bytes()
        ),
        Err(ArtifactError::UnsupportedVersion { found: 5 })
    ));
    let empty = EffectAsset::new("old compatible source", 1.0);
    let text = empty.to_pretty_ron().unwrap();
    assert!(!text.contains("point_lights"));
    assert!(
        EffectAsset::from_ron(&text)
            .unwrap()
            .point_lights
            .is_empty()
    );
}

#[test]
fn invalid_authored_routes_envelopes_and_colors_fail_compilation() {
    let original = source();
    for case in 0..13 {
        let mut asset = original.clone();
        match case {
            0 => asset.point_lights[0].route = EventRouteId::new(),
            1 => asset.particle_outputs[0].aggregation = EventAggregation::EachEvent { limit: 2 },
            2 => {
                let mut route = asset.particle_outputs[0].clone();
                route.id = EventRouteId::new();
                route.trigger = EventTrigger::OnSpawn;
                asset.particle_outputs.push(route);
            }
            3 => {
                let mut binding = asset.point_lights[0].clone();
                binding.id = EventRouteId::new();
                asset.point_lights.push(binding);
            }
            4 => asset.point_lights[0].pulse.duration_seconds = 0.0,
            5 => asset.point_lights[0].pulse.range.keys[0].value = 0.0,
            6 => asset.point_lights[0].pulse.intensity_lumens.keys.swap(0, 1),
            7 => {
                asset.point_lights[0]
                    .color_parameter
                    .as_mut()
                    .unwrap()
                    .normalized_age = 2.0
            }
            8 => {
                asset.point_lights[0]
                    .color_parameter
                    .as_mut()
                    .unwrap()
                    .parameter = ParameterId::new()
            }
            9 => asset.point_lights[0].id = EventRouteId::from_u128(0),
            10 => {
                asset.point_lights[0].pulse.range.id =
                    asset.point_lights[0].pulse.intensity_lumens.id
            }
            11 => asset.parameters[0].default = Value::Scalar(1.0),
            _ => asset.parameters[0].default = gradient([2.0, 0.0, 0.0]),
        }
        assert!(
            EffectCompiler::default().compile(&asset).is_err(),
            "case {case}"
        );
    }
    let mut disabled = original;
    disabled.emitters[0].enabled = false;
    assert!(
        EffectCompiler::default()
            .compile(&disabled)
            .unwrap()
            .point_lights
            .is_empty()
    );
}

#[test]
fn non_exposed_color_parameters_lower_to_a_literal_pulse() {
    let mut asset = source();
    asset.parameters[0].exposed = false;
    let compiled = EffectCompiler::default().compile(&asset).unwrap();
    assert!(compiled.parameters.is_empty());
    assert!(compiled.point_lights[0].color_parameter.is_none());
    assert_eq!(compiled.point_lights[0].pulse.linear_color, [1.0, 0.2, 0.1]);
    assert_eq!(
        decode_effect(&encode_effect(&compiled).unwrap())
            .unwrap()
            .point_lights,
        compiled.point_lights
    );
}

#[test]
fn malformed_compiled_light_data_is_rejected_on_artifact_reload() {
    let compiled = EffectCompiler::default().compile(&source()).unwrap();
    for case in 0..9 {
        let mut broken = compiled.clone();
        match case {
            0 => broken.point_lights[0].route = usize::MAX,
            1 => broken.point_lights[0].pulse.linear_color[0] = 2.0,
            2 => broken.point_lights[0].pulse.range.keys.clear(),
            3 => broken.point_lights[0].color_parameter = Some((ParameterSlot(9999), 0.5)),
            4 => broken.point_lights[0].color_parameter.as_mut().unwrap().1 = -1.0,
            5 => broken.point_lights.push(broken.point_lights[0].clone()),
            6 => {
                let route = broken.point_lights[0].route;
                let aestra_runtime::CompiledEventRoute::ParticleOutput(r) =
                    &mut broken.event_routes[route]
                else {
                    panic!()
                };
                r.aggregation = EventAggregation::EachEvent { limit: 2 };
            }
            7 => {
                let r = broken.event_routes[broken.point_lights[0].route].clone();
                broken.event_routes.push(r);
            }
            _ => broken.point_lights[0].source = EventRouteId::from_u128(0),
        }
        let result = encode_effect(&broken).and_then(|bytes| decode_effect(&bytes));
        assert!(result.is_err(), "case {case}");
    }
}

#[test]
fn stable_nested_paths_use_final_clip_overrides_after_child_lifetime() {
    let child = source();
    let parameter = child.point_lights[0]
        .color_parameter
        .as_ref()
        .unwrap()
        .parameter;
    let mut middle = EffectAsset::new("middle", 10.0);
    let mut inner = EffectClip::new(child.id, 0.0, 1.0);
    inner
        .parameter_overrides
        .insert(parameter, gradient([0.2, 0.3, 0.9]));
    middle.effect_clips.push(inner.clone());
    let mut root = EffectAsset::new("root", 20.0);
    let first = EffectClip::new(middle.id, 0.0, 10.0);
    let second = EffectClip::new(middle.id, 10.0, 10.0);
    root.effect_clips.extend([first.clone(), second.clone()]);
    let compiled = EffectCompiler::default()
        .compile_resolved_project(&ResolvedEffectProject {
            root,
            dependencies: BTreeMap::from([(middle.id, middle), (child.id, child.clone())]),
            material_programs: BTreeMap::new(),
            material_functions: BTreeMap::new(),
        })
        .unwrap();
    let reloaded = CompiledEffectProject {
        root: Arc::new(decode_effect(&encode_effect(&compiled.root).unwrap()).unwrap()),
        dependencies: compiled
            .dependencies
            .iter()
            .map(|(id, effect)| {
                (
                    *id,
                    Arc::new(decode_effect(&encode_effect(effect).unwrap()).unwrap()),
                )
            })
            .collect(),
    };
    // No instance, clock or temporary presentation is needed to resolve either occurrence.
    for outer in [first.id, second.id] {
        let (_, pulse) = reloaded
            .point_light_for_output(&[outer, inner.id], child.id, &event(), &[])
            .unwrap()
            .unwrap();
        assert_eq!(pulse.linear_color, [0.2, 0.3, 0.9]);
    }
    assert_eq!(
        reloaded.point_light_for_output(&[first.id], child.id, &event(), &[]),
        Err(LightBindingError::MissingSource)
    );
    assert_eq!(
        reloaded.point_light_for_output(&[EffectClipId::new()], child.id, &event(), &[]),
        Err(LightBindingError::MissingSource)
    );
    assert_eq!(
        reloaded.point_light_for_output(&vec![first.id; 64], child.id, &event(), &[]),
        Err(LightBindingError::MissingSource)
    );
}
