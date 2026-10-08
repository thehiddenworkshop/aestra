# Bevy fireworks showcase

`bevy/aestra-bevy/examples/fireworks.rs` is the public integration host. It loads the
same saved effects and materials as the editor, not an implementation imported from
`aestra-viewer`. The viewer remains the capture, regression and benchmark harness.

```powershell
cargo run --release --locked -p aestra-bevy --example fireworks
cargo run --release --locked -p aestra-bevy --example fireworks -- --tier medium
cargo run --release --locked -p aestra-bevy --example fireworks -- --history replay-enabled
```

The default is native GPU presentation, high quality, fixed HDR exposure and bloom,
and **playback-only** history. The 26-second, play-once show uses 13 clips referencing
four reusable shell effects: multi-break, crackle, crossette and strobe. Restart with R.
High/medium/low select authored particle/event/trail budgets at compilation and separate
host representative-light ceilings of 8/4/2. These are policies, not hardware-independent
performance promises. Native additive sprites use a two-pixel minimum footprint and
trails a one-pixel minimum width, with the runtime's energy attenuation; this is
presentation sampling, not changed particle counts or trail history. The HUD reports the active backend and smoothed wall-frame duration
(including vsync), not isolated GPU timings. Use viewer/bench tooling for measured costs.

Controls: Space pauses/resumes; R restarts; 1/2/3 choose close/audience/wide cameras;
L toggles authored burst lights; M mutes sound; Esc exits. Shader preparation holds
the initial player at time zero; preparation errors/timeouts exit instead of silently
consuming the beginning of the show. Representative flash lights use the saved
`point_lights` bindings and the public `AestraTransientLightPlugin`, with shadows off.
The environment is deliberately minimal: dark background, ground and two dark
PBR receiver structures (non-emissive, lit by moon/burst lights).

## Editor to host

1. In the editor, use File → Open Project and select `assets/test`.
2. Open `effects/fireworks_show.aestra.ron` to edit clips, placement, timing and overrides.
3. Open its four shared shell effects or referenced material programs to edit their appearance.
4. Save, then restart the example. It reads saved source files at launch, not stale embedded text.

For a separate project, pass its **asset root**, not necessarily its repository root:

```powershell
cargo run --release --locked -p aestra-bevy --example fireworks -- --project C:/MyShow/assets --effect effects/my_show.aestra.ron
```

`fireworks/project.rs` shows load → resolve → compile → `EffectPlayer::from_project`.
`--effect` is relative to that asset root, or absolute. Dependencies/materials resolve
through `ProjectAssetIndex`. Shipping hosts can compile offline and load an artifact.
No per-particle CPU polling, effect duplication or direct render-backend ownership is required.
The original procedural three-emitter teaching demo is retained as `fireworks_event_chain`.

## Optional host sound

Audio is outside the portable VFX runtime. Enable the example's WAV decoder explicitly:

```powershell
cargo run --release --locked -p aestra-bevy --example fireworks --features fireworks-audio
cargo run --release --locked -p aestra-bevy --example fireworks --features fireworks-audio -- --audio-root C:/MySounds/fireworks
```

The default local folder is `bevy/aestra-bevy/assets/audio/fireworks`. Expected files:
`Firework_Large_01.wav` through `05`, likewise `Medium` and `Small`. Missing groups are
reported and remain silent; `--no-audio` bypasses loading. The local pack's existing
`assets/CREDITS` names Joel Summers. Those user-provided files are not added or modified
by this implementation. Confirm redistribution rights before shipping/bundling a pack;
the repository's source-code license does not grant rights to third-party recordings.

`fireworks/audio.rs` consumes actual `AestraOutputEvent` particle cues:

| Cue | One-shot group |
| --- | --- |
| `main_break` | Large |
| `secondary_break` | Medium |
| `crackle`, `crossette_split` | Small |
| `launch` | Unbound; this pack has no separate launch recording |

This is an illustrative binding, not a final sound mix. Spatial placement uses the
event's world position, with camera listener and approximate 343m/s propagation delay
corrected for GPU delivery latency. Variant selection uses source seed/tick. Epoch,
clip path, kind, emitter and tick qualify deduplication; event magnitude never becomes
one voice per star. Limits are 256 packets/update, 64 pending cues, 24 simultaneous
voice entities and 4096 seen identities/epoch. Saturation drops new requests. Incoming
cues more than one second late or over eight seconds from arrival are ignored. Pending
cues and active voices are cleared on restart/seek/root removal; pause freezes sound,
and the play-once ending allows audible tails. Unloaded/failed files are not played late;
voice entities time out after ten unpaused seconds even without an audio device. Bevy
spatial audio is simple panning, not HRTF/occlusion/acoustic simulation.

