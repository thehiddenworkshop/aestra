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
    assert_eq!(program.entry_points.len(), 20);
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
    let stage = &compiled.extension_stages[0];
    assert!(
        !stage.cpu_reference,
        "the compiled stage says it has no CPU path"
    );
    assert_eq!(stage.modules.len(), 5);
    stage.block.validate().unwrap();
    // Every op's declared accesses match what its WGSL entry actually uses.
    check_program_block(&stage.block, &registry.programs).expect("accesses are truthful");
    // sources + buoyancy + vorticity×3 + advect and correct velocity + divergence + 24 relaxations +
    // project + advect and correct density
    assert_eq!(
        stage.block.compute_pass_count(),
        1 + 1 + 3 + 2 + 1 + 24 + 1 + 2
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
    let mut effect = smoke_effect(&registry);
    effect.simulation_stages[0]
        .modules
        .retain(|module| module.module_type.0 != MODULE_VORTICITY);
    let stage = compile_stage(&registry, &effect);
    check_program_block(&stage.block, &registry.programs).unwrap();
    assert_eq!(stage.block.compute_pass_count(), 1 + 1 + 2 + 1 + 24 + 1 + 2);
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
        plain.block.compute_pass_count() + 1,
        "one mark_solids pass"
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
    assert_eq!(base, 18 + 16, "after the one source");
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
    // Source 0 starts at word 18; its references at +8 (position) and +11 (velocity).
    let words = &stage.block.constants;
    assert_eq!(words[9], 1, "one source");
    assert_eq!(
        &words[26..29],
        &[0, 0, 0],
        "slot 0, bit 0 (position), offset 0"
    );
    assert_eq!(
        &words[29..32],
        &[0, 1, 3],
        "linear velocity: the layout's second field (bit 1), packed after position's 3 words"
    );

    // An unbound source reads the constant fallback marker.
    let stage = compile_stage(&registry, &smoke_effect(&registry));
    assert_eq!(stage.block.constants[26], u32::MAX);
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
            Some(14 + binding as u32)
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
    // The Combustion block follows the one source's 16 words.
    assert_eq!(fire.block.constants.len(), 18 + 16 + 6);
    assert_eq!(f32::from_bits(fire.block.constants[34]), 0.5, "ignition");

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
    let source = include_str!("../../../sample-project/effects/fluid_smoke.aestra.ron");
    let effect = EffectAsset::from_ron(source).unwrap();
    let compiled = EffectCompiler::with_extensions(fluid_registry())
        .compile(&effect)
        .unwrap();
    let velocity = compiled.extension_stages[0]
        .block
        .field(&aestra_core::ResourceTypeId::new(RESOURCE_VELOCITY))
        .unwrap();
    for emitter in &compiled.emitters {
        let follow = emitter.field_follow.as_ref().expect("both emitters follow");
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
