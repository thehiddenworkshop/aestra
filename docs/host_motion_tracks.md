# Deterministic host motion

`assets/effects/moving_trail_lab.aestra.ron` demonstrates an optional
`host_transform_track` on an effect. Open **Moving Trail Lab** in the editor or run:

```sh
cargo run -p aestra-viewer -- --backend gpu --semantic-materials --effect assets/effects/moving_trail_lab.aestra.ron
```

The track is an effect-transform target for the shared animation curves, relative
to a stable host placement. Position and scale each use three ordinary `Curve`
channels with independent keys; rotation uses a linked `QuaternionCurve`.
Their key times are simulation seconds, while ordinary particle/property curves
retain their normalized time domain. Each channel starts at zero with strictly
increasing finite times. Scale stays positive and rotations stay normalized.
Endpoints are held outside each channel's key range.
`repeat: true` instead repeats at the last key's time, requiring matching first
and last poses. The track period is independent of effect duration.

- Once/restart playback samples its normal simulation time. Continuous playback
  samples unwrapped elapsed time, preserving the trajectory across effect loops.
- The host entity's placement is not overwritten. World pose is
  `placement * track.sample(simulation_time)`.
- GPU trail replay uploads a separate historical pose before each 60 Hz
  observation, including an exact sub-frame target. Tracked live playback fills
  skipped ticks using the same rule. A sub-frame observation is not reused as a
  canonical replay prefix.
- Checkpoints are compatible only with the same track, placement, seed, emitter
  inputs and history revision. Replacing/removing motion invalidates observed
  history. Ordinary seeks retain compatible checkpoints.
- Mixed-renderer effects with tracked trails conservatively disable Bevy's
  current-pose frustum culling during tracked playback, since a bounded seek can
  still be drawing an earlier pose. Trail-specific GPU history culling remains active.
- The serialized track survives compilation and compiled-artifact round trips.
  Older source and compiled assets omit it and retain their previous behavior.

## Interpolation and compatibility

The Curves panel offers **Step**, **Linear**, and **Smooth** for each scalar curve.
Step holds the preceding value until the next key, Linear has a constant rate
between keys, and Smooth eases each segment using `x*x*(3-2*x)`. This setting
also controls scalar property curves, emission-rate integration, and GPU sampling.
It is a curve-wide choice, not a per-key tangent editor.
Normalized Step sampling uses a one-f32-epsilon boundary tolerance to keep CPU
and GPU lifetime-division rounding on the same side of a jump; transform curves
sample authored seconds with exact boundaries.

Rotation offers the same timing choices over shortest-arc quaternion spherical
interpolation. Its four component plots describe one orientation track; they
are not four independently interpolated scalar curves.

Old curves without an interpolation field retain Smooth. Legacy
`host_transform_track: (keys: [...], repeat: ...)` assets load into the canonical
`(curves: (translation: ..., rotation: ..., scale: ...), repeat: ...)` representation
with Linear interpolation, preserving their existing motion. Saving writes only
the canonical curves. Pose markers are a derived union of channel key times,
not a second stored animation. The legacy `HostTransformKey` type remains available
with `HostTransformTrack::from_pose_keys` for hosts constructing tracks in code.

## Editing and integration

`EffectCommand::SetHostTransformTrack` validates edits transactionally, reports a
semantic diff and supports undo/redo. The editor preview consumes the compiled
track without depending on `aestra-bevy`.

### Editor workflow

The **Host Motion** lane sits below Events. Click its heading or a pose diamond
to open the pose inspector in Properties. Use the lane's **+**, **Key at playhead**,
or **Insert** while inspecting motion to add a sampled pose at the playhead.
An existing key at that time is selected instead of duplicated. Creating a track
away from zero also creates the required time-zero anchor.

- Drag a diamond to retime it with the timeline's snapping mode, or edit **Time**
  in Properties. The first key stays at zero. Coincident key times are rejected.
- Edit position, XYZ rotation in degrees, and positive scale using the shared
  draggable Feather numeric controls. Intermediate values update the preview;
  releasing commits one undoable edit. Escape cancels the preview.
- **Edit transform curves** opens the shared Curves panel. Choose a position or
  scale channel, select its interpolation, add a key at the playhead, drag keys
  to change time/value, or delete the selected key. These operations affect only
  that channel. The graph axes remain fixed during a drag; release commits one
  undoable change and Escape cancels. Every channel retains its zero anchor.
  Rotation component views support key insertion, deletion and horizontal
  retiming; edit the orientation with the pose inspector or viewport gizmo.
