//! Generates `sample-project/effects/plugin_lab.aestra.ron` (extensible-stages M10): a sprite fountain
//! whose "Field Forces" simulation stage — a plugin stage type — hosts two Vortex modules. Written
//! through the real `save_ron`, reloaded, and compiled with the example extension installed, printing
//! the plugin stage's reference execution trace.
//!
//! Run from the repository root: `cargo run -p aestra-example-extension --example gen_plugin_lab`.

use aestra_compiler::{EffectCompiler, ExtensionRegistry};
use aestra_core::{
    EffectAsset, EffectPlaybackMode, Emitter, ModuleParameters, ModuleTypeId, StageKind,
    StageTypeId, Value,
};
use aestra_example_extension::{ExampleExtension, MODULE_VORTEX, STAGE_FIELD_FORCES};

const PATH: &str = "sample-project/effects/plugin_lab.aestra.ron";
const STAGE: &str = "Field Forces";

fn main() {
    let mut registry = ExtensionRegistry::builtin();
    registry.install(&ExampleExtension).expect("install");

    let mut effect = EffectAsset::new("Plugin Lab", 3.0);
    effect.playback_mode = EffectPlaybackMode::LoopContinuous;
    let mut emitter = Emitter::basic_sprite("Swirl", 3.0);
    emitter.max_particles = 256;
    emitter
        .simulation_stage_types
        .insert(STAGE.into(), StageTypeId::new(STAGE_FIELD_FORCES));
    for (label, strength, radius, axis) in [
        ("Core", 6.0, 1.0, [0.0, 1.0, 0.0]),
        ("Outer", 2.5, 4.0, [0.0, 1.0, 0.25]),
    ] {
        let mut vortex = registry
            .modules
            .instantiate(&ModuleTypeId::new(MODULE_VORTEX))
            .expect("vortex instantiates");
        vortex.stage = StageKind::Simulation(STAGE.into());
        vortex.label = Some(label.into());
        let ModuleParameters::Custom(values) = &mut vortex.parameters else {
            unreachable!("plugin modules carry a generic payload");
        };
        values.insert("strength".into(), Value::Scalar(strength));
        values.insert("radius".into(), Value::Scalar(radius));
        values.insert("axis".into(), Value::Vec3(axis));
        emitter.modules.push(vortex);
    }
    effect.emitters.push(emitter);
    // Record the plugin requirement exactly as the editor does on save (extensible-stages M11).
    effect.extensions = registry.derive_requirements(&effect);

    effect.save_ron(PATH).expect("write sample");
    let reloaded = EffectAsset::load_ron(PATH).expect("reload sample");
    assert_eq!(reloaded, effect, "the sample round-trips");
    let compiled = EffectCompiler::with_extensions(registry)
        .compile(&reloaded)
        .expect("the sample compiles with the plugin");
    let stage = &compiled.emitters[0].extension_stages[0];
    println!(
        "wrote {PATH}: stage '{}' ({}) -> {:?}",
        stage.name,
        stage.stage_type.as_str(),
        aestra_runtime::execute_reference(&stage.block).steps
    );
}
