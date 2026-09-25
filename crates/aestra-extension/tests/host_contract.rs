//! Extensible-stages M12 acceptance: an externally installed example extension is discovered,
//! version checked, registered, disabled, and diagnosed without modifying Aestra source.

use aestra_compiler::{EffectCompiler, ExtensionRegistry};
use aestra_core::{
    DiagnosticCode, EffectAsset, Emitter, ExtensionId, ModuleParameters, ModuleTypeId, StageKind,
    StageTypeId, Value,
};
use aestra_extension::host::{PackageStatus, load_packages};
use aestra_runtime::execute_reference;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const WIND: &str = "org.example.aestra-wind";
const GUST: &str = "org.example.aestra-wind::module/gust";
const WIND_STAGE: &str = "org.example.aestra-wind::stage/wind";

/// The committed sample project's extensions directory — the "externally installed" location.
fn sample_extensions() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../sample-project/extensions")
}

/// Built-ins plus the linked example extension (the wind package depends on it).
fn base() -> ExtensionRegistry {
    let mut registry = ExtensionRegistry::builtin();
    registry
        .install(&aestra_example_extension::ExampleExtension)
        .unwrap();
    registry
}

fn registry_with(dirs: &[PathBuf], disabled: &BTreeSet<ExtensionId>) -> ExtensionRegistry {
    let loaded = load_packages(dirs, disabled, &base());
    let mut registry = base();
    for extension in &loaded.extensions {
        registry.install(extension.as_ref()).unwrap();
    }
    registry
}

/// Writes a package with the given manifest and content into `dir/name`.
fn write_package(dir: &Path, name: &str, manifest: &str, content: &str) {
    let root = dir.join(name);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("extension.ron"), manifest).unwrap();
    std::fs::write(root.join("content.ron"), content).unwrap();
}

fn manifest(id: &str, api: &str, dependencies: &str) -> String {
    format!(
        r#"(id: "{id}", name: "{id}", version: "1.0.0", aestra_api: "{api}", dependencies: {{ {dependencies} }})"#
    )
}

fn status_of(dirs: &[PathBuf], disabled: &BTreeSet<ExtensionId>, id: &str) -> PackageStatus {
    load_packages(dirs, disabled, &base())
        .report
        .status_of(&ExtensionId::new(id))
        .cloned()
        .expect("package discovered")
}

/// An effect with a Gust in a Wind stage and another in the linked Field Forces stage.
fn wind_effect(registry: &ExtensionRegistry) -> EffectAsset {
    let mut effect = EffectAsset::new("Windy", 2.0);
    let mut emitter = Emitter::basic_sprite("Leaves", 2.0);
    for stage in ["Wind", "Field Forces"] {
        let mut gust = registry
            .modules
            .instantiate(&ModuleTypeId::new(GUST))
            .expect("a packaged module instantiates from its schema");
        gust.stage = StageKind::Simulation(stage.into());
        emitter.modules.push(gust);
    }
    emitter
        .simulation_stage_types
        .insert("Wind".into(), StageTypeId::new(WIND_STAGE));
    emitter.simulation_stage_types.insert(
        "Field Forces".into(),
        StageTypeId::new(aestra_example_extension::STAGE_FIELD_FORCES),
    );
    effect.emitters.push(emitter);
    effect.extensions = registry.derive_requirements(&effect);
    effect
}

#[test]
fn the_sample_package_is_discovered_version_checked_and_registered() {
    let dirs = [sample_extensions()];
    let loaded = load_packages(&dirs, &BTreeSet::new(), &base());
    let report = &loaded.report.packages;
    assert_eq!(report.len(), 1, "{report:?}");
    assert_eq!(report[0].status, PackageStatus::Registered);
    assert_eq!(report[0].name, "Aestra Wind");
    assert_eq!(report[0].version, "0.2.0");
    assert!(report[0].permissions.is_empty());

    let registry = registry_with(&dirs, &BTreeSet::new());
    assert!(registry.modules.get(&ModuleTypeId::new(GUST)).is_some());
    assert!(registry.stages.get(&StageTypeId::new(WIND_STAGE)).is_some());
    assert_eq!(
        registry.provider_of(GUST).unwrap().display_name,
        "Aestra Wind"
    );
}

