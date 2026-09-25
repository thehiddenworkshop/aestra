//! Extensible-stages M11: exchanging effects that use a plugin is safe. An effect records the plugins it
//! needs; opened where one is missing it still loads, keeps its data, can be edited and saved, and the
//! compiler says exactly what is unavailable; reinstalling the plugin restores compilation. Payload
//! schemas are versioned: older payloads migrate through the plugin, newer ones are preserved untouched.

use aestra_compiler::{EffectCompiler, ExtensionRegistry, RequirementStatus, SchemaStatus};
use aestra_core::{
    DiagnosticCode, EffectAsset, Emitter, ExtensionId, ExtensionRequirement, ModuleInstance,
    ModuleParameters, ModuleTypeId, StageKind, StageTypeId, Value,
};
use aestra_example_extension::{
    ExampleExtension, MODULE_VORTEX, PLUGIN_ID, STAGE_FIELD_FORCES, VORTEX_SCHEMA_VERSION,
};

fn plugin_registry() -> ExtensionRegistry {
    let mut registry = ExtensionRegistry::builtin();
    registry.install(&ExampleExtension).unwrap();
    registry
}

/// A sprite emitter whose "Field Forces" stage hosts one Vortex, with requirements recorded as the
/// editor records them on save.
fn plugin_effect(registry: &ExtensionRegistry) -> EffectAsset {
    let mut effect = EffectAsset::new("Exchange", 2.0);
    let mut emitter = Emitter::basic_sprite("Swirl", 2.0);
    let mut vortex = registry
        .modules
        .instantiate(&ModuleTypeId::new(MODULE_VORTEX))
        .unwrap();
    vortex.stage = StageKind::Simulation("Field Forces".into());
    emitter.modules.push(vortex);
    emitter
        .simulation_stage_types
        .insert("Field Forces".into(), StageTypeId::new(STAGE_FIELD_FORCES));
    effect.emitters.push(emitter);
    effect.extensions = registry.derive_requirements(&effect);
    effect
}

fn vortex(effect: &mut EffectAsset) -> &mut ModuleInstance {
    effect.emitters[0]
        .modules
        .iter_mut()
        .find(|module| module.module_type.0 == MODULE_VORTEX)
        .unwrap()
}

fn payload(module: &mut ModuleInstance) -> &mut std::collections::BTreeMap<String, Value> {
    let ModuleParameters::Custom(values) = &mut module.parameters else {
        panic!("plugin modules carry a generic payload");
    };
    values
}

fn diagnostics(
    registry: ExtensionRegistry,
    effect: &EffectAsset,
) -> Vec<(DiagnosticCode, String, String)> {
    EffectCompiler::with_extensions(registry)
        .compile(effect)
        .unwrap_err()
        .report()
        .diagnostics
        .iter()
        .map(|diagnostic| {
            (
                diagnostic.code,
                diagnostic.path.clone(),
                diagnostic.message.clone(),
            )
        })
        .collect()
}

#[test]
fn saving_records_the_installed_plugin_as_a_version_requirement() {
    let effect = plugin_effect(&plugin_registry());
    assert_eq!(
        effect.extensions,
        vec![ExtensionRequirement::new(
            PLUGIN_ID,
            format!("^{}", env!("CARGO_PKG_VERSION"))
        )]
    );
    let saved = effect.to_pretty_ron().unwrap();
    assert!(
        saved.contains("extensions"),
        "requirements are written to the file"
    );
    // Built-in-only effects record nothing.
    let plain = EffectAsset::new("Plain", 1.0);
    assert!(plugin_registry().derive_requirements(&plain).is_empty());
}

#[test]
fn removing_and_reinstalling_the_plugin_meets_the_m11_acceptance() {
    // Created and saved with the plugin installed.
    let saved = plugin_effect(&plugin_registry()).to_pretty_ron().unwrap();

    // Reopened in a build without the plugin: the effect still loads, with its unknown data intact.
    let without = ExtensionRegistry::builtin();
    let mut reopened = EffectAsset::from_ron(&saved).expect("loads without the plugin");
    let original_vortex = vortex(&mut reopened).clone();

    // The compiler explains exactly what is unavailable — the module and the stage, naming the plugin
    // and the recorded requirement.
    let found = diagnostics(without.clone(), &reopened);
    let missing: Vec<_> = found
        .iter()
        .filter(|(code, ..)| *code == DiagnosticCode::MissingExtension)
        .collect();
    assert_eq!(
        missing.len(),
        2,
        "one for the module, one for its stage: {found:?}"
    );
    assert!(
        missing
            .iter()
            .any(|(_, path, message)| path.ends_with(".module_type")
                && message.contains(MODULE_VORTEX)
                && message.contains(&format!("{PLUGIN_ID} ^")))
    );
    assert!(
        missing
            .iter()
            .any(|(_, path, message)| path.ends_with(".stage")
                && message.contains(STAGE_FIELD_FORCES))
    );

    // Unrelated fields can be edited and saved; saving where the plugin is missing keeps its
    // requirement and payload verbatim.
    reopened.name = "Exchange (edited)".into();
    reopened.emitters[0].max_particles = 64;
    reopened.extensions = without.derive_requirements(&reopened);
    let resaved = reopened.to_pretty_ron().expect("saves without the plugin");
    let mut reloaded = EffectAsset::from_ron(&resaved).unwrap();
    assert_eq!(reloaded.name, "Exchange (edited)");
    assert_eq!(*vortex(&mut reloaded), original_vortex);
    assert_eq!(
        reloaded.extensions,
        EffectAsset::from_ron(&saved).unwrap().extensions
    );

    // Reinstalling the plugin restores compilation.
    let compiled = EffectCompiler::with_extensions(plugin_registry())
        .compile(&reloaded)
        .expect("compiles again once the plugin is back");
    assert_eq!(compiled.emitters[0].extension_stages.len(), 1);
}

