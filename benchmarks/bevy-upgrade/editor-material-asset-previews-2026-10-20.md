# Saved-material asset-preview validation — Bevy 0.20 qualification

Date: 2026-10-20. B20-2 slice 88. Shipping remains Bevy 0.19.1,
the isolated candidate remains 0.20.0, and Rust remains 1.98.1.

## Implementation

- `material_graph/asset_preview.rs` now imports its explicit production
  dependencies and is directly compiled in the 0.20 graph qualification gate.
- The native renderer entry point and signature are unchanged. Both engines use
  the actual shared deterministic pixel evaluator and standard material seed.
  The candidate's canonical preset path aliases the real production module.
- Only imports, test wiring and redundant parent-import cleanup change. A source
  audit confirms the renderer/validator body, recursive visit helper, all three
  original tests and remaining material-root tests are unchanged.
- Frozen-input hashing now includes the asset-preview module and its new
  qualification test file. No manifest, lockfile, toolchain or CI changes occur.

Limits remain thumbnail policy, not VFX runtime limits: at most 128 pixels per
side, 256 program expressions, 128 parameters and 64 program-expression levels.
Strict asset rendering continues to reject unsupported reachable outputs rather
than substituting the permissive graph checker fallback. Existing supplied
function-library responsibilities and browser/GPU routing are not redesigned.

## Shared regressions

The three existing cases now run on both engines: pixel parity without mutation;
unsupported/missing/cyclic output rejection and disconnected-node tolerance; and
size/depth limits with scanline cancellation.

Eleven new cases cover:

- Exact size/expression/parameter maxima are accepted; one-over inputs are rejected.
- Exactly 64 expression levels are accepted; both direct over-depth and a memoized
  shared subtree reached from a deeper second output are rejected.
- Vertex displacement requires a scene for both sprite and mesh domains.
- Duplicate expression IDs are rejected even when the duplicates are disconnected.
- Every texture-sampling form, both screen derivatives, bare function inputs and
  raw WESL calls fail explicitly.
- An unsupported branch is checked even when a Select would not evaluate it.
- A disabled texture node follows its real bypass without requiring an unused
  missing texture binding, preserving the input program.
- Missing saved parameter defaults fail; authored defaults recover and match
  equivalent direct constants.
- Supplied project graph functions and saved input defaults render the same pixels
  as direct values; missing library entries fail, without mutating program/library.
- A NaN saved value fails strict evaluation rather than displaying a checker.
- Early cancellation wins before invalid-size and missing-expression validation.

The traversal budget checks authored program expressions; this slice does not
extend it into function-library bodies. Native GPU execution and browser request/
capture orchestration are not part of these tests.

## Validation

Accepted frozen run:
`target/bevy-020-qualification/runs/1645cd573be04943a4f1b86faccd01f6/`.
It executes **2,260 passing checks**, including **1,828 editor graph checks**
(136.40 seconds after compilation), with the existing ignored editor timing probe.
Counts are executions across gates, not unique tests. All **1,037 source input
hashes** are unchanged. Native execution is false.

The complete candidate graph suite also passes with default parallel execution:
1,828 passed, 1 existing ignored timing probe, no failures (22.11 seconds).
Both strict all-target Clippy gates, both formatting gates and whitespace checks
pass. Root manifests, lockfile, toolchain and CI workflow are unchanged.

Shipping passes **2,740 editor unit tests + 3 architecture tests**, with default
parallel execution, the same ten existing ignores and no failures.
Workspace GPU-feature all-target checking passes. Post-documentation input drift is zero.

## Remaining work

Continue with material-graph workspace host composition and remaining plugin/
application integration, then full editor workflows. Browser GPU thumbnail
orchestration remains separate from this CPU validation gate. Native picking,
shader output, rendered zoom/DPI and complete project/document workflows require
native acceptance before switching shipping dependencies. No visual references
are approved.
