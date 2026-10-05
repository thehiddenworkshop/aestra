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

## Host lighting quality

Lighting remains opt-in: install `AestraTransientLightPlugin` for representative
pulses and `AestraParticleLightPlugin` for selected-particle realization. Choose
`ParticleLightMode` separately; quality never switches a host to positional readback.
An explicit policy works during setup or before the next update when changing quality:

```rust,no_run
use aestra_bevy::LightingQualityPolicy;
use bevy::prelude::*;
let mut app = App::new(); // Install your rendering/Aestra/lighting plugins separately.
let mut lighting = LightingQualityPolicy::preset("medium").unwrap();
lighting.particle.max_range = 80.0;
lighting.apply(app.world_mut()).unwrap();
// The host can keep representative flashes without selected-particle lighting.
lighting.particle.enabled = false;
lighting.apply(app.world_mut()).unwrap();
```

The example-host high/medium/low caps are 8/4/2 representative lights and
96/48/24 selected lights. These are configurable ceilings, not device-independent
performance guarantees. Both families have independent enabled/count/range/lumen
controls; shadows stay off. This is a global policy, not automatic per-effect distance LOD.
Applying a policy preserves byte, request and age/lag
budgets, authored binding mode, playback and history. Invalid policies change
nothing. GPU/async transport settings converge through ordinary pipelined updates,
not a synchronous GPU wait. No plugin or representative intent is created by applying
the policy, but a nonzero particle budget explicitly opts the selector into work for
authored particle-light outputs. External clients still need the pinned Bevy PBR
patch described in `vendor/bevy_pbr/AESTRA_PATCH.md` for the qualified native path.

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
must not be assumed to have this delivery guarantee.

Project particle cues now address the **root player**, with a stable `clip_path` and the
root player's epoch, not a temporary child entity or its private GPU epoch. `particle`
contains the source effect ID, inherited seed, mapped root time and optional world position.
`event.origin` identifies the source emitter; `event.tick` is still its source-local tick.
Reject stale epochs against the root player, even if the child presentation no longer exists.
FirstPerTick identity within a fixed source program is root + epoch + clip path + emitter +
output kind + tick; it is not an identity for every particle in an EachEvent batch.

For spatial sound use `output.particle.as_ref().and_then(|p| p.world_position)`, rather than
applying the current clip transform to `event.value` again. Authored ancestor/leaf motion
and clip placement are sampled at the event tick with full affine matrices (including
nonuniform scale/shear). The ECS root's `GlobalTransform` is sampled at **delivery time**:
arbitrarily moving host placement is not a recorded historical pose. Missing placement or
nonfinite position yields `None`, not `[0, 0, 0]`. Velocity is not supplied by this packet.
Source-offset preroll and a newly created child inside a seek are silent through their entry
boundary; existing children silence reconstruction through the new source time on seek.
Normal live advancement preserves the delivery epoch/boundary.

The native `f6-show --fireworks-cue-check <fresh.json>` checker exercises repeated and
transformed clips under a nonidentity static root placement, restart and an 18-second seek.
It binds all shell launches/breaks and selected secondary transitions to host sound names,
but plays no audio. Legacy child `finished`, homing and extension-stage notifications retain
their previous entity/payload semantics; they are not certified by this particle-route gate.

`FirstPerTick` coalesces one packet per route/tick: `event.value` is the representative
particle's **effect-local position** (three floats), `event.magnitude` is the matching source
particle count, and `event.tick` is the simulation tick. Choose spatial voices, delays,
pooling and mixing in the host. It is not one packet per child
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

## Representative transient scene lights (opt-in)

The engine-neutral `aestra_runtime::{PointLightPulse, TransientPointLight}` contract
describes a world position, root-clock occurrence time, normalized linear RGB, lumens,
range, radius and lifetime. Intensity and range curves sample normalized pulse age
(at most 32 finite, ordered keys). Effects can persist `point_lights` bindings to stable
`ParticleOutputRoute` IDs. Nothing in the core is named after fireworks.

For automatic authored lights, install `AestraTransientLightPlugin` after `AestraPlugin`
and configure `TransientLightSettings`. The host does not need to map output names,
colors or envelopes. For example, before compiling an authored effect:

```rust,no_run
use aestra_core::{EffectAsset, PointLightBinding, PointLightPulse};
fn author_light(effect: &mut EffectAsset) {
    let route = effect.particle_outputs[0].id; // a validated FirstPerTick route
    effect.point_lights.push(PointLightBinding::new(
        route, PointLightPulse::flash([1.0, 0.2, 0.1], 50_000.0, 30.0, 0.5),
    ));
}
```

