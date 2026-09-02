use aestra_authoring::{
    MaterialAuthoringDocument, MaterialInspectionError, MaterialInspectionTarget, MaterialInspector,
};
use aestra_core::{
    EffectAsset, Emitter, MaterialExpressionId, MaterialId, MaterialParameterId, MaterialProgramId,
    material::{
        MaterialExpression, MaterialExpressionKind, MaterialInstance, MaterialOutputs,
        MaterialParameterValue, MaterialProgram, MaterialProgramRef, MaterialRenderState,
        MaterialValue,
    },
};
use std::collections::BTreeMap;

fn inspectable_program(id: MaterialProgramId) -> MaterialProgram {
    let source = MaterialExpressionId::from_u128(0x7101);
    let input_min = MaterialExpressionId::from_u128(0x7102);
    let input_max = MaterialExpressionId::from_u128(0x7103);
    let output_min = MaterialExpressionId::from_u128(0x7104);
    let output_max = MaterialExpressionId::from_u128(0x7105);
    let remap = MaterialExpressionId::from_u128(0x7106);
    let color = MaterialExpressionId::from_u128(0x7107);
    let mut program = MaterialProgram::additive_sprite("Inspectable");
    program.id = id;
    program.expressions = vec![
        MaterialExpression {
            id: source,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.5)),
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
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.0)),
        },
        MaterialExpression {
            id: output_max,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(1.0)),
        },
        MaterialExpression {
            id: remap,
            kind: MaterialExpressionKind::Remap {
                value: source,
                input_min,
                input_max,
                output_min,
                output_max,
            },
        },
        MaterialExpression {
            id: color,
            kind: MaterialExpressionKind::Constant(MaterialValue::ColorSrgb([1.0; 4])),
        },
    ];
    program.outputs = MaterialOutputs {
        color,
        alpha: remap,
    };
    program
}

fn inspection_document() -> (MaterialAuthoringDocument, MaterialProgramId, MaterialId) {
    let mut effect = EffectAsset::new("Inspection", 2.0);
    effect.emitters.push(Emitter::basic_sprite("Emitter", 2.0));
    let program = MaterialProgramId::from_u128(0x7100);
    let instance = MaterialId::from_u128(0x7110);
    effect.material_instances.push(MaterialInstance {
        id: instance,
        program: MaterialProgramRef::Project(program),
        values: BTreeMap::new(),
        render_state: MaterialRenderState::additive_sprite(),
    });
    (
        MaterialAuthoringDocument::new(effect, vec![inspectable_program(program)]),
        program,
        instance,
    )
}

#[test]
fn material_inspection_is_serializable_deterministic_and_non_mutating() {
    let (document, program, instance) = inspection_document();
    let before = document.clone();
    let target = MaterialInspectionTarget::Instance(instance);

    let encoded_target = ron::to_string(&target).unwrap();
    assert_eq!(
        ron::from_str::<MaterialInspectionTarget>(&encoded_target).unwrap(),
        target
    );
    let report = MaterialInspector::inspect(&document, target).unwrap();

    assert_eq!(document, before);
    assert!(report.is_valid());
    assert_eq!(report.program.id, program);
    assert_eq!(report.instance.as_ref().map(|item| item.id), Some(instance));
    assert_eq!(report.controls.as_ref().unwrap().material, Some(instance));
    assert!(report.stack.is_some());
    assert!(
        !report.operations.is_empty(),
        "valid stack should advertise compiler-approved semantic operations"
    );
    assert_eq!(
        MaterialInspector::inspect(&document, target).unwrap(),
        report,
        "inspection ordering and content must be deterministic"
    );
    let encoded_report = ron::to_string(&report).unwrap();
    assert_eq!(
        ron::from_str::<aestra_authoring::MaterialInspectionReport>(&encoded_report).unwrap(),
        report
    );
}

#[test]
fn invalid_material_inspection_returns_diagnostics_without_misleading_projections() {
    let (mut document, program, _) = inspection_document();
    let color = document.programs[0].outputs.color;
    document.programs[0].outputs.alpha = color;
    let before = document.clone();

    let report =
        MaterialInspector::inspect(&document, MaterialInspectionTarget::Program(program)).unwrap();

    assert_eq!(document, before);
    assert!(!report.is_valid());
    assert!(!report.diagnostics.diagnostics.is_empty());
    assert!(report.controls.is_none());
    assert!(report.stack.is_none());
    assert!(report.operations.is_empty());
    assert!(report.presets.is_empty());
    assert_eq!(report.program, before.programs[0]);
}

#[test]
fn invalid_instance_inspection_keeps_valid_program_capabilities_and_reports_the_instance() {
    let (mut document, _, instance) = inspection_document();
    let unknown = MaterialParameterId::from_u128(0x72ff);
    document.effect.material_instances[0].values.insert(
        unknown,
        MaterialParameterValue::Constant(MaterialValue::Float(1.0)),
    );

    let report =
        MaterialInspector::inspect(&document, MaterialInspectionTarget::Instance(instance))
            .unwrap();

    assert!(!report.is_valid());
    assert!(report.controls.is_none());
    assert!(report.stack.is_some());
    assert!(!report.operations.is_empty());
    assert!(
        report
            .diagnostics
            .diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.path.ends_with(&format!(".values[{unknown}]")) })
    );
}

#[test]
fn material_inspection_rejects_stale_stable_targets() {
    let (document, _, _) = inspection_document();
    let missing_program = MaterialProgramId::from_u128(0x73fe);
    let missing_instance = MaterialId::from_u128(0x73ff);

    assert_eq!(
        MaterialInspector::inspect(
            &document,
            MaterialInspectionTarget::Program(missing_program),
        )
        .unwrap_err(),
        MaterialInspectionError::ProgramNotFound(missing_program)
    );
    assert_eq!(
        MaterialInspector::inspect(
            &document,
            MaterialInspectionTarget::Instance(missing_instance),
        )
        .unwrap_err(),
        MaterialInspectionError::InstanceNotFound(missing_instance)
    );
}
