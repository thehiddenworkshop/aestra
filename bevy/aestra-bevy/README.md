# aestra-bevy

Bevy integration and **reference playback runtime** for compiled Aestra effects.

This is the canonical client for Aestra. If you are embedding Aestra in an
application — or writing an integration for another engine, such as
`aestra-godot` or `aestra-unity` — **start here**. The ECS wiring differs per
host, but the pipeline is the same everywhere.

## The client pipeline

1. Load an authored `EffectAsset`.
2. Resolve it against a project asset root (child clips + material programs) with
   `aestra_project::ProjectAssetIndex`.
3. Compile the resolved project into a `CompiledEffectProject` with
   `EffectCompiler` — offline, in a shipping game.
4. Spawn an `EffectPlayer` for the artifact and add `AestraPlugin`. The plugin
   owns simulation advance, choreography dispatch, and rendering; hosts never
   touch the render backend (`aestra-bevy-render`) directly.

Runtime control — play/pause, `seek`, `set_parameter`, choreography events —
all goes through the `EffectPlayer` handle.

## Where to look

| I want… | Read |
|---|---|
| The minimal load-and-play host | [`examples/minimal_player.rs`](examples/minimal_player.rs) |
| The public API surface + rationale | crate docs (`cargo doc -p aestra-bevy --open`) |
| A demanding real-world host | [`apps/aestra-viewer`](../../apps/aestra-viewer) (adds capture, diagnostics, GPU bench) |

## Run the example

```sh
cargo run -p aestra-bevy --example minimal_player
```

Controls: `Space` play/pause · `R` restart · `←`/`→` step one frame.

## Layering

`aestra-bevy` is the batteries-included client plugin. It sits on top of
`aestra-bevy-render` (the rendering backend: GPU compute pipelines, materials,
statistics), which it re-exports. The two are kept separate on purpose — the
editor consumes the render backend directly and does its own orchestration,
while this crate is the supported surface for playback hosts. See
[`docs/new/aestra_client_runtime_unification_plan.md`](../../docs/new/aestra_client_runtime_unification_plan.md).
