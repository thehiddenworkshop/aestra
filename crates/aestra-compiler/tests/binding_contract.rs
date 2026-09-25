//! Host bindings HB1: registry-aware validation of binding declarations.

use aestra_compiler::{EffectCompiler, ExtensionRegistry};
use aestra_core::{
    AESTRA_FIELD_LINEAR_VELOCITY, BindingFieldId, BindingKindId, BindingUpdateMode, DiagnosticCode,
    EffectAsset, EffectBinding, Emitter, ExtensionId,
};

fn effect(bindings: Vec<EffectBinding>) -> EffectAsset {
    let mut effect = EffectAsset::new("Bound", 1.0);
    effect.emitters.push(Emitter::basic_sprite("Emitter", 1.0));
    effect.bindings = bindings;
    effect
}

fn diagnostics(effect: &EffectAsset) -> Vec<(DiagnosticCode, String, String)> {
    match EffectCompiler::with_extensions(ExtensionRegistry::builtin()).compile(effect) {
        Ok(_) => Vec::new(),
        Err(error) => error
            .report()
            .diagnostics
            .iter()
            .map(|d| (d.code, d.path.clone(), d.message.clone()))
            .collect(),
    }
}

#[test]
fn spatial_source_and_target_bindings_compile() {
    let mut target = EffectBinding::spatial("Target", BindingUpdateMode::Live);
    target
        .optional_fields
        .insert(BindingFieldId::new(AESTRA_FIELD_LINEAR_VELOCITY));
    let effect = effect(vec![
        EffectBinding::spatial("Source", BindingUpdateMode::SnapshotOnSpawn),
        target,
    ]);
    assert_eq!(diagnostics(&effect), Vec::new());
}

#[test]
fn an_unknown_core_kind_or_unsupplied_field_is_an_invalid_reference() {
    let mut unknown = EffectBinding::spatial("Target", BindingUpdateMode::Live);
    unknown.kind = BindingKindId::new("aestra.binding.nonexistent");
    let found = diagnostics(&effect(vec![unknown]));
    assert!(found.iter().any(|(code, path, _)| {
        *code == DiagnosticCode::InvalidReference && path == "effect.bindings[0].kind"
    }));

    let mut wrong_field = EffectBinding::spatial("Target", BindingUpdateMode::Live);
    wrong_field
        .required_fields
        .insert(BindingFieldId::new("aestra.field.temperature"));
    let found = diagnostics(&effect(vec![wrong_field]));
    assert!(found.iter().any(|(code, path, message)| {
        *code == DiagnosticCode::InvalidReference
            && path == "effect.bindings[0].fields.aestra.field.temperature"
            && message.contains("does not supply")
    }));
}

#[test]
fn a_plugin_kind_whose_extension_is_missing_is_reported_and_recorded() {
    let mut source = EffectBinding::spatial("Emitter Source", BindingUpdateMode::Live);
    source.kind = BindingKindId::new("org.example.fluid::binding/source");
    let mut asset = effect(vec![source]);
    let registry = ExtensionRegistry::builtin();
    asset.extensions = registry.derive_requirements(&asset);
    assert_eq!(
        asset.extensions.len(),
        1,
        "the plugin kind is a recorded requirement"
    );
    assert_eq!(
        asset.extensions[0].plugin,
        ExtensionId::new("org.example.fluid")
    );

    let found = diagnostics(&asset);
    assert!(found.iter().any(|(code, path, message)| {
        *code == DiagnosticCode::MissingExtension
            && path == "effect.bindings[0].kind"
            && message.contains("org.example.fluid")
    }));
    // The declaration survives a save/load round trip without the plugin.
    let reopened = EffectAsset::from_ron(&asset.to_pretty_ron().unwrap()).unwrap();
    assert_eq!(reopened.bindings, asset.bindings);
}
