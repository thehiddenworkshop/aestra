# Bevy fireworks showcase

`demos/fireworks/src/main.rs` is the public integration host. It loads the
same saved effects and materials as the editor, not an implementation imported from
`aestra-viewer`. The viewer remains the capture, regression and benchmark harness.

```powershell
cargo run --release --locked -p aestra-demo-fireworks
cargo run --release --locked -p aestra-demo-fireworks -- --tier medium
cargo run --release --locked -p aestra-demo-fireworks -- --history replay-enabled
```

The default is **host-owned rockets**: five scheduled launches plus Enter to launch,
with host-owned position, velocity, gravity and fuse. The host publishes current
world poses before `ResolveHostInputs`; attached emitters deposit particles into a
stationary VFX owner. Detonation stops wake emission, removes the gameplay rocket,
and starts a separate calibrated burst at the host's position. Wake/burst owners
remain alive for their finite visual tails. Normal host admission has no owner-count
ceiling; `--max-owners N` is an optional host policy for controlled workloads.
Sound is bound directly to the host detonation; GPU output events do not decide hits.
Space and R affect the host and VFX together. This is a host-integration demo;
smoke appearance still needs the existing F8 art review.

Native GPU presentation, high quality, fixed HDR exposure/bloom and **playback-only**
history remain the defaults. `--cinematic` selects the original authored show.
Selecting a smoke candidate, lab or explicit `--effect` also selects authored playback.
The 26-second, play-once cinematic show uses 13 clips referencing
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

The executable and offline authoring tools now live in `demos/fireworks`; Cargo
workspace CI still builds and tests them. The editable asset project remains in
`assets/test`, shared with editor/viewer qualification to avoid divergent copies.
Historical evidence retains its original source paths and hashes. The focused
`cargo run -p aestra-bevy --example host_projectile` example teaches the same
ownership boundary without the demo's sound, cameras, art variants or review tools.

## Calibrated selected-star lighting (2026-10-28)

`author_fireworks_star_lights` adds one material-free point-light output to each
calibrated shell's Main stars. Its separate saved effects preserve every particle
module, renderer, material, event link, smoke budget, sound output and burst flash.
No smoke, dense ember or Burst flash emitter gains selected-particle lights.
The output samples the exposed Main-star gradient, so the pair's blue override
also controls its light color. Its age curve cools 6000 → 3000 → 0 lumens, with
12-unit range, radius 0.1 and shadows off. These are scene tuning values rather
than calibrated photometry.

Authored per-output ceilings are high 32, medium 16, low 24. Low deliberately
retains the previously qualified sparse-star admission, rather than the rejected
eight-light draft. Host global ceilings remain independently bounded at 96/48/24.
These are capacity bounds; neither the HUD nor reports call them live-light counts.
The native same-frame adapter does not copy selected particle positions to the CPU.

The default host controller opts into this art and plugin using `--star-lights`.
Representative flashes have an independent `--no-flash-lights` switch. L toggles
flashes and P toggles selected lighting after launch; both affect presentation only.
Particle selection, transport and realization caps change together through the
public host policy. An explicit cinematic asset without authored outputs is rejected
when `--star-lights` is requested, avoiding a silently ineffective option.

```powershell
cargo run --release --locked -p aestra-demo-fireworks -- --star-lights
cargo run --release --locked -p aestra-demo-fireworks -- --star-lights --no-flash-lights
cargo run --release --locked -p aestra-demo-fireworks -- --smoke-art-show --effect effects/fireworks_shell_star_lights.aestra.ron --experimental-oit --star-lights --no-audio
```

The retained two-shell timeline permits matched neither/flash-only/star-only/combined
controls. `benchmarks/fireworks/run-star-light-review.ps1` runs that matrix and
records binary/source fingerprints. `-Cost` uses a separate ordinary lane, without
image requests or capacity scans. The baseline host and cinematic show remain
unchanged unless opted in; final smoke art and expanded-show costs remain open.
The [native integration review](../../benchmarks/fireworks/star-light-smoke-integration-2026-10-28.md)
records 84 control captures, a separate medium host screenshot, the subtle blue
smoke response and the single ordinary timing pair, including their limits.

