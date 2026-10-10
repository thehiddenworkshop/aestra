# Bevy 0.20 dependency and patch preflight

This is migration evidence tooling, not a new benchmark or a replacement for CI.
See the [upgrade plan](../../docs/new/AESTRA_BEVY_0_20_UPGRADE_PLAN.md) and
[2026-10-09 audit](preflight-2026-10-09.md) and the subsequent
[dependency isolation](dependency-isolation-2026-10-09.md).

## Dependency resolution without changing Aestra

From the repository root, using PowerShell 7:

```powershell
$probe = ./benchmarks/bevy-upgrade/prepare-020-probe.ps1 -Resolve
Get-Content "$probe/resolution-summary.json"
cargo tree --manifest-path "$probe/Cargo.toml" --locked --offline --all-features -d
```

The script copies every workspace member manifest into a fresh ignored directory in
`target/bevy-0.20-preflight/`, applies explicit migration-version edits to those copies,
and omits the local 0.19 patch table. It retains the engine-neutral SVG renderer,
all active workspace features and the repository toolchain. The two explicitly
[suspended physics adapters](../../bevy/PHYSICS_ADAPTERS.md) are outside this graph.
The original preflight report below remains historical evidence of the pre-isolation
17-member graph, not the current resolution result. A Windows MSVC platform filter
is used for dependency resolution. Use `-Resolve -Offline` after the registry/index
and required sources have been populated; omit `-Resolve` to prepare only.

Original manifests and lockfile are hashed before/after. `probe-inputs.json` records
provenance; `metadata.json`, `Cargo.lock`, `resolve.log` and `resolution-summary.json`
retain the experiment. Preparation/resolution success is **not** compatibility:
check `compatible_single_engine_graph`. The script reports a mixed graph explicitly.
Do not interpret unrelated math-conversion versions as identical to ECS type mixing.

Rust targets in this main manifest copy are placeholders. **Never run builds or tests
against it or claim API compatibility from it.** No vendored source is changed, and
this does not authorize dropping the real patches. Baseline-specific transformations
fail if the input manifest has changed and needs a fresh review.

## Real, isolated keyboard regression

The separate `keyboard-dispatch` workspace references the real
[keyboard-020.rs](keyboard-020.rs) test source and exact published Bevy 0.20.0 crates.
It has no GPU, window loop or OS clipboard access and does not inherit Aestra's patches.

```powershell
cargo +1.98.1-x86_64-pc-windows-msvc test --manifest-path "$probe/keyboard-dispatch/Cargo.toml" --offline -- --test-threads=1
```

On published 0.20.0, the held-Control control test passes and the same-frame chord
test fails: the focused observer sees Control released at A-down. The failing assertion
expresses required behavior; it is **not** ignored, inverted or marked `should_panic`.
This negative qualification fixture is outside the shipping workspace/ordinary CI.
Keep its failure as evidence that the patch cannot yet be removed. Once testing a
candidate fix, both tests must pass, followed by the full editor shortcut regressions.

Use the repository's Rust version and an installed target toolchain. The example
above deliberately selects MSVC on Windows; it is not a Rust-version upgrade.

## Candidate 0.20 fixes: real build and native qualification

`patches-020/` is a separate, locked workspace using actual published Bevy 0.20.0
sources and `vendor/bevy_input_focus_020` / `vendor/bevy_pbr_020`. Unlike the manifest
resolution probe, it compiles real code. The root workspace still uses 0.19.1 and
its original patches; these candidates do not silently switch the shipping engine.

From the repository root with PowerShell 7 on Windows:

```powershell
./benchmarks/bevy-upgrade/run-020-patches.ps1
# Run explicitly on a native GPU; native tests run serially in separate processes.
./benchmarks/bevy-upgrade/run-020-patches.ps1 -Native
```