`LightColorParameter` optionally samples a gradient parameter at a fixed normalized
age when emitted. Exposed parameters use root live values or the leaf clip's authored
overrides/defaults; non-exposed defaults lower to literal colors. Root values are read
at delivery, not snapshotted at the event tick. Nested sources resolve by stable clip
path even after their temporary presentation has expired. Invalid sampled colors fail
closed. Binding validation rejects missing routes, EachEvent aggregation, duplicate
bindings and ambiguous output/emitter pairs (the native packet has no route ID).
Source v4 gains an optional field; compiled artifacts are v6 and old versions must be
recompiled. Editor controls are a subsequent slice; use source/API authoring for now.

For a custom host mapping, disable automatic authored bindings:

```rust,no_run
use aestra_bevy::{
    AestraPlugin, AestraTransientLightPlugin, AestraLightOutput, AestraOutputEvent,
    AestraSet, PointLightPulse, TransientLightSettings,
};
use bevy::prelude::*;

fn bind_lights(mut cues: MessageReader<AestraOutputEvent>, mut lights: MessageWriter<AestraLightOutput>) {
    for cue in cues.read().filter(|cue| cue.event.kind == "burst") {
        if let Some(light) = AestraLightOutput::from_particle(
            cue, PointLightPulse::flash([1.0, 0.2, 0.1], 50_000.0, 30.0, 0.5),
        ) {
            lights.write(light);
        }
    }
}
let mut app = App::new();
app.add_plugins((DefaultPlugins, AestraPlugin, AestraTransientLightPlugin))
    .insert_resource(TransientLightSettings { authored_bindings: false, max_lights: 8, ..default() })
    .add_systems(Update, bind_lights.after(AestraSet::Playback).before(AestraSet::SceneOutputs));
```

Bind a selected **FirstPerTick** particle route, not every star/EachEvent output. The
conversion deliberately ignores particle count: one cue produces one representative
light. Legacy stage/choreography/finished messages lacking spatial metadata are not
converted. For other use cases hosts can submit the envelope themselves with root,
epoch and a stable `TransientLightKey`. Pulse identity includes clip path, output,
emitter and tick, scoped to root/epoch; do not reuse it for independent intents.

The adapter defaults to 16 pooled, shadowless point lights, 128 requests/frame,
1,000,000 lumens and 200 world-unit range. Host settings can disable lights or lower
these budgets live; portable ceilings are 64 lights and 1,024 requests/frame. Excess
new intents are dropped, not queued or allowed to evict a live pulse; read
`TransientLightStatistics` for accepted/duplicate/budget/invalid/stale/expired/disabled
counts and active/allocated/peak occupancy. Bound the producer's message volume too:
the adapter caps consumption, not allocations made by arbitrary host producers.
Automatic binding inspection shares the requests/frame scan cap; excess source packets
are discarded without a backlog. `source_packets_dropped` includes unbound cues, not
only lost lights; `binding_invalid` counts unresolved/malformed authored light cues.

Lights reuse entities, inherit the root's `RenderLayers`, and remain unparented at the
already-transformed world position (no double root transform). Shadows/contact shadows
are disabled. Decay uses **occurrence time**, so delayed readback does not replay peak
brightness. Pause freezes the pulse; seek/restart epoch changes, expiry, root despawn,
or disabling the adapter clear active intents. Future and already-expired submissions
are dropped; there is no future scheduler or light-history reconstruction on seek.
Host ECS motion after the event does not move that already-emitted pulse.

Viewer opt-in: `--fireworks-f0 --fireworks-f0-probe f6-show --backend gpu --transient-lights`.
The saved shell assets bind their representative particle routes to clip-overridden star
colors; the viewer only installs the adapter and 8/4/2 high/medium/low budgets.
Add `--history playback-only --fireworks-cue-check fresh-report.json` to validate
admission and cleanup over full playback, seek and restart. These lights affect ordinary
lit scene meshes; Aestra's current unlit particle smoke is **not** illuminated by them.
Editor light controls, lit smoke and measured production lighting budgets remain F7/F8 gates.

## Selected particle lights (F7E portable baseline)

This is separate from representative event flashes. Author a `particle_point_light`
scene output on a particle emitter, then explicitly enable a globally capped GPU
selection and its Bevy adapter:

