use aestra_core::{
    AssetId, DiagnosticCode, DiagnosticSeverity, EffectAsset, Emitter, MaterialExpressionId,
    MaterialId, MaterialParameterId, MaterialProgramId, ParameterId,
    material::{
        MaterialAddressMode, MaterialCullMode, MaterialDepthTest, MaterialDomain,
        MaterialEvaluationDomain, MaterialExpression, MaterialExpressionDomain,
        MaterialExpressionKind, MaterialFilterMode, MaterialInput, MaterialInstance,
        MaterialMipFilterMode, MaterialParameter, MaterialParameterValue, MaterialProgram,
        MaterialProgramRef, MaterialRenderState, MaterialSamplerDescriptor, MaterialSchemaVersion,
        MaterialTextureColorSpace, MaterialTextureDescriptor, MaterialValue, MaterialValueType,
    },
};
use std::collections::BTreeMap;

#[test]
fn semantic_material_program_and_instance_round_trip_with_stable_ids() {
    let parameter = MaterialParameterId::from_u128(0x100);
    let mut program = MaterialProgram::additive_sprite("Magic Flame");
    program.id = MaterialProgramId::from_u128(0x200);
    program.parameters.push(MaterialParameter {
        id: parameter,
        name: "intensity".into(),
        value_type: MaterialValueType::Float,
        evaluation_domain: MaterialEvaluationDomain::Effect,
        default: Some(MaterialValue::Float(1.0)),
    });

    let instance = MaterialInstance {
        id: MaterialId::from_u128(0x300),
        program: MaterialProgramRef::Project(program.id),
        values: BTreeMap::from([(
            parameter,
            MaterialParameterValue::Constant(MaterialValue::Float(2.5)),
        )]),
        render_state: MaterialRenderState::additive_sprite(),
    };

    let encoded = program.to_pretty_ron().unwrap();
    let decoded = MaterialProgram::from_ron(&encoded).unwrap();
    let instance_encoded = ron::to_string(&instance).unwrap();
    let instance_decoded: MaterialInstance = ron::from_str(&instance_encoded).unwrap();

    assert_eq!(decoded, program.normalized());
    assert_eq!(decoded.to_pretty_ron().unwrap(), encoded);
    assert_eq!(instance_decoded, instance);
    assert_eq!(decoded.schema_version, MaterialSchemaVersion::CURRENT);
    assert_eq!(instance_decoded.program.id(), decoded.id);
}

#[test]
fn additive_sprite_program_is_structurally_valid() {
    let program = MaterialProgram::additive_sprite("Additive Sprite");
    let report = program.validate_structure();

    assert!(report.is_valid(), "{:#?}", report.diagnostics);
    assert!(report.diagnostics.is_empty());
}

#[test]
fn shared_subexpressions_are_valid_and_unreachable_nodes_are_warnings() {
    let mut program = MaterialProgram::additive_sprite("Shared");
    let shared = program.outputs.color;
    program.expressions[1].kind = MaterialExpressionKind::Multiply(shared, shared);
    let unreachable = MaterialExpressionId::from_u128(0x401);
    program.expressions.push(MaterialExpression {
        id: unreachable,
        kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.5)),
    });

    let report = program.validate_structure();

    assert!(report.is_valid(), "{:#?}", report.diagnostics);
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic.severity == DiagnosticSeverity::Warning
            && diagnostic.path == "material_program.expressions[2]"
    }));
}

#[test]
fn expression_cycles_and_missing_references_are_rejected() {
    let mut program = MaterialProgram::additive_sprite("Broken");
    let first = program.expressions[0].id;
    let second = program.expressions[1].id;
    let missing = MaterialExpressionId::from_u128(0x500);
    program.expressions[0].kind = MaterialExpressionKind::Add(second, missing);
    program.expressions[1].kind = MaterialExpressionKind::Multiply(first, first);

    let report = program.validate_structure();

    assert!(!report.is_valid());
    assert!(
        report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == DiagnosticCode::InvalidReference)
    );
    assert!(
        report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == DiagnosticCode::ReferenceCycle)
    );
}

