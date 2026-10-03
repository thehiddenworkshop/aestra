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

Analytic `EffectPlayer` playback uses its fixed clock as the sole time authority:
ordinary forward ticks do not invalidate trail history, including across continuous
loops. Restart loops, explicit seeks/restarts and history-affecting edits still reset
or reconstruct it. Engine-neutral integrations can use
`EffectInstance::advance_clock_with_choreography_events(previous_clock, current_clock, output)`
with consecutive forward clock snapshots. Do not integrate a floating-point delta
and then correct the instance to a slightly lower exact frame time: that correction
is deliberately treated as a backward discontinuity by `set_playback_time`.
External repositioning must use the seek contract, not the forward-advance API.

## Explicit particle quality budgets

Hosts select the compile-time tier with
`EffectCompiler::default().with_tier(QualityTier::medium())`, including when using
`compile_resolved_project`. Each effect/dependency selects its own optional
`EffectAsset::particle_budgets["medium"]` profile. `ParticleBudgetProfile` maps stable
emitter IDs to exact capacities, event-link IDs to exact counts, and trail-renderer
IDs to exact owner capacities. Omitted targets, or a missing tier profile, keep
authored values. High always remains authored. Existing assets need no migration.

Ordinary event fan-out is **not** automatically multiplied by `QualityTier.particles`.
Author demand and live/history capacities together: reducing a child pool alone can
reject births, and retired trail owners still need storage. Profiles are validated
against authored maxima and applied to a copy before lowering and resource checks.
They cannot waive platform limits. Emission-module rates/bursts, live host input
counts, lifetimes, velocities, material bindings and output routes are unchanged.
Recompile/replace through the normal host workflow to change tiers; no live tier switch.

The four F5 shell assets plus `fireworks_secondary_volley.aestra.ron` include medium/low
profiles. Lower tiers keep every crossette arm, crackle's carrier delay and strobe
timing while reducing density. These are bounded examples, not certified finale LODs.
Profiles can be edited in RON or through the transactional `SetParticleBudgets`
authoring command; deletion prunes dangling targets and undo restores them. A dedicated
editor profile UI and automatic budget authoring for duplicated emitters are not provided.
Lowering authored maxima below a profile requires adjusting the profile in the same
transaction. Host-driven bursts remain the host's admission responsibility.

## Particle-driven host cues

Author ordinary `EventDefinition` outputs and `ParticleOutputRoute` routes (`OnSpawn`,
`OnDeath`, `OnCollision`), then consume `MessageReader<AestraOutputEvent>` every update.
The saved `fireworks_multi_break.aestra.ron` fixture exports `launch`, `main_break`,
and `secondary_break`; they come from actual particle transitions, not audio timers.
The native host consumer/check is `apps/aestra-viewer/src/fireworks_cues.rs`.

`fireworks_crackle.aestra.ron` adds a burning-carrier delay before short spark clusters:
96 main-star deaths → 96 carriers → 12 sparks per carrier death. Its `crackle` route observes
carrier `OnDeath`; the same viewer host check validates all 96 pops, seeking and restart.
Event-routed births do not themselves emit `OnSpawn` under the current simulation contract.
Use link admission telemetry or subsequent actual transitions for those particles; do not
assume an `OnSpawn` observer sees every event-created child. This is a bounded high-tier fixture,
not finale-scale, acoustic or artistic certification.

`fireworks_crossette.aestra.ron` uses four ordinary death links, each spawning one child
per main-star death. Four constant XY diagonal directions guarantee a four-arm cross
around the inherited moving center, rather than four random samples. Thirty-two parents
produce 128 arms; all arms share the parent position and 30% inherited velocity. The plane
is effect-local and follows effect placement, not each parent's heading. `crossette_split`
observes main-star `OnDeath` once per split, not four independent sound routes; the viewer's
`f5-crossette` probe and host check validate admission, resume and restart. No new runtime
trigger or firework-specific API is needed for this bounded fixed-plane variant.

Native GPU particle-route and timeline-cue messages carry `playback_epoch: Some(epoch)`.
For a root player, reject queued messages whose epoch no longer equals
`player.instance().history_epoch()`. A seek silences particle reconstruction through its
destination; forward live ticks resume delivery. Restart starts a fresh epoch and permits
new launch/break cues. Checkpoint restore restores simulation, not an old delivery identity.
Old/out-of-order GPU ring readbacks cannot replay already-delivered route ticks. Legacy
homing/finished/stage outputs remain `None`: they are **unqualified**, not epoch zero, and
must not be assumed to have this delivery guarantee. Nested spatial-audio routing is not
certified by the root fixture check.

`FirstPerTick` coalesces one packet per route/tick: `event.value` is the representative
particle's **effect-local position** (three floats), `event.magnitude` is the matching source
particle count, and `event.tick` is the simulation tick. Apply host placement before spatial
audio; choose voices, delays, pooling and mixing in the host. It is not one packet per child
spawn or one voice per spark. `EachEvent` has different packet/count semantics. GPU messages
arrive asynchronously; ticks identify occurrence time, not arrival-frame time.

