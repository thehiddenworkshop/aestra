use aestra_compiler::{
    MaterialCompiler, MaterialControlKind, MaterialControlReflectionError, MaterialControlSource,
    MaterialControlValueOrigin, MaterialResourceConstraint,
};
use aestra_core::{
    AssetId, MaterialExpressionId, MaterialId, MaterialParameterId, MaterialProgramId, ParameterId,
    material::{
        MaterialEvaluationDomain, MaterialExpression, MaterialExpressionKind, MaterialInput,
        MaterialInstance, MaterialParameter, MaterialParameterValue, MaterialProgram,
        MaterialProgramRef, MaterialRenderState, MaterialTextureDescriptor, MaterialValue,
        MaterialValueType,
    },
};
use std::collections::BTreeMap;

#[test]
fn reflection_exposes_controls_sources_resources_and_live_input_requirements() {
    let (program, ids) = reflected_program();

    let catalog = MaterialCompiler.reflect_controls(&program, None).unwrap();

    assert_eq!(catalog.program, program.id);
    assert_eq!(catalog.material, None);
    assert_eq!(catalog.parameters.len(), 6);
    assert_eq!(catalog.required_inputs.vertex, [MaterialInput::Uv0]);
    assert_eq!(
        catalog.required_inputs.particle,
        [MaterialInput::ParticleColor, MaterialInput::ParticleOpacity]
    );
    assert_eq!(catalog.required_inputs.scene, [MaterialInput::EffectTime]);
    assert_eq!(
        catalog.required_inputs.all().collect::<Vec<_>>(),
        [
            MaterialInput::Uv0,
            MaterialInput::ParticleColor,
            MaterialInput::ParticleOpacity,
            MaterialInput::EffectTime,
        ]
    );

    let texture = control(&catalog, ids.texture);
    assert_eq!(texture.control, MaterialControlKind::Texture);
    assert_eq!(texture.supported_sources, [MaterialControlSource::Constant]);
    assert!(matches!(
        texture.resource_constraint,
        Some(MaterialResourceConstraint::Texture2D(_))
    ));

    let tint = control(&catalog, ids.tint);
    assert_eq!(tint.control, MaterialControlKind::Color);
    assert_eq!(
        tint.supported_sources,
        [
            MaterialControlSource::Constant,
            MaterialControlSource::RandomRange,
        ]
    );

    let intensity = control(&catalog, ids.intensity);
    assert_eq!(
        intensity.supported_sources,
        [
            MaterialControlSource::Constant,
            MaterialControlSource::EffectParameter,
            MaterialControlSource::RandomRange,
        ]
    );
    assert_eq!(
        intensity.value_origin,
        MaterialControlValueOrigin::ProgramDefault
    );
    assert_eq!(
        intensity.current_value,
        Some(MaterialParameterValue::Constant(MaterialValue::Float(1.0)))
    );

    let direction = control(&catalog, ids.direction);
    assert_eq!(direction.control, MaterialControlKind::Vector3);
    assert_eq!(
        direction.supported_sources,
        [
            MaterialControlSource::Constant,
            MaterialControlSource::EmitterParameter,
            MaterialControlSource::RandomRange,
        ]
    );

    let specialized = control(&catalog, ids.specialized);
    assert_eq!(specialized.control, MaterialControlKind::Toggle);
    assert_eq!(
        specialized.supported_sources,
        [MaterialControlSource::Constant]
    );

    let required = control(&catalog, ids.required_offset);
    assert_eq!(required.control, MaterialControlKind::Vector2);
    assert_eq!(required.default_value, None);
    assert_eq!(required.current_value, None);
    assert_eq!(required.value_origin, MaterialControlValueOrigin::Required);
}

#[test]
fn instance_reflection_projects_overrides_and_render_state_without_gpu_types() {
    let (program, ids) = reflected_program();
    let effect_parameter = ParameterId::from_u128(0xB101);
    let emitter_parameter = ParameterId::from_u128(0xB102);
    let material = MaterialId::from_u128(0xB103);
    let instance = MaterialInstance {
        id: material,
        program: MaterialProgramRef::Project(program.id),
        values: BTreeMap::from([
            (
                ids.intensity,
                MaterialParameterValue::EffectParameter(effect_parameter),
            ),
            (
                ids.direction,
                MaterialParameterValue::EmitterParameter(emitter_parameter),
            ),
            (
                ids.tint,
                MaterialParameterValue::RandomRange {
                    min: MaterialValue::ColorSrgb([0.1, 0.2, 0.3, 1.0]),
                    max: MaterialValue::ColorSrgb([0.8, 0.9, 1.0, 1.0]),
                    domain: MaterialEvaluationDomain::Instance,
                },
            ),
            (
                ids.required_offset,
                MaterialParameterValue::Constant(MaterialValue::Vec2([0.25, 0.5])),
            ),
        ]),
        render_state: MaterialRenderState::additive_sprite(),
    };

    let catalog = MaterialCompiler
        .reflect_controls(&program, Some(&instance))
        .unwrap();

    assert_eq!(catalog.material, Some(material));
    assert_eq!(catalog.current_render_state, instance.render_state);
    assert_eq!(
        control(&catalog, ids.intensity).current_value,
        Some(MaterialParameterValue::EffectParameter(effect_parameter))
    );
    assert_eq!(
        control(&catalog, ids.direction).current_value,
        Some(MaterialParameterValue::EmitterParameter(emitter_parameter))
    );
    assert_eq!(
        control(&catalog, ids.required_offset).value_origin,
        MaterialControlValueOrigin::InstanceOverride
    );
}

