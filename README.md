# Aestra

> [!WARNING]
> **Work in progress.** Aestra is under active development; features, APIs, and effect formats may change without notice.

**Aestra is a Bevy-native VFX choreography toolkit.** It aims to give Rust/Bevy teams the authoring depth of Niagara while keeping effect assets portable, deterministic, and comfortable to integrate into any game runtime.

## Run

```powershell
cargo run -p aestra-editor
```

The sample asset lives at [`assets/effects/prism_bloom.aestra.ron`](assets/effects/prism_bloom.aestra.ron).

## Workspace

```text
aestra/
├── aestra-editor/           Bevy UI choreography editor
├── aestra-bevy/             Bevy playback and rendering integration
├── aestra-viewer/           Viewer, frame capture, and contact-sheet binary
├── assets/effects/          Authored `.aestra.ron` choreography assets
├── assets/textures/         Renderer textures referenced through stable asset IDs
└── crates/
    ├── aestra-core/         Engine-independent semantic effect model
    ├── aestra-authoring/    Commands, transactions, history, locks, and diffs
    ├── aestra-compiler/     Module registry, validation, and typed lowering
    └── aestra-runtime/      Compiled artifacts and deterministic CPU execution
```

The workspace deliberately has three top-level product modules. Shared internal libraries live under `crates/`; `aestra-core` owns authored format v2, `aestra-authoring` owns UI-independent editing, `aestra-compiler` owns module discovery and lowering, and `aestra-runtime` owns immutable execution plans and instance state. `aestra-bevy` adapts compiled instances to Bevy playback, and both binaries use the same compile/runtime path.

## Viewer and visual analysis

Open the bundled example:

```powershell
cargo run -p aestra-viewer
```

Open another effect:

```powershell
cargo run -p aestra-viewer -- --effect path/to/effect.aestra.ron
```

The bundled textured example can be opened with:

```powershell
cargo run -p aestra-viewer -- --effect assets/effects/ember_sigil.aestra.ron
```

The imported flipbook example exercises explicit atlas frames across CPU, GPU-readback,
and native WESL presentation:

```powershell
cargo run -p aestra-viewer -- --effect assets/effects/plasma_burst.aestra.ron
```

Capture evenly spaced, exact 60 Hz simulation frames plus a single AI-friendly contact sheet:

```powershell
cargo run -p aestra-viewer -- --capture captures/prism-bloom --frames 9
```

The capture directory receives numbered PNG frames, `contact-sheet.png`, and `capture-manifest.md`. The manifest records every sampled frame index and seed. In interactive mode, use Left/Right to step exact frames, `[`/`]` to change the seed, and `S` for a single screenshot. Pass `--seed <decimal-or-hex>` to reproduce a particular run.
The manifest records the requested and selected backend, fallback reason, adapter,
driver, physical capacity, and configured particle budget. Use `--backend
auto|gpu|gpu-readback|cpu` to exercise a specific policy, or
`--max-gpu-particles <count>` to test budget fallback.

Run the native-GPU visual regression against the approved, effect-only reference:

```powershell
cargo run -p aestra-viewer -- --visual-test aestra-viewer/tests/references/prism_bloom target/visual-regression/prism-bloom --frames 8
```

Use the same workflow for the textured renderer reference:

```powershell
cargo run -p aestra-viewer -- --effect assets/effects/ember_sigil.aestra.ron --visual-test aestra-viewer/tests/references/ember_sigil target/visual-regression/ember-sigil --frames 8
```

The command exits with an error when a frame exceeds the tolerant foreground RMSE,
coverage, changed-pixel, or centroid limits. It writes amplified `diff-*.png` images
and `regression-report.md` to the output directory. After intentionally approving a
visual change, regenerate the reference with:

```powershell
cargo run -p aestra-viewer -- --approve-visual-reference aestra-viewer/tests/references/prism_bloom --frames 8
```

## Bevy plugin

```rust
use aestra_bevy::{AestraPlugin, EffectAsset, EffectPlayer};
use bevy::prelude::*;

fn main() {
    App::new()
        .add_plugins((DefaultPlugins, AestraPlugin))
        .add_systems(Startup, |mut commands: Commands| {
            let effect = EffectAsset::load_ron("assets/effects/prism_bloom.aestra.ron")
                .expect("valid effect");
            commands.spawn(EffectPlayer::new(&effect));
            commands.spawn(Camera2d);
        })
        .run();
}
```

Each player receives an `EffectProfiler` component. It exposes measured CPU and
particle statistics alongside compiler-estimated draw, dispatch, and buffer costs;
unsupported measurements such as GPU time remain explicitly unavailable.
Texture paths in an effect's asset registry are relative to the consuming Bevy
application's `AssetPlugin` root. Missing files use a visible checkerboard fallback
and are reported through the effect profile instead of silently removing the draw.
Renderers reference stable material IDs; sprite materials own blend state, softness,
particle-color or typed value bindings, texture assets, and normalized UV regions.
Shared materials compile once and can be reused by multiple renderers.
Flipbook renderers reference a stable atlas definition separately from their material.
Definitions store an imported texture, explicit normalized frame UVs, frame rate, and
loop policy; renderers select particle-age or effect-time playback with deterministic
random starts and forward, reverse, or ping-pong ordering.

## Development

```powershell
cargo fmt --all -- --check
cargo check --workspace
cargo test --workspace
```

See [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) for the product architecture and phased roadmap.