- While inspecting Host Motion, the viewport shows the host trajectory and pose
  markers. Click an unselected marker to select the same key in the timeline and
  Properties without moving the playhead. The selected marker is white. Overlapping
  loop endpoints can be selected in turn; coincident interior poses are also
  directly accessible through the timeline.
- Use the viewport's Move, Rotate or Scale tools on the selected pose. These edit
  the effect transform at that marker time, not the emitter transform. Only changed
  channels receive a key; unrelated independently timed channels remain untouched.
  Shift enables precision and Ctrl enables snapping, as with emitter gizmos.
  The path and effect preview update during the drag; release commits one undo
  step, and Escape restores the original pose. Changing selection or the document
  during a drag cancels it instead of applying it to a different key.
- **Hold** keeps the final pose after the endpoint. **Repeat** requires a positive
  period and matching endpoint poses. **Close loop** explicitly copies the first
  pose to the last and enables Repeat; a one-key track gains an endpoint at the
  effect duration. In Repeat mode, editing either endpoint pose updates both.
- **Delete key** (or Delete while inspecting motion) removes the selected key.
  The zero anchor cannot be removed while other keys remain. Deletions that would
  break a repeating seam are rejected; switch to Hold to reshape the endpoints.
  **Clear motion** removes the whole track through the same undo history.
- **Frame all** includes motion endpoints beyond the effect duration without
  changing playback length. The playhead still follows the effect's playback
  range; use the key time field for motion keys outside that range.

Motion edits invalidate preview history, so subsequent scrubbing rebuilds trails
from the new trajectory. Undo/redo restores the track and its replay inputs.
To try it, open Moving Trail Lab, edit the middle pose, scrub backward, then undo.

The viewport line represents host translation relative to the editor's stable
placement, not an individual particle's trail. Rotation and scale are edited with
the gizmo but do not bend this line. Very large imported tracks use a decimated
path/marker display (at most 513 markers), always retaining both endpoints and the
selected key. The path samples the shared curve between markers (at most 2048
segments), leaving jumps at displayed Step keys disconnected; the authored track and
playback are never decimated.

Hosts can validate an immutable `aestra_runtime::CompiledHostTransformTrack` and
replace it through `EffectInstance::set_host_transform_track` or the Bevy
`EffectPlayer` method of the same name. Instances share data, not clocks or
history state. Engine-neutral particle samples remain local-space; other engine
adapters apply `host_transform_context().matrix_at(time)` when presenting them.
`host_transform_at(time)` returns only the instance's own track, not its ancestry.

## Nested clips

Open **Nested Moving Trail Lab** in the editor or viewer to try a two-level composition:
the root moves and rotates a carrier, which moves and rotates Moving Trail Lab.
Nonuniform scale and rotated clip placements exercise the complete affine chain.

```sh
cargo run -p aestra-viewer -- --backend gpu --semantic-materials --effect assets/effects/nested_moving_trail_lab.aestra.ron
```

Each presented child composes the ancestor motion, its clip placement, and its
own motion in that order. Composition stays in matrix form, preserving shear
from rotated, nonuniformly scaled parents. CPU presentation, GPU rendering and
bounds use the same full transform. `CompiledEffectProject::evaluate` exposes it
as `ProjectParticleSample::world_from_effect`; the particle itself stays local.

Clip scheduling uses the parent's loop phase. Continuous child clocks retain
unwrapped source time; restart children wrap at their own duration. Clip start,
source offset and the current loop occurrence determine a fixed inverse clock
offset for each ancestor. Every historical trail observation samples the entire
chain at those historical times, never at the parent's current pose. Source-offset
pre-roll follows that same inverse mapping, holding a track's first pose before
time zero. A new parent occurrence or child restart starts a new replay context.

Engine integrations can construct `InheritedHostTransform` with `for_child`,
using the offset returned by `CompiledEffectClip::map_instance_time`, and attach
it with `EffectInstance::set_inherited_host_transform`. Most hosts can instead
consume `CompiledEffectProject::instances`; `instances_with` supplies clip filtering,
transient timing and root motion overrides. The editor, reference evaluation and
Bevy adapter share this scheduler.
Changing any ancestor track, clip placement or clock offset invalidates the
child's history. Equivalent chains and ordinary seeks retain compatible checkpoints.

### Bevy project playback

