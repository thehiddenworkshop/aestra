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
        MaterialVectorComponent,
    },
};
use std::collections::BTreeMap;

#[test]
fn select_requires_a_boolean_condition_and_matching_non_resource_branches() {
    let condition = MaterialExpressionId::from_u128(0x50_001);
    let if_false = MaterialExpressionId::from_u128(0x50_002);
    let if_true = MaterialExpressionId::from_u128(0x50_003);
    let select = MaterialExpressionId::from_u128(0x50_004);
    let mut program = MaterialProgram::additive_sprite("Invalid select");
    program.expressions.extend([
        MaterialExpression {
            id: condition,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(1.0)),
        },
        MaterialExpression {
            id: if_false,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.0)),
        },
        MaterialExpression {
            id: if_true,
            kind: MaterialExpressionKind::Constant(MaterialValue::Vec2([1.0; 2])),
        },
        MaterialExpression {
            id: select,
            kind: MaterialExpressionKind::Select {
                condition,
                if_false,
                if_true,
            },
        },
    ]);
    program.outputs.alpha = select;

    let report = program.analyze().unwrap_err();

    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == DiagnosticCode::MaterialTypeMismatch
            && diagnostic.path.ends_with(".condition")
            && diagnostic.message.contains("expects Bool")
    }));
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == DiagnosticCode::MaterialTypeMismatch
            && diagnostic.message.contains("Select branches")
    }));
}

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
fn explicit_lod_texture_sampling_requires_a_float_level_and_declared_texture() {
    let texture_parameter = MaterialParameterId::from_u128(0xA101);
    let texture = MaterialExpressionId::from_u128(0xA102);
    let uv = MaterialExpressionId::from_u128(0xA103);
    let level = MaterialExpressionId::from_u128(0xA104);
    let sample = MaterialExpressionId::from_u128(0xA105);
    let mut program = MaterialProgram::additive_sprite("Explicit texture level");
    program.parameters.push(MaterialParameter {
        id: texture_parameter,
        name: "texture".into(),
        value_type: MaterialValueType::Texture2D(texture_descriptor()),
        evaluation_domain: MaterialEvaluationDomain::Instance,
        default: Some(MaterialValue::Texture2D(AssetId::from_u128(0xA106))),
    });
    program.expressions.extend([
        MaterialExpression {
            id: texture,
            kind: MaterialExpressionKind::Parameter(texture_parameter),
        },
        MaterialExpression {
            id: uv,
            kind: MaterialExpressionKind::Input(MaterialInput::Uv0),
        },
        MaterialExpression {
            id: level,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(2.0)),
        },
        MaterialExpression {
            id: sample,
            kind: MaterialExpressionKind::SampleTextureLevel { texture, uv, level },
        },
    ]);
    program.outputs.color = sample;

    let analysis = program.analyze().unwrap();
    assert_eq!(
        analysis.expressions[&sample].value_type,
        MaterialValueType::Color
    );
    assert_eq!(
        analysis.expressions[&sample].evaluation_domain,
        MaterialExpressionDomain::Fragment
    );

    program
        .expressions
        .iter_mut()
        .find(|expression| expression.id == level)
        .unwrap()
        .kind = MaterialExpressionKind::Constant(MaterialValue::Vec2([2.0; 2]));
    let report = program.validation_report();
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == DiagnosticCode::MaterialTypeMismatch
            && diagnostic.path.ends_with(".level")
    }));
}

#[test]
fn gradient_texture_sampling_requires_vec2_derivatives_and_stays_fragment_local() {
    let texture_parameter = MaterialParameterId::from_u128(0xA201);
    let texture = MaterialExpressionId::from_u128(0xA202);
    let uv = MaterialExpressionId::from_u128(0xA203);
    let ddx = MaterialExpressionId::from_u128(0xA204);
    let ddy = MaterialExpressionId::from_u128(0xA205);
    let sample = MaterialExpressionId::from_u128(0xA206);
    let mut program = MaterialProgram::additive_sprite("Gradient texture sample");
    program.parameters.push(MaterialParameter {
        id: texture_parameter,
        name: "texture".into(),
        value_type: MaterialValueType::Texture2D(texture_descriptor()),
        evaluation_domain: MaterialEvaluationDomain::Instance,
        default: Some(MaterialValue::Texture2D(AssetId::from_u128(0xA207))),
    });
    program.expressions.extend([
        MaterialExpression {
            id: texture,
            kind: MaterialExpressionKind::Parameter(texture_parameter),
        },
        MaterialExpression {
            id: uv,
            kind: MaterialExpressionKind::Input(MaterialInput::Uv0),
        },
        MaterialExpression {
            id: ddx,
            kind: MaterialExpressionKind::DerivativeX { value: uv },
        },
        MaterialExpression {
            id: ddy,
            kind: MaterialExpressionKind::DerivativeY { value: uv },
        },
        MaterialExpression {
            id: sample,
            kind: MaterialExpressionKind::SampleTextureGradient {
                texture,
                uv,
                ddx,
                ddy,
            },
        },
    ]);
    program.outputs.color = sample;

    let analysis = program.analyze().unwrap();
    for expression in [ddx, ddy, sample] {
        assert_eq!(
            analysis.expressions[&expression].evaluation_domain,
            MaterialExpressionDomain::Fragment
        );
    }
    assert_eq!(
        analysis.expressions[&ddx].value_type,
        MaterialValueType::Vec2
    );
    assert_eq!(
        analysis.expressions[&sample].value_type,
        MaterialValueType::Color
    );

    program
        .expressions
        .iter_mut()
        .find(|expression| expression.id == ddx)
        .unwrap()
        .kind = MaterialExpressionKind::Constant(MaterialValue::Float(1.0));
    let report = program.validation_report();
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == DiagnosticCode::MaterialTypeMismatch && diagnostic.path.ends_with(".ddx")
    }));
}

