//! Generates `sample-project/effects/fluid_dam_break.aestra.ron` (fluid F8): a column of water released
//! in a closed box — a *Liquid Solver* domain (Liquid Grid, Liquid Block) — that collapses, runs across
//! the floor over a sphere and splashes up the far wall, drawn by its Liquid Look. Written through the
//! real `save_ron`, reloaded, and compiled with the fluid extension installed.
//!
//! Run from the repository root: `cargo run -p aestra-fluid --example gen_fluid_dam_break`; an argument
//! writes it elsewhere instead.

use aestra_compiler::{EffectCompiler, ExtensionRegistry};
use aestra_core::{
    EffectAsset, EffectPlaybackMode, ModuleParameters, ModuleTypeId, StageKind, Value,
};
use aestra_fluid::{
    FluidExtension, MODULE_LIQUID_GRID, MODULE_LIQUID_LOOK, MODULE_SPHERE_COLLIDER, liquid_effect,
};

const PATH: &str = "sample-project/effects/fluid_dam_break.aestra.ron";

fn set(effect: &mut EffectAsset, type_id: &str, name: &str, value: Value) {
    let module = effect.simulation_stages[0]
        .modules
        .iter_mut()
        .find(|module| module.module_type.0 == type_id)
        .expect("the dam break has the module");
    let ModuleParameters::Custom(values) = &mut module.parameters else {
        unreachable!("plugin modules carry a generic payload");
    };
    values.insert(name.into(), value);
}

fn main() {
    let mut registry = ExtensionRegistry::builtin();
    registry.install(&FluidExtension).expect("install");

    let mut effect = liquid_effect(&registry);
    effect.name = "Fluid Dam Break".into();
    effect.duration = 4.0;
    effect.playback_mode = EffectPlaybackMode::LoopContinuous;
    for type_id in [MODULE_LIQUID_LOOK, MODULE_SPHERE_COLLIDER] {
        let mut module = registry
            .modules
            .instantiate(&ModuleTypeId::new(type_id))
            .expect("the fluid extension is installed");
        module.stage = StageKind::Simulation(effect.simulation_stages[0].name.clone());
        effect.simulation_stages[0].modules.push(module);
    }

    // A 96-unit box (48 cells of 2) on the floor at y = 0; the block (the Liquid Block's defaults)
    // stands against the -x wall, and a sphere sits in the flow's path.
    for (type_id, name, value) in [
        (MODULE_LIQUID_GRID, "resolution", Value::U32(48)),
        (MODULE_LIQUID_GRID, "cell_size", Value::Scalar(2.0)),
        (MODULE_LIQUID_GRID, "center", Value::Vec3([0.0, 48.0, 0.0])),
        (
            MODULE_SPHERE_COLLIDER,
            "position",
            Value::Vec3([12.0, 6.0, 0.0]),
        ),
        (MODULE_SPHERE_COLLIDER, "radius", Value::Scalar(14.0)),
        (MODULE_LIQUID_LOOK, "steps", Value::U32(128)),
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
    println!(
        "wrote {path}: domain '{}', {} dispatches per tick, {} presentation(s)",
        stage.name,
        stage.block.compute_pass_count(),
        stage.presentations.len()
    );
}
