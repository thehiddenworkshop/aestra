# F8.1C4N — guarded shared transparency in the public HDR host

The public `aestra-bevy` fireworks example now offers `--experimental-oit` with either
`--smoke-art-show` or `--persistent-smoke-show`. This is an explicit guarded pilot,
**not default-show migration, 60 FPS acceptance or photographic smoke-art approval**.
No viewer implementation is imported, and no saved effect/material is changed.

```powershell
cargo run --release --locked -p aestra-bevy --example fireworks -- --smoke-art-show --experimental-oit
```

The [JSON evidence](fragment-list-public-host-2026-10-26.json) records 64 source hashes,
the actual native binary hash, six raw full-show reports, ten PNGs and eight logs.
Raw outputs are local generated artifacts under
`target/fireworks-f8/public-oit-2026-10-26-b`. Machine-local paths in raw reports are
normalized to repository-relative paths in the committed evidence; hashes cover the
original raw files. Measurements use Windows, RTX 4070 SUPER/Vulkan, Rust 1.98.1,
Bevy 0.20.0 and wgpu 30.0.1.

## Admission and unchanged host behavior

The host audits its compiled root and every dependency before opening a window. The
new public `ExperimentalOitSettings::supports_renderer` helper and the actual native
pipeline share one predicate. This avoids a separate host copy drifting from renderer
eligibility. The host requires resolved semantic materials and rejects incompatible
geometry/material operators and extension stages. Six candidate/tier equality tests
prove the opt-in changes no compiled project: 39 static renderer definitions (27 sprites,
12 trails), not a count of simultaneously active GPU draws. A ribbon mutation is
rejected before the window starts. Host ground/receiver structures remain opaque.

The opt-in keeps the existing HDR, Tony display transform, bloom .15, environment,
authored sound/light routes, history policy and tier selection. It explicitly selects
MSAA Off, average four fragments/pixel, local threshold 32 and a 128 MiB node/head guard.
Native device/camera approval is required before playback; missing initial admission
times out. A camera-budget or non-native-backend rejection pauses/exits with a reason
instead of silently presenting ordinary transparency. Fatal shader errors remain
fatal even after startup. These guards are not global fragment-loss detection.

At the tested 1280x720, node/head storage is 47,923,200 bytes (45.70 MiB), not total VRAM.
The same policy requires 102.83 MiB at 1080p; larger targets/device limits can reject it.
Runtime budget rejection and timeout are host unit gates; earlier C4K native tests
cover renderer resizing/rejection. No native 4K/window-minimize qualification is claimed.
Normal controls remain unchanged when report recording is absent.

## Actual-window method

The final six runs use one built **development** binary (application opt-level 1,
dependencies opt-level 3), with the audio feature compiled but `--no-audio` in every run.
They run serially without concurrent compilation or another native GPU probe. This is
not an optimized release/game/audio benchmark.

Four **pacing lanes** run the wispy candidate: ordinary high, then OIT high/medium/low.
They use automatic host time, forward PlaybackOnly at speed one, no seek/replay/manual
fixed-time schedule, and no screenshots or OIT diagnostic scans. Each reaches exactly
37 seconds and runs ten cleanup updates. Wall time is about 37.135 seconds including
cleanup; summed Bevy game/real deltas are about 36.985 seconds because the terminal
update is outside the interval collection. No paused updates are recorded.

Two **image/capacity lanes** run high wispy and high persistent candidates. They add
asynchronous capacity scans and screenshots at nominal 8/15/25/32 seconds, and therefore
are excluded from cost comparisons. Delivered request times are recorded, not claimed
as exact render-frame timestamps. Report recording fixes playback/camera/light inputs;
Esc still cancels. Its output records host-update intervals including vsync, not GPU
completion or exact display-present timestamps.

## Results and retained limitations

