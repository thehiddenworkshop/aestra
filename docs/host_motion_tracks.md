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
Its CPU measurements describe reference evaluation of every audible instance, including
clip parameter overrides, not GPU execution time. Trail counters are asynchronous native
observations; invalidated telemetry stays unavailable until a matching epoch arrives.
The viewer's `preview-report.json` uses project-wide `metrics`, adds per-path `instances`,
and includes trail capacity, occupied/retired owners, evictions and truncated histories.

Additive totals retain measured/estimated provenance; any missing contributor makes
that total unavailable. Non-trail instances contribute known zero trail usage. Native
GPU particle counts and timings are not fabricated from empty CPU sample arrays.
Capacity and buffer memory cover active instances, not the entire dependency library.
Texture memory and overdraw remain unavailable for compositions because shared assets
and overlapping draws cannot be summed accurately without further measurement.

Project peak particles are the maximum observed simultaneous total, not the sum of
independent instance peaks. Missing observations mark retained peaks as estimated
lower bounds until reset. Per-instance peaks are retained only while that path stays
active; replacement root effects reset project history. Reset Peaks resets both levels.

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