## Verification and remaining work

```powershell
cargo test --locked -p aestra-bevy --example fireworks --features fireworks-audio
cargo clippy --locked -p aestra-bevy --example fireworks --example fireworks_event_chain --features fireworks-audio -- -D warnings
```

GPU-free tests resolve all three tiers, shared materials and automatic renderer bindings,
the history API, event routing/positions/delay, stale/duplicate rejection, bounded queues
and voice admission/mute/restart/root cleanup. The existing `AESTRA_EXAMPLE_CAPTURE`
environment hook captures a native window screenshot and exits after image delivery;
`AESTRA_EXAMPLE_CAPTURE_SECONDS` chooses simulation time (default 5, clamped to show end).

Verified on 2026-10-05 (Windows, RTX 4070 SUPER, Vulkan, development build): eight
audio-enabled GPU-free tests, four without the feature, and strict scoped Clippy passed.
Native high/playback-only and low/replay-enabled three-second captures render the saved
show. Two full high/playback-only 26-second audio runs admitted all 13 authored flashes
(peak three, zero invalid/stale bindings) and 134 sound voices, dropping 89 requests;
the final run observed 24 actual Bevy spatial sinks at peak and no queued sounds at end.
This verifies decoding/binding/cleanup, not subjective sound quality. Raw images live in
`target/fireworks-bevy-showcase/`. Short-run shutdown can log Bevy readback-channel-closed
warnings; captures completed and processes exited successfully. An initial warm-up run
incorrectly treated a retryable unloaded shader as fatal; the corrected retry policy has
a regression test and all subsequent native runs passed.

## Smoke lighting qualification fixture (F8.3B)

```powershell
cargo run --release --locked -p aestra-bevy --example fireworks -- --smoke-lighting --no-audio
```

This opt-in mode plays `assets/test/effects/fireworks_smoke_lighting.aestra.ron`, not the
full show. It has a pre-existing fluid-smoke source and two overlapping, material-free
star bursts. Saved representative bindings receive real native particle output packets;
saved selected-star outputs use `AestraParticleLightPlugin` with `ParticleLightMode::SameFrameGpu`.
The host has two representative slots and only 8/4/2 selected slots for high/medium/low.
No source-particle enumeration, selected-position readback or viewer fixture injection is used.
Root scale/camera are lab-specific; the full show's presentation is unchanged.

Space pauses, R restarts, L toggles both light families and Esc exits. Enabling flashes
after their birth does not replay old output packets: restart for fresh flashes. Native
capacity statistics are bounds, not active selected-light counts. There is no sprite rendering,
bloom or host scene lighting to counterfeit smoke response; this is a diagnostic fixture,
not finished fireworks art. Existing fluid-source smoke is **not** generic particle-to-fluid injection.

Two repeated high/medium/low native runs pass before-birth, independent light-family on/off,
one-slot host-budget restoration, expiry, restart/rebinding and root retirement controls.
All 48 corresponding PNGs match exactly. Matched frozen-pass GPU diagnostics are recorded,
but the small workload is clock/noise-sensitive and does not certify full-show overhead.
See [`output-smoke-lighting-2026-10-07.md`](../../benchmarks/fireworks/output-smoke-lighting-2026-10-07.md)
for evidence and the visit-budget constraint found during qualification.

## Lit particle-smoke fixture (F8.3C)

```powershell
cargo run --release --locked -p aestra-bevy --example fireworks -- --particle-smoke-lighting --no-audio
```

This mode loads `effects/fireworks_particle_smoke_lighting.aestra.ron`: one isolated
alpha-blended smoke puff and the same two real burst/selected-light families, without
a fluid extension, bloom, synthetic host lights or selected-position readback. Controls
and host light caps match the fluid lab. The two lab options are mutually exclusive.

