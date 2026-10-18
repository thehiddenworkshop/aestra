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

## Focus, modal and code-editor history ownership compatibility

B20-2 slice 19 shares the actual early history routing gate, pointer/keyboard focus
observers, bounded code-buffer history and document protection state with the 0.20
candidate. Shipping action execution and menu availability use the same modal/I/O
policy. The real code-editor widget retains its existing behavior and gate; small
version-specific pointer aliases preserve native focus dispatch.

Sixteen new shared regressions cover Unicode/coalesced/bounded buffer history,
nearest/neutral focus ownership, active/empty/stale code targets, every modal branch
and busy-until-applied I/O. One existing focus test also now runs on 0.20. See
[implementation, scope and evidence](editor-history-ownership-2026-10-13.md).

The accepted final frozen headless run executes **667 checks**, including **248
graph/session/history checks** and **43 code/search/context-menu widget checks**,
with all **690 input hashes** unchanged. Strict candidate all-target Clippy with
`editor-qualification,async-qualification`, workspace all-target Clippy/check with
`aestra-bench/gpu`, and both formatting checks pass. Root dependency/toolchain/lock
diffs remain empty. No new native acceptance or complete action-observer parity is
claimed. Next: filesystem/presentation history and complete shortcut integration,
then remaining application/panel composition and native workflows.

Shipping regression: **1,381 editor unit + 3 architecture tests pass**, with ten
existing ignores unchanged. Existing Windows incremental-cache hard-link fallback
warnings do not fail these gates.

## Filesystem chronology and redo synchronization compatibility

B20-2 slice 20 imports the complete production filesystem/document ordering bridge
and its real session-backed context types. Shipping and the 0.20 candidate also
share effect-ledger synchronization, scoped presentation-redo clearing and document
order retirement. No candidate ordering algorithm or replacement journal is used.

Seventeen new shared regressions use real deletion/recovery journals and actual
material/function publication/undo to check global chronology, stale revisions,
retryable conflicts, exact restoration, project/document resets, bounded history
and scoped/global redo forks. Recovery remains available independently of the
bounded session-local undo stack. See
[implementation, scope and evidence](editor-filesystem-history-2026-10-13.md).

The accepted frozen headless run executes **684 checks**, including **265
graph/session/history checks**, with all **693 input hashes** unchanged. Strict
candidate all-target Clippy with `editor-qualification,async-qualification` and both
formatting checks pass. No new native acceptance or complete GUI-deletion/action-
observer parity is claimed. Next: compound presentation-aware undo/redo, remaining
filesystem/UI action composition and complete shortcut dispatch.

Shipping regression: **1,398 editor unit + 3 architecture tests pass**, with ten
existing ignores unchanged. Strict workspace all-target Clippy and all-target
checking with `aestra-bench/gpu`, formatting and whitespace validation pass. Root
manifest/toolchain/lockfile diffs remain empty. Existing Windows incremental-cache
hard-link fallback warnings do not fail the gates.

## Compound semantic/presentation history compatibility

B20-2 slice 21 shares the actual compound undo/redo execution block, captured
project/session resolution, ECS presentation borrows, real preview visibility/cache
data and project-wide graph inventory. Shipping calls the same sources. Raster
workers retain their existing scheduling and cache policy; no candidate validator,
preview-state substitute or widget replacement is introduced.

Fourteen new shared regressions cover single-step semantic/layout restoration for
effect/material/function history, preview visibility, layout-only state preservation,
missing/stale state rejection, failed semantic execution and retry, no-op attachment
and external source comments preserved during draft-only undo/redo. See
[implementation, scope and evidence](editor-compound-history-2026-10-13.md).

The accepted frozen headless run executes **698 checks**, including **279
graph/session/history checks**, with all **700 input hashes** unchanged. Strict
candidate all-target Clippy with `editor-qualification,async-qualification` and both
formatting checks pass. No native acceptance or complete application-observer parity
is claimed. Next: remaining application history/actions, filesystem/UI composition
and complete shortcut/native workflow acceptance.

Shipping regression: **1,412 editor unit + 3 architecture tests pass**, with ten
existing ignores unchanged. Strict workspace all-target Clippy and all-target
checking with `aestra-bench/gpu`, formatting and whitespace validation pass. Root
manifest/toolchain/lockfile diffs remain empty. Existing Windows incremental-cache
hard-link fallback warnings do not fail the gates.

## Application history routing compatibility

B20-2 slice 22 shares the actual application undo/redo router and ECS borrows with
the 0.20 candidate. Shipping keeps the existing observer event and passes the
unchanged guarded filesystem recovery worker through deferred commands.

Fourteen new shared regressions combine real focus/document registries, code/modal/
I/O ownership, global chronology, effect/material/function execution, redo forks,
compound validation and failure-safe retry. The candidate captures native deletion
dispatch with the exact journal/direction/revision; it does not substitute or claim
qualification of the full filesystem recovery worker/UI lifecycle. See
[implementation, scope and evidence](editor-history-routing-2026-10-13.md).

The accepted frozen headless run executes **712 checks**, including **293
graph/session/history checks**, with all **702 input hashes** unchanged. Strict
candidate all-target Clippy with `editor-qualification,async-qualification` and both
formatting checks pass. No new native acceptance is claimed. Next: guarded
filesystem/UI history composition, history controls and complete shortcut dispatch,
then remaining application/panel composition and native workflow acceptance.

Shipping regression: **1,426 editor unit + 3 architecture tests pass**, with ten
existing ignores unchanged. Strict workspace all-target Clippy and checking with
`aestra-bench/gpu`, formatting and whitespace validation pass. Root manifest,
toolchain and lockfile diffs remain empty.

## Guarded filesystem history execution compatibility

B20-2 slice 23 shares the actual serialized filesystem-history worker, closed-document
recovery, draft/reference checks and catalog publication with the 0.20 candidate.
Shipping browser/dialog adapters retain their existing behavior and ordering.
The candidate observes only UI callbacks, not substitutes for filesystem or catalog
logic; one regression connects the actual application router to the actual worker.

Seventeen new shared regressions cover exact restore/redo, fresh journals, deferred
publication, modal/document/catalog/order guards, retryable filesystem conflicts,
saved/unsaved/proposed references, exact draft/target/source-path recovery and
preservation of unrelated work. See
[implementation, scope and evidence](editor-filesystem-execution-2026-10-13.md).
The accepted frozen headless run executes **729 checks**, including **310
graph/session/history checks**, with all **708 input hashes** unchanged. One existing
graph timing probe remains ignored. Strict candidate all-target Clippy and both
formatting checks pass.

Shipping regression: **1,443 editor unit + 3 architecture tests pass**, with ten
existing ignores unchanged. Strict workspace all-target Clippy and checking with
`aestra-bench/gpu`, formatting and whitespace validation pass.

Only the candidate adds the already-transitive `ron 0.12` development dependency;
root dependency/toolchain/lock inputs remain unchanged. Full initial delete/restore
modal, browser UI, controls/availability and shortcut composition remain open.
No new native editor acceptance is claimed.

## Complete delete/restore dialog and browser publication compatibility

B20-2 slice 24 shares the complete production deletion coordinator and Feathers
dialog with the candidate, including actual browser/bookmark publication callbacks,
focus restoration, localized controls and scrollbar composition/visibility.
The candidate uses real Feathers plugins, theme, text measurement and UI layout
with an explicit headless window/camera; native GPU/window-loop acceptance is not
implied.

Fifteen new shared regressions cover initial deletion/manual restoration, safe
retry and stale action guards, pagination, real localized/DPI layout and scrolling,
Escape/Close focus return, busy/idle controls, actual history/bookmark integration
and recovery of unsaved material work without saving over source bytes. See
[implementation, scope and evidence](editor-deletion-dialog-2026-10-13.md).
The accepted final frozen headless run executes **744 checks**, including **325
graph/session/history checks**, with all **715 input hashes** unchanged. One existing
graph timing probe remains ignored. Shipping: **1,458 unit + 3 architecture tests
pass**, with ten existing ignores unchanged. Strict all-target Clippy passes on both
engines, as do workspace checking with `aestra-bench/gpu`, both formatting checks
and whitespace validation.

Root dependency/toolchain/lock inputs remain unchanged. Next: history controls/
availability and complete shortcuts, then remaining application/panel composition
and interactive native acceptance. Native bookmark preference/open observers and
the rest of the asset-browser plugin remain separate consumers.

## History controls, availability and keyboard ingress compatibility

B20-2 slice 25 shares actual history-button handling, availability, classification,
event-time keyboard collection and history shortcut dispatch with the 0.20 candidate.
Shipping retains its original Interaction query; the candidate uses actual native
Pressed/Hovered transitions and removals without deprecated compatibility aliases.
Actual menu state/markers and panel identities are shared production sources.