## Eroding smoke and host overlap

The newer [eroding smoke/host overlap review](../../benchmarks/fireworks/smoke-look-host-overlap-2026-10-28.md)
uses separate assets and `--tuned-smoke --launch-pattern overlap` in the host demo.
Host admission has no default count ceiling; `--max-owners N` is optional. The
review runner names a 24-owner workload budget explicitly. It records twelve
host launches/detonations, retained tails and all-tier cleanup, along with the
ordinary pacing outliers. This does not promote either smoke draft to final art.

## Independent player preparation and launch pacing

`AestraPlugin` now maintains `EffectPresentationCache` for independently spawned
players. Repeated uses of the same compiled artifact share immutable material
programs; each presentation still owns its mutable values, bindings and history.
Same-ID artifact replacements get new programs, and unused artifacts retire from
the cache. A loaded compiled catalog intentionally keeps its programs warm.

The demo prepares the same cache during loading and passes fresh presentations
to the camera request, through the public SDK:

```rust,ignore
// In a loading system with ResMut<EffectPresentationCache>:
let request = EffectPipelinePreparation::new(
    std::iter::once(project.root.clone())
        .chain(project.dependencies.values().cloned())
        .map(|effect| presentations.instantiate(effect)),
);
commands.entity(camera).insert(request);
```

Without explicit loading preparation, the first player for an artifact still
pays compilation once. This cache does not pool mutable simulation buffers or
introduce an owner-count ceiling. The [native activation review](../../benchmarks/fireworks/host-activation-2026-10-28.md)
measures presentation construction falling from roughly 31 ms to 0.011 ms, and
launch-phase ordinary p95 falling from 47.0 ms to 17.6–17.7 ms in the same
twelve-rocket workload. It preserves admission totals and final cleanup.

The smoke review runner accepts `-Cost -Instrumented` for a separate timestamp
lane; `-Cost` alone remains ordinary pacing. Reports split launch/overlap, active
sparks, smoke drain and idle using host state and asynchronous population
observations. These are window/vsync measurements on the documented hardware,
not release/finale performance guarantees or final smoke-art acceptance.

## Audience smoke revision (2026-10-28)

The `--tuned-smoke` candidate now makes deposited launch smoke visible within
roughly 100 ms, strengthens thin star filaments at audience distance, and dilutes
older smoke while retaining a local response to later bursts. Particle births,
motion, lifetimes, luminous art, sampler/event budgets and light-count limits are
unchanged. Ambient brightness is unchanged; the material's scene-light gain and
smoke appearance curves are tuned instead.

```powershell
cargo run --release --locked -p aestra-demo-fireworks -- --tuned-smoke --star-lights
```

The [audience-camera review](../../benchmarks/fireworks/audience-smoke-2026-10-28.md)
records before/after wakes, four matched older-haze lighting/visibility controls,
all-tier twelve-rocket overlap and cleanup, 102 captures and separate pacing
runs. One noisy ordinary run remains recorded; whole-game performance and final
photographic acceptance remain open. CLI defaults and calibrated control assets
stay unchanged. The following camera review now covers moving/side views.

## Side, near and moving-camera smoke review

The report hook and smoke review runner now accept `-Camera side`, `near` and
`orbit`. Orbit smoothly sweeps the camera using the existing playback clock;
capture records include the requested camera translation/rotation and root time.
The unparented camera publishes its current world pose before render preparation.
No effect seek, replay, material or budget edit is involved, and moving-camera
review is rejected in ordinary cost/profiling lanes.

