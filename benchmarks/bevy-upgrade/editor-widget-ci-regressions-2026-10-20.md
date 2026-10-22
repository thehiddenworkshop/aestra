# Parallel CI numeric and tooltip regressions

Date: 2026-10-20. Fix for the two failures in the supplied Windows CI output.
Shipping remains Bevy 0.19.1; the isolated candidate remains Bevy 0.20.0.

## Cause and changes

- Numeric scrubbing used queued SelectAll/Insert operations to replace a model
  value. Selection is derived from Parley's text layout, so it is not a reliable
  whole-buffer replacement while layout/font initialization is incomplete. The
  reported result concatenated the old and new values: 0.50.51.
- The numeric replacement helper now clears stale queued edits and replaces the
  model-owned buffer directly, then queues caret placement. The same helper is
  used for ordinary typed model-to-widget updates. Focus guards, numeric type
  routing, user-input filters, stepping and single final-value events remain
  covered by the existing tests.
- Tooltip lifecycle tests relied on Instant elapsed time remaining below 650ms
  between calls. Under parallel CI load, a newly hovered target could legitimately
  mature before the assertion expecting no popup.
- Tooltips now measure the same unpaused delay using Bevy Time<Real>. Lifecycle
  tests drive TimeUpdateStrategy with explicit frame durations and freeze time
  between steps, instead of depending on CPU scheduling. A paused virtual
  preview clock does not pause hover help.
- The full default-parallel shipping run also exposed four existing numeric/hex
  typing tests that selected text before the embedded font had finished loading.
  Their shared headless fixture now checks the actual font asset's readiness and
  registers it with the text collection before composing fields. The bounded wait
  fails with asset-load diagnostics; it does not replace keyboard/selection code
  or weaken any value, filtering or commit assertions.

Five new shared regressions cover layout-independent replacement with stale
queued edits, multiple drag updates before layout, repeated model updates before
layout, the exact custom-delay boundary while virtual time is paused, and the
full restarted delay after an externally removed popup/target switch.

The two original failing assertions are retained. No tests are ignored, CI test
parallelism is not disabled, and no CI timeout, dependency or toolchain is changed.

## Validation

The focused Bevy 0.20 numeric/color/tooltip suites pass with default parallel
test execution: 66 color, 49 numeric and 17 tooltip checks. Both strict all-target
Clippy gates and both formatting gates pass.

The full shipping editor suite passes three consecutive times with default
parallel execution: 2,678 unit tests and 3 architecture tests per run, with the
same 10 pre-existing ignores and no failures.

Frozen Bevy 0.20 run
`target/bevy-020-qualification/runs/eef9531ef46e4676a66b5c6c925a9f9c/`
is accepted: 2,151 passing executions, including 1,719 editor graph checks;
all 1,009 source input hashes remain unchanged. Native execution is false.
Workspace GPU-feature all-target checking also passes. The shipping Cargo
manifests, lockfile, toolchain and CI workflow are unchanged.

These are headless behavioral tests, not native rendering or complete editor
workflow acceptance.
