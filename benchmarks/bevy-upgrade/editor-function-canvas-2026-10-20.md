# Function-native canvas composition — Bevy 0.20

Date: 2026-10-20. B20-2 slice 90. Shipping remains Bevy 0.19.1,
the isolated candidate remains 0.20.0, and Rust remains 1.98.1.

## Implementation

`material_function_editor/graph/composition.rs` is now the actual production
function canvas on both engines: toolbar, categorized searchable Add Node menu,
expression/output nodes, sockets, constant controls, wires and pointer context
menus. Shared component identities live in `graph/model.rs`; the native input
handlers continue consuming those same types. There is no surrogate material
program, alternate canvas or qualification-only action implementation.

The canvas requires real compact/icon/searchable action menus. Their implementation
and three original tests move from `feathers/combo_box.rs` to
`combo_box/actions.rs`, retaining canonical public exports. The candidate uses
the real existing selection/menu, search-field and Feathers scene infrastructure.
Function wire identity is extracted unchanged to `material_graph/insertion/function_wire.rs`;
semantic insertion planning, dispatch and registration remain native-owned.

Source audits preserve every extracted composition/helper body, all action-menu
implementation (apart from equivalent absolute module paths), remaining insertion
logic and original function-graph tests. Existing menu-test assertions are retained;
their fixture corrections are described below. Only imports,
visibility, module wiring and a documented composition-argument lint allowance
change. No manifest, lockfile, toolchain or CI changes are made.
All 31 remaining native function input/wire-handler bodies are unchanged, as are
the 12 original menu-test assertions.

## Shared regressions

Eight new cases compose the production function graph:

- An explicit restored/unfocused function target creates actual expression nodes,
  semantic sockets, function output ports, insertion wire identities and a matching
  project/view/drop target without changing the effect, target or edit serial.
- Split views share authored expression positions while retaining separate cameras,
  scoped node actions and toolbar frame destinations.
- Missing/wrong-project/non-function and custom-WESL targets create no surrogate
  graph. The unchanged native host retains the signature/inspection fallback.
- Node and connection menus require both function identity and owning view;
  required output and optional argument connection labels remain distinct.
- Scalar/vector/color constants retain all 14 component controls, values, limits,
  steps and expression/component identities; booleans retain their two typed choices.
- Real Add Node search events filter and restore actual catalog rows without
  changing the function or semantic edit serial.
- Invalid unsaved function drafts still show a graph without replacing the draft
  or active effect target.
- Bootstrap placement is deterministic, outputs follow expression columns, and
  output/argument/named-input mapping preserves semantic IDs and rejects unknown ports.

The existing three action-menu tests additionally run on both engines: focusable
items, retained icon-menu open/reopen focus, and isolated case-insensitive search.
The first search test attempt selected the inner text child instead of the actual
container; correcting the test event source required no production behavior change.

The first full run exposed two incomplete old menu fixtures: one lacked visibility
propagation/window-owned focus; the other assumed menu focus acquisition completed
in Update. Bevy 0.20 performs it in PostUpdate. Both now use the actual shared
headless editor UI stack and the retained-icon case executes the complete frame.
Opening also supplies Visible, matching the real menu trigger. All existing
Open/focus/close/reopen and TabIndex assertions are preserved; all three tests pass.

## Validation

Accepted frozen run:
`target/bevy-020-qualification/runs/356765afe11a4fbb95af875a754919a4/`.
It executes **2,281 passing checks**, including **1,849 editor graph checks**
(121.33 seconds after compilation), with the existing ignored editor timing probe.
Counts are executions across gates, not unique tests. All **1,050 input hashes**
are unchanged. Native execution is false.

The complete graph suite also passes with default parallel execution: **1,849
passed**, one existing ignored timing probe, no failures (13.81 seconds).
Both strict all-target Clippy gates, both formatting gates and whitespace checks
pass. Root manifests, lockfile, toolchain and CI workflow are unchanged.

Shipping passes **2,758 editor unit + 3 architecture tests**, with default parallel
execution, the same ten existing ignores and no failures. Workspace GPU-feature
all-target checking passes. Post-documentation input drift is zero.

## Remaining work

Qualify function signature-editor/inspection fallback composition and the complete
material-graph workspace host branch, then remaining graph/plugin/application
input and action integration. This slice validates composed production entities
and scoped action payloads, not native function input dispatch, rendered wires,
picking, DPI or complete editor workflows. Native acceptance remains required
before changing shipping dependencies; no visual reference is approved.