#[test]
fn reflection_rejects_an_instance_that_does_not_match_the_program() {
    let (program, _) = reflected_program();
    let instance = MaterialInstance {
        id: MaterialId::from_u128(0xBAD1),
        program: MaterialProgramRef::Project(MaterialProgramId::from_u128(0xBAD2)),
        values: BTreeMap::new(),
        render_state: MaterialRenderState::additive_sprite(),
    };

    assert!(matches!(
        MaterialCompiler.reflect_controls(&program, Some(&instance)),
        Err(MaterialControlReflectionError::InvalidInstance(_))
    ));
}

fn control(
    catalog: &aestra_compiler::MaterialControlCatalog,
    id: MaterialParameterId,
) -> &aestra_compiler::MaterialControlDescriptor {
    catalog
        .parameters
        .iter()
        .find(|control| control.id == id)
        .unwrap()
}

#[derive(Clone, Copy)]
struct ParameterIds {
    texture: MaterialParameterId,
    tint: MaterialParameterId,
    intensity: MaterialParameterId,
    direction: MaterialParameterId,
    specialized: MaterialParameterId,
    required_offset: MaterialParameterId,
}

fn reflected_program() -> (MaterialProgram, ParameterIds) {
    let ids = ParameterIds {
        texture: MaterialParameterId::from_u128(0xA101),
        tint: MaterialParameterId::from_u128(0xA102),
        intensity: MaterialParameterId::from_u128(0xA103),
        direction: MaterialParameterId::from_u128(0xA104),
        specialized: MaterialParameterId::from_u128(0xA105),
        required_offset: MaterialParameterId::from_u128(0xA106),
    };
    let texture_descriptor = MaterialTextureDescriptor {
        color_space: aestra_core::material::MaterialTextureColorSpace::SrgbColor,
        sampler: Default::default(),
    };
    let mut program = MaterialProgram::additive_sprite("Reflected flame");
    program.id = MaterialProgramId::from_u128(0xA100);
    program.parameters = vec![
        MaterialParameter {
            id: ids.texture,
            name: "Main Texture".into(),
            value_type: MaterialValueType::Texture2D(texture_descriptor),
            evaluation_domain: MaterialEvaluationDomain::Instance,
            default: Some(MaterialValue::Texture2D(AssetId::from_u128(0xA201))),
        },
        MaterialParameter {
            id: ids.tint,
            name: "Tint".into(),
            value_type: MaterialValueType::Color,
            evaluation_domain: MaterialEvaluationDomain::Instance,
            default: Some(MaterialValue::ColorSrgb([1.0; 4])),
        },
        MaterialParameter {
            id: ids.intensity,
            name: "Intensity".into(),
            value_type: MaterialValueType::Float,
            evaluation_domain: MaterialEvaluationDomain::Effect,
            default: Some(MaterialValue::Float(1.0)),
        },
        MaterialParameter {
            id: ids.direction,
            name: "Direction".into(),
            value_type: MaterialValueType::Vec3,
            evaluation_domain: MaterialEvaluationDomain::Emitter,
            default: Some(MaterialValue::Vec3([0.0, 1.0, 0.0])),
        },
        MaterialParameter {
            id: ids.specialized,
            name: "Specialized".into(),
            value_type: MaterialValueType::Bool,
            evaluation_domain: MaterialEvaluationDomain::ShaderStatic,
            default: Some(MaterialValue::Bool(false)),
        },
        MaterialParameter {
            id: ids.required_offset,
            name: "Required Offset".into(),
            value_type: MaterialValueType::Vec2,
            evaluation_domain: MaterialEvaluationDomain::Instance,
            default: None,
        },
    ];

    let texture = MaterialExpressionId::from_u128(0xA301);
    let uv = MaterialExpressionId::from_u128(0xA302);
    let sampled = MaterialExpressionId::from_u128(0xA303);
    let particle_color = MaterialExpressionId::from_u128(0xA304);
    let color = MaterialExpressionId::from_u128(0xA305);
    let opacity = MaterialExpressionId::from_u128(0xA306);
    let effect_time = MaterialExpressionId::from_u128(0xA307);
    let alpha = MaterialExpressionId::from_u128(0xA308);
    program.expressions = vec![
        MaterialExpression {
            id: texture,
            kind: MaterialExpressionKind::Parameter(ids.texture),
        },
        MaterialExpression {
            id: uv,
            kind: MaterialExpressionKind::Input(MaterialInput::Uv0),
        },
        MaterialExpression {
            id: sampled,
            kind: MaterialExpressionKind::SampleTexture { texture, uv },
        },
        MaterialExpression {
            id: particle_color,
            kind: MaterialExpressionKind::Input(MaterialInput::ParticleColor),
        },
        MaterialExpression {
            id: color,
            kind: MaterialExpressionKind::Multiply(sampled, particle_color),
        },
        MaterialExpression {
            id: opacity,
            kind: MaterialExpressionKind::Input(MaterialInput::ParticleOpacity),
        },
        MaterialExpression {
            id: effect_time,
            kind: MaterialExpressionKind::Input(MaterialInput::EffectTime),
        },
        MaterialExpression {
            id: alpha,
            kind: MaterialExpressionKind::Multiply(opacity, effect_time),
        },
    ];
    program.outputs.color = color;
    program.outputs.alpha = alpha;
    (program, ids)
}
