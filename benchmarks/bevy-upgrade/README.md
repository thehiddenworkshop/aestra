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

## Production outer simulation graph

Shipping and the candidate now share the actual outer simulation system, particle
world upload, coupling construction and presentation-readiness marker. Three new
native gates run independent/linked stateful particles through that system into
real alpha/draw consumers and asynchronous simulation timestamps, including
delayed-shader recovery. Fix independent particles being excluded from light
eligibility because optional lockstep work counters were mistaken for readiness.

The complete plugin, outer-graph joint fluid/history and volume/light installers,
combined pipelined integration, editor compatibility and visual/performance parity
remain open. Shipping remains Bevy 0.19.1. See
[implementation and evidence](outer-simulation-graph-2026-10-11.md).

## Production outer fluid and joint history

Two more native gates use the shared outer graph and canonical stage preparer,
not the narrow fluid dispatcher: actual field-follow/solver state, inline/paged
trails, domain-born particles, real output rings and timestamped joint restore.
Missing extracted domains, unavailable timelines or emission resources must wait
without consuming ticks/births, then recover exactly. A production readiness guard
now prevents those dependencies from silently becoming link-only coupling.

The complete extension-stage/volume/light installers and combined pipelined host
integration remain open, alongside editor compatibility and final parity.
Shipping remains Bevy 0.19.1. See
[implementation and evidence](outer-fluid-history-2026-10-11.md).

## Production particle-light installer

The candidate now includes the actual light collection/extraction/preparation/
selection installer, consuming separate analytic and stateful owners from the
shared production graph. An additional native gate checks global/per-output caps,
same-frame readiness, paused/transform refresh, nested identities, replacement,
budget rejection, disable/re-enable, backend changes and teardown. No selector is
duplicated and all readbacks are test-only.

Volume presentation, host light realization, full-plugin/pipelined integration and
final parity remain open; shipping is still Bevy 0.19.1. See
[implementation and evidence](production-light-installer-2026-10-11.md).

## Production extension-stage and volume installers

The candidate now calls the real stage/volume installers, including main-world
view creation, stage inputs, extraction, actual fluid dispatch, density texture
copies and the actual PBR material draw. The version-selected shader boundary
keeps shipping WGSL unchanged and uses the qualified WESL composition for 0.20.
An additional native gate checks live field/texture agreement, nonblack material
pixels, pause/forward playback, look-edit reuse, grid rebuild, stage timestamps,
debug-slice fallback, camera re-entry and owner teardown.

This covers dense volume-only integration, not sparse/coupled volume rendering
or full-plugin/pipelined integration. Host light realization and final parity
remain open; shipping is still Bevy 0.19.1. See
[implementation and evidence](production-volume-installer-2026-10-11.md).

## Host particle-light realization

The candidate now includes the actual host light plugin and both realization modes.
Narrow version-selected extraction/WGSL-WESL boundaries keep host pooling, source
authorization, budgets and GPU injection shared. The actual asynchronous transport
uses the established fallible mapped-view boundary for wgpu 30.

A required native receiver gate verifies clamped async proxies, GPU motion/layer
refresh and reuse, transport rejection/recovery, mode transitions, same-render-
sequence injection with no additional position readbacks, actual illumination,
camera-layer/manifest rejection, hidden/despawned source retirement and teardown.
The real stage/volume installers are registered too; combined volume-light pixels,
complete-plugin and pipelined host-light integration remain open. Shipping stays
Bevy 0.19.1. See [implementation and evidence](host-light-realization-2026-10-11.md).

## Combined pipelined installers

The actual host project/binding clock now runs with the shared outer graph and
production stage/volume/light installers on Bevy's real pipelined render app.
An additional required asynchronous native gate covers two nested roots, live
fluid bindings, readiness/output convergence, both host-light modes, exact-frame
GPU injection, actual receiver pixels, stage/simulation profiling and teardown.
It also reproduces and fixes paused empty timing batches superseding in-flight
stage measurements; batches now start only when a stage advances.

