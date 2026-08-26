# Aestra

**Aestra is a Bevy-native VFX choreography toolkit.** It aims to give Rust/Bevy teams the authoring depth of Niagara while keeping effect assets portable, deterministic, and comfortable to integrate into any game runtime.

The first vertical slice already includes:

- a complete editor shell built with Bevy UI, including File/Edit/View/Help menus;
- project effect discovery, native New/Open/Save/Save As workflows, and unsaved-change protection;
- command-based undo/redo for emitter, timing, curve, gradient, and layer edits;
- a discoverable built-in module registry and typed compiler execution plans;
- immutable compiled effects and seeded deterministic runtime instances;
- typed effect-parameter bindings with zero-recompile runtime overrides;
- compiled curves/gradients, constant folding, and particle-attribute liveness;
- an effect library, layer stack, viewport, inspector, interactive timeline, transport, and status bar;
- deterministic emitter evaluation with point, circle, ring, and cone shapes;
- ranges, smooth curves, color gradients, drag, gravity, turbulence, and layer timing;
- renderer declarations for billboards, ribbons, and meshes;
- event-link data for spawn, death, and collision choreography;
- RON asset validation, loading, and saving;
- an authored four-layer `Prism Bloom` sample;
- interactive timeline scrubbing, playback, emitter tuning, curve previews, and gradient presets.

## Run

```powershell
cargo run -p aestra-editor
```

Controls:

- `Space`: play or pause
- `R`: restart the effect
- `G`: toggle the preview grid
- `Ctrl+N` / `Ctrl+O`: create or open an effect
- `Ctrl+S` / `Ctrl+Shift+S`: save or save as
- `Ctrl+Z` / `Ctrl+Y`: undo or redo
- `Ctrl+Enter`: add an emitter
- `Ctrl+D` / `Delete`: duplicate or delete the selected emitter
- layer rows: select an emitter
- timeline: click or drag to scrub
- inspector `-` / `+`: tune emission, timing, size, opacity, and duration

The sample asset lives at [`assets/effects/prism_bloom.aestra.ron`](assets/effects/prism_bloom.aestra.ron).

## Workspace

```text
aestra/
├── aestra-editor/           Bevy UI choreography editor
├── aestra-bevy/             Bevy playback and rendering integration
├── aestra-viewer/           Viewer, frame capture, and contact-sheet binary
├── assets/effects/          Authored `.aestra.ron` choreography assets
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

## Development

```powershell
cargo fmt --all -- --check
cargo check --workspace
cargo test --workspace
```

The editor round-trip tests create a new multi-emitter effect, save it, and load it through the shared semantic model used by the viewer and game plugin.

The runtime packs immutable compiled plans into bounded GPU buffers, evaluates them with embedded WESL compute shaders, compacts the live set, and presents alpha/additive sprites through GPU indirect draws. Conservative authored bounds participate in Bevy visibility culling. `AestraSettings` defaults to `PresentationMode::Auto`: native GPU is preferred, GPU readback is used when compute works without native indirect presentation, and the deterministic CPU interpreter is the final fallback. `GpuCapabilities`, `AestraRuntimeStatus`, and per-effect `EffectRuntimeStatus` expose the decision and its reason through the same `AestraPlugin` / `EffectPlayer` surface.

See [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) for the product architecture and phased roadmap.
