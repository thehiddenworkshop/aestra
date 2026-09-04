use aestra_core::{
    MaterialPresetId,
    material::{
        MaterialPresetCategory, MaterialPresetDefault, MaterialPresetDescriptor,
        MaterialPresetSchemaVersion, MaterialStackModifierKind, MaterialStackProperty,
        MaterialValue,
    },
};

fn hologram_preset() -> MaterialPresetDescriptor {
    MaterialPresetDescriptor {
        schema_version: MaterialPresetSchemaVersion::CURRENT,
        id: MaterialPresetId::from_u128(0xA357_A101),
        display_name: " Hologram ".into(),
        description: " Shapes a scan band. ".into(),
        category: MaterialPresetCategory::Shaping,
        tags: vec!["Scan".into(), " hologram ".into(), "scan".into()],
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
fn material_preset_assets_round_trip_with_normalized_metadata() {
    let preset = hologram_preset();
    let encoded = preset.to_pretty_ron().unwrap();
    let decoded = MaterialPresetDescriptor::from_ron(&encoded).unwrap();

    assert_eq!(decoded.display_name, "Hologram");
    assert_eq!(decoded.description, "Shapes a scan band.");
    assert_eq!(decoded.tags, vec!["hologram", "scan"]);
    assert_eq!(decoded, preset.normalized());
}

#[test]
fn material_preset_assets_reject_invalid_recipe_defaults() {
    let mut preset = hologram_preset();
    preset.defaults[0].property = MaterialStackProperty::Speed;

    let error = preset.validate().unwrap_err().to_string();
    assert!(error.contains("Smoothstep does not expose Speed"));
}