The runner verifies untouched published-file hashes, the single-engine graph and
selection of both local candidate patches. It checks nine keyboard requirements
(including the exact two unpatched-probe assertions and actual 0.20 TextInput edits),
all upstream input-focus unit tests, and four cluster pool/reset/growth regressions.
Native tests retain the original generation/growth/mode/view/allocator assertions.
The headless host omits Winit and pipelined rendering; it is not a production host,
editor interaction, visual or frame-time benchmark. No OS clipboard is read.

Fresh logs, metadata, source and executable hashes, and success summary are saved to
`target/bevy-020-qualification/runs/<id>/`. Existing evidence is never overwritten;
failed runs retain their logs but produce no accepted summary. The first build needs
cached/downloaded dependencies; the runner deliberately uses `--locked --offline`.
Use `cargo fetch --manifest-path benchmarks/bevy-upgrade/patches-020/Cargo.toml --locked`
first on a machine without the dependencies. The independent lockfile is versioned.

These migration-only tests are outside ordinary CI until the root engine migrates.
Before selecting the candidate patches in the root manifest, complete B20-1/B20-2
and re-run the full editor and native renderer gates; patch qualification alone
does not establish Aestra 0.20 compatibility. Root Cargo patches also do not
automatically reach downstream users.

## Portable compiler migration

The shipping workspace's engine-neutral compiler now pins WESL 0.6.0/Naga 30.0.1;
the engine itself remains Bevy 0.19.1 until the renderer/editor compatibility port.
Only generated WGSL text crosses that temporary compiler-version boundary.
See [implementation, cache invalidation and checks](portable-compiler-2026-10-09.md).
The resolution probe asserts these pins and still tests the prospective single
0.20 engine graph without compiling placeholder sources.

## Bevy-dependent shader composition

The production volume/material shader bodies now share a composition module with
the candidate WESL dialect. Only the legacy dialect ships with the current 0.19
engine; the 0.20 branch is deliberately test-only until the coherent switch.
`patches-020/shader_composition.rs` compiles actual smoke/fire/liquid, lit material
and editor graph bodies through Bevy 0.20's real `ShaderCache` and the shader sources
selected by the independent lockfile. It uses no stand-in cluster/View/UI types.
The graph adapter changes only the import; real editor asset registration remains
part of the engine/editor port.

`run-020-patches.ps1` now includes these headless checks. Its `-Native` mode also
validates 18 wgpu 30 shader modules/render pipelines, **after** the existing cluster
tests exit. No display/window is created. A real Vulkan GPU is required; missing
hardware fails rather than producing accepted evidence. Inferred pipeline layouts
and a fixture UI vertex do not qualify the not-yet-ported Bevy CPU-side layouts,
visual output or performance. Source/binary hashes and logs are retained in the
same fresh run directory; the shipping Cargo.lock is unchanged.

See [implementation and validation](shader-composition-2026-10-09.md).

## Storage-buffer compatibility

The production renderer now isolates its ShaderBuffer construction, updates,
indirect usage and CPU-byte access in `gpu/storage_buffers.rs`. Its encase encoder
is shared verbatim with the real 0.20 candidate `gpu/storage_buffers_020.rs`; the
candidate is not selected by the shipping 0.19 workspace yet.

`patches-020/storage_buffers.rs` exercises actual Aestra records, CPU-data draining
and repeated updates. The runner includes these headless checks and, with `-Native`,
real asset extraction/preparation, allocation reuse, resize/label/usage identity
invalidation, byte readback and GPU-owned resize preservation. This fourth native
test executes only after both cluster tests and the shader test have exited.
Missing hardware fails; no native wait or particle readback is added to production.

This is an asset-boundary gate, not full ECS extraction, automatic refresh of
Aestra's custom bind groups, visual parity or a performance benchmark. See
[implementation and evidence](storage-buffers-2026-10-09.md).

## App-labeled draw and optional resource extraction