This covers analytic particles and dense volumes, not a complete-plugin or
joint volume-light scattering oracle. Monolithic particle preparation/transform
wiring, editor compatibility and final parity remain open. Shipping stays on
Bevy 0.19.1. See [implementation and evidence](pipelined-installers-2026-10-11.md).

## Production clock and transform synchronization

The combined pipelined candidate now calls the actual main-world synchronization
installer. Its fixture only allocates initial analytic assets; it no longer
uploads clocks, seeds, epochs or render transforms each frame. Paused nested root
placement edits reach actual async light proxies, and buffers survive playback,
restart and seeks. Focused tests also drain 0.20 storage uploads and verify exact
matrix/context/cutoff lanes and asset reuse across repeated host updates.

Initial/dynamic particle/material preparation and draw-child wiring still need
qualification before claiming the full plugin. Shipping stays Bevy 0.19.1. See
[implementation and evidence](production-transform-sync-2026-10-11.md).

## Production particle and material preparation

The combined pipelined candidate now calls actual initial allocation and live
emitter/renderer/material preparation; its manual particle allocator is removed.
Shipping and candidate share the preparation installer, project asset cache and
trail readback setup. A version-selected generated-shader boundary preserves
shipping WGSL and creates actual 0.20 WESL assets for both sampling variants.

Shared regressions cover allocation reuse, backend/capacity gates, persistent
trail resources, readback ownership and live material edits. Candidate tests also
compile the actual generated assets through Bevy's ShaderCache and run existing
bounds, sampling, asset-root and timing regressions. Main-world draw-child/
visibility wiring, full-plugin bootstrap, editor compatibility and final parity
remain open. Shipping stays Bevy 0.19.1. See
[implementation and evidence](production-preparation-2026-10-11.md).

## Production draw-child and visibility wiring

Shipping and candidate now share the main-world installer for host-motion draw
transforms, exact global matrices, bounds/wireframe scheduling and replay/sampling
culling policy. Existing visibility regressions run on both engine graphs.
The combined threaded native gate observes actual extracted draw records and
checks paused placement centers, inherited hide/show, stable child IDs and buffer
reuse without fixture-owned visibility.

Full-plugin bootstrap, combined coupled readiness, editor compatibility, the
root dependency switch and final parity remain open. Shipping stays Bevy 0.19.1.
See [implementation and evidence](production-visibility-2026-10-11.md).

## Complete renderer and host plugin bootstrap

The combined pipelined candidate now adds the real `AestraPlugin` and opt-in host
light plugin, sharing shipping renderer/GPU/host startup instead of manually
assembling systems or assigning a backend. It also uses the actual CPU fallback,
input-event and project profiling modules. Native checks additionally require
particle pixels, submitted geometry and owner/project timing delivery; receiver
lighting measurements exclude emissive draw overdraw without changing thresholds.

Full-plugin coupled stateful/fluid/trail readiness, editor compatibility, the root
dependency switch and final parity remain open. Shipping stays Bevy 0.19.1. See
[implementation and evidence](production-plugin-bootstrap-2026-10-11.md).

## Full-plugin coupled readiness

An additional required native gate runs persistent particles, dense fluid and
paged trails through the actual threaded host/renderer plugins. An absent shader
holds all joint simulation until readiness; insertion resumes the same allocation.
Actual buffers, submitted living/dead-head trails, paused idempotence, exact-frame
seek/restart reconstruction, PlaybackOnly capture policy and teardown are checked.

The gate exposed a one-ULP clock mismatch that made valid post-seek telemetry look
like a future frame. Shared production presentation now uses the canonical host
clock, without weakening timestamp validation. Editor compatibility, the coherent
dependency switch and final parity remain open; shipping stays Bevy 0.19.1. See
[implementation and evidence](full-plugin-coupled-readiness-2026-10-11.md).

