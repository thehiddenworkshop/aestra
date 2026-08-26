# Aestra

**Aestra is a Bevy-native VFX choreography toolkit.** It aims to give Rust/Bevy teams the authoring depth of Niagara while keeping effect assets portable, deterministic, and comfortable to integrate into any game runtime.

The first vertical slice already includes:

- a complete editor shell built with Bevy UI;
- an effect library, layer stack, viewport, inspector, timeline, transport, and status bar;
- deterministic emitter evaluation with point, circle, ring, and cone shapes;
- ranges, smooth curves, color gradients, drag, gravity, turbulence, and layer timing;
- renderer declarations for billboards, ribbons, and meshes;
- event-link data for spawn, death, and collision choreography;
- RON asset validation, loading, and saving;
- an authored four-layer `Prism Bloom` sample;
- interactive playback and emitter tuning.

## Run

```powershell
cargo run -p aestra-editor
```

Controls:

- `Space`: play or pause
- `R`: restart the effect
- `Ctrl+S`: save the current effect
- layer rows: select an emitter
- inspector `−` / `+`: tune emission, burst, and lifetime

The sample asset lives at [`assets/effects/prism_bloom.aestra.ron`](assets/effects/prism_bloom.aestra.ron).

## Workspace

```text
aestra/
├── aestra-editor/           Bevy UI choreography editor
├── aestra-bevy/             Reusable effect schema and Bevy playback plugin
├── aestra-viewer/           Viewer, frame capture, and contact-sheet binary
├── assets/effects/          Authored `.aestra.ron` choreography assets
└── crates/                  Reserved for extracted internal shared crates
```

The workspace deliberately has three top-level product modules. `aestra-bevy` owns the shared effect format and playback contract; both binaries consume it. Following the common large-Rust-workspace convention, `crates/` is reserved for smaller internal libraries if shared concerns are extracted later.

## Viewer and visual analysis

Open the bundled example:

```powershell
cargo run -p aestra-viewer
```

Open another effect:

```powershell
cargo run -p aestra-viewer -- --effect path/to/effect.aestra.ron
```

Capture evenly spaced frames plus a single AI-friendly contact sheet:

```powershell
cargo run -p aestra-viewer -- --capture captures/prism-bloom --frames 9
```

The capture directory receives numbered PNG frames, `contact-sheet.png`, and `capture-manifest.md`. In interactive mode, press `S` for a single screenshot.

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
            commands.spawn(EffectPlayer::new(effect));
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

The current runtime is a deterministic CPU reference using pooled Bevy sprites. It establishes authoring semantics and makes the plugin usable immediately. A future GPU backend can preserve the same `AestraPlugin` / `EffectPlayer` integration surface.

See [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) for the product architecture and phased roadmap.
