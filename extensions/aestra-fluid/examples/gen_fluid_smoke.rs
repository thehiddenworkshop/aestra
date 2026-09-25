//! Generates `sample-project/effects/fluid_smoke.aestra.ron` (fluid F1): a looping smoke plume — an
//! emitter hosting a *Fluid Solver* stage (grid, density source, buoyancy, vorticity). The emitter
//! spawns no particles: until particles are coupled to the fluid (fluid F2) the plume is shown through
//! the editor/viewer debug slice of its density field. Written through the real `save_ron`, reloaded,
//! and compiled with the fluid extension installed.
//!
//! Run from the repository root: `cargo run -p aestra-fluid --example gen_fluid_smoke`.

use aestra_compiler::{EffectCompiler, ExtensionRegistry};
use aestra_core::{EffectAsset, EffectPlaybackMode, ModuleInstance, ModuleParameters, Value};
use aestra_fluid::{
    FluidExtension, MODULE_DENSITY_SOURCE, MODULE_GRID, MODULE_VORTICITY, smoke_effect,
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
    let emitter = &mut effect.emitters[0];
    emitter.name = "Smoke Domain".into();
    emitter.duration = 6.0;
    // No particles yet: the fluid is presented through its density slice until fluid F2.
    emitter.modules[0] = ModuleInstance::emission(0.0, 0);

    // A 120-unit box (48 cells of 2.5) around the origin; the source near its floor.
    set(&mut effect, MODULE_GRID, "resolution", Value::U32(48));
    set(&mut effect, MODULE_GRID, "cell_size", Value::Scalar(2.5));
    set(
        &mut effect,
        MODULE_GRID,
        "center",
        Value::Vec3([0.0, 40.0, 0.0]),
    );
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
    // Record the plugin requirement exactly as the editor does on save (extensible-stages M11).
    effect.extensions = registry.derive_requirements(&effect);

    effect.save_ron(PATH).expect("write sample");
    let reloaded = EffectAsset::load_ron(PATH).expect("reload sample");
    assert_eq!(reloaded, effect, "the sample round-trips");
    let compiled = EffectCompiler::with_extensions(registry)
        .compile(&reloaded)
        .expect("the sample compiles with the fluid");
    let stage = &compiled.extension_stages[0];
    println!(
        "wrote {PATH}: stage '{}' ({}), {} dispatches per tick",
        stage.name,
        stage.stage_type.as_str(),
        stage.block.compute_pass_count()
    );
}
