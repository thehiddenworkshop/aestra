//! Generates `sample-project/effects/fluid_smoke.aestra.ron` (fluid F2/F3): a looping smoke plume — an
//! effect-level *Fluid Solver* domain (grid, density source, buoyancy, vorticity, turbulence), drawn as
//! lit volumetric smoke by its Volume Look. The volume is the smoke: no particles.
//! Written through the real `save_ron`, reloaded, and compiled with the fluid extension installed.
//!
//! Run from the repository root: `cargo run -p aestra-fluid --example gen_fluid_smoke`; an argument
//! writes it elsewhere instead.

use aestra_compiler::{EffectCompiler, ExtensionRegistry};
use aestra_core::{
    EffectAsset, EffectPlaybackMode, ModuleParameters, ModuleTypeId, StageKind, Value,
};
use aestra_fluid::{
    FluidExtension, MODULE_DENSITY_SOURCE, MODULE_GRID, MODULE_TURBULENCE, MODULE_VORTICITY,
    smoke_effect,
};

const PATH: &str = "sample-project/effects/fluid_smoke.aestra.ron";

fn set(effect: &mut EffectAsset, type_id: &str, name: &str, value: Value) {
    let module = effect.simulation_stages[0]
        .modules
        .iter_mut()
        .find(|module| module.module_type.0 == type_id)
        .expect("the smoke effect has the module");
    let ModuleParameters::Custom(values) = &mut module.parameters else {
        unreachable!("plugin modules carry a generic payload");
    };
    values.insert(name.into(), value);
}

fn main() {
    let mut registry = ExtensionRegistry::builtin();
    registry.install(&FluidExtension).expect("install");

    let mut effect = smoke_effect(&registry);
    effect.name = "Fluid Smoke".into();
    effect.duration = 6.0;
    effect.playback_mode = EffectPlaybackMode::LoopContinuous;
    effect.emitters = Vec::new();
    // A gentle stir, so the plume wavers instead of rising as a regular column.
    let mut turbulence = registry
        .modules
        .instantiate(&ModuleTypeId::new(MODULE_TURBULENCE))
        .expect("the fluid extension is installed");
    turbulence.stage = StageKind::Simulation(effect.simulation_stages[0].name.clone());
    effect.simulation_stages[0].modules.push(turbulence);

    // A 120-unit box (48 cells of 2.5) around the origin; the source near its floor.
    set(&mut effect, MODULE_GRID, "resolution", Value::U32(48));
    set(&mut effect, MODULE_GRID, "cell_size", Value::Scalar(2.5));
    set(
        &mut effect,
        MODULE_GRID,
        "center",
        Value::Vec3([0.0, 40.0, 0.0]),
    );
    // Pressure solved to a tolerance by multigrid-preconditioned CG (fluid F5). Flow maps (F6) stay
    // off: on a plume this small they add about 0.5 ms a tick for no visible gain.
    set(
        &mut effect,
        MODULE_GRID,
        "multigrid_pressure",
        Value::Bool(true),
    );
    set(&mut effect, MODULE_GRID, "flow_map", Value::Bool(false));
    set(
        &mut effect,
        MODULE_DENSITY_SOURCE,
        "position",
        Value::Vec3([0.0, -8.0, 0.0]),
    );
    set(
        &mut effect,
        MODULE_DENSITY_SOURCE,
        "radius",
        Value::Scalar(10.0),
    );
    set(
        &mut effect,
        MODULE_DENSITY_SOURCE,
        "velocity",
        Value::Vec3([0.0, 35.0, 0.0]),
    );
    set(
        &mut effect,
        MODULE_VORTICITY,
        "strength",
        Value::Scalar(0.5),
    );
    set(
        &mut effect,
        MODULE_TURBULENCE,
        "strength",
        Value::Scalar(30.0),
    );
    // Record the plugin requirement exactly as the editor does on save (extensible-stages M11).
    effect.extensions = registry.derive_requirements(&effect);

    let path = std::env::args().nth(1).unwrap_or_else(|| PATH.into());
    effect.save_ron(&path).expect("write sample");
    let reloaded = EffectAsset::load_ron(&path).expect("reload sample");
    assert_eq!(reloaded, effect, "the sample round-trips");
    let compiled = EffectCompiler::with_extensions(registry)
        .compile(&reloaded)
        .expect("the sample compiles with the fluid");
    let stage = &compiled.extension_stages[0];
    println!(
        "wrote {path}: domain '{}' ({}), {} dispatches per tick",
        stage.name,
        stage.stage_type.as_str(),
        stage.block.compute_pass_count(),
    );
}
