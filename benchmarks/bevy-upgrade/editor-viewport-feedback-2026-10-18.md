# B20-2 slice 66: preview feedback and profiler ingestion

## Implementation

Shipping remains on Bevy **0.19.1**. The isolated **0.20.0** editor candidate now
executes the original mock/input routing, output history and event-link drop
consumers from `viewport/feedback.rs`, plus actual profiler ingestion/state from
`profiler/state.rs`.

- Four viewport functions move unchanged: apply preview mocks/recorded inputs,
  map compiled link drops to authored links, record root outputs and track drops.
  The eight-output bound and original documentation move with them.
- The complete profiler aggregation state, outcome, existing test frame adapter,
  bounded-history constant and implementations are shared. Parent-visible fields
  and peak reset retain access for the unchanged native profiler UI.
- The renderer's actual event-link statistics/count records and accumulator move
  unchanged to `gpu/event_statistics.rs`. Existing public re-exports are retained;
  neither the readback observer nor its admission/drop accounting policy changes.
  The candidate renderer gate imports the same module.
- The editor candidate imports the complete unchanged production output-context
  module, including the real AestraOutputEvent message and its routing context.
  It does not replace renderer records with smaller editor-only stand-ins.

Source audits confirm all four feedback functions, profiler state/implementations,
event statistics and accumulator, native readback producer, viewport/profiler plugin
registration, all 43 remaining viewport root functions, remaining profiler functions
and both existing root test blocks are unchanged after parent-visibility
normalization where needed. Root dependency/toolchain inputs and authored formats
are unchanged. No new playback, event or profiling policy is introduced.

## Regression coverage and limits

Nineteen shared feedback tests include three adapted existing viewport regressions
and sixteen additional tests. Eight shared profiler tests include three existing
ingestion regressions and five additional tests. The stand-in integration fixture
uses an explicit temporary catalog instead of native sample-project discovery.

- Root and session instances share the same recorded mock trace. Nested/unowned
  presentations keep their own inputs. Mock changes/removal, missing presentations,
  replacement roots and seeks preserve recording/cache and ownership policies.
- Recorded inputs reach roots/session, can be cleared, and are not injected into
  nested players. Stable mocks/inputs do not advance runtime history epochs.
- Root/path filtering rejects nested, unowned, incomplete and removed senders.
  Payloads and arrival order are retained, including duplicate/out-of-order ticks;
  reading an empty next frame does not hear events again. The latest eight outputs
  remain bounded while the heard counter tracks all accepted arrivals.
- The editor retains its existing root/path-only output filtering; this slice does
  not add epoch validation or deduplication. Native producer/host validation remains
  a separate responsibility, not a newly claimed consumer guarantee.
- Link-drop membership changes rebuild UI; changing a count alone does not.
  Missing root statistics clear old drops without reading nested observations.
  Short/extra/zero counters, invalid compiled indices and mismatched authored link
  identities/triggers are handled by the original mapping policy.
- Authored data, dirty state, document revision and exact semantic undo/redo lengths
  remain unchanged by feedback. Mock configuration is never serialized into effects.
- Profiler ingestion preserves provenance, bounded history, active project identity
  and peak/reset behavior. Reordering is nonstructural; removing instances resets
  history. Missing or partial CPU observations remain unavailable, not measured zero.
  An actual measured zero remains measurable; empty projects retire old metrics.

The candidate additionally executes the output-context module's two existing
routing/placement tests. These are headless consumer/model checks: they do not
establish native GPU output delivery, readback latency, rendering, sampling quality
or full application scheduling. GPU-observation-to-profile integration in
`update_preview`, native profiler presentation and preview gizmo drawing remain
separate gates. No visual reference is changed or approved.

## Validation

Accepted frozen headless run (2026-10-18):
`target/bevy-020-qualification/runs/a35695f1c0224ecc9fe9f4e4ff6fcbe8/`.
The editor graph/session/history/consumer suite passes **1,338 tests**, with its
existing timing probe ignored, in 62.97 seconds after compilation. Focused runs
also pass all nineteen feedback and eight profiler-ingestion tests.

The complete qualification executes **1,760 checks**, with all **918 input hashes**
unchanged. Counts are executions across gates, not unique tests. The feedback
module/tests, profiler root/state/tests and renderer event-statistics module are
the six new inventory paths. The runner confirms Bevy **0.20.0**, Rust **1.98.1**,
`native_executed: false`, and unchanged shipping lock SHA-256
`ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.

Shipping passes **2,339 editor unit + 3 architecture tests**, with ten existing
ignores unchanged. Strict all-target Clippy passes on both engines, alongside
workspace all-target checking with `aestra-bench/gpu`, both formatting checks and
Git whitespace validation. Root dependency/toolchain inputs remain unchanged;
post-documentation input drift is zero. Existing Windows incremental-cache
hard-link fallback warnings are non-fatal.

The existing shipping renderer regression
`gpu::tests::event_link_statistics_record_admission_and_separate_both_drop_causes`
also passes explicitly against the retained public re-export and unchanged
accumulator (one passed, no GPU required).

Next: GPU-observation-to-profile integration in `update_preview`, then remaining
profiler presentation and gizmo/host-motion/graph/application consumers. B20-2
remains open; shipping stays on 0.19.1 until the complete compatibility release gate.
