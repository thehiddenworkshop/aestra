//! Extensible-stages M13, without a GPU: the fluid registers through the public SDK only, declares
//! itself GPU-only, lowers one authored stage into a checked multi-pass solver block, packs host-bound
//! inputs for the GPU, survives the artifact round trip, and is diagnosed — not dropped — when absent.

use aestra_compiler::{EffectCompiler, ExtensionRegistry};
use aestra_core::ResourceTypeId;
use aestra_core::{
    AESTRA_FIELD_LINEAR_VELOCITY, AESTRA_FIELD_POSITION, BindingUpdateMode, DiagnosticCode,
    EffectAsset, EffectBinding, HostFieldRef, ModuleParameters, PropertySource, StageTypeId, Value,
};
use aestra_fluid::{
    FluidExtension, MODULE_COMBUSTION, MODULE_DENSITY_SOURCE, MODULE_GRID, MODULE_VOLUME_LOOK,
    MODULE_VORTICITY, PROGRAM_SOLVER, PROGRAM_VOLUME, RESOURCE_DENSITY, RESOURCE_FUEL,
    RESOURCE_TEMPERATURE, RESOURCE_VELOCITY, STAGE_FLUID_SOLVER, VOLUME_ENTRY, fire_effect,
    smoke_effect,
};
use aestra_gpu::check_program_block;
use aestra_runtime::{
    AESTRA_RESOURCE_FRAME, AESTRA_RESOURCE_HOST_BINDINGS, CompiledExtensionStage, ExecutionOp,
    ResourceAccessMode, SimulationSeekMode, StagePresentation,
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
    effect.simulation_stages[0]
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
    compiled.extension_stages[0].clone()
}

/// The effect with its pressure solved by Jacobi sweeps rather than MGPCG.
fn jacobi(mut effect: EffectAsset) -> EffectAsset {
    set_input(
        &mut effect,
        MODULE_GRID,
        "multigrid_pressure",
        Value::Bool(false),
    );
    effect
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
    assert_eq!(program.entry_points, aestra_fluid::entry_points());
    // The whole program — solver passes plus the host-binding accessors — validates.
    aestra_gpu::program_interface(program).expect("the solver program validates");
}

#[test]
fn one_authored_stage_lowers_to_a_checked_multi_pass_solver() {
    let registry = fluid_registry();
    let effect = jacobi(smoke_effect(&registry));
    let compiled = EffectCompiler::with_extensions(registry.clone())
        .compile(&effect)
        .unwrap();
    assert_eq!(
        compiled.seek_mode,
        SimulationSeekMode::RestartReplay,
        "a fluid is history-dependent"
    );
    let stage = &compiled.extension_stages[0];
    assert!(
        !stage.cpu_reference,
        "the compiled stage says it has no CPU path"
    );
    assert_eq!(stage.modules.len(), 5);
    stage.block.validate().unwrap();
    // Every op's declared accesses match what its WGSL entry actually uses.
    check_program_block(&stage.block, &registry.programs).expect("accesses are truthful");
    // sources + buoyancy + vorticity×3 + advect and correct velocity + divergence + 12 relaxations (the
    // default) +
    // project + advect and correct density
    assert_eq!(
        stage.block.compute_pass_count(),
        1 + 1 + 3 + 2 + 1 + 12 + 1 + 2
    );
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
    let mut effect = jacobi(smoke_effect(&registry));
    effect.simulation_stages[0]
        .modules
        .retain(|module| module.module_type.0 != MODULE_VORTICITY);
    let stage = compile_stage(&registry, &effect);
    check_program_block(&stage.block, &registry.programs).unwrap();
    assert_eq!(stage.block.compute_pass_count(), 1 + 1 + 2 + 1 + 12 + 1 + 2);
}

/// The pressure is solved by MGPCG by default (fluid F5): a setup, then one convergent repeat whose
/// test reads the solve's scalars, capped at the pressure iterations, and whose body — a V-cycle over
/// every level, then the conjugate-gradient step — holds no copy.
#[test]
fn the_pressure_is_solved_by_mgpcg_in_one_convergent_repeat() {
    use aestra_runtime::RepeatPolicy;
    assert_eq!(aestra_fluid::multigrid_levels(32), [32, 16, 8, 4]);
    assert_eq!(aestra_fluid::multigrid_levels(48), [48, 24, 12, 6, 3]);
    assert_eq!(aestra_fluid::multigrid_levels(20), [20, 10, 5]);
    assert_eq!(aestra_fluid::multigrid_levels(128), [128, 64, 32, 16, 8, 4]);

    let registry = fluid_registry();
    let mut effect = smoke_effect(&registry);
    set_input(
        &mut effect,
        MODULE_GRID,
        "pressure_tolerance",
        Value::Scalar(1e-4),
    );
    set_input(
        &mut effect,
        MODULE_GRID,
        "pressure_iterations",
        Value::U32(12),
    );
    let stage = compile_stage(&registry, &effect);
    check_program_block(&stage.block, &registry.programs).expect("accesses are truthful");
    let repeats: Vec<_> = stage
        .block
        .ops
        .iter()
        .filter_map(|op| match op {
            ExecutionOp::Repeat { policy, body } => Some((policy, body)),
            _ => None,
        })
        .collect();
    let [(policy, body)] = repeats.as_slice() else {
        panic!("one repeat: the solve");
    };
    assert_eq!(
        **policy,
        RepeatPolicy::UntilConverged {
            residual: ResourceTypeId::new(aestra_fluid::RESOURCE_PCG_REDUCTION),
            tolerance: 1e-4,
            max: 12,
            test_first: true,
        }
    );
    let entries: Vec<(&str, u32)> = body
        .iter()
        .filter_map(|op| match op {
            ExecutionOp::Compute(compute) => {
                Some((compute.entry_point.as_str(), compute.dispatch.x))
            }
            ExecutionOp::Barrier => None,
            other => panic!("only compute ops and barriers: {other:?}"),
        })
        .collect();
    // Down the levels of 32³, each dispatched over its own cells; the coarsest red, black, red; back
    // up; then the conjugate-gradient step (the fine level's first red sweep rides the previous step).
    // 19 dispatches an iteration.
    assert_eq!(
        entries,
        [
            ("mg_smooth_black_0", 8),
            ("mg_restrict_1", 4),
            ("mg_smooth_black_1", 4),
            ("mg_restrict_2", 2),
            ("mg_smooth_black_2", 2),
            ("mg_restrict_3", 1),
            ("mg_smooth_black_3", 1),
            ("mg_smooth_red_3", 1),
            ("mg_prolong_2", 2),
            ("mg_smooth_red_2", 2),
            ("mg_prolong_1", 4),
            ("mg_smooth_red_1", 4),
            ("mg_prolong_0", 8),
            ("pcg_smooth_dot", 8),
            ("pcg_beta", 1),
            ("pcg_apply", 8),
            ("pcg_alpha", 1),
            ("pcg_step", 8),
            ("pcg_residual", 1),
        ]
    );
    // Under Jacobi, the solve's resources are declared but a few bytes each.
    let jacobi = compile_stage(&registry, &jacobi(smoke_effect(&registry)));
    let bytes = |stage: &CompiledExtensionStage, id: &str| {
        stage
            .block
            .resources
            .iter()
            .find(|resource| resource.id.as_str() == id)
            .unwrap()
            .bytes
    };
    let solution = aestra_fluid::RESOURCE_MG_SOLUTION;
    assert_eq!(bytes(&jacobi, solution), 16);
    assert_eq!(
        bytes(&stage, solution),
        (32u64.pow(3) + 16u64.pow(3) + 8u64.pow(3) + 4u64.pow(3)) * 4
    );
    assert_eq!(
        stage.block.resources.len(),
        jacobi.block.resources.len(),
        "the same bindings either way"
    );
}