The [camera review](../../benchmarks/fireworks/smoke-views-2026-10-28.md) records
209 captures, two sampled motion previews, unchanged demand and clean retirement.
Side/moving views retain spatially placed haze; the near view exposes more of the
mottled puff structure. The pair's 7,379 near-view pixels above the local fast-sort
threshold already use the existing overflow merge sort; a separate GPU profile
confirms that pass runs. The counter is not itself a dropped-layer failure.

The following independent-host transparency review now records both failed
capacity cases and a bounded low-tier configuration. Close-up art, general
transparent-scene participation and default adoption remain separate.

## Independent-host shared transparency: capacity qualification

`--experimental-oit` now admits the independently spawned host wake/burst catalog
through the same compiled-renderer audit as the saved show candidates. It preserves
particle births, trajectories, lifetimes and light policies. Ordinary transparency
remains the default, and normal host admission still has no owner-count ceiling.

The [native review](../../benchmarks/fireworks/host-shared-transparency-2026-10-28.md)
records eight image runs and 97 captures at 1280x720 on RTX 4070 SUPER/Vulkan.
Both ordinary/shared controls explicitly use `Msaa::Off`. The default four-node-per-pixel
reservation loses up to 19,144,079 fragments in a high-tier near-view frame; high
orbit and low near fail too. Completing a report is not a capacity pass.

An explicit low-tier reservation of eight passes observed near/orbit capacity,
admission and cleanup, using 92,160,000 bytes (87.89 MiB) for nodes/heads:

```powershell
cargo run --release --locked -p aestra-demo-fireworks -- --tuned-smoke --star-lights --tier low --launch-pattern overlap --experimental-oit --oit-fragments-per-pixel 8 --no-audio
```

This changes reservation within the existing 128 MiB ceiling, not the ceiling or
default. Larger windows can be rejected by the same budget. Normal interactive
experimental mode enables bounded asynchronous capacity/integrity scans and stops
with an error after observed fragment loss or invalid links; it never silently
switches rendering modes. Detection cannot prevent an earlier failed frame.

The runner accepts `-Transparency ordinary|shared -MatchMsaa`, an explicit
`-FragmentsPerPixel 8`, and `-RequireLossFree` to fail a scanned qualification while
retaining its raw report. Cost/profile runs omit health scans and cannot certify
capacity or the full cost of the guarded interactive mode. High-tier near demand
would need at least 264.79 MiB just for nodes/heads: filling the remaining current
budget cannot fix it. Next is memory-scalable high-density compositing; this is
not general host, photographic or production-finale acceptance.

The subsequent [packed-zero review](../../benchmarks/fireworks/packed-identity-2026-10-28.md)
removes exact identities of the existing RGB9E5/alpha8 compositor before node
allocation. Black absorbers and faint additive light are preserved by a 20-case
frozen native oracle. High near capture demand falls 11.93%, while allocated
buffers remain unchanged; it still needs roughly 234 MiB for observed nodes/heads
and fails the 128 MiB policy. Low reserve-eight near/orbit remains loss-free in
observed samples with unchanged demand and cleanup. Forward host screenshots are
not bit-identical frozen references.

The [isolated partition prototype](../../benchmarks/fireworks/partition-probe-2026-10-28.md)
now reuses one node pool between disjoint scissored regions. Three native repeats
match the complete HDR reference exactly, preserve total fragment attempts and
recover after forced overflow. Sixteen regions use 82.73% fewer node/head bytes
than a tightly sized full-screen control, but cost roughly 5.5–5.7 times its GPU
span in the small 48-layer fixture. Four regions halve storage at about twice
the time. Aggregate timing uses two timestamp writes per frame, with detailed
profiling separate. This is test-only evidence; no runtime mode is exposed yet.
Next is guarded opt-in host transport and actual near/orbit memory/cost review,
including real material and repeated trail-geometry work. Ordinary remains default.

## Opt-in persistent lit-smoke show candidate (F8.1C3)