Twenty-two new regressions cover focused effect/material/function/code ownership,
global chronology and layout-only history, modal/I/O blocking, one-shot activation,
native hover/release colors, transient menu closure and real layout-aware keyboard
messages. See [implementation, scope and evidence](editor-history-controls-2026-10-13.md).

The accepted final frozen headless run executes **766 checks**, including **347
graph/session/history checks**, with all **725 input hashes** unchanged. One existing
graph timing probe remains ignored. Shipping: **1,480 unit + 3 architecture tests
pass**, with ten existing ignores unchanged. Strict all-target Clippy passes on both
engines, as do workspace checking with `aestra-bench/gpu`, both formatting checks
and whitespace validation. Root dependency/toolchain/lock inputs remain unchanged.

The complete application ShortcutContext/modal/palette/window/focus ownership and
tab-navigation are not claimed by the dispatcher's explicit blocked-signal tests.
Next: qualify those actual ownership sources and remaining keyboard/action consumers,
then full panel composition and interactive native acceptance. Shipping remains on
0.19.1 until the complete compatibility release gate passes.

## Actual shortcut ownership and tab-navigation compatibility

B20-2 slice 26 imports the complete production keyboard plugin and ShortcutContext,
using actual modal, graph-menu, extraction-dialog, module-palette, browser-surface,
window and focus state. The real history keyboard wrapper now supplies the real
ownership decision in candidate integration tests. Actual retained-popup tab
eligibility/restoration and editor root composition are shared too; tiny version
adapters select lifecycle/text-input API spellings, not policy.

Twenty-two new shared tests plus eight existing input/tab regressions qualify all
30 input cases on both engines. Real KeyboardInput and Tab/Shift+Tab messages cover
blocking, fresh-chord recovery without stale replay, ancestor/reparenting ownership,
browser-local isolation, window focus and same-frame tab eligibility. See
[implementation, scope and evidence](editor-shortcut-ownership-2026-10-13.md).

The final frozen headless run executes **796 checks**, including **377 graph/session/
history checks**, with all **740 input hashes** unchanged. One existing graph timing
probe remains ignored. Shipping: **1,502 editor unit + 3 architecture tests pass**,
with ten existing ignores unchanged. Strict all-target Clippy passes on both engines,
as do workspace checking with `aestra-bench/gpu`, both formatting checks and whitespace
validation. Root dependency/toolchain/lock inputs remain unchanged.

Next: remaining transport/timeline/viewport/material clipboard keyboard and application
action consumers, then full panel/plugin composition and native workflow acceptance.
The ownership gate does not claim qualification of every handler that consumes it.
Shipping remains on 0.19.1 until the complete compatibility release gate passes.

## Transport keyboard, actions and playback compatibility

B20-2 slice 27 shares actual transport keyboard dispatch, repeat handling, action
execution, playback advancement and their registration. The native toolbar plugin
keeps its control/scene/icon handlers and main application's system-set ordering.
No session substitute, new dependency or playback/checkpoint policy is introduced.

Nineteen new integration regressions plus seven existing transport tests cover all
26 shared transport cases: real input order, logical letters, event-time Alt/repeat
gating, actual modal/menu/focus/window ownership, no stale or doubled repeat work,
stop/restart/loop behavior and real Bevy-time advancement. See
[implementation, scope and evidence](editor-transport-playback-2026-10-13.md).

The final frozen headless run executes **822 checks**, including **403 graph/session/
history checks**, with all **744 input hashes** unchanged. One existing graph timing
probe remains ignored. Shipping: **1,521 editor unit + 3 architecture tests pass**,
with ten existing ignores unchanged. Strict all-target Clippy passes on both engines,
as do workspace checking with `aestra-bench/gpu`, both formatting checks and whitespace
validation. Root dependency inputs remain unchanged. No new native editor acceptance
is claimed.

Next: remaining timeline/viewport/material clipboard keyboard/action consumers and
transport toolbar composition, then full panel/plugin and native workflow acceptance.
Shipping remains on 0.19.1 until the complete compatibility release gate passes.

## Complete transport toolbar compatibility

B20-2 slice 28 imports the complete production transport plugin, including real
Feathers buttons, SVG loading, tooltip/accessibility content, activation handling,
icons and loop-mode selection. Version adapters retain the shipping Interaction
filter and use actual native Pressed/Hovered transitions on 0.20.
Toolbar layout and icon/mode synchronization are unchanged.

Nineteen new regressions plus existing toolbar/playback tests form **47 passing
transport checks**. Full headless UI composition covers English/French, 1x/2x DPI,
real Space/Enter and headless click events, disabled controls, SVG sharing/release,
idle identity and loop-selection undo. Integration tests exposed and fixed duplicate
Space activation and accepted actions lost during visual release on both engines.
See [implementation, scope and evidence](editor-transport-toolbar-2026-10-14.md).

The final frozen headless run executes **864 checks**, including **445 graph/session/
history checks**, with all **747 input hashes** unchanged. One existing graph timing
probe remains ignored. Shipping: **1,540 editor unit + 3 architecture tests pass**,
with ten existing ignores unchanged. Strict all-target Clippy passes on both engines,
as do workspace checking with `aestra-bench/gpu`, both formatting checks and whitespace
validation. Root dependency inputs remain unchanged. No new native-window/GPU
acceptance is claimed.

Next: remaining timeline/viewport/material clipboard keyboard and action consumers,
then full panel/main-plugin composition and native editor workflows. Shipping remains
on 0.19.1 until the complete compatibility release gate passes.

## Discrete viewport shortcut compatibility

B20-2 slice 29 shares the actual viewport shortcut system and registration, canvas
ownership marker, action payloads and system-set identities. Shipping calls the same
registration; source comparison confirms the shortcut function's behavior is unchanged.
The candidate uses the native transform-gizmo mode type, with no dependency changes.

Eighteen new shared tests cover real keyboard dispatch, grid/maximize versus hovered
framing/gizmo modes, actual modal/focus/window ownership, ordered same-frame taps,
logical letters/AZERTY number-row keys, repeat suppression, fresh-key recovery,
canvas teardown and non-interference with held WASD. See
[implementation, scope and evidence](editor-viewport-shortcuts-2026-10-14.md).

The accepted frozen headless run executes **882 checks**, including **463 graph/
session/history/consumer checks**, with all **751 input hashes** unchanged. One
existing graph timing probe remains ignored. Shipping passes **1,558 editor unit +
3 architecture tests**, with ten existing ignores unchanged. Strict all-target
Clippy passes on both engines, as do workspace checking with `aestra-bench/gpu`,
both formatting checks and whitespace validation. Root dependency inputs
remain unchanged; no native-window/GPU acceptance is claimed.

Next: timeline and material/function clipboard keyboard/action consumers, followed
by remaining viewport/application execution and full editor workflows. Continuous
camera navigation, rendered viewport execution and toolbar composition are not
claimed by this discrete-input gate. Shipping remains on 0.19.1 until B20-3 passes.

## Shared clipboard payload and function graph consumers

B20-2 slice 30 shares the immutable clipboard payload, actual per-view selection,
function graph identity/target activation, and complete function clipboard keyboard/
execution consumer. Shipping calls the same registration once. Production capture,
remapping, validation, selection and edit logic is unchanged apart from visibility
and imports; material adapters retain their current entry points.

Twenty-one new regressions and two relocated existing regressions form **23 shared
clipboard checks**. Real keyboard/session/catalog/compound-history fixtures cover
copy/cut/paste/duplicate/delete, output-protected atomic rejection, missing external
wires, placement, scoped targeting, focus/modal ownership, repeat suppression and
undo/redo. See [implementation, scope and evidence](editor-function-clipboard-2026-10-14.md).

The accepted frozen headless run executes **905 checks**, including **486 graph/
session/history/consumer checks**, with all **761 input hashes** unchanged. One
existing graph timing probe remains ignored. Shipping passes **1,579 editor unit +
3 architecture tests**, with ten existing ignores unchanged. Strict all-target
Clippy passes on both engines, as do workspace checking with `aestra-bench/gpu`,
both formatting checks and whitespace validation. Dependency inputs are unchanged.

Next: the complete material graph keyboard/selection-edit consumer, then timeline,
remaining application execution and full panel/native workflow acceptance. Material/
function menu composition and native rendering are not claimed by the shared
function-consumer gate. Shipping remains on 0.19.1 until B20-3 passes.

## Material graph keyboard and selection-edit consumers

B20-2 slice 31 shares the actual material selection-edit keyboard consumer,
clipboard adapters, semantic edit execution, view/node identities and inspector
selection. Shipping calls the same registration once. Source audits confirm the
keyboard, selection-edit/preparation/activation and clipboard bodies are unchanged
apart from imports/visibility; existing native menu tests are retained.