#[test]
fn uv_transforms_round_trip_and_preserve_their_typed_semantic_sockets() {
    let uv = MaterialExpressionId::from_u128(0xAA01);
    let speed = MaterialExpressionId::from_u128(0xAA02);
    let time = MaterialExpressionId::from_u128(0xAA03);
    let pan = MaterialExpressionId::from_u128(0xAA04);
    let center = MaterialExpressionId::from_u128(0xAA05);
    let angle = MaterialExpressionId::from_u128(0xAA06);
    let rotate = MaterialExpressionId::from_u128(0xAA07);
    let scale_value = MaterialExpressionId::from_u128(0xAA08);
    let scale = MaterialExpressionId::from_u128(0xAA09);
    let alpha = MaterialExpressionId::from_u128(0xAA0A);
    let mut program = MaterialProgram::additive_sprite("UV transforms");
    program.expressions.extend([
        MaterialExpression {
            id: uv,
            kind: MaterialExpressionKind::Input(MaterialInput::Uv0),
        },
        MaterialExpression {
            id: speed,
            kind: MaterialExpressionKind::Constant(MaterialValue::Vec2([0.25, -0.5])),
        },
        MaterialExpression {
            id: time,
            kind: MaterialExpressionKind::Input(MaterialInput::EffectTime),
        },
        MaterialExpression {
            id: pan,
            kind: MaterialExpressionKind::PanUv { uv, speed, time },
        },
        MaterialExpression {
            id: center,
            kind: MaterialExpressionKind::Constant(MaterialValue::Vec2([0.5, 0.5])),
        },
        MaterialExpression {
            id: angle,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.75)),
        },
        MaterialExpression {
            id: rotate,
            kind: MaterialExpressionKind::RotateUv {
                uv: pan,
                center,
                angle,
            },
        },
        MaterialExpression {
            id: scale_value,
            kind: MaterialExpressionKind::Constant(MaterialValue::Vec2([1.5, 0.75])),
        },
        MaterialExpression {
            id: scale,
            kind: MaterialExpressionKind::ScaleUv {
                uv: rotate,
                center,
                scale: scale_value,
            },
        },
        MaterialExpression {
            id: alpha,
            kind: MaterialExpressionKind::ExtractComponent {
                value: scale,
                component: MaterialVectorComponent::X,
            },
        },
    ]);
    program.outputs.alpha = alpha;

    let analysis = program.analyze().unwrap();
    assert_eq!(
        analysis.expressions[&pan].value_type,
        MaterialValueType::Vec2
    );
    assert_eq!(
        analysis.expressions[&pan].evaluation_domain,
        MaterialExpressionDomain::Fragment
    );
    assert_eq!(
        analysis.expressions[&rotate].value_type,
        MaterialValueType::Vec2
    );
    assert_eq!(
        analysis.expressions[&rotate].evaluation_domain,
        MaterialExpressionDomain::Fragment
    );
    assert_eq!(
        analysis.expressions[&scale].value_type,
        MaterialValueType::Vec2
    );
    assert_eq!(
        analysis.expressions[&scale].evaluation_domain,
        MaterialExpressionDomain::Fragment
    );

    let encoded = program.to_pretty_ron().unwrap();
    assert!(encoded.contains("PanUv"));
    assert!(encoded.contains("RotateUv"));
    assert!(encoded.contains("ScaleUv"));
    assert_eq!(
        MaterialProgram::from_ron(&encoded).unwrap(),
        program.normalized()
    );
}

#[test]
fn pan_uv_reports_the_socket_with_the_wrong_type() {
    let uv = MaterialExpressionId::from_u128(0xAB01);
    let speed = MaterialExpressionId::from_u128(0xAB02);
    let time = MaterialExpressionId::from_u128(0xAB03);
    let pan = MaterialExpressionId::from_u128(0xAB04);
    let alpha = MaterialExpressionId::from_u128(0xAB05);
    let mut program = MaterialProgram::additive_sprite("Invalid panning UV");
    program.expressions.extend([
        MaterialExpression {
            id: uv,
            kind: MaterialExpressionKind::Input(MaterialInput::Uv0),
        },
        MaterialExpression {
            id: speed,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(1.0)),
        },
        MaterialExpression {
            id: time,
            kind: MaterialExpressionKind::Input(MaterialInput::ParticleColor),
        },
        MaterialExpression {
            id: pan,
            kind: MaterialExpressionKind::PanUv { uv, speed, time },
        },
        MaterialExpression {
            id: alpha,
            kind: MaterialExpressionKind::ExtractComponent {
                value: pan,
                component: MaterialVectorComponent::X,
            },
        },
    ]);
    program.outputs.alpha = alpha;

    let report = program.validation_report();

    assert!(!report.is_valid());
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == DiagnosticCode::MaterialTypeMismatch
            && diagnostic.path.ends_with(".speed")
            && diagnostic.message.contains("expects Vec2")
    }));
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == DiagnosticCode::MaterialTypeMismatch
            && diagnostic.path.ends_with(".time")
            && diagnostic.message.contains("expects Float")
    }));
}