```powershell
cargo run --release --locked -p aestra-demo-fireworks -- --persistent-smoke-show
cargo run --release --locked -p aestra-demo-fireworks --features fireworks-audio -- --persistent-smoke-show --tier medium
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
cargo run --release --locked -p aestra-demo-fireworks -- --smoke-art-show
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
cargo run --release --locked -p aestra-demo-fireworks -- --smoke-art-show --experimental-oit
cargo run --release --locked -p aestra-demo-fireworks -- --persistent-smoke-show --experimental-oit
```

This explicit opt-in preserves the show's HDR, Tony display transform, bloom, environment,
sound bindings, tier policy and controls. The normal show/candidates still use ordinary
transparency without the flag. The option admits the independent host catalog or
one of the two show-candidate flags; small smoke labs and the default cinematic
show are outside its admission policy.

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
The memory guard alone cannot detect fragment loss. Interactive mode additionally
scans capacity/integrity asynchronously and exits after observed loss. Report
lanes deliberately retain failed runs for review; only image reports scan capacity.

An optional development qualification hook records the actual window's full show:

```powershell
$env:AESTRA_EXAMPLE_REPORT = "$PWD/target/fireworks-f8/my-host-run-1.json"
$env:AESTRA_EXAMPLE_REPORT_IMAGES = "0"
cargo run --locked -p aestra-demo-fireworks --features fireworks-audio -- --smoke-art-show --experimental-oit --no-audio
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
The demo also retains its compute/global-pipeline and device-admission loading gates.

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

### Ordered stateful parameters (F8.1C4R)

Coupled stateful playback also reuses parameter destinations and groups bound to
current resources. Distinct upload ranges feed ordered encoder copies before each
tick/presentation; live destinations are never repeatedly queue-written. Independent
multi-tick compute batches retain separate per-tick parameters. Unchanged packed
physics inputs reuse their upload buffer; changed data/resources invalidate groups.

Profiling adds copy/queue-write payload counters, parameter/page ownership and the
latest CPU `stateful_render_preparation` window. That window excludes other render
preparation systems, GPU work and driver behavior. Pages retain busy-period high
water capacity and release on an encoded frame without parameter uploads.

The measured simulation CPU p95 falls from 3.33 to 1.73 ms; all full-show controls
retain 13 flashes and zero late pipelines. Host p95 remains 24.5–26.0 ms, and one
control still has a 73 ms interval/three ticks of sampled lag. Stable 60 FPS remains
open. See [parameter reuse evidence](../../benchmarks/fireworks/reused-stateful-parameters-2026-10-27.md).

### Saved star-path smoke candidate (F8.1C4S)

```powershell
cargo run --release --locked -p aestra-demo-fireworks -- --smoke-art-show --effect effects/fireworks_show_star_path_smoke.aestra.ron --experimental-oit
```

This separate saved 13-clip candidate deposits smoke at Main stars' distance
crossings with zero velocity inheritance and zero independent emission. Puffs
remain along the paths, drift slowly, expand and fade over 7–9 seconds. A smoother
dedicated density material and subdued shell haze reduce the earlier granular
puffs. Original rocket/star emitters, choreography and cue/light routes are retained.
The normal, persistent and wispy shows keep their saved assets unchanged.

Final high-tier audience/close and density-hidden native reviews each observe
37,996 accepted births across 13 owners, zero source/destination admission drops,
13 flashes, zero observed OIT fragment loss/invalid links and a full drain.
The image runs each record one global-cache pending update; they do not establish
zero late pipeline work. Separate cost runs record zero pending updates, but
candidate host p95 varies 20.3–26.7 ms versus 18.4 ms for the retained wispy control
in this batch. These vsync-inclusive dev-build intervals are not isolated GPU cost
or stable 60 FPS acceptance. All tiers compile; native medium/low art gates remain.
Strand regularity and broad haze still need tuning before photographic art/default
adoption. See [captures, rejected drafts and reproduction](../../benchmarks/fireworks/star-path-smoke-2026-10-27.md).

F8.1C4T now extends native audience/close and flash-muted review to all tiers.
High/medium/low retain 37,996/18,959/9,313 smoke births with zero observed admission
or fragment drops and a full drain. Representative flashes recolor older smoke;
selected-star lighting is not enabled in this candidate. Low explicitly rejects
two flashes at its two-light ceiling. An initial medium close run loses one flash
without a reported rejection and records 1.1 seconds of sampled encoding lag; a
successful repeat does not erase that failure. Robust flash delivery remains open.
`AESTRA_EXAMPLE_REPORT_FLASHES=0` is an image-only zero-intensity control that
preserves admission and simulation. See the
[all-tier review, retained failure and runner](../../benchmarks/fireworks/star-path-tiers-lighting-2026-10-27.md).

F8.1C4U fixes two coupled source-cue suppression cases: a paced forward backlog
classified as replay, and first trail initialization suppressing a launch. Native
regressions reproduce both failures and preserve seek/pause/restart guards.
Five final native controls each account for13 launch and13 main-break cues and13
bound intents; high/medium accept13 flashes, while low accepts11 with two explicit
budget rejections. A bounded report audit and new source-binding counters retain
stage accounting. Earlier failures remain recorded; this is not lossless transport
or pacing/art approval. See
[forward cue fixes and controls](../../benchmarks/fireworks/forward-output-cues-2026-10-27.md).

### Single-shell point-star reference calibration

```powershell
cargo run --release --locked -p aestra-demo-fireworks -- --smoke-art-show --effect effects/fireworks_point_star_candidate.aestra.ron --experimental-oit --no-audio
```

This separate 18-second shell targets the user's video reference: small bright
white-to-warm heads, short thin tails and a 2.4–3.2-second varied burn. The explicit
high/medium/low budgets are 128/64/32 stars with matching trail capacities. Main
head/tail radiance are separately exposed; launch/flash materials stay unchanged.
Replace `candidate` with `control` to compare the retained shorter burn and longer
tails with the same white palette. Both isolate the shell from secondary breaks
and deposited star smoke; launch/burst haze remains. The normal show is unchanged.

`author_fireworks_point_stars` regenerates both editable assets offline. Six native
camera/tier runs deliver 78 lifecycle captures with cue/flash/drain controls. This is
a point-star art candidate, not gold-tail, smoke integration, performance or default
approval. See [images, evidence and reproduction](../../benchmarks/fireworks/point-star-calibration-2026-10-27.md).

### Dense gold-tail reference calibration

```powershell
cargo run --release --locked -p aestra-demo-fireworks -- --smoke-art-show --effect effects/fireworks_gold_tail_candidate.aestra.ron --experimental-oit --no-audio
```

This opt-in 18-second shell uses 64/32/16 ballistic stars and dense distance-born
gold embers. Six embers every 0.4 units produce a scattered shower that cools and
falls behind the heads. Explicit high/medium/low ember pools are 32,768/16,384/8,192;
sprites avoid adding a ribbon history for every shed spark. Main head, tail and
ember radiance are independently exposed. The existing launch/flash bindings remain.

`author_fireworks_gold_tails` regenerates the candidate and an isolated warm control.
Native counts admit all 67,254/33,324/15,354 tier ember births with full drain.
Six selected camera/tier cases deliver 84 captures; a separate ordinary host pair
records pacing. This remains single-shell art/admission review. Deposited smoke,
show composition, final art and production performance are subsequent work.
See [density comparison, counts, costs and reproduction](../../benchmarks/fireworks/gold-tail-calibration-2026-10-27.md).

### Calibrated shells with deposited smoke

```powershell
cargo run --release --locked -p aestra-demo-fireworks -- --smoke-art-show --effect effects/fireworks_shell_smoke_candidate.aestra.ron --experimental-oit --no-audio
```

This 24-second review pair adds deposited smoke to separate variants of the
calibrated dense gold and point-star shells. Gold fires first; blue point stars
start 5.5 seconds later and their representative flash lights older haze. The
original calibrated luminous emitters, cues and dense ember recipe are retained.

`author_fireworks_shell_smoke` regenerates the two source variants and
candidate/control roots. Gold smoke shares the ember source's 0.4/max2 distance
sampler; point smoke uses 0.8/max2. Both use zero inherited velocity and no
independent emission. Explicit pools support the longer lives and denser paths.
The original sources remain available through `fireworks_shell_smoke_control`.

Eight native image cases deliver 128 captures across camera/tier and independent
light/density controls; a nine-capture population witness proves natural expiry
before clip retirement at high tier. Selected-star lighting, final smoke art and
production-show performance remain subsequent work. See
[images, counts, lifetime observations and costs](../../benchmarks/fireworks/shell-smoke-integration-2026-10-28.md).

### Wind-drifted smoke art variant (F8.1C4V)

```powershell
cargo run --release --locked -p aestra-demo-fireworks -- --smoke-art-show --effect effects/fireworks_show_wind_haze_smoke.aestra.ron --experimental-oit
```

This separate saved variant gives deposited smoke more drift/turbulence, uneven
age growth/fade and softer density masks. Older smoke spreads into a wider field;
original stars, rockets, shell/launch haze, choreography, cues and budgets remain
unchanged. C4S and the default show remain comparison choices. Rebuild only the
new effects with `author_fireworks_wind_smoke` after editing its offline recipe.

Six native image controls pass cue/admission/capacity/drain checks at all tiers,
with separate flash-muted and path-density-hidden high controls. Two larger drafts
remain rejected for fragment loss. A single ordinary pacing pair measures host
p95 27.94/27.84 ms for retained/new sources; it is not isolated GPU cost or stable
60 FPS acceptance. Individual puffs, volumetric depth and final art approval remain
open. See [before/after, controls and reproduction](../../benchmarks/fireworks/wind-haze-smoke-2026-10-27.md).

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
4. Save, then restart the demo. It reads saved source files at launch, not stale embedded text.

For a separate project, pass its **asset root**, not necessarily its repository root:

```powershell
cargo run --release --locked -p aestra-demo-fireworks -- --project C:/MyShow/assets --effect effects/my_show.aestra.ron
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
layout also works without that variable. For nonstandard standalone demo layouts,
use explicit `--project` and `--audio-root` paths. Relative CLI paths remain relative
to the launch working directory; existing audio folders are made absolute before
registering the Bevy asset source. Release builds omit `dev-dynamic-linking`; see
the separate Windows linkage report for development DLL requirements. This is a
content-layout contract, not a clean-machine installer certification.

## Optional host sound

Audio is outside the portable VFX runtime. Enable the demo's WAV decoder explicitly:

```powershell
cargo run --release --locked -p aestra-demo-fireworks --features fireworks-audio
cargo run --release --locked -p aestra-demo-fireworks --features fireworks-audio -- --audio-root C:/MySounds/fireworks
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
cargo test --locked -p aestra-demo-fireworks --features fireworks-audio
cargo clippy --locked -p aestra-demo-fireworks --all-targets --features fireworks-audio -- -D warnings
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
cargo run --release --locked -p aestra-demo-fireworks -- --smoke-lighting --no-audio
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
cargo run --release --locked -p aestra-demo-fireworks -- --particle-smoke-lighting --no-audio
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
cargo run --release --locked -p aestra-demo-fireworks -- --particle-smoke-lighting --effect effects/fireworks_particle_smoke_overlap.aestra.ron --no-audio
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
cargo run --release --locked -p aestra-demo-fireworks -- --smoke-persistence --no-audio
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
cargo run --release --locked -p aestra-demo-fireworks -- --smoke-cohorts --no-audio
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
