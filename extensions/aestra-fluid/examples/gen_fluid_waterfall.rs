//! Generates `sample-project/effects/fluid_waterfall.aestra.ron` (fluid F10): a waterfall — a *Liquid
//! Solver* domain whose source pours water off a ledge (a box collider) into a pool below — throwing
//! up spray where it lands: a Secondary Emission module asks for droplets where fast water leaves the
//! liquid, and a spray emitter's Spawn From Domain gives them birth there, on the GPU,
//! deterministically. Written through the real `save_ron`, reloaded, and compiled with the fluid
//! extension installed.
//!
//! Run from the repository root: `cargo run -p aestra-fluid --example gen_fluid_waterfall`; an argument
//! writes it elsewhere instead.

use aestra_compiler::{EffectCompiler, ExtensionRegistry};
use aestra_core::{
    ColorKey, Curve, CurveKey, EffectAsset, EffectPlaybackMode, Emitter, EmitterShape, Gradient,
    ModuleInstance, ModuleParameters, ModuleTypeId, ScalarRange, StageKind, Value,
};
use aestra_fluid::{
    FluidExtension, MODULE_BOX_COLLIDER, MODULE_LIQUID_BLOCK, MODULE_LIQUID_GRID,
    MODULE_LIQUID_LOOK, MODULE_LIQUID_SOURCE, MODULE_SECONDARY_EMISSION, liquid_effect,
};

const PATH: &str = "sample-project/effects/fluid_waterfall.aestra.ron";

fn set(effect: &mut EffectAsset, type_id: &str, name: &str, value: Value) {
    let module = effect.simulation_stages[0]
        .modules
        .iter_mut()
        .find(|module| module.module_type.0 == type_id)
        .expect("the waterfall has the module");
    let ModuleParameters::Custom(values) = &mut module.parameters else {
        unreachable!("plugin modules carry a generic payload");
    };
    values.insert(name.into(), value);
}

/// Droplets born where the falling water breaks up, with its velocity, slowed by the air and falling
/// back: small, pale, fading fast. They spawn nothing of their own.
fn spray() -> Emitter {
    let mut emitter = Emitter::basic_sprite("Spray", 4.0);
    emitter.max_particles = 16384;
    let appearance = ModuleInstance::appearance(
        Curve::new(vec![CurveKey::new(0.0, 1.5), CurveKey::new(1.0, 3.0)]),
        Curve::new(vec![
            CurveKey::new(0.0, 0.45),
            CurveKey::new(0.5, 0.25),
            CurveKey::new(1.0, 0.0),
        ]),
        Gradient::new(vec![
            ColorKey::new(0.0, [0.9, 0.96, 1.0, 1.0]),
            ColorKey::new(1.0, [0.7, 0.82, 0.92, 1.0]),
        ]),
    );
    emitter.modules = vec![
        ModuleInstance::emission(0.0, 0),
        ModuleInstance::shape(EmitterShape::Point),
        ModuleInstance::initialize(
            ScalarRange::new(0.4, 1.0),
            ScalarRange::new(0.0, 20.0),
            [0.0, 1.0, 0.0],
            180.0,
            ScalarRange::new(0.0, 0.0),
        ),
        ModuleInstance::spawn_from_domain(0.8),
        ModuleInstance::motion([0.0, -200.0, 0.0], 2.0, 0.0),
        appearance,
    ];
    emitter
}

fn main() {
    let mut registry = ExtensionRegistry::builtin();
    registry.install(&FluidExtension).expect("install");

    let mut effect = liquid_effect(&registry);
    effect.name = "Fluid Waterfall".into();
    effect.duration = 4.0;
    effect.playback_mode = EffectPlaybackMode::LoopContinuous;
    effect.emitters = vec![spray()];
    for type_id in [
        MODULE_BOX_COLLIDER,
        MODULE_LIQUID_LOOK,
        MODULE_SECONDARY_EMISSION,
    ] {
        let mut module = registry
            .modules
            .instantiate(&ModuleTypeId::new(type_id))
            .expect("the fluid extension is installed");
        module.stage = StageKind::Simulation(effect.simulation_stages[0].name.clone());
        effect.simulation_stages[0].modules.push(module);
    }

    // A sheet of water: four sources side by side along the ledge's top, pouring over its edge.
    for z in [-27.0, -9.0, 9.0, 27.0] {
        let mut source = registry
            .modules
            .instantiate(&ModuleTypeId::new(MODULE_LIQUID_SOURCE))
            .expect("the fluid extension is installed");
        source.stage = StageKind::Simulation(effect.simulation_stages[0].name.clone());
        let ModuleParameters::Custom(values) = &mut source.parameters else {
            unreachable!("plugin modules carry a generic payload");
        };
        for (name, value) in [
            ("position", Value::Vec3([-38.0, 66.0, z])),
            ("radius", Value::Scalar(6.0)),
            ("velocity", Value::Vec3([45.0, 0.0, 0.0])),
            ("rate", Value::Scalar(4000.0)),
        ] {
            values.insert(name.into(), value);
        }
        effect.simulation_stages[0].modules.push(source);
    }

    // A 96-unit box (48 cells of 2) on the floor at y = 0. A ledge 60 units tall stands against the
    // -x wall; the sheet falls off its edge into a pool 8 deep.
    for (type_id, name, value) in [
        (MODULE_LIQUID_GRID, "resolution", Value::U32(48)),
        (MODULE_LIQUID_GRID, "cell_size", Value::Scalar(2.0)),
        (MODULE_LIQUID_GRID, "center", Value::Vec3([0.0, 48.0, 0.0])),
        (MODULE_LIQUID_GRID, "particle_budget", Value::U32(400_000)),
        (MODULE_LIQUID_BLOCK, "center", Value::Vec3([8.0, 4.0, 0.0])),
        (MODULE_LIQUID_BLOCK, "size", Value::Vec3([80.0, 8.0, 96.0])),
        (
            MODULE_BOX_COLLIDER,
            "position",
            Value::Vec3([-40.0, 30.0, 0.0]),
        ),
        (
            MODULE_BOX_COLLIDER,
            "half_extents",
            Value::Vec3([8.0, 30.0, 48.0]),
        ),
        (MODULE_LIQUID_LOOK, "steps", Value::U32(128)),
        // Spray where fast water flies clear of the liquid.
        (MODULE_SECONDARY_EMISSION, "rate", Value::Scalar(5.0)),
        (MODULE_SECONDARY_EMISSION, "threshold", Value::Scalar(0.35)),
        (MODULE_SECONDARY_EMISSION, "min_speed", Value::Scalar(60.0)),
        (MODULE_SECONDARY_EMISSION, "capacity", Value::U32(1024)),
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
        "the spray spawns from the water"
    );
    println!(
        "wrote {path}: domain '{}', {} dispatches per tick",
        stage.name,
        stage.block.compute_pass_count(),
    );
}
