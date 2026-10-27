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

## Opt-in persistent lit-smoke show candidate (F8.1C3)

```powershell
cargo run --release --locked -p aestra-bevy --example fireworks -- --persistent-smoke-show
cargo run --release --locked -p aestra-bevy --example fireworks --features fireworks-audio -- --persistent-smoke-show --tier medium
```

This loads `effects/fireworks_show_persistent_smoke.aestra.ron` and four separate saved
shell variants. The default 26-second show and its sources remain unchanged. The candidate
keeps all 13 launch times, transforms, seeds, star colors, event links, capacities and
sound/representative-light routes. Burst smoke uses the shared lit seeded-billow program,
12–14-second lifetimes and 18-second clip windows. The show lasts 37 seconds, including
a two-second quiet tail after the last clip. No host timer injects smoke births.

It keeps the full audience camera, HDR/bloom, environment, audio options and controls;
it is not a 0.1-scale smoke lab. Its representative-light budgets remain 8/4/2, with no
invented selected-star outputs. Per-draw GPU depth sorting is enabled for its alpha smoke;
this does not solve inter-draw transparency. The flag cannot be combined with a smoke lab;
an explicit `--effect` still overrides its default path.

The native viewer gate separately checks authored overlap lighting, reversible mute/density
controls, a visible 30-second tail, natural zero populations at 33 seconds while late clips
remain owned, and final buffer retirement. LDR capture/cost tooling excludes the public
host's HDR/environment/audio. This is a migration candidate, not photo-approved smoke,
production-finale density, continuous selected-star lighting or complete F8 acceptance.
See [`persistent-smoke-show-2026-10-08.md`](../../benchmarks/fireworks/persistent-smoke-show-2026-10-08.md)
for reproducible controls, matched live costs and the retained failed cue-boundary sample.

## Wispy smoke art draft, transparency probes and rocket deposition (F8.1C4A–D)

```powershell
cargo run --release --locked -p aestra-bevy --example fireworks -- --smoke-art-show
```

This is a separate saved 37-second draft, not a replacement for the default show or
the C3 control. Its four shell variants keep all sound/light cue routes, capacities,
tier policies and seeds. Burst smoke is distributed over a nine-unit sphere, with
smaller particles, more drift/turbulence and a seeded, anisotropic, noise-warped wisp
mask instead of overlapping round billows. Ambient brightness is reduced; later growth
keeps the quiet tail visible. All changes live in editable saved effects/materials, not
host-side particle manipulation. Open `effects/fireworks_show_wispy_smoke.aestra.ron`
in the editor to tune it. The flag excludes C3 and smoke labs; explicit `--effect` still wins.

Rocket exhaust now uses an authored `OnDistance(spacing: 1.0, max_per_tick: 8)` link
from **Launch shell** to **Launch smoke**, one child per effect-local unit with zero
inherited velocity. Smoke lives 8–10 seconds, expands, and drifts gently in a shared
wind instead of being fired upward from the launch origin. Its old fixed-origin
emission is disabled. C3 and the default show remain unchanged comparison controls.

The generic trigger works in event links and particle output routes. Spacing must be
finite and at least 0.001; the per-particle tick limit is 1–64. Routes sharing a source
must agree on both settings. The runtime samples each resolved fixed-tick motion
chord, carrying residual distance per particle; there is no initial sample at birth.
Stationary particles emit nothing. Overspeed drops excess crossings without backlog;
the existing GPU source-overflow counter reports these drops as well as capture-buffer
overflow. The shared capture ceiling stays 1,024 records/tick; count expansion/list
budgets and destination admission still apply. Size spacing and work limits accordingly.

CPU reference users configure `StatefulSimulation::set_distance_emission` with
`CompiledEffect::distance_emission(source_index)` before playback. The Bevy GPU
backend configures it automatically. Checkpoints retain residuals, and changing the
settings invalidates GPU simulation history. GPU persistent state grows from 36 to
40 bytes/slot (four additional bytes); presented particles stay 48 bytes. No new
particle readback, host timer or playback-only history allocation is introduced.
Authored RON/API support is available; dedicated distance-setting editor controls are
not added by this runtime slice. The earlier dated C4A evidence describes the art
snapshot **before** this exhaust change, not the current source hashes.
See [`distance-smoke-2026-10-09.md`](../../benchmarks/fireworks/distance-smoke-2026-10-09.md)
for the repeated native admission, checkpoint and lifecycle evidence.