The real draw payload, semantic material binding, wireframe geometry, sprite-cull
bounds and host world-SDF resource now live in small shared production modules.
`gpu/extraction.rs` installs the shipping 0.19 boundary; `gpu/extraction_020.rs`
implements the explicit `RenderApp`-labeled 0.20 traits and plugins.

`patches-020/extraction.rs` imports these actual types and runs the same six lifecycle
contracts as the shipping renderer through each version's real `ExtractPlugin` and
render schedule, including deferred command application. It also runs the actual
mesh-input tests and a 0.20 no-render-app removal regression. No GPU or substitute
draw components are needed for these ECS tests. Hidden draws explicitly extract
`None`, removing old data rather than merely skipping updates. Optional world-SDF
removal now explicitly removes the render resource too.

The qualification runner includes the fixture and hashes its production sources.
This first slice did not qualify effect-buffer/stage/light extraction; the next
slice below covers those payloads. Custom draw commands, pipelined rendering and
visual/performance parity, plus the editor port, remain before the coherent engine
switch. See
[implementation and evidence](extraction-2026-10-09.md).

## Effect, domain and particle-light extraction

The same fixture now imports the complete production effect-buffer/stateful,
extension-stage/field-target and particle-light selection/transport metadata.
Four additional contracts run unchanged through both engine versions' real
extraction schedules. They cover per-owner snapshots and cleanup, policy/epoch
changes, shared allocations and compiled light-artifact identity, clearing field
targets/light sources, and host-selected light/readback settings and frame values.

These are metadata/lifecycle checks, not a real 0.20 fluid solve, light selection,
asynchronous GPU copy or full render integration. Existing native shader, storage
and cluster tests remain separate and serial. No production GPU wait, particle
mirror or replay requirement is added. See
[implementation and evidence](state-extraction-2026-10-09.md).

## Renderer settings, pacing and device publication

The fixture now shares the actual renderer settings, adaptive catch-up pacer,
custom pacing extraction and device discovery/publication system. Both adapters
run the same settings/pacing lifecycle and publication-policy contracts; the
candidate also runs the unchanged production capability and pacing regressions.

With `-Native`, a fifth serial native process creates real Vulkan/Bevy renderer
resources and runs capability publication through the actual 0.20 extraction
schedule. It checks device identity/limits, backend changes and unchanged-frame
change tracking. Missing hardware fails. It does not submit a draw or establish
full renderer/pipelined/visual parity. Shipping still selects only Bevy 0.19.1.
See [implementation and evidence](control-extraction-2026-10-09.md).

## Per-view visibility and retained draw phases

The fixture now compiles the actual sampled-sprite culling and shared phase/view
module. Both shipping queue systems use the same phase-item constructors tested
against 0.20. Existing culling/selection/order regressions and new retained-entry,
camera-sort, command-metadata and CPU/GPU visibility-list contracts run unchanged
under both engine versions. Camera tests use Bevy's real projection API.

These headless checks qualify the view/phase boundary, not the entire queue system,
pipeline specialization, custom draw-command bodies or mesh/prepass bind groups.
Native checks remain the same five serial processes; they do not establish visual
parity for this view slice. See
[implementation and evidence](view-phases-2026-10-09.md).

## Production pipeline specialization and explicit layouts

The fixture now imports Aestra's actual pipeline specializer, semantic draw keys,
scene-depth layouts and portable material layout translator. The shipping renderer
uses the same modules, retaining its 0.19.1 engine graph and public material API.
The independent fixture enables the real 2D rendering plugin alongside 3D and
defaults 0.20's new per-stage pipeline-constant fields.

With `-Native`, a sixth serial native check validates 192 production descriptor
requests through Bevy's real PipelineCache, using explicit layouts rather than
inferred ones. Coverage includes 2D/3D, SDR/HDR, 1x/4x MSAA, semantic point lighting,
depth fade, blending and billboard/mesh-line diagnostic paths. Shaders compose
through the actual ShaderCache and locked WESL libraries before flat WGSL is passed
to PipelineCache. It does not submit draws or establish visual parity, semantic
triangle-list mesh/prepass binding parity, full queue integration or editor readiness.
See [implementation and evidence](pipeline-specialization-2026-10-09.md).