Compile dependencies with `EffectCompiler::compile_project(&effect, &index)`,
then spawn `EffectPlayer::from_project(Arc::new(project))`. Keep controlling the
root player with the usual pause, speed, seed, seek and restart APIs. The plugin
creates and reconciles child `PresentedEffect` entities marked `EffectClipInstance`;
they do not own independent clocks. All are parented directly to the root's stable
placement, avoiding double application of animated ancestry. They inherit root
visibility, render layers and render mode, plus their clip's seed/parameter overrides.
Inactive clips disappear; removing the root player cleans up its managed instances,
and despawning the root removes the hierarchy. Separate root players remain isolated.

The viewer resolves project effects, material programs and material functions
before starting. `--semantic-materials` migrates root and dependency materials in
memory only. Texture/mesh loading uses that project's asset directory. Captures
detect trails anywhere in the dependency tree and advance the root and children
together; sequential external hosts may use `EffectPlayer::set_playback_time`,
while discontinuous jumps should use `seek_simulation_time`.

### Gameplay notifications

`AestraChoreographyEvent` includes notifications from the root and every nested clip.
Its `player` is always the root entity, `clip_path` is the stable sequence of clip IDs
(empty for root events), and `effect` identifies the compiled source. The payload's
event ID and time remain source-local. A repeated source in two clips therefore has
two distinct paths; no transient presentation entity is needed to identify it.

Events are collected from crossed intervals, not from active children at the end of
the frame. Clips that start and finish in one update still notify. Clip activation
includes an event exactly at the source offset, but never replays earlier source
events. Clip ends are inclusive. Both Restart and Continuous loops notify at each
crossed occurrence, including end events followed by time-zero events at a boundary.
Ordering is root crossing time, then clip path, with stable source order for ties.
Restart roots follow fixed-clock frame boundaries even for non-frame-aligned durations.

Normal forward playback dispatches; pause, zero speed and sub-tick updates do not.
Seeking, stepping, checkpoint restoration and `set_playback_time` clear pending
notifications and stay silent. Resuming excludes the seek target itself. Explicit
`restart()` starts a fresh sequence, including time-zero events on the first tick.
This avoids triggering gameplay/audio while scrubbing or generating captures.

Manual project integrations can use `drain_project_choreography_events` (including
root events) instead of the legacy root-only `drain_choreography_events` queue.
The portable `CompiledEffectProject::choreography_events_between` API exposes the
same interval traversal; fixed-clock hosts can use `choreography_events_for_clock_advance`.
Neither API spawns child effects or plays sounds automatically: observers interpret
the typed payloads. Particle lifecycle links continue executing within each effect.
### Project profiling

`ProjectProfiler` on each Bevy root exposes `ProjectProfile::total` and an active
`instances` list, identified by clip path and source effect ID. Read it after
`AestraSet::Profile`, following presentation preparation. The existing `EffectProfiler`
continues describing the root alone. Child activation, expiry, seeks and loop wraps
reconcile the active set; separate root players never share counters.

The editor Profiler uses the same aggregation and shows an active-instance breakdown.
Its CPU measurements describe actual CPU-reference evaluation, including clip parameter
overrides, not GPU execution time; native-GPU previews do not run duplicate CPU simulations.
Native-GPU live counts and trail counters
are asynchronous observations. Live counts reuse per-emitter draw-command counts, reading
only `16 * emitter_count + 16` bytes per observation, never particle records. The optional
trailer carries a context token, epoch and simulation time. Seeks, restarts, seed/parameter
edits, recompilation and rebuilt owners invalidate observations; late, out-of-order results
cannot replace newer counts. `GpuParticleStatistics::observation` exposes the observed time
and counts. During bounded trail replay this time can precede the requested seek target.
Invalidated telemetry stays unavailable until a matching observation arrives. CPU timing
is unavailable when no CPU evaluation ran; native-GPU live counts come from readback.
The viewer's `preview-report.json` uses project-wide `metrics`, adds per-path `instances`,
and includes trail capacity, occupied/retired owners, evictions and truncated histories.

Additive totals retain measured/estimated provenance; any missing contributor makes
that total unavailable. Non-trail instances contribute known zero trail usage. Native
GPU rendering time remains unavailable; submitted geometry uses independent draw-command telemetry.
Capacity and buffer memory cover active instances, not the entire dependency library.
Texture memory and overdraw remain unavailable for compositions because shared assets
and overlapping draws cannot be summed accurately without further measurement.

Project peak particles are the maximum observed simultaneous total, not the sum of
independent instance peaks. Missing observations mark retained peaks as estimated
lower bounds until reset. Per-instance peaks are retained only while that path stays
active; replacement root effects reset project history. Reset Peaks resets both levels.

