# B20-2 slice 56: Curves property-editing core

## Implementation

Shipping remains on Bevy **0.19.1**. The isolated **0.20.0** candidate now runs
the production scalar/vector curve and gradient editing helpers needed by the
remaining Curves action consumers.

`curves/property_editing.rs` shares nineteen original helpers and the existing
ComplexKeyEdit identity: active/bound value resolution, scalar/vector mutations,
pointer insertion, key edits, bounds, sampling, graph projection, output labels
and non-mutating drag-preview math. The native module imports the same helpers;
no authoring behavior or format changes. The candidate uses a bounded Curves
namespace with the actual state and action identity, and exposes the existing
numeric formatter through its existing numeric-widget API.

Source audits compare all nineteen helpers and the enum after visibility/format
normalization. All fourteen remaining root functions compare unchanged, as do
the native Curves plugin installation and complete existing root test block.
Host-motion and state sources, root dependency features, lockfile and toolchain
remain unchanged. This extraction is a prerequisite for action integration, not
an assertion that the complete Curves dispatcher or UI now runs on 0.20.

## Regression coverage

Twenty-seven new shared tests use real editor sessions, module metadata, authored
source caches and effect-parameter bindings. They execute production helpers and
the production global HistoryAction/Routing path, without substitute semantic
handlers. Coverage includes:

- Scalar addition/deletion, vector channel-scoped addition/deletion/value/time
  edits, gradient addition/deletion/time and independent RGBA adjustment; stable
  curve/gradient identities, preserved other channels and output ranges.
- Local and bound values, distinct cached/default semantic identities,
  normalized stored sampling, exact neighbor time limits and minimum key count.
- Actual pointer-to-graph conversion including existing plot padding, duplicate
  key selection, endpoint clamping, gradient ramp sampling and vector-channel
  clamping. These are helper calls with explicit coordinates, not native picking.
- Missing selections/modules/metadata, dangling bindings without local fallback,
  incompatible edits, invalid indices/channels and metadata/authored bounds.
- Curve/gradient preview projection, unchanged preview documents, normalized
  labels and degenerate output-range sampling.

Each accepted edit is reversed through global undo, replayed through global redo,
then checked for a single history entry. Mutation/guard fixtures verify that their
source effect file remains byte-identical. Binding fixtures keep parameter-default
and cached-local curve/gradient IDs distinct, as required by source validation.

These checks do not install the Curves activation bridge, keyboard consumer,
dock-opening behavior, host-curve editing/drag consumers, full panel/plugin or
native layout/picking/rendering. Existing stale-action/cancellation hardening
remains separate from compatibility qualification.

## Validation

Accepted frozen headless run (2026-10-18):
`target/bevy-020-qualification/runs/02261e06b3c141e8b7eefbfb8e3c456e/`.
It executes **1,548 checks**, including **1,127 editor graph/session/history and
consumer checks**, with all **876 input hashes** unchanged. Counts are executions
across gates, not unique tests. Three new paths join the inventory: the production
helper module, shared tests and bounded candidate Curves namespace. The existing
graph timing probe stays ignored; the graph suite takes 49.41 seconds after
compilation.

The runner confirms Bevy **0.20.0**, Rust **1.98.1**, `native_executed: false`,
and unchanged shipping lock SHA-256
`ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.
The twenty-seven new tests also pass in isolation. Strict candidate all-target
Clippy and both root/candidate formatting checks pass.

Shipping passes **2,155 editor unit + 3 architecture tests**, with ten existing
ignores unchanged. Strict shipping workspace all-target Clippy and workspace
all-target checking with `aestra-bench/gpu` pass. Root dependency/toolchain diffs
remain empty, Git whitespace validation passes and post-documentation input
drift is zero. Existing Windows incremental-cache hard-link fallback warnings
are non-fatal.

Next: qualify the actual Curves activation/keyboard/dock-opening and host-curve
consumers, then complete panel/plugin integration and native editor workflows.
B20-2 remains open; shipping stays on 0.19.1 until the full compatibility gate.