#[test]
fn duplicate_ids_and_invalid_parameter_defaults_are_rejected() {
    let mut program = MaterialProgram::additive_sprite("Invalid parameters");
    let parameter = MaterialParameterId::from_u128(0x600);
    let invalid = MaterialParameter {
        id: parameter,
        name: "intensity".into(),
        value_type: MaterialValueType::Float,
        evaluation_domain: MaterialEvaluationDomain::Instance,
        default: Some(MaterialValue::Vec2([1.0, 2.0])),
    };
    program.parameters.push(invalid.clone());
    program.parameters.push(invalid);

    let report = program.validate_structure();

    assert!(!report.is_valid());
    assert!(
        report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == DiagnosticCode::DuplicateId)
    );
    assert!(
        report
            .diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.code == DiagnosticCode::ParameterTypeMismatch })
    );
}

#[test]
fn material_instance_rejects_invalid_dynamic_sources() {
    let parameter = MaterialParameterId::from_u128(0x700);
    let instance = MaterialInstance {
        id: MaterialId::from_u128(0x701),
        program: MaterialProgramRef::Project(MaterialProgramId::from_u128(0x702)),
        values: BTreeMap::from([
            (
                parameter,
                MaterialParameterValue::EmitterParameter(ParameterId::from_u128(0)),
            ),
            (
                MaterialParameterId::from_u128(0x703),
                MaterialParameterValue::RandomRange {
                    min: MaterialValue::Float(0.0),
                    max: MaterialValue::Vec2([1.0, 2.0]),
                    domain: MaterialEvaluationDomain::ShaderStatic,
                },
            ),
        ]),
        render_state: MaterialRenderState::additive_sprite(),
    };

    let report = instance.validate_structure();

    assert!(!report.is_valid());
    assert!(
        report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == DiagnosticCode::NilId)
    );
    assert!(
        report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == DiagnosticCode::ParameterTypeMismatch)
    );
    assert!(
        report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == DiagnosticCode::InvalidValue)
    );
}

#[test]
fn material_instance_rejects_non_numeric_random_ranges() {
    let parameter = MaterialParameterId::from_u128(0x710);
    let instance = MaterialInstance {
        id: MaterialId::from_u128(0x711),
        program: MaterialProgramRef::Project(MaterialProgramId::from_u128(0x712)),
        values: BTreeMap::from([(
            parameter,
            MaterialParameterValue::RandomRange {
                min: MaterialValue::Bool(false),
                max: MaterialValue::Bool(true),
                domain: MaterialEvaluationDomain::Effect,
            },
        )]),
        render_state: MaterialRenderState::additive_sprite(),
    };

    let report = instance.validate_structure();

    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == DiagnosticCode::InvalidValue
            && diagnostic.message.contains("numeric endpoints")
    }));
}

#[test]
fn effect_local_material_instances_round_trip_and_can_back_renderer_references() {
    let material = MaterialId::from_u128(0x800);
    let mut effect = EffectAsset::new("Semantic material owner", 1.0);
    effect.material_instances.push(MaterialInstance {
        id: material,
        program: MaterialProgramRef::Project(MaterialProgramId::from_u128(0x801)),
        values: BTreeMap::new(),
        render_state: MaterialRenderState::additive_sprite(),
    });
    let mut emitter = Emitter::basic_sprite("Emitter", 1.0);
    emitter.renderers[0].material = material;
    effect.emitters.push(emitter);

    let encoded = effect.to_pretty_ron().unwrap();
    let decoded = EffectAsset::from_ron(&encoded).unwrap();

    assert_eq!(decoded, effect);
    assert!(encoded.contains("material_instances"));
    assert_eq!(decoded.emitters[0].renderers[0].material, material);
}

#[test]
fn legacy_and_semantic_materials_share_one_effect_local_identity_namespace() {
    let mut effect = EffectAsset::new("Duplicate material identity", 1.0);
    effect.material_instances.push(MaterialInstance {
        id: effect.materials[0].id,
        program: MaterialProgramRef::Project(MaterialProgramId::from_u128(0x802)),
        values: BTreeMap::new(),
        render_state: MaterialRenderState::additive_sprite(),
    });

    let report = effect.validation_report();

    assert!(!report.is_valid());
    assert!(
        report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == DiagnosticCode::DuplicateId)
    );
}

