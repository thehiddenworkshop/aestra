# B20-1: nested project and live binding scheduling — 2026-10-10

Shipping remains Bevy **0.19.1** / wgpu **29.0.4**. The isolated candidate uses
Bevy **0.20.0** / wgpu **30.0.1**. Root manifests/lock, public API names and authored
formats are unchanged. This extends the accepted root clock-to-stage bridge.

## Shared implementation and fixes

- Import the actual host `project.rs` and `bindings.rs` into the isolated candidate,
  including their existing regressions. Add `aestra-project` as an explicit
  candidate dev dependency; it was already present transitively. Move the canonical
  `AestraSet` beside playback and preserve its public host re-export, so the native
  harness uses the real input-before-playback set ordering and deferred boundaries.
- Fix root restarts leaving surviving offset clips on their old GPU simulation.
  A root epoch transition beginning at zero now invalidates each surviving child's
  simulation revision. Keep the child's output-suppression boundary in source time.
  This resets even when the first new requested tick exceeds the old tick, without
  recreating compatible child entities. Ordinary positive seeks retain revisions.
- Forward per-slot host entity identities alongside binding snapshots, shallow to
  deep. A different entity with the same current value now calls the existing
  `rebind` API at each forwarded level. This refreshes snapshot-on-spawn latches
  and host-input identity; ordinary motion does neither. Reuse the existing
  resolved-target component rather than introducing a new public binding API.
- Extend runner source hashing to both canonical host modules and register a
  twenty-second explicitly executed, serial native gate. Keep the root gate too.

No shader changes, extra GPU readbacks, production GPU waits or mandatory replay
history are introduced. The shared fixes apply to shipping 0.19 and candidate 0.20.

## Native combined gate

`host_stage_native::native_020_nested_project_bindings_drive_independent_stage_timelines`
compiles a resolved root → parent → fluid leaf project. Two roots share its artifact
but bind different host entities. The leaf uses the forwarded live position for
its actual dense-smoke density source and reports pressure force on a plate.

The harness installs production binding/trace systems, root presentation preparation,
clock/choreography dispatch, project reconciliation, shallow-to-deep forwarding and
final root synchronization. It then uses the same actual extraction, stage runtime
preparation, timeline, async callback encoder and main-world receiver as the prior
root gate. Deterministic Bevy time controls the root; the test does not supply
child tick requests or construct stage runtimes manually.

Checks:

1. Both levels' source offsets compose to leaf tick 30 at root time zero. A
   20-frame advance reaches leaf tick 50 once; the other root remains paused at 30.
2. Actual render-world binding bytes match the leaf's resolved snapshots. A source
   inside the grid produces nonzero GPU force; the other root's outside source
   leaves its otherwise identical grid empty. Verify fixed seed, parameter override
   and inherited render layers on the presentation (not a particle-render oracle).
3. Root restart followed by a 30-frame first advance preserves child entity identity
   but resets the leaf to a four-tick bounded reconstruction toward tick 60.
4. The nested authored sound cue reaches the root once with its two-element path
   and playback epoch. Forward seek reconstructs to tick 120 without replaying cues
   or stage impacts; a subsequent live tick resumes child-local stage impacts.
5. Rebinding to another entity with equal coordinates changes the leaf's host-input
   epoch. Subsequent movement reaches extraction. Restart with the source outside
   the grid yields zero force rather than retaining the old plume. The second root
   remains isolated. A backward seek reconstructs to tick 45.
6. Clip expiry retires its presentation without deleting the other root's child;
   root teardown clears stage runtimes. Playback-only keeps zero checkpoint bytes
   throughout. Native Vulkan validation reports no error.

The two new shared regressions separately exercise restart versus positive-seek
revision behavior, and live movement versus equal-value retargeting through two
levels with a snapshot-on-spawn leaf binding. Existing project and binding tests
also run against Bevy 0.20, including layer/dependency/override reconciliation,
history-policy changes, missing bindings and root recorded-trace scrubbing.

## Acceptance boundaries

This is combined project/live-binding correctness evidence, **not** the complete
`AestraPlugin` on 0.20. The narrow render-graph driver still advances actual stage
timelines with a fixed four-tick budget; the production outer graph scheduler,
volume presentation, profiling, input-event routing, physics/world providers,
asynchronous pipeline readiness and pipelined rendering are not installed here.
Targets supply deterministic `GlobalTransform` snapshots; this is not a new test
of application transform-propagation latency. The actual resolver's documented
previous-propagation sampling order is unchanged.

