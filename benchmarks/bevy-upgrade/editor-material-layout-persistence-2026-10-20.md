# Project-bound material graph layout persistence — Bevy 0.20 qualification

Date: 2026-10-20. B20-2 slice 85. Shipping remains Bevy 0.19.1,
the isolated candidate remains 0.20.0, and Rust remains 1.98.1.

## Implementation

- `material_graph/layout_persistence.rs` shares the production project-layout
  resource, load/restore, debounced write, exit flush and registration. The native
  EditorMaterialGraphPlugin delegates this registration.
- Startup loading, lifecycle reconciliation after PersistenceSet::Lifecycle and
  before EditorSet::UiRebuild, and the Last camera/persist/exit chain retain their
  existing schedule. The unchanged PersistenceSet identity is shared from
  `persistence/sets.rs`.
- Candidate tests directly import production persistence, layout lifecycle and
  function-layout modules. Canonical facade paths alias real document/view
  registries, action-button composition and transient cleanup; no substitute
  persistence or camera implementation is introduced.
- Disk schema, 300 ms save delay, base-versus-effective placement policy,
  project namespaces, focused visible camera ownership and read-only protection
  for corrupt/newer files are unchanged.
- Frozen-input hashing covers the new modules, their tests and function fixture.

A source audit confirms all seven extracted persistence function bodies, three
function-layout bodies, six lifecycle/notice bodies, and the original material-root
tests are unchanged. The existing full function-canvas restart test is moved
unchanged into the shipping-only suite: its complete function-editor composition
is not yet imported by the 0.20 candidate. It is not ignored or replaced with a
mock. The remaining five function-layout tests and six lifecycle tests now run
in both suites.

## Shared regressions

Nine new tests install the complete production registrar against real temporary
project files, without wall-clock sleeps:

- Startup restores persisted base positions, pin/collapse state without semantic edits.
- Actual AppExit flushes pending base placements, not temporary drag offsets.
- The pending debounce deadline fires without requiring a subsequent graph edit.
- Corruption introduced after loading blocks the exit write and preserves exact bytes.
- Unsupported startup files remain read-only through update and exit.
- Stable saved layouts do not dirty persistence on idle updates.
- Clean exit does not unnecessarily reserialize the file.
- Late catalog replacement cannot flush one project's layout into another project.
- A queued lifecycle project switch removes old views and live controls before rebuild.

Existing shared cases also exercise same-ID project isolation, semantic reload and
removal, recoverable load failures, external newer files, material/function round
trips, and focused split-view camera ownership.

## Validation

Accepted frozen run:
`target/bevy-020-qualification/runs/f12c17f5efd3458cbd2a630b8b3766c6/`.
It executes **2,215 passing checks**, including **1,783 editor graph checks**
(91.16 seconds after compilation), with the existing ignored editor timing probe.
Counts are executions across gates, not unique tests. All **1,021 source input
hashes** are unchanged. Native execution is false.

Both strict all-target Clippy gates, both formatting gates and whitespace
validation pass. Root manifests, lockfile, toolchain and CI workflow are unchanged.

The complete candidate graph suite also passes with default parallel execution:
1,783 passed, 1 existing ignored timing probe, no failures (12.09 seconds).
Shipping passes **2,707 editor unit tests + 3 architecture tests**, with default
parallel execution, the same ten existing ignores and no failures.
Workspace GPU-feature all-target checking passes. Post-documentation input drift is zero.

## Remaining work

Continue B20-2 with material preview-worker orchestration, then remaining
host/plugin composition and application handlers. This qualifies headless
project-layout persistence and scheduling, not the complete material plugin or
native editor workflows. The full function-canvas restart case still needs the
function-editor composition in the candidate. Native picking, rendered zoom/DPI,
shader output and complete project/document workflows require native acceptance
before switching shipping dependencies. No visual references are approved.