#[test]
fn rotate_uv_reports_the_socket_with_the_wrong_type() {
    let uv = MaterialExpressionId::from_u128(0xAC01);
    let center = MaterialExpressionId::from_u128(0xAC02);
    let angle = MaterialExpressionId::from_u128(0xAC03);
    let rotate = MaterialExpressionId::from_u128(0xAC04);
    let alpha = MaterialExpressionId::from_u128(0xAC05);
    let mut program = MaterialProgram::additive_sprite("Invalid rotating UV");
    program.expressions.extend([
        MaterialExpression {
            id: uv,
            kind: MaterialExpressionKind::Input(MaterialInput::Uv0),
        },
        MaterialExpression {
            id: center,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.5)),
        },
        MaterialExpression {
            id: angle,
            kind: MaterialExpressionKind::Constant(MaterialValue::Vec2([0.0, 1.0])),
        },
        MaterialExpression {
            id: rotate,
            kind: MaterialExpressionKind::RotateUv { uv, center, angle },
        },
        MaterialExpression {
            id: alpha,
            kind: MaterialExpressionKind::ExtractComponent {
                value: rotate,
                component: MaterialVectorComponent::X,
            },
        },
    ]);
    program.outputs.alpha = alpha;

    let report = program.validation_report();

    assert!(!report.is_valid());
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == DiagnosticCode::MaterialTypeMismatch
            && diagnostic.path.ends_with(".center")
            && diagnostic.message.contains("expects Vec2")
    }));
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == DiagnosticCode::MaterialTypeMismatch
            && diagnostic.path.ends_with(".angle")
            && diagnostic.message.contains("Float radians")
    }));
}

#[test]
fn scale_uv_reports_the_socket_with_the_wrong_type() {
    let uv = MaterialExpressionId::from_u128(0xAD01);
    let center = MaterialExpressionId::from_u128(0xAD02);
    let scale_value = MaterialExpressionId::from_u128(0xAD03);
    let scale = MaterialExpressionId::from_u128(0xAD04);
    let alpha = MaterialExpressionId::from_u128(0xAD05);
    let mut program = MaterialProgram::additive_sprite("Invalid scaling UV");
    program.expressions.extend([
        MaterialExpression {
            id: uv,
            kind: MaterialExpressionKind::Input(MaterialInput::ParticleColor),
        },
        MaterialExpression {
            id: center,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.5)),
        },
        MaterialExpression {
            id: scale_value,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(2.0)),
        },
        MaterialExpression {
            id: scale,
            kind: MaterialExpressionKind::ScaleUv {
                uv,
                center,
                scale: scale_value,
            },
        },
        MaterialExpression {
            id: alpha,
            kind: MaterialExpressionKind::ExtractComponent {
                value: scale,
                component: MaterialVectorComponent::X,
            },
        },
    ]);
    program.outputs.alpha = alpha;

    let report = program.validation_report();

    for socket in [".uv", ".center", ".scale"] {
        assert!(report.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == DiagnosticCode::MaterialTypeMismatch
                && diagnostic.path.ends_with(socket)
                && diagnostic.message.contains("expects Vec2")
        }));
    }
}

#[test]
fn remap_promotes_scalar_bounds_and_round_trips_its_semantic_sockets() {
    let value = MaterialExpressionId::from_u128(0xAE01);
    let input_min = MaterialExpressionId::from_u128(0xAE02);
    let input_max = MaterialExpressionId::from_u128(0xAE03);
    let output_min = MaterialExpressionId::from_u128(0xAE04);
    let output_max = MaterialExpressionId::from_u128(0xAE05);
    let remap = MaterialExpressionId::from_u128(0xAE06);
    let alpha = MaterialExpressionId::from_u128(0xAE07);
    let mut program = MaterialProgram::additive_sprite("Vector remap");
    program.expressions.extend([
        MaterialExpression {
            id: value,
            kind: MaterialExpressionKind::Input(MaterialInput::Uv0),
        },
        MaterialExpression {
            id: input_min,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.0)),
        },
        MaterialExpression {
            id: input_max,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(1.0)),
        },
        MaterialExpression {
            id: output_min,
            kind: MaterialExpressionKind::Constant(MaterialValue::Vec2([-1.0, -2.0])),
        },
        MaterialExpression {
            id: output_max,
            kind: MaterialExpressionKind::Constant(MaterialValue::Vec2([1.0, 2.0])),
        },
        MaterialExpression {
            id: remap,
            kind: MaterialExpressionKind::Remap {
                value,
                input_min,
                input_max,
                output_min,
                output_max,
            },
        },
        MaterialExpression {
            id: alpha,
            kind: MaterialExpressionKind::ExtractComponent {
                value: remap,
                component: MaterialVectorComponent::X,
            },
        },
    ]);
    program.outputs.alpha = alpha;

    let analysis = program.analyze().unwrap();
    assert_eq!(
        analysis.expressions[&remap].value_type,
        MaterialValueType::Vec2
    );
    assert_eq!(
        analysis.expressions[&remap].evaluation_domain,
        MaterialExpressionDomain::Fragment
    );
    let encoded = program.to_pretty_ron().unwrap();
    assert!(encoded.contains("Remap"));
    assert_eq!(
        MaterialProgram::from_ron(&encoded).unwrap(),
        program.normalized()
    );
}