Nested stage impacts retain their existing child-entity/empty-path/no-epoch
contract. The gate verifies **choreography** root/path routing, not a new routing
API for aggregate stage messages. Stage aggregate seek suppression retains the
conservative first-boundary-straddling interval tradeoff documented in the prior
report. Recorded traces are covered by shared unit tests, not a combined native
trace-driven project gate. Forward-only live bindings do not become exact replay
streams merely because their current snapshot is forwarded.

Editor compatibility, the coherent shipping dependency switch and native
visual/performance parity remain open. No art references are changed or approved.

## Validation

All Cargo checks use pinned MSVC Rust 1.98.1 with locked, offline dependencies.
Native GPU processes run serially.

```powershell
./benchmarks/bevy-upgrade/run-020-patches.ps1 -Native
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-bevy --lib --locked --offline
# AESTRA_REQUIRE_GPU_CONFORMANCE=1, restoring its previous value afterward:
cargo +1.98.1-x86_64-pc-windows-msvc test -p aestra-bevy-render --lib --test gpu_conformance --test stateful_conformance --test trail_compaction_conformance --test trail_culling_conformance --test domain_spawn --locked --offline -- --test-threads=1
cargo +1.98.1-x86_64-pc-windows-msvc check --workspace --all-targets --features aestra-bench/gpu --locked --offline
cargo +1.98.1-x86_64-pc-windows-msvc clippy --workspace --all-targets --features aestra-bench/gpu --locked --offline -- -D warnings
cargo +1.98.1-x86_64-pc-windows-msvc clippy --manifest-path benchmarks/bevy-upgrade/patches-020/Cargo.toml --target-dir target/bevy-020-qualification --all-targets --locked --offline -- -D warnings
cargo +1.98.1-x86_64-pc-windows-msvc fmt --all -- --check
cargo +1.98.1-x86_64-pc-windows-msvc fmt --manifest-path benchmarks/bevy-upgrade/patches-020/Cargo.toml -- --check
```

- Candidate: **178 passed** — 9 keyboard, 40 upstream focus, 4 cluster units,
  4 shader, 3 storage, 96 extraction/shared and **22 explicit serial native gates**.
- Shipping host library: **63 passed**, without filtering.
- Shipping renderer library: **157 passed**, three pre-existing ignored native tests.
  Hardware-required conformance: **40 passed** — 31 stateful, 3 general, 1 trail
  compaction, 1 culling and 4 domain/accepted-birth-output checks.
- Workspace all-target check, both strict all-target Clippy checks, both formatting
  checks, PowerShell AST validation and whitespace checks pass. Approved retries
  outside the sandbox were needed for formatting/Clippy configuration access;
  no lint was disabled. Compiler cache hard-link warnings fell back to copying.
- Root manifests/lock remain unchanged. Changes are not committed.

Accepted final source-frozen run:
`target/bevy-020-qualification/runs/9286961940ca47c397c46bd08ba8e0e9/`.
All hashed source and executable inputs remained unchanged throughout. The new
combined gate passed in **3.86 seconds** on NVIDIA RTX 4070 SUPER / Vulkan; companion
gates report driver 616.92. Some other gates selected AMD Radeon Graphics / Vulkan /
driver 26.3.1. This is correctness evidence, not visual or performance acceptance.

| Evidence | SHA-256 |
| --- | --- |
| `summary.json` | `11309f50d87273773f3704fef0e0f0af50e8bf1effe7d1ce1eb53d5af3372fd6` |
| `inputs-after.json` | `50f83c21448189e79e9549eea14e7df9798d066749116c373a676a0f5c440c87` |
| extraction executable | `f5e624fe9b279873abbf3e964c2d7954af5cd4a26957360c8c761d59784217a3` |
| candidate lock | `46b289acd9b39a69e941b432203cd9e33b8c565ce65d4f3eca8feddce6a7f589` |
| shipping lock | `ba8a8ed4f8c146a7a8888f4d5ad52c4cef7bbbbf415b2d61c92fb2e969690356` |
