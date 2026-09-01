use aestra_core::{
    EffectAsset, MaterialId, MaterialParameterId, MaterialProgramId,
    material::{
        MaterialInstance, MaterialParameterValue, MaterialProgram, MaterialProgramRef,
        MaterialRenderState, MaterialValue,
    },
};
use aestra_project::{
    ProjectAssetId, ProjectAssetIndex, ProjectMaterialDependencyDiagnosticCode,
    ProjectMaterialProgramStatus, ResolveMaterialProgramError,
};
use std::{collections::BTreeMap, fs};

fn write_program(path: &std::path::Path, id: MaterialProgramId, name: &str) -> MaterialProgram {
    let mut program = MaterialProgram::additive_sprite(name);
    program.id = id;
    program.save_ron(path).unwrap();
    program
}

#[test]
fn material_program_identity_survives_create_rename_and_move() {
    let temporary = tempfile::tempdir().unwrap();
    let nested = temporary.path().join("nested");
    fs::create_dir(&nested).unwrap();
    let mut index = ProjectAssetIndex::scan(temporary.path());
    let mut program = MaterialProgram::additive_sprite("Magic Flame");
    program.id = MaterialProgramId::from_u128(0xA001);

    let created = index.create_material_program_source(&program).unwrap();

    assert_eq!(
        created.path.file_name().unwrap(),
        "magic_flame.aestra.material.ron"
    );
    assert_eq!(
        created.reference,
        Some(MaterialProgramRef::Project(program.id))
    );
    assert_eq!(
        ProjectAssetId::from(created.reference.unwrap()),
        ProjectAssetId::MaterialProgram(program.id)
    );

    let renamed = index
        .rename_material_program_source(created.id, "Arcane Flame")
        .unwrap();
    assert_eq!(
        renamed.path.file_name().unwrap(),
        "arcane_flame.aestra.material.ron"
    );
    assert_eq!(renamed.reference, created.reference);
    assert_eq!(
        index
            .load_material_program(created.reference.unwrap())
            .unwrap()
            .name,
        "Arcane Flame"
    );

    let moved = index
        .move_material_program_source(renamed.id, &nested)
        .unwrap();
    assert_eq!(moved.path, nested.join("arcane_flame.aestra.material.ron"));
    assert_eq!(moved.reference, created.reference);
    assert_eq!(index.effects().len(), 0);
    assert_eq!(index.material_programs().len(), 1);
}

#[test]
fn duplicate_material_program_ids_are_visible_but_not_resolvable() {
    let temporary = tempfile::tempdir().unwrap();
    let id = MaterialProgramId::from_u128(0xA002);
    write_program(&temporary.path().join("one.aestra.material.ron"), id, "One");
    write_program(&temporary.path().join("two.aestra.material.ron"), id, "Two");

    let index = ProjectAssetIndex::scan(temporary.path());
    let reference = MaterialProgramRef::Project(id);

    assert_eq!(index.material_programs().len(), 2);
    assert!(index.material_programs().iter().all(|entry| matches!(
        entry.status,
        ProjectMaterialProgramStatus::DuplicateId { .. }
    )));
    assert!(matches!(
        index.resolve_material_program(reference),
        Err(ResolveMaterialProgramError::Duplicate { ref sources, .. }) if sources.len() == 2
    ));
}

#[test]
fn source_identity_changes_after_material_program_indexing_are_rejected() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("program.aestra.material.ron");
    let original = MaterialProgramId::from_u128(0xA003);
    let replacement = MaterialProgramId::from_u128(0xA004);
    write_program(&path, original, "Original");
    let index = ProjectAssetIndex::scan(temporary.path());

    write_program(&path, replacement, "Replacement");

    assert!(matches!(
        index.load_material_program(MaterialProgramRef::Project(original)),
        Err(ResolveMaterialProgramError::SourceChanged { .. })
    ));
}

#[test]
fn effect_projects_resolve_material_programs_and_validate_instances() {
    let temporary = tempfile::tempdir().unwrap();
    let id = MaterialProgramId::from_u128(0xA005);
    let program = write_program(
        &temporary.path().join("flame.aestra.material.ron"),
        id,
        "Flame",
    );
    let material = MaterialId::from_u128(0xB001);
    let mut effect = EffectAsset::new("Owner", 1.0);
    effect.material_instances.push(MaterialInstance {
        id: material,
        program: MaterialProgramRef::Project(id),
        values: BTreeMap::new(),
        render_state: MaterialRenderState::additive_sprite(),
    });

    let index = ProjectAssetIndex::scan(temporary.path());
    let resolved = index.resolve_effect_project(&effect).unwrap();

    assert_eq!(
        resolved.material_programs.get(&id),
        Some(&program.normalized())
    );

    effect.material_instances[0].values.insert(
        MaterialParameterId::from_u128(0xBAD),
        MaterialParameterValue::Constant(MaterialValue::Float(1.0)),
    );
    let report = index.resolve_effect_project(&effect).unwrap_err();
    assert!(report.material_diagnostics.iter().any(|diagnostic| {
        diagnostic.code == ProjectMaterialDependencyDiagnosticCode::InvalidInstance
            && diagnostic.owner == effect.id
            && diagnostic.material == material
    }));
}

#[test]
fn missing_material_programs_produce_typed_dependency_diagnostics() {
    let temporary = tempfile::tempdir().unwrap();
    let missing = MaterialProgramId::from_u128(0xA006);
    let mut effect = EffectAsset::new("Missing program", 1.0);
    effect.material_instances.push(MaterialInstance {
        id: MaterialId::from_u128(0xB002),
        program: MaterialProgramRef::Project(missing),
        values: BTreeMap::new(),
        render_state: MaterialRenderState::additive_sprite(),
    });

    let index = ProjectAssetIndex::scan(temporary.path());
    let report = index.resolve_effect_project(&effect).unwrap_err();

    assert_eq!(report.diagnostics.len(), 0);
    assert_eq!(report.material_diagnostics.len(), 1);
    assert_eq!(
        report.material_diagnostics[0].code,
        ProjectMaterialDependencyDiagnosticCode::Missing
    );
    assert_eq!(
        report.material_diagnostics[0].reference,
        MaterialProgramRef::Project(missing)
    );
}