### GPU simulation timing

`EffectProfile::gpu_simulation_time_ns` measures the GPU simulation window for one
instance: counter reset, particle simulation, ribbon linking and trail-history updates.
When replay needs several observations in one frame, the window spans the first through
last compute pass (including intervening replay copies). It excludes draw calls, CPU
evaluation, timestamp readback and final checkpoint copies. The existing `gpu_time_ns`
field is **not** populated with this partial cost. The editor labels the new metric
**GPU SIMULATION**; viewer reports add `gpu_simulation_time_ns` to totals and each instance.

The renderer checks the enabled device's `TIMESTAMP_QUERY` feature and timestamp period.
Pass-boundary queries do not require the optional inside-encoder timestamp feature or
`RenderDiagnosticsPlugin`. The existing aggregate diagnostics/Tracy span is unchanged.
Unsupported devices or pending/invalid observations report unavailable, not a CPU estimate.
Empty choreography carriers contribute known zero simulation work to project totals.

At most three timestamp batches are in flight, with at most 256 effect instances per
batch (two timestamps per instance). Backpressure skips sampling without waiting for the
GPU; no blocking poll is used. Query resolve/readback storage is bounded to 24 KiB plus
driver query storage. Only the latest completed frame reaches the main world, and missing
contributors keep project totals unavailable. Context tokens reject seeks, restarts,
parameter/seed edits, rebuilt owners and recompilation; frame sequence numbers reject
out-of-order callbacks. Despawned owners cannot receive timing data. Results are delayed
observations rather than same-frame guarantees, and project totals sum the active
instances' valid simulation windows, not the application's total GPU frame time.

### Submitted geometry

Native GPU profiles count commands that actually execute in the render pass, after view
visibility, render layers, frustum culling and pipeline/binding readiness checks. A hidden
instance contributes measured zero once its frame observation arrives. Multiple renderers
and multiple views count separately: they submit real additional work. Repeated source
effects remain isolated by their presentation owner and project clip path.

`submitted_instances` counts draw instances (trail segments are not live particles).
`submitted_vertices` counts vertex/index references: indexed mesh references can revisit
the same vertex. `submitted_primitives` counts triangles, or lines for mesh wireframe.
`draw_calls` includes issued zero-instance indirect commands. These values describe geometry
submitted before shader rejection, clipping, depth testing and rasterization; degenerate
ribbons may still be submitted. They are not visible-pixel counts,
unique vertex counts, overdraw estimates or GPU timings. CPU-reference geometry remains
unavailable rather than borrowing native-GPU observations.

Trails compact valid segments and rounded caps on the GPU after history reconstruction.
The compact list preserves owner/primitive order for alpha blending and uses the same
expiry and zero-length rules as the vertex shader, including retired tails and partially
expired anchors. Per-view culling then preserves that count or sets it to zero. Views
without bounds culling still use the compact list. While compaction pipelines load, draws
fall back to the full candidate range, which may include empty segments.
Compaction adds three render-preparation dispatches per trail renderer, shared across views;
the profiler's dispatch and buffer estimates include this cost. Simulation GPU timestamps
exclude compaction, so fewer submitted instances alone do not establish a GPU-time speedup.

The renderer copies only the first eight bytes of each indirect command and each owner's
16-byte simulation context trailer. Trail commands use the actual per-view culling or
compaction buffer; fallback direct draws use their exact CPU-issued ranges. Sampling is bounded to 256 owners,
2048 draws and three 20 KiB readback buffers. A draw-budget overflow invalidates the entire
observation; omitted owners are unavailable, never a partial measured total. Backpressure
skips sampling without blocking rendering. Only the latest completed frame is delivered.
Context tokens, sequence numbers and simulation times reject stale readbacks after seeks,
restarts, edits or owner replacement. View changes appear asynchronously, not in the same
frame as a visibility toggle. Totals sum the same frame's valid owner snapshots.

### GPU trail preparation timings

The profiler and viewer report expose `gpu_trail_compaction_time_ns` and
`gpu_trail_culling_time_ns` separately from simulation. One compaction timestamp window
encloses classification, prefixing and scattering for all trail renderers owned by an
instance. A culling window includes that instance's eligible per-view visibility dispatches.
These are GPU pass-boundary elapsed windows (including inter-pass dependencies), not CPU
submission times, individual shader instruction costs or draw/rasterization times.