## Production draw commands and actual bindings

The fixture now shares actual command tuples, mesh/material/effect/depth preparation
and producer-owned draw work records with the shipping renderer. A narrow adapter
handles 0.20's depth-only prepass accessor; the shipping graph stays on 0.19.1.

With `-Native`, a seventh serial native process submits 18 draws through the real
registered DrawFunctions using actual 2D/3D view groups, 1x/4x depth prepasses,
allocator-backed indexed/nonindexed meshes and deformed wireframes. Authored lit
smoke and a fixture-textured variant exercise uniform/texture/sampler layouts.
It also verifies readiness skips, trail indirect routing, unchanged-mesh reuse and
stale-preparation cleanup. Synthetic completed producer records test command routing,
not compute dispatch. This is not full queue/schedule, visual or performance parity.
See [implementation and evidence](draw-bindings-2026-10-10.md).

## Installed queues and retained-phase lifecycle

The eighth serial native gate imports the actual render installer and both queues.
It runs normal extraction, ShaderBuffer/Image/Mesh asset preparation, embedded
WESL loading, visibility/layers, prepasses, per-view pipeline specialization and
transparent render-graph submission. Exact phase membership and submitted owners
are checked through material/geometry/visibility changes and draw/view removal.
Shared fixes retire unsupported/invisible entries and stale mesh-to-sprite geometry.

Preparation markers and fixture indirect-count writes are not production compute
dispatch. Pipelined rendering, full producers, pixels and performance remain open.
See [implementation and evidence](queue-schedule-2026-10-10.md).

## Actual alpha compute feeding installed queues

The fixture imports the unchanged production alpha-sort preparation and dispatch
code, with explicit imports and shared simulation/sort ordering sets. A ninth
serial native gate runs its AssetServer/PipelineCache compute passes before the
actual per-view render bindings and transparent commands. It verifies opposite
camera permutations, camera movement, zero/partial/full populations, reuse,
same-capacity source replacement, growth and teardown. A controlled dispatch
gate verifies that prepared-but-undispatched draws fail closed rather than use
stale indices; it is not an asynchronous shader-loading timing test.

A tenth native process runs the existing 42-case sparse-pool shader contract,
including capacity 65,537, against wgpu 30. Test-only observation/readback does
not add production COPY_SRC usages, waits or CPU particle mirrors. Simulation
inputs remain seeded fixtures; trail producers, pipelined rendering and full
visual/performance parity remain open. Shipping still uses Bevy 0.19.1.
See [implementation and evidence](alpha-compute-2026-10-10.md).

## Actual trail compaction and per-view culling feeding installed queues

The candidate imports the actual trail installers, preparation and dispatch.
Shared simulation/compaction/culling sets preserve same-frame ordering before
rendering. The bounded timestamp transport and preparation context are shared,
with narrow adapters for Bevy's resource-wrapper removal and wgpu's fallible
mapped views. The public timing API and shipping Bevy 0.19.1 graph are preserved.

Native qualification checks stable indices across the 1,024-owner prefix-page
boundary, two different camera decisions, exact indirect buffers consumed by the
installed commands, source rebinding, output reuse/growth, all three dispatch
fallback combinations, stale epochs, empty/expired histories and draw/view removal.
Real asynchronous timestamp batches preserve owner/token metadata. Only test
readbacks wait; normal playback stays GPU-resident and nonblocking.

Histories are seeded, not simulated. Controlled dispatch gates are not evidence
of asynchronous shader readiness, and synchronous headless rendering is not
pipelined, visual or performance parity. See
[implementation and evidence](trail-compute-2026-10-10.md).

## Actual analytic particles and trail history feeding installed queues