```rust,no_run
use aestra_bevy::{AestraPlugin, AestraParticleLightPlugin, AestraParticleLightSettings,
    ParticleLightReadbackSettings, ParticleLightRealizationSettings};
use bevy::prelude::*;
let mut app = App::new();
app.add_plugins((DefaultPlugins, AestraPlugin, AestraParticleLightPlugin))
    .insert_resource(AestraParticleLightSettings { max_lights: 48, ..default() })
    .insert_resource(ParticleLightReadbackSettings { max_lights: 48, ..default() })
    .insert_resource(ParticleLightRealizationSettings { max_lumens: 100_000.0, max_range: 30.0 });
```

The selector defaults to **zero**, so adding the adapter alone enables no lights.
The realized prefix is bounded by the authored quality limits, the global selector
cap and the transport cap. There is no fixed emitter/output ceiling or entity per
source particle. Selector storage also has an independent host/device byte budget.
GPU simulation works with `PlaybackHistoryPolicy::PlaybackOnly`; replay is not required.

Transport defaults: up to 96 selected records, three reusable in-flight staging slots,
1 MiB total GPU staging, 1 MiB estimated manifest payload per snapshot, 100 ms maximum
age and eight main frames maximum lag. Only `48 * prefix_capacity + 16` bytes are copied;
no source particle buffer is mapped and no current-frame GPU wait occurs. The one-result
mailbox replaces superseded results and rejects out-of-order/invalidated callbacks.
Each in-flight snapshot, the mailbox and the main pool retain bounded source manifests;
referenced compiled artifacts are kept alive until those snapshots retire, not deep-copied.
Busy slots drain before resizing after a budget change; no new over-budget work is admitted.

The reusable, unparented `ParticleLightProxy` entities receive already-world-space
positions and their source presentation's `RenderLayers`. Lumens/range are independently
clamped (defaults 1,000,000 / 200), radius is clamped to range, and both shadow kinds
stay off. Source removal, backend/visibility/context changes, seed/revision/restart,
compiled-artifact replacement, configuration changes and result expiry clear held lights.
Until a new set arrives, the previous valid set may be held **only within the age/lag
limits**. This can cause visual lag on fast-moving sources; there is no extrapolation.
Set `AestraParticleLightSettings.max_lights = 0` to clear entities and remove selection,
copy and clustered scene-light work (idle coordinator systems remain installed).

`ParticleLightStatistics` reports active/allocated/peak counts, prefix truncation,
source rejection, expiry, accepted-update age/frame lag, pool CPU cost and readback
busy/failure/stale/overwrite counters. Transport latency is not final display latency.
GPU diagnostics expose `aestra::gpu::particle_light_copy`; the timestamp does not
include map completion or the later main-world/cluster update.

For measured authored fixtures, use the viewer's `--particle-light-bench
--particle-light-realization --headless-bench --gpu-bench report.json` with the
hero/volley/show probes, native GPU and playback-only. Add `--transient-lights` on
the full show to measure independent representative flashes alongside selected stars.
This baseline lights ordinary lit meshes, not current unlit Aestra particle/volume smoke.
Production finale certification and perceptual fast-star lag acceptance remain separate gates.
The [paced F7E2 probe](../../benchmarks/fireworks/particle-light-latency-2026-10-04.md)
measured about two frames of final-image lag at 60 Hz (about five metres at
150 m/s), failing its initial flagship registration budget. Use this portable
path for latency-tolerant workloads; it is not approved for nearby fast stars.

The [F7E3 same-frame proof](../../benchmarks/fireworks/particle-light-gpu-proof-2026-10-04.md)
passes the same registration gate using one reserved native clustered-light slot
on an unchanged `StandardMaterial` receiver, without selected-light position
readback. It is test-only, not a shipping API: one source/output, one camera,
default layers and GPU clustering. Bounded production integration and authored
hero/volley/show profiling remain the next gate. Render-world consumers can order
after `gpu::particle_lights::ParticleLightSelectionSet` and before camera rendering;
always use the current frame's selected records and matching manifest.

### Same-frame native GPU mode (F7E4A)

The canonical particle-light plugin now includes a bounded native adapter:

```rust,no_run
use aestra_bevy::*;
use bevy::prelude::*;

App::new()
    .add_plugins((DefaultPlugins, AestraPlugin, AestraParticleLightPlugin))
    .insert_resource(AestraParticleLightSettings { max_lights: 96, ..default() })
    .insert_resource(ParticleLightGpuSettings { max_lights: 96, ..default() })
    .insert_resource(ParticleLightMode::SameFrameGpu);
