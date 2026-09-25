//! The M10 linked-plugin vertical slice: a real extension crate registers a stage, module, domain,
//! resource and renderer through the public SDK, and an authored effect using them validates, lowers to
//! Execution IR, runs through the reference backend, and survives the plugin being absent.

use aestra_compiler::{
    AestraExtension, EffectCompiler, ExtensionManifest, ExtensionRegistry, RegistryConflict,
    StageTypeDescriptor,
};
use aestra_core::{
    DiagnosticCode, EffectAsset, Emitter, ExtensionId, ModuleParameters, ModuleTypeId,
    RendererTypeId, StageKind, StageTypeId, Value,
};
use aestra_example_extension::{
    ExampleExtension, MODULE_VORTEX, PLUGIN_ID, RENDERER_DEBUG_POINTS, STAGE_FIELD_FORCES,
};
use aestra_runtime::{ExecutionOp, execute_reference};

fn plugin_registry() -> ExtensionRegistry {
    let mut registry = ExtensionRegistry::builtin();
    registry
        .install(&ExampleExtension)
        .expect("the example extension installs");
    registry
}

/// A sprite emitter with a "Field Forces" simulation stage hosting one Vortex module.
fn plugin_lab_effect(registry: &ExtensionRegistry) -> EffectAsset {
    let mut effect = EffectAsset::new("Plugin Lab", 2.0);
    let mut emitter = Emitter::basic_sprite("Swirl", 2.0);
    let mut vortex = registry
        .modules
        .instantiate(&ModuleTypeId::new(MODULE_VORTEX))
        .expect("a plugin module instantiates from its schema defaults");
    vortex.stage = StageKind::Simulation("Field Forces".into());
    emitter.modules.push(vortex);
    emitter
        .simulation_stage_types
        .insert("Field Forces".into(), StageTypeId::new(STAGE_FIELD_FORCES));
    effect.emitters.push(emitter);
    effect
}

fn vortex_mut(effect: &mut EffectAsset) -> &mut aestra_core::ModuleInstance {
    effect.emitters[0]
        .modules
        .iter_mut()
        .find(|module| module.module_type.0 == MODULE_VORTEX)
        .unwrap()
}

fn set_vortex_input(effect: &mut EffectAsset, name: &str, value: Value) {
    let ModuleParameters::Custom(values) = &mut vortex_mut(effect).parameters else {
        panic!("a plugin module carries a generic payload");
    };
    values.insert(name.into(), value);
}

fn codes(error: aestra_compiler::CompileError) -> Vec<DiagnosticCode> {
    error
        .report()
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code)
        .collect()
}

#[test]
fn the_extension_registers_every_descriptor_kind_under_its_namespace() {
    let registry = plugin_registry();
    assert_eq!(registry.installed().len(), 1);
    assert_eq!(registry.installed()[0].plugin, ExtensionId::new(PLUGIN_ID));
    assert!(
        registry
            .stages
            .get(&StageTypeId::new(STAGE_FIELD_FORCES))
            .is_some()
    );
    assert!(
        registry
            .modules
            .get(&ModuleTypeId::new(MODULE_VORTEX))
            .is_some()
    );
    assert!(
        registry
            .renderers
            .is_extension(&RendererTypeId::new(RENDERER_DEBUG_POINTS))
    );
    assert!(registry.validate().is_empty(), "descriptors are consistent");
    // The built-in registry stays plugin-free.
    assert!(
        ExtensionRegistry::builtin()
            .stages
            .get(&StageTypeId::new(STAGE_FIELD_FORCES))
            .is_none()
    );
}

#[test]
fn installing_the_same_extension_twice_is_rejected() {
    let mut registry = plugin_registry();
    assert_eq!(
        registry.install(&ExampleExtension),
        Err(RegistryConflict::DuplicateExtension(ExtensionId::new(
            PLUGIN_ID
        )))
    );
}

/// A misbehaving plugin that registers a stage outside its own namespace.
struct Squatter;

impl AestraExtension for Squatter {
    fn manifest(&self) -> ExtensionManifest {
        ExtensionManifest {
            plugin: ExtensionId::new("org.squatter"),
            display_name: "Squatter".into(),
            version: "0.0.0".into(),
        }
    }

    fn register(&self, registry: &mut ExtensionRegistry) -> Result<(), RegistryConflict> {
        registry.register_stage(StageTypeDescriptor {
            type_id: StageTypeId::new("aestra.stage.fluid"),
            display_name: "Fluid".into(),
            role: None,
            provides: Default::default(),
            backend: Default::default(),
        })
    }
}

#[test]
fn a_plugin_cannot_register_outside_its_namespace_and_leaves_the_registry_unchanged() {
    let mut registry = ExtensionRegistry::builtin();
    let error = registry.install(&Squatter).unwrap_err();
    assert_eq!(
        error,
        RegistryConflict::OutsideNamespace {
            plugin: ExtensionId::new("org.squatter"),
            id: "aestra.stage.fluid".into(),
        }
    );
    assert!(
        registry
            .stages
            .get(&StageTypeId::new("aestra.stage.fluid"))
            .is_none(),
        "a rejected install registers nothing"
    );
    assert!(registry.installed().is_empty());
}