#[test]
fn an_installed_plugin_that_does_not_satisfy_the_requirement_is_incompatible() {
    let registry = plugin_registry();
    let mut effect = plugin_effect(&registry);
    effect.extensions = vec![ExtensionRequirement::new(PLUGIN_ID, "^1.4")];
    assert!(matches!(
        registry.requirement_status(&effect.extensions[0]),
        RequirementStatus::Incompatible(_)
    ));
    let found = diagnostics(registry.clone(), &effect);
    assert!(found.iter().any(|(code, path, message)| {
        *code == DiagnosticCode::IncompatibleExtension
            && path == "effect.extensions[0]"
            && message.contains("^1.4")
    }));
    // Saving with the incompatible plugin keeps the stricter requirement rather than hiding it.
    assert_eq!(registry.derive_requirements(&effect), effect.extensions);

    effect.extensions = vec![ExtensionRequirement::new(PLUGIN_ID, "not a version")];
    let found = diagnostics(registry, &effect);
    assert!(found.iter().any(|(code, path, _)| {
        *code == DiagnosticCode::InvalidValue && path == "effect.extensions[0].version"
    }));
}

#[test]
fn an_older_payload_migrates_through_the_plugin() {
    let registry = plugin_registry();
    let mut effect = plugin_effect(&registry);
    // A v1 Vortex: `strength` was called `speed`.
    let module = vortex(&mut effect);
    module.schema_version = None;
    let values = payload(module);
    values.remove("strength");
    values.insert("speed".into(), Value::Scalar(9.0));
    assert_eq!(
        registry.module_schema_status(vortex(&mut effect)),
        Some(SchemaStatus::Older {
            stored: 1,
            current: VORTEX_SCHEMA_VERSION
        })
    );

    // The compiler migrates a copy, so the caller's document is untouched…
    let compiled = EffectCompiler::with_extensions(registry.clone())
        .compile(&effect)
        .expect("an older payload compiles through its migration");
    assert_eq!(
        compiled.emitters[0].extension_stages[0].modules[0]
            .parameters
            .get("strength"),
        Some(&Value::Scalar(9.0))
    );
    assert!(payload(vortex(&mut effect)).contains_key("speed"));

    // …and the editor upgrades the document itself when it opens it.
    let report = registry.migrate_effect(&mut effect);
    assert_eq!(report.upgraded.len(), 1);
    assert!(report.failed.is_empty());
    let module = vortex(&mut effect);
    assert_eq!(module.schema_version, Some(VORTEX_SCHEMA_VERSION));
    assert_eq!(payload(module).get("strength"), Some(&Value::Scalar(9.0)));
    assert!(!payload(module).contains_key("speed"));
}

#[test]
fn a_newer_payload_is_preserved_untouched_and_not_compiled() {
    let registry = plugin_registry();
    let mut effect = plugin_effect(&registry);
    let module = vortex(&mut effect);
    module.schema_version = Some(VORTEX_SCHEMA_VERSION + 1);
    payload(module).insert("turbulence".into(), Value::Scalar(0.5));
    let authored = module.clone();

    let report = registry.migrate_effect(&mut effect);
    assert!(report.upgraded.is_empty() && report.failed.is_empty());
    assert_eq!(*vortex(&mut effect), authored, "never silently rewritten");

    let found = diagnostics(registry, &effect);
    assert!(found.iter().any(|(code, _, message)| {
        *code == DiagnosticCode::IncompatibleExtension
            && message.contains(&format!("schema v{}", VORTEX_SCHEMA_VERSION + 1))
    }));
}

#[test]
fn a_payload_with_no_migration_path_is_reported_and_left_as_authored() {
    let registry = plugin_registry();
    let mut effect = plugin_effect(&registry);
    // Schema v0 predates every migration the plugin ships.
    vortex(&mut effect).schema_version = Some(0);
    let authored = vortex(&mut effect).clone();
    let report = registry.migrate_effect(&mut effect);
    assert_eq!(report.failed.len(), 1);
    assert_eq!(*vortex(&mut effect), authored);
    let found = diagnostics(registry, &effect);
    assert!(found.iter().any(|(code, _, message)| {
        *code == DiagnosticCode::IncompatibleExtension && message.contains("no Vortex migration")
    }));
}

#[test]
fn the_provider_of_a_plugin_type_is_its_installed_manifest() {
    let registry = plugin_registry();
    let manifest = registry.provider_of(MODULE_VORTEX).unwrap();
    assert_eq!(manifest.plugin, ExtensionId::new(PLUGIN_ID));
    assert!(registry.provider_of("aestra.module.motion").is_none());
    assert!(
        ExtensionRegistry::builtin()
            .provider_of(MODULE_VORTEX)
            .is_none()
    );
}
