use aestra_core::{
    MaterialPresetId,
    material::{
        MaterialPresetCategory, MaterialPresetDefault, MaterialPresetDescriptor,
        MaterialPresetGraphNodeKind, MaterialPresetRecipe, MaterialPresetSchemaVersion,
        MaterialPresetValueRef, MaterialStackModifierKind, MaterialStackProperty, MaterialValue,
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
        recipe: MaterialPresetRecipe::Stack {
            modifiers: vec![
                MaterialStackModifierKind::Remap,
                MaterialStackModifierKind::Smoothstep,
            ],
            defaults: vec![MaterialPresetDefault {
                step: 1,
                property: MaterialStackProperty::EdgeMinimum,
                value: MaterialValue::Float(0.42),
            }],
        },
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
    let MaterialPresetRecipe::Stack { defaults, .. } = &mut preset.recipe else {
        unreachable!()
    };
    defaults[0].property = MaterialStackProperty::Speed;

    let error = preset.validate().unwrap_err().to_string();
    assert!(error.contains("Smoothstep does not expose Speed"));
}

#[test]
fn graph_material_preset_asset_round_trips_with_named_portable_nodes() {
    let preset = MaterialPresetDescriptor::from_ron(include_str!(
        "../../../assets/materials/hologram.aestra.material-preset.ron"
    ))
    .unwrap();
    let MaterialPresetRecipe::Graph(recipe) = &preset.recipe else {
        panic!("Hologram must exercise the graph recipe format")
    };

    assert_eq!(recipe.nodes.len(), 12);
    assert!(recipe.nodes.iter().any(|node| matches!(
        node.kind,
        MaterialPresetGraphNodeKind::Function(
            aestra_core::material::MaterialGraphFunction::Fresnel
        )
    )));
    assert_eq!(
        MaterialPresetDescriptor::from_ron(&preset.to_pretty_ron().unwrap()).unwrap(),
        preset
    );
}

#[test]
fn graph_material_presets_reject_cycles_and_forward_references() {
    let mut preset = MaterialPresetDescriptor::from_ron(include_str!(
        "../../../assets/materials/hologram.aestra.material-preset.ron"
    ))
    .unwrap();
    let MaterialPresetRecipe::Graph(recipe) = &mut preset.recipe else {
        unreachable!()
    };
    recipe.nodes[3].inputs.insert(
        "Normal".into(),
        MaterialPresetValueRef::Node("fresnel".into()),
    );

    let error = preset.validate().unwrap_err().to_string();
    assert!(error.contains("before it is declared"));
}