#[test]
fn remap_rejects_non_numeric_and_mismatched_vector_sockets() {
    let value = MaterialExpressionId::from_u128(0xAF01);
    let input_min = MaterialExpressionId::from_u128(0xAF02);
    let input_max = MaterialExpressionId::from_u128(0xAF03);
    let output_min = MaterialExpressionId::from_u128(0xAF04);
    let output_max = MaterialExpressionId::from_u128(0xAF05);
    let remap = MaterialExpressionId::from_u128(0xAF06);
    let alpha = MaterialExpressionId::from_u128(0xAF07);
    let mut program = MaterialProgram::additive_sprite("Invalid remap");
    program.expressions.extend([
        MaterialExpression {
            id: value,
            kind: MaterialExpressionKind::Constant(MaterialValue::Vec2([0.0, 1.0])),
        },
        MaterialExpression {
            id: input_min,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.0)),
        },
        MaterialExpression {
            id: input_max,
            kind: MaterialExpressionKind::Constant(MaterialValue::Vec3([1.0; 3])),
        },
        MaterialExpression {
            id: output_min,
            kind: MaterialExpressionKind::Constant(MaterialValue::Bool(false)),
        },
        MaterialExpression {
            id: output_max,
            kind: MaterialExpressionKind::Constant(MaterialValue::Vec2([1.0; 2])),
        },
        MaterialExpression {
            id: remap,
            kind: MaterialExpressionKind::Remap {
                value,
                input_min,
                input_max,
                output_min,
                output_max,
            },
        },
        MaterialExpression {
            id: alpha,
            kind: MaterialExpressionKind::ExtractComponent {
                value: remap,
                component: MaterialVectorComponent::X,
            },
        },
    ]);
    program.outputs.alpha = alpha;

    let report = program.validation_report();

    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == DiagnosticCode::MaterialTypeMismatch
            && diagnostic.path.ends_with(".input_max")
            && diagnostic.message.contains("received Vec3")
    }));
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == DiagnosticCode::MaterialTypeMismatch
            && diagnostic.path.ends_with(".output_min")
            && diagnostic.message.contains("received Bool")
    }));
}

#[test]
fn smoothstep_promotes_scalar_edges_and_round_trips_its_semantic_sockets() {
    let edge_min = MaterialExpressionId::from_u128(0xB101);
    let edge_max = MaterialExpressionId::from_u128(0xB102);
    let value = MaterialExpressionId::from_u128(0xB103);
    let smoothstep = MaterialExpressionId::from_u128(0xB104);
    let alpha = MaterialExpressionId::from_u128(0xB105);
    let mut program = MaterialProgram::additive_sprite("Vector smoothstep");
    program.expressions.extend([
        MaterialExpression {
            id: edge_min,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.25)),
        },
        MaterialExpression {
            id: edge_max,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.75)),
        },
        MaterialExpression {
            id: value,
            kind: MaterialExpressionKind::Input(MaterialInput::Uv0),
        },
        MaterialExpression {
            id: smoothstep,
            kind: MaterialExpressionKind::Smoothstep {
                edge_min,
                edge_max,
                value,
            },
        },
        MaterialExpression {
            id: alpha,
            kind: MaterialExpressionKind::ExtractComponent {
                value: smoothstep,
                component: MaterialVectorComponent::X,
            },
        },
    ]);
    program.outputs.alpha = alpha;

    let analysis = program.analyze().unwrap();
    assert_eq!(
        analysis.expressions[&smoothstep].value_type,
        MaterialValueType::Vec2
    );
    assert_eq!(
        analysis.expressions[&smoothstep].evaluation_domain,
        MaterialExpressionDomain::Fragment
    );
    let encoded = program.to_pretty_ron().unwrap();
    assert!(encoded.contains("Smoothstep"));
    assert_eq!(
        MaterialProgram::from_ron(&encoded).unwrap(),
        program.normalized()
    );
}