Each stage reuses the non-blocking timestamp infrastructure with its own latest-frame
mailbox: at most three batches of 256 owner windows (4 KiB readback and 4 KiB resolve storage
per batch). Unsupported adapters, failed maps and owners omitted by the query budget are
unavailable. Backpressure skips measurement without blocking rendering or growing storage.
Pipeline-pending/no-eligible-work owners have no new timed window; completed empty snapshots
clear previous stage values. Non-trail native-GPU instances and empty choreography carriers
contribute known zero. CPU-reference execution does not borrow these GPU observations.
Owner/context tokens reject stale results after seeks, restarts and edits; sequence numbers
reject out-of-order completions independently for each stage. Project totals sum active paths,
preserving missing measurements rather than presenting partial totals. Stages arrive
asynchronously and must not be added together as if they were a synchronized total frame time.
Query resolution stays in the measured-work command buffer; the readback copy is submitted
in the following command buffer. This avoids stale Vulkan query copies without adding CPU
waits. Empty batches create no copy buffer, and the existing three-slot backpressure remains.
Zero timestamp endpoints are rejected as unavailable, rather than reported as zero cost.

Run the opt-in native preparation benchmark on an otherwise idle GPU:

```powershell
cargo run -p aestra-bench --features gpu --locked -- --gpu-trails preparation --seed 7
```

The harness requires timestamp queries, warms up eight iterations, then reports median and
p95 over 64 samples. It uses the production portable compaction/culling shaders, 1,024 owner
slots, 64 history points and flat caps, with 16 active owners (sparse) or 1,024 (dense), and
one/four views. It checks the resulting indirect count every iteration. Blocking waits are
confined to this explicit benchmark, never editor profiling. There is no machine-independent
performance threshold and no claim of full rendering speedup.

Example on AMD Radeon integrated graphics, Vulkan, driver 26.3.1 (2026-09-06):

| Case | Views | Candidate → compact instances | Compaction median / p95 | Culling median / p95 |
| --- | ---: | ---: | ---: | ---: |
| Sparse | 1 | 64,512 → 1,008 | 0.793 / 1.062 ms | 3.20 / 3.48 µs |
| Sparse | 4 | 64,512 → 1,008 | 0.792 / 1.805 ms | 3.28 / 3.60 µs |
| Dense | 1 | 64,512 → 64,512 | 1.607 / 4.390 ms | 3.32 / 3.52 µs |
| Dense | 4 | 64,512 → 64,512 | 1.603 / 4.395 ms | 3.36 / 3.64 µs |

Dense histories can incur compaction cost with no reduction in submitted geometry. These
results motivate measuring draw cost and any adaptive bypass before claiming a net win.

### Full-range versus compacted trail rendering

The opt-in A/B benchmark now includes the production trail vertex/alpha fragment shaders,
per-view culling, indirect drawing and target clears. It compares the full candidate range
against compaction plus drawing, with the same 1,024 owner slots and 64-point histories.
Sparse owners are noncontiguous; dense histories overlap differently colored alpha-blended
trails to detect ordering changes. Each of one/four views renders a 1,024 × 512 RGBA8 target.
Eight paired warmups precede 64 measured pairs, alternating A/B and B/A order. One synchronized
GPU timestamp window spans preparation through the last draw, with separate compaction,
culling and summed render-pass windows. Stage medians need not add up to the total median.
This measures a controlled rendering workload, not application frame time or CPU overhead.

On Windows, run with the explicit MSVC toolchain and choose a backend:

```powershell
$env:WGPU_BACKEND = 'dx12'
cargo +1.98.1-x86_64-pc-windows-msvc run -p aestra-bench --features gpu --locked -- --gpu-trails rendering --seed 7
Remove-Item Env:WGPU_BACKEND
```

The benchmark asserts indirect counts every iteration and byte-identical, nonblank images
for every case/view after warmup. Image copies, CPU comparisons and blocking map waits are
outside the GPU measurement window. Query/readback storage is reused. Incomplete or
non-monotonic timestamps fail the experiment rather than becoming zero/underflowed timings.
`WGPU_BACKEND` selects the adapter backend; absent that override it uses primary backends.
Both experiments live in `apps/aestra-bench/src/gpu_trails/`, not renderer integration
tests. The optional `gpu` feature leaves the normal CPU runner GPU-independent. Common
`--frames` (default 64), `--warmup` (8), `--seed`, `--commit` and `--out` options apply.
The supplied seed's low 32 bits populate GPU globals and are recorded explicitly. Successful
runs write JSON under `benchmarks/gpu-baselines/trails-<timestamp>/` by default; `--out`
selects a specific report path. Reports include adapter/backend/driver metadata, raw samples,
median/p95, indirect counts and image-check outcomes. Failed experiments do not write a new
report. Renderer correctness remains covered by `trail_compaction_conformance.rs` and
`trail_culling_conformance.rs`, without timing thresholds or benchmark loops.

