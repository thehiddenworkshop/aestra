use aestra_authoring::{
    LegacyMaterialMigrationError, MaterialApi, MaterialApiErrorCode, MaterialApiRequest,
    MaterialApiResponse, MaterialAuthoringDocument, MaterialCommand, MaterialCommandError,
    MaterialCommandHistory, MaterialCompilationReporter, MaterialConnectionTarget,
    MaterialInspectionError, MaterialInspectionTarget, MaterialInspector, MaterialOutputSocket,
    MaterialParameterBinding, MaterialToolCommand, MaterialToolPlanner, MaterialTransaction,
    plan_legacy_sprite_material_migration,
};
use aestra_compiler::{MaterialGraphCreateKind, MaterialGraphFunction};
use aestra_core::{
    DiagnosticCode, EffectAsset, Emitter, EmitterId, MaterialExpressionId, MaterialFunctionId,
    MaterialId, MaterialParameterId, RendererId,
    material::{
        MaterialExpressionKind, MaterialInstance, MaterialProgram, MaterialProgramRef,
        MaterialRenderState, MaterialValue, MaterialValueType,
    },
};
use std::collections::BTreeMap;

fn document() -> MaterialAuthoringDocument {
    MaterialAuthoringDocument::standalone(vec![MaterialProgram::additive_sprite("Unused")])
}

#[test]
fn standalone_graph_can_be_created_edited_inspected_compiled_and_undone() {
    let mut document = document();
    let before = document.clone();
    let program = document.programs[0].id;
    let mut history = MaterialCommandHistory::default();
    let plan = MaterialToolPlanner::plan(
        &document,
        MaterialToolCommand::CreateMaterialGraphNode {
            program,
            kind: MaterialGraphCreateKind::Constant(MaterialValueType::Float),
            source: None,
            target: Some(MaterialConnectionTarget::ProgramOutput(
                MaterialOutputSocket::Alpha,
            )),
        },
    )
    .unwrap();
    assert_eq!(document, before, "planning must be read-only");
    assert!(
        !history
            .execute(&mut document, plan.transaction)
            .unwrap()
            .is_empty()
    );
    let expression = document.programs[0].outputs.alpha;
    let plan = MaterialToolPlanner::plan(
        &document,
        MaterialToolCommand::ReplaceMaterialExpression {
            program,
            expression,
            replacement: MaterialExpressionKind::Constant(MaterialValue::Float(0.25)),
        },
    )
    .unwrap();
    history.execute(&mut document, plan.transaction).unwrap();
    let edited = document.clone();
    assert!(document.effect.is_none());
    document.validate().unwrap();

    let target = MaterialInspectionTarget::Program(program);
    let inspection = MaterialInspector::inspect(&document, target).unwrap();
    assert!(inspection.is_valid());
    assert!(inspection.instance.is_none());
    assert!(!inspection.graph.nodes.is_empty());
    let compilation = MaterialCompilationReporter::compile(&document, target).unwrap();
    assert!(compilation.is_valid());
    assert!(compilation.ir.is_some());
    assert!(compilation.instance.is_none());

    history.undo(&mut document).unwrap().unwrap();
    history.undo(&mut document).unwrap().unwrap();
    assert_eq!(document, before);
    assert!(!history.can_undo());
    history.redo(&mut document).unwrap().unwrap();
    history.redo(&mut document).unwrap().unwrap();
    assert_eq!(document, edited);
    assert!(!history.can_redo());
}