/// Flow maps (fluid F6) replace the velocity advection with the leapfrog step and the forward march,
/// and add the cycle's end — the backward maps, the compensation, a second solve that starts
/// converged on every other step, the safeguard — sizing their state by the cycle and the grid. The
/// grid and the cycle are bounded.
#[test]
fn flow_maps_lower_a_leapfrog_cycle_sized_by_its_length() {
    use aestra_runtime::RepeatPolicy;
    let registry = fluid_registry();
    let mut effect = smoke_effect(&registry);
    set_input(&mut effect, MODULE_GRID, "flow_map", Value::Bool(true));
    set_input(&mut effect, MODULE_GRID, "flow_map_cycle", Value::U32(6));
    let stage = compile_stage(&registry, &effect);
    check_program_block(&stage.block, &registry.programs).expect("accesses are truthful");
    assert_eq!(stage.block.constants[18], 6, "the cycle's steps");
    let plain = compile_stage(&registry, &smoke_effect(&registry));
    assert_eq!(plain.block.constants[18], 0, "no flow maps");

    let entries: Vec<String> = aestra_runtime::execute_reference(&stage.block)
        .steps
        .iter()
        .filter_map(|step| step.strip_prefix("compute:fluid/").map(str::to_owned))
        .collect();
    assert!(
        !entries
            .iter()
            .any(|entry| entry.contains("advect_velocity"))
    );
    for entry in [
        "lfm_forces",
        "lfm_advect",
        "lfm_march_forward",
        "lfm_pull_back",
        "lfm_measure_error",
        "lfm_compensate",
        "lfm_impulse_divergence",
        "lfm_project",
        "lfm_energy",
        "lfm_energy_total",
        "lfm_restart",
    ] {
        assert_eq!(
            entries.iter().filter(|name| *name == entry).count(),
            1,
            "{entry} once a tick"
        );
    }
    let solves: Vec<_> = stage
        .block
        .ops
        .iter()
        .filter_map(|op| match op {
            ExecutionOp::Repeat {
                policy: RepeatPolicy::UntilConverged { test_first, .. },
                ..
            } => Some(*test_first),
            _ => None,
        })
        .collect();
    assert_eq!(solves, [true, true], "the midpoint solve and the cycle's");

    let bytes = |id: &str| {
        stage
            .block
            .resources
            .iter()
            .find(|resource| resource.id.as_str() == id)
            .unwrap()
            .bytes
    };
    let cells = 32u64.pow(3);
    assert_eq!(bytes(aestra_fluid::RESOURCE_LFM_HISTORY), cells * 12 * 6);
    assert_eq!(bytes(aestra_fluid::RESOURCE_LFM_FORWARD), cells * 72);
    assert_eq!(stage.block.resources.len(), plain.block.resources.len());

    for (name, value) in [
        ("flow_map_cycle", Value::U32(0)),
        ("flow_map_cycle", Value::U32(17)),
        ("resolution", Value::U32(128)),
    ] {
        let mut invalid = effect.clone();
        set_input(&mut invalid, MODULE_GRID, name, value);
        let error = EffectCompiler::with_extensions(registry.clone())
            .compile(&invalid)
            .unwrap_err();
        assert!(
            codes(error).contains(&DiagnosticCode::LoweringFailed),
            "{name}"
        );
    }
}

/// Jacobi sweeps ping-pong between the pressure grids (fluid F5): an even count copies nothing, an
/// odd one copies its last sweep back once.
#[test]
fn jacobi_sweeps_ping_pong_without_copies() {
    let registry = fluid_registry();
    let copies = |iterations: u32| {
        let mut effect = jacobi(smoke_effect(&registry));
        set_input(
            &mut effect,
            MODULE_GRID,
            "pressure_iterations",
            Value::U32(iterations),
        );
        let stage = compile_stage(&registry, &effect);
        check_program_block(&stage.block, &registry.programs).unwrap();
        aestra_runtime::execute_reference(&stage.block)
            .steps
            .iter()
            .filter(|step| step.contains("pressure_scratch->"))
            .count()
    };
    assert_eq!(copies(24), 0);
    assert_eq!(copies(25), 1);
    assert_eq!(copies(1), 1);
}

