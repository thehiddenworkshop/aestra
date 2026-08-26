use aestra_core::{DiagnosticCode, EffectAsset, Emitter, EmitterId, RendererInstance, ScalarRange};

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
        .push(RendererInstance::sprite(aestra_core::BlendMode::Alpha, 0.2));
    effect.emitters.push(emitter);

    effect.validate().unwrap();
    assert_eq!(effect.emitters[0].renderers.len(), 2);
}
