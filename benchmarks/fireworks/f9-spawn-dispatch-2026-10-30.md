# F9: skip empty scheduled-spawn dispatches

The shared GPU runtime now dispatches rate/burst spawning only when the CPU's
existing fixed-tick spawn calculation requests births. Nonempty dispatches use
`ceil(spawn_count / 64)` workgroups instead of the entire emitter capacity.
Integration/death processing still runs every tick. Domain, host-input and particle
event spawning remain separate and unchanged.

Both coupled and independent stateful scheduling use the same explicit tick
result: bind group, parameter buffer and scheduled spawn workgroup count. Fractional
carry, burst ticks, cutoffs, parameter packing, checkpoint/replay, presentation and
event ordering retain their existing behavior. No material, workload, owner cap,
quality policy, shader ABI or resource guard changed.

## Editor benefit and correctness

This is shared renderer work, so regular editor previews, projectile effects and
other stateful effects benefit without demo-specific runtime conditions. Both
**Smoke Material Lab** and **Smoke Wake Lab** render successfully after rebuilding
the editor-viewport regression executable against the changed runtime. All ten
fixed-time captures match the preceding kernel's RGB pixels exactly.

A new native GPU regression compares coupled and independent scheduling with
fractional emission, a 129-particle burst crossing workgroup boundaries, stop,
natural drain, pause and backward replay. It checks actual GPU particle records,
birth counters and presentation, as well as skipped dispatch accounting. All
23 native coupled regressions pass, including the two explicitly hardware-only
field-follow/moving-emitter cases. The opt-in accounting regression also passes;
strict renderer/demo Clippy, formatting and diff checks pass.

The kernel/sample files match the frozen smoke input hashes, and the 97 original
showcase asset hashes remain unchanged.

## Encoded work reduction

The opt-in `stateful_spawn` category records scheduled tick opportunities as
`calls`, and nonempty spawn commands as `dispatches`. These are CPU encoding
counts, not a GPU driver trace or total simulation-dispatch count. The previous
encoder issued one spawn command per opportunity, even when the kernel immediately
returned for every invocation.

| Candidate profile | Observed opportunities | Spawn commands | Avoided empty commands |
|---|---:|---:|---:|
| Low finale | 189,806 | 5,015 | 184,791 (97.36%) |
| Medium finale | 190,080 | 5,011 | 185,069 (97.36%) |

This removes work in one kernel class. It does not remove integration, event
gather/expansion, history recording, presentation or per-owner parameter work.

## Separate timing observations

A fresh sequential low-tier control/candidate pair uses the identical 32-shell
finale and frozen project: native RTX 4070 SUPER/Vulkan, optimized release,
1280×720 HDR/TonyMcMapface, bloom 0.15, Msaa::Off, PlaybackOnly, audio off,
4×4 OIT with the existing health scans. No compilation or screenshots run during
profiling. GPU p95 milliseconds:

| Phase | Control graph | Candidate graph | Control simulation | Candidate simulation |
|---|---:|---:|---:|---:|
| Launch / overlap | 18.030 | 15.872 | 11.470 | 9.372 |
| Active fireworks | 18.910 | 18.032 | 14.393 | 12.447 |
| Smoke drain | 13.463 | 13.013 | 11.630 | 11.213 |
| Idle | 2.234 | 2.230 | 0.004 | 0.004 |

The active graph difference is 0.879 ms in this pair. This is an initial
observation, not a repeated causal speedup or stable FPS guarantee. Phase tagging
is asynchronous, scopes are nested and their percentiles must not be added. The
graph interval excludes presentation and some uploads.

An additional medium candidate profile records active graph/simulation p95 of
21.563/15.987 ms. It is not paired with a new medium control, so no medium speedup
is asserted. Both measured candidate tiers still miss the 16.67 ms reference
active graph interval. The earlier high-tier performance gate remains open;
this pass does not claim a new high-tier timing qualification.

## Workload and remaining failures

All three candidate image lanes retain 32 launches/detonations, 64 peak owners,
unrestricted owner admission, zero admission waits and complete final cleanup.
Event audits have no source overflow, expansion omission, destination rejection
or truncation. Accepted gold-ember/path-smoke totals match the prior image runs:

| Tier | Gold ember births | Star-path smoke births | Maximum dropped fragments |
|---|---:|---:|---:|
| High | 1,015,758 | 309,714 | **580,544 — failed** |
| Medium | 505,062 | 154,151 | 0 |
| Low | 259,644 | 78,354 | 0 |

All 33 candidate demo captures deliver, including the failed high run. The
unchanged strict runner rejects that high capacity result. Light-budget differences
between independent runs remain explicit in the raw reports; no exact forward
frame or flash-count parity is asserted. Node/head storage stays 47,923,200 bytes
under the same 128 MiB guard, not total VRAM. This scheduling optimization does
not fix fragment storage pressure.

The [ledger](f9-spawn-dispatch-2026-10-30.json) fingerprints both executables,
reports/manifests, production/test sources, native PNGs and all ten exact editor
matches. The baseline is the preserved pre-change finale executable; the candidate
is a fresh release build. Use the existing `host-finale` review runner with either
binary and fresh output folders to reproduce image and separate profiling lanes.

Keep the smoke/workload frozen. Next inspect remaining integration/presentation
and event-dispatch work using the new accounting, alongside high-tier
transparency capacity. F9's production budget and full-frame repeated validation
remain open.