The fixture shares the shipping analytic pipeline setup, bind-group preparation,
particle dispatch and history-recording block. The actual paged dispatcher is
path-imported too; no duplicate simulation or page/merge algorithm is maintained.
Two further serial native gates start with compiler-authored records and zeroed
particle/history storage rather than seeded GPU results.

The analytic gate checks CPU-reference particles feeding actual per-view alpha
sorting. The trail gate checks inline/paged histories at 70/1,025 owners, stable
world-space samples while the emitter moves, retained dead-head tails, progressive
expiry, epoch resets, source rebinding, output reuse/growth and the exact GPU-written
indirect buffers consumed by installed draw commands. Both use `PlaybackOnly`.

Time and pose come from test controllers, not the complete host scheduler.
Stateful/routed simulation, asynchronous shader readiness, pipelined rendering,
pixels and performance still need their own gates. Shipping remains Bevy 0.19.1.
See [analytic evidence](analytic-simulation-2026-10-10.md) and
[trail-history evidence](trail-simulation-2026-10-10.md).

## Actual independent stateful simulation feeding installed queues

The candidate shares the shipping persistent-state lifecycle, twelve-binding
pipeline startup, parameter packing, independent fixed-tick scheduler and
presentation/compaction code. Compiler-authored inputs start with empty GPU
storage; native output is compared with the CPU stateful reference and consumed
by installed per-view alpha sorting and draw commands.

The gate covers bounded preview catch-up, repeated-time updates, particle death
and slot reuse, stop-emission timing, one-shot bursts, seed/capacity invalidation,
persistent allocation reuse, opposite cameras and teardown under `PlaybackOnly`
without checkpoint storage. It exposed and fixes repeated tick parameters within
a dispatch batch. The device gate now checks the actual twelve-storage-binding
layout rather than nine. No production waits or readbacks are added.

This does not qualify the coupled/routed/full host scheduler, replay, async
pipeline readiness, pipelined rendering, pixels or performance. Shipping remains
Bevy 0.19.1. See [stateful evidence](stateful-simulation-2026-10-10.md).

## Actual coupled particle events and routes feeding installed queues

Shipping and candidate now share the lockstep scheduler, shared-counter reset,
event-link gathering/spawning, input-burst consumption, output-ring aggregation
and statistics stamping. A narrow history coordination interface delegates to
the existing shipping trail observer. The engine-neutral stage executor compiles
against both wgpu versions through the existing mapped-view boundary.

The sixteenth serial native gate starts with empty GPU storage and three authored
persistent emitters. Two death links produce successive generations; host bursts
spawn at supplied event positions. It verifies link demand/acceptance counters,
output ticks/epochs and same-tick aggregation, live/free counts, exact opposite-view
alpha permutations (including ordinal ties), installed submissions, pause
idempotence, bounded catch-up, checkpoint-free playback and teardown.

Time and lowered host-route records remain fixture controlled. Fluid-domain stepping,
joint stateful trail history, full host scheduling/async output delivery, asynchronous
pipeline readiness, pipelined rendering and visual/performance parity remain open.
No production waits, particle mirrors or shader changes are added; shipping remains
Bevy 0.19.1. See [coupled evidence](coupled-simulation-2026-10-10.md).

## Actual fluid coupling and joint stateful trail history

The seventeenth serial native gate compiles the real dense-smoke fluid extension
and drives authored persistent trail heads through Follow Field. It shares the
production lockstep scheduler, stage executor, trail observer, GPU checkpoints
and historical pose upload code; no simulation or replay algorithm is duplicated.

The gate verifies evolving fluid fields and field-driven head motion, bounded
catch-up with lockstep clocks, paused-time idempotence, retained dead-head tails,
expiry, inline/paged histories at 70/1,025 owners, source replacement and teardown.
It also compares playback-only and replay-enabled results and verifies exact joint
tick-60 restoration after an epoch change. Playback-only captures no particle,
domain or trail snapshots. Installed commands consume the real per-view culling
buffers. Paged scratch is transient and excluded from persistent-history equality.