## Editor theme compatibility

The editor now shares its unchanged palette through version-selected Feathers
theme adapters. The 0.20 adapter preserves exact per-control colors across all
four surface contexts without accidentally sharing menu/button semantic fills.
A required headless gate exercises actual Feathers buttons, menu styling,
disabled text, context propagation and live theme replacement.

This begins B20-2; it is not a complete editor port or visual parity approval.
Scene/text/pointer compatibility, the root graph switch and final acceptance
remain open. Shipping stays Bevy 0.19.1. See
[implementation and evidence](editor-theme-2026-10-11.md).

## Shared editor scenes and text-input compatibility

B20-2 slice 2 shares 16 actual editor Feathers wrappers and the existing
single-line text-input plugin between shipping and candidate qualification.
Eight shared regressions exercise composition, real keyboard Unicode editing,
Enter/focus-loss submission and field identity. Two additional 0.20 checks cover
the new keyboard behavior marker and Escape propagation after focus release.
Slider values use an explicit component override compatible with both versions.

The source-frozen runner requires the new editor-controls gate and hashes its
production scene/text/plugin inputs. Numeric/color-plane scenes, pointer/cursor
compatibility, complete editor workflows and visual acceptance remain open.
This is not a complete B20-2 port or a root dependency switch. See
[implementation and evidence](editor-controls-2026-10-11.md).

Final validation (2026-10-12): **1,179 shipping editor unit + 3 architecture tests**
pass. The source-frozen native runner accepts **278 checks**, including all **34
native gates**, with **475 input hashes** and five native executable hashes
unchanged. Strict workspace/candidate lint, workspace checking and formatting pass.

## Numeric scrub pointer/cursor compatibility

B20-2 slice 3 shares the actual numeric scrub plugin with the candidate. Small
version-selected aliases cover 0.20's flat pointer events and cursor types moving
from Feathers to picking. Eight shared tests retain numeric policy and exercise
real event dispatch, text replacement, modifier stepping, clamping, propagation,
single final submission and cursor restoration. The required editor-controls
gate now contains 18 tests; its new production inputs are source-frozen.

Full numeric scene/arrow composition is still pending: 0.20's built-in handlers
must not compete with Aestra's scrub/step behavior. Color-plane composition,
other pointer consumers and full editor acceptance also remain open. See
[implementation and evidence](editor-scrub-2026-10-12.md).

Validation (2026-10-12): **1,185 editor unit + 3 architecture tests**, strict
workspace/candidate lint, workspace checking and formatting pass. The accepted
source-frozen headless run passes **252 checks**, with **480 input hashes**
unchanged. Native renderer gates were not rerun for this UI-only slice.

## Controlled numeric composition/value/arrow compatibility

B20-2 slice 4 shares the complete production numeric composition, bridge,
scrub and arrows across shipping/candidate graphs. It uses actual Feathers text
primitives with an app-owned click/scrub surface rather than composing competing
native 0.20 number handlers. Document-owned typed updates, axis labels/sigils,
hover contrast/geometry and custom semantic stepping remain available.
Selection-only events and programmatic display writes no longer duplicate
numeric changes; Enter submits on press only.

The source-frozen runner now requires `editor-numbers`: ten new shared composition
regressions plus existing arrow/scrub/text/SVG/localization checks, 46 total.
Actual numeric source, dependent catalogs and shipped icons are hashed. Native
hit-testing/visual acceptance and upstream numeric adoption remain separate;
shipping still uses Bevy 0.19.1. See
[implementation, scope and evidence](editor-numbers-2026-10-12.md).

Validation (2026-10-12): **1,195 editor unit + 3 architecture tests** pass;
strict workspace/candidate lint, workspace checking and both formatting checks
pass. The source-frozen headless run accepts **298 executed checks**, with
**540 input hashes** unchanged. No new native renderer evidence is claimed.

## Complete color-picker compatibility

