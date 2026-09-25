//! Extensible-stages M13, without a GPU: the fluid registers through the public SDK only, declares
//! itself GPU-only, lowers one authored stage into a checked multi-pass solver block, packs host-bound
//! inputs for the GPU, survives the artifact round trip, and is diagnosed — not dropped — when absent.

use aestra_compiler::{EffectCompiler, ExtensionRegistry};
use aestra_core::{
    AESTRA_FIELD_LINEAR_VELOCITY, AESTRA_FIELD_POSITION, BindingUpdateMode, DiagnosticCode,
    EffectAsset, EffectBinding, HostFieldRef, ModuleParameters, PropertySource, StageTypeId, Value,
};
use aestra_fluid::{
    FluidExtension, MODULE_DENSITY_SOURCE, MODULE_GRID, MODULE_VORTICITY, PROGRAM_SOLVER,
    RESOURCE_VELOCITY, STAGE_FLUID_SOLVER, smoke_effect,
};
use aestra_gpu::check_program_block;
use aestra_runtime::{
    AESTRA_RESOURCE_FRAME, AESTRA_RESOURCE_HOST_BINDINGS, CompiledExtensionStage, ExecutionOp,
    ResourceAccessMode, SimulationSeekMode,
};

fn fluid_registry() -> ExtensionRegistry {
    let mut registry = ExtensionRegistry::builtin();
    registry
        .install(&FluidExtension)
        .expect("the fluid extension installs");
    registry
}

fn module_mut<'a>(
    effect: &'a mut EffectAsset,
    type_id: &str,
) -> &'a mut aestra_core::ModuleInstance {
    effect.emitters[0]
        .modules
        .iter_mut()
        .find(|module| module.module_type.0 == type_id)
        .unwrap()
}

fn set_input(effect: &mut EffectAsset, type_id: &str, name: &str, value: Value) {
    let ModuleParameters::Custom(values) = &mut module_mut(effect, type_id).parameters else {
        panic!("a plugin module carries a generic payload");
    };
    values.insert(name.into(), value);
}

fn compile_stage(registry: &ExtensionRegistry, effect: &EffectAsset) -> CompiledExtensionStage {
    let compiled = EffectCompiler::with_extensions(registry.clone())
        .compile(effect)
        .expect("the fluid effect compiles");
    compiled.emitters[0].extension_stages[0].clone()
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
fn the_solver_stage_is_registered_gpu_only_with_its_program() {
    let registry = fluid_registry();
    let stage = registry
        .stages
        .get(&StageTypeId::new(STAGE_FLUID_SOLVER))
        .unwrap();
    assert!(
        !stage.backend.has_cpu_reference(),
        "cpu_reference = Unavailable, declared truthfully"
    );
    let program = registry
        .programs
        .get(&aestra_core::ComputeProgramId::new(PROGRAM_SOLVER))
        .unwrap();
    assert_eq!(program.entry_points.len(), 8);
    // The whole program — solver passes plus the host-binding accessors — validates.
    aestra_gpu::program_interface(program).expect("the solver program validates");
}

#[test]
fn one_authored_stage_lowers_to_a_checked_multi_pass_solver() {
    let registry = fluid_registry();
    let effect = smoke_effect(&registry);
    let compiled = EffectCompiler::with_extensions(registry.clone())
        .compile(&effect)
        .unwrap();
    assert_eq!(
        compiled.seek_mode,
        SimulationSeekMode::RestartReplay,
        "a fluid is history-dependent"
    );
    let stage = &compiled.emitters[0].extension_stages[0];
    assert!(
        !stage.cpu_reference,
        "the compiled stage says it has no CPU path"
    );
    assert_eq!(stage.modules.len(), 4);
    stage.block.validate().unwrap();
    // Every op's declared accesses match what its WGSL entry actually uses.
    check_program_block(&stage.block, &registry.programs).expect("accesses are truthful");
    // sources + vorticity×2 + advect velocity + divergence + 24 relaxations + project + advect density
    assert_eq!(stage.block.compute_pass_count(), 1 + 2 + 1 + 1 + 24 + 1 + 1);
    assert_eq!(
        stage.block.binding_of(&aestra_core::ResourceTypeId::new(
            AESTRA_RESOURCE_HOST_BINDINGS
        )),
        Some(10),
        "the solver's binding order"
    );
    let bytes = |id: &str| {
        stage
            .block
            .resources
            .iter()
            .find(|resource| resource.id.as_str() == id)
            .unwrap()
            .bytes
    };
    assert_eq!(
        bytes(RESOURCE_VELOCITY),
        32 * 32 * 32 * 16,
        "bounded by the grid"
    );
}

#[test]
fn without_a_vorticity_module_no_confinement_passes_are_lowered() {
    let registry = fluid_registry();
    let mut effect = smoke_effect(&registry);
    effect.emitters[0]
        .modules
        .retain(|module| module.module_type.0 != MODULE_VORTICITY);
    let stage = compile_stage(&registry, &effect);
    check_program_block(&stage.block, &registry.programs).unwrap();
    assert_eq!(stage.block.compute_pass_count(), 1 + 1 + 1 + 24 + 1 + 1);
}

#[test]
fn invalid_grids_and_missing_grids_fail_lowering() {
    let registry = fluid_registry();
    let mut effect = smoke_effect(&registry);
    set_input(&mut effect, MODULE_GRID, "resolution", Value::U32(30));
    let error = EffectCompiler::with_extensions(registry.clone())
        .compile(&effect)
        .unwrap_err();
    assert!(codes(error).contains(&DiagnosticCode::LoweringFailed));

    let mut effect = smoke_effect(&registry);
    effect.emitters[0]
        .modules
        .retain(|module| module.module_type.0 != MODULE_GRID);
    let error = EffectCompiler::with_extensions(registry)
        .compile(&effect)
        .unwrap_err();
    assert!(codes(error).contains(&DiagnosticCode::LoweringFailed));
}

#[test]
fn a_block_naming_an_unregistered_program_fails_to_compile() {
    let mut registry = fluid_registry();
    registry.programs = Default::default();
    let error = EffectCompiler::with_extensions(registry)
        .compile(&smoke_effect(&fluid_registry()))
        .unwrap_err();
    let report = error.report();
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == DiagnosticCode::LoweringFailed
            && diagnostic.message.contains("unregistered compute program")
    }));
}