#[test]
fn smoothstep_reports_each_incompatible_numeric_socket() {
    let edge_min = MaterialExpressionId::from_u128(0xB201);
    let edge_max = MaterialExpressionId::from_u128(0xB202);
    let value = MaterialExpressionId::from_u128(0xB203);
    let smoothstep = MaterialExpressionId::from_u128(0xB204);
    let alpha = MaterialExpressionId::from_u128(0xB205);
    let mut program = MaterialProgram::additive_sprite("Invalid smoothstep");
    program.expressions.extend([
        MaterialExpression {
            id: edge_min,
            kind: MaterialExpressionKind::Constant(MaterialValue::Bool(false)),
        },
        MaterialExpression {
            id: edge_max,
            kind: MaterialExpressionKind::Constant(MaterialValue::Vec3([1.0; 3])),
        },
        MaterialExpression {
            id: value,
            kind: MaterialExpressionKind::Constant(MaterialValue::Vec2([0.5; 2])),
        },
        MaterialExpression {
            id: smoothstep,
            kind: MaterialExpressionKind::Smoothstep {
                edge_min,
                edge_max,
                value,
            },
        },
        MaterialExpression {
            id: alpha,
            kind: MaterialExpressionKind::ExtractComponent {
                value: smoothstep,
                component: MaterialVectorComponent::X,
            },
        },
    ]);
    program.outputs.alpha = alpha;

    let report = program.validation_report();
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == DiagnosticCode::MaterialTypeMismatch
            && diagnostic.path.ends_with(".edge_min")
            && diagnostic.message.contains("received Bool")
    }));
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == DiagnosticCode::MaterialTypeMismatch
            && diagnostic.path.ends_with(".value")
            && diagnostic.message.contains("received Vec2")
    }));
}

#[test]
fn fresnel_accepts_sprite_billboard_inputs_and_returns_a_fragment_mask() {
    let normal = MaterialExpressionId::from_u128(0xB211);
    let view = MaterialExpressionId::from_u128(0xB212);
    let power = MaterialExpressionId::from_u128(0xB213);
    let fresnel = MaterialExpressionId::from_u128(0xB214);
    let mut program = MaterialProgram::additive_sprite("Fresnel");
    program.expressions.extend([
        MaterialExpression {
            id: normal,
            kind: MaterialExpressionKind::Input(MaterialInput::Normal),
        },
        MaterialExpression {
            id: view,
            kind: MaterialExpressionKind::Input(MaterialInput::ViewDirection),
        },
        MaterialExpression {
            id: power,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(3.0)),
        },
        MaterialExpression {
            id: fresnel,
            kind: MaterialExpressionKind::Fresnel {
                normal,
                view,
                power,
            },
        },
    ]);
    program.outputs.alpha = fresnel;

    let analysis = program.analyze().unwrap();
    assert_eq!(
        analysis.expressions[&fresnel].value_type,
        MaterialValueType::Float
    );
    assert_eq!(
        analysis.expressions[&fresnel].evaluation_domain,
        MaterialExpressionDomain::Fragment
    );
    let encoded = program.to_pretty_ron().unwrap();
    assert!(encoded.contains("Fresnel"));
    assert_eq!(
        MaterialProgram::from_ron(&encoded).unwrap(),
        program.normalized()
    );
}

#[test]
fn radial_mask_round_trips_typed_semantic_sockets_and_outputs_a_fragment_mask() {
    let uv = MaterialExpressionId::from_u128(0xB301);
    let center = MaterialExpressionId::from_u128(0xB302);
    let radius = MaterialExpressionId::from_u128(0xB303);
    let softness = MaterialExpressionId::from_u128(0xB304);
    let invert = MaterialExpressionId::from_u128(0xB305);
    let radial_mask = MaterialExpressionId::from_u128(0xB306);
    let mut program = MaterialProgram::additive_sprite("Radial mask");
    program.expressions.extend([
        MaterialExpression {
            id: uv,
            kind: MaterialExpressionKind::Input(MaterialInput::Uv0),
        },
        MaterialExpression {
            id: center,
            kind: MaterialExpressionKind::Constant(MaterialValue::Vec2([0.5, 0.5])),
        },
        MaterialExpression {
            id: radius,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.45)),
        },
        MaterialExpression {
            id: softness,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.1)),
        },
        MaterialExpression {
            id: invert,
            kind: MaterialExpressionKind::Constant(MaterialValue::Bool(false)),
        },
        MaterialExpression {
            id: radial_mask,
            kind: MaterialExpressionKind::RadialMask {
                uv,
                center,
                radius,
                softness,
                invert,
            },
        },
    ]);
    program.outputs.alpha = radial_mask;

    let analysis = program.analyze().unwrap();
    assert_eq!(
        analysis.expressions[&radial_mask].value_type,
        MaterialValueType::Float
    );
    assert_eq!(
        analysis.expressions[&radial_mask].evaluation_domain,
        MaterialExpressionDomain::Fragment
    );
    let encoded = program.to_pretty_ron().unwrap();
    assert!(encoded.contains("RadialMask"));
    assert_eq!(
        MaterialProgram::from_ron(&encoded).unwrap(),
        program.normalized()
    );
}

