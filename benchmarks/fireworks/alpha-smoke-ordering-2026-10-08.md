# F8.3D — bounded alpha-smoke ordering (2026-10-08)

**Accepted scope:** per-draw overlapping alpha Sprite repeatability on the named native
adapter. Not full-show smoke art, cross-draw transparency or production-finale performance.
Machine-readable reports, source/image hashes and rejected Fast controls are retained in
[`alpha-smoke-ordering-2026-10-08.json`](alpha-smoke-ordering-2026-10-08.json).

## Diagnosis and implementation

The saved 48-puff workload reproduces the Fast-mode failure under the original thresholds:
before-birth difference max 2/255, expired difference max 3/255 and rigid-hierarchy restoration
83 changed pixels/max 10/255. These negatives must stay within 1/255. Atomic alive compaction
does not define alpha compositing order; a stable source/simulation does not guarantee an
identical rendered image.

`TransparentOrderMode::DepthBackToFront` now sorts the GPU presentation list separately for
each visible native Camera3d alpha Sprite/Flipbook draw. Classification reads the GPU indirect
live count, current effect transform and view transform. Keys compare center depth far-to-near,
then spawn ordinal and physical slot. Padded 256-key pages use bitonic sorting; parallel
binary-rank merges produce one permutation. Simulation records, alive compaction and indirect
counts remain unchanged. There is no runtime particle or sorted-index readback, CPU proxy or
duplicated emitter workaround.

The public particle-smoke lab opts in. Normal show defaults stay Fast/unlit; additive rendering
is unchanged. `StableCapture` keeps its existing bounded ordinal-only behavior. The viewer
offers `--depth-transparency`; hosts set `AestraSettings.transparent_order`.

Capacity controls storage/work, even when few particles are live. For padded capacity N,
owned scratch/index storage is 36N bytes plus 80 bytes per stage and 80 vertex-parameter bytes,
per view/draw pair. The 48-puff fixture pads to 256 slots and owns **9,456 bytes / six buffers**.
Buffers reuse across unchanged capacity; source handles and bind groups refresh each frame.
Mode-off and owner removal retire this ownership. CPU ownership statistics are not exact
driver-free history or total VRAM accounting. Comparison work is O(N log² N), storage O(N).

## Qualification

- Native compute: **42 exact-permutation cases**, capacities 1/255/256/257/4097/8193/65537,
  zero/half/full live counts, nonzero alive offset and opposite view directions. The CPU oracle
  reads back only in this ignored test. Opposite cameras run sequentially; this is not a
  simultaneous multi-view image gate.
- Native saved-fixture images: two final all-tier runs pass unchanged lighting, one-slot,
  gain-off/restored, hierarchy, expiry, restart/rebinding, owner-removal and mode-switch controls.
  **66 corresponding PNGs match byte-for-byte.** All negative deltas are zero except the common
  rigid-parent transform, max 1/255, within the unchanged tolerance.
- Each frozen observation reports one sorted pair, 9,456 owned bytes and no new sort buffers
  that frame. Mode-off and final owner removal report zero pairs/owned bytes. This is not a
  no-allocation claim for bind groups or the entire renderer.
- The public Bevy host successfully renders the saved overlap fixture at approximately 1.92 s.
  `target/fireworks-f8/bevy-alpha-overlap.png` is a visually inspected 1280×720 capture, not an
  approved smoke-art reference. Shutdown logs include closed screenshot-readback warnings.

Representative illumination affects 3,126 pixels/max 104/255 at every tier. Both families affect
4,032 / 3,943 / 3,778 pixels at high/medium/low, respectively. Total compiled particle capacity
is 112 (48 smoke + two 32-star sources); selected-light caps are 8/4/2. This is not tier-scaled
smoke density or a large live pool.

The final run's isolated `alpha_sort` GPU timestamp spans with both light families enabled:

| Tier | Median ms | p95 ms | Samples |
| --- | ---: | ---: | ---: |
| High | 0.030720 | 0.032768 | 200 |
| Medium | 0.017408 | 0.019456 | 199 |
| Low | 0.017408 | 0.019456 | 200 |

These are 40 warm-up plus 200 paused update observations, asynchronously delivered and
deduplicated. Repeat timings vary (including sample delivery); no paired whole-frame or
large-capacity cost gate is claimed. Do not sum/subtract pass medians into a full-show budget.
All lighting and transparent-pass observations are retained in the JSON.

## Reproduce

Use an absolute report directory; the test runs from the package directory. On this machine
the default GNU toolchain lacked `dlltool.exe`; explicit installed MSVC was used without changing
the user's toolchain defaults.

```powershell
$env:AESTRA_VOLUME_OUTPUT_REPORTS = '<absolute fresh output directory>'
$env:AESTRA_SMOKE_ORDER = 'depth' # use 'fast' to reproduce the rejected baseline
cargo +1.98.1-x86_64-pc-windows-msvc test --locked -p aestra-viewer overlapping_particle_smoke_is_repeatable_without_position_readback -- --ignored --nocapture --test-threads=1
cargo +1.98.1-x86_64-pc-windows-msvc test --locked -p aestra-bevy-render --lib native_alpha_sort -- --ignored --nocapture --test-threads=1
cargo +1.98.1-x86_64-pc-windows-msvc run --locked -p aestra-bevy --example fireworks -- --particle-smoke-lighting --effect effects/fireworks_particle_smoke_overlap.aestra.ron --no-audio
```

GPU-free viewer tests: 96 passed/21 ignored. Audio-enabled fireworks example tests: 10 passed.
Sort shader/plan tests: two passed/one native test ignored by default. Workspace check, scoped
strict Clippy and formatting pass. Sandbox-only asset-discovery denials were rerun successfully
outside the sandbox. The initial new native attempt failed shader registration before any
usable visual evidence; embedded AssetServer loading fixed that setup error.

## Remaining gates

Sorting does not interleave different draw calls or sort Mesh/Ribbon/Trail/2D draws. General
layers, simultaneous views, other hardware/cadences and large-pool/live/full-show costs remain
unqualified. Device binding limits still apply; removing the legacy capture ceiling does not
mean unlimited resources. The next slice is **F8.1 reusable smoke art/persistence**, then
authored overlap/full-show cost qualification before migrating the show. F8 remains in progress.
