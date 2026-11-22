//! Generates Grid2D samples for the editor and viewer. Pass `--flow` or `--smoke` for those samples.
//! Run from the repository root with `cargo run --locked -p aestra-fluid --example gen_grid_2d_diffusion`.

use aestra_compiler::{EffectCompiler, ExtensionRegistry};
use aestra_core::EffectAsset;
use aestra_fluid::{
    FluidExtension, grid_2d_diffusion_effect, grid_2d_flow_effect, grid_2d_smoke_effect,
};

fn main() {
    let mut registry = ExtensionRegistry::builtin();
    registry.install(&FluidExtension).expect("install fluid");
    let mut args = std::env::args().skip(1).peekable();
    let (effect, default_path) = if args.next_if(|arg| arg == "--smoke").is_some() {
        (
            grid_2d_smoke_effect(&registry),
            "sample-project/effects/grid_2d_smoke.aestra.ron",
        )
    } else if args.next_if(|arg| arg == "--flow").is_some() {
        (
            grid_2d_flow_effect(&registry),
            "sample-project/effects/grid_2d_flow.aestra.ron",
        )
    } else {
        (
            grid_2d_diffusion_effect(&registry),
            "sample-project/effects/grid_2d_diffusion.aestra.ron",
        )
    };
    let path = args.next().unwrap_or_else(|| default_path.into());
    effect.save_ron(&path).expect("write Grid2D sample");
    let reloaded = EffectAsset::load_ron(&path).expect("reload Grid2D sample");
    EffectCompiler::with_extensions(registry)
        .compile(&reloaded)
        .expect("compile Grid2D sample");
    println!("wrote {path}");
}