#[test]
fn standalone_invalid_edits_roll_back_and_preserve_redo() {
    let mut document = document();
    let before = document.clone();
    let program = document.programs[0].id;
    let mut renamed = document.programs[0].clone();
    renamed.name = "Renamed".into();
    let mut history = MaterialCommandHistory::default();
    history
        .execute(
            &mut document,
            MaterialTransaction::single(
                "Rename",
                MaterialCommand::ReplaceMaterialProgram {
                    id: program,
                    program: renamed,
                },
            ),
        )
        .unwrap();
    history.undo(&mut document).unwrap().unwrap();
    let result = history.execute(
        &mut document,
        MaterialTransaction::single(
            "Broken output",
            MaterialCommand::SetMaterialOutput {
                program,
                output: MaterialOutputSocket::Alpha,
                expression: MaterialExpressionId::from_u128(0xab400),
            },
        ),
    );
    assert!(matches!(result, Err(MaterialCommandError::Validation(_))));
    assert_eq!(document, before);
    assert!(!history.can_undo());
    assert!(history.can_redo());
    history.redo(&mut document).unwrap().unwrap();
    assert_eq!(document.programs[0].name, "Renamed");
}

#[test]
fn effect_only_commands_reject_standalone_atomically() {
    let before = document();
    let program = before.programs[0].id;
    let instance = MaterialInstance {
        id: MaterialId::from_u128(0xab401),
        program: MaterialProgramRef::Project(program),
        values: BTreeMap::new(),
        render_state: MaterialRenderState::additive_sprite(),
    };
    let commands = [
        MaterialCommand::AddMaterialInstance {
            instance: instance.clone(),
            index: 0,
        },
        MaterialCommand::RemoveMaterialInstance { id: instance.id },
        MaterialCommand::ReplaceMaterialInstance {
            id: instance.id,
            instance: instance.clone(),
        },
        MaterialCommand::SetMaterialInstanceParameter {
            instance: instance.id,
            parameter: MaterialParameterId::from_u128(0xab402),
            value: None,
        },
        MaterialCommand::SetMaterialInstanceRenderState {
            instance: instance.id,
            render_state: MaterialRenderState::additive_sprite(),
        },
        MaterialCommand::AssignRendererMaterial {
            emitter: EmitterId::from_u128(0xab403),
            renderer: RendererId::from_u128(0xab404),
            material: instance.id,
        },
    ];
    for command in commands {
        let mut document = before.clone();
        let mut history = MaterialCommandHistory::default();
        let mut renamed = document.programs[0].clone();
        renamed.name = "Must roll back".into();
        let result = history.execute(
            &mut document,
            MaterialTransaction::new(
                "Mixed edit",
                vec![
                    MaterialCommand::ReplaceMaterialProgram {
                        id: program,
                        program: renamed,
                    },
                    command,
                ],
            ),
        );
        assert!(
            matches!(result, Err(MaterialCommandError::EffectContextRequired)),
            "{result:?}"
        );
        assert_eq!(document, before);
        assert!(!history.can_undo());
        assert!(!history.can_redo());
    }
    assert!(matches!(
        plan_legacy_sprite_material_migration(&before),
        Err(LegacyMaterialMigrationError::Command(
            MaterialCommandError::EffectContextRequired
        ))
    ));
    assert_eq!(
        MaterialInspector::inspect(&before, MaterialInspectionTarget::Instance(instance.id)),
        Err(MaterialInspectionError::InstanceNotFound(instance.id)),
    );
    let response = MaterialApi::handle(
        &before,
        MaterialApiRequest::PlanEdit {
            command: MaterialToolCommand::BindMaterialParameter {
                instance: instance.id,
                parameter: MaterialParameterId::from_u128(0xab402),
                binding: MaterialParameterBinding::ProgramDefault,
            },
        },
    );
    let MaterialApiResponse::Error(error) = response else {
        panic!("{response:?}")
    };
    assert_eq!(error.code, MaterialApiErrorCode::InvalidRequest);
    assert!(error.message.contains("effect-instance context"));
}

#[test]
fn standalone_validation_still_rejects_duplicate_programs() {
    let mut document = document();
    document.programs.push(document.programs[0].clone());
    assert!(
        document
            .validation_report()
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == DiagnosticCode::DuplicateId)
    );
}