B20-2 slice 5 imports the real production color picker into the 0.20 graph,
including the native plane, color sliders, numeric channels, tabs, hex editing
and Automatic reset. Channel values remain unchanged: upstream 0.20 corrects
the pointer/thumb vertical mapping to agree with the HSL gradient. Click commits
no longer duplicate native drag commits; programmatic hex display writes cannot
dirty the color or clear Automatic mode.

The required `editor-colors` gate adds twelve shared regressions alongside five
existing picker and 46 dependency checks, **63 executed checks**. The dependency
checks intentionally repeat the numeric gate. Both pointer boundaries and the
production regressions are source-frozen. Shipping remains Bevy 0.19.1; native
hit-testing and visual acceptance are not claimed. See
[implementation, scope and evidence](editor-colors-2026-10-12.md).

Validation (2026-10-12): **1,207 editor unit + 3 architecture tests** pass;
strict workspace/candidate lint and both formatting checks pass. The accepted
source-frozen headless run executes **361 checks**, with **545 input hashes**
unchanged. No new renderer-native evidence is claimed.

All-target workspace checking with `aestra-bench/gpu` also passes for slice 5.

## Custom text/search/context-menu compatibility

B20-2 slice 6 imports the production custom code editor, search fields, context
menus and button bridge into the isolated 0.20 graph. Shared pointer and action
state boundaries preserve shipping behavior while avoiding deprecated candidate
aliases. Pointer focus retains buffered focus changes; Search Clear no longer
duplicates live changes or accepts empty/disabled clear actions.

The required `editor-text` gate runs 32 checks, including ten new shared
regressions and one candidate native-pressed bridge check. Real keyboard
dispatch, text/focus/visibility schedules and Feathers controls are used, not
widget substitutes. Shipping remains Bevy 0.19.1; native workflow, OS clipboard
and visual acceptance remain open. See
[implementation, scope and evidence](editor-text-2026-10-12.md).

Validation (2026-10-12): **1,217 editor unit + 3 architecture tests** pass,
with ten existing ignores unchanged. Strict workspace/candidate lint and both
formatting checks pass. The accepted source-frozen headless run passes
**393 executed checks**, with **556 input hashes** unchanged. No new native
renderer or full-editor visual acceptance is claimed.

## Tooltip pointer/layout compatibility

B20-2 slice 7 imports the production tooltip widget into the 0.20 graph through
the shared pointer boundary. Cursor-anchored popups now account for UI scale and
physical camera viewport offsets, preserve the nearest/default camera and stay
outside the clipped graph subtree. The candidate editor feature set now includes
the default font, so the real text-wrapping gate cannot pass on empty text.

The required `editor-tooltips` gate runs six existing checks and nine new shared
lifecycle regressions: **15 checks**. It includes font-backed wrapping and all
nine 1x/1.5x/2x UI/display-scale combinations for cursor placement. Shipping
remains Bevy 0.19.1; native visual/workflow acceptance is not claimed. See
[implementation, scope and evidence](editor-tooltips-2026-10-12.md).

Validation (2026-10-12): **1,226 editor unit + 3 architecture tests** pass,
with ten existing ignores unchanged. Strict workspace/candidate lint, all-target
workspace checking with `aestra-bench/gpu` and both formatting checks pass. The
accepted source-frozen headless run passes **408 executed checks**, with all
**559 input hashes** unchanged. No new renderer-native or full-editor visual
acceptance is claimed.

## Graph navigation/material/solver compatibility

B20-2 slice 8 imports production viewport navigation, wire/grid UI materials
and bounded snapping/resize solvers into the isolated candidate. It fixes UI-scale
pan drift, foreign-window wheel input and stale cursor capture on viewport removal.
The required `editor-graph` gate executes 33 checks, with one existing manual
timing probe ignored. All graph source files and the shared resize profiling
fixture are source-frozen. The editor feature set includes real UI rendering
types; no package versions change. Shipping remains Bevy 0.19.1.