`materials/fireworks_lit_smoke.aestra.material.ron` opts into the portable
`ScenePointIrradiance` Vec3 fragment input. Its graph combines gray albedo, low ambient
light and scene irradiance, with an instance-editable Scene light gain. Coverage remains
particle opacity × radial mask. The native Bevy 3D adapter evaluates unshadowed isotropic
point-light scattering at the fragment's world position using the existing clustered
lights. It visits at most 32 entries per fragment; it does not select the strongest lights.
Unsupported providers and the 2D path return zero scene irradiance, not fabricated lighting.
Existing unlit shaders/materials retain their behavior and resource layout.

Repeated native high/medium/low runs pass independent light-family, live gain, budget,
rigid parent-transform, expiry, restart/rebinding and owner-removal controls. The 51
corresponding original captures match exactly. This is an isolated-puff material/API
qualification, **not dense smoke or AAA art acceptance**: the initial 48-overlapping-puff
attempt failed image repeatability, consistent with unsorted alpha draw order. That failure
is retained; the bounded per-draw ordering fix is qualified separately below. Reusable smoke
art/persistence and full-show costs remain open. See
[`particle-smoke-lighting-2026-10-07.md`](../../benchmarks/fireworks/particle-smoke-lighting-2026-10-07.md).

## Overlapping alpha smoke (F8.3D)

```powershell
cargo run --release --locked -p aestra-bevy --example fireworks -- --particle-smoke-lighting --effect effects/fireworks_particle_smoke_overlap.aestra.ron --no-audio
```

The particle-smoke lab now opts into `TransparentOrderMode::DepthBackToFront`. Hosts can
set `AestraSettings.transparent_order` to this mode; the viewer exposes `--depth-transparency`.
`Fast` remains the default for normal shows and additive rendering. The old bounded,
ordinal-only `StableCapture` mode is unchanged.

The new native Camera3d path sorts each visible alpha Sprite/Flipbook draw back-to-front
on the GPU, with birth-ordinal and slot tie-breaks. Separate view-local permutations leave
simulation/alive buffers untouched. There is no particle/index readback or 4,096-particle
capture ceiling. It does **not** interleave particles belonging to different draw calls,
sort meshes/ribbons/trails, or provide a 2D transparency solution.

The saved 48-puff overlap fixture passes unchanged lighting/restoration gates at all three
tiers. GPU permutation tests cover zero/partial/full pools through 65,537 slots and opposite
camera directions. Buffers are reused while capacity is unchanged and ownership is retired
on mode-off/owner removal. `GpuAlphaSortStatistics::snapshot()` exposes CPU ownership/work
metadata, not GPU particle positions or driver VRAM accounting.

Work and scratch storage scale with **capacity**, per visible view/draw pair, even for a sparse
pool: two padded key arrays plus an index array (36 bytes per power-of-two slot), with small
per-stage/vertex parameter buffers. Binary-rank merges use O(N log² N) comparisons. The
48-puff result is a correctness gate, not large-pool/live-show performance or AAA smoke-art
acceptance. See
[`alpha-smoke-ordering-2026-10-08.md`](../../benchmarks/fireworks/alpha-smoke-ordering-2026-10-08.md).

## Accumulating particle-smoke prototype (F8.1A)

```powershell
cargo run --release --locked -p aestra-bevy --example fireworks -- --smoke-persistence --no-audio
```

This opt-in mode loads `effects/fireworks_smoke_persistence.aestra.ron`, the reusable
`fireworks_persistent_smoke.aestra.material.ron` and `smoke_billow.aestra.material-function.ron`.
Three seeded noise octaves give each rotated puff a mottled density and irregular soft edge;
particle normalized age evolves the pattern. There are no generated bitmap dependencies.
Scene light gain, density and ambient are material parameters. The exposed Drift acceleration
effect parameter drives the existing Motion module: a fixed acceleration approximation, **not
physical wind advection**, fluid injection or a volume renderer.

One 96-slot smoke pool emits eight initial puffs plus 20/s for four seconds (88 births), with
12–14-second lives, expansion, slow rotation and fade. Smoke remains after emission ends, then
drains by 18 seconds. Real red/blue break outputs at 8/10 seconds illuminate the accumulated
cloud through the existing representative/selected paths. Only smoke contributes sprite pixels.
All tiers keep this same smoke workload; selected-light caps alone are 8/4/2. Playback-only and
per-draw depth sorting remain the defaults for this lab. Other lab options and the normal show
keep their own saved assets unchanged.