#[test]
fn a_plugin_stage_lowers_to_multi_pass_execution_ir_and_runs_in_the_reference_backend() {
    let registry = plugin_registry();
    let effect = plugin_lab_effect(&registry);
    let compiled = EffectCompiler::with_extensions(registry)
        .compile(&effect)
        .expect("the plugin effect compiles");

    let emitter = &compiled.emitters[0];
    assert_eq!(emitter.extension_stages.len(), 1);
    let stage = &emitter.extension_stages[0];
    assert_eq!(stage.stage_type, StageTypeId::new(STAGE_FIELD_FORCES));
    assert_eq!(stage.name, "Field Forces");
    assert_eq!(stage.modules.len(), 1);
    assert_eq!(
        stage.modules[0].parameters.get("strength"),
        Some(&Value::Scalar(4.0)),
        "the module plan carries its resolved (default-filled) inputs"
    );
    stage.block.validate().unwrap();
    assert!(
        stage
            .block
            .ops
            .iter()
            .any(|op| matches!(op, ExecutionOp::Barrier)),
        "the stage lowers to more than one pass, separated by a barrier"
    );
    assert_eq!(
        execute_reference(&stage.block).steps,
        vec![
            "compute:field_forces/vortex",
            "barrier",
            "compute:field_forces/apply"
        ]
    );
    // The built-in lifecycle stages are untouched by the plugin stage.
    assert!(!emitter.execution.particle_update.is_empty());
}

#[test]
fn a_plugin_module_outside_a_stage_that_provides_its_capability_is_a_stage_mismatch() {
    let registry = plugin_registry();
    let mut effect = plugin_lab_effect(&registry);
    vortex_mut(&mut effect).stage = StageKind::ParticleUpdate;
    let error = EffectCompiler::with_extensions(registry)
        .compile(&effect)
        .unwrap_err();
    assert!(codes(error).contains(&DiagnosticCode::StageMismatch));
}

#[test]
fn plugin_module_inputs_are_validated_by_schema_and_by_the_lowerer() {
    let registry = plugin_registry();

    // Out of the declared bounds: the schema rejects it before lowering.
    let mut effect = plugin_lab_effect(&registry);
    set_vortex_input(&mut effect, "radius", Value::Scalar(-1.0));

    let error = EffectCompiler::with_extensions(registry.clone())
        .compile(&effect)
        .unwrap_err();
    assert!(codes(error).contains(&DiagnosticCode::InvalidValue));

    // Within the schema but semantically invalid: the plugin's lowerer rejects it.
    let mut effect = plugin_lab_effect(&registry);
    set_vortex_input(&mut effect, "axis", Value::Vec3([0.0, 0.0, 0.0]));

    let error = EffectCompiler::with_extensions(registry)
        .compile(&effect)
        .unwrap_err();
    assert!(codes(error).contains(&DiagnosticCode::LoweringFailed));
}

#[test]
fn a_missing_plugin_reports_diagnostics_but_preserves_the_authored_data() {
    let effect = plugin_lab_effect(&plugin_registry());
    let saved = effect
        .to_pretty_ron()
        .expect("serializes without plugin code");

    // Reopened where the plugin is not linked: the data loads intact…
    let reopened = EffectAsset::from_ron(&saved).expect("loads without plugin code");
    assert_eq!(reopened, effect, "stage type and module payload round-trip");

    // …and compiling it names what is missing instead of silently dropping it.
    let error = EffectCompiler::with_extensions(ExtensionRegistry::builtin())
        .compile(&reopened)
        .unwrap_err();
    let codes = codes(error);
    assert!(codes.contains(&DiagnosticCode::MissingExtension));

    // Saving again still carries the plugin's stage and module.
    assert_eq!(reopened.to_pretty_ron().unwrap(), saved);
}

#[test]
fn the_committed_plugin_lab_sample_compiles_with_the_plugin_and_names_it_without() {
    let source = include_str!("../../../sample-project/effects/plugin_lab.aestra.ron");
    let effect = EffectAsset::from_ron(source).expect("the sample loads without plugin code");
    let compiled = EffectCompiler::with_extensions(plugin_registry())
        .compile(&effect)
        .expect("the sample compiles with the plugin");
    let stage = &compiled.emitters[0].extension_stages[0];
    assert_eq!(stage.modules.len(), 2, "both labelled vortices lower");
    assert_eq!(stage.block.compute_pass_count(), 3);

    let error = EffectCompiler::with_extensions(ExtensionRegistry::builtin())
        .compile(&effect)
        .unwrap_err();
    assert!(codes(error).contains(&DiagnosticCode::MissingExtension));
}

#[test]
fn linking_makes_the_default_compiler_include_the_extension() {
    aestra_example_extension::link();
    aestra_example_extension::link(); // idempotent
    assert!(
        aestra_compiler::linked_extensions()
            .iter()
            .any(|manifest| manifest.plugin == ExtensionId::new(PLUGIN_ID))
    );
    let registry = ExtensionRegistry::linked();
    let effect = plugin_lab_effect(&registry);
    let compiled = EffectCompiler::default()
        .compile(&effect)
        .expect("the default compiler sees linked plugins");
    assert_eq!(compiled.emitters[0].extension_stages.len(), 1);
}
