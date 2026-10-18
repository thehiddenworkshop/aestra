# B20-2 slice 53: host-motion inspector composition

## Implementation

Shipping remains on Bevy **0.19.1**. The isolated **0.20.0** candidate now builds
the actual host-motion inspector, its controlled numeric fields, motion actions,
Curves link and scroll area.

- `timeline/inspector_composition.rs` shares the original inspector, label,
  numeric-field and Added-based initialization helpers. The native host module
  reexports the inspector and installs the same initialization system. Numeric
  scenes come directly from the existing implementation instead of through its
  native UI-shell reexport; no second upstream number widget is installed.
- `shell/scroll_key.rs` and `curves/action.rs` share the complete existing
  ScrollMemoryKey and CurvesAction identities. Native modules retain their public
  reexports. No reduced fixture enum or replacement application action is added.
- `feathers/button/action.rs` shares the actual generic primary/secondary action
  button constructor, retaining the native public API and action classification.
- The candidate imports the actual field-row and complete scroll-area module,
  including persisted scroll markers and both viewport constructors. The
  previously qualified scrollbar implementation remains the same source.

Audits compare the four inspector/helper contracts (with the explicit numeric
scene reexport expansion), both complete enums, generic button constructor,
remaining button activation/audit functions and unchanged scroll module. Native
host installation, Timeline/Curves/Shell plugin wiring and all four existing
host-motion/timeline/Curves/Shell root test blocks remain unchanged.
Root dependency features, toolchain and authored formats are unchanged. Existing
English host-motion descriptions and numeric interaction policy are preserved.

## Regression coverage

Twenty-two new shared tests construct real scenes with the existing headless
Feathers/text/input plugins, actual motion consumers and global history routing:

- Inactive, missing-track, anchor, selected, stale/unselected, error and repeat
  states; exact numeric target identities, ranges, steps and initial text/values.
- Actual Properties scroll-memory identity and adjacent bar targeting; the
  concrete CurvesAction::OpenHost payload and Feathers classification; generic
  primary/secondary button API, field-row indentation/minimum width and the real
  two-axis scroll wrapper and bar targets.
- Non-mutating position preview, one globally reversible final edit, invalid
  scale rollback, time-key reordering with pose preservation, degree-to-quaternion
  edits, Escape/late-release cancellation, and silent programmatic display updates.
- A real EditableText edit through the numeric scene followed by keyboard Enter,
  plus constructed Add/Delete/Clear/Repeat/Close-loop buttons using production
  activation and semantic consumers. Authored files remain byte-identical.

The candidate also runs the existing shared field-row contract and one new
engine-specific public button-reexport probe. The latter verifies generic scene
composition, not application action dispatch. Existing widget regression targets
still use their own bounded host context; no new application behavior is inferred
from that probe.

These are headless composition/consumer checks. They do not qualify native visual
layout, pointer picking, numeric hover/drag acceptance in a window, persisted
scroll capture/restore, floating windows or complete timeline/Curves/Shell plugins.
The Curves link has its real payload and scene but its remaining dock-opening
consumer is not newly qualified. Prior interaction-hardening follow-ups remain
separate.

## Validation

Accepted frozen headless run (2026-10-18):
`target/bevy-020-qualification/runs/a5526de04eb04a0f89f269578b10cc25/`.
It executes **1,495 checks**, including **1,074 graph/session/history/consumer
checks**, with all **869 input hashes** unchanged. Counts are executions across
gates, not unique tests. Six paths join the inventory: five new source/test files
and the existing field-row implementation. The existing graph timing probe stays
ignored; the graph process takes 38.46 seconds after compilation.

The runner confirms Bevy **0.20.0**, Rust **1.98.1**, `native_executed: false`,
and unchanged shipping lock SHA-256
`ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.
Shipping passes **2,104 editor unit + 3 architecture tests**, with ten existing
ignores unchanged. Strict all-target Clippy passes on both engines, alongside
workspace all-target checking with `aestra-bench/gpu`, both formatting checks and
whitespace validation. Root dependency/toolchain diffs remain empty. Existing
Windows incremental-cache hard-link fallback warnings are non-fatal.

Next: remaining full timeline panel composition and Curves action/panel/plugin
integration, then viewport/application consumers and native workflow acceptance.
B20-2 remains open; shipping stays on 0.19.1 until the complete compatibility gate.
