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

## Live playback or replay history

Game hosts that only advance effects can opt out of automatic checkpoint work:

```rust
use aestra_bevy::{EffectPlayer, PlaybackHistoryPolicy};

let player = EffectPlayer::from_compiled(compiled)
    .with_history_policy(PlaybackHistoryPolicy::PlaybackOnly);
```

`ReplayEnabled` is the compatibility default. Change a running player with
`player.set_history_policy(...)` and inspect it with `player.history_policy()`.
The policy follows nested effects and survives hot replacement. It is independent
of the asset's looping mode, seek quality, and authored particle events.

`PlaybackOnly` skips automatic CPU/GPU checkpoint snapshots, not live particles,
trail history, or extension simulation. Switching to it releases CPU scrub caches
immediately and GPU caches at the next render preparation, without resetting live
state. Switching back resumes future GPU captures; CPU scrub caching still needs
`enable_scrub_cache(budget)`, which also opts into `ReplayEnabled`.

Explicit backward seeks remain available, but reconstruct from zero without a
cache. Neither policy records historical live host inputs: exact reconstruction
of moving targets, bindings, or external events needs a host-supplied trace.
For hosts orchestrating the renderer directly, `PresentedEffect` has the same
builder/getter/setter; engine-neutral `EffectInstance` and `StageTimeline` also
expose the policy. No setting is written into the authored effect.

Compare the viewer's live GPU work using `--history playback-only` or
`--history replay-enabled` with `--gpu-bench output.json`. The report records the
policy and the checkpoint bytes copied in each matched simulation frame.

## Photographic preview profile

Hosts can opt an effect camera into the same fixed profile used by the editor and viewer:

```rust
use aestra_bevy::preview::PhotographicPreview;
use bevy::prelude::*;

fn camera(mut commands: Commands) {
    let mut camera = commands.spawn((Camera3d::default(), Transform::from_xyz(0.0, 20.0, 80.0)
        .looking_at(Vec3::new(0.0, 20.0, 0.0), Vec3::Y)));
    PhotographicPreview::default().apply(&mut camera);
}
```

The serializable profile contains fixed relative exposure stops (-8 to 8), `DisplayTransform`
(Tony, ACES or Reinhard), and natural bloom strength (0 to 1; 0 removes bloom). It normalizes
non-finite/out-of-range host values before applying. HDR is an intermediate render target,
not HDR monitor output. Bloom runs before scene-wide display exposure; no auto exposure,
simulation mutation or material rewrite is involved. The host still chooses which camera
receives the profile: do not apply it to UI/gizmo cameras. Applying it replaces that camera's
grading, tonemapping and bloom settings. `restore_legacy_3d_response` restores Bevy's legacy
3D defaults; hosts with a custom baseline should restore their own camera snapshot instead.
When HDR and LDR cameras share a window, their intermediate textures are separate. Later UI/gizmo
cameras should clear their source to `Color::NONE` and alpha-composite their output with no output
clear (`CameraOutputMode::Write`, `BlendState::ALPHA_BLENDING`, `ClearColorConfig::None`), so an opaque
or stale LDR source does not hide the tonemapped effect. Their grading remains independent.

## Fireworks radiance controls

The editable shells in `assets/test/effects/fireworks_*.aestra.ron` expose independent
`Star radiance` and `Trail radiance` scalar parameters, using ordinary material effect bindings:

```rust
use aestra_bevy::Value;

let radiance = source.parameters.iter().find(|p| p.name == "Star radiance").unwrap().id;
player.set_parameter(radiance, Value::Scalar(8.0))?;
// player.clear_parameter(radiance)?; restores the authored default.
```

Choose variations before playback for game workloads; existing parameter-edit/history
invalidation semantics still apply. These are artistic linear RGB gains, not physical watts:
defaults are 8 for sprite stars/launch/flash and 4 for trails, with a graph-level clamp to 0..64.
Alpha, smoke, cooling gradients and simulation inputs are independent. Unit gain restores the
old material response. HDR preserves values above 1 until the display transform; start with
the photographic profile's default 0 stops. No new shell-specific runtime setter is required.
Low-level material hosts can now also import `MaterialBindingContext` from `aestra_bevy`
alongside `MaterialRuntimeBinding` to resolve the same dynamic effect bindings.

## Subpixel additive sprites

For distant luminous stars, hosts can opt into a native GPU presentation policy:

```rust
use aestra_bevy::SpriteSampling;

app.insert_resource(SpriteSampling { minimum_pixels: 2.0 });
```

The default is 0 (disabled). The resource normalizes to 0..8 physical main-pass pixels;
non-finite inputs disable it. It measures the shorter rotated quad axis using each camera's
unjittered projection and viewport, expands undersampled quads uniformly, then attenuates
final alpha by inverse expanded area. This preserves continuous footprint energy without
changing authored opacity inputs, radiance, particle size/state, event counts or replay history.
Resource edits update renderer inputs without restarting simulation. It does not enable HDR.

Only native GPU **additive Sprite** draws qualify. Flipbooks, meshes, ribbons, trails and CPU
reference/readback presentation are unchanged. Wireframe remains a diagnostic outline rather
than energy-attenuated shading. The floor is a host quality policy, not an asset/particle limit.
Qualifying draws bypass CPU frustum culling because view-dependent expansion can exceed their
world AABB; normal raster clipping remains. Disabling the resource restores ordinary sprite
culling. Budget the extra offscreen submissions and fill cost before enabling it for a finale.

This is not an analytic pixel integral or a guarantee of shimmer-free arbitrary textured masks.
Projection is exact for ordinary camera-facing perspective/orthographic quads at constant
view depth; custom projections use a center-Jacobian estimate. Zero/degenerate footprints are
not resurrected. Start at 2 pixels and compare temporal appearance and GPU cost on target hardware.

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
