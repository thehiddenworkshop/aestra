use aestra_core::{
    MaterialPresetId,
    material::{
        MaterialPresetCategory, MaterialPresetDefault, MaterialPresetDescriptor,
        MaterialPresetSchemaVersion, MaterialStackModifierKind, MaterialStackProperty,
        MaterialValue,
    },
};
use aestra_project::{
    ProjectAssetId, ProjectAssetIndex, ProjectMaterialPresetStatus, ResolveMaterialPresetError,
};

fn hologram_preset(id: MaterialPresetId) -> MaterialPresetDescriptor {
    MaterialPresetDescriptor {
        schema_version: MaterialPresetSchemaVersion::CURRENT,
        id,
        display_name: "Hologram".into(),
        description: "Shapes a source signal into a holographic scan band.".into(),
        category: MaterialPresetCategory::Shaping,
        tags: vec!["hologram".into(), "scan".into()],
        modifiers: vec![
            MaterialStackModifierKind::Remap,
            MaterialStackModifierKind::Smoothstep,
        ],
        defaults: vec![MaterialPresetDefault {
            step: 1,
            property: MaterialStackProperty::EdgeMinimum,
            value: MaterialValue::Float(0.42),
        }],
    }
}

#[test]
fn project_index_resolves_typed_material_preset_assets() {
    let temporary = tempfile::tempdir().unwrap();
    let preset = hologram_preset(MaterialPresetId::from_u128(0xA357_A201));
    preset
        .save_ron(temporary.path().join("hologram.aestra.material-preset.ron"))
        .unwrap();

    let index = ProjectAssetIndex::scan(temporary.path());

    assert_eq!(index.material_presets().len(), 1);
    assert_eq!(index.material_presets()[0].preset, Some(preset.id));
    assert_eq!(
        ProjectAssetId::from(preset.id),
        ProjectAssetId::MaterialPreset(preset.id)
    );
    assert_eq!(index.load_material_preset(preset.id).unwrap(), preset);
    assert_eq!(
        index.load_material_presets().unwrap().get(&preset.id),
        Some(&preset)
    );
}

#[test]
fn duplicate_material_preset_ids_are_visible_but_not_resolvable() {
    let temporary = tempfile::tempdir().unwrap();
    let preset = hologram_preset(MaterialPresetId::from_u128(0xA357_A202));
    preset
        .save_ron(temporary.path().join("one.aestra.material-preset.ron"))
        .unwrap();
    preset
        .save_ron(temporary.path().join("two.aestra.material-preset.ron"))
        .unwrap();

    let index = ProjectAssetIndex::scan(temporary.path());

    assert!(index.material_presets().iter().all(|entry| matches!(
        entry.status,
        ProjectMaterialPresetStatus::DuplicateId { .. }
    )));
    assert!(matches!(
        index.resolve_material_preset(preset.id),
        Err(ResolveMaterialPresetError::Duplicate { ref sources, .. }) if sources.len() == 2
    ));
}
