# F8.1C4P — Profile clip activation and reuse material programs

This follows [C4O](prepared-public-host-2026-10-26.md). It removes a measured CPU
activation stall without changing catch-up budgets, authored effects, histories,
capacities, renderer policy or host choreography. Full-show pacing remains open.

## Finding and change

The public host's initial profile separated CPU playback/encoding, initial GPU-artifact
preparation, GPU simulation windows and existing render diagnostic phases. GPU-artifact
preparation was at most about 1 ms, but the playback CPU window reached 76 ms.
Direct measurement then located **22.18–32.02 ms per clip** in construction of
`PresentedEffect`: every future shell recompiled its immutable semantic material
programs. Three simultaneous births accumulated those costs in one playback update.

Project playback now owns default presentation prototypes for its root and all
compiled dependencies. They are built during initial project preparation. A new
clip uses `PresentedEffect::fresh_instance()`, sharing immutable compiled programs
while constructing independent runtime bindings and default instance state. Seeds,
parameters, clocks, histories, resource overrides and output identities remain separate;
the normal clip configuration applies authored overrides and history policy afterward.

The cache is **per root/project artifact**, not global. Replacing a project artifact
invalidates it even if effect IDs are unchanged; removing a root retires its prototypes.
Independent roots currently have separate prototype sets. Compilation moves to initial
preparation; it is not eliminated. Hosts constructing presentations directly can prepare
a prototype once and call `fresh_instance()` for subsequent default instances.

## Measurement

Windows, RTX 4070 SUPER/Vulkan, Rust 1.98.1, Bevy 0.20.0, wgpu 30.0.1.
Development application opt-level 1/dependencies opt-level 3, `fireworks-audio` compiled,
native `--no-audio`. Actual 1280x720 HDR/Tony/bloom public example, experimental OIT
with the existing 128 MiB guard, PlaybackOnly, full 37-second authored shows and ten
cleanup updates. No compilation or other native GPU probes ran during these windows.

The directly measured uncached profile and cached profile are separate instrumented
runs. Two additional cached wispy runs and one persistent run have profiling off;
all runs have screenshots and GPU capacity scans off. Seven reports are retained:
two preliminary discovery runs, one directly measured uncached run and four cached runs.
Nested/repeated GPU diagnostic paths must not be summed as independent frame costs.
Only the outer simulation scope is used for the aggregate simulation comparisons below.

| Instrumented wispy high OIT | Uncached | Cached |
| --- | ---: | ---: |
| CPU clip construction range | 22.178–32.025 ms | 0.0048–0.0198 ms |
| Maximum CPU playback window | 76.268 ms | 0.231 ms |
| Playback windows over 16.67 ms | 6 | 0 |
| GPU simulation p95 | 8.935 ms | 8.368 ms |
| GPU simulation maximum | 17.337 ms | 18.090 ms |
| Maximum sampled encoding lag | 183.334 ms | 50.001 ms |

The CPU activation gate passes for this workload. The GPU figures are observations,
not a GPU optimization claim. Timers measure scheduled CPU wall windows, including
possible system interleaving; GPU measurements are asynchronous. Initial activation
timers exclude render-thread GPU allocation/submission. Per-owner timestamp/work
totals may omit retired or query-budget-omitted owners. Observed checkpoint capture
bytes are zero under PlaybackOnly; these runs do not spend time capturing replay history.

| Cached plain high-tier case | Host p95 ms | Host maximum ms | Max sampled encoding lag ms |
| --- | ---: | ---: | ---: |
| Wispy OIT repetition 1 | 31.214 | 78.434 | 100.001 |
| Wispy OIT repetition 2 | 31.680 | 60.136 | 33.334 |
| Persistent OIT | 27.194 | 57.813 | 16.668 |

All four cached runs finish with **zero late pipelines, exact clip scheduling,
no blocked/missing scheduled owners and 13 accepted flashes**. Their final observed
particle populations are empty. That is not proof of driver memory reclamation.
Host intervals include vsync; results are not stable-60-FPS acceptance, exact visible
registration or photographic smoke approval. Repetition-to-repetition lag varies, so
the profile comparison is not a universal latency bound or an isolated performance ratio.

## Reproduce profiling

```powershell
$env:AESTRA_EXAMPLE_REPORT = "$PWD/target/fireworks-f8/new-profile-run.json"
$env:AESTRA_EXAMPLE_REPORT_IMAGES = "0"
$env:AESTRA_EXAMPLE_PROFILE = "1"
cargo run --locked -p aestra-bevy --example fireworks --features fireworks-audio -- --smoke-art-show --experimental-oit --no-audio
```

Use a fresh absolute report path. Profiling adds `RenderDiagnosticsPlugin` and optional
CPU timers; `AESTRA_EXAMPLE_PROFILE=0` leaves those off. Profile and image/capacity runs
are rejected when combined. The existing bounded asynchronous timestamp transport is
reused; there are no new particle readbacks, GPU waits, fake warm-up or replay seeks.

Validation: renderer library **178 passed, three ignored**; host library **65 passed**;
audio-enabled example **29 passed**; strict workspace Clippy, formatting and diff checks
pass. Tests cover immutable program sharing with fresh output identity/state, artifact
replacement with stable IDs, root retirement, and optional CPU timers on blocked owners.
Full workspace tests were not rerun.

[Machine-readable evidence](profiled-public-host-2026-10-26.json) retains reports and
hashes. Uncached binary: `03B8625770240DFBE5D1E92D002DA1B3C35B99995A574FC8CE30FD8A8A7EBE5A`.
Cached binary: `A1BC1874A541974F5944C7F98D84F6C85E6246F07F2383CB744565B311D73D21`.

Next measure render-thread allocation and coupled simulation upload/dispatch costs,
including repeated event-list/parameter allocation, before changing live catch-up policy.
CPU material compilation is no longer the measured birth-time spike; simulation,
render-thread activation and whole-host tail latency remain separate qualification gates.