Fifteen new shared regressions cover real copy/cut/paste/duplicate/delete,
Ctrl+Shift+E extraction, typed connection reset, compound semantic/layout/preview
undo/redo, live-node placement, target/scope/ownership guards, logical/event-time
shortcuts, repeats and atomic rejection. See
[implementation, scope and evidence](editor-material-keyboard-2026-10-14.md).

The accepted frozen headless run executes **920 checks**, including **501 graph/
session/history/consumer checks**, with all **767 input hashes** unchanged. One
existing graph timing probe remains ignored. Shipping passes **1,594 editor unit +
3 architecture tests**, with ten existing ignores unchanged. Strict all-target
Clippy passes on both engines, as do workspace checking with `aestra-bench/gpu`,
both formatting checks and whitespace validation. Dependency inputs are unchanged.

Next: timeline keyboard/action consumers, then remaining application handlers,
graph menu/palette composition and complete editor workflows. This does not claim
native input/rendering or complete editor parity. Shipping remains on 0.19.1 until
B20-3 passes.

## Timeline marker/event document actions

B20-2 slice 32 shares the production timeline action/snap contracts and six
session-backed marker/event branches. The native observer retains its original
timeline selection cleanup and registration. Source audits confirm unchanged edit
bodies apart from the optional-localizer boundary; duration/view/region execution
remains native-owned.

Fourteen new shared checks cover playhead creation, localized marker labels,
identity/payload preservation, selection, dirty state, stale-ID rejection, mixed
global undo/redo and redo forks. Three additional shipping-only checks exercise the
actual native observer's TimelineState cleanup. See
[implementation, scope and evidence](editor-timeline-document-actions-2026-10-14.md).

The accepted frozen headless run executes **934 checks**, including **515 graph/
session/history/consumer checks**, with all **772 input hashes** unchanged. One
existing graph timing probe remains ignored. Shipping passes **1,611 editor unit +
3 architecture tests**, with ten existing ignores unchanged. Strict all-target
Clippy passes on both engines, as do workspace all-target checking with
`aestra-bench/gpu`, both formatting checks and whitespace validation. Dependency
inputs remain unchanged.

Next: timeline state/keyboard/choreography dependencies and complete consumers,
then remaining application handlers, graph menu/palette composition and full editor
workflows. This narrow branch does not qualify the full timeline observer/keyboard
or native rendering on 0.20. Shipping remains on 0.19.1 until B20-3 passes.

## Production timeline state and keyboard consumer

B20-2 slice 33 imports the full production TimelineState and choreography keyboard
consumer, with real canvas/set identities, drag/automation/nested-inspection types,
host-motion keyboard ownership, choreography action payloads and automation value
projection. Shipping retains the same chained input registration and behavior.
Source audits verify unchanged bodies apart from visibility/imports and replacing
a one-line value helper with the exact method it wrapped.

Nineteen new shared regressions cover ordered/logical/event-time shortcuts,
delete precedence, nested inspection, host-motion ownership, focus/hover/modal
guards, actual marker/event deletion/undo, curve/gradient/vector-channel insertion,
navigation restoration and normalized track ordering. Four existing state and six
existing automation-curve checks also enter the candidate gate. See
[implementation, scope and evidence](editor-timeline-keyboard-2026-10-14.md).

The accepted frozen headless run executes **963 checks**, including **544 graph/
session/history/consumer checks**, with all **785 input hashes** unchanged. One
existing graph timing probe remains ignored. Shipping passes **1,630 editor unit +
3 architecture tests**, with ten existing ignores unchanged. Strict all-target
Clippy passes on both engines, as do workspace all-target checking with
`aestra-bench/gpu`, both formatting checks and whitespace validation. Dependency
inputs remain unchanged.

Next: remaining timeline/choreography action execution and panel/plugin
composition, then remaining viewport/application and graph palette/menu consumers.
Action routing is covered, not full choreography/pose semantic execution or native
editor workflow acceptance. Shipping remains on 0.19.1 until B20-3 passes.

## Host-motion and automation action execution

B20-2 slice 34 shares the production host-motion action/numeric observers and
complete automation action branch, command builder and stored-curve sampling.
The candidate uses the full production CurvesState and host-curve transient types.
Audits verify all 23 moved contracts/functions and unchanged native motion
registration/activation; only visibility, formatting and internal type paths differ.

Twenty-three new shared regressions execute real keyboard-driven edits, pose
creation/deletion/repeat/close-loop, numeric live previews/final commits/cancellation,
loop seams, scalar/gradient/vector automation, parameter bindings, source-offset
insertion, rejection and mixed global undo/redo/forks. See
[implementation, scope and evidence](editor-timeline-execution-2026-10-14.md).

The accepted frozen headless run executes **986 checks**, including **567 graph/
session/history/consumer checks**, with all **791 input hashes** unchanged. One
existing graph timing probe remains ignored. Shipping passes **1,653 editor unit +
3 architecture tests**, with ten existing ignores unchanged. Strict all-target
Clippy passes on both engines, as do workspace all-target checking with
`aestra-bench/gpu`, both formatting checks and whitespace validation. Dependency
inputs remain unchanged.

Next: remaining top-level timeline/choreography, emitter/region and referenced-
effect actions, then complete panel/plugin composition. Native pointer/numeric
cancellation controllers, the full Curves panel and native workflow acceptance
remain separate. Shipping remains on 0.19.1 until B20-3 passes.

## Complete timeline/choreography action observers

B20-2 slice 35 imports both complete production action observers, region semantic
edits and catalog-backed nested source resolution/selection. Native registrations
stay unchanged. Audits verify all 13 moved contracts; an unused layout parameter
is removed rather than providing substitute layout state in the candidate.

Twenty-six new shared checks cover emitter edits and confirmation protection,
modifier selection, split/join/duplicate/delete regions, exact undo/redo, preview
solo/mute, nested inspection and source-opening intents, context cleanup, full
automation delegation and real keyboard-to-observer execution. See
[implementation, scope and evidence](editor-timeline-choreography-2026-10-14.md).

The accepted frozen headless run executes **1,012 checks**, including **593 graph/
session/history/consumer checks**, with all **795 input hashes** unchanged. One
existing graph timing probe remains ignored. Shipping passes **1,679 editor unit +
3 architecture tests**, with ten existing ignores unchanged. Strict all-target
Clippy passes on both engines, as do workspace all-target checking with
`aestra-bench/gpu`, both formatting checks and whitespace validation. Dependency
inputs remain unchanged.

Next: timeline pointer/navigation, popup/numeric cancellation controllers and full
timeline/Curves panel/plugin composition. Native rendering and complete document-
opening workflow acceptance are not claimed. Shipping remains on 0.19.1 until B20-3
passes.

## Timeline navigation and cancellation controllers

B20-2 slice 36 runs the actual production wheel/pan, popup dismissal/focus
restoration and host-motion drag/numeric cancellation systems on the candidate.
Native composition keeps the real pane/header/popup identities and unchanged
system registration/order. A tiny cursor boundary handles the 0.20 namespace move.
Audits verify 18 moved system/helper/type contracts without behavior changes.

Twenty-four new shared checks cover input units/ownership and scroll bounds,
anchored zoom, middle-button pan, popup hover/Escape/outside dismissal, deferred
focus restoration, timeline drag/cursor cleanup, stale host gestures, numeric
preview cancellation and late-final suppression. See
[implementation, scope and evidence](editor-timeline-navigation-2026-10-15.md).

The accepted frozen headless run executes **1,036 checks**, including **617 graph/
session/history/consumer checks**, with all **800 input hashes** unchanged.
Shipping passes **1,703 editor unit + 3 architecture tests**. Existing ignores are
unchanged. Strict all-target Clippy passes on both engines, along with workspace
all-target checking with `aestra-bench/gpu`, both formatting checks and whitespace
validation. Dependency inputs remain unchanged.

Next: timeline pointer seeking and drag gestures, then full timeline/Curves
panel/plugin composition. Explicit test geometry and drag state do not qualify
native picking/layout or drag begin/move/final-commit observers. Shipping remains
on 0.19.1 until B20-3 passes.

## Timeline seeking and region/marker/event pointer gestures

B20-2 slice 37 runs the actual seeking, control propagation stoppers and region,
marker and choreography-event drag start/move/end observers on the candidate.
Shared snap/distance/cursor helpers and real UI identities preserve production
behavior; pointer boundaries only select equivalent upstream event names.
Audits verify 28 moved observer/helper/type contracts, and native TimelinePlugin
registration/order stays unchanged during this slice.