```

`PortableAsync` remains the default. `SameFrameGpu` automatically stops
selected-light readback and removes the portable proxy pool; readback settings
do not need to be rewritten. Changing mode/global cap reconciles the pools.
The GPU pool reserves at most `min(global cap, GPU adapter cap)` zero-lumen
shadowless entities, reused across frames rather than allocated per particle.
The pool is hidden when no possible GPU light source remains, without deleting
reusable slots. Visible CPU placeholders use a finite zero-lumen 1 mm range;
only GPU injection supplies an active light's authored range/position. Empty
selections with possible sources can still incur bounded pointlike cluster work.
Current-frame GPU selection writes their actual world-space light records before
Bevy GPU clustering. Normal `StandardMaterial` receivers and independent host
lights/representative flashes use the same native buffer without a custom shader.

`ParticleLightGpuSettings` bounds mapping/token buffers and qualified source
metadata. `max_buffer_bytes` includes the **logical** reserved Bevy light-record
cost, not the renderer's allocation granularity, camera-cluster/pipeline memory
or separately budgeted selection storage. It is not a total renderer-memory cap.
The shared `ParticleLightRealizationSettings` supplies intensity/range clamps.
`ParticleLightGpuStatistics::snapshot()` reports render dispatches, last sequence,
reserved slots, written-capacity bounds, invalid sources, bytes and typed failures.
It does not read back an exact active GPU light count. Main-world observations
may trail the render world under pipelining.

This first slice supports **default source/camera layers and native storage/GPU
clustering only**. Other configurations fail closed with diagnostics, never an
automatic delayed fallback. Pipeline warmup is reported as `PipelineLoading`.
Source epochs, seed, artifact identity, enabled output, nested clip context and
root/owner visibility are qualified at extraction; removed/invalid sources and
unused slots cannot retain yesterday's contribution. The reserved marker
`ParticleLightGpuSlot` is inspection-only; do not modify its components.

The [F7E4A native gate](../../benchmarks/fireworks/particle-light-gpu-adapter-2026-10-04.md)
passes fast-star registration, pool/lifecycle/budget and independent host-light
fixtures. The viewer now profiles authored hero/volley/show workloads with
`--particle-light-bench --particle-light-realization --particle-light-mode gpu`
(or `async`). `--particle-light-gpu-cap 0` disables only realization while
retaining selection and independent representative flashes, unlike the global
`--particle-light-cap 0`. Reports distinguish slot/capacity bounds from selected
counts, and retain preparation/selection/injection/clustering/render-window costs.
GPU benchmark hosts preallocate tiered native cluster lists: Bevy's defaults can
overflow/grow on the current show and briefly corrupt lighting. These initial
capacities can still grow; they are neither general host recommendations nor
part of Aestra's adapter byte budget. See the
[F7E4B1 workload study](../../benchmarks/fireworks/particle-light-gpu-workloads-2026-10-04.md).
The [F7E4B2A image gate](../../benchmarks/fireworks/particle-light-gpu-images-2026-10-04.md)
now passes matched authored high-tier hero/volley/show diffuse receiver images with
and without bloom, including disable restoration and endpoint controls. These are
paused forward samples, not paced registration or artistic approval. The
[F7E4B2B registration gate](../../benchmarks/fireworks/particle-light-gpu-registration-2026-10-05.md)
passes a separate 25/75/150 m/s calibration tracer under high-tier authored show
overlap, with perspective/HDR/bloom and deliberately delayed negative controls.
Same-image centroids establish spatial registration, not display latency or natural
authored-star art approval; representative flashes are disabled for color isolation.
The earlier control-phase cadence miss remains recorded, not a universal 60 Hz claim.
The [F7E4B3A repeated-cost gate](../../benchmarks/fireworks/particle-light-gpu-costs-2026-10-05.md)
passes three high-tier authored on/off pairs per hero/volley/show. It observes public native
physical index/offsets-counts buffers and asynchronous index demand without new GPU readbacks.
The benchmark-only adaptive-grid target now matches its initial index capacity on both sides;
the earlier grid oscillation and a separate abnormal native shutdown exit remain recorded.
Stable public sizes are not total resident-memory or allocation-churn certification: private
Z-slice/scratchpad/staging and old in-flight allocations remain unmeasured. The
[F7E4B3B1 retirement gate](../../benchmarks/fireworks/particle-light-gpu-retirement-2026-10-05.md)
now passes reusable-pool idle/hidden/removal, finite empty/rejected fallbacks and
far-origin registration. All three repeated high-tier show cleanup windows match
the control at one acknowledged index (previously 346176), without changing the
8413-child workload. This retires idle work, not native buffer high-water allocations
or exact per-slot GPU liveness. Private/in-flight native budgets,
medium/low repeated costs, non-default per-view layers, additional hardware and
production-finale certification remain open.

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