Node/marquee observers, measured geometry ownership and native graph visuals
remain open. See [implementation, scope and evidence](editor-graph-navigation-2026-10-12.md).

Validation (2026-10-12): **1,240 editor unit + 3 architecture tests** pass,
with ten existing ignores unchanged. Strict workspace/candidate lint and both
formatting checks pass. The accepted source-frozen headless run passes
**441 executed checks**, with all **587 input hashes** unchanged. No new
renderer-native or full-editor visual acceptance is claimed.

All-target workspace checking with `aestra-bench/gpu` also passes for slice 8.

## Graph marquee/node-ownership compatibility

B20-2 slice 9 imports production marquee observers, shared node state,
ancestor routing and fresh-press release guards into the isolated candidate.
It fixes scale-dependent marquee coordinates, missing final release movement,
foreign pointer ownership and stale/cancelled overlays. The required
`editor-graph` gate now executes 47 checks, including fourteen new shared
interaction regressions; one existing manual timing probe remains ignored.
A separate shipping regression covers all six actual embedded-control filters.

Full node drag transactions, measured geometry/document authority and native
graph workflows remain open. Shipping remains Bevy 0.19.1. See
[implementation, scope and evidence](editor-graph-marquee-2026-10-12.md).

Validation (2026-10-12): **1,255 editor unit + 3 architecture tests** pass;
ten existing ignores remain unchanged. Strict workspace/candidate lint,
all-target workspace checking with `aestra-bench/gpu`, both formatting checks
and whitespace validation pass. The accepted source-frozen headless run executes
**455 checks**, with all **592 input hashes** unchanged. No new native graph
or full-editor workflow acceptance is claimed.

## Graph drag/geometry-dependency compatibility

B20-2 slice 10 imports the real node drag transactions, placement memory,
document/view identities, stable geometry registry, snapping controller and
wire-insertion controller into the isolated candidate. Eleven shared interaction
tests and eight existing registry/spacing tests extend the required graph gate.
Shipping remains Bevy 0.19.1; the root dependency graph is unchanged.

Host UI geometry collection/document authority, semantic action dispatch and
native graph acceptance remain open. Delta scaling and legacy drag ownership
behavior are preserved, not claimed as native pointer qualification. See
[implementation, scope and evidence](editor-graph-drag-2026-10-12.md).

Validation (2026-10-12): **1,266 editor unit + 3 architecture tests pass**;
ten existing ignores remain unchanged. Strict workspace/candidate all-target
lint, workspace all-target checking with `aestra-bench/gpu`, both formatting
checks and whitespace validation pass. The accepted headless frozen run passes
**474 executed checks**, including **66 graph checks**, with all **602 input
hashes** unchanged. No new native editor evidence is claimed.

## Graph collection/document-authority compatibility

B20-2 slice 11 imports the actual post-layout collector and document/view
registries into the candidate. Project scope is passed directly from the host
catalog through a system pipe, without a mirrored root resource. Partial registry
teardown and inconsistent active view/document state cannot steal geometry.
Eleven shared regressions include actual UI layout at three display scales;
existing collector/document lifecycle tests also run in the candidate. A separate
shipping regression checks catalog replacement. The root graph stays unchanged.

Full panel composition, semantic action dispatch and native workflows remain
open. See [implementation, scope and evidence](editor-graph-collection-2026-10-12.md).

Validation (2026-10-12): **1,278 editor unit + 3 architecture tests pass**;
ten existing ignores remain unchanged. Strict workspace/candidate lint,
all-target workspace checking with `aestra-bench/gpu`, both formatting checks
and whitespace validation pass. The accepted frozen headless run executes
**490 checks**, including **82 graph checks**, with all **607 input hashes**
unchanged. No new native editor acceptance is claimed.

## Graph semantic preparation/history compatibility