Twenty-eight new shared checks cover seeking, hierarchy propagation, preview-only
movement, one release commit, exact undo/redo, region trimming/source offsets,
group movement/clamping and multi-selection, scaled/bounded motion, snap modes,
stale/mismatched targets, Escape/late release and anchored marker/event edits. See
[implementation, scope and evidence](editor-timeline-gestures-2026-10-15.md).

The accepted frozen headless run executes **1,064 checks**, including **645 graph/
session/history/consumer checks**, with all **806 input hashes** unchanged.
Shipping passes **1,731 editor unit + 3 architecture tests**. Existing ignores are
unchanged. Strict all-target Clippy passes on both engines, along with workspace
all-target checking with `aestra-bench/gpu`, both formatting checks and whitespace
validation. Dependency inputs remain unchanged.

Next: remaining effect-clip, automation and host-motion pointer gestures and
click/context-menu consumers, then complete timeline/Curves panel/plugin
composition. Targeted real ECS pointer events do not qualify native picking,
layout or window input. Shipping remains on 0.19.1 until B20-3 passes.

## Source-aware effect-clip pointer gestures

B20-2 slice 38 runs the actual effect-clip start/move/end observers, source-aware
trimming, snapping and final timing command on the candidate. Native construction
keeps the real control identity and unchanged attachment names. Audits verify all
eight moved observer/helper/type contracts with only visibility, formatting and
equivalent pointer aliases differing; TimelinePlugin registration/order is intact.

Twenty-three new shared checks cover real catalog source policy and missing-source
fallback, preview-only motion, scaled/bounded movement, one-shot source bounds,
looping offset wrapping/expansion, minimum duration, release/selection, exact global
undo/redo, snapping, stale/mismatched targets, Escape/late release, mid-drag deletion
and marker-anchored clips. Referenced source bytes remain unchanged. See
[implementation, scope and evidence](editor-effect-clip-gestures-2026-10-15.md).

The accepted frozen headless run executes **1,087 checks**, including **668 graph/
session/history/consumer checks**, with all **808 input hashes** unchanged.
Shipping passes **1,754 editor unit + 3 architecture tests**. Existing ignores are
unchanged. Strict all-target Clippy passes on both engines, along with workspace
all-target checking with `aestra-bench/gpu`, both formatting checks and whitespace
validation. Dependency inputs remain unchanged.

Next: automation-key/lane and host-motion gestures, remaining click/context-menu
consumers and complete timeline/Curves panel/plugin composition. Targeted upstream
events do not qualify native picking/layout/window input. Shipping remains on
0.19.1 until B20-3 passes.

## Automation gestures and live presentation

B20-2 slice 39 runs actual automation selection/double-click insertion, key and
lane gestures, source/snap projection and live graph/key/raster presentation.
Native lane discovery remains separate; real component identities and controller
attachment names stay intact. Audits verify 19 moved function/impl/type contracts
with only visibility, formatting and equivalent upstream pointer aliases differing.
Native TimelinePlugin registration/order is unchanged.

Twenty-seven new shared checks cover actual action/history/CurvesState consumers,
scalar/vector/gradient and parameter-backed edits, source offsets, neighbor bounds,
scaled motion, snapping, stale/mismatched keys, live raster/node previews, click/
drag propagation and non-semantic lane resize. See
[implementation, scope and evidence](editor-automation-gestures-2026-10-15.md).

The accepted frozen headless run executes **1,114 checks**, including **695 graph/
session/history/consumer checks**, with all **810 input hashes** unchanged.
Shipping passes **1,781 editor unit + 3 architecture tests**. Existing ignores are
unchanged. Strict all-target Clippy passes on both engines, along with workspace
all-target checking with `aestra-bench/gpu`, both formatting checks and whitespace
validation. Dependency inputs remain unchanged.

Existing automation Escape cancellation and mismatched lane-resize release
limitations are documented, not fixed or claimed safe by this compatibility slice.
Next: host-motion gestures, remaining click/context-menu consumers and complete
timeline/Curves panel/plugin composition. Native picking/layout/window input and
GPU raster output remain separate. Shipping stays on 0.19.1 until B20-3 passes.

## Host-motion gestures and in-place key visuals

B20-2 slice 40 runs actual host-pose selection, start/move/end observers,
displacement policy and key visuals on the candidate. Native composition keeps
the real control identity and unchanged attachment names. Audits verify seven
moved function/type contracts, allowing only visibility, formatting, equivalent
pointer aliases and the upstream pointer-location accessor adapter. Native
TimelinePlugin and host-motion install registration/order remain intact.

Twenty-seven shared checks cover preview-only motion, one release commit, exact
global undo/redo, sorted selection/trailing-click suppression, cancellation and
stale canonical edits, anchor/loop constraints, snapping, floating-window/canvas
routing, hierarchy propagation and stable live/idle visual components. See
[implementation, scope and evidence](editor-host-motion-gestures-2026-10-15.md).

The accepted frozen headless run executes **1,141 checks**, including **722 graph/
session/history/consumer checks**, with all **812 input hashes** unchanged.
Shipping passes **1,808 editor unit + 3 architecture tests**. Existing ignores are
unchanged. Strict all-target Clippy passes on both engines, along with workspace
all-target checking with `aestra-bench/gpu`, both formatting checks and whitespace
validation. Dependency inputs remain unchanged.

Existing host-motion active-drag routing does not capture pointer/target ownership;
no new foreign-pointer guarantee is claimed. Automation cancellation/resize
cleanup follow-ups remain separate. Next: remaining click/context-menu consumers,
then complete timeline/Curves panel/plugin composition. Native picking/layout/
window input and full workflow acceptance remain open. Shipping stays on 0.19.1
until B20-3 passes.

## Timeline clicks and context-menu intent

B20-2 slice 41 runs actual marker/event/region/effect-clip selection, source-opening
double clicks, referenced-emitter click timing, pointer/keyboard popup opening,
native header action dispatch and emitter-group collapse. Native construction
keeps controller attachment names and real identities. Audits verify 25 moved
function/type/constant contracts; changes are limited to visibility, formatting,
equivalent upstream event/marker aliases and the pointer-location accessor.
Native TimelinePlugin registration/order is unchanged.

Thirty-two shared checks cover real selection/catalog/source-intent/CurvesState/
history consumers, double-click timing and row isolation, group/region selection,
popup exclusivity/positioning, hierarchy propagation, keyboard focus/shortcut
routing, native action classification and undo. Source bytes remain unchanged.
See [implementation, scope and evidence](editor-timeline-clicks-2026-10-15.md).

The accepted frozen headless run executes **1,173 checks**, including **754 graph/
session/history/consumer checks**, with all **815 input hashes** unchanged.
Shipping passes **1,840 editor unit + 3 architecture tests**. Existing ignores are
unchanged. Strict all-target Clippy passes on both engines, along with workspace
all-target checking with `aestra-bench/gpu`, both formatting checks and whitespace
validation. Dependency inputs remain unchanged.

Stale-header menu identities and bubbled reorder-handle group collapse retain
their original policy; separate behavior hardening is documented. No native
picking/layout/window-input, full popup/panel composition or filesystem document
opening acceptance is claimed. Next: timeline action dispatch and track-color
consumers, then drop/reorder/presentation and full timeline/Curves composition.
Shipping stays on 0.19.1 until B20-3 passes.

## Timeline action dispatch and track-color controls

B20-2 slice 42 runs actual timeline/choreography activation queue/dispatch,
list-selection routing, debug classification audit and color preview/commit.
The shipping adapter retains Changed<Interaction>; the candidate uses native
Changed<Pressed> queue/clear semantics. Shared action/menu/history policies,
real component identities and native TimelinePlugin order remain intact.
Audits verify 14 moved function/type contracts with explicit query/queue/clear
adapter substitutions rather than claiming byte-identical input code.

Twenty-six shared checks cover explicit-versus-raw activation, coalescing and
repeated dispatch, transition gating, menu cleanup/revisions, invalid controls,
real emitter/effect/referenced list selection, preview-only swatch/region updates,
targeted final/default color edits, English/French status, exact global undo/redo
and debug control audits. See
[implementation, scope and evidence](editor-timeline-action-controls-2026-10-15.md).

The accepted frozen headless run executes **1,199 checks**, including **780 graph/
session/history/consumer checks**, with all **821 input hashes** unchanged.
Shipping passes **1,866 editor unit + 3 architecture tests**. Existing ignores are
unchanged. Strict all-target Clippy passes on both engines, along with workspace
all-target checking with `aestra-bench/gpu`, both formatting checks and whitespace
validation. Dependency inputs remain unchanged.

