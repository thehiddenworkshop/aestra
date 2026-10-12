# B20-2 slice 3 — Numeric scrub pointer/cursor compatibility

## Implementation

The shipping editor and isolated 0.20 fixture now execute the same production
numeric scrub implementation in `feathers/number_input/scrub.rs`. The shipping
`number_input.rs` remains a facade for existing panel consumers and step arrows.
`NumberScrubPlugin` registers the original resource, three observers and cursor
decoration system. Shipping decoration retains the existing Feathers Sync set.
No numeric policy, arrow geometry, palette or domain command behavior is changed.

Two small version-selected boundaries cover actual upstream API moves:

- 0.19: `Pointer<DragStart/Drag/DragEnd>`, Feathers cursor components/resources.
- 0.20: flat `PointerDragStart/PointerDrag/PointerDragEnd`, picking cursor types.

Production boundaries are aliases/re-exports, not synthetic event translators.
Only test constructors differ between versions. The shared tests use real typed
pointer event dispatch, the production plugin, and real Feathers editable text
with text-edit application. The existing headless text harness accepts setup
before plugin finalization so the numeric plugin follows the actual app lifecycle.

## Required regression gate

The existing two numeric policy tests are shared with the candidate, alongside
six new interaction regressions:

- Cursor decoration preserves the existing field entities and text.
- A descendant drag updates real text and emits owner-scoped live values,
  followed by exactly one final value on release and cursor restoration.
- Shift fine stepping, Control coarse stepping and Shift precedence.
- Bounds clamping and exclusion of vertical-only pointer travel.
- Secondary buttons and unrelated targets cannot modify/finish an active scrub;
  a no-change primary release still submits the initial value.
- Numeric dragging stops propagation to ancestor controls.

The required `editor-controls` gate grows from 10 to 18 tests. The qualification
runner also freezes the facade, both pointer boundaries, shared scrub code and
tests, in addition to its existing production inputs. No dependency feature,
shipping lockfile or renderer source change is needed for this slice.

## Scope and next step

This qualifies the generic Aestra scrub core, **not full 0.20 numeric widget
composition**. The fixture attaches `ScrubbableNumber` to the existing real
Feathers text-field composition; it deliberately does not combine it with
0.20's new built-in number-input drag/arrow observers. Native pointer hit testing,
OS cursor integration, cancellation/despawn during a drag and visual parity
remain outside this gate.

The next compatibility slice must explicitly reconcile `NumberInputValue`,
the removed `UpdateNumberInput` and `number_format` scene property, and upstream
scrub/arrow behavior with Aestra's existing value bridge, modifiers, hover contrast
and step controls. Do not install competing drag handlers. Then qualify the
color-plane composition and its changed vertical mapping, followed by custom
code/search consumers and complete editor workflows.

Shipping remains Bevy 0.19.1. B20-2, the coherent root graph switch and final
native visual/performance acceptance are still open.

## Validation

Accepted source-frozen **headless** run:
`target/bevy-020-qualification/runs/1704b1156d53485e9a7ef4121f161faf/`.
Pinned MSVC Rust 1.98.1, locked/offline dependencies: **252 checks pass**
(9 keyboard, 8 theme, 18 editor controls, 40 upstream focus, 4 cluster units,
4 shader, 3 storage and 166 shared/candidate checks). All **480 input hashes**
remain unchanged. The editor-controls gate completes in **2.18 seconds**.
The shipping lockfile remains
`ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.

Native renderer gates were not rerun for this UI-only slice; the previous
slice's 34 native checks remain separately recorded in
[its report](editor-controls-2026-10-11.md), not counted as fresh acceptance here.
Shipping validation: **1,185 editor unit tests and 3 architecture tests pass**;
the ten existing ignores are unchanged. The full suite first hit sandbox access
restrictions in temporary project/settings fixtures; the approved filesystem
rerun passes all tests, including the six unchanged step-arrow regressions.

Strict workspace all-target Clippy and checking with `aestra-bench/gpu`, strict
candidate all-target Clippy with `editor-qualification`, both formatting checks
and `git diff --check` pass. Existing incremental-cache hard-link warnings fall back
to copying. No lint suppression or reduced test assertions are introduced.
