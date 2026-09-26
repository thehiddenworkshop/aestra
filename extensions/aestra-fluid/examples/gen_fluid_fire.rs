//! Generates `sample-project/effects/fluid_fire.aestra.ron` (fluid F3): a looping fire — an
//! effect-level *Fluid Solver* domain whose source emits fuel and the heat that ignites it, a
//! Combustion module burning it into heat and smoke, and a Volume Look drawing the flames as blackbody
//! emission inside lit smoke — plus embers riding the flames. Written through the real `save_ron`,
//! reloaded, and compiled with the fluid extension installed.
//!
//! Run from the repository root: `cargo run -p aestra-fluid --example gen_fluid_fire`.

use aestra_compiler::{EffectCompiler, ExtensionRegistry};
use aestra_core::{
    EffectAsset, EffectPlaybackMode, Emitter, EmitterShape, ModuleInstance, ModuleParameters,
    ModuleTypeId, ScalarRange, StageKind, Value,
};
use aestra_fluid::{
    FluidExtension, MODULE_BUOYANCY, MODULE_COMBUSTION, MODULE_DENSITY_SOURCE, MODULE_GRID,
    MODULE_TURBULENCE, MODULE_VOLUME_LOOK, MODULE_VORTICITY, fire_effect,
};

const PATH: &str = "sample-project/effects/fluid_fire.aestra.ron";

fn set(effect: &mut EffectAsset, type_id: &str, name: &str, value: Value) {
    let module = effect.simulation_stages[0]
        .modules
        .iter_mut()
        .find(|module| module.module_type.0 == type_id)
        .expect("the fire effect has the module");
    let ModuleParameters::Custom(values) = &mut module.parameters else {
        unreachable!("plugin modules carry a generic payload");
    };
    values.insert(name.into(), value);
}

/// Sparks spawned in the burning core that the flames carry up, against gravity.
fn embers() -> Emitter {
    let mut emitter = Emitter::basic_sprite("Embers", 6.0);
    emitter.max_particles = 256;
    let appearance = emitter
        .modules
        .iter()
        .find(|module| module.module_type.0 == aestra_core::MODULE_APPEARANCE)
        .cloned()
        .expect("a sprite emitter has an appearance");
    emitter.modules = vec![
        ModuleInstance::emission(30.0, 0),
        ModuleInstance::shape(EmitterShape::Sphere { radius: 6.0 }),
        ModuleInstance::initialize(
            ScalarRange::new(1.0, 2.0),
            ScalarRange::new(5.0, 15.0),
            [0.0, 1.0, 0.0],
            40.0,
            ScalarRange::new(0.0, 0.0),
        ),
        ModuleInstance::motion([0.0, -15.0, 0.0], 0.0, 0.0),
        ModuleInstance::follow_field(4.0),
        appearance,
    ];
    emitter
}

fn main() {
    let mut registry = ExtensionRegistry::builtin();
    registry.install(&FluidExtension).expect("install");

    let mut effect = fire_effect(&registry);
    effect.name = "Fluid Fire".into();
    effect.duration = 6.0;
    effect.playback_mode = EffectPlaybackMode::LoopContinuous;
    effect.emitters = vec![embers()];
    let mut turbulence = registry
        .modules
        .instantiate(&ModuleTypeId::new(MODULE_TURBULENCE))
        .expect("the fluid extension is installed");
    turbulence.stage = StageKind::Simulation(effect.simulation_stages[0].name.clone());
    effect.simulation_stages[0].modules.push(turbulence);

    // A 120-unit box (48 cells of 2.5); the burner sits near its floor. Open on every side but the
    // floor, so the smoke rises out and air is drawn in around the flames.
    for (type_id, name, value) in [
        (MODULE_GRID, "resolution", Value::U32(48)),
        (MODULE_GRID, "cell_size", Value::Scalar(2.5)),
        (MODULE_GRID, "center", Value::Vec3([0.0, 40.0, 0.0])),
        (MODULE_GRID, "density_dissipation", Value::Scalar(0.35)),
        (MODULE_GRID, "open_top", Value::Bool(true)),
        (MODULE_GRID, "open_sides", Value::Bool(true)),
        (MODULE_GRID, "sharp_advection", Value::Bool(true)),
        (
            MODULE_DENSITY_SOURCE,
            "position",
            Value::Vec3([0.0, -4.0, 0.0]),
        ),
        (MODULE_DENSITY_SOURCE, "radius", Value::Scalar(12.0)),
        (
            MODULE_DENSITY_SOURCE,
            "velocity",
            Value::Vec3([0.0, 10.0, 0.0]),
        ),
        (MODULE_DENSITY_SOURCE, "density_rate", Value::Scalar(0.2)),
        (
            MODULE_DENSITY_SOURCE,
            "temperature_rate",
            Value::Scalar(6.0),
        ),
        (MODULE_DENSITY_SOURCE, "fuel_rate", Value::Scalar(5.0)),
        (MODULE_BUOYANCY, "strength", Value::Scalar(4.0)),
        (MODULE_VORTICITY, "strength", Value::Scalar(0.3)),
        (MODULE_TURBULENCE, "strength", Value::Scalar(40.0)),
        (MODULE_TURBULENCE, "scale", Value::Scalar(18.0)),
        (MODULE_TURBULENCE, "evolution", Value::Scalar(2.0)),
        (MODULE_COMBUSTION, "thermal_lift", Value::Scalar(10.0)),
        (MODULE_COMBUSTION, "cooling", Value::Scalar(1.4)),
        (MODULE_VOLUME_LOOK, "opacity", Value::Scalar(0.08)),
        (MODULE_VOLUME_LOOK, "color", Value::Vec3([0.35, 0.33, 0.32])),
        (MODULE_VOLUME_LOOK, "fire_intensity", Value::Scalar(0.15)),
        (
            MODULE_VOLUME_LOOK,
            "temperature_scale",
            Value::Scalar(800.0),
        ),
    ] {
        set(&mut effect, type_id, name, value);
    }
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
        "wrote {PATH}: domain '{}', {} dispatches per tick, {} field(s) drawn",
        stage.name,
        stage.block.compute_pass_count(),
        match &stage.presentations[0] {
            aestra_runtime::StagePresentation::Volume(volume) => volume.fields.len(),
        }
    );
}
