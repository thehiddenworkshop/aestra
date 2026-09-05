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
track without depending on `aestra-bevy`. A dedicated motion-key timeline editor
is not part of this slice; author keys in RON or through the command API.

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
