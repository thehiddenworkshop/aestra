# B20-2 slice 58: complete Curves panel and host graph composition

## Implementation

Shipping remains on Bevy **0.19.1**. The isolated **0.20.0** candidate now
constructs the actual property and host-transform Curves panels and executes
their attached graph observers.

- `curves/composition.rs` shares all eleven original workspace/property scene
  helpers, graph-key/value-label markers, list/vector selectors, ordinate labels,
  interpolation controls, scalar/vector graphs, gradients and key toolbars.
- `curves/host_composition.rs` shares the original host constructor, ten channel
  labels, graph/key markers and attached click/drag-start/drag/drag-end observers.
  The native workspace public API is reexported from its original module; native
  Curves plugin wiring, host installation and existing root tests are unchanged.
- `properties/stack_button.rs` shares the original generic compact action-button
  helper, retaining its properties API and actual tool-button scene. The candidate
  uses that helper and the existing production mini-button implementation.
- `timeline/panel_input.rs` exposes the existing version-specific pointer/cursor
  aliases once for both panels. Internal alias/test-fixture visibility expands
  only to crate scope; there is no duplicated adapter module or event algorithm.
  A new test-only drag-end builder supplies the same logical release distance
  through each upstream event representation. No runtime input policy changes.

Source audits compare the eleven helpers, complete host constructor and compact
button helper after visibility/format/trailing-comma and pointer-alias expansion.
Native Curves plugin wiring, host installation and both existing Curves/host root
test blocks compare unchanged. Existing graph geometry, styling, single-graph
query behavior, pointer ownership and cancellation policy are preserved.
Root dependency features, engine graph, toolchain and authored formats stay intact.
The candidate's superseded editing-module dead-code exemptions are removed.

## Regression coverage and limits

Eighteen new shared headless tests construct production scenes with actual
metadata, sessions, action consumers, workspace model and global history routing:

- Scalar graph/key/value-label/control structure and scrollbar targeting; empty,
  missing, hidden-source and unselected feedback in both locales; normalized output
  labels/storage; actual vector selectors, property list and Feathers Add button.
- Attached primary versus secondary key selection, scalar previews updating the
  actual raster data/nodes/value-label visibility, one final reversible release,
  vector channel isolation and gradient release preserving RGBA values.
- Actual ten-channel host panel and controls; constructed host drag start, preview,
  release and trailing-click consumption; linked quaternion retiming; Escape and
  late release; secondary-button guards. Logical geometry is checked at two scales.
- Retained identity during idle updates and retirement of the full nested control
  tree when the panel root is removed.

Preview assertions keep the authored effect unchanged. Accepted releases and Add
commands exercise real global undo/redo and one semantic history entry; source
effect files remain byte-identical. Curves starts active to avoid saving user
workspace settings through the real reveal consumer.

These fixtures supply explicit ComputedNode geometry and typed upstream pointer
events. They do not establish native layout/picking/rendering, raster image visual
acceptance, application UI rebuilding, persisted scrolling, floating windows or
full Curves plugin schedule integration. Existing single-graph/pointer-ownership
and stale/cancellation hardening remain separate from this compatibility port.

## Validation

Accepted frozen headless run (2026-10-18):
`target/bevy-020-qualification/runs/fb5d4dbee12845f48d91b56cee1c7d85/`.
The editor graph/session/history/consumer suite passes **1,168 tests**, with its
existing timing probe ignored; it takes 36.03 seconds after compilation. The
eighteen new tests also pass in isolation. Strict all-target Clippy passes on both
engines, along with both formatting checks and Git whitespace validation.

The complete run executes **1,589 checks**, with all **889 input hashes**
unchanged. Counts are executions across gates, not unique tests. Five new paths
join the inventory: property composition, its tests, host composition, the stack
button and shared panel input API. The runner confirms Bevy **0.20.0**, Rust
**1.98.1**, `native_executed: false`, and unchanged shipping lock SHA-256
`ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.

Shipping passes **2,196 editor unit + 3 architecture tests**, with ten existing
ignores unchanged. Workspace all-target checking with `aestra-bench/gpu` passes.
Root dependency/toolchain diffs remain empty and post-documentation input drift
is zero. Existing Windows incremental-cache hard-link fallback warnings are
non-fatal.

Next: full Curves plugin schedule integration, then remaining viewport/application
consumers and native editor workflows. B20-2 remains open; shipping stays on
0.19.1 until the complete compatibility release gate.
