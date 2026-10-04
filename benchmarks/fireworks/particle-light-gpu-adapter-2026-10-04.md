# F7E4A — bounded same-frame native adapter

Implemented/measured 2026-10-04 on top of F7E3 (`ddfeca8e`). RTX 4070 SUPER,
Vulkan; reported driver `NVIDIA`, not a numeric driver version.
The canonical `AestraParticleLightPlugin` now exposes `ParticleLightMode`:
default `PortableAsync`, opt-in `SameFrameGpu`. This is the first bounded
integration slice, **not final production-finale or general-layer certification**.

## Contract

- Reserve and reuse at most `min(global selection cap, GPU adapter cap)`
  zero-lumen shadowless Bevy lights. Source particle capacity does not determine
  entity allocation. Caps are configurable, not a new fixed 96-light ceiling.
- Resolve each reserved entity's current native light-buffer index. Overwrite
  only these indices after current-frame Aestra selection and before Bevy GPU
  clustering. Import Bevy's WGSL light ABI; normal `StandardMaterial` shading
  and independent host/representative lights are unchanged.
- Automatically stop selected-position readback and remove portable proxies
  in GPU mode, even when the host leaves readback caps enabled. No blocking GPU
  poll, CPU particle sampling, extrapolation or replay is introduced.
- Bound mapping/token/uniform buffers and qualified source metadata. Account
  for logical reserved Bevy records too; this is not a total GPU-memory cap.
  Bevy allocation granularity, camera clusters, pipeline caches and in-flight
  renderer resource retention are separate from this adapter's current-frame
  budget. Selection keeps its independent global/scratch admission budgets.
- Reuse metadata buffers when their shape is unchanged; drop them on disable,
  rejection or empty preparation. Do not mark unchanged main-world lights dirty
  each tick. Placeholders cannot CPU-frustum-cull moving lights using their
  intentionally unrelated main-world position; native GPU clustering uses the
  current actual light position/range.
- Qualify current sources using root/owner epochs, seed, history revision,
  originating artifact, enabled output and nested clip context. Explicit and
  inherited visibility are respected. GPU flags suppress invalid manifest tokens;
  unused slots are dark. Typed diagnostics expose budget, layer, storage/clustering,
  missing slot, selector and pipeline errors; warmup is explicit.
- **Default layers only.** Bevy's native GPU clustering does not use the CPU
  layer-assignment path. Non-default source/camera layers are rejected rather
  than silently leaking illumination. General per-view layer integration and
  multi-view acceptance remain open. Unsupported GPU/PBR configurations fail
  closed; a host must explicitly choose portable mode if delayed lighting is acceptable.

Statistics report reserved/written **capacity**, not a GPU-read-back active light
count. Last dispatched sequence and current render observations are asynchronously
visible to the main world. Shadow maps/contact shadows remain off.

## Native verification

The shared paced [registration method](particle-light-latency-2026-10-04.md)
now runs the canonical adapter instead of the test-only one-slot bridge.
Bevy pipelining and playback-only history remain enabled; control, async and
GPU phases each provide 60 moving ticks and ten final-image samples per speed.
The readback cap stays enabled during GPU mode to test automatic suppression.

The unchanged gate passes at 25/75/150 m/s: p95 world residuals are
0.0028/0.0115/0.0375 m, at most 0.015 equivalent 60 Hz frame. This is equivalent
spatial offset comparable to control subpixel variation, not measured scanout
or sub-millisecond transport latency. The async comparison still lags about two
frames. The [retained JSON](particle-light-gpu-adapter-2026-10-04.json) records
timing distributions, raw CSV hashes and representative capture hashes.

Additional native assertions cover:

- two distinct roots/artifacts and two lit spots with three reserved slots;
- stable slot entities across ticks, a removed root becoming dark while the
  remaining root and an ordinary blue host light remain lit;
- a hidden transform parent suppressing its child's light;
- explicit default-layer restriction and manifest/buffer-budget rejection;
- recovery after budget changes, with the pool shrinking to one slot;
- receiver, source and camera moved 10 km from the world origin;
- global disable, re-enable and removal of all roots without stale contribution.

Unit tests cover configurable caps above 96, clamps/budget failures, slot reuse,
shrink, external deletion recovery, mode changes and preserved host lights.
The existing portable identity/readback regressions remain enabled.

## Reproduction

Run native tests alone, not all ignored GPU probes simultaneously:

```powershell
cargo test --locked -p aestra-bevy --test particle_light_latency bounded_gpu_adapter -- --ignored --nocapture
./benchmarks/fireworks/validate-particle-light-latency.ps1 -ReportsDirectory target/fireworks-f7/particle-light-gpu-adapter -GpuAdapter
cargo test --locked -p aestra-bevy --lib
cargo test --locked -p aestra-bevy-render --lib particle_light_readback
```

Generated output contains 90 registration images, six disable/removal images,
nine additional contract captures and four CSVs under
`target/fireworks-f7/particle-light-gpu-adapter`. The native harness asserts the
contract captures; the read-only validator checks registration/cadence/resource
telemetry and hashes those captures. Image hashing alone is not image acceptance.

## Next gate

F7E4B: expose this mode in viewer profiling and remeasure authored hero,
overlapping volley and full show at every quality tier, alongside representative
flashes. Retain GPU selection, adapter/clustering/full-frame CPU/GPU/byte costs,
default-layer visibility/lifecycle and final-image registration. No density or
production-finale claim follows from isolated fixtures. General per-view layers,
additional hardware/cadences, perspective/bloom artwork, editor lighting and lit
smoke remain separate open gates; preserve portable mode and engine-neutral core.