Disabled-control gating still belongs to upstream controls; None color events do
not themselves restore live fills. No new injected-Activate guard or color-preview
cancellation guarantee is claimed. Native widget/panel/input acceptance remains
separate. Next: timeline asset-drop/reorder and remaining presentation consumers,
then complete timeline/Curves integration. Shipping stays on 0.19.1 until B20-3.

## Timeline asset placement and track reorder

B20-2 slice 43 runs actual asset-drop preview/enter/leave, canvas/header placement,
compound clip/order transaction, rejection feedback/expiry and emitter/effect-clip
reorder lifecycle. The candidate now uses the complete production AssetPayload,
AuthoringDropGuard and shared ancestry/snapshot helpers with real catalog/session/
history consumers. Native identities, attachment names and plugin order remain
intact; the entire native root timeline test module compares unchanged.
Audits verify 36 moved function/type/impl/alias contracts with only visibility,
formatting, namespace resolution and equivalent pointer aliases differing.

Twenty-five shared checks cover source ancestry, placement clamping/insertion,
Undo routing without closing the material target, snapshot/modal/proposal/Escape
guards, duplicate/self/cyclic/incompatible sources, preview cleanup/feedback
expiry, mixed-track reorder/cursor transitions, exact global undo/redo and no-op/
stale/missing targets. See
[implementation, scope and evidence](editor-timeline-drop-reorder-2026-10-15.md).

The accepted frozen headless run executes **1,224 checks**, including **805 graph/
session/history/consumer checks**, with all **830 input hashes** unchanged.
Shipping passes **1,891 editor unit + 3 architecture tests**. Existing ignores are
unchanged. Strict all-target Clippy passes on both engines, along with workspace
all-target checking with `aestra-bench/gpu`, both formatting checks and whitespace
validation. Dependency inputs remain unchanged.

Reorder retains its permissive secondary-button and uncaptured pointer/target
ownership policy; canvas placement still rejects ambiguous canvases. These are
documented hardening follow-ups, not silent behavior changes. Native input/layout/
feedback rendering and full composition remain separate. Next: timeline drop-gap/
reorder hints, remaining geometry/scroll presentation, then full timeline/Curves
integration. Shipping stays on 0.19.1 until B20-3.

## Timeline presentation and scrolling

B20-2 slice 44 shares actual timeline bar/handle/marker/event/referenced-child
geometry, ruler/playhead/guide and transport labels, synchronized scrolling,
emitter reveal/focus, hover insertion/drop gaps/preview and reorder borders.
Nested catalog-backed track projections and visible automation lane validation/
height aggregation remain production implementations on both engines. Native
identities, UI attachment names, plugin order and the entire root timeline test
module remain intact. Audits verify 53 moved function/type contracts with only
visibility, formatting and explicit imports differing.

Twenty-seven new regressions cover live/stale/clipped geometry, scroll input and
clamping/priority, delayed focus, insertion cleanup/grid restoration, preview
placement/labels, reorder decoration, nested metadata and visible/custom-height
automation lanes. See
[implementation, scope and evidence](editor-timeline-presentation-2026-10-15.md).

The accepted frozen headless run executes **1,251 checks**, including **832 graph/
session/history/consumer checks**, with all **833 input hashes** unchanged.
Shipping passes **1,918 editor unit + 3 architecture tests**. Existing ignores are
unchanged. Strict all-target Clippy passes on both engines, alongside workspace
all-target checking with `aestra-bench/gpu`, both formatting checks and whitespace
validation. Dependency inputs remain unchanged.

Explicit computed geometry does not qualify native layout/picking or full UI
composition. Single-canvas/viewport queries, largest-canvas ruler spacing and
previous interaction-hardening follow-ups retain their current policies. Next:
timeline/Curves lane/track construction and full panel/plugin composition, then
remaining viewport/application consumers and native workflow acceptance.
Shipping stays on 0.19.1 until B20-3.

## Automation lane composition

B20-2 slice 45 builds actual automation headers, graphs, key controls and resize
handles with real Feathers icon buttons, attached production observers and curve
raster requests. The candidate uses registry-driven scalar/vector/gradient lane
discovery, public binding/source-bank resolution and localized labels. Actual
linked-extension registry/default and property value/label helpers are shared
from properties/module_data.rs without changing existing properties exports.
Seven extracted contracts and six newly candidate-qualified automation definitions
retain their behavior. Native plugin order and complete existing root timeline,
automation and properties test modules remain unchanged.

Twenty new checks cover both locales, metadata/value/channel resolution,
construction geometry and classification, selection, source offsets/offscreen
identity, CPU raster generation/reuse, attached pointer controls, single header
add/delete activation, globally reversible edits and non-semantic resizing.
Two existing production metadata tests also join the candidate graph. See
[implementation, scope and evidence](editor-timeline-lane-composition-2026-10-15.md).

The accepted frozen headless run executes **1,273 checks**, including **854 graph/
session/history/consumer checks**, with all **836 input hashes** unchanged.
Shipping passes **1,938 editor unit + 3 architecture tests**. Existing ignores are
unchanged. Strict all-target Clippy passes on both engines, alongside workspace
all-target checking with `aestra-bench/gpu`, both formatting checks and whitespace
validation. Dependency inputs remain unchanged.

Actual scenes/observers and CPU raster generation do not qualify native UI input,
layout/rendering, floating windows or full panel/plugin composition. Existing
interaction-hardening follow-ups remain separate. Next: remaining emitter/effect-
clip/referenced-track construction and complete timeline/Curves integration, then
remaining viewport/application consumers and native workflow acceptance.
Shipping stays on 0.19.1 until B20-3.

## Referenced-track composition

B20-2 slice 46 builds actual referenced-emitter/effect-clip headers and read-only
track rows from catalog projections, retaining list/native-control identity,
localized badges, selected/muted/disabled styling and attached production
selection/source-opening/press observers. Existing track label/color helpers and
generic Feathers tool-button composition are shared with shipping. Native plugin
order, existing root timeline/referenced-effect tests and remaining button bodies
are unchanged. Audits verify eight function contracts with only import,
visibility, formatting and equivalent namespace differences.

Twenty new shared checks cover nested/reused paths, grid/depth/trim geometry,
missing/offscreen timing, selection and source intents, disclosure behavior, both
locales and live root-clip trim compatibility. Four existing referenced checks
and one additional tool-button API probe also join candidate gates. See
[implementation, scope and evidence](editor-referenced-track-composition-2026-10-15.md).

The accepted frozen headless run executes **1,298 checks**, including **878 graph/
session/history/consumer checks**, with all **839 input hashes** unchanged.
Shipping passes **1,958 editor unit + 3 architecture tests**. Existing ignores are
unchanged. Strict all-target Clippy passes on both engines, alongside workspace
all-target checking with `aestra-bench/gpu`, both formatting checks and whitespace
validation. Dependency inputs remain unchanged.

Scene/observer/intent checks do not qualify native layout/picking/rendering,
floating windows, external source navigation or full plugin composition. Existing
hardening and nested disclosure labeling remain separate. Next: outer emitter/
root effect-clip headers and their actual menus/popovers, remaining track/toolbar
construction and full timeline/Curves integration, then viewport/application
consumers and native workflows. Shipping stays on 0.19.1 until B20-3.

## Outer effect-clip headers and popup composition

B20-2 slice 47 builds actual root effect-clip headers, reorder handles and their
five-item context menus. Complete pointer-popup constructors/observers and the
real AssetAction identity are shared with shipping. Native exports, plugin wiring
and complete existing root timeline/context-menu tests remain unchanged. Audits
verify fifteen function contracts and the action enum without behavior changes.

Twenty new shared tests cover list/control identity and geometry, selected styling,
localized inverse labels, native inline clicks versus Feathers activation,
secondary-click routing, actual popup actions and propagation, single activation
consumption, source/list intents, deletion cleanup/undo and mixed-track reorder
history. The existing authored-order versus normalized-order deletion policy is
explicitly preserved. See [implementation, scope and evidence](editor-effect-clip-header-composition-2026-10-15.md).

The accepted frozen headless run executes **1,318 checks**, including **898 graph/
session/history/consumer checks**, with all **843 input hashes** unchanged.
Shipping passes **1,978 editor unit + 3 architecture tests**. Existing ignores are
unchanged. Strict all-target Clippy passes on both engines, alongside workspace
all-target checking with `aestra-bench/gpu`, both formatting checks and whitespace
validation. Root dependency/toolchain inputs remain unchanged.

These scene/observer tests use explicit geometry, not native picking/layout/
clamping/rendering, floating windows or full timeline/Curves plugin acceptance.
Source navigation and explode host execution are not mocked or claimed qualified.
Next: outer emitter headers and actual color/automation/context popovers, remaining
track/toolbar construction and full timeline/Curves integration, then viewport/
application consumers and native workflows. Shipping stays on 0.19.1 until B20-3.

