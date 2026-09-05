# Deterministic host motion

`assets/effects/moving_trail_lab.aestra.ron` demonstrates an optional
`host_transform_track` on an effect. Open **Moving Trail Lab** in the editor or run:

```sh
cargo run -p aestra-viewer -- --backend gpu --semantic-materials --effect assets/effects/moving_trail_lab.aestra.ron
```

The track describes motion relative to a stable host placement. Each key has a
simulation time in seconds and an `EmitterTransform` (translation, quaternion
rotation, positive scale). Translation and scale interpolate linearly; rotation
uses shortest-arc spherical interpolation. Keys must begin at zero and have
strictly increasing finite times. Endpoints are held outside the key range.
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

Hosts can validate an immutable `aestra_runtime::CompiledHostTransformTrack` and
replace it through `EffectInstance::set_host_transform_track` or the Bevy
`EffectPlayer` method of the same name. Instances share data, not clocks or
history state. Engine-neutral particle samples remain local-space; other engine
adapters must apply `host_transform_at(time)` when presenting them.

## Scope

This is explicit supplied motion, not a recorder of arbitrary live entity motion.
Seeking without a track still reconstructs history at the current placement.
Changing that placement repositions the supplied trajectory and invalidates its
checkpoints. The track animates the presented effect instance's own emitters;
choreography child clips keep their own placements and tracks, rather than
implicitly inheriting an animated parent track. Composed parent/clip trajectories,
teleport markers, parameter history and scene-dependent forces remain separate work.

Trail history is still sampled at 60 Hz and bounded by renderer capacities and
the shared checkpoint budget. This reconstructs the sampled trajectory, not an
analytic continuous curve. Sharp motion may require more history capacity or
different distance/adaptive sampling settings.