| Actual configuration | Host median / p95 / max (ms) | Late pipeline updates | Peak async live particles |
| --- | ---: | ---: | ---: |
| Wispy high ordinary | 16.652 / 32.941 / 109.259 | 11 | 2,126 |
| Wispy high OIT | 16.649 / 32.466 / 108.866 | 11 | 2,125 |
| Wispy medium OIT | 16.642 / 32.620 / 96.854 | 12 | 998 |
| Wispy low OIT | 16.665 / 32.451 / 91.950 | 11 | 530 |

These are single observations per configuration, not confidence intervals. Ordinary
retains default Sample4 MSAA while OIT requires Off, so this is not an isolated causal
overhead comparison. The pending-pipeline counts are retained; they do not explain
every measured hitch. **Stable 60 Hz and full-show pacing are not accepted.**

Both image/capacity lanes finish with zero global drops/invalid links across **3,990**
completed observations (1,929 wispy, 2,061 persistent). Peak over-sort pixels are
168/22,439 respectively; C4M2 sorts these lists instead of approximating their order.
All eight scheduled captures are delivered. Final reports have zero last asynchronous
live counts and zero observed child owners after cleanup. This is not a driver-free or
total-VRAM proof; camera OIT storage remains resident until exit.

High/medium final reports admit 13 flash lights; low admits 11 with peak two. The tier
budgets are unchanged. The hook does not record every light-drop reason, so these
figures are not loss-free light-cue certification. Native runs are silent; the existing
audio cue/voice/restart tests are preserved, not a measured sound-playback workload.

Two additional single captures request a bright phase at 17.45 seconds in ordinary
and OIT modes. They are **not a valid pixel pair**: the ordinary log has nine admitted
flashes at request time versus twelve for OIT, and the visible burst phases differ.
They are retained as asynchronous host observations, not proof of a causal compositor
improvement or a presentation-registration oracle. The OIT image visibly contains
three colored bursts and colored illumination of older smoke. Wispy smoke still looks
granular; the persistent candidate still has round billows. Neither is photo-approved.

The earlier `public-oit-2026-10-26-a` interactive pilot is retained. It overlapped
compilation and lacked fixed controls/clock metadata (3,717 intervals, 22 flash admits).
Its pacing/counts are excluded from qualification, without assigning an unproven cause.
The final hook adds fixed recording inputs and wall/Bevy/pause metadata.

## Reproduction and validation

See [the host guide](../../docs/examples/fireworks.md) for interactive use. For a fresh
full-show pacing report from the repository root:

```powershell
$env:AESTRA_EXAMPLE_REPORT = "$PWD/target/fireworks-f8/my-public-host-run-1.json"
$env:AESTRA_EXAMPLE_REPORT_IMAGES = "0"
cargo run --locked -p aestra-bevy --example fireworks --features fireworks-audio -- --smoke-art-show --experimental-oit --no-audio
```

Use a fresh absolute filename each time. For a separate image/capacity run, set
`AESTRA_EXAMPLE_REPORT_IMAGES=1`; do not combine with `AESTRA_EXAMPLE_CAPTURE`.
Remove report env variables to restore normal interactive recording-free operation.
The optional hook adds only a dev dependency on the already-locked `serde_json` crate;
the runtime library does not acquire a reporting dependency.

Default example tests pass (24), audio-feature example tests pass (28), renderer tests
pass (172, three ignored). Strict workspace/all-target Clippy with benchmark GPU/audio
features, formatting and diff checks pass. Tests cover both candidates/all tiers with
unchanged compiled payloads, unsupported geometry, camera HDR/bloom/MSAA profile,
admission/budget failures and missing initial-admission timeout. No threshold is relaxed.

Next prepare late child pipelines before visible births and qualify actual-host pacing
and presentation registration before promotion. Continue budgeted star-path smoke and
bright/quiet HDR art review on the explicit candidate. The default show stays unchanged;
foreign/custom transparency, dynamic overrides, unsupported view configurations, broad
hardware/release performance and photographic acceptance remain outside this pilot.