The public HDR images show less spherical smoke and colored illumination from later
bursts. These are development-build, asynchronous window captures, not exact-frame
references or a production performance measurement. Continuous smoke along star paths,
convincing illuminated cloud depth and photographic art approval remain open.

### Experimental shared transparency in the public host (F8.1C4N)

```powershell
cargo run --release --locked -p aestra-bevy --example fireworks -- --smoke-art-show --experimental-oit
cargo run --release --locked -p aestra-bevy --example fireworks -- --persistent-smoke-show --experimental-oit
```

This explicit opt-in preserves the show's HDR, Tony display transform, bloom, environment,
sound bindings, tier policy and controls. The normal show/candidates still use ordinary
transparency without the flag. The option requires one of the two show-candidate flags;
the small smoke labs and default show are outside its admission policy.

Before opening a window, the host audits the compiled root and every dependency using
the renderer's shared `ExperimentalOitSettings::supports_renderer` predicate. The saved
candidates contain 39 renderer definitions: 27 sprites and 12 trails. Both candidates
at all three tiers preserve their compiled effects exactly. This host requires resolved
semantic materials and rejects unsupported geometry/material operators or extension stages;
it does not silently exclude an incompatible draw from its transparency claim. Host ground
and receiver structures remain opaque. Arbitrary foreign/custom transparency is not certified.

The audience camera explicitly uses `Msaa::Off`, average four fragments/pixel, local
threshold 32 and a 128 MiB shared node/head guard. At 1280x720 this allocates 47,923,200
bytes (45.70 MiB); at 1080p it needs 102.83 MiB. These are node/head bytes, not total
VRAM. Actual camera/device admission is required before playback starts. Budget/backend
rejection pauses playback and exits with a reason, including rejection after resizing;
there is no silent ordinary-rendering fallback or automatic reduction in resolution.
Global fragment loss is a separate possibility; the memory guard is not a loss detector.

An optional development qualification hook records the actual window's full show:

```powershell
$env:AESTRA_EXAMPLE_REPORT = "$PWD/target/fireworks-f8/my-host-run-1.json"
$env:AESTRA_EXAMPLE_REPORT_IMAGES = "0"
cargo run --locked -p aestra-bevy --example fireworks --features fireworks-audio -- --smoke-art-show --experimental-oit --no-audio
```

Use a fresh absolute report filename. The hook exits after the show and ten cleanup
updates, fixes playback/camera/light controls during recording (Esc still exits), and
records wall/Bevy clocks, pauses, pipeline preparation, backend, camera profile and
host-update intervals including vsync. It is not an isolated GPU or exact present-time
measurement. Normal interactive controls are unaffected when the hook is absent.

Set `AESTRA_EXAMPLE_REPORT_IMAGES=1` in a **separate run** for screenshots at nominal
8/15/17.45/25/32 seconds and opt-in asynchronous OIT capacity scans. Capture request times
are recorded; delivered window images are not exact-frame references. The image lane
adds readbacks/scans and must not be used as a pacing-cost lane. Do not combine this
hook with the older single-image `AESTRA_EXAMPLE_CAPTURE` hook. Remove the report env
variables to return to normal interactive use.

See [public-host HDR evidence](../../benchmarks/fireworks/fragment-list-public-host-2026-10-26.md)
for the development-build pilot, retained preparation/pacing limits and image review.
This flag is not default-show migration, a guaranteed 60 FPS result, or photographic
smoke-art approval. Full-show pacing remains an explicit measurement gate.

### Preparing future clip pipelines (F8.1C4O)

The two saved smoke-show candidates now prepare future shell material pipelines before
starting the clock, in ordinary and experimental modes. The public host API is:

```rust,ignore
use aestra_bevy::{EffectPipelinePreparation, EffectPipelinePreparationStatus};
// Attach to the actual active Camera3d, with its final HDR/MSAA/OIT settings.
commands.entity(camera).insert(EffectPipelinePreparation::for_project(&project));
// In the host's loading gate, query that component and wait for:
// EffectPipelinePreparationStatus::Ready { .. }
```

`new(presentations)` also accepts explicit host material bindings. Preparation reuses
the live material shader cache and view pipeline specialization. It creates no clip
entities, particle buffers, cues or simulation warm-up, and never seeks the show.
Every compiled dependency is included even when its first clip starts later. Clip
parameter overrides change uniform values, so they share the prepared program keys.
Replacing a request invalidates its status; rendering reevaluates the actual view key.
Keep the camera active while waiting: status describes its latest extracted configuration.