The route ring holds **32 simulation ticks**, with at most 16 stored positions per tick for
`EachEvent`. Readback stalls can lose older observations; coalescing does not make this a
lossless gameplay-authority bus. Keep consumers and host voice budgets bounded. Sound files,
playback and scene acoustics are never owned by Aestra.

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
world AABB. Analytic, non-displaced sprites now receive a conservative per-view queue check
with a physical-pixel margin, so fully offscreen effects can skip drawing without clipping
expanded edge footprints. Bounds refresh from current particle inputs and the propagated host
transform; camera data is prepared once per view. Stateful/event histories, displaced materials,
jittered/custom projections and unsupported transforms retain the safe raster-clipping fallback.
Disabling the resource restores ordinary sprite culling. This skips draw work, not simulation:
budget visible overlap/fill and fallback submissions before enabling it for a finale.

This is not an analytic pixel integral or a guarantee of shimmer-free arbitrary textured masks.
Projection is exact for ordinary camera-facing perspective/orthographic quads at constant
view depth; custom projections use a center-Jacobian estimate. Zero/degenerate footprints are
not resurrected. Start at 2 pixels and compare temporal appearance and GPU cost on target hardware.

## Subpixel additive trails

```rust
use aestra_bevy::TrailRasterSampling;
app.insert_resource(TrailRasterSampling { minimum_pixels: 2.0 });
```

This independent policy defaults to 0, normalizes finite inputs to 0..8 physical main-pass
pixels and affects only native GPU **additive Trail** presentation. It measures each point's
faded width through the camera's unjittered projection, widening only undersampled geometry.
Bodies attenuate final alpha by inverse width; round caps attenuate by inverse area. Authored
widths, radiance, particle state, UVs, history sampling/LOD, pool sizes and replay are unchanged.
Live resource edits update presentation without restarting the effect. Ribbons, non-additive
trails and CPU reference/readback paths are excluded. Zero/expired widths stay invisible.

Pixel expansion exceeds ordinary world bounds, so enabled trails retain conservative CPU
visibility. Per-view GPU culling pads current whole-history bounds for the physical main-pass
resolution on standard rigid perspective/orthographic cameras, including round caps. Unknown
or stale bounds, unsupported camera transforms/projections and eye-plane-crossing histories
fail open; jittered views and custom vertex displacement retain the conservative fallback.
Culling suppresses offscreen raster work, not simulation or history recording. Visible fill
still needs budgeting. The procedural-mask raster tests are not a guarantee of shimmer-free
arbitrary textures or strongly tapering/depth-varying joins. Wireframe remains a diagnostic
outline, not energy-attenuated shading.

Viewer comparison: `--trail-min-pixels 0|2|4` (native `auto`/`gpu` only). Reports record
`trail_minimum_pixels` separately from the sprite floor and photographic response.

For concentrated fill and offscreen submission comparisons, the viewer has trail-only probes
`f4-trail-fill` / `f4-trail-offscreen`: 8,192 live parents and owner slots, eight-point pools,
flat caps, 0.08-second history, quarter-pixel head width, and a moving 16-pixel seed patch at
the reference 960×540 viewport. They are raster stress fixtures, not authored shells/finale tiers.

```sh
cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe f4-trail-fill --camera wide --semantic-materials --backend gpu --history playback-only --hdr --trail-min-pixels 2 --gpu-bench target/fireworks-f4/trail-fill-2.json
```

Repeat with floors 0/2/4 and both probes. Raster **benchmarks only** advance simulation by a
fixed 1/60 second per viewer frame, keeping work comparable even when rendering slows; normal
playback and other live-throughput benchmarks keep their original clocks. Reports record this
step, nominal calibration, observed physical viewport/backend, minimum/peak observed occupancy,
evictions/truncation and fresh diagnostic sample counts. Missing timings remain unavailable,
not measured zero. Asynchronous counts are observations, not frame-aligned certificates; check
them and requested-time ranges before comparing timings. Do not sum pass percentiles into a
whole-frame budget or infer real-time catch-up throughput from the fixed-step probes.

## Per-particle material phase

`MaterialInput::ParticleRandom` is a stable native presentation value: a hash of the folded
render seed, emitter index and spawn ordinal, never physical storage or compacted draw index.
It is independent of simulation random channels. Sprite/mesh use their particle identity;
trails use their owner's identity across the history, ribbons use the segment's first endpoint
(flat interpolation). No particle-buffer ABI or bind-group change is required. Unused reads
are removed from the material varying interface. Changing the effect seed changes phases;
slot reuse receives the new spawn ordinal rather than inheriting a previous particle's phase.

The editable `assets/test/effects/fireworks_strobe.aestra.ron` uses a generic custom WESL
Periodic Gate with `ParticleNormalizedAge * cycles_per_life + ParticleRandom`. It gates both
RGB and alpha to zero off-duty; effect-bound radiance/cycles/duty are ordinary live material
values. Three-second life, 18 cycles and 0.18 duty give 6 Hz. Lifetime edits change frequency;
there is no mutable host timer or flash sound route. Load through the normal resolved-project
compiler so its material-function library is expanded before runtime binding. The renderer
accepts these compiler-expanded custom calls without needing the authoring library again.
Audio assets, per-flash sound policies, voice budgets and mixing remain host responsibilities.

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