B20-2 slice 12 imports the real material-command preparation and material-program
history resource/stack transitions. The shipping editor calls these shared
modules; catalog publication/rollback and session routing remain host-side.
Twelve new shared tests cover semantic operations, reversible extraction,
failure-safe undo/redo, history bounds and root/asset isolation. See
[implementation, scope and evidence](editor-graph-commands-2026-10-12.md).
Full UI dispatch, function/ordered/layout history integration and native editor
workflows remain open; the root engine graph is unchanged.

Validation (2026-10-12): **1,290 editor unit + 3 architecture tests pass**,
with ten existing ignores unchanged. Strict workspace/candidate all-target lint,
workspace all-target checking with `aestra-bench/gpu`, formatting and whitespace
checks pass. The accepted frozen headless run executes **502 checks**, including
**94 graph checks**, with all **613 input hashes** unchanged. No new native
editor acceptance is claimed.

## Graph activation and target-routing compatibility

B20-2 slice 13 imports production activation queue/event dispatch, the actual
history action identity and material target/view routing. Shipping calls the
same modules, retaining host menu/session/catalog policy. Fourteen new shared
tests cover activation guards, consumption and focused/stale/split/WESL routing.
A shipping regression exercises real history button execution and menu dismissal.
See [implementation, scope and evidence](editor-graph-routing-2026-10-12.md).
Full host action/history integration, panel composition and native workflows
remain open; the root engine graph is unchanged.

The accepted frozen headless run executes **522 checks**, including **108 graph
checks** and **38 text checks**, with all **620 input hashes** unchanged. Strict
candidate all-target Clippy passes. No new native editor acceptance is claimed.

Shipping validation: **1,305 editor unit + 3 architecture tests pass**, with
ten existing ignores unchanged. Strict workspace all-target Clippy, workspace
all-target checking with `aestra-bench/gpu`, both formatting checks and whitespace
validation pass. Root manifest/toolchain/lockfile diffs remain empty.

## Graph presentation transaction/order compatibility

B20-2 slice 14 imports production presentation transaction validation/apply and
per-document edit chronology. Shipping uses the same code while retaining host
semantic resolution, publication and filesystem ordering. Thirteen new shared
regressions cover context/bound isolation, compound attachment, preview state,
manual placement conflicts and stale identity rejection. See
[implementation, scope and evidence](editor-graph-history-2026-10-12.md).
Complete host semantic undo, function history and native editor workflows remain
open; no root dependency switch or optional widget replacement is included.

The accepted frozen headless run executes **535 checks**, including **121 graph
checks**, with all **628 input hashes** unchanged. Strict candidate all-target
Clippy and both formatting checks pass. No new native editor acceptance is claimed.

Shipping regression: **1,318 editor unit + 3 architecture tests pass**, with ten
existing ignores unchanged. Root manifest/toolchain/lockfile diffs remain empty.

Strict workspace all-target Clippy and all-target workspace checking with
`aestra-bench/gpu`, formatting and whitespace validation pass.

## Graph semantic snapshot/function-history compatibility

B20-2 slice 15 imports the production semantic snapshot resolver, persisted graph
identities and real function-history resource/transitions. Shipping uses the same
logic, retaining authoritative catalog inventory, source readiness, call-site
validation, publication and session chronology. Twelve new shared tests cover
compiler projection parity, identity/source guards, semantic invalidation and
failure-safe bounded/scoped function history. See
[implementation, scope and evidence](editor-graph-semantics-2026-10-12.md).
Full catalog/host execution, panel composition and native editor acceptance remain
open; root dependencies remain unchanged.

The accepted frozen headless run executes **547 checks**, including **133 graph
checks**, with all **652 input hashes** unchanged. Strict candidate all-target
Clippy and both formatting checks pass. No new native editor acceptance is claimed.

Shipping regression: **1,330 editor unit + 3 architecture tests pass**, with ten
existing ignores unchanged. Strict workspace all-target Clippy and all-target
workspace checking with `aestra-bench/gpu`, formatting and whitespace checks pass.
Root manifest/toolchain/lockfile diffs remain empty.