The initial API supports rendered 3D sprites, trails and flipbooks. Meshes, ribbons and
extension stages fail explicitly instead of reporting partial readiness. Asset residency,
arbitrary material replacement and simulation readiness are separate responsibilities.
The example also retains its compute/global-pipeline and device-admission loading gates.

Full-show reports opt into `gpu::GpuPlaybackDiagnostics`, a bounded latest-frame CPU
command-encoding ledger. Reports distinguish blocked owners, requested versus encoded
simulation time, and clip presentation versus the host schedule. This does not wait for
GPU completion or claim that a particular screenshot contains that exact simulation tick.
Runtime catch-up pacing remains unchanged. A measured attempt to give particle-only
coupling the full exact-tick allowance reduced encoding lag but had poor tail latency
and flash delivery; it was reverted. Preparation readiness does not guarantee that
simulation catch-up or actual display presentation stays within a frame budget.
Final high-tier controls have zero late pipelines and exact clip scheduling, but
50–67 ms sampled encoding lag and 28–32.5 ms p95 host intervals remain open. See
[preparation and registration evidence](../../benchmarks/fireworks/prepared-public-host-2026-10-26.md).

### Clip activation profiling and program reuse (F8.1C4P)

Project playback now prepares default presentation prototypes for all dependencies
during initial loading. Later clips share immutable compiled semantic programs via
`PresentedEffect::fresh_instance()`, with independent instance state and bindings.
The cache is per root/project artifact, invalidated by project replacement and
retired with its root. Hosts constructing presentations directly can prepare their
own prototype and use the same API. Compilation moves to loading; it is not eliminated.

For a separate profiling run, set `AESTRA_EXAMPLE_PROFILE=1` alongside the full-show
report environment variables, with images disabled. This enables Bevy GPU diagnostics
and optional CPU playback/encoding/activation timers. The report includes per-owner
simulation windows/work counts and clip construction durations. GPU results are
asynchronous; CPU activation excludes render-thread GPU allocation and submission.
Set the flag to `0` for plain pacing controls. Profiling and image/capacity scans
cannot be combined. Normal interactive playback leaves these CPU timers off.

The measured high-tier wispy construction cost fell from 22–32 ms per clip to
0.005–0.020 ms; the largest playback CPU window fell from 76 ms to 0.23 ms.
GPU simulation and whole-host pacing remain separate open gates. See
[profile and repeated controls](../../benchmarks/fireworks/profiled-public-host-2026-10-26.md).

### Event-link scratch reuse and encoding accounting (F8.1C4Q)

Lockstep playback now retains per-effect event-list/gather/spawn scratch instead of
creating it every advancing frame/tick. Descriptor changes invalidate affected slots;
removed links and retired owners release ownership. Per-tick parameters and bind
groups remain fresh. Catch-up policy, cues and PlaybackOnly are unchanged.

The profiling report adds `encoding_operations` (CPU scopes, buffer requests/payload
bytes, bind groups and dispatch counts) and `event_scratch_retained` (owned links,
buffers and bytes). These cover instrumented simulation call sites in observed
latest-ledger frames, not complete render-world allocation, driver residency/frees
or measured transfer traffic. Plain runs keep this accounting off.

Observed event-link buffer requests fall from 461,009 to 450 and CPU simulation
encoding p95 from 5.61 to 3.33 ms. Three plain full-show controls preserve all 13
flashes and zero late pipelines; host p95 remains 24.9–26.9 ms, so stable 60 FPS
and final smoke-art acceptance remain open. See
[scratch reuse and native controls](../../benchmarks/fireworks/reused-event-scratch-2026-10-27.md).