## Emitter headers and their popovers

B20-2 slice 48 builds actual emitter headers and their context, color and automation
visibility popovers. The candidate uses complete production color/numeric/text
controls and real registry-discovered lanes. Three header presentation markers
join shared interaction state. Eight extracted function contracts, native plugin
wiring and complete existing root timeline tests remain unchanged.

Twenty new shared tests cover list/native action identity, geometry and selection,
localized enabled/solo/diagnostic states, inline click versus Activate routing,
region-sensitive menu actions, semantic mute/duplicate undo, actual color live
preview/final commit/Automatic undo, lane visibility/spacers and mixed-order
reorder history. Forty-nine existing control regressions also execute in the
integrated graph gate. See [implementation, scope and evidence](editor-emitter-header-composition-2026-10-15.md).

The accepted frozen headless run executes **1,387 checks**, including **967 graph/
session/history/consumer checks**, with all **846 input hashes** unchanged.
Shipping passes **1,998 editor unit + 3 architecture tests**. Existing ignores
remain unchanged. Strict all-target Clippy passes on both engines, alongside
workspace all-target checking with `aestra-bench/gpu`, both formatting checks
and whitespace validation. Root dependency/toolchain inputs remain unchanged.

Actual controls and explicit-geometry observer tests do not qualify native input,
layout/clamping/rendering, floating windows, extraction/deletion workflows or full
timeline/Curves plugin composition. Existing hardening policies stay separate.
Next: remaining ruler/event-lane, track and toolbar construction, full timeline/
Curves integration, then viewport/application consumers and native workflows.
Shipping stays on 0.19.1 until B20-3.

## Ruler and choreography-event lane composition

B20-2 slice 49 builds actual ruler ticks, marker controls, choreography-event
lane/controls and the hidden effect-drop spacer. Three extracted constructor
contracts preserve their behavior; native plugin wiring and the complete existing
root timeline tests remain unchanged. Selection, drag, presentation and history
consumers are the actual production implementations.

Eighteen new shared tests cover retained native cue/tick identities, both locales,
selected styling and lane/spacer geometry, real presentation/zoom/offscreen and
missing-cue policy, click versus Activate and press isolation, attached marker/
event drag previews, payload-preserving commits, globally reversible history,
two DPI scales, bounds and no-motion release/cursor restoration. See
[implementation, scope and evidence](editor-ruler-cue-composition-2026-10-15.md).

The accepted frozen headless run executes **1,405 checks**, including **985 graph/
session/history/consumer checks**, with all **848 input hashes** unchanged.
Strict all-target Clippy passes on both engines, alongside workspace all-target
checking with `aestra-bench/gpu`, both formatting checks and whitespace validation.
Shipping passes **2,016 editor unit + 3 architecture tests**, with existing ignores
unchanged. Root dependency/toolchain inputs are unchanged.

These explicit-geometry scene/observer checks do not qualify native layout/picking/
rendering, floating-window routing or full timeline/Curves plugin acceptance.
Earlier interaction-hardening follow-ups stay separate. Next: remaining track/
toolbar construction and full timeline/Curves integration, then viewport/
application consumers and native workflows. Shipping stays on 0.19.1 until B20-3.

## Timeline toolbar and selection-combo composition

B20-2 slice 50 builds the production timeline toolbar and real selection-combo
controls. The toolbar expression, three helpers, generic combo option type and
default actual shell action identity retain their native contracts. Combo idle
initialization has an explicit 0.19/0.20 boundary; no deprecated compatibility
component or substitute dispatch is added on 0.20. Native plugin wiring, existing
timeline/searchable-menu tests and eight remaining combo/menu helpers are unchanged.

Twenty new shared toolbar tests exercise actual transport/timeline/history
consumers, retained SVG/control geometry, localized labels, snapping and view
state, semantic cue/duration/region edits and single activation consumption.
Three shared combo API/composition tests and one engine-specific idle-state test
per graph cover the remaining new contracts. See
[implementation, scope and evidence](editor-timeline-toolbar-composition-2026-10-15.md).

The accepted frozen headless run executes **1,429 checks**, including **1,009 graph/
session/history/consumer checks**, with all **857 input hashes** unchanged.
Shipping passes **2,040 editor unit + 3 architecture tests**, with existing ignores
unchanged. Strict all-target Clippy passes on both engines, alongside workspace
all-target checking with `aestra-bench/gpu`, both formatting checks and whitespace
validation. Root dependency/toolchain inputs are unchanged.

Typed activation/scene tests do not qualify native pointer/keyboard input, menu
focus/lifecycle, layout/rendering, floating windows or full timeline/Curves
composition. Prior hardening and full host action-menu/shell integration stay
separate. Next: remaining track construction and full timeline/Curves integration,
then viewport/application consumers and native workflows. Shipping stays on
0.19.1 until B20-3.

## Choreography track assembly

B20-2 slice 51 assembles the production choreography header and body lists:
root and nested effect clips, emitter regions, visible automation lanes and both
drop spacers. The two original expressions retain their contracts; native plugin
wiring and the complete existing root timeline tests remain unchanged.

Eighteen new shared tests exercise exact mixed/nested grid alignment and heights,
lane visibility, reused source paths, native controls, clipping and styling,
attached selection/context/press observers, and actual moves/trims with exact
timing at two DPI scales and globally reversible history. See
[implementation, scope and evidence](editor-track-composition-2026-10-18.md).

The accepted frozen headless run executes **1,447 checks**, including **1,027
graph/session/history/consumer checks**, with all **859 input hashes** unchanged.
Shipping passes **2,058 editor unit + 3 architecture tests**, with existing ignores
unchanged. Strict all-target Clippy passes on both engines, alongside workspace
all-target checking with `aestra-bench/gpu`, both formatting checks and whitespace
validation. Root dependency/toolchain inputs remain unchanged.

Explicit-geometry scene/observer checks do not qualify native picking/layout/
rendering, menu lifecycle, floating windows or complete timeline/Curves plugins.
Earlier interaction-hardening follow-ups remain separate. Next: remaining full
timeline/Curves integration, including host-motion construction and scrolling,
then viewport/application consumers and native workflows. Shipping remains
0.19.1 until B20-3.

## Host-motion lane and timeline scrollbar composition

B20-2 slice 52 builds the production host-motion header/key lane and both
timeline scrollbar gutters. The native constructors and action bridge are shared;
the existing version-specific input boundary preserves Interaction on 0.19 and
upstream Pressed insertion/removal on 0.20. Actual key gestures, semantic actions,
history, Feathers bars and scrolling consumers remain the production code.
Native plugin wiring, remaining inspector helpers and existing timeline/host
test blocks retain their contracts.

Fourteen shared tests cover real host controls, activation consumption, empty/
existing/new keys, exact retiming at two scales, global history, anchor protection,
trailing clicks and in-place visuals. Ten shared tests cover bar targets/geometry,
paired-pane scroll state, view mapping, overflow/clamping and attached gesture
isolation. See
[implementation, scope and evidence](editor-host-motion-scroll-composition-2026-10-18.md).

The accepted frozen headless run executes **1,471 checks**, including **1,051
graph/session/history/consumer checks**, with all **863 input hashes** unchanged.
Shipping passes **2,082 editor unit + 3 architecture tests**, with existing ignores
unchanged. Strict all-target Clippy passes on both engines, alongside workspace
all-target checking with `aestra-bench/gpu`, both formatting checks and whitespace
validation. Root dependency/toolchain inputs remain unchanged.

These explicit-geometry scene/observer checks do not qualify native input/layout/
rendering, scrollbar thumb input, persisted scroll memory, floating windows or
complete plugins. Earlier hardening remains separate. Next: remaining full
timeline/Curves integration and host-motion inspector construction, then viewport/
application consumers and native workflows. Shipping remains 0.19.1 until B20-3.

## Host-motion inspector composition

B20-2 slice 53 builds the actual host-motion inspector with controlled numeric
fields, motion actions, the concrete Curves link and scroll area. Four original
helpers, complete scroll-memory/Curves action identities, generic action-button
construction and real field-row/scroll wrappers are shared. Native public APIs,
plugin wiring and existing root test blocks retain their contracts.

Twenty-two shared tests cover inspector states, initialization/ranges/steps,
actual text editing and Enter, previews/validation/cancellation, time/pose edits,
constructed motion actions and global history, scroll targets and field layout.
The candidate additionally runs the existing field-row contract and a new public
button-reexport probe. See
[implementation, scope and evidence](editor-host-motion-inspector-2026-10-18.md).