#[test]
fn sharp_advection_adds_the_maccormack_corrections_and_keeps_every_binding() {
    let registry = fluid_registry();
    let sharp = compile_stage(&registry, &smoke_effect(&registry));
    let mut plain = smoke_effect(&registry);
    set_input(
        &mut plain,
        MODULE_GRID,
        "sharp_advection",
        Value::Bool(false),
    );
    let plain = compile_stage(&registry, &plain);
    check_program_block(&plain.block, &registry.programs).unwrap();
    assert_eq!(
        sharp.block.compute_pass_count(),
        plain.block.compute_pass_count() + 2,
        "one correction per advected field (velocity, density)"
    );
    assert_eq!(sharp.block.resources, plain.block.resources);
    assert_eq!(
        (sharp.block.constants[11], plain.block.constants[11]),
        (1, 0)
    );
}

#[test]
fn turbulence_rides_the_buoyancy_pass_and_the_look_fades_at_open_sides() {
    let registry = fluid_registry();
    let calm = compile_stage(&registry, &smoke_effect(&registry));
    assert_eq!(calm.block.constants[14], 0, "no turbulence: zero strength");
    let mut effect = smoke_effect(&registry);
    let turbulence = with_module(&registry, &mut effect, aestra_fluid::MODULE_TURBULENCE);
    let ModuleParameters::Custom(values) = &mut effect.simulation_stages[0]
        .modules
        .iter_mut()
        .find(|module| module.id == turbulence)
        .unwrap()
        .parameters
    else {
        unreachable!()
    };
    values.insert("scale".into(), Value::Scalar(4.0));
    values.insert("masked".into(), Value::Bool(false));
    let stirred = compile_stage(&registry, &effect);
    check_program_block(&stirred.block, &registry.programs).unwrap();
    assert_eq!(
        stirred.block.compute_pass_count(),
        calm.block.compute_pass_count(),
        "no pass of its own"
    );
    assert_eq!(stirred.block.resources, calm.block.resources);
    let words = &stirred.block.constants;
    assert_eq!(f32::from_bits(words[14]), 20.0, "strength");
    assert_eq!(
        f32::from_bits(words[15]),
        0.25,
        "noise cycles per unit: 1 / scale"
    );
    assert_eq!(f32::from_bits(words[16]), 1.5, "evolution");
    assert_eq!(words[17], 0, "unmasked");

    // The look knows the grid's open sides (the top, by default) and how deep to fade in from them.
    let StagePresentation::Volume(look) = &calm.presentations[0];
    assert_eq!(look.constants[19], 1 << 3, "the top is open");
    assert_eq!(f32::from_bits(look.constants[20]), 0.15);
}

fn with_module(
    registry: &ExtensionRegistry,
    effect: &mut EffectAsset,
    type_id: &str,
) -> aestra_core::ModuleId {
    let mut module = registry
        .modules
        .instantiate(&aestra_core::ModuleTypeId::new(type_id))
        .unwrap();
    module.stage = aestra_core::StageKind::Simulation(aestra_fluid::SMOKE_STAGE.into());
    let id = module.id;
    effect.simulation_stages[0].modules.push(module);
    id
}