#[test]
fn effect_local_material_bindings_must_reference_owned_effect_parameters() {
    let missing = ParameterId::from_u128(0x900);
    let mut effect = EffectAsset::new("Missing material binding", 1.0);
    effect.material_instances.push(MaterialInstance {
        id: MaterialId::from_u128(0x901),
        program: MaterialProgramRef::Project(MaterialProgramId::from_u128(0x902)),
        values: BTreeMap::from([(
            MaterialParameterId::from_u128(0x903),
            MaterialParameterValue::EffectParameter(missing),
        )]),
        render_state: MaterialRenderState::additive_sprite(),
    });

    let report = effect.validation_report();

    assert!(!report.is_valid());
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == DiagnosticCode::InvalidReference
            && diagnostic.message.contains(&missing.to_string())
    }));
}

fn texture_descriptor() -> MaterialTextureDescriptor {
    MaterialTextureDescriptor {
        color_space: MaterialTextureColorSpace::SrgbColor,
        sampler: MaterialSamplerDescriptor {
            filter: MaterialFilterMode::Linear,
            mip_filter: MaterialMipFilterMode::Linear,
            address_u: MaterialAddressMode::ClampToEdge,
            address_v: MaterialAddressMode::ClampToEdge,
        },
    }
}

#[test]
fn material_analysis_infers_typed_sockets_and_evaluation_domains() {
    let texture_parameter = MaterialParameterId::from_u128(0xA01);
    let intensity_parameter = MaterialParameterId::from_u128(0xA02);
    let texture = MaterialExpressionId::from_u128(0xA03);
    let uv = MaterialExpressionId::from_u128(0xA04);
    let sample = MaterialExpressionId::from_u128(0xA05);
    let intensity = MaterialExpressionId::from_u128(0xA06);
    let color = MaterialExpressionId::from_u128(0xA07);
    let alpha = MaterialExpressionId::from_u128(0xA08);
    let mut program = MaterialProgram::additive_sprite("Typed flame");
    program.parameters = vec![
        MaterialParameter {
            id: texture_parameter,
            name: "flame_texture".into(),
            value_type: MaterialValueType::Texture2D(texture_descriptor()),
            evaluation_domain: MaterialEvaluationDomain::Instance,
            default: Some(MaterialValue::Texture2D(AssetId::from_u128(0xA09))),
        },
        MaterialParameter {
            id: intensity_parameter,
            name: "intensity".into(),
            value_type: MaterialValueType::Float,
            evaluation_domain: MaterialEvaluationDomain::Effect,
            default: Some(MaterialValue::Float(1.0)),
        },
    ];
    program.expressions = vec![
        MaterialExpression {
            id: texture,
            kind: MaterialExpressionKind::Parameter(texture_parameter),
        },
        MaterialExpression {
            id: uv,
            kind: MaterialExpressionKind::Input(MaterialInput::Uv0),
        },
        MaterialExpression {
            id: sample,
            kind: MaterialExpressionKind::SampleTexture { texture, uv },
        },
        MaterialExpression {
            id: intensity,
            kind: MaterialExpressionKind::Parameter(intensity_parameter),
        },
        MaterialExpression {
            id: color,
            kind: MaterialExpressionKind::Multiply(sample, intensity),
        },
        MaterialExpression {
            id: alpha,
            kind: MaterialExpressionKind::Input(MaterialInput::ParticleOpacity),
        },
    ];
    program.outputs.color = color;
    program.outputs.alpha = alpha;

    let analysis = program.analyze().unwrap();

    assert_eq!(
        analysis.expressions[&sample].value_type,
        MaterialValueType::Color
    );
    assert_eq!(
        analysis.expressions[&intensity].evaluation_domain,
        MaterialExpressionDomain::Effect
    );
    assert_eq!(
        analysis.expressions[&color].evaluation_domain,
        MaterialExpressionDomain::Fragment
    );
}

#[test]
fn material_validation_rejects_output_and_socket_type_mismatches() {
    let mut program = MaterialProgram::additive_sprite("Bad types");
    let vector = MaterialExpressionId::from_u128(0xB01);
    let boolean = MaterialExpressionId::from_u128(0xB02);
    let add = MaterialExpressionId::from_u128(0xB03);
    program.expressions.extend([
        MaterialExpression {
            id: vector,
            kind: MaterialExpressionKind::Input(MaterialInput::WorldPosition),
        },
        MaterialExpression {
            id: boolean,
            kind: MaterialExpressionKind::Constant(MaterialValue::Bool(true)),
        },
        MaterialExpression {
            id: add,
            kind: MaterialExpressionKind::Add(vector, boolean),
        },
    ]);
    program.outputs.alpha = vector;

    let first = program.validation_report();
    let second = program.validation_report();

    assert_eq!(first, second);
    assert!(!first.is_valid());
    assert!(first.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == DiagnosticCode::MaterialTypeMismatch
            && diagnostic.path == "material_program.outputs.alpha"
    }));
    assert!(first.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == DiagnosticCode::MaterialTypeMismatch
            && diagnostic.path.ends_with(".kind")
    }));
}

