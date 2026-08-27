# ADR 0006: Native 3D particle model and format-v3 reset

- Status: accepted
- Date: 2026-08-27

## Context

The editor viewport, effect transforms, cameras, billboards, and game integration
are three-dimensional, but format v2 stored particle position, launch direction,
gravity, shape sampling, bounds, and the GPU ABI in two dimensions. A planar
simulation behind a 3D viewport prevents volumetric effects and makes semantic
shape gizmos misleading.

## Decision

Format v3 is natively three-dimensional from authored data through rendering:

- particle position, launch direction, gravity, turbulence, bounds, CPU samples,
  GPU storage, and WESL simulation use XYZ values;
- launch spread samples a solid-angle cone around a normalized 3D direction;
- circle and ring remain planar local-XY primitives;
- sphere, hemisphere, box, cylinder, and volumetric cone are first-class spawn
  shapes oriented in effect-local space;
- every emitter owns a persisted local translation, quaternion rotation, and
  positive XYZ scale that are applied consistently by CPU and GPU execution;
- the editor exposes all shapes through one choice control and draws direct 3D
  wireframe handles for their dimensions; standard transform gizmos edit emitter
  transforms through preview transactions and one undoable command per drag;
- only `CURRENT_FORMAT_VERSION` is accepted. Format v2 has no compatibility
  parser or runtime branch.

The deterministic CPU evaluator remains the conformance oracle. The GPU ABI and
WESL sources must evolve in the same change and retain shader-validation tests.

## Consequences

Format-v2 effects must be recreated or explicitly converted outside the runtime.
The format reset is intentional while Aestra remains pre-stable. Effects can now
occupy real depth, react to gravity on every axis, and render correctly while the
editor camera orbits the scene.