#[test]
fn colliders_pack_their_shapes_and_mark_solids_first() {
    let registry = fluid_registry();
    let plain = compile_stage(&registry, &smoke_effect(&registry));
    assert_eq!(plain.block.constants[12], 0, "no colliders");
    let mut effect = smoke_effect(&registry);
    for type_id in [
        aestra_fluid::MODULE_SPHERE_COLLIDER,
        aestra_fluid::MODULE_BOX_COLLIDER,
        aestra_fluid::MODULE_CAPSULE_COLLIDER,
    ] {
        with_module(&registry, &mut effect, type_id);
    }
    let stage = compile_stage(&registry, &effect);
    check_program_block(&stage.block, &registry.programs).expect("accesses are truthful");
    assert_eq!(
        stage.block.compute_pass_count(),
        plain.block.compute_pass_count() + 1 + 4,
        "one mark_solids pass, and the solid flags of the 4 multigrid levels of 32³"
    );
    let first = stage
        .block
        .ops
        .iter()
        .find_map(|op| match op {
            ExecutionOp::Compute(compute) => Some(compute.entry_point.as_str()),
            _ => None,
        })
        .unwrap();
    assert_eq!(first, "mark_solids");
    let ids = |stage: &CompiledExtensionStage| {
        stage
            .block
            .resources
            .iter()
            .map(|resource| resource.id.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(ids(&stage), ids(&plain), "same bindings");
    let words = &stage.block.constants;
    assert_eq!(words[12], 3);
    let base = words[13] as usize;
    assert_eq!(base, 24 + 17, "after the one source");
    let kinds: Vec<u32> = (0..3).map(|index| words[base + index * 24]).collect();
    assert_eq!(kinds, [0, 1, 2], "sphere, box, capsule");
    assert_eq!(
        words[base + 4],
        u32::MAX,
        "an unbound centre reads its value"
    );
    assert_eq!(words.len(), base + 3 * 24);
}

#[test]
fn a_host_object_can_drive_a_collider_and_invalid_colliders_fail() {
    let registry = fluid_registry();
    let mut effect = smoke_effect(&registry);
    let sphere = with_module(&registry, &mut effect, aestra_fluid::MODULE_SPHERE_COLLIDER);
    let object = EffectBinding::spatial("Paddle", BindingUpdateMode::Live);
    let object_id = object.id;
    effect.bindings = vec![object];
    let collider = effect.simulation_stages[0]
        .modules
        .iter_mut()
        .find(|module| module.id == sphere)
        .unwrap();
    collider
        .property_sources
        .insert("position".into(), PropertySource::HostBinding);
    collider.host_bindings.insert(
        "position".into(),
        HostFieldRef::new(object_id, AESTRA_FIELD_POSITION),
    );
    let stage = compile_stage(&registry, &effect);
    let base = stage.block.constants[13] as usize;
    assert_eq!(&stage.block.constants[base + 4..base + 7], &[0, 0, 0]);

    // The centre is a viewport handle.
    let metadata = registry
        .modules
        .get(&aestra_core::ModuleTypeId::new(
            aestra_fluid::MODULE_SPHERE_COLLIDER,
        ))
        .unwrap();
    assert!(metadata.inputs.iter().any(|input| input.name == "position"
        && input.handle == Some(aestra_core::PropertyHandle::Position)));

    // At most four, and a positive radius.
    let mut crowded = smoke_effect(&registry);
    for _ in 0..5 {
        with_module(
            &registry,
            &mut crowded,
            aestra_fluid::MODULE_SPHERE_COLLIDER,
        );
    }
    assert!(
        EffectCompiler::with_extensions(registry.clone())
            .compile(&crowded)
            .is_err()
    );
    let mut flat = smoke_effect(&registry);
    with_module(&registry, &mut flat, aestra_fluid::MODULE_CAPSULE_COLLIDER);
    set_input(
        &mut flat,
        aestra_fluid::MODULE_CAPSULE_COLLIDER,
        "radius",
        Value::Scalar(0.0),
    );
    assert!(
        EffectCompiler::with_extensions(registry)
            .compile(&flat)
            .is_err()
    );
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
    effect.simulation_stages[0]
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
    // Source 0 starts at word 24; its references at +8 (position) and +11 (velocity).
    let words = &stage.block.constants;
    assert_eq!(words[9], 1, "one source");
    assert_eq!(
        &words[32..35],
        &[0, 0, 0],
        "slot 0, bit 0 (position), offset 0"
    );
    assert_eq!(
        &words[35..38],
        &[0, 1, 3],
        "linear velocity: the layout's second field (bit 1), packed after position's 3 words"
    );

    // An unbound source reads the constant fallback marker.
    let stage = compile_stage(&registry, &smoke_effect(&registry));
    assert_eq!(stage.block.constants[32], u32::MAX);
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
        decoded.extension_stages, compiled.extension_stages,
        "programs, constants and the GPU-only flag round-trip"
    );
}

#[test]
fn the_volume_look_presents_the_density_and_never_touches_the_solver() {
    let registry = fluid_registry();
    let effect = smoke_effect(&registry);
    let stage = compile_stage(&registry, &effect);
    let [StagePresentation::Volume(volume)] = stage.presentations.as_slice() else {
        panic!("one volume presentation");
    };
    assert_eq!(volume.program.as_str(), PROGRAM_VOLUME);
    assert_eq!(volume.entry_point, VOLUME_ENTRY);
    assert_eq!(volume.fields, [ResourceTypeId::new(RESOURCE_DENSITY)]);
    assert_eq!(volume.layouts(&stage.block).unwrap()[0].dims, [32; 3]);
    assert_eq!(volume.constants[0], 48, "march steps");

    // A look edit changes only the presentation: the running simulation is untouched.
    let mut brighter = effect.clone();
    set_input(
        &mut brighter,
        MODULE_VOLUME_LOOK,
        "light_intensity",
        Value::Scalar(3.0),
    );
    let brighter = compile_stage(&registry, &brighter);
    assert_eq!(brighter.block, stage.block);
    assert_ne!(brighter.presentations, stage.presentations);

    // Without a look there is nothing to draw, and still the same solver.
    let mut plain = effect;
    plain.simulation_stages[0]
        .modules
        .retain(|module| module.module_type.0 != MODULE_VOLUME_LOOK);
    let plain = compile_stage(&registry, &plain);
    assert!(plain.presentations.is_empty());
    assert_eq!(plain.block, stage.block);
}

#[test]
fn the_volume_program_is_valid_wgsl_against_the_volume_interface() {
    let registry = fluid_registry();
    let program = registry
        .programs
        .get(&aestra_core::ComputeProgramId::new(PROGRAM_VOLUME))
        .unwrap();
    assert_eq!(program.entry_points, [VOLUME_ENTRY]);
    let source = format!(
        "{}\n{}\n@fragment fn main(@builtin(position) pixel: vec4<f32>) -> @location(0) vec4<f32> {{\n    \
         return {VOLUME_ENTRY}(AestraVolumeRay(vec3<f32>(0.5, 0.0, 0.5), vec3<f32>(0.0, 0.01, 0.0), \
         0.0, 100.0, vec3<f32>(100.0), pixel.xy));\n}}",
        aestra_gpu::volume::volume_interface_wgsl("0"),
        program.wgsl
    );
    let module = naga::front::wgsl::parse_str(&source).expect("parses");
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::default(),
    )
    .validate(&module)
    .expect("the march function validates against the interface");
}

#[test]
fn invalid_looks_and_presentations_fail_to_compile() {
    let registry = fluid_registry();
    for (input, value) in [
        ("steps", Value::U32(0)),
        ("shadow_steps", Value::U32(99)),
        ("light_direction", Value::Vec3([0.0; 3])),
        ("opacity", Value::Scalar(-1.0)),
    ] {
        // Out-of-range inputs fail schema validation or lowering; either way nothing compiles.
        let mut effect = smoke_effect(&registry);
        set_input(&mut effect, MODULE_VOLUME_LOOK, input, value);
        let result = EffectCompiler::with_extensions(registry.clone()).compile(&effect);
        assert!(result.is_err(), "{input} is rejected");
    }

    // A presentation must bind grid fields of its block, on one grid.
    let stage = compile_stage(&registry, &smoke_effect(&registry));
    let StagePresentation::Volume(volume) = &stage.presentations[0];
    let mut unknown = volume.clone();
    unknown.fields = vec![ResourceTypeId::new(aestra_fluid::RESOURCE_PRESSURE)];
    assert!(
        unknown
            .layouts(&stage.block)
            .unwrap_err()
            .contains("not a grid field")
    );
    let mut crowded = volume.clone();
    crowded.fields = vec![ResourceTypeId::new(RESOURCE_DENSITY); 5];
    assert!(crowded.layouts(&stage.block).is_err());
    let mut velocity_and_density = volume.clone();
    velocity_and_density
        .fields
        .push(ResourceTypeId::new(RESOURCE_VELOCITY));
    assert_eq!(velocity_and_density.layouts(&stage.block).unwrap().len(), 2);
}

