//! Generates `sample-project/effects/fluid_fireball.aestra.ron` (fluid F10): a fireball — a *Fluid
//! Solver* domain whose source bursts fuel and heat for half a second, burning into a rising, rolling
//! ball of flame and smoke — throwing off sparks where it burns: a Secondary Emission module asks for
//! them in the hot, fast cells, and a spark emitter's Spawn From Domain gives them birth there, on the
//! GPU, deterministically. Written through the real `save_ron`, reloaded, and compiled with the fluid
//! extension installed.
//!
//! Run from the repository root: `cargo run -p aestra-fluid --example gen_fluid_fireball`; an argument
//! writes it elsewhere instead.

use aestra_compiler::{EffectCompiler, ExtensionRegistry};
use aestra_core::{
    ColorKey, Curve, CurveKey, EffectAsset, EffectPlaybackMode, Emitter, EmitterShape, Gradient,
    ModuleInstance, ModuleParameters, ModuleTypeId, ScalarRange, StageKind, Value,
};
use aestra_fluid::{
    FluidExtension, MODULE_BUOYANCY, MODULE_COMBUSTION, MODULE_DENSITY_SOURCE, MODULE_GRID,
    MODULE_SECONDARY_EMISSION, MODULE_TURBULENCE, MODULE_VOLUME_LOOK, MODULE_VORTICITY,
    fire_effect,
};

const PATH: &str = "sample-project/effects/fluid_fireball.aestra.ron";

fn set(effect: &mut EffectAsset, type_id: &str, name: &str, value: Value) {
    let module = effect.simulation_stages[0]
        .modules
        .iter_mut()
        .find(|module| module.module_type.0 == type_id)
        .expect("the fireball has the module");
    let ModuleParameters::Custom(values) = &mut module.parameters else {
        unreachable!("plugin modules carry a generic payload");
    };
    values.insert(name.into(), value);
}

/// Sparks born where the fireball burns hot and fast, with its velocity and a kick of their own,
/// then falling out of it: tiny, white-hot yellow cooling to red. They spawn nothing of their own.
fn sparks() -> Emitter {
    let mut emitter = Emitter::basic_sprite("Sparks", 4.0);
    emitter.max_particles = 4096;
    let appearance = ModuleInstance::appearance(
        Curve::new(vec![CurveKey::new(0.0, 3.0), CurveKey::new(1.0, 1.0)]),
        Curve::new(vec![
            CurveKey::new(0.0, 1.0),
            CurveKey::new(0.7, 0.8),
            CurveKey::new(1.0, 0.0),
        ]),
        Gradient::new(vec![
            ColorKey::new(0.0, [1.0, 0.95, 0.7, 1.0]),
            ColorKey::new(0.3, [1.0, 0.6, 0.15, 1.0]),
            ColorKey::new(1.0, [0.6, 0.08, 0.02, 1.0]),
        ]),
    );
    emitter.modules = vec![
        ModuleInstance::emission(0.0, 0),
        ModuleInstance::shape(EmitterShape::Point),
        ModuleInstance::initialize(
            ScalarRange::new(0.8, 1.8),
            ScalarRange::new(10.0, 30.0),
            [0.0, 1.0, 0.0],
            180.0,
            ScalarRange::new(0.0, 0.0),
        ),
        ModuleInstance::spawn_from_domain(1.0),
        ModuleInstance::motion([0.0, -30.0, 0.0], 0.4, 0.0),
        appearance,
    ];
    emitter
}

fn main() {
    let mut registry = ExtensionRegistry::builtin();
    registry.install(&FluidExtension).expect("install");

    let mut effect = fire_effect(&registry);
    effect.name = "Fluid Fireball".into();
    effect.duration = 4.0;
    effect.playback_mode = EffectPlaybackMode::LoopContinuous;
    effect.emitters = vec![sparks()];
    for type_id in [MODULE_TURBULENCE, MODULE_SECONDARY_EMISSION] {
        let mut module = registry
            .modules
            .instantiate(&ModuleTypeId::new(type_id))
            .expect("the fluid extension is installed");
        module.stage = StageKind::Simulation(effect.simulation_stages[0].name.clone());
        effect.simulation_stages[0].modules.push(module);
    }

    // A 120-unit box (48 cells of 2.5), open but for its floor. The source bursts for half a second
    // near the floor; the ball it lights rises and rolls up out of the box.
    for (type_id, name, value) in [
        (MODULE_GRID, "resolution", Value::U32(48)),
        (MODULE_GRID, "cell_size", Value::Scalar(2.5)),
        (MODULE_GRID, "center", Value::Vec3([0.0, 50.0, 0.0])),
        (MODULE_GRID, "density_dissipation", Value::Scalar(0.25)),
        (MODULE_GRID, "open_top", Value::Bool(true)),
        (MODULE_GRID, "open_sides", Value::Bool(true)),
        (MODULE_GRID, "sharp_advection", Value::Bool(true)),
        (MODULE_GRID, "multigrid_pressure", Value::Bool(true)),
        (MODULE_GRID, "flow_map", Value::Bool(false)),
        (
            MODULE_DENSITY_SOURCE,
            "position",
            Value::Vec3([0.0, 14.0, 0.0]),
        ),
        (MODULE_DENSITY_SOURCE, "radius", Value::Scalar(14.0)),
        (
            MODULE_DENSITY_SOURCE,
            "velocity",
            Value::Vec3([0.0, 30.0, 0.0]),
        ),
        (MODULE_DENSITY_SOURCE, "density_rate", Value::Scalar(1.0)),
        (
            MODULE_DENSITY_SOURCE,
            "temperature_rate",
            Value::Scalar(40.0),
        ),
        (MODULE_DENSITY_SOURCE, "fuel_rate", Value::Scalar(30.0)),
        (MODULE_DENSITY_SOURCE, "duration", Value::Scalar(0.5)),
        (MODULE_BUOYANCY, "strength", Value::Scalar(4.0)),
        (MODULE_VORTICITY, "strength", Value::Scalar(0.5)),
        (MODULE_TURBULENCE, "strength", Value::Scalar(30.0)),
        (MODULE_TURBULENCE, "scale", Value::Scalar(14.0)),
        (MODULE_TURBULENCE, "evolution", Value::Scalar(2.0)),
        (MODULE_COMBUSTION, "thermal_lift", Value::Scalar(12.0)),
        (MODULE_COMBUSTION, "cooling", Value::Scalar(1.0)),
        (MODULE_VOLUME_LOOK, "opacity", Value::Scalar(0.08)),
        (MODULE_VOLUME_LOOK, "color", Value::Vec3([0.3, 0.28, 0.27])),
        (MODULE_VOLUME_LOOK, "fire_intensity", Value::Scalar(0.15)),
        (
            MODULE_VOLUME_LOOK,
            "temperature_scale",
            Value::Scalar(800.0),
        ),
        // Sparks where it burns hot and moves fast.
        (MODULE_SECONDARY_EMISSION, "rate", Value::Scalar(2.0)),
        (MODULE_SECONDARY_EMISSION, "threshold", Value::Scalar(2.0)),
        (MODULE_SECONDARY_EMISSION, "min_speed", Value::Scalar(15.0)),
        (MODULE_SECONDARY_EMISSION, "capacity", Value::U32(64)),
    ] {
        set(&mut effect, type_id, name, value);
    }
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
    assert!(
        compiled.emitters[0].domain_spawn.is_some(),
        "the sparks spawn from the fire"
    );
    println!(
        "wrote {path}: domain '{}', {} dispatches per tick",
        stage.name,
        stage.block.compute_pass_count(),
    );
}