The runner hashes the full fluid extension and the imported pose/replay code too.
This is dense-smoke field-follow and joint-history qualification, not domain-emitted
particles, sparse/liquid coupling, full host/output delivery, asynchronous pipeline
readiness, pipelined rendering or visual/performance parity. Shipping stays on
Bevy 0.19.1. See [implementation and evidence](coupled-trails-2026-10-10.md).

## Actual domain births through trail queues and output rings

The eighteenth serial native gate extends the shared fluid/trail harness with
compiler-authored Secondary Emission and Spawn From Domain. The real dense-smoke
solver produces a 16-record list; ten free destination slots accept its prefix
with exact positions and inherited velocities. There are no fixture-seeded
particles or emission records. Full-capacity rejection, paused-time idempotence
and later slot reuse exercise the production scheduler and spawner.

The actual OnSpawn output ring counts accepted births and selects their lowest
ordinals; rejected births do not become cues. Real trail history feeds installed
per-view compaction/culling queues. Playback-only keeps snapshots empty; optional
joint tick-60 restoration reproduces fields and particle/trail history without
exporting duplicate births during reconstruction.

This does not qualify asynchronous delivery to host consumers, the complete
main-world host scheduler, sparse/liquid spawning, async pipeline readiness,
pipelining, pixels or performance. All reads/waits are test-only and shipping
remains Bevy 0.19.1. See [implementation and evidence](domain-spawn-2026-10-10.md).

## Actual asynchronous particle-output delivery

The nineteenth explicit serial native gate uses the actual `PresentedEffect`,
readback observer, output-context routing and delivery high-water marks. Bevy's
own `Readback::buffer` transport completes real GPU domain-birth counters and
raises messages into the main-world `AestraOutputEvent` stream. No fixture-triggered
completion or replacement delivery algorithm is used.

The gate covers wrapped-ring tick ordering, exact payload/count and nested
root/clip/epoch identity, source-tick motion and root time, world placement,
paused/repeated completions, stale epochs, restart, seek suppression, live
resumption and teardown. The existing material/presentation/context/delivery unit
tests compile against 0.20 too. Shipping API exports remain stable on Bevy 0.19.1.

Host clock/input preparation remains fixture-controlled; complete host scheduling,
stage-output delivery, async shader readiness, pipelining and visual/performance
parity remain open. The existing ring retains 32 ticks, not arbitrary stalled
history. See [implementation and evidence](async-host-outputs-2026-10-10.md).

## Asynchronous extension-stage output delivery

The twentieth serial native gate runs actual fluid pressure/force producers in
two stages, through the executor's async copy/map callback and the shared
main-world mailbox receiver. It checks force values, impact messages, owner
isolation, output-less stages, copy-and-clear, paused read gating and removal.
The moved finished-event and impact unit tests also run against 0.20.

A shared regression fixes first-completion batches overwriting sibling-stage
values or duplicating rising-edge events before deferred components are inserted.
Full player/clock/project scheduling, stage callback epoch/reordering safety,
nested stage routing, async readiness and pipelining remain separate gates.
Shipping stays on Bevy 0.19.1. See [implementation and evidence](stage-outputs-2026-10-10.md).

## Stage callback lifecycle and ordering safety

The next slice strengthens that shared transport instead of adding a substitute
candidate algorithm. Actual callback stamps retain presentation/artifact identity,
seed, playback epoch, history revision and host-input epoch. The receiver rejects
stale contexts, clears previous-context values, sorts each batch chronologically
and applies independent per-stage high-water marks. Production read gating also
includes context, allowing fresh reads at the same paused tick after a transition.