The accepted frozen headless run executes **1,495 checks**, including **1,074
graph/session/history/consumer checks**, with all **869 input hashes** unchanged.
Shipping passes **2,104 editor unit + 3 architecture tests**, with existing ignores
unchanged. Strict all-target Clippy passes on both engines, alongside workspace
all-target checking with `aestra-bench/gpu`, both formatting checks and whitespace
validation. Root dependency/toolchain inputs remain unchanged.

Headless Feathers/text/input composition does not qualify native visual/pointer
acceptance, persisted scroll lifecycle, floating windows or complete plugins.
The Curves link's dock-opening consumer remains separate. Next: remaining full
timeline panel composition and Curves action/panel/plugin integration, then
viewport/application consumers and native workflows. Shipping remains 0.19.1.

## Complete timeline panel composition

B20-2 slice 54 constructs the entire production timeline tree from the actual
toolbar, track/header rows, cue/motion lanes, scroll viewports/gutters and feedback
overlays. The original constructor and five panel marker contracts are shared;
native TimelinePlugin wiring and the existing root test block remain unchanged.

Twenty-two new shared tests cover complete/empty/localized construction, paired
grid alignment, retained control identities, attached seeking/drop/drag observers,
actual toolbar/header/transport actions, global history, feedback cleanup,
scroll synchronization, idle stability and nested teardown. See
[implementation, scope and evidence](editor-timeline-panel-composition-2026-10-18.md).

The accepted frozen headless run executes **1,517 checks**, including **1,096
graph/session/history/consumer checks**, with all **871 input hashes** unchanged.
Shipping passes **2,126 editor unit + 3 architecture tests**, with existing ignores
unchanged. Strict all-target Clippy passes on both engines, alongside workspace
all-target checking with `aestra-bench/gpu`, both formatting checks and whitespace
validation. Root dependency/toolchain inputs remain unchanged.

These explicit-geometry checks do not qualify native visual layout/picking/
rendering, floating windows or complete plugin schedules. Next: TimelinePlugin
schedule integration, then remaining Curves action/panel/plugin integration and
viewport/application consumers before native workflows. Shipping remains 0.19.1.

## Curves property-editing core

B20-2 slice 56 shares nineteen original curve/gradient helpers and the existing
key-edit identity, retaining all native root functions, plugin wiring and tests.
The isolated candidate now executes scalar/vector key edits, gradient RGBA edits,
bound-parameter resolution, graph projection and preview math from production.

Twenty-seven shared tests cover local/bound values, normalized ranges, channel
isolation, pointer coordinates, stale/invalid inputs and minimum-key guards.
Accepted edits exercise real global undo/redo with one history entry; source files
remain unchanged. See
[implementation, scope and evidence](editor-curves-property-editing-2026-10-18.md).

The accepted frozen run executes **1,548 checks**, including **1,127 editor graph/
session/history/consumer checks**, with all **876 input hashes** unchanged.
Shipping passes **2,155 editor unit + 3 architecture tests**, with existing ignores
unchanged. Strict all-target Clippy passes on both engines, alongside workspace
all-target checking with `aestra-bench/gpu`, both formatting checks and whitespace
validation. Root dependency/toolchain inputs remain unchanged.

This qualifies the editing prerequisites, not complete Curves action routing or
native UI. Next: activation/keyboard/dock-opening and host-curve consumers, then
complete Curves composition/plugin integration. Shipping remains on 0.19.1.

## Curves activation, keyboard and action routing

B20-2 slice 57 shares the actual activation observer, keyboard consumer and
complete Curves dispatcher, with the original semantic action branches intact.
The native input query is retained on 0.19; 0.20 maps marker transitions/removals
to the same control phases. Fifteen host-curve helpers, the complete persisted
docking model and actual panel reveal function are shared production dependencies.
Native plugin installation, host construction and four root test blocks retain
their contracts.

Twenty-three new shared tests cover real activation consumption and repeat/idle
guards, styling ownership, property/vector/gradient commands, host-transform
actions, actual Insert/Delete focus/hover guards and globally reversible edits.
Dock model checks preserve closed/floating tab behavior. Action fixtures keep
Curves already active so the real reveal consumer does not save user settings;
closed-panel persistence and native floating windows remain separate. See
[implementation, scope and evidence](editor-curves-action-routing-2026-10-18.md).

The accepted frozen run executes **1,571 checks**, including **1,150 editor graph/
session/history/consumer checks**, with all **884 input hashes** unchanged.
Shipping passes **2,178 editor unit + 3 architecture tests**, with existing ignores
unchanged. Strict all-target Clippy passes on both engines, alongside workspace
all-target checking with `aestra-bench/gpu`, both formatting checks and whitespace
validation. Root dependency/toolchain inputs remain unchanged.
Next: complete Curves panel composition, host graph drag observers and full plugin
integration before native workflow acceptance. Shipping remains on 0.19.1.

## Complete Curves panel and host graph composition

B20-2 slice 58 builds the actual property/host-transform panels and attached
graph observers. Eleven original scene helpers, the complete host constructor,
markers/labels and generic properties stack-button helper are shared. Timeline
and Curves reuse one engine pointer/cursor boundary. Native workspace APIs,
plugin wiring and existing root tests retain their contracts.

Eighteen new shared tests cover constructed scalar/vector/gradient/host controls,
both locales and empty/error states, actual command routing, temporary raster/
node/label previews, final history, quaternion retiming, Escape/late-release and
secondary-button guards, two scales, idle identity and nested teardown. See
[implementation, scope and evidence](editor-curves-panel-composition-2026-10-18.md).

The accepted frozen qualification executes **1,589 checks**, including **1,168 editor
graph/session/history/consumer checks**, with all **889 input hashes** unchanged.
Shipping passes **2,196 editor unit + 3 architecture tests**, with existing ignores
unchanged. Strict all-target Clippy passes on both engines, alongside workspace
all-target checking with `aestra-bench/gpu`, both formatting checks and whitespace
validation. Root dependency/toolchain inputs remain unchanged.

Explicit-geometry typed-event checks do not establish native layout/picking/
rendering, application rebuilding or full plugin schedules. Next: complete Curves
plugin integration, then remaining viewport/application consumers and native
workflows. Shipping remains on 0.19.1.

## Full Curves plugin integration

B20-2 slice 59 shares the actual Curves plugin, system set and host cancellation
installer. Original state initialization, activation observer and keyboard/action
ordering remain unchanged. All 41 existing action/panel tests now install that
plugin instead of duplicating its wiring.

Three new shared tests verify resource preservation, exact executable system/set
membership and ordering, and deferred activation consumed once through the real
set gate. All 71 focused Curves tests pass. See
[implementation, scope and evidence](editor-curves-plugin-integration-2026-10-18.md).

The accepted frozen qualification executes **1,592 checks**, including **1,171
editor graph/session/history/consumer checks**, with all **892 input hashes**
unchanged. Shipping passes **2,199 editor unit + 3 architecture tests**, with
existing ignores unchanged. Strict all-target Clippy passes on both engines,
alongside workspace all-target checking with `aestra-bench/gpu`, both formatting
checks and whitespace validation. Root dependency/toolchain inputs are unchanged.

This qualifies the Curves plugin's headless schedule, not the whole application
or native layout/picking/rendering and floating-window workflows. Next: remaining
viewport execution/navigation consumers, then graph palette/menu and application
handlers before native acceptance. Shipping remains on 0.19.1.

## Production viewport camera navigation

B20-2 slice 60 shares the original navigation consumer, complete camera
controller/state, transform helper and existing camera/proxy markers. Native
ViewportPlugin registration and existing root tests are unchanged. There is no
replacement orbit algorithm or shipping input behavior change.

Eighteen shared tests (ten mirrored native regressions and eight additional edge
cases) cover RMB orbit, MMB pan, WASD fly without RMB, zoom, framing, capture,
focus/modal/Assets gating, modifier sides, real-time motion and idle transform
stability. See
[implementation, scope and evidence](editor-viewport-navigation-2026-10-18.md).

The accepted frozen qualification executes **1,610 checks**, including **1,189
editor graph/session/history/consumer checks**, with all **895 input hashes**
unchanged. Shipping passes **2,217 editor unit + 3 architecture tests**, with
existing ignores unchanged. Strict all-target Clippy passes on both engines,
alongside both formatting checks and whitespace validation. Root dependency/
toolchain inputs remain unchanged. Workspace all-target checking with
`aestra-bench/gpu` passes; post-documentation input drift is zero.

This qualifies consumer execution with actual Bevy message/state inputs, not OS
input delivery, rendered camera/viewport lifecycle, full plugin scheduling or
native editor workflows. Next: viewport action activation/execution and camera
viewport lifecycle. Shipping remains on 0.19.1.

## Viewport action activation and execution