#[test]
fn a_packaged_stage_and_module_compile_to_execution_ir_like_linked_ones() {
    let registry = registry_with(&[sample_extensions()], &BTreeSet::new());
    let effect = wind_effect(&registry);
    let compiled = EffectCompiler::with_extensions(registry)
        .compile(&effect)
        .expect("the packaged extension compiles");
    let stages = &compiled.emitters[0].extension_stages;
    assert_eq!(stages.len(), 2);
    let wind = stages.iter().find(|stage| stage.name == "Wind").unwrap();
    assert_eq!(
        execute_reference(&wind.block).steps,
        vec!["compute:Wind/gust", "barrier", "compute:Wind/advect"]
    );
    assert_eq!(
        wind.modules[0].parameters.get("strength"),
        Some(&Value::Scalar(3.0))
    );
    // The packaged Gust also runs in the linked extension's Field Forces stage (cross-extension).
    let field = stages
        .iter()
        .find(|stage| stage.name == "Field Forces")
        .unwrap();
    assert_eq!(
        execute_reference(&field.block).steps,
        vec![
            "compute:field_forces/gust",
            "barrier",
            "compute:field_forces/apply"
        ]
    );
}

#[test]
fn a_disabled_package_is_reported_and_its_effects_name_it_as_missing() {
    let dirs = [sample_extensions()];
    let disabled: BTreeSet<_> = [ExtensionId::new(WIND)].into();
    assert_eq!(status_of(&dirs, &disabled, WIND), PackageStatus::Disabled);
    let enabled = registry_with(&dirs, &BTreeSet::new());
    let effect = wind_effect(&enabled);

    let without = registry_with(&dirs, &disabled);
    assert!(without.modules.get(&ModuleTypeId::new(GUST)).is_none());
    let error = EffectCompiler::with_extensions(without)
        .compile(&effect)
        .unwrap_err();
    assert!(
        error
            .report()
            .diagnostics
            .iter()
            .any(
                |diagnostic| diagnostic.code == DiagnosticCode::MissingExtension
                    && diagnostic.message.contains(WIND)
            )
    );
}

#[test]
fn a_packaged_payload_migrates_through_declared_steps() {
    let registry = registry_with(&[sample_extensions()], &BTreeSet::new());
    let mut effect = wind_effect(&registry);
    let gust = effect.emitters[0].modules.last_mut().unwrap();
    gust.schema_version = None;
    let ModuleParameters::Custom(values) = &mut gust.parameters else {
        unreachable!()
    };
    values.remove("strength");
    values.insert("power".into(), Value::Scalar(7.0));
    let report = registry.migrate_effect(&mut effect);
    assert_eq!(report.upgraded.len(), 1, "{report:?}");
    let ModuleParameters::Custom(values) = &effect.emitters[0].modules.last().unwrap().parameters
    else {
        unreachable!()
    };
    assert_eq!(values.get("strength"), Some(&Value::Scalar(7.0)));
    assert!(!values.contains_key("power"));
}

