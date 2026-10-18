# B20-2 slice 63: photographic preview and overlay composition

## Implementation

Shipping remains on Bevy **0.19.1**. The isolated **0.20.0** candidate now
executes the actual photographic response and overlay-composition consumers from
`viewport/photographic.rs`, with their original applied-state cache components.

Both system bodies are unchanged. The cache types are parent-visible for the
existing system registration; their data and behavior are unchanged. Native
ViewportPlugin registration/ordering, all 61 remaining viewport root functions and
the complete existing viewport root test block compare unchanged in source audits.
The candidate already imports the real renderer photographic profile source and
complete settings module from slice 61; this slice does not add stand-in types,
another profile algorithm or a renderer dependency graph.

The consumer still normalizes desired profiles without rewriting settings,
updates only tagged effect cameras, and skips already-applied profiles. Default UI
cameras or exact layer-15 cameras receive transparent overlay composition; tagged
effect cameras remain excluded from that overlay consumer. Fresh disabled cameras
retain their baseline components, while disabling a previously applied profile
restores the established legacy defaults rather than an arbitrary saved snapshot.
Root engine/dependency/toolchain inputs and authored formats remain unchanged.

## Regression coverage and limits

Eighteen new shared headless tests execute production consumers on both engines:

- Fresh disabled cameras retain custom baseline components/output and gain only
  cache markers. Enabled profiles apply HDR, grading, tonemapping, dither and bloom
  only to tagged effect cameras; overlays remain LDR.
- All three display transforms, live exposure/bloom edits and zero/positive bloom
  transitions use the actual photographic profile implementation. Nonfinite and
  out-of-range values normalize without changing raw settings values.
- Disabling restores legacy HDR/bloom/grading/tonemapping/dither and overlay output
  defaults. Repeated idle enabled/disabled updates do not rewrite camera/profile/
  bloom components or cache markers, as checked through actual Bevy change ticks.
  Different raw values with the same normalized profile also avoid reapplication.
- New effect cameras receive the enabled profile without refreshing old cameras;
  recreated UI/gizmo cameras receive composition. Removing cache markers permits
  explicit reapplication; this does not add automatic repair of externally edited
  components while the original cache remains present.
- Exact layer masks, default UI ownership and effect-marker precedence are
  preserved. Unrelated cameras keep their own HDR, grading, bloom and output.
  Overlay blending is the exact ALPHA_BLENDING state with transparent clears.
- Actual photographic consumers cooperate with startup overlay configuration and
  delayed viewport activation through valid/zero-size/restored transitions in the
  headless fixture. Effect data, document/UI revisions, history and status remain
  unchanged across toggles; no camera entities is also safe.

Fixtures use actual camera, photographic, settings and change-tracking components;
the lifecycle fixture supplies explicit geometry and window dimensions. No settings
or effect files are written. These checks do not establish native HDR/bloom image
quality, actual display/surface behavior, GPU render-stage/cluster ordering or
complete ViewportPlugin/application scheduling. No changed visual reference is
approved, no optional OIT/control adoption is included, and native visual/workflow
acceptance remains required before the engine swap.

## Validation

Accepted frozen headless run (2026-10-18):
`target/bevy-020-qualification/runs/81746cb9a03c4c4a92bcc31b2487a470/`.
The editor graph/session/history/consumer suite passes **1,257 tests**, with its
existing timing probe ignored, in 52.58 seconds after compilation. All eighteen
new photographic consumer tests pass, including the focused gate.

The complete run executes **1,678 checks**, with all **907 input hashes**
unchanged. Counts are executions across gates, not unique tests. The photographic
module and tests are the two new inventory paths. The runner confirms Bevy
**0.20.0**, Rust **1.98.1**, `native_executed: false`, and unchanged shipping lock
SHA-256
`ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.
Shipping passes **2,268 editor unit + 3 architecture tests**, with ten existing
ignores unchanged. Strict all-target Clippy passes on both engines, alongside
workspace all-target checking with `aestra-bench/gpu`, both formatting checks and
Git whitespace validation. Root dependency/toolchain inputs remain unchanged and
post-documentation input drift is zero. Existing Windows incremental-cache
hard-link fallback warnings are non-fatal.

Next: grid/display presentation consumers, then runtime preview/gizmo integration
and native editor workflows. B20-2 remains open; shipping stays on 0.19.1 until
the complete compatibility release gate.