The existing native gate now holds real queued fluid callbacks across restart
and same-artifact presentation replacement, rejects them and verifies fresh
same-tick reads and later live impacts. Shared regressions cover seeks, seed and
revision changes, identity propagation, duplicate/out-of-order reads and sibling
stage isolation. No production waits or mandatory replay are added.

Full host clock/stage reset/reconstruction scheduling, reconstructed-stage event
suppression, nested stage routing, async readiness, pipelining and final
visual/performance parity remain open. Shipping still uses Bevy 0.19.1.
See [implementation and evidence](stage-output-lifecycle-2026-10-10.md).

## Canonical host clock and stage reset scheduling

The twenty-first explicit native gate installs the actual `EffectPlayer` clock,
presentation preparation/synchronization, stage input builder and stage runtime
preparation against Bevy 0.20. A narrow render-graph driver advances the actual
timelines with a four-tick budget and uses production asynchronous output delivery.
Requested ticks are derived from the player, not a fixture request map.

Restart, explicit context invalidation and presentation replacement now rebuild
compatible stage runtimes. Ordinary positive seeks retain compatible history;
live host rebinding invalidates checkpoints without discarding the live solver.
Stage force values still update during seeks, but reconstruction and the first
boundary-straddling aggregate cannot raise gameplay impacts. Later live reads can.
Player advancement no longer depends on profiler/runtime-status availability.

The gate covers normal clock advance, large-first-frame restart, bounded forward
and backward seek reconstruction, impact suppression/live resumption, context and
presentation resets, playback-only retention and teardown. It does not install
the complete host plugin, nested clip/binding reconciliation, volume presentation,
profiling or the production outer render-graph scheduler. Async readiness,
pipelining, editor compatibility and final visual/performance parity remain open.
Shipping remains Bevy 0.19.1. See [implementation and evidence](host-stage-scheduling-2026-10-10.md).

## Nested project and live binding scheduling

The twenty-second explicit native gate extends that bridge with the actual host
project reconciliation and binding capture/forwarding systems, ordered by the
canonical `AestraSet`. Two roots share one compiled project with two clip levels
and positive source offsets; their live sources produce different real GPU fluid
forces. Verify independent clocks, seed/parameter/layer inheritance, nested cue
routing, silent seek reconstruction, live resumption and clip/root retirement.

Fix surviving offset children retaining old GPU state after a root restart, and
propagate per-slot entity identities so equal-value retargets invalidate host-input
history and relatch snapshot-on-spawn bindings through every forwarded level.
Ordinary motion and positive seeks retain their compatible history behavior.

The gate remains a bounded stage driver, not the entire host plugin or production
outer graph scheduler. Async readiness/pipelining, editor migration and native
visual/performance parity remain open; shipping stays on Bevy 0.19.1.
See [implementation and evidence](project-host-scheduling-2026-10-10.md).

## Asynchronous readiness and real render-thread scheduling

The native runner also builds a separate `async-qualification` executable with
`bevy/multi_threaded`. Its delayed-shader gate retains the actual stateful pipeline
descriptors but withholds their shader asset for eight frames: no ticks or spawn
ordinals may be consumed. After the real asynchronous cache recovers, CPU/GPU
particle positions, alpha permutations, installed draw queues, repeated-time and
burst-once checks still apply.

A second gate installs Bevy's actual `PipelinedRenderingPlugin`: `RenderApp` moves
out of the main app, channel transport is present, and graph execution runs on a
different thread. Two nested roots exercise real GPU fluid forces and production
output delivery through restart, seek suppression, live resumption, paused
deduplication and shutdown. Test-only shared telemetry observes the thread without
reaching into a local render world or manually pumping its schedule.

Existing synchronous gates are retained. Both configurations have separate binary
hashes, and all native processes still run serially. This adds a one-time threaded
build variant; it does not change shipping dependencies, enable mandatory replay,
or qualify the complete host plugin/production outer graph.
See [implementation and evidence](async-pipelined-scheduling-2026-10-10.md).