## Catalog-backed semantic publication compatibility

B20-2 slice 16 imports the actual editor catalog and draft store, plus production
material/function publication, project compilation and extraction/undo rollback.
Shipping uses the same modules, retaining session installation, chronology and
watch/I/O adapters. Ten new shared tests cover actual sources/drafts/compiler results,
failed publication, compensation, conflict diagnostics and history retry. The
candidate also runs five existing draft-save regressions. See
[implementation, scope and evidence](editor-publication-2026-10-12.md).
Complete session/action execution, watcher/I/O and native editor workflows remain
open; no root engine switch or widget replacement is included.

The accepted frozen headless run executes **562 checks**, including **148 graph
checks**, with all **661 input hashes** unchanged. Strict candidate all-target
Clippy and both formatting checks pass. No new native editor acceptance is claimed.

Shipping regression: **1,340 editor unit + 3 architecture tests pass**, with ten
existing ignores unchanged. Strict workspace all-target Clippy and all-target
workspace checking with `aestra-bench/gpu`, formatting and whitespace checks pass.
Root manifest/toolchain/lockfile diffs remain empty.

## Real session preview and scoped semantic-history compatibility

B20-2 slice 17 imports the actual editor session, preview installer, standalone graph
context, material/function execution, semantic history ledger and action-domain
executor. Shipping calls the same sources; complete application focus/modal/I/O,
filesystem and presentation routing remain host-owned. Twelve new shared regressions
exercise installation/failure preservation, candidate precedence, project-scoped
undo/redo, retryable failures and real extraction/save history. Existing session,
semantic-fixture and preview-mock tests now run on 0.20 too. See
[implementation, scope and evidence](editor-session-history-2026-10-12.md).
No root dependency switch, optional widget replacement or new native acceptance
is included. The next boundary is project watch/I/O and remaining action integration.

The accepted frozen headless run executes **613 checks**, including **199 graph/session
checks**, with all **675 input hashes** unchanged. Strict candidate all-target Clippy
and both formatting checks pass. No new native editor acceptance is claimed.

Shipping regression: **1,352 editor unit + 3 architecture tests pass**, with ten
existing ignores unchanged. Strict workspace all-target Clippy and all-target
workspace checking with `aestra-bench/gpu`, formatting and whitespace checks pass.
Root manifest/toolchain/lockfile diffs remain empty.

## Project watch and background I/O compatibility

B20-2 slice 18 imports actual project I/O, refresh and lifecycle scheduling into the
candidate. Shipping installs the same services, retaining native texture-root
integration. Thirteen new shared regressions cover queue draining/busy/rejection,
stale guards, preserved drafts, actual asynchronous filesystem refresh, project
switches, malformed-source recovery and I/O/watch ordering. Eleven existing refresh
tests and eight localization tests also execute on 0.20. See
[implementation, scope and evidence](editor-project-lifecycle-2026-10-12.md).

The graph/session/lifecycle gate now uses
`--features editor-qualification,async-qualification` to exercise the real
multithreaded I/O pool. Other gate feature combinations remain separate. This adds
a first-build cost for the combined feature graph, without changing shipping
dependencies or introducing a synchronous task substitute.

The accepted frozen headless run executes **645 checks**, including **231
graph/session/lifecycle checks**, with all **680 input hashes** unchanged. Strict
candidate all-target Clippy with the combined feature set and both formatting
checks pass. No new native editor acceptance is claimed.

Shipping regression: **1,365 editor unit + 3 architecture tests pass**, with ten
existing ignores unchanged. Strict workspace all-target Clippy and all-target
checking with `aestra-bench/gpu`, formatting and whitespace checks pass. Root
manifest/toolchain/lockfile diffs remain empty. Next: complete history/action
integration, remaining application/panel composition and native workflow acceptance.