#[test]
fn radial_mask_reports_each_mistyped_socket() {
    let uv = MaterialExpressionId::from_u128(0xB401);
    let center = MaterialExpressionId::from_u128(0xB402);
    let radius = MaterialExpressionId::from_u128(0xB403);
    let softness = MaterialExpressionId::from_u128(0xB404);
    let invert = MaterialExpressionId::from_u128(0xB405);
    let radial_mask = MaterialExpressionId::from_u128(0xB406);
    let mut program = MaterialProgram::additive_sprite("Invalid radial mask");
    program.expressions.extend([
        MaterialExpression {
            id: uv,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.0)),
        },
        MaterialExpression {
            id: center,
            kind: MaterialExpressionKind::Constant(MaterialValue::Vec3([0.0; 3])),
        },
        MaterialExpression {
            id: radius,
            kind: MaterialExpressionKind::Constant(MaterialValue::Vec2([1.0; 2])),
        },
        MaterialExpression {
            id: softness,
            kind: MaterialExpressionKind::Constant(MaterialValue::Bool(false)),
        },
        MaterialExpression {
            id: invert,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.0)),
        },
        MaterialExpression {
            id: radial_mask,
            kind: MaterialExpressionKind::RadialMask {
                uv,
                center,
                radius,
                softness,
                invert,
            },
        },
    ]);
    program.outputs.alpha = radial_mask;

    let report = program.validation_report();
    for socket in [".uv", ".center", ".radius", ".softness", ".invert"] {
        assert!(report.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == DiagnosticCode::MaterialTypeMismatch
                && diagnostic.path.ends_with(socket)
                && diagnostic.message.contains("RadialMask")
        }));
    }
}

#[test]
fn dissolve_round_trips_typed_semantic_sockets_and_outputs_a_particle_mask() {
    let source = MaterialExpressionId::from_u128(0xB501);
    let threshold = MaterialExpressionId::from_u128(0xB502);
    let edge_width = MaterialExpressionId::from_u128(0xB503);
    let invert = MaterialExpressionId::from_u128(0xB504);
    let dissolve = MaterialExpressionId::from_u128(0xB505);
    let mut program = MaterialProgram::additive_sprite("Dissolve");
    program.expressions.extend([
        MaterialExpression {
            id: source,
            kind: MaterialExpressionKind::Input(MaterialInput::ParticleRandom),
        },
        MaterialExpression {
            id: threshold,
            kind: MaterialExpressionKind::Input(MaterialInput::ParticleNormalizedAge),
        },
        MaterialExpression {
            id: edge_width,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.08)),
        },
        MaterialExpression {
            id: invert,
            kind: MaterialExpressionKind::Constant(MaterialValue::Bool(false)),
        },
        MaterialExpression {
            id: dissolve,
            kind: MaterialExpressionKind::Dissolve {
                source,
                threshold,
                edge_width,
                invert,
            },
        },
    ]);
    program.outputs.alpha = dissolve;

    let analysis = program.analyze().unwrap();
    assert_eq!(
        analysis.expressions[&dissolve].value_type,
        MaterialValueType::Float
    );
    assert_eq!(
        analysis.expressions[&dissolve].evaluation_domain,
        MaterialExpressionDomain::Particle
    );
    let encoded = program.to_pretty_ron().unwrap();
    assert!(encoded.contains("Dissolve"));
    assert_eq!(
        MaterialProgram::from_ron(&encoded).unwrap(),
        program.normalized()
    );
}

#[test]
fn dissolve_reports_each_mistyped_socket() {
    let source = MaterialExpressionId::from_u128(0xB601);
    let threshold = MaterialExpressionId::from_u128(0xB602);
    let edge_width = MaterialExpressionId::from_u128(0xB603);
    let invert = MaterialExpressionId::from_u128(0xB604);
    let dissolve = MaterialExpressionId::from_u128(0xB605);
    let mut program = MaterialProgram::additive_sprite("Invalid dissolve");
    program.expressions.extend([
        MaterialExpression {
            id: source,
            kind: MaterialExpressionKind::Constant(MaterialValue::Vec2([0.0; 2])),
        },
        MaterialExpression {
            id: threshold,
            kind: MaterialExpressionKind::Constant(MaterialValue::Bool(false)),
        },
        MaterialExpression {
            id: edge_width,
            kind: MaterialExpressionKind::Constant(MaterialValue::Vec3([0.0; 3])),
        },
        MaterialExpression {
            id: invert,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.0)),
        },
        MaterialExpression {
            id: dissolve,
            kind: MaterialExpressionKind::Dissolve {
                source,
                threshold,
                edge_width,
                invert,
            },
        },
    ]);
    program.outputs.alpha = dissolve;

    let report = program.validation_report();
    for socket in [".source", ".threshold", ".edge_width", ".invert"] {
        assert!(report.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == DiagnosticCode::MaterialTypeMismatch
                && diagnostic.path.ends_with(socket)
                && diagnostic.message.contains("Dissolve")
        }));
    }
}

