# B20-2 slice 65: runtime preview/player reconciliation

## Implementation

Shipping remains on Bevy **0.19.1**. The isolated **0.20.0** candidate now executes
the production project-resolution and preview-player reconciliation consumers in
`viewport/runtime_preview.rs`, using the real EditorSession, project catalog,
TimelineState, CompiledEffectProject and complete PresentedEffect payload.

Eight original functions are shared unchanged: configured root/instance factories,
root/child spawning, project resolution, desired-instance collection, rendered
player reconciliation and the live-edit compatibility classifier. The original
instance-path component, resolved-project resource and desired-instance record
move with them; visibility is widened only to the parent viewport where needed.
No new engine dependency, authored format or playback API is introduced.

Source audits confirm all eight function bodies and the three data definitions
are unchanged after parent-visibility normalization. Native ViewportPlugin
registration, all 47 remaining root functions and the complete existing viewport
root test block remain unchanged. Initial scene construction still calls the same
player-spawn helper. The candidate does not install the full native viewport plugin.

## Regression coverage and limits

Twenty-four shared headless tests run on both engines. Nine reproduce existing
viewport regressions; the transform-only fixture uses an explicit temporary catalog
instead of the native application's default sample-project discovery. Fifteen add
coverage for the extracted consumers. Filesystem fixtures use temporary directories.

- Real root/child construction preserves compiled identity, clock, seed, exposed
  parameter overrides, required Transform/Visibility components and render layer.
  Invalid previews are skipped by the root factory/spawn helper.
- Catalog refresh recompiles referenced effects and project materials while keeping
  an interaction-preview root. Missing references retain a valid root fallback;
  adding the source and refreshing the catalog restores nested presentation.
- An invalid root retires owned players. Failed revisions are cached until the
  document or catalog changes, and undo recovers the resolved project. Unrelated
  presentations remain untouched.
- Stable project inputs retain project/artifact and player entity identities.
  Active clip windows retire/recreate children, while sibling clips sharing an
  artifact retain separate paths, entities, local times and fixed seeds.
- Nested players carry inherited historical placement in the actual runtime
  transform context, not their entity Transform. A two-level temporary project
  agrees with reference project evaluation for leaf sample transforms.
- Seek/restart determinism, unwrapped continuous simulation time, preview-to-exact
  seek fidelity and half-tick synchronization tolerance retain existing policies.
  Changing the root seed does not overwrite a fixed child seed.
- Transform-only replacement retains the entity; structural effect/child changes
  respawn the appropriate player. Clip pose changes remain live-compatible, while
  clip timing/topology and emitter-count changes do not.
- Authored data, dirty state and exact semantic undo/redo lengths are preserved.
  Initial catalog installation still advances the runtime revision to invalidate
  simulation checkpoints; stable later reconciliation does not. This is not a new
  authored edit or history entry.

Entity retention is not a claim of idle component-write suppression or preservation
of every player-owned cache during live replacement. The original replacement,
failure and seek policies are deliberately retained. These tests do not execute
native GPU simulation, device extraction, render ordering or pixel/performance
acceptance. Full viewport scheduling, camera scene construction, mock bindings,
output/profiling consumers and gizmo/host-motion integration remain separate gates.
Timeline mute/solo and drag-timing code remains unchanged; this slice does not add
new end-to-end gesture coverage for those paths. No visual reference is approved.

## Validation

Accepted frozen headless run (2026-10-18):
`target/bevy-020-qualification/runs/f04e1bec777044ca9251e7e1f3bf479d/`.
The editor graph/session/history/consumer suite passes **1,309 tests**, with its
existing timing probe ignored, in 52.04 seconds after compilation. All twenty-four
shared runtime preview tests also pass in the focused gate.

The complete qualification executes **1,731 checks**, with all **912 input hashes**
unchanged. Counts are executions across gates, not unique tests. The production
runtime-preview module and shared test file are the two new inventory paths.
The runner confirms Bevy **0.20.0**, Rust **1.98.1**, `native_executed: false`,
and unchanged shipping lock SHA-256
`ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.

Shipping passes **2,312 editor unit + 3 architecture tests**, with ten existing
ignores unchanged. Strict all-target Clippy passes on both engines, alongside
workspace all-target checking with `aestra-bench/gpu`, both formatting checks and
Git whitespace validation. Root dependency/toolchain inputs remain unchanged;
post-documentation input drift is zero. Existing Windows incremental-cache
hard-link fallback warnings are non-fatal.

Next: remaining preview mock binding, output and profiling consumers, then
gizmo/host-motion integration and remaining graph/application consumers. B20-2
remains open; shipping stays on 0.19.1 until the complete compatibility release gate.
