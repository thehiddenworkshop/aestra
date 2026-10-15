# B20-2 slice 45: automation lane composition

## Implementation

Shipping stays on Bevy **0.19.1**. The isolated **0.20.0** candidate now builds the
actual automation lane headers, graph bodies, key controls and resize handles.

- `timeline/lane_composition.rs` shares the two lane constructors and their
  choreography icon button. These retain real Feathers tool-button scenes, SVG
  handles, accessibility labels/tooltips, geometry, native control classification,
  curve raster requests and attached selection/drag/resize/graph observers.
- The candidate imports production `timeline/automation.rs`, including registry-
  driven scalar/vector/gradient discovery, public binding/source-bank resolution,
  localized labels and existing visibility helpers/tests. No fixture registry or
  simplified lane discovery stands in for production.
- `properties/module_data.rs` shares the actual linked-extension module registry
  resource/default, active parameter lookup and property label/description mapping.
  Properties reexports the same API; existing callers and initialization remain
  unchanged. The version boundary selects equivalent upstream Button/cursor names.
- Three new source/test files join the frozen inventory. No dependency, feature,
  toolchain, authored format or control replacement is bundled.

Audits verify seven extracted contracts (three constructors, two property helpers,
one registry type and its default), plus the six newly candidate-qualified
automation function/type definitions. Function bodies retain their behavior;
only visibility, formatting, explicit imports and equivalent engine namespaces
differ. Native TimelinePlugin order and the complete existing root timeline,
automation and properties test modules remain unchanged.

## Regression coverage

Twenty new shared checks build real UI entities and use production controllers:

- Real registry input IDs, unique lanes, English/French labels and tooltip
  shortcuts; constant inputs and unknown modules with curves are omitted.
- Bound public defaults and vector source banks produce the correct keys/channels.
- Matching header/body grid rows and custom height, scalar geometry/native
  classification, selected/muted fills, gradient columns/line widths, selected
  vector channel, add/delete action payloads and conditional delete controls.
- Selected-region source offsets, offscreen graph/key identity and real raster
  ImageNode generation with idle image-handle reuse.
- Attached key click/press/drag consumers, selection without semantic edits,
  preview-only drag followed by one globally reversible edit, graph double-click
  insertion into a source bank and resize synchronization without document edits.
- Real header Activate events emit exactly one add/delete action. Pending
  activation clears, idle does not repeat it, and global undo/redo restores keys.

Two existing production automation metadata checks also join the candidate graph;
they are not counted as new shipping tests. An initial add-button fixture placed
the playhead before the emitter and correctly made no change at its existing
endpoint. The successful insertion test now places it inside the source interval;
production endpoint policy is unchanged.

These fixtures run actual scenes/observers and CPU raster generation, but do not
install the complete timeline/Curves/application plugin or native UI/picking/
rendering stack. Targeted pointer events and explicit geometry are not native
window input, layout, visual or floating-window acceptance. Previously recorded
pointer ownership, automation cancellation and disabled-control policies remain
separate hardening work, not newly fixed guarantees.

## Validation

Accepted frozen headless run (2026-10-15):
`target/bevy-020-qualification/runs/a9a64f3eeaff41d9a1b841b232fb8e4e/`.
It executes **1,273 checks**, including **854 graph/session/history/consumer checks**,
with all **836 input hashes** unchanged. One existing graph timing probe stays
ignored. Counts are executions across gates, not unique tests. The graph process
takes 21.26 seconds after compilation.

The runner confirms Bevy **0.20.0**, Rust **1.98.1**, `native_executed: false` and
unchanged shipping lock SHA-256
`ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.
Shipping passes **1,938 editor unit + 3 architecture tests**, with ten existing
ignores unchanged. Strict all-target Clippy passes on both engines, alongside
workspace all-target checking with `aestra-bench/gpu`, both formatting checks and
whitespace validation. Root dependency/toolchain diffs remain empty. Existing
Windows incremental-cache hard-link fallback warnings are non-fatal.

## Remaining scope

Next: remaining emitter/effect-clip/referenced-track construction, then complete
timeline/Curves panel/plugin integration, remaining viewport/application consumers
and native workflow acceptance. B20-2 remains in progress; shipping changes engine
graphs only after B20-3 acceptance.