#[test]
fn dissolve_edge_round_trips_typed_semantic_sockets_and_outputs_a_particle_mask() {
    let source = MaterialExpressionId::from_u128(0xB701);
    let threshold = MaterialExpressionId::from_u128(0xB702);
    let edge_width = MaterialExpressionId::from_u128(0xB703);
    let invert = MaterialExpressionId::from_u128(0xB704);
    let dissolve_edge = MaterialExpressionId::from_u128(0xB705);
    let mut program = MaterialProgram::additive_sprite("Dissolve edge");
    program.expressions.extend([
        MaterialExpression {
            id: source,
            kind: MaterialExpressionKind::Input(MaterialInput::ParticleRandom),
        },
        MaterialExpression {
            id: threshold,
            kind: MaterialExpressionKind::Input(MaterialInput::ParticleNormalizedAge),
        },
        MaterialExpression {
            id: edge_width,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.08)),
        },
        MaterialExpression {
            id: invert,
            kind: MaterialExpressionKind::Constant(MaterialValue::Bool(false)),
        },
        MaterialExpression {
            id: dissolve_edge,
            kind: MaterialExpressionKind::DissolveEdge {
                source,
                threshold,
                edge_width,
                invert,
            },
        },
    ]);
    program.outputs.alpha = dissolve_edge;

    let analysis = program.analyze().unwrap();
    assert_eq!(
        analysis.expressions[&dissolve_edge].value_type,
        MaterialValueType::Float
    );
    assert_eq!(
        analysis.expressions[&dissolve_edge].evaluation_domain,
        MaterialExpressionDomain::Particle
    );
    let encoded = program.to_pretty_ron().unwrap();
    assert!(encoded.contains("DissolveEdge"));
    assert_eq!(
        MaterialProgram::from_ron(&encoded).unwrap(),
        program.normalized()
    );
}

#[test]
fn dissolve_edge_reports_each_mistyped_socket() {
    let source = MaterialExpressionId::from_u128(0xB801);
    let threshold = MaterialExpressionId::from_u128(0xB802);
    let edge_width = MaterialExpressionId::from_u128(0xB803);
    let invert = MaterialExpressionId::from_u128(0xB804);
    let dissolve_edge = MaterialExpressionId::from_u128(0xB805);
    let mut program = MaterialProgram::additive_sprite("Invalid dissolve edge");
    program.expressions.extend([
        MaterialExpression {
            id: source,
            kind: MaterialExpressionKind::Constant(MaterialValue::Vec2([0.0; 2])),
        },
        MaterialExpression {
            id: threshold,
            kind: MaterialExpressionKind::Constant(MaterialValue::Bool(false)),
        },
        MaterialExpression {
            id: edge_width,
            kind: MaterialExpressionKind::Constant(MaterialValue::Vec3([0.0; 3])),
        },
        MaterialExpression {
            id: invert,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.0)),
        },
        MaterialExpression {
            id: dissolve_edge,
            kind: MaterialExpressionKind::DissolveEdge {
                source,
                threshold,
                edge_width,
                invert,
            },
        },
    ]);
    program.outputs.alpha = dissolve_edge;

    let report = program.validation_report();
    for socket in [".source", ".threshold", ".edge_width", ".invert"] {
        assert!(report.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == DiagnosticCode::MaterialTypeMismatch
                && diagnostic.path.ends_with(socket)
                && diagnostic.message.contains("DissolveEdge")
        }));
    }
}

#[test]
fn depth_fade_round_trips_linear_depth_sockets() {
    let scene_depth = MaterialExpressionId::from_u128(0xb901);
    let pixel_depth = MaterialExpressionId::from_u128(0xb902);
    let fade_distance = MaterialExpressionId::from_u128(0xb903);
    let invert = MaterialExpressionId::from_u128(0xb904);
    let depth_fade = MaterialExpressionId::from_u128(0xb905);
    let mut program = MaterialProgram::additive_sprite("Depth fade");
    program.expressions.extend([
        MaterialExpression {
            id: scene_depth,
            kind: MaterialExpressionKind::Input(MaterialInput::SceneDepth),
        },
        MaterialExpression {
            id: pixel_depth,
            kind: MaterialExpressionKind::Input(MaterialInput::PixelDepth),
        },
        MaterialExpression {
            id: fade_distance,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.5)),
        },
        MaterialExpression {
            id: invert,
            kind: MaterialExpressionKind::Constant(MaterialValue::Bool(false)),
        },
        MaterialExpression {
            id: depth_fade,
            kind: MaterialExpressionKind::DepthFade {
                scene_depth,
                pixel_depth,
                fade_distance,
                invert,
            },
        },
    ]);
    program.outputs.alpha = depth_fade;

    let analysis = program.analyze().unwrap();
    assert_eq!(
        analysis.expressions[&depth_fade].value_type,
        MaterialValueType::Float
    );
    assert_eq!(
        analysis.expressions[&depth_fade].evaluation_domain,
        MaterialExpressionDomain::Fragment
    );
    let encoded = program.to_pretty_ron().unwrap();
    assert!(encoded.contains("DepthFade"));
    assert_eq!(
        MaterialProgram::from_ron(&encoded).unwrap(),
        program.normalized()
    );
}

