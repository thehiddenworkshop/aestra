# B20-2 slice 67: GPU observations to preview profiles

## Implementation

Shipping remains on Bevy **0.19.1**. The isolated **0.20.0** editor candidate now
executes the original `update_preview` consumer from `viewport/telemetry.rs`, with
the actual session, resolved project, timeline, PresentedEffect and ProfilerState.
Its complete function body and native plugin registration remain unchanged.

Five small production renderer modules separate existing observation data from
device/producer setup: geometry samples, field/trail observations, simulation and
preparation timing read models, and timestamp payloads. Existing re-export paths
remain intact. All eight record definitions and four read-model implementations
are unchanged after GPU-parent visibility normalization where necessary. No new
public telemetry constructor or test-only renderer API is shipped.

The candidate imports the complete unchanged particle-statistics component and
ReadbackComplete observer, actual backend capabilities/settings, and the shared
observation models. Its fixture helpers live only in the isolated candidate.
Count fixtures are encoded as real readback words and delivered through the
production observer; geometry/timestamp/field fixtures explicitly supply model
observations. These are not replacement component types or measured GPU samples.

Source audits confirm the ingestion function, all 42 remaining viewport root
functions, native plugin registration and existing viewport test body are unchanged.
Remaining renderer producer function bodies, including timing delivery, timestamp
transport and preparation, remain unchanged. Root dependencies, toolchain and
authored formats are unchanged. No profiling, playback or invalidation policy is
altered to satisfy a fixture.

## Regression coverage and limits

Ten shared tests include one existing shipping GPU/readback regression and nine
additional tests. Nine further candidate-only tests reach renderer-private
observation setup without widening the shipping API. They cover:

- Headless reference fallback, reuse of presented samples without a second
  reference evaluation, native-GPU unknown counts without CPU evaluation, backend
  transitions and rejection of unowned or mismatched compiled presentations.
- Genuine readback decoding, complete per-emitter counts, measured geometry and
  simulation/preparation time, invalid/future observations, and immediate
  seek/restart/seed/revision invalidation under the original component contracts.
- GPU-readback sample counts remain distinct from compact native counts. CPU
  reference presentation does not consume GPU simulation timing. Fluid-stage time
  is independent of particle backend and future samples remain unavailable.
- Real authored trail capacity supports occupancy/retirement/truncation/eviction
  and preparation-time ingestion; context changes retire the appropriate data.
- Nested reference profiles aggregate into project totals/live counts while only
  root samples enter the session. Empty projects clear old samples/counts.
  Repeated ingestion rebuilds UI only when profile structure changes.
- Authored effect, semantic history, dirty state, simulation clock and seed remain
  unchanged. No-stage/no-trail workloads retain existing known-zero semantics;
  unavailable current counts can retain an estimated project peak, not a measured
  live count. Draw time is never inferred from simulation timestamps.

Five existing particle-statistics tests and eleven existing capability tests also
run in the editor candidate. Existing renderer timing/field/geometry tests continue
to use the same observation models through retained namespace re-exports.

This is consumer/model integration, not native asynchronous readback latency,
GPU pipeline/workload correctness, visual/performance approval or complete
ViewportPlugin/application scheduling. Native profiler presentation and remaining
gizmo/host-motion workflows stay separate. No changed visual reference is approved.

## Validation

Accepted frozen run (2026-10-19):
`target/bevy-020-qualification/runs/20b60a86359344a5830916e9db74fef7/`.
The editor graph/session/history/consumer suite passes **1,373 tests**, with its
existing timing probe ignored, in 46.14 seconds after compilation. The focused
telemetry gate passes all nineteen shared/candidate-only cases.

The complete qualification executes **1,795 checks**, with all **927 input hashes**
unchanged. Counts are executions across gates, not unique tests. Three editor
telemetry sources, five renderer observation sources and the candidate telemetry
wrapper are the nine new inventory paths. The runner confirms Bevy **0.20.0**,
Rust **1.98.1**, `native_executed: false`, and unchanged shipping lock SHA-256
`ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356`.

Shipping passes **2,349 editor unit + 3 architecture tests**, with ten existing
ignores unchanged. Eight existing shipping renderer timing regressions pass in
the focused `--lib timing` gate; this does not replace opt-in native qualification.
Strict all-target Clippy passes on both engines, alongside workspace all-target
checking with `aestra-bench/gpu`, both formatting checks and Git whitespace
validation. Root dependency/toolchain inputs remain unchanged; post-documentation
input drift is zero. Existing Windows incremental-cache hard-link fallback warnings
are non-fatal.

Next: profiler presentation/action/plugin consumers, then remaining viewport
gizmo/host-motion and graph/application integration. B20-2 remains open; shipping
stays on 0.19.1 until the complete compatibility release gate.