B20-2 slice 61 shares the actual viewport action queue, dispatcher, complete
executor, display state and control audit. The native input query and registration
contract remain intact; the candidate uses changed native Pressed markers. Real
settings persistence/localized status, complete settings and photographic profile
source, and transient maximized-panel state are shared dependencies, not stand-ins.

Seventeen new shared tests cover toolbar activation/consumption, real keyboard
dispatch, actual framing navigation, all action payloads, transient versus saved
state, localized success/failure, malformed-file protection, set gating and control
audits. Presentation actions preserve the effect and semantic history. All writes
use temporary files. Fourteen existing settings and three photographic-profile
tests also run on the candidate. See
[implementation, scope and evidence](editor-viewport-actions-2026-10-18.md).

The accepted frozen qualification executes **1,644 checks**, including **1,223
editor graph/session/history/consumer checks**, with all **903 input hashes**
unchanged. Shipping passes **2,234 editor unit + 3 architecture tests**, with
existing ignores unchanged. Strict all-target Clippy passes on both engines,
alongside workspace all-target checking with `aestra-bench/gpu`, both formatting
checks and whitespace validation. Root dependency/toolchain inputs remain
unchanged and post-documentation input drift is zero.

Native toolbar composition, OS/pointer input, camera viewport lifecycle and full
plugin/application scheduling remain separate. Next: camera viewport lifecycle,
then remaining preview/render and graph/application consumers. Shipping remains
on 0.19.1.

## Camera viewport lifecycle

B20-2 slice 62 shares the three original camera viewport/activation and overlay
configuration functions unchanged. Native startup/PostUpdate wiring, all 63
remaining viewport root functions and existing root tests remain intact.

Sixteen new shared tests cover physical bounds/conversion, valid resize, invalid
or missing/ambiguous canvas state, zero-size surface restore, the original
one-valid-update activation delay, exact gizmo-layer isolation, preserved camera
composition, explicit physical geometry at nine scale combinations and primary
window ownership. See
[implementation, scope and evidence](editor-viewport-camera-lifecycle-2026-10-18.md).

The accepted frozen qualification executes **1,660 checks**, including **1,239
editor graph/session/history/consumer checks**, with all **905 input hashes**
unchanged. Shipping passes **2,250 editor unit + 3 architecture tests**, with
existing ignores unchanged. Strict all-target Clippy passes on both engines,
alongside workspace all-target checking with `aestra-bench/gpu`, both formatting
checks and whitespace validation. Root dependency/toolchain inputs remain
unchanged and post-documentation input drift is zero.

These explicit-geometry headless transitions do not establish native surface/
layout/DPI or GPU cluster/render-stage acceptance, full application scheduling or
floating-window workflows. Next: photographic preview/overlay composition, then
remaining grid/display, runtime preview and gizmo consumers. Shipping remains
on 0.19.1.

## Photographic preview and overlay composition

B20-2 slice 63 shares the original photographic-response and overlay-composition
consumers and applied-state caches. Native plugin wiring, all 61 remaining root
functions and existing viewport root tests remain unchanged. The real settings
and renderer photographic profile sources from slice 61 remain the dependencies.

Eighteen shared tests cover live normalized profiles, HDR/bloom/tonemapping and
legacy restoration, exact overlay blending/ownership, camera recreation, idle
change suppression, normalized cache equivalence, lifecycle cooperation and
unchanged effect/history. No files are written. See
[implementation, scope and evidence](editor-viewport-photographic-2026-10-18.md).

The accepted frozen qualification executes **1,678 checks**, including **1,257
editor graph/session/history/consumer checks**, with all **907 input hashes**
unchanged. Shipping passes **2,268 editor unit + 3 architecture tests**, with
existing ignores unchanged. Strict all-target Clippy passes on both engines,
alongside workspace all-target checking with `aestra-bench/gpu`, both formatting
checks and whitespace validation. Root dependency/toolchain inputs remain
unchanged and post-documentation input drift is zero.

This is component/consumer qualification, not native HDR image quality, actual
surface behavior, GPU ordering or full ViewportPlugin/application scheduling.
Next: grid/display presentation consumers, then runtime preview/gizmo integration
and native workflows. Shipping remains on 0.19.1.

## Grid/display presentation consumers

B20-2 slice 64 shares the six original grid/display/gizmo-feedback consumers and
the grid material/uniform definitions unchanged. The candidate uses the complete
production PresentedEffect payload and real material binding/layout dependencies.
Native plugin wiring, all 55 remaining viewport root functions and existing root
tests remain unchanged.

Twenty new shared tests cover grid anchoring/spacing/fade, ownership and missing/
replaced assets, real presentation display routing, SVG/text feedback and actual
viewport actions without authored edits. Eight existing renderer dependency tests
also run in the editor graph gate. A new required shader test composes and validates
the unchanged production grid shader and checks its actual material binding index
and 96-byte uniform layout. See
[implementation, scope and evidence](editor-viewport-presentation-2026-10-18.md).

The accepted frozen qualification executes **1,707 checks**, including **1,285
editor graph/session/history/consumer checks**, with all **910 input hashes**
unchanged. Shipping passes **2,288 editor unit + 3 architecture tests**, with ten
existing ignores. Strict all-target Clippy passes on both engines, alongside
workspace all-target checking with `aestra-bench/gpu`, both formatting checks and
whitespace validation. Root dependency/toolchain inputs remain unchanged and
post-documentation input drift is zero.

This does not establish native material pipeline execution, image quality, full
viewport/application scheduling or idle-write optimization. Next: runtime preview/
player setup and project-instance reconciliation, then gizmo/host-motion integration
and remaining graph/application consumers. Shipping remains on 0.19.1.

## Runtime preview/player reconciliation

B20-2 slice 65 shares the eight original player factories, project resolution,
desired-instance collection, reconciliation and live-edit compatibility functions
with their three production records. The candidate uses actual session, catalog,
timeline and complete renderer payloads. Native plugin wiring, all 47 remaining
viewport root functions and existing root tests remain unchanged.

Twenty-four shared tests (nine adapted shipping regressions and fifteen additional
tests) cover root/nested clocks, seeds, parameters and ancestry; catalog/material
refresh, missing/invalid source recovery, clip retirement/reentry, retained entity
identity, structural versus live changes, seek fidelity and half-tick tolerance.
Initial checkpoint-invalidation revision behavior is distinguished from authored
edits and semantic history. Temporary catalogs isolate filesystem fixtures. See
[implementation, scope and evidence](editor-viewport-runtime-2026-10-18.md).

The accepted frozen qualification executes **1,731 checks**, including **1,309
editor graph/session/history/consumer checks**, with all **912 input hashes**
unchanged. Shipping passes **2,312 editor unit + 3 architecture tests**, with ten
existing ignores. Strict all-target Clippy passes on both engines, alongside
workspace all-target checking with `aestra-bench/gpu`, both formatting checks and
whitespace validation. Root dependency/toolchain inputs remain unchanged and
post-documentation input drift is zero.

This is headless consumer qualification, not native simulation/rendering or full
viewport scheduling. Remaining mock binding/output/profiling consumers come next,
followed by gizmo/host-motion and graph/application integration. Shipping remains
on 0.19.1.

## Preview feedback and profiler ingestion

B20-2 slice 66 shares four original mock/input/output/drop consumers and the actual
profiler aggregation/history state. The candidate uses complete production output
messages and event-link statistics. The renderer statistics records/accumulator
move unchanged behind existing public re-exports; its native readback producer is
unchanged. Both native plugin registrations, all 43 remaining viewport functions,
remaining profiler functions and existing root tests remain intact.

Nineteen shared feedback tests and eight shared profiler-ingestion tests cover
recorded mock/input ownership, seek/replacement reuse, root output filtering and
bounded history, dropped-link UI refresh policy, missing/invalid observations,
semantic-state preservation, measurement provenance, history bounds and project
identity/peak reset. Six tests adapt existing shipping regressions. The candidate
also executes two existing output-context routing tests. See
[implementation, scope and evidence](editor-viewport-feedback-2026-10-18.md).

The accepted frozen qualification executes **1,760 checks**, including **1,338
editor graph/session/history/consumer checks**, with all **918 input hashes**
unchanged. Shipping passes **2,339 editor unit + 3 architecture tests**, with ten
existing ignores. Strict all-target Clippy passes on both engines, alongside
workspace all-target checking with `aestra-bench/gpu`, both formatting checks and
whitespace validation. Root dependency/toolchain inputs remain unchanged and
post-documentation input drift is zero.

This preserves existing policies, including root/path-only output filtering; it
does not introduce consumer epoch validation, sorting or deduplication. Next:
GPU-observation-to-profile integration, then remaining profiler presentation and
gizmo/host-motion/graph/application consumers. Native delivery/rendering and full
viewport workflows remain separate gates. Shipping remains on 0.19.1.
