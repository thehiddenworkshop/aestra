# Fireworks startup admission

Unresolved camera targets now remain in preparation until both dimensions are
nonzero. They never install OIT approval. The demo waits for this explicitly
retryable status instead of exiting on zero estimated shared capacity. Invalid
settings, MSAA, capacity/device failures and observed fragment loss remain fatal;
zero estimated bytes alone is not permission to retry. The existing 60-second
startup timeout and pipeline warm-up gate remain in force.

The native recovery regression also exposed a second readiness bug: partition
admission accessed a newly queued render-pipeline descriptor before Bevy processed
its ID. Bevy's descriptor accessor panics in that state. Admission now queries
the compiled pipeline first and leaves the phase pending until it is available.
The descriptor audit and participant checks still run before admission; pending
partition phases remain suppressed rather than silently rendered by a fallback.

## Evidence

The [JSON ledger](startup-admission-2026-10-29.json) records source/executable
hashes, native results and retained failures.

- Renderer regression: missing target info, both zero dimensions and either zero
  axis remove stale approval. Invalid settings remain rejected; a resolved target
  still requires actual GPU support.
- Demo regressions: preparation cannot start playback or exit early, it still
  times out, real zero-byte failures stop playback, and a resolved target restores
  admission. Partition requests cannot hide camera rejection behind an empty
  render-thread mailbox.
- Native regression: explicitly seed zero computed target extent with an
  unavailable image for eight updates, then load that image through Bevy's asset
  arrival path. Unpartitioned and 4×4 partitioned targets recover with exact pixel
  equality and a valid 8,985,600-byte allocation estimate. This allocates no
  zero-size GPU texture. The camera cache is seeded explicitly because Bevy
  retains old computed sizes for unavailable image handles.
- Existing native phase regression: all 12 image/diagnostic cases pass, including
  overflow/recovery, genuine foreign/view rejection, empty resets, visibility,
  resize and ordinary restoration. PNG pixels match the regional-scan baseline.
- Six fresh window launches (three of each OIT mode) deliver native GPU captures
  after forward playback starts. These are startup checks, not full-show art or
  performance qualification.
- Full high-tier tuned near/orbit 4×4 shows pass observed capacity: 12 launches,
  detonations and flashes per show, **374,514 gold-tail ember** and **115,283
  star-path smoke births**, zero admission losses and complete particle/owner
  cleanup. Node/head storage remains **47,923,200 bytes**; particle-sort ownership
  remains zero. Ten live images are retained. Both full shows observed zero
  preparation updates, so the deterministic regression supplies the forced
  transient coverage; successful fresh launches do not prove every window path.

188 renderer tests, 55 demo tests, strict scoped all-target Clippy and workspace
formatting pass. Native checks were run explicitly; four GPU tests remain ignored
by the renderer's ordinary unit suite. Previous zero-capacity run failures and the
descriptor panic remain retained. Early fixture/harness setup failures are
described separately in the ledger and are not counted as accepted runs.

## Scope and next work

This is startup readiness and bounded target-recovery coverage, not generic
window/minimize/multi-camera lifecycle certification. The image lanes include
scans/readbacks and some CPU validation work ran concurrently; no timing or
frame-budget improvement is inferred. Density, lights, host admission policy,
ordinary transparency default and storage ceilings are unchanged. Next reduce
repeated per-region geometry work, preserving exact images, route/capacity
controls and real-time playback.
