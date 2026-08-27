use aestra_core::{
    AssetDefinition, DiagnosticCode, EffectAsset, EffectParameter, Emitter, EmitterId,
    MODULE_EMISSION, MaterialProperties, ParameterId, RendererInstance, ScalarRange, Value,
};

#[test]
fn semantic_ids_survive_round_trip() {
    let mut effect = EffectAsset::new("Round Trip", 1.5);
    effect.emitters.push(Emitter::basic_sprite("Emitter", 1.5));
    let effect_id = effect.id;
    let emitter_id = effect.emitters[0].id;

    let encoded = effect.to_pretty_ron().unwrap();
    let decoded = EffectAsset::from_ron(&encoded).unwrap();

    assert_eq!(decoded.id, effect_id);
    assert_eq!(decoded.emitters[0].id, emitter_id);
}

#[test]
fn duplicated_emitters_receive_fresh_owned_ids() {
    let original = Emitter::basic_sprite("Original", 1.0);
    let original_module_ids = original
        .modules
        .iter()
        .map(|item| item.id)
        .collect::<Vec<_>>();
    let original_renderer_ids = original
        .renderers
        .iter()
        .map(|item| item.id)
        .collect::<Vec<_>>();
    let mut duplicate = original.clone();

    duplicate.regenerate_ids();

    assert_ne!(duplicate.id, original.id);
    assert!(
        duplicate
            .modules
            .iter()
            .zip(original_module_ids)
            .all(|(module, original_id)| module.id != original_id)
    );
    assert!(
        duplicate
            .renderers
            .iter()
            .zip(original_renderer_ids)
            .all(|(renderer, original_id)| renderer.id != original_id)
    );
}

#[test]
fn validation_reports_multiple_semantic_failures() {
    let mut effect = EffectAsset::new("Invalid", 0.0);
    let mut emitter = Emitter::basic_sprite("Broken", 1.0);
    emitter.id = EmitterId::from_u128(0);
    emitter.max_particles = 0;
    emitter.renderers.clear();
    *emitter.lifetime_mut() = ScalarRange::new(2.0, 1.0);
    effect.emitters.push(emitter);

    let report = effect.validation_report();

    assert!(!report.is_valid());
    for code in [
        DiagnosticCode::InvalidDuration,
        DiagnosticCode::NilId,
        DiagnosticCode::InvalidTiming,
        DiagnosticCode::InvalidCapacity,
        DiagnosticCode::InvalidValue,
        DiagnosticCode::MissingRenderer,
    ] {
        assert!(
            report.diagnostics.iter().any(|item| item.code == code),
            "missing diagnostic {code:?}"
        );
    }
}

#[test]
fn one_emitter_supports_multiple_renderers() {
    let mut effect = EffectAsset::new("Multi Renderer", 1.0);
    let mut emitter = Emitter::basic_sprite("Emitter", 1.0);
    emitter
        .renderers
        .push(RendererInstance::sprite(effect.materials[0].id));
    effect.emitters.push(emitter);

    effect.validate().unwrap();
    assert_eq!(effect.emitters[0].renderers.len(), 2);
}

#[test]
fn texture_assets_are_stable_validated_renderer_references() {
    let mut effect = EffectAsset::new("Textured", 1.0);
    let texture = AssetDefinition::texture("Spark", "textures/spark.png");
    let texture_id = texture.id;
    let emitter = Emitter::basic_sprite("Emitter", 1.0);
    let MaterialProperties::Sprite {
        texture: material_texture,
        ..
    } = &mut effect.materials[0].properties;
    *material_texture = Some(texture_id);
    effect.assets.push(texture);
    effect.emitters.push(emitter);

    let encoded = effect.to_pretty_ron().unwrap();
    let decoded = EffectAsset::from_ron(&encoded).unwrap();
    assert_eq!(decoded.assets[0].id, texture_id);
    assert!(encoded.contains("textures/spark.png"));

    effect.assets.clear();
    let report = effect.validation_report();
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == DiagnosticCode::InvalidReference
            && diagnostic.path.ends_with("properties.texture")
    }));
}

#[test]
fn module_parameter_bindings_survive_round_trip() {
    let mut effect = EffectAsset::new("Bound", 1.0);
    let parameter = EffectParameter {
        id: ParameterId::new(),
        name: "Spawn Rate".into(),
        default: Value::Scalar(24.0),
        exposed: true,
    };
    let mut emitter = Emitter::basic_sprite("Emitter", 1.0);
    emitter
        .modules
        .iter_mut()
        .find(|module| module.module_type.0 == MODULE_EMISSION)
        .unwrap()
        .bindings
        .insert("spawn_rate".into(), parameter.id);
    effect.parameters.push(parameter);
    effect.emitters.push(emitter);

    let encoded = effect.to_pretty_ron().unwrap();
    let decoded = EffectAsset::from_ron(&encoded).unwrap();

    assert_eq!(decoded, effect);
    assert!(encoded.contains("bindings"));
}

#[test]
fn saving_replaces_an_existing_effect_without_leaving_temporary_files() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("effect.aestra.ron");
    let mut original = EffectAsset::new("Original", 1.0);
    original
        .emitters
        .push(Emitter::basic_sprite("Emitter", 1.0));
    original.save_ron(&path).unwrap();

    let mut replacement = original.clone();
    replacement.name = "Replacement".into();
    replacement.save_ron(&path).unwrap();

    assert_eq!(EffectAsset::load_ron(&path).unwrap(), replacement);
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[test]
fn binding_types_are_validated_in_the_semantic_model() {
    let mut effect = EffectAsset::new("Bad binding", 1.0);
    let parameter = EffectParameter {
        id: ParameterId::new(),
        name: "Wrong Type".into(),
        default: Value::Vec2([1.0, 2.0]),
        exposed: true,
    };
    let mut emitter = Emitter::basic_sprite("Emitter", 1.0);
    emitter
        .modules
        .iter_mut()
        .find(|module| module.module_type.0 == MODULE_EMISSION)
        .unwrap()
        .bindings
        .insert("spawn_rate".into(), parameter.id);
    effect.parameters.push(parameter);
    effect.emitters.push(emitter);

    let report = effect.validation_report();
    assert!(
        report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == DiagnosticCode::ParameterTypeMismatch)
    );
}
