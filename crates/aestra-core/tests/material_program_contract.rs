use aestra_core::{
    DiagnosticCode, DiagnosticSeverity, MaterialExpressionId, MaterialId, MaterialParameterId,
    MaterialProgramId, ParameterId,
    material::{
        MaterialEvaluationDomain, MaterialExpression, MaterialExpressionKind, MaterialInstance,
        MaterialParameter, MaterialParameterValue, MaterialProgram, MaterialProgramRef,
        MaterialRenderState, MaterialSchemaVersion, MaterialValue, MaterialValueType,
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