#[test]
fn standalone_snapshot_roundtrip_has_no_effect_and_preserves_source_format() {
    let document = document();
    let encoded = ron::to_string(&document).unwrap();
    assert!(!encoded.contains("effect:"));
    assert_eq!(
        ron::from_str::<MaterialAuthoringDocument>(&encoded).unwrap(),
        document
    );
    let source = ron::to_string(&document.programs[0]).unwrap();
    assert_eq!(
        MaterialProgram::from_ron(&source).unwrap(),
        document.programs[0].normalized()
    );

    let mut effect = EffectAsset::new("Existing workflow", 2.0);
    effect.emitters.push(Emitter::basic_sprite("Emitter", 2.0));
    let contextual = MaterialAuthoringDocument::new(effect.clone(), document.programs);
    let roundtrip =
        ron::from_str::<MaterialAuthoringDocument>(&ron::to_string(&contextual).unwrap()).unwrap();
    assert_eq!(roundtrip, contextual);
    assert_eq!(roundtrip.require_effect().unwrap(), &effect);
    roundtrip.validate().unwrap();

    // A pre-AB4 snapshot encoded an EffectAsset directly, not Some(EffectAsset).
    #[derive(serde::Serialize, serde::Deserialize)]
    struct LegacySnapshot {
        effect: EffectAsset,
        programs: Vec<MaterialProgram>,
    }
    let legacy = LegacySnapshot {
        effect,
        programs: contextual.programs.clone(),
    };
    assert_eq!(
        ron::from_str::<MaterialAuthoringDocument>(&ron::to_string(&legacy).unwrap()).unwrap(),
        contextual
    );
    let legacy_reader =
        ron::from_str::<LegacySnapshot>(&ron::to_string(&contextual).unwrap()).unwrap();
    assert_eq!(legacy_reader.effect, legacy.effect);
    assert_eq!(legacy_reader.programs, legacy.programs);
}

#[test]
fn standalone_function_extraction_compiles_with_the_document_library() {
    let mut document = document();
    let before = document.clone();
    let program = document.programs[0].id;
    let mut history = MaterialCommandHistory::default();
    let plan = MaterialToolPlanner::plan(
        &document,
        MaterialToolCommand::CreateMaterialGraphNode {
            program,
            kind: MaterialGraphCreateKind::Function(MaterialGraphFunction::Smoothstep),
            source: Some(document.programs[0].outputs.alpha),
            target: Some(MaterialConnectionTarget::ProgramOutput(
                MaterialOutputSocket::Alpha,
            )),
        },
    )
    .unwrap();
    history.execute(&mut document, plan.transaction).unwrap();
    let plan = MaterialToolPlanner::plan(
        &document,
        MaterialToolCommand::ExtractMaterialFunction {
            program,
            function: MaterialFunctionId::from_u128(0xab405),
            name: "Threshold".into(),
            expressions: vec![document.programs[0].outputs.alpha],
        },
    )
    .unwrap();
    history.execute(&mut document, plan.transaction).unwrap();
    document.validate().unwrap();
    let target = MaterialInspectionTarget::Program(program);
    assert!(
        MaterialInspector::inspect(&document, target)
            .unwrap()
            .is_valid()
    );
    let compilation = MaterialCompilationReporter::compile(&document, target).unwrap();
    assert!(compilation.is_valid(), "{:?}", compilation.diagnostics);
    assert!(compilation.ir.is_some());

    let mut missing_function = document.clone();
    missing_function.material_functions.clear();
    assert!(missing_function.validate().is_err());
    let failed = MaterialCompilationReporter::compile(&missing_function, target).unwrap();
    assert!(!failed.is_valid());
    assert!(failed.ir.is_none());

    history.undo(&mut document).unwrap().unwrap();
    assert!(document.material_functions.is_empty());
    history.undo(&mut document).unwrap().unwrap();
    assert_eq!(document, before);
}
