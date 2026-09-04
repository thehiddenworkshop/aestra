use aestra_core::{
    MaterialExpressionId, MaterialFunctionId, MaterialFunctionInputId, MaterialFunctionOutputId,
    material::{
        MaterialExpression, MaterialExpressionKind, MaterialFunction, MaterialFunctionInput,
        MaterialFunctionOutput, MaterialFunctionRef, MaterialSchemaVersion, MaterialValueType,
    },
};
use aestra_project::{
    ProjectAssetId, ProjectAssetIndex, ProjectMaterialFunctionOperationError,
    ProjectMaterialFunctionStatus, ResolveMaterialFunctionError,
};

fn identity_function(id: MaterialFunctionId) -> MaterialFunction {
    let input = MaterialFunctionInputId::from_u128(0xD002);
    let expression = MaterialExpressionId::from_u128(0xD003);
    MaterialFunction {
        id,
        schema_version: MaterialSchemaVersion::CURRENT,
        name: "Identity Float".into(),
        inputs: vec![MaterialFunctionInput {
            id: input,
            name: "Value".into(),
            value_type: MaterialValueType::Float,
        }],
        outputs: vec![MaterialFunctionOutput {
            id: MaterialFunctionOutputId::from_u128(0xD004),
            name: "Value".into(),
            value_type: MaterialValueType::Float,
            expression,
        }],
        expressions: vec![MaterialExpression {
            id: expression,
            kind: MaterialExpressionKind::FunctionInput(input),
        }],
    }
}

#[test]
fn project_index_resolves_typed_material_function_assets() {
    let temporary = tempfile::tempdir().unwrap();
    let function = identity_function(MaterialFunctionId::from_u128(0xD001));
    function
        .save_ron(
            temporary
                .path()
                .join("identity.aestra.material-function.ron"),
        )
        .unwrap();

    let index = ProjectAssetIndex::scan(temporary.path());
    let reference = MaterialFunctionRef::Project(function.id);

    assert_eq!(index.material_functions().len(), 1);
    assert_eq!(index.material_functions()[0].reference, Some(reference));
    assert_eq!(
        ProjectAssetId::from(reference),
        ProjectAssetId::MaterialFunction(function.id)
    );
    assert_eq!(index.load_material_function(reference).unwrap(), function);
    assert_eq!(
        index.load_material_functions().unwrap().get(&function.id),
        Some(&function)
    );
}

#[test]
fn duplicate_material_function_ids_are_visible_but_not_resolvable() {
    let temporary = tempfile::tempdir().unwrap();
    let function = identity_function(MaterialFunctionId::from_u128(0xD101));
    function
        .save_ron(temporary.path().join("one.aestra.material-function.ron"))
        .unwrap();
    function
        .save_ron(temporary.path().join("two.aestra.material-function.ron"))
        .unwrap();

    let index = ProjectAssetIndex::scan(temporary.path());
    let reference = MaterialFunctionRef::Project(function.id);

    assert!(index.material_functions().iter().all(|entry| matches!(
        entry.status,
        ProjectMaterialFunctionStatus::DuplicateId { .. }
    )));
    assert!(matches!(
        index.resolve_material_function(reference),
        Err(ResolveMaterialFunctionError::Duplicate { ref sources, .. }) if sources.len() == 2
    ));
}

#[test]
fn material_function_sources_can_be_created_and_safely_deleted() {
    let temporary = tempfile::tempdir().unwrap();
    let function = identity_function(MaterialFunctionId::from_u128(0xD201));
    let reference = MaterialFunctionRef::Project(function.id);
    let mut index = ProjectAssetIndex::scan(temporary.path());

    let entry = index.create_material_function_source(&function).unwrap();
    assert_eq!(entry.reference, Some(reference));
    assert!(
        entry
            .path
            .ends_with("identity_float.aestra.material-function.ron")
    );
    assert_eq!(index.load_material_function(reference).unwrap(), function);

    let mut changed = function.clone();
    changed.name = "Changed externally".into();
    assert!(matches!(
        index.delete_material_function_source(reference, &changed),
        Err(ProjectMaterialFunctionOperationError::SourceConflict { .. })
    ));
    index
        .delete_material_function_source(reference, &function)
        .unwrap();
    assert!(matches!(
        index.resolve_material_function(reference),
        Err(ResolveMaterialFunctionError::Missing { .. })
    ));
}