The new native transparency oracle uses pure-red/blue half-alpha sheets in separate
players, opposite cameras and reversed spawn orders. It checks linear source-over
against a deliberately wrong-order control, rather than using repeatability as proof.
This qualifies separated sheets only. Native interleaved- and moving-pool probes now
confirm the limitation in both Fast and DepthBackToFront: two red particles around a
blue pool compose incorrectly, and fixed-host draw ordering does not follow a particle
crossing another pool. Split-draw positive controls match the analytic reference.
These are expected-failure diagnostics, **not** cross-draw transparency acceptance.
An experimental camera opt-in now connects eligible native alpha/additive sprite,
flipbook, trail and semantic mesh-particle draws
to Bevy's bounded fragment lists. Its unlit semantic-sprite oracles pass, including
budget/MSAA rejection and disable/re-enable controls. It is **not enabled by default on this show**:
deliberate saturation probes now expose global fragment loss and local-sort approximation
with opt-in bounded GPU diagnostics. Those degraded cases are not accepted blending.
The default depth-prepass path now passes a partial opaque StandardMaterial wall oracle,
including moving-depth and hide/restore controls. Bounded HDR radiance/exposure and
the saved lit-smoke material now also pass analytic and correctly ordered split-draw
controls. This is not smoke-art approval or authored light-route requalification.
Deterministic RGBA fixtures now also qualify semantic sprites/flipbooks and legacy
alpha flipbooks, with frame/texture/material changes and zero-alpha controls. These
are integration fixtures, not realistic smoke assets or a disk hot-reload test.
The bounded mixed-draw gate now checks **positive** depth-ordered alpha/additive
composition: eligible Aestra draws share the lists, and additive emission uses zero
extinction so it does not dim layers behind it. Foreground/background, ordinary sorted
references, isolated-source and camera/OIT restoration controls cover both pool-sort
modes. This supersedes the earlier expected-failure diagnostic, not its historical
evidence. It is not arbitrary-host transparency acceptance: ribbons, multiply,
depth-writing/overlay/wireframe and foreign/custom paths remain outside the qualified
set. Ordinary non-participating draws can still be incorrectly covered by the final
resolve. The Aestra-owned resolve now preserves premultiplied RGB on local overflow,
including additive emission with zero extinction; it is installed only on approved
opt-in cameras. Full-target overflow lists now receive an in-place GPU link merge
sort before resolve, removing arrival-order early blending for accepted fragments.
The existing local limit is a fast-path threshold, not a quality cap on those lists.
Global fragment loss remains possible. Healthy allocation alone is not exact rendering.
Bounded tests additionally cover perspective/orthographic
prepass occlusion, Less/LessEqual, a near-clipped semantic mesh, offscreen target
resizing and two-camera shared-buffer isolation/release. Multi-camera overflow
diagnostics remain unavailable; arbitrary depth/custom projections, sub-viewports,
resolution overrides, window lifecycle and full-finale live costs are not certified.
The earlier paced 540p/1080p benchmark exposed dense local overflow even at 32 slots;
it motivated this ordering repair. Over-sort counters now identify merge-sort work,
not necessarily approximate blending. Default storage still rejects 1080p; explicit provisioning
costs about 103 MiB of node/head storage. These are bounded 6..10-second native workload
measurements, not production/full-show acceptance. Public defaults stay unchanged.
See [paced costs and capacity](../../benchmarks/fireworks/fragment-list-live-2026-10-26.md).
The dense overflow oracle now qualifies both the color repair and bounded ordering:
40 homogeneous smoke layers and 40 additive layers pass the unchanged .01 linear error
bound at local thresholds 1/8/32. The 41-layer interleaved cases now pass too,
with maximum error below .003 and byte-exact repeats of all 52 captures.
The native unmanaged-camera control retains Bevy's resolver; opt-in removal/restoration
is checked separately. During cold specialization the native pipeline remains active
until the Aestra variant is ready, so this is not a first-frame color guarantee.
C4M1 completed the math/oracle slice; C4M2 removes its interleaved-order failure without
new fragment storage. The same production shader also passes sparse GPU lists through
4,097 nodes. This remains an experimental compositor, not show migration or art approval.
See [ordering and current costs](../../benchmarks/fireworks/fragment-list-order-2026-10-26.md)
and the retained [earlier overflow failures](../../benchmarks/fireworks/fragment-list-overflow-2026-10-26.md).
See [depth/camera-lifecycle evidence](../../benchmarks/fireworks/fragment-list-camera-depth-2026-10-25.md),
[shared mixed-list evidence](../../benchmarks/fireworks/fragment-list-unified-mixed-2026-10-25.md),
[original mixed-pass failure](../../benchmarks/fireworks/fragment-list-mixed-2026-10-25.md),
[textured/flipbook evidence](../../benchmarks/fireworks/fragment-list-textures-2026-10-25.md),
[HDR/lit-material evidence](../../benchmarks/fireworks/fragment-list-hdr-lighting-2026-10-23.md),
[opaque-depth evidence](../../benchmarks/fireworks/fragment-list-depth-2026-10-23.md),
[saturation diagnostics](../../benchmarks/fireworks/fragment-list-saturation-2026-10-23.md),
[prototype scope and evidence](../../benchmarks/fireworks/fragment-list-prototype-2026-10-23.md)
and the original limitation probes in
[`cross-pool-transparency-2026-10-23.md`](../../benchmarks/fireworks/cross-pool-transparency-2026-10-23.md).
See [`smoke-transparency-art-2026-10-09.md`](../../benchmarks/fireworks/smoke-transparency-art-2026-10-09.md)
for controls, retained failed art iteration and remaining gates.

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