#[test]
fn the_program_check_rejects_untruthful_accesses() {
    let registry = fluid_registry();
    let stage = compile_stage(&registry, &smoke_effect(&registry));
    let edit = |edit: &dyn Fn(&mut aestra_runtime::ComputeOp)| {
        let mut block = stage.block.clone();
        for op in &mut block.ops {
            if let ExecutionOp::Compute(compute) = op
                && compute.entry_point == "advect_velocity"
            {
                edit(compute);
            }
        }
        check_program_block(&block, &registry.programs)
    };
    // An access the shader needs is missing.
    let missing = edit(&|compute| {
        compute
            .accesses
            .retain(|access| access.resource.as_str() != AESTRA_RESOURCE_FRAME)
    });
    assert!(missing.unwrap_err().contains("does not declare"));
    // A written resource is declared read-only.
    let read_only = edit(&|compute| {
        for access in &mut compute.accesses {
            access.mode = ResourceAccessMode::Read;
        }
    });
    assert!(read_only.unwrap_err().contains("declared read-only"));
}

#[test]
fn host_bound_source_inputs_pack_their_slot_presence_bit_and_offset() {
    let registry = fluid_registry();
    let mut effect = smoke_effect(&registry);
    let mut emitter = EffectBinding::spatial("Emitter", BindingUpdateMode::Live);
    emitter
        .optional_fields
        .insert(aestra_core::BindingFieldId::new(
            AESTRA_FIELD_LINEAR_VELOCITY,
        ));
    let emitter_id = emitter.id;
    effect.bindings = vec![emitter];
    let source = module_mut(&mut effect, MODULE_DENSITY_SOURCE);
    for (input, field) in [
        ("position", AESTRA_FIELD_POSITION),
        ("velocity", AESTRA_FIELD_LINEAR_VELOCITY),
    ] {
        source
            .property_sources
            .insert(input.into(), PropertySource::HostBinding);
        source
            .host_bindings
            .insert(input.into(), HostFieldRef::new(emitter_id, field));
    }
    let stage = compile_stage(&registry, &effect);
    // Source 0 starts at word 10; its references at +8 (position) and +11 (velocity).
    let words = &stage.block.constants;
    assert_eq!(words[9], 1, "one source");
    assert_eq!(
        &words[18..21],
        &[0, 0, 0],
        "slot 0, bit 0 (position), offset 0"
    );
    assert_eq!(
        &words[21..24],
        &[0, 1, 3],
        "linear velocity: the layout's second field (bit 1), packed after position's 3 words"
    );

    // An unbound source reads the constant fallback marker.
    let stage = compile_stage(&registry, &smoke_effect(&registry));
    assert_eq!(stage.block.constants[18], u32::MAX);
}

#[test]
fn the_compiled_solver_survives_the_artifact_round_trip() {
    let registry = fluid_registry();
    let compiled = EffectCompiler::with_extensions(registry)
        .compile(&smoke_effect(&fluid_registry()))
        .unwrap();
    let bytes = aestra_artifact::encode_effect(&compiled).unwrap();
    let decoded = aestra_artifact::decode_effect(&bytes).unwrap();
    assert_eq!(
        decoded.emitters[0].extension_stages, compiled.emitters[0].extension_stages,
        "programs, constants and the GPU-only flag round-trip"
    );
}

#[test]
fn a_missing_fluid_extension_is_diagnosed_and_the_data_preserved() {
    let effect = smoke_effect(&fluid_registry());
    let saved = effect.to_pretty_ron().unwrap();
    let reopened = EffectAsset::from_ron(&saved).expect("loads without plugin code");
    assert_eq!(reopened, effect);
    let error = EffectCompiler::with_extensions(ExtensionRegistry::builtin())
        .compile(&reopened)
        .unwrap_err();
    assert!(codes(error).contains(&DiagnosticCode::MissingExtension));
    assert_eq!(reopened.to_pretty_ron().unwrap(), saved);
}

#[test]
fn the_committed_smoke_sample_compiles_and_survives_a_missing_plugin_unchanged() {
    let source = include_str!("../../../sample-project/effects/fluid_smoke.aestra.ron");
    let effect = EffectAsset::from_ron(source).expect("the sample loads without plugin code");
    let compiled = EffectCompiler::with_extensions(fluid_registry())
        .compile(&effect)
        .expect("the sample compiles with the plugin");
    let stage = &compiled.emitters[0].extension_stages[0];
    assert!(!stage.cpu_reference);
    let density = stage
        .block
        .field(&aestra_core::ResourceTypeId::new(
            aestra_fluid::RESOURCE_DENSITY,
        ))
        .expect("the density grid declares its field layout");
    assert_eq!(density.dims, [48; 3]);
    assert_eq!(density.components, 1);

    // Opened where the plugin is not linked: named, not dropped, and saved back unchanged.
    let error = EffectCompiler::with_extensions(ExtensionRegistry::builtin())
        .compile(&effect)
        .unwrap_err();
    assert!(codes(error).contains(&DiagnosticCode::MissingExtension));
    assert_eq!(
        effect.to_pretty_ron().unwrap().replace("\r\n", "\n"),
        source.replace("\r\n", "\n")
    );
}
