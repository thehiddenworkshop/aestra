# B20-2 slice 4 — Controlled numeric composition and value bridge

## Implementation

Shipping Bevy 0.19.1 and the isolated 0.20.0 candidate now execute the same
`feathers/number_input/widget.rs` composition, numeric bridge, scrub policy and
hover-arrow implementation. Existing scalar/integer/labeled scene entry points
remain available through the scene facade. The editor, color picker, host-motion
panel and node-graph control exclusion use the app-owned numeric boundary.

The native 0.20 number widget owns private drag surfaces, step controls and
observers without a public opt-out. Composing it with Aestra's existing handlers
would install competing interactions. This compatibility slice therefore uses
the actual Feathers text-container/text-input primitives, not the native number
widget or its private hierarchy. `NumberInputValue` retains upstream typed value
variants; the app-owned `UpdateNumberInput` event and `NumberFormat` retain the
document-driven update contract. Native component-based synchronization is not
adopted here. B20-4's upstream numeric adoption/removal pilot remains separate.

The composition retains labels, axis sigils, numeric filtering, the 20-character
limit, direct editable-child ownership and two 12-pixel hover steppers with the
existing contrast background and SVG icons. An app-owned transparent surface
defers focus acquisition until a click, preventing press-to-focus/text selection
from competing with scrubbing. It becomes non-pickable during text editing.
Click-before-DragEnd release dispatch does not turn a completed scrub into a
text edit. Cursor/pointer aliases also cover the flattened 0.20 click event.

The bridge deliberately tightens several event rules:

- Only changed, focused text emits live typed values; cursor/selection changes,
  programmatic refreshes and unfocused scrub/arrow text writes do not duplicate
  numeric submissions.
- Enter submits on press, not again on release; focus loss remains a final edit.
- Partial/invalid/non-finite typed floats do not submit. Mismatched programmatic
  value variants are ignored, and focused edits are not overwritten by refresh.
- Disabled ancestors block numeric commits and scrub initiation. Text editing
  also blocks scrub initiation. Existing modifiers, bounds, integer saturation
  and externally handled `CustomNumberStep` behavior remain unchanged.

## Required regression gate

`editor_numbers.rs` imports production composition, arrows, scrub, SVG loading
and localization alongside real Feathers editing/focus plugins. Ten new shared
composition regressions cover typing, single Enter/focus-loss submission,
programmatic updates, filters/length, labels/sigils, hover geometry, exact f64 and
large i64 values, integer saturation, invalid input, disabled ancestors,
click/scrub focus separation, and duplicate-free scrub/arrow submissions.

The required `editor-numbers` gate also executes the six existing arrow, eight
scrub, eight text, six SVG and eight localization regressions: 46 checks total.
SVG test asset discovery works from both manifests. The source-frozen runner
hashes the additional production code, both catalogs and shipped SVG corpus.
Only the isolated qualification manifest adds the existing production
resvg/tiny-skia/Fluent dependencies; shipping manifests and lockfile do not change.
The geometry-layout test harness now initializes the focus resource required by
the real numeric bridge rather than using a stub numeric composition.

## Scope and next step

This is headless composition/event qualification, not native pointer hit-test,
OS cursor, DPI/layout/visual parity or full editor-workflow acceptance. Pointer
events and focus requests are dispatched through real observers, with the
production picking release order exercised explicitly. Mid-gesture disable,
cancellation/despawn and end-to-end host input still need native acceptance.
No new native renderer evidence is claimed for this UI-only slice.

Next: color-plane scene compatibility and its changed vertical mapping, then
remaining pointer/custom-text consumers and full editor workflow acceptance.
B20-2 and the coherent root dependency switch remain open; shipping stays 0.19.1.

## Validation

Accepted source-frozen **headless** run:
`target/bevy-020-qualification/runs/bd93d693d98a4843bd131147a5420176/`.
Pinned MSVC Rust 1.98.1, locked/offline: **298 executed checks pass**
(9 keyboard, 8 theme, 18 editor controls, 46 numeric/dependency checks,
40 upstream focus, 4 cluster units, 4 shader, 3 storage and 166 shared/candidate
checks). All **540 input hashes** remain unchanged. The numeric gate takes
**4.14 seconds**. Shipping lock SHA256 remains
`ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.

Shipping: **1,195 editor unit tests and 3 architecture tests pass**, with the
ten existing ignores unchanged. Temporary project/settings fixtures require
the approved filesystem run. Strict all-target workspace Clippy/checking with
`aestra-bench/gpu`, strict all-target candidate Clippy with `editor-qualification`,
both formatting checks and `git diff --check` pass. Existing incremental-cache
hard-link warnings fall back to copying.

No renderer-native gates were rerun. Prior native evidence remains separately
dated in [the earlier report](editor-controls-2026-10-11.md). The only new
production diagnostic allowance is the narrowly scoped 0.20 deprecation for
`template_value`, required by the shared 0.19 scene implementation; candidate
fixtures also allow unused out-of-scope panel helpers. No behavioral lint or
regression assertion is suppressed.