## Running a relocated package

Keep the following content layout when copying the executables outside the checkout:

```text
package/
  apps/aestra-editor/aestra-editor.exe
  apps/fireworks/fireworks.exe
  assets/          # repository assets, including test/ for the showcase
  sample-project/  # curated editor examples; exclude local .aestra/ state
  audio/           # optional, separately licensed fireworks WAVs
```

The editor resolves its bundled sample project/extensions and the showcase resolves
`assets/test` and optional `audio` relative to the executable, not the working directory
or the compile-time checkout. Missing package content does not fall back to source files.
No Cargo environment variables or `--project`/`--audio-root` flags are required.
The editor's saved/recent project selection still takes precedence over its examples.

`cargo run` uses Cargo's **runtime** `CARGO_MANIFEST_DIR` for development, including
custom target directories. Direct execution from the standard `target/{profile}`
layout also works without that variable. For nonstandard standalone example layouts,
use explicit `--project` and `--audio-root` paths. Relative CLI paths remain relative
to the launch working directory; existing audio folders are made absolute before
registering the Bevy asset source. Release builds omit `dev-dynamic-linking`; see
the separate Windows linkage report for development DLL requirements. This is a
content-layout contract, not a clean-machine installer certification.

## Optional host sound

Audio is outside the portable VFX runtime. Enable the example's WAV decoder explicitly:

```powershell
cargo run --release --locked -p aestra-bevy --example fireworks --features fireworks-audio
cargo run --release --locked -p aestra-bevy --example fireworks --features fireworks-audio -- --audio-root C:/MySounds/fireworks
```

The development default is `bevy/aestra-bevy/assets/audio/fireworks`; a relocated
package uses its `audio/` folder as described above. Expected files:
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

Bevy 0.20 native recheck (2026-10-22): the complete default and wispy shows finish
successfully with the same 134 admitted/89 dropped sound voices, zero pending cues
and 24 peak spatial sinks. Seven native captures cover rocket smoke, overlapping
colored lighting and the quiet tail. All 18 audio-enabled tests and strict scoped
Clippy pass. This is compatibility evidence, not a release-performance or final
smoke/mix approval; see the
[native showcase report](../../benchmarks/bevy-upgrade/native-showcase-2026-10-22.md).

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

## Live smoke costs and exact cleanup (F8.1C1–C2)

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

The historical C1 run exposed a cleanup gap: stateful float accumulation stopped short of
the last fixed tick at the `Once` boundary, leaving faded smoke alive. Its failed cleanup
counts remain recorded. F8.1C2 now uses exact per-tick clock snapshots, boundary-aware GPU
target ticks and fixed-grid CPU/GPU particle ages. The stricter 30-run native matrix
proves zero populations in all 60 cohort-root observations at exactly 18 seconds, without
seek, replay or forced clearing. Two final image runs match all 84 captures; timing fixes
change 66 captures from the historical baseline. See
[`smoke-live-costs-2026-10-08.md`](../../benchmarks/fireworks/smoke-live-costs-2026-10-08.md)
for historical costs and
[`smoke-final-tick-2026-10-08.md`](../../benchmarks/fireworks/smoke-final-tick-2026-10-08.md)
for corrected costs, cleanup and reproduction. F8.1C3 adds the opt-in saved-show candidate
above; the normal show remains unchanged/unlit. Cross-draw transparency and public-host
HDR smoke-art review are the next gates, not another replay optimization.

## Normal show status

The normal show is the first showcase-host integration, **not final AAA acceptance**. Shared shells
currently use particle smoke and representative burst lights. Continuous selected-star
lighting is not authored/enabled here; F7's benchmark-fixture lights are not silently
injected into saved show assets. The dedicated F8.3B saved fixture above now qualifies both
light families on fluid smoke, and F8.3C qualifies the opt-in particle material on an isolated
puff. The default show's particle smoke remains unlit. The separate F8.1C3 candidate
qualifies bounded persistent lit smoke under saved 13-clip overlap; it is not the accepted
replacement for the default show or a production-finale performance claim.
Cross-draw ordering, final smoke art, generic fluid injection, natural-star art, final environment/audio mix and
production-finale density remain separate work. Keep these gates in the fireworks roadmap.