Example on AMD Radeon integrated graphics, DirectX 12, driver 32.0.21043.5001 (2026-09-06):

| Case | Views | Full total median / p95 | Compact total median / p95 | Full / compact draw median |
| --- | ---: | ---: | ---: | ---: |
| Sparse | 1 | 0.376 / 0.417 ms | 0.908 / 0.957 ms | 0.358 / 0.046 ms |
| Sparse | 4 | 1.268 / 1.439 ms | 1.039 / 2.736 ms | 1.218 / 0.165 ms |
| Dense | 1 | 1.617 / 1.693 ms | 3.242 / 5.160 ms | 1.597 / 1.469 ms |
| Dense | 4 | 6.036 / 9.783 ms | 7.997 / 12.753 ms | 5.909 / 5.940 ms |

All four image comparisons passed. Sparse draws drop from 64,512 to 1,008 instances per
view; dense draws remain 64,512. Compaction reduced sparse four-view median total time by
about 18%, but its p95 was worse, and all other total medians regressed. This supports
investigating occupancy/view-aware bypass, not enabling a threshold from one adapter run.
Repeated runs and other GPUs/backends are needed before selecting a policy. No runtime
adaptive bypass or whole-frame profiler timing is introduced by this benchmark.

### Vulkan timestamp readback correction

The original Vulkan experiment returned zero/stale timestamps when query resolution and the
resolve-buffer-to-readback copy shared a command encoder. A reduced compute/clear/compute
probe reproduces six zero timestamps without any Aestra shaders. Moving only query resolution
to another encoder did not help; placing the **copy after resolution in a second command
buffer** fixes both the reduced probe and the full experiment. Reintroducing the original
copy placement makes the probe fail on its first iteration. This matches the synchronization
failure reported in [wgpu issue #6406](https://github.com/gfx-rs/wgpu/issues/6406); it is not
an error in trail geometry or timestamp-index arithmetic.

The fix is shared by the preparation/rendering benchmark paths and applied to the live
simulation/preparation profiler. It does not change the timed passes, insert CPU waits,
disable timestamp validation or introduce automatic backend switching. Reports now record
`timestamp_readback: "separate_command_buffer"`. Previously captured files remain historical
observations; rejected Vulkan samples were never published as valid baseline data.

Three new 8-warmup/64-sample A/B runs on **each** backend are archived in
[`benchmarks/gpu-baselines/trails-timestamp-fix-2026-09-06/`](../benchmarks/gpu-baselines/trails-timestamp-fix-2026-09-06/).
All timestamp sequences, indirect counts and exact nonblank image comparisons passed.
Sparse four-view median savings range from 1.3–2.7% on Vulkan and 17.5–20.8% on DirectX 12
on this adapter. Vulkan compact p95 is worse in all three runs. Single-view sparse and both
dense cases remain slower with compaction on both backends. These observations argue against
a universal threshold; more occupancy/view-count/adapter coverage is needed before bypass.

Run the reduced native **correctness** probe independently of benchmark sampling:

```powershell
$env:WGPU_BACKEND = 'vulkan' # repeat with 'dx12'
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-bench --features gpu --locked mixed_pass_queries -- --ignored --nocapture
Remove-Item Env:WGPU_BACKEND
```

The probe checks first-use and recycled query buffers across eight submissions; it has no
performance threshold. Unsupported adapters fail this explicitly requested diagnostic; normal
CPU-only CI does not run it. Other invalid/non-monotonic observations still fail the benchmark
without writing a report, and failed live-profiler measurements remain unavailable.

## Scope

This is explicit supplied motion, not a recorder of arbitrary live entity motion.
Seeking without a track still reconstructs history at the current placement.
Changing that placement repositions the supplied trajectory and invalidates its
checkpoints. Choreography children inherit explicit ancestor tracks, not an
unrecorded history of arbitrary external entity movement. Teleport markers,
parameter history and scene-dependent forces remain separate work.

Trail history is still sampled at 60 Hz and bounded by renderer capacities and
the shared checkpoint budget. This reconstructs the sampled trajectory, not an
analytic continuous curve. Sharp motion may require more history capacity or
different distance/adaptive sampling settings.
