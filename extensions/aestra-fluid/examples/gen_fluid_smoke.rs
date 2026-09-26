//! Generates `sample-project/effects/fluid_smoke.aestra.ron` (fluid F2/F3): a looping smoke plume — an
//! effect-level *Fluid Solver* domain (grid, density source, buoyancy, vorticity), drawn as lit volumetric
//! smoke by its Volume Look — and two emitters whose particles follow it: light "Smoke Puffs" that ride
//! the plume, and heavier "Embers" pulled more weakly against gravity.
//! Written through the real `save_ron`, reloaded, and compiled with the fluid extension installed.
//!
//! Run from the repository root: `cargo run -p aestra-fluid --example gen_fluid_smoke`.

use aestra_compiler::{EffectCompiler, ExtensionRegistry};
use aestra_core::{
    EffectAsset, EffectPlaybackMode, Emitter, EmitterShape, ModuleInstance, ModuleParameters,
    ScalarRange, Value,
};
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

/// A sprite emitter spawning around the origin (inside the plume) whose particles follow the domain.
fn follower(
    name: &str,
    rate: f32,
    lifetime: (f32, f32),
    speed: (f32, f32),
    gravity: f32,
    strength: f32,
) -> Emitter {
    let mut emitter = Emitter::basic_sprite(name, 6.0);
    emitter.max_particles = 512;
    let appearance = emitter
        .modules
        .iter()
        .find(|module| module.module_type.0 == aestra_core::MODULE_APPEARANCE)
        .cloned()
        .expect("a sprite emitter has an appearance");
    emitter.modules = vec![
        ModuleInstance::emission(rate, 0),
        ModuleInstance::shape(EmitterShape::Sphere { radius: 8.0 }),
        ModuleInstance::initialize(
            ScalarRange::new(lifetime.0, lifetime.1),
            ScalarRange::new(speed.0, speed.1),
            [0.0, 1.0, 0.0],
            60.0,
            ScalarRange::new(0.0, 0.0),
        ),
        ModuleInstance::motion([0.0, gravity, 0.0], 0.0, 0.0),
        ModuleInstance::follow_field(strength),
        appearance,
    ];
    emitter
}

fn main() {
    let mut registry = ExtensionRegistry::builtin();
    registry.install(&FluidExtension).expect("install");

    let mut effect = smoke_effect(&registry);
    effect.name = "Fluid Smoke".into();
    effect.duration = 6.0;
    effect.playback_mode = EffectPlaybackMode::LoopContinuous;
    // Two emitters following the one domain (fluid F2b): Follow Field makes each stateful.
    effect.emitters = vec![
        follower("Smoke Puffs", 60.0, (3.0, 4.0), (0.0, 4.0), 0.0, 8.0),
        follower("Embers", 25.0, (1.5, 2.5), (10.0, 20.0), -20.0, 3.0),
    ];

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
        "wrote {PATH}: domain '{}' ({}), {} dispatches per tick; followers: {:?}",
        stage.name,
        stage.stage_type.as_str(),
        stage.block.compute_pass_count(),
        compiled
            .emitters
            .iter()
            .map(|emitter| (emitter.name.as_str(), emitter.field_follow.is_some()))
            .collect::<Vec<_>>()
    );
}