#[test]
fn sampled_textures_require_instance_resource_declarations() {
    let mut program = MaterialProgram::additive_sprite("Undeclared texture");
    let texture = MaterialExpressionId::from_u128(0xC01);
    let uv = MaterialExpressionId::from_u128(0xC02);
    let sample = MaterialExpressionId::from_u128(0xC03);
    program.expressions.extend([
        MaterialExpression {
            id: texture,
            kind: MaterialExpressionKind::Constant(MaterialValue::Texture2D(AssetId::from_u128(
                0xC04,
            ))),
        },
        MaterialExpression {
            id: uv,
            kind: MaterialExpressionKind::Input(MaterialInput::Uv0),
        },
        MaterialExpression {
            id: sample,
            kind: MaterialExpressionKind::SampleTexture { texture, uv },
        },
    ]);
    program.outputs.color = sample;

    let report = program.validation_report();

    assert!(!report.is_valid());
    assert!(
        report
            .diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.code == DiagnosticCode::MissingResourceDeclaration })
    );
}

#[test]
fn unsupported_domains_inputs_and_render_states_are_diagnosed() {
    let mut program = MaterialProgram::additive_sprite("Unsupported policy");
    program.domain = MaterialDomain::Mesh;
    program.parameters.push(MaterialParameter {
        id: MaterialParameterId::from_u128(0xD01),
        name: "texture".into(),
        value_type: MaterialValueType::Texture2D(texture_descriptor()),
        evaluation_domain: MaterialEvaluationDomain::Effect,
        default: None,
    });
    program.render_state_policy.allowed[0] = MaterialRenderState {
        blend: aestra_core::BlendMode::Additive,
        depth_test: MaterialDepthTest::Disabled,
        depth_write: true,
        cull_mode: MaterialCullMode::Back,
    };
    program.render_state_policy.default = program.render_state_policy.allowed[0];

    let report = program.validation_report();

    assert!(!report.is_valid());
    assert!(
        report
            .diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.code == DiagnosticCode::UnsupportedMaterialDomain })
    );
    assert!(
        report
            .diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.code == DiagnosticCode::EvaluationDomainMismatch })
    );
    assert!(
        report
            .diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.code == DiagnosticCode::InvalidRenderState })
    );
}

#[test]
fn material_value_types_accept_only_compatible_effect_parameter_values() {
    assert!(MaterialValueType::Float.accepts_effect_value(&aestra_core::Value::Scalar(1.0)));
    assert!(MaterialValueType::Vec2.accepts_effect_value(&aestra_core::Value::Vec2([1.0, 2.0])));
    assert!(
        MaterialValueType::Vec3.accepts_effect_value(&aestra_core::Value::Vec3([1.0, 2.0, 3.0]))
    );
    assert!(
        MaterialValueType::Vec4
            .accepts_effect_value(&aestra_core::Value::Vec4([1.0, 2.0, 3.0, 4.0]))
    );
    assert!(
        MaterialValueType::Color
            .accepts_effect_value(&aestra_core::Value::Vec4([1.0, 0.5, 0.25, 1.0]))
    );
    assert!(
        MaterialValueType::Texture2D(texture_descriptor())
            .accepts_effect_value(&aestra_core::Value::Asset(AssetId::from_u128(0xE01)))
    );
    assert!(MaterialValueType::Bool.accepts_effect_value(&aestra_core::Value::Bool(true)));

    assert!(!MaterialValueType::Float.accepts_effect_value(&aestra_core::Value::Bool(true)));
    assert!(!MaterialValueType::Vec3.accepts_effect_value(&aestra_core::Value::Vec4([0.0; 4])));
    assert!(!MaterialValueType::Color.accepts_effect_value(&aestra_core::Value::Vec3([0.0; 3])));
}
