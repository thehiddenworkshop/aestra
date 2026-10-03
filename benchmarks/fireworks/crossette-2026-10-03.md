# F5D — bounded four-arm crossette, 2026-10-03

An ordinary editable asset represents rocket death → 32 main stars → four directional
children per main-star death (128 arms). This proves fixed-plane crossette behavior at bounded
high-tier density, not AAA artistic quality or overlapping-finale throughput. No core runtime
change or firework-specific trigger/API.

## Asset and host contract

`assets/test/effects/fireworks_crossette.aestra.ron` uses nine emitters and seven death links.
The rocket also feeds one flash and 48 smoke particles. Main stars live 0.85–0.95 seconds
at speed 18–22. Four arm emitters use constant directions (±1, ±1, 0), speed 10, lifetime
0.75 seconds, drag 0.25, gravity (0, -9.81, 0) and 30% inherited parent velocity. Each
branch receives exactly one particle at the real parent death position. Equal arm dynamics
retain a four-way cross around a common moving center; independent random four-sample
directions would not guarantee that geometry. Four links express distinct directions,
not a capacity-limit workaround. The fixed plane is effect-local XY, follows effect
placement, and is **not parent-heading oriented**. Per-parent orientation/roll is still open.

Total capacity is 274 particles and 161 trail owners, including 128 arms. Arm history
lasts 0.5 seconds at 60 Hz, within the authored 64-point budget. The fixture reuses hero
HDR materials and exposed radiance/color controls, with a shared arm-color parameter.
Smoke remains an unlit approximation. The split exports `crossette_split` from main-star
`OnDeath`, not child `OnSpawn` (event-routed births do not emit that trigger). `FirstPerTick`
coalesces one representative effect-local position and parent count. The host binds
`host/firework_crossette_split` once per split, not four times for four arms. Actual audio,
spatial voices and mixing remain host-owned; F5B's qualified epoch and lossy ring policy apply.

## Native evidence

Local ignored artifacts under `target/fireworks-f5/`. RTX 4070 SUPER / Vulkan, dev build,
960×540, high tier, seed `0xf1e0000000000001`, playback-only, manual 60 Hz, semantic materials,
HDR/Tony/exposure 0/bloom 0.15, authored pixel floors 0/0.

- `f5d-crossette-live.json`: uninterrupted simulation, 120 warm-up / 600 measured frames,
  stable ordering. All seven links report demand = accepted **32 / 1 / 48 / 32 / 32 / 32 / 32**.
  Source overflow, expansion omission, target rejection, trail eviction and truncation zero.
  Estimated effect buffers 760,324 bytes, not total device memory. Peak live 183,
  occupied trails 160, retired trails 128; 716 event readback samples. Checkpoint-capture
  bytes remain zero. Aggregate simulation p50/p95/p99 0.787/1.903/1.992 ms includes idle tail
  and asynchronous phase sampling; concurrent local validation runs also shared this GPU.
  No speed comparison or finale performance certification is inferred.
- `f5d-crossette-host-cues.json`: actual Bevy messages, epochs 1/2/3. Initial and restarted
  streams each contain launch at tick 1, main break at tick 80, and six split packets / 32
  parents, ticks 132–137. Seek to frame 133 silences reconstruction through tick 132; five
  packets / 29 parents resume, ticks 133–137. Kinds, ticks, counts and representative positions
  match the original/restarted streams. Zero stale messages occurred here; deliberate stale
  readback rejection remains covered by deterministic F5B tests, not this workload.
- `f5d-crossette-review/review-manifest.json`: close/audience/wide × eight lifecycle frames =
  24 PNGs. Base revision `ca13f38b`, dirty source hashes recorded. Native backend, HDR response,
  all 32 main stars and 32 heads in each arm cohort verified. Endpoint live/occupied/retired,
  eviction/truncation measured zero at frame 420. Inspected close/audience contact sheets
  show red main expansion followed by gold arm clusters and lingering smoke. Dim/sparse
  audience-distance response needs human/moving-footage review. No artistic/golden approval.

## Regression and reproduction

Viewer: 69 passed, five fixture exporters ignored. Required native GPU conformance: 30 passed
serially. The crossette test compares CPU/GPU positions at ticks 60/70/95/150, validates real
parent birth positions, exactly 32 particles in each of four branches, XY diagonal geometry,
opposite-arm symmetry, link-order independence, inherited motion and all 128 retirements.
Warnings-as-errors Clippy passes for viewer/render adapter, all targets. Normal interactive
playback remains wall-clock; deterministic 60 Hz is only for check/capture/benchmark evidence.

```powershell
cargo test --locked -p aestra-viewer
$env:AESTRA_REQUIRE_GPU_CONFORMANCE = '1'
cargo test --locked -p aestra-bevy-render --test stateful_conformance -- --test-threads=1
cargo clippy --locked -p aestra-viewer -p aestra-bevy-render --all-targets -- -D warnings
# Fresh output paths only:
cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe f5-crossette --backend gpu --history playback-only --semantic-materials --camera audience --hdr --fireworks-cue-check target/fireworks-f5/crossette-cues-new.json
cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe f5-crossette --backend gpu --history playback-only --semantic-materials --camera audience --stable-transparency --hdr --gpu-bench target/fireworks-f5/crossette-live-new.json
pwsh -File benchmarks/fireworks/capture-shell-review.ps1 -Shell crossette -Response authored -OutputDirectory target/fireworks-f5/crossette-review-new
```

Next: bounded strobe authoring (F5E). Parent-oriented crossettes, tier-specific fan-out/count
policy, nested spatial audio, lit smoke, finale overlap/resource gates and artistic/moving
footage acceptance remain open. Ordinary quality tiers do not scale these link counts;
this high-tier fixture does not claim lower-tier readiness.