#[test]
fn version_checks_and_dependency_problems_are_diagnosed() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().to_path_buf();
    let dirs = [dir.clone()];
    let empty = "()";
    write_package(&dir, "future", &manifest("org.x.future", "^9", ""), empty);
    write_package(
        &dir,
        "orphan",
        &manifest("org.x.orphan", "^0.1", r#""org.x.absent": "^1""#),
        empty,
    );
    write_package(
        &dir,
        "picky",
        &manifest("org.x.picky", "^0.1", r#""org.example.aestra": "^2""#),
        empty,
    );
    write_package(&dir, "base", &manifest("org.x.base", "^0.1", ""), empty);
    write_package(
        &dir,
        "dependent",
        &manifest("org.x.dependent", "^0.1", r#""org.x.base": "^1""#),
        empty,
    );
    write_package(
        &dir,
        "cycle-a",
        &manifest("org.x.a", "^0.1", r#""org.x.b": "^1""#),
        empty,
    );
    write_package(
        &dir,
        "cycle-b",
        &manifest("org.x.b", "^0.1", r#""org.x.a": "^1""#),
        empty,
    );
    write_package(&dir, "broken", "this is not ron", empty);

    let none = BTreeSet::new();
    assert!(matches!(
        status_of(&dirs, &none, "org.x.future"),
        PackageStatus::IncompatibleApi { .. }
    ));
    assert!(matches!(
        status_of(&dirs, &none, "org.x.orphan"),
        PackageStatus::MissingDependency { .. }
    ));
    assert!(matches!(
        status_of(&dirs, &none, "org.x.picky"),
        PackageStatus::IncompatibleDependency { .. }
    ));
    assert_eq!(
        status_of(&dirs, &none, "org.x.dependent"),
        PackageStatus::Registered
    );
    assert_eq!(
        status_of(&dirs, &none, "org.x.a"),
        PackageStatus::DependencyCycle
    );
    // Disabling a dependency makes its dependents unavailable, not missing.
    let disabled: BTreeSet<_> = [ExtensionId::new("org.x.base")].into();
    assert_eq!(
        status_of(&dirs, &disabled, "org.x.dependent"),
        PackageStatus::DependencyUnavailable {
            id: "org.x.base".into()
        }
    );
    let report = load_packages(&dirs, &none, &base()).report;
    assert!(
        report
            .problems()
            .any(|package| matches!(package.status, PackageStatus::InvalidManifest(_)))
    );
    // Every problem has a human-readable explanation.
    assert!(
        report
            .problems()
            .all(|package| !package.status.describe().is_empty())
    );
}

#[test]
fn registration_rejects_namespace_violations_duplicates_and_linked_ids() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().to_path_buf();
    let dirs = [dir.clone()];
    write_package(
        &dir,
        "squatter",
        &manifest("org.x.squatter", "^0.1", ""),
        r#"(capabilities: ["aestra.capability.squatted"])"#,
    );
    write_package(
        &dir,
        "linked",
        &manifest("org.example.aestra", "^0.1", ""),
        "()",
    );
    write_package(&dir, "one", &manifest("org.x.twin", "^0.1", ""), "()");
    write_package(&dir, "two", &manifest("org.x.twin", "^0.1", ""), "()");
    write_package(
        &dir,
        "needy",
        &manifest("org.x.needy", "^0.1", ""),
        r#"(modules: [(id: "org.x.needy::module/m", name: "M", requires: AnyOf(["org.x.nowhere::capability/c"]), entry_point: "m")])"#,
    );

    let none = BTreeSet::new();
    assert!(matches!(
        status_of(&dirs, &none, "org.x.squatter"),
        PackageStatus::RegistryConflict(message) if message.contains("namespace")
    ));
    assert_eq!(
        status_of(&dirs, &none, "org.example.aestra"),
        PackageStatus::AlreadyLinked
    );
    let report = load_packages(&dirs, &none, &base()).report;
    let twins: Vec<_> = report
        .packages
        .iter()
        .filter(|package| package.id == Some(ExtensionId::new("org.x.twin")))
        .map(|package| package.status.clone())
        .collect();
    assert_eq!(twins[0], PackageStatus::Registered);
    assert!(matches!(twins[1], PackageStatus::DuplicateId { .. }));
    assert!(matches!(
        status_of(&dirs, &none, "org.x.needy"),
        PackageStatus::RegistryConflict(message) if message.contains("org.x.nowhere::capability/c")
    ));
}

#[test]
fn invalid_content_is_rejected_before_registration() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().to_path_buf();
    write_package(
        &dir,
        "bad-default",
        &manifest("org.x.bad", "^0.1", ""),
        r#"(modules: [(id: "org.x.bad::module/m", name: "M", requires: Unconstrained, entry_point: "m",
            inputs: [(name: "k", label: "K", value_type: Scalar, default: Scalar(-1.0),
                      control: Number(step: 1.0, min: Some(0.0), max: None))])])"#,
    );
    assert!(matches!(
        status_of(&[dir], &BTreeSet::new(), "org.x.bad"),
        PackageStatus::InvalidContent(message) if message.contains("default")
    ));
}

#[test]
fn an_effect_finds_its_projects_extensions_directory() {
    let effect = sample_extensions().join("../effects/plugin_lab.aestra.ron");
    let dirs = aestra_extension::host::project_extension_dirs(&effect);
    assert_eq!(dirs.len(), 1);
    assert!(dirs[0].ends_with("extensions"));
    assert_eq!(
        aestra_extension::host::discover(&dirs).len(),
        1,
        "the sample project's wind package"
    );
}
