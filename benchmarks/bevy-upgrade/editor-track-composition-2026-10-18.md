# B20-2 slice 51: choreography track assembly

## Implementation

Shipping remains on Bevy **0.19.1**. The isolated **0.20.0** candidate now builds
the actual choreography header and row lists together, rather than exercising
only their individual constructors.

`timeline/track_composition.rs` shares the two original assembly expressions:
root effect clips and their catalog-backed nested projections; root emitters,
implicit/explicit regions and visible automation lanes; and the hidden drop
spacer on each side. The shipping panel calls these same constructors. Existing
grid placement, selection/mute/solo styling, localized descriptions, clipping,
trim hit areas and attached production observers remain intact.

The shared module uses the existing version-specific upstream button and cursor
names. It does not add a substitute input dispatcher. Obsolete imports/reexports
are removed from the native timeline/regions wrappers. No handler, authored
format, root dependency feature or toolchain change is included.

The two extracted expression contracts compare unchanged after formatting
normalization (including redundant closure/match braces). Native TimelinePlugin
wiring and the complete existing root timeline test block compare unchanged.
The frozen inventory includes both new production/test files.

## Regression coverage

Eighteen new tests run unchanged on both engine graphs, with real catalog files,
module metadata, scene constructors, attached pointer observers, action consumers
and global undo/redo routing. They cover:

- Exact matching header/body grid rows and heights for mixed explicit track
  ordering, expanded nested/reused effects and automation lanes; collapsed and
  hidden lanes, custom lane height and an empty effect.
- Distinct paths for reused nested sources, actual automation graph identities
  and keys, implicit/explicit region bar counts and native control classification.
- Clipped bars and independent boundary visibility, selected/muted/suppressed
  effect styling, localized control construction, cursor/hit-area metadata and
  the hidden drop spacers on both sides.
- Attached primary selection and secondary region context-menu consumers, plus
  press isolation from the parent canvas.
- Move and both trim controls for effect clips and emitter regions at two DPI
  scales, exact final start/source-offset/duration, non-mutating previews,
  exactly one globally reversible edit per release, cursor cleanup and no-motion
  releases. Source/project files remain byte-identical.

The initial stronger timing assertions exposed an incorrect high-DPI test input:
screen distance must grow with the scale factor for the same logical movement.
Only the fixture was corrected; the production conversion was preserved. Both
final engine graphs pass with exact timing assertions enabled.

These are headless scene/observer tests with explicit canvas geometry. They do
not qualify native picking/layout/rendering, menu focus/lifecycle, floating-window
routing, the remaining panel/host-motion/scrollbar construction or full timeline/
Curves plugin integration. Existing pointer ownership, stale-action and
cancellation hardening follow-ups remain separate.

## Validation

Accepted frozen headless run (2026-10-18):
`target/bevy-020-qualification/runs/3a0e8a055726430d95880954d1f3ceb4/`.
It executes **1,447 checks**, including **1,027 graph/session/history/consumer
checks**, with all **859 input hashes** unchanged. Counts are executions across
gates, not unique tests. The existing graph timing probe remains ignored;
the graph process takes 27.57 seconds after compilation.

The runner confirms Bevy **0.20.0**, Rust **1.98.1**, `native_executed: false`,
and unchanged shipping lock SHA-256
`ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.
Shipping passes **2,058 editor unit + 3 architecture tests**, with ten existing
ignores unchanged. Strict all-target Clippy passes on both engines, alongside
workspace all-target checking with `aestra-bench/gpu`, both formatting checks and
whitespace validation. Root dependency/toolchain diffs remain empty. Existing
Windows incremental-cache hard-link fallback warnings are non-fatal.

Next: remaining full timeline/Curves panel/plugin integration, including
host-motion construction and scrolling, then viewport/application consumers and
native workflow acceptance. B20-2 remains open; shipping stays on 0.19.1 until
the complete compatibility gate.
