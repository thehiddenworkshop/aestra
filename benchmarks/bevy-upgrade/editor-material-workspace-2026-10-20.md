# Material/effect workspace host composition — Bevy 0.20

Date: 2026-10-20. B20-2 slice 89. Shipping remains Bevy 0.19.1;
the isolated candidate remains 0.20.0 and Rust remains 1.98.1.

## Implementation

The native material graph host now delegates its material/effect branch to
`material_graph/workspace.rs`. The isolated candidate imports that exact source,
using the actual header, canvas, palette, context-menu components and drop-target
identity. Explicit target projection is shared through `projection.rs`; the
identity and stale-target checks move unchanged to `asset_drop/target.rs`.
Actual drop planning, execution and registration remain in the native host.

The function-editor branch, including its graph/signature views, stays unchanged
in the native wrapper. It is not replaced with a qualification-only canvas or
claimed as ported by these tests. No manifest, lockfile, toolchain or CI changes
are made.

A source audit confirms that projection, drop-target methods, menu constructors,
the complete native function branch, remaining asset-drop logic and original
material-root tests are unchanged. The material/effect branch is preserved except
for two intentional menu ownership checks described below. Import/visibility
changes only expose the real modules to both compilation roots.

## View ownership regression

A new real-composition regression reproduced node menus leaking into a second
view of the same program (one menu found where zero was expected). Both node and
connection menus previously matched only the program. They now also require the
owning view, matching the Add Node palette's existing scoped policy. Positive
ownership checks, another split view and the unscoped tool view are tested.
No menu actions or semantic command policy are changed.

## Shared regression coverage

Seven new workspace tests run the actual material/effect composition:

- A restored standalone tab projects its explicit document before focus, with
  matching geometry/drop identities and without changing the active target,
  effect or semantic edit serial.
- Split views retain independent pan/zoom and shared authored node placement.
- The selected effect renderer binds its real material instance to previews,
  without standalone Back navigation.
- Nonsemantic effects and wrong-project standalone targets show the existing
  empty/error state with no canvas/drop marker in both English and French.
- An invalid unsaved normalized program retains a diagnostic graph without
  publishing a replacement or editing the effect.
- Add Node palettes require matching program, view and editing target.
- Node/connection menus are composed only in their owning view.

Three new drop-identity tests cover effect replacement versus saved-program
identity, function target/project namespace and mismatched owner/context rejection,
and preservation of program document keys. They exercise the actual constructors,
checks and keys; full browser drop execution remains separately qualified.

## Validation

Accepted frozen run:
`target/bevy-020-qualification/runs/9560956c5d9d489abbdd7dddcca30a0e/`.
It executes **2,270 passing checks**, including **1,838 editor graph checks**
(114.38 seconds after compilation), with the existing ignored editor timing probe.
Counts are executions across gates, not unique tests. All **1,044 input hashes**
are unchanged. Native execution is false.

Both strict all-target Clippy gates, both formatting gates and whitespace checks
pass. Root manifests, lockfile, toolchain and CI workflow are unchanged.

The complete candidate graph suite also passes with default parallel execution:
1,838 passed, 1 existing ignored timing probe, no failures (23.40 seconds).
Post-documentation input drift is zero.

Shipping passes **2,750 editor unit tests + 3 architecture tests**, with default
parallel execution, the same ten existing ignores and no failures.
Workspace GPU-feature all-target checking passes. Post-documentation input drift
remains zero.

## Remaining work

Continue with function-graph workspace composition, then remaining material-plugin
and application action integration. Native input propagation, picking, rendered
zoom/DPI, GPU presentation and complete document workflows still require native
acceptance before changing shipping dependencies. No visual reference is approved.
