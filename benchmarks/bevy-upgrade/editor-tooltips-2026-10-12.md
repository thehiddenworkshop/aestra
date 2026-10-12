# B20-2 slice 7 — Tooltip pointer and layout compatibility

The isolated Bevy 0.20 graph now compiles the **production** tooltip widget and
runs its shared layout and lifecycle regressions. Shipping remains Bevy 0.19.1;
root manifests and lockfile are unchanged. Graph dragging/marquee/resize and
material integration remain the next separate slice.

## Implementation

- Tooltips use the existing shared pointer boundary for hover and drag-start:
  generic picking events on 0.19, flat events on 0.20. Bevy still supplies the
  event types, relative cursor state and native popover layout.
- Cursor-anchored roots now use viewport-local UI units instead of directly
  storing window cursor pixels in `Val::Px`. The camera viewport origin is
  converted from physical to logical pixels, then `UiScale` is applied once.
  This fixes drift with UI scaling and offset camera viewports without applying
  display scaling twice. Element-anchored tooltip behavior is unchanged.
- The detached cursor anchor retains the nearest inherited UI camera, or
  explicitly selects the default UI camera. It remains outside the graph's
  transformed/clipped subtree; its body stays noninteractive and window-aware.
- The isolated editor feature set now enables Bevy's default font, matching the
  shipping graph. The real wrapping test initially failed because the old
  candidate graph lacked that font and therefore measured empty text. No text
  size, wrapping or visibility assertion was relaxed to pass qualification.

## Required regression gate

The source-frozen runner now requires `editor-tooltips`, hashing the production
widget and shared test source along with both pointer boundaries and the fixture.
The gate executes **15 checks**: six existing contract/layout tests and nine
new shared lifecycle tests. There are no fixture-local tooltip substitutes.

New coverage includes delayed creation, repeated-hover timer stability, one
noninteractive accessible popup, owner switching, leaving/removing/despawning
targets, drag dismissal before/after popup creation, external popup removal and
same-frame owner/drag transitions, unrelated hover isolation, inherited/default
camera selection, detached-root/body cleanup and cursor availability. Cursor
coordinates are checked across all nine combinations of 1x/1.5x/2x UI and
display scale with an offset viewport. The existing font-backed layout test
checks long unbroken paths at three UI scales and two popup widths.

Lifecycle tests dispatch real pointer events with deterministic relative-cursor
state and directly age the hover timer; they do not sleep or synthesize OS mouse
input. The separate wrapping test runs actual UI propagation, layout, measurement
and text systems with the built-in font. The fixture's system-parameter lint
allowance matches the shipping editor's existing policy.

## Remaining scope

Native OS hit testing, full popover edge-clamping visuals, graph transforms,
multiwindow/offscreen render targets, cursor behavior and full editor workflows
still require acceptance. This slice does not claim a full graph-widget port or
native visual approval. Optional new-widget adoption stays separate from the
required compatibility migration.

Next: graph pointer/cursor interactions and their geometry/material dependencies,
then application action dispatch and complete workflow qualification before the
coherent root dependency switch. No renderer-native gates were rerun here.

## Source-frozen validation

Accepted **headless** run:
`target/bevy-020-qualification/runs/b5e07b3db1b54caea3ed51652ba7e5cc/`.
Pinned MSVC Rust 1.98.1, locked/offline: **408 executed checks pass**
(9 keyboard, 8 theme, 18 controls, 46 numeric, 63 color/dependency,
32 custom text/menu/dependency, 15 tooltips, 40 upstream focus, 4 cluster units,
4 shader, 3 storage, 166 shared/candidate). Dependency checks deliberately
repeat across gates; these are execution counts, not unique-test counts.
All **559 input hashes** remain unchanged. The tooltip gate takes **0.33 seconds**
after compilation. The editor gates were all rerun with the font-enabled feature
set; earlier passing gates were not assumed to remain valid.

Shipping lock SHA256 remains
`ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.
Candidate strict all-target Clippy, both formatting checks and whitespace
validation pass. Existing incremental-cache hard-link warnings fall back to
copying. No new renderer-native or full-editor visual evidence is claimed.

Shipping validation: **1,226 editor unit + 3 architecture tests pass**, with
ten existing ignores unchanged. Strict all-target workspace Clippy and checking
with `aestra-bench/gpu` pass using the pinned toolchain, locked and offline.
