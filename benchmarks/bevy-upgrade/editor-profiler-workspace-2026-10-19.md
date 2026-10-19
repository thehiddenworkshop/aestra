# B20-2 slice 68: Profiler workspace, actions and plugin

## Implementation

Shipping remains on Bevy **0.19.1**. The isolated **0.20.0** editor candidate now
executes the actual Profiler workspace, formatters, live label systems, reset
consumer and plugin. Production code is separated into `profiler/workspace.rs`,
`controls.rs` and `plugin.rs`; the native application retains its existing exports.
The candidate imports those same files and the complete existing state module.
Shared panel chrome is imported from production too, not replaced by test widgets.

All **20 presentation function bodies**, the plugin implementation and the five
existing root profiler tests are unchanged. The state/aggregation code is unchanged.
The native 0.19 action query retains its original `Changed<Interaction>`,
Feathers classification and pending-activation requirement. The 0.20 boundary maps
native `Changed<Pressed>` to that same contract. Both consume pending activation
and clear their respective pressed state; the reset branch itself is unchanged.
Native button aliases are test-only and use the actual ui-widgets button installed
by the Feathers scene on each engine, not the legacy UI button marker.

No ordering is added between the existing Actions and Sync sets. No reset,
profiling, formatting, history, document-edit or authored-format policy changes.
No new public runtime or renderer API and no root dependency/toolchain changes.

## Regression coverage and limits

**23 shared tests** execute on both engines: two existing shipping regressions
mirrored into the candidate, plus 21 new cases. Eight previously qualified state
tests also run in the focused candidate Profiler gate, for **31 passing tests**.
The new/shared coverage includes:

- All 20 metric-to-field mappings, duration/memory unit boundaries and unavailable
  values, measured/estimated/unavailable labels and colors, both locales and
  actionable trail-point warnings. GPU simulation time stays distinct from draw
  time; an unknown value is not presented as a measured zero.
- Real waiting/populated workspace composition, native Feathers reset controls,
  accessible labels, non-picking text, persisted Profiler scrolling, metric cards,
  emitter rows and all 96 history bars.
- Right-aligned normalized history, latest-sample highlighting, empty/zero samples
  and overflow-safe summaries. State changes update existing entities; unchanged
  state/localizer retains the original early-return behavior.
- Locale changes update dynamic labels, emitter summaries, history and project
  details. Nested instance paths remain visible; empty projects clear old detail
  text. Invalid pending transactions display the last-valid-preview status without
  applying the invalid candidate or dirtying the document.
- Actual plugin installation preserves preinstalled state, registers the original
  three systems in their existing sets, and routes Activate through the pending
  gate. Reset clears history and peaks once with localized status, preserving the
  authored effect, document revision, undo/redo lengths and clean state. Ungated
  physical presses and unrelated controls cannot trigger resets; set gating
  defers pending activation until enabled.

These are headless composition, ECS consumer and schedule checks. They do not
approve native rendering, picking/keyboard delivery through the entire application,
visual layout at every DPI, GPU timing accuracy or profiling performance.
Static headings and availability composition retain their original rebuild policy;
this slice does not introduce new live relocalization/recomposition behavior.

## Validation

Accepted frozen run (2026-10-19):
`target/bevy-020-qualification/runs/0cbae389982c4da485571e4328c2db56/`.
The complete qualification executes **1,818 checks**, including **1,396 editor
graph/session/history/consumer checks** with the existing timing probe ignored,
in 50.15 seconds for that editor suite after compilation.
All **935 input hashes** are unchanged. Counts are executions across gates, not
unique tests. Six Profiler source/test paths, shared panel chrome and the candidate
wrapper are the eight new inventory paths.

The runner confirms Bevy **0.20.0**, Rust **1.98.1**,
`native_executed: false`, and unchanged shipping lock SHA-256
`ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.

Shipping passes **2,372 editor unit + 3 architecture tests**, with ten existing
ignores unchanged. Strict all-target Clippy passes on both engines, alongside
workspace all-target checking with `aestra-bench/gpu`, both formatting checks and
Git whitespace validation. Post-documentation input drift is zero. Existing
Windows incremental-cache hard-link fallback warnings remain non-fatal.

Next: remaining viewport gizmo/host-motion consumers, followed by graph and
application workflows. B20-2 remains open; shipping stays on 0.19.1 until the
complete compatibility release gate.
