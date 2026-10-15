# B20-2 slice 46: referenced-track composition

## Implementation

Shipping remains on Bevy **0.19.1**. The isolated **0.20.0** candidate now builds
production referenced-emitter/effect-clip headers and read-only track rows from
the actual catalog projections. Outer emitter/root effect-clip headers and their
menus/color popovers remain a following integration slice; none are stubbed here.

- `timeline/referenced_effect.rs` uses explicit production dependencies and the
  existing qualified Button namespace. Its constructors retain grid/depth layout,
  list/native-control identity, selection styling, localized read-only badges,
  SVG disclosure icons and attached selection/source-opening/press observers.
- `timeline/track_style.rs` shares the existing localized emitter label and
  emitter/reference color helpers. Native outer headers still call those helpers
  through the same parent names.
- `feathers/button/tool.rs` shares the actual generic tool-button composition.
  button.rs reexports the same API; the application mini_button alias is unchanged.
  The graph candidate imports it directly, while the text-widget gate tests the
  public reexport and real Feathers scene/classification. No lint suppression or
  substitute button was needed for that fixture.
- Source-path resolver exports still resolve to the same production function.
  Three new files join the frozen inventory; dependency, feature, toolchain and
  authored-format inputs stay unchanged.

Audits verify **eight function contracts**: four extracted button/label/color
helpers and four newly candidate-qualified referenced construction/timing
functions. Bodies are unchanged apart from visibility, formatting and import/
equivalent namespace resolution. Native TimelinePlugin order, complete existing
root timeline/referenced-effect test modules and the remaining button handlers/
constructor bodies compare unchanged.

## Regression coverage

Twenty new shared checks create real source files/catalogs, reuse a leaf effect
through two nested branches, and build the actual headers/bars:

- Matching header/body grid rows, native list/action classification, selected
  emitter/clip path styling, depth indentation, disabled-name/custom-color policy,
  muted/solo-suppressed fills and trimmed ancestor timing/identity.
- Missing-timing children retain empty layout rows; offscreen bars retain their
  controls. Reused sources keep distinct branch actions and child selection.
- Actual bar click/list activation clears Curves selection without edits;
  secondary clicks do not select/open. Bar/header double-click requests the correct
  leaf-emitter source without replacing the current document or modifying files.
- Real disclosure Activate changes only nested expansion; empty-source controls
  hide and expanded controls load the existing down-chevron asset. Read-only
  labels follow both locales, control presses stop before the canvas, and built
  bars remain compatible with live root-clip trim presentation.

Four existing referenced timing/path/click checks now also execute on the candidate.
One additional candidate text-gate regression exercises the unchanged tool-button
reexport, Feathers Button identity, dimensions, themed text and action marker.
These five checks are not new shipping tests.

Fixtures capture production DocumentAction intents rather than performing external
source navigation. They use actual scenes/observers and explicit node geometry,
not native layout, picking, floating-window routing or rendered visual acceptance.
No new pointer-ownership, disabled-control or asynchronous source-open guarantee
is claimed. Existing nested disclosure labeling (including its empty button label)
and earlier interaction-hardening follow-ups remain separate behavior work.

## Validation

Accepted frozen headless run (2026-10-15):
`target/bevy-020-qualification/runs/c6b4af0c755d4e9cb249320495397226/`.
It executes **1,298 checks**, including **878 graph/session/history/consumer checks**,
with all **839 input hashes** unchanged. One existing graph timing probe stays
ignored. Counts are executions across gates, not unique tests. The graph process
takes 17.61 seconds after compilation.

The runner confirms Bevy **0.20.0**, Rust **1.98.1**, `native_executed: false` and
unchanged shipping lock SHA-256
`ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.
Shipping passes **1,958 editor unit + 3 architecture tests**, with ten existing
ignores unchanged. Strict all-target Clippy passes on both engines, alongside
workspace all-target checking with `aestra-bench/gpu`, both formatting checks and
whitespace validation. Root dependency/toolchain diffs remain empty. Existing
Windows incremental-cache hard-link fallback warnings are non-fatal.

## Remaining scope

Next: outer emitter/root effect-clip headers with their actual menus/popovers,
remaining timeline track/toolbar construction, complete timeline/Curves plugin
composition, then remaining viewport/application consumers and native workflows.
B20-2 stays in progress; shipping changes engine graphs only after B20-3 acceptance.