#[test]
fn depth_fade_reports_each_mistyped_socket() {
    let scene_depth = MaterialExpressionId::from_u128(0xba01);
    let pixel_depth = MaterialExpressionId::from_u128(0xba02);
    let fade_distance = MaterialExpressionId::from_u128(0xba03);
    let invert = MaterialExpressionId::from_u128(0xba04);
    let depth_fade = MaterialExpressionId::from_u128(0xba05);
    let mut program = MaterialProgram::additive_sprite("Invalid depth fade");
    program.expressions.extend([
        MaterialExpression {
            id: scene_depth,
            kind: MaterialExpressionKind::Constant(MaterialValue::Vec2([0.0; 2])),
        },
        MaterialExpression {
            id: pixel_depth,
            kind: MaterialExpressionKind::Constant(MaterialValue::Bool(false)),
        },
        MaterialExpression {
            id: fade_distance,
            kind: MaterialExpressionKind::Constant(MaterialValue::Vec3([0.0; 3])),
        },
        MaterialExpression {
            id: invert,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.0)),
        },
        MaterialExpression {
            id: depth_fade,
            kind: MaterialExpressionKind::DepthFade {
                scene_depth,
                pixel_depth,
                fade_distance,
                invert,
            },
        },
    ]);
    program.outputs.alpha = depth_fade;

    let report = program.validation_report();
    for socket in [".scene_depth", ".pixel_depth", ".fade_distance", ".invert"] {
        assert!(report.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == DiagnosticCode::MaterialTypeMismatch
                && diagnostic.path.ends_with(socket)
                && diagnostic.message.contains("DepthFade")
        }));
    }
}

#[test]
fn soft_particle_round_trips_typed_alpha_and_depth_sockets() {
    let alpha = MaterialExpressionId::from_u128(0xbb01);
    let scene_depth = MaterialExpressionId::from_u128(0xbb02);
    let pixel_depth = MaterialExpressionId::from_u128(0xbb03);
    let fade_distance = MaterialExpressionId::from_u128(0xbb04);
    let invert = MaterialExpressionId::from_u128(0xbb05);
    let soft_particle = MaterialExpressionId::from_u128(0xbb06);
    let mut program = MaterialProgram::additive_sprite("Soft particle");
    program.expressions.extend([
        MaterialExpression {
            id: alpha,
            kind: MaterialExpressionKind::Input(MaterialInput::ParticleOpacity),
        },
        MaterialExpression {
            id: scene_depth,
            kind: MaterialExpressionKind::Input(MaterialInput::SceneDepth),
        },
        MaterialExpression {
            id: pixel_depth,
            kind: MaterialExpressionKind::Input(MaterialInput::PixelDepth),
        },
        MaterialExpression {
            id: fade_distance,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.5)),
        },
        MaterialExpression {
            id: invert,
            kind: MaterialExpressionKind::Constant(MaterialValue::Bool(false)),
        },
        MaterialExpression {
            id: soft_particle,
            kind: MaterialExpressionKind::SoftParticle {
                alpha,
                scene_depth,
                pixel_depth,
                fade_distance,
                invert,
            },
        },
    ]);
    program.outputs.alpha = soft_particle;

    let analysis = program.analyze().unwrap();
    assert_eq!(
        analysis.expressions[&soft_particle].value_type,
        MaterialValueType::Float
    );
    assert_eq!(
        analysis.expressions[&soft_particle].evaluation_domain,
        MaterialExpressionDomain::Fragment
    );
    let encoded = program.to_pretty_ron().unwrap();
    assert!(encoded.contains("SoftParticle"));
    assert_eq!(
        MaterialProgram::from_ron(&encoded).unwrap(),
        program.normalized()
    );
}

#[test]
fn soft_particle_reports_each_mistyped_socket() {
    let alpha = MaterialExpressionId::from_u128(0xbc01);
    let scene_depth = MaterialExpressionId::from_u128(0xbc02);
    let pixel_depth = MaterialExpressionId::from_u128(0xbc03);
    let fade_distance = MaterialExpressionId::from_u128(0xbc04);
    let invert = MaterialExpressionId::from_u128(0xbc05);
    let soft_particle = MaterialExpressionId::from_u128(0xbc06);
    let mut program = MaterialProgram::additive_sprite("Invalid soft particle");
    program.expressions.extend([
        MaterialExpression {
            id: alpha,
            kind: MaterialExpressionKind::Constant(MaterialValue::Vec2([0.0; 2])),
        },
        MaterialExpression {
            id: scene_depth,
            kind: MaterialExpressionKind::Constant(MaterialValue::Bool(false)),
        },
        MaterialExpression {
            id: pixel_depth,
            kind: MaterialExpressionKind::Constant(MaterialValue::Vec3([0.0; 3])),
        },
        MaterialExpression {
            id: fade_distance,
            kind: MaterialExpressionKind::Constant(MaterialValue::Vec4([0.0; 4])),
        },
        MaterialExpression {
            id: invert,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.0)),
        },
        MaterialExpression {
            id: soft_particle,
            kind: MaterialExpressionKind::SoftParticle {
                alpha,
                scene_depth,
                pixel_depth,
                fade_distance,
                invert,
            },
        },
    ]);
    program.outputs.alpha = soft_particle;

    let report = program.validation_report();
    for socket in [
        ".alpha",
        ".scene_depth",
        ".pixel_depth",
        ".fade_distance",
        ".invert",
    ] {
        assert!(report.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == DiagnosticCode::MaterialTypeMismatch
                && diagnostic.path.ends_with(socket)
                && diagnostic.message.contains("SoftParticle")
        }));
    }
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