#[test]
fn combustion_adds_the_fire_grids_passes_and_glow_and_nothing_else() {
    let registry = fluid_registry();
    let smoke = compile_stage(&registry, &smoke_effect(&registry));
    let fire = compile_stage(&registry, &fire_effect(&registry));
    check_program_block(&fire.block, &registry.programs).expect("fire accesses are truthful");
    // add_heat + combust, then advect and correct temperature and fuel.
    assert_eq!(
        fire.block.compute_pass_count(),
        smoke.block.compute_pass_count() + 6
    );
    // The fire grids come last, so every smoke binding keeps its index.
    for (index, resource) in smoke.block.resources.iter().enumerate() {
        assert_eq!(fire.block.binding_of(&resource.id), Some(index as u32));
    }
    for (binding, resource) in [
        RESOURCE_TEMPERATURE,
        aestra_fluid::RESOURCE_TEMPERATURE_NEXT,
        RESOURCE_FUEL,
        aestra_fluid::RESOURCE_FUEL_NEXT,
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(
            fire.block.binding_of(&ResourceTypeId::new(resource)),
            Some(28 + binding as u32)
        );
        assert_eq!(
            smoke.block.binding_of(&ResourceTypeId::new(resource)),
            None,
            "smoke allocates no fire grid"
        );
    }
    for field in [RESOURCE_TEMPERATURE, RESOURCE_FUEL] {
        assert_eq!(
            fire.block
                .field(&ResourceTypeId::new(field))
                .unwrap()
                .components,
            1
        );
    }
    // The Combustion block follows the one source's 17 words.
    assert_eq!(fire.block.constants.len(), 24 + 17 + 6);
    assert_eq!(f32::from_bits(fire.block.constants[41]), 0.5, "ignition");

    // The look burns only where there is fire: the temperature is its slot 1.
    let (StagePresentation::Volume(fire_look), StagePresentation::Volume(smoke_look)) =
        (&fire.presentations[0], &smoke.presentations[0]);
    assert_eq!(
        fire_look.fields,
        [
            ResourceTypeId::new(RESOURCE_DENSITY),
            ResourceTypeId::new(RESOURCE_TEMPERATURE)
        ]
    );
    assert_eq!(fire_look.constants[16], 1);
    assert_eq!(smoke_look.fields, [ResourceTypeId::new(RESOURCE_DENSITY)]);
    assert_eq!(smoke_look.constants[16], u32::MAX);

    // Round-trips like any stage.
    let compiled = EffectCompiler::with_extensions(registry.clone())
        .compile(&fire_effect(&registry))
        .unwrap();
    let decoded =
        aestra_artifact::decode_effect(&aestra_artifact::encode_effect(&compiled).unwrap())
            .unwrap();
    assert_eq!(decoded.extension_stages, compiled.extension_stages);

    // One Combustion per stage.
    let mut twice = fire_effect(&registry);
    let combustion = twice.simulation_stages[0]
        .modules
        .iter()
        .find(|module| module.module_type.0 == MODULE_COMBUSTION)
        .unwrap()
        .clone();
    twice.simulation_stages[0]
        .modules
        .push(aestra_core::ModuleInstance {
            id: aestra_core::ModuleId::new(),
            ..combustion
        });
    assert!(
        EffectCompiler::with_extensions(registry)
            .compile(&twice)
            .is_err()
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
    let stage = &compiled.extension_stages[0];
    assert!(!stage.cpu_reference);
    let density = stage
        .block
        .field(&aestra_core::ResourceTypeId::new(
            aestra_fluid::RESOURCE_DENSITY,
        ))
        .expect("the density grid declares its field layout");
    assert_eq!(density.dims, [48; 3]);
    assert_eq!(density.components, 1);
    assert!(compiled.emitters.is_empty(), "the volume is the smoke");

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

#[test]
fn the_committed_fire_sample_burns_glows_and_survives_a_missing_plugin_unchanged() {
    let source = include_str!("../../../sample-project/effects/fluid_fire.aestra.ron");
    let effect = EffectAsset::from_ron(source).expect("the sample loads without plugin code");
    let registry = fluid_registry();
    let compiled = EffectCompiler::with_extensions(registry.clone())
        .compile(&effect)
        .expect("the sample compiles with the plugin");
    let stage = &compiled.extension_stages[0];
    check_program_block(&stage.block, &registry.programs).unwrap();
    assert!(
        stage
            .block
            .field(&ResourceTypeId::new(RESOURCE_TEMPERATURE))
            .is_some()
    );
    let StagePresentation::Volume(look) = &stage.presentations[0];
    assert_eq!(look.fields.len(), 2, "smoke and fire");
    assert!(
        compiled
            .emitters
            .iter()
            .all(|emitter| emitter.field_follow.is_some()),
        "the embers ride the flames"
    );

    let error = EffectCompiler::with_extensions(ExtensionRegistry::builtin())
        .compile(&effect)
        .unwrap_err();
    assert!(codes(error).contains(&DiagnosticCode::MissingExtension));
    assert_eq!(
        effect.to_pretty_ron().unwrap().replace("\r\n", "\n"),
        source.replace("\r\n", "\n")
    );
}

#[test]
fn followers_resolve_to_the_domains_velocity_and_are_gpu_only() {
    // The fire's embers ride the flames.
    let source = include_str!("../../../sample-project/effects/fluid_fire.aestra.ron");
    let effect = EffectAsset::from_ron(source).unwrap();
    let compiled = EffectCompiler::with_extensions(fluid_registry())
        .compile(&effect)
        .unwrap();
    let velocity = compiled.extension_stages[0]
        .block
        .field(&aestra_core::ResourceTypeId::new(RESOURCE_VELOCITY))
        .unwrap();
    assert!(!compiled.emitters.is_empty());
    for emitter in &compiled.emitters {
        let follow = emitter.field_follow.as_ref().expect("the embers follow");
        assert_eq!(follow.stage, 0);
        assert_eq!(&follow.field, velocity);
        assert_eq!(
            emitter.simulation_class,
            aestra_runtime::SimulationClass::Stateful,
            "following promotes the emitter"
        );
    }
    assert!(compiled.requirements.gpu_fields);
    let cpu = compiled.requirements.compatibility_report(
        &aestra_runtime::BackendCapabilities::default(),
        aestra_runtime::CompatibilityTarget::CpuReference,
    );
    assert_eq!(
        cpu.issues.first().map(|issue| issue.code),
        Some(aestra_runtime::CompatibilityIssueCode::GpuFieldsUnavailable),
        "the CPU reference says why it cannot run this"
    );

    let decoded =
        aestra_artifact::decode_effect(&aestra_artifact::encode_effect(&compiled).unwrap())
            .unwrap();
    assert_eq!(
        decoded.emitters, compiled.emitters,
        "the follow survives the artifact"
    );
    assert!(decoded.requirements.gpu_fields);
}

#[test]
fn a_follower_without_a_domain_is_an_invalid_reference() {
    let mut effect = EffectAsset::new("Lonely", 2.0);
    let mut emitter = aestra_core::Emitter::basic_sprite("Puffs", 2.0);
    emitter
        .modules
        .push(aestra_core::ModuleInstance::follow_field(4.0));
    effect.emitters.push(emitter);
    let error = EffectCompiler::with_extensions(fluid_registry())
        .compile(&effect)
        .unwrap_err();
    assert!(error.report().diagnostics.iter().any(|diagnostic| {
        diagnostic.code == DiagnosticCode::InvalidReference
            && diagnostic
                .message
                .contains("no domain declaring a vector field")
    }));
}

/// The smoke effect on a sparse 512³ grid of `budget` bricks (fluid F7).
fn sparse_smoke(registry: &ExtensionRegistry, budget: u32) -> EffectAsset {
    let mut effect = smoke_effect(registry);
    set_input(&mut effect, MODULE_GRID, "sparse", Value::Bool(true));
    set_input(&mut effect, MODULE_GRID, "resolution", Value::U32(512));
    set_input(&mut effect, MODULE_GRID, "brick_budget", Value::U32(budget));
    effect
}

#[test]
fn a_sparse_grid_lowers_to_passes_over_its_active_bricks() {
    let registry = fluid_registry();
    let stage = compile_stage(&registry, &sparse_smoke(&registry, 100));
    check_program_block(&stage.block, &registry.programs).expect("accesses are truthful");
    let computes: Vec<_> = stage
        .block
        .ops
        .iter()
        .flat_map(|op| match op {
            ExecutionOp::Compute(compute) => vec![compute.clone()],
            ExecutionOp::Repeat { body, .. } => body
                .iter()
                .filter_map(|op| match op {
                    ExecutionOp::Compute(compute) => Some(compute.clone()),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        })
        .collect();
    assert!(computes.iter().all(|compute| {
        compute.program.as_ref().map(|id| id.as_str()) == Some(aestra_fluid::PROGRAM_SOLVER_SPARSE)
    }));
    // The allocation runs first; the passes after it cover the bricks it keeps.
    let entries: Vec<_> = computes.iter().map(|c| c.entry_point.as_str()).collect();
    assert_eq!(
        entries[..6],
        [
            "brick_activity",
            "brick_need",
            "brick_plan",
            "brick_assign",
            "brick_compact",
            "brick_zero"
        ]
    );
    let slots = 101u32;
    for compute in &computes {
        let level = match compute.entry_point.as_str() {
            "brick_need" | "brick_assign" | "brick_plan" | "brick_compact" => None,
            entry if entry.starts_with("pcg_") && compute.dispatch.x == 1 => None,
            "brick_activity" => Some(1),
            entry => Some(
                entry
                    .rsplit('_')
                    .next()
                    .and_then(|level| level.parse::<u32>().ok())
                    .filter(|_| entry.starts_with("mg_"))
                    .unwrap_or(0),
            ),
        };
        match level {
            Some(level) => {
                let counts = compute.indirect.as_ref().expect("sized on the device");
                assert_eq!(
                    counts.resource.as_str(),
                    aestra_fluid::RESOURCE_BRICK_DISPATCH
                );
                assert_eq!(counts.word, 4 * level, "{}", compute.entry_point);
                assert_eq!(
                    compute.dispatch.x,
                    (slots * (512 >> (3 * level))).div_ceil(64),
                    "{}: bounded by every slot",
                    compute.entry_point
                );
            }
            None => assert!(compute.indirect.is_none(), "{}", compute.entry_point),
        }
    }
    // Every field is a pool of the budget's bricks and the empty slot; the table covers 64³ bricks.
    let bytes = |id: &str| {
        stage
            .block
            .resources
            .iter()
            .find(|resource| resource.id.as_str() == id)
            .unwrap()
            .bytes
    };
    assert_eq!(bytes(RESOURCE_VELOCITY), slots as u64 * 512 * 16);
    assert_eq!(bytes(RESOURCE_DENSITY), slots as u64 * 512 * 4);
    assert_eq!(
        bytes(aestra_fluid::RESOURCE_BRICKS),
        (16 + 2 * slots as u64 + 64u64.pow(3)) * 4
    );
    assert_eq!(stage.block.constants[19], slots);
    // Its fields are laid out as bricks over the 512³ grid, so it is drawn, followed and sliced.
    let density = stage
        .block
        .field(&ResourceTypeId::new(RESOURCE_DENSITY))
        .unwrap();
    assert_eq!(density.dims, [512; 3]);
    let bricks = density.bricks.as_ref().expect("bricked");
    assert_eq!(
        (
            bricks.edge,
            bricks.slots,
            bricks.table.as_str(),
            bricks.table_word,
            bricks.slot_bricks_word
        ),
        (
            8,
            slots,
            aestra_fluid::RESOURCE_BRICKS,
            16 + 2 * slots,
            16 + slots
        )
    );
    assert_eq!(stage.presentations.len(), 1, "the volume look");

    // Through the artifact, device-sized passes included.
    let compiled = EffectCompiler::with_extensions(registry.clone())
        .compile(&sparse_smoke(&registry, 100))
        .unwrap();
    let decoded =
        aestra_artifact::decode_effect(&aestra_artifact::encode_effect(&compiled).unwrap())
            .unwrap();
    assert_eq!(decoded.extension_stages, compiled.extension_stages);
}

#[test]
fn a_sparse_grid_takes_a_multiple_of_the_brick_up_to_512_and_no_flow_maps() {
    let registry = fluid_registry();
    let fails = |effect: &EffectAsset| {
        let error = EffectCompiler::with_extensions(registry.clone())
            .compile(effect)
            .unwrap_err();
        codes(error).contains(&DiagnosticCode::LoweringFailed)
    };
    let mut off_brick = sparse_smoke(&registry, 64);
    set_input(&mut off_brick, MODULE_GRID, "resolution", Value::U32(60));
    assert!(fails(&off_brick), "60 is not a multiple of 8");
    let mut too_big = sparse_smoke(&registry, 64);
    set_input(&mut too_big, MODULE_GRID, "resolution", Value::U32(520));
    assert!(fails(&too_big));
    let mut mapped = sparse_smoke(&registry, 64);
    set_input(&mut mapped, MODULE_GRID, "flow_map", Value::Bool(true));
    assert!(fails(&mapped), "flow maps stay dense");
    let mut over_budget = sparse_smoke(&registry, 64);
    set_input(
        &mut over_budget,
        MODULE_GRID,
        "brick_budget",
        Value::U32(aestra_fluid::MAX_BRICK_BUDGET + 1),
    );
    assert!(fails(&over_budget));
    // A dense grid still stops at 128.
    let mut dense = smoke_effect(&registry);
    set_input(&mut dense, MODULE_GRID, "resolution", Value::U32(256));
    assert!(fails(&dense));
}

#[test]
fn a_liquid_lowers_to_checked_particle_and_grid_passes() {
    let registry = fluid_registry();
    let liquid = aestra_fluid::liquid_effect(&registry);
    let stage = compile_stage(&registry, &liquid);
    check_program_block(&stage.block, &registry.programs).expect("liquid accesses are truthful");
    assert_eq!(stage.stage_type.as_str(), aestra_fluid::STAGE_LIQUID_SOLVER);
    let entries: Vec<String> = stage
        .block
        .ops
        .iter()
        .filter_map(|op| match op {
            ExecutionOp::Compute(compute) => Some(compute.entry_point.clone()),
            _ => None,
        })
        .collect();
    // Plan and emit once a tick, then a substep of transfer, solve and move (the default).
    assert_eq!(entries[..2], ["liquid_plan", "liquid_emit"]);
    assert_eq!(
        entries
            .iter()
            .filter(|entry| *entry == "liquid_p2g")
            .count(),
        1
    );
    assert_eq!(
        entries
            .iter()
            .filter(|entry| *entry == "liquid_g2p")
            .count(),
        1
    );
    // The particles persist; the grid velocity and liquid fraction are fields.
    let particles = stage
        .block
        .resources
        .iter()
        .find(|resource| resource.id.as_str() == aestra_fluid::RESOURCE_LIQUID_PARTICLES)
        .unwrap();
    assert_eq!(particles.bytes, 131_072 * 80);
    assert_eq!(
        particles.lifetime,
        aestra_runtime::ResourceLifetime::Persistent
    );
    assert!(
        stage
            .block
            .field(&ResourceTypeId::new(RESOURCE_DENSITY))
            .is_some()
    );
    // Through the artifact.
    let compiled = EffectCompiler::with_extensions(registry.clone())
        .compile(&liquid)
        .unwrap();
    let decoded =
        aestra_artifact::decode_effect(&aestra_artifact::encode_effect(&compiled).unwrap())
            .unwrap();
    assert_eq!(decoded.extension_stages, compiled.extension_stages);
}

#[test]
fn a_liquid_look_presents_the_liquid_fraction_with_a_valid_march() {
    let registry = fluid_registry();
    let mut liquid = aestra_fluid::liquid_effect(&registry);
    let mut look = registry
        .modules
        .instantiate(&aestra_core::ModuleTypeId::new(
            aestra_fluid::MODULE_LIQUID_LOOK,
        ))
        .unwrap();
    look.stage = aestra_core::StageKind::Simulation(aestra_fluid::LIQUID_STAGE.into());
    liquid.simulation_stages[0].modules.push(look);
    let stage = compile_stage(&registry, &liquid);
    let [StagePresentation::Volume(volume)] = stage.presentations.as_slice() else {
        panic!("one volume presentation, got {:?}", stage.presentations);
    };
    assert_eq!(volume.program.as_str(), aestra_fluid::PROGRAM_LIQUID_LOOK);
    assert_eq!(volume.entry_point, aestra_fluid::LIQUID_LOOK_ENTRY);
    assert_eq!(volume.fields, [ResourceTypeId::new(RESOURCE_DENSITY)]);
    volume
        .layouts(&stage.block)
        .expect("the fraction is a declared field");
    // The march function composes with the volume interface into valid WGSL.
    let source = format!(
        "{}\n{}\n@fragment fn main() -> @location(0) vec4<f32> {{\n    \
         return {}(AestraVolumeRay(vec3<f32>(0.5), vec3<f32>(0.0, 0.0, 1.0), 0.0, 1.0, \
         vec3<f32>(96.0), vec2<f32>(0.0)));\n}}",
        aestra_gpu::volume::volume_interface_wgsl("0"),
        aestra_fluid::LIQUID_LOOK_WGSL,
        aestra_fluid::LIQUID_LOOK_ENTRY,
    );
    let module = naga::front::wgsl::parse_str(&source).expect("parses");
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::default(),
    )
    .validate(&module)
    .expect("validates");
}

// ---- Secondary emission (fluid F10) ----

/// A fire asking for sparks, and an emitter born from them.
fn sparks_effect(registry: &ExtensionRegistry) -> EffectAsset {
    let mut effect = fire_effect(registry);
    let mut emission = registry
        .modules
        .instantiate(&aestra_core::ModuleTypeId::new(
            aestra_fluid::MODULE_SECONDARY_EMISSION,
        ))
        .unwrap();
    emission.stage = aestra_core::StageKind::Simulation(effect.simulation_stages[0].name.clone());
    effect.simulation_stages[0].modules.push(emission);
    effect.emitters[0]
        .modules
        .push(aestra_core::ModuleInstance::spawn_from_domain(0.8));
    effect
}

#[test]
fn spawners_resolve_to_the_domains_emission_list_and_are_gpu_only() {
    let registry = fluid_registry();
    let compiled = EffectCompiler::with_extensions(registry.clone())
        .compile(&sparks_effect(&registry))
        .unwrap();
    let stage = &compiled.extension_stages[0];
    check_program_block(&stage.block, &registry.programs).unwrap();
    let list = stage
        .block
        .emission(&ResourceTypeId::new(aestra_fluid::RESOURCE_EMISSION))
        .expect("the fire declares its emission list");
    assert_eq!(list.capacity, 1024);
    // The list and its ranks bind where the emission program expects them.
    assert_eq!(stage.block.binding_of(&list.resource), Some(36));
    let spawn = compiled.emitters[0]
        .domain_spawn
        .as_ref()
        .expect("the emitter spawns from the fire");
    assert_eq!(
        (spawn.stage, &spawn.emission, spawn.inherit),
        (0, list, 0.8)
    );
    assert_eq!(
        compiled.emitters[0].simulation_class,
        aestra_runtime::SimulationClass::Stateful
    );
    assert!(compiled.requirements.gpu_fields);
    let decoded =
        aestra_artifact::decode_effect(&aestra_artifact::encode_effect(&compiled).unwrap())
            .unwrap();
    assert_eq!(decoded.emitters, compiled.emitters);
    assert_eq!(decoded.extension_stages, compiled.extension_stages);

    // The liquid's list binds in the same place.
    let mut liquid = aestra_fluid::liquid_effect(&registry);
    let mut emission = registry
        .modules
        .instantiate(&aestra_core::ModuleTypeId::new(
            aestra_fluid::MODULE_SECONDARY_EMISSION,
        ))
        .unwrap();
    emission.stage = aestra_core::StageKind::Simulation(liquid.simulation_stages[0].name.clone());
    liquid.simulation_stages[0].modules.push(emission);
    let compiled = EffectCompiler::with_extensions(registry.clone())
        .compile(&liquid)
        .unwrap();
    let block = &compiled.extension_stages[0].block;
    check_program_block(block, &registry.programs).unwrap();
    assert_eq!(
        block.binding_of(&ResourceTypeId::new(aestra_fluid::RESOURCE_EMISSION)),
        Some(36)
    );
}

#[test]
fn a_spawner_without_an_emitting_domain_is_an_invalid_reference() {
    let registry = fluid_registry();
    let mut effect = fire_effect(&registry);
    effect.emitters[0]
        .modules
        .push(aestra_core::ModuleInstance::spawn_from_domain(1.0));
    let error = EffectCompiler::with_extensions(registry)
        .compile(&effect)
        .unwrap_err();
    assert!(error.report().diagnostics.iter().any(|diagnostic| {
        diagnostic.code == DiagnosticCode::InvalidReference
            && diagnostic.message.contains("no domain emitting particles")
    }));
}

#[test]
fn the_fireball_and_waterfall_samples_spawn_from_their_domains() {
    let registry = fluid_registry();
    for (source, list) in [
        (
            include_str!("../../../sample-project/effects/fluid_fireball.aestra.ron"),
            aestra_fluid::RESOURCE_EMISSION,
        ),
        (
            include_str!("../../../sample-project/effects/fluid_waterfall.aestra.ron"),
            aestra_fluid::RESOURCE_EMISSION,
        ),
    ] {
        let effect = EffectAsset::from_ron(source).unwrap();
        let compiled = EffectCompiler::with_extensions(registry.clone())
            .compile(&effect)
            .unwrap();
        let stage = &compiled.extension_stages[0];
        check_program_block(&stage.block, &registry.programs).unwrap();
        let declared = stage.block.emission(&ResourceTypeId::new(list)).unwrap();
        let spawn = compiled.emitters[0].domain_spawn.as_ref().unwrap();
        assert_eq!(&spawn.emission, declared, "{}", effect.name);
        assert_eq!(
            effect.to_pretty_ron().unwrap().replace("\r\n", "\n"),
            source.replace("\r\n", "\n")
        );
        // Without the fluid, the sample is diagnosed, not dropped.
        let error = EffectCompiler::with_extensions(ExtensionRegistry::builtin())
            .compile(&effect)
            .unwrap_err();
        assert!(codes(error).contains(&DiagnosticCode::MissingExtension));
    }
}