The native gate separately controls density, light families, reversible flash-intensity mute,
fade/drain and owner cleanup. Disabling representative lights (`enabled=false`, including L)
retires existing one-shot pulses; enabling admits future pulses, it does not resurrect past ones.
Use `max_lumens=0` and restore its previous value for a reversible representative-light mute.
The prototype is not a photo-approved art reference, show migration or large-pool cost gate.
See [`smoke-persistence-2026-10-08.md`](../../benchmarks/fireworks/smoke-persistence-2026-10-08.md).

## Shell-born smoke cohorts (F8.1B)

```powershell
cargo run --release --locked -p aestra-bevy --example fireworks -- --smoke-cohorts --no-audio
```

This opt-in mode loads `effects/fireworks_smoke_cohorts.aestra.ron`. Two ballistic rockets
break at 2/4 seconds. Real GPU `OnDeath` links spawn 32 smoke puffs and 32 light-bearing
stars per rocket; smoke and stars have no independent emission. Both smoke cohorts share
one 96-slot pool and the F8.1A lit billow material. Smoke persists, overlaps and drains by
18 seconds. Only smoke contributes visible sprite pixels; the launch emitters have
schema-required zero-opacity markers, and stars have no renderers.

Representative flashes now bind accepted **child-star `OnSpawn`** outputs (F8.1B2), while
selected moving lights follow those stars. GPU event/domain/input births append a bounded,
deterministic prefix of accepted children to the existing output capture; rejected births
produce no cue, replay stays silent and restart permits fresh cues. The unchanged 1,024-record
per-emitter/tick capture and bounded asynchronous host ring are not a lossless gameplay bus.
The earlier failed child-output attempt and parent-`OnDeath` qualification remain in the
evidence. See [`child-birth-outputs-2026-10-08.md`](../../benchmarks/fireworks/child-birth-outputs-2026-10-08.md).

All tiers preserve the same smoke density; only selected-light caps change (8/4/2).
The native gate checks exact asynchronous link admission/drop totals, no smoke before the
first break, independent older/younger cohorts, and illumination of older smoke by the later
break with younger smoke removed. Density, independent light families, frozen restoration,
drain and owner cleanup have separate controls. This is a bounded correctness qualification,
not photo-approved smoke art, cross-draw transparency or live/full-show performance.
See [`smoke-cohorts-2026-10-08.md`](../../benchmarks/fireworks/smoke-cohorts-2026-10-08.md).

## Live smoke cost qualification (F8.1C1)

The viewer's explicit native test measures the same saved cohort asset during forward
playback, not paused screenshots. It compares one pool (64 smoke particles) and four
coincident roots (256 smoke particles) against test-only no-smoke-draw controls, keeping
births, simulation, output routes, capacities and lighting identical. The four-root case is
a synthetic cost stress, **not authored choreography or cross-draw sorting acceptance**.
The saved 26-second show's existing unlit smoke is measured separately, without silently
replacing its assets. No viewer instrumentation is imported into this public host.

Measurements use playback-only history, fixed 60 Hz, a 960×540 LDR target, async GPU
diagnostic deduplication and separate per-source simulation timings. They exclude this
example's HDR environment/audio and are not production-finale, whole-game or frame-paired
incremental-cost proof. The suite checks real particle populations/admission, stable sort
allocation and owner retirement; pass percentiles must not be summed.

Live sampling also exposes a cleanup gap: stateful float accumulation can stop short of
the last fixed tick at the `Once` boundary, leaving a fully faded smoke particle alive.
The report retains the actual final counts; no seek hides the gap. Fixing clock agreement
and proving final-tick cleanup precede show migration. See
[`smoke-live-costs-2026-10-08.md`](../../benchmarks/fireworks/smoke-live-costs-2026-10-08.md)
for the reproducible native command, costs and remaining acceptance gates.

## Normal show status

The normal show is the first showcase-host integration, **not final AAA acceptance**. Shared shells
currently use particle smoke and representative burst lights. Continuous selected-star
lighting is not authored/enabled here; F7's benchmark-fixture lights are not silently
injected into saved show assets. The dedicated F8.3B saved fixture above now qualifies both
light families on fluid smoke, and F8.3C qualifies the opt-in particle material on an isolated
puff. The full show's particle smoke remains unlit; full-show overlap/costs remain separate.
Cross-draw ordering, final smoke art, generic fluid injection, natural-star art, final environment/audio mix and
production-finale density remain separate work. Keep these gates in the fireworks roadmap.
