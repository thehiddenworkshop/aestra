# F7D2A — Global particle-light admission and live GPU integration

Date: 2026-10-03. This is integration/conformance evidence, **not full-show timing or visual acceptance**.
F7D1's [selection-only measurements](particle-light-selection-2026-10-03.md) remain a separate baseline.

## Host contract

The Bevy renderer installs selection systems but defaults to **disabled**. Opt in with:

```rust
use aestra_bevy_render::gpu::particle_lights::AestraParticleLightSettings;

app.insert_resource(AestraParticleLightSettings {
    max_lights: 96, // global across every GPU presentation in this RenderApp
    max_scratch_bytes: 64 * 1024 * 1024,
});
```

Author emitter `ParticlePointLight` scene outputs through the existing F7C source/compiler API.
Each resolved quality cap and the global host cap are independent. Zero disables all light jobs;
unsupported aggregate u32/device/buffer budgets reject selection instead of falling back to uncapped
output runs. The memory budget covers per-output/global ping-pong scratch, keys, plans, counters and
dispatch uniforms, not simulation/trail buffers or device/pipeline overhead. Returned `scratch_bytes`
reports **only ping-pong scratch**; `reserved_bytes` reports all budgeted selection buffers. Neither
is total renderer/device memory.

`GpuSelectedParticleLights` lives in the **render world**. Its `frame()` contains sorted 48-byte GPU
records, aggregate counters, selected capacity, source manifest, sequence and rejected-output count.
It does **not** spawn Bevy point lights or deliver a main-world selected-set message. F7E must provide
a bounded asynchronous copy/pool or another measured realization path. The live implementation never
maps source particle buffers or waits synchronously for the GPU. Blocking selected-set reads below
exist only in the explicit native tests.

Tokens index the **matching frame manifest**, not persistent identities. Qualify each selected ordinal
with root/owner epochs, nested clip path, source effect/seed/revision and emitter/region/output IDs.
Validate epochs before applying delayed results. Buffers are reused next frame: async consumers must
enqueue their bounded GPU copy in order and retain the corresponding manifest, not defer copying a
cloned handle until after another frame overwrites it.

## Native validation

Adapter reported by native conformance: AMD Radeon(TM) Graphics, integrated GPU, Vulkan,
AMD proprietary driver 26.3.1; vendor 4098, device 5710. No RTX results are claimed here.

Commands:

```powershell
cargo test --locked -p aestra-gpu --lib
cargo test --locked -p aestra-bevy-render --test particle_light_conformance -- --nocapture --test-threads=1
cargo test --locked -p aestra-bevy-render --test particle_light_playback -- --ignored --nocapture --test-threads=1
cargo clippy --locked -p aestra-gpu -p aestra-bevy-render --all-targets -- -D warnings
cargo check --locked --workspace --all-targets
cargo fmt --all -- --check
```

- All 25 `aestra-gpu` unit tests pass, including ABI/composed shaders, checked global schedules and
  retaining the inputs of light-only consumers through material attribute pruning.
- Existing per-output native CPU agreement passes through 65,536 particle slots after adding the
  live transform binding; source bytes remain unchanged in the small immutability probes.
- Global native conformance passes `(run width, runs, cap)` = `(1,1,1)`, `(3,5,7)`, `(65,3,129)` and
  `(16,65,64)`. Each reuses scratch through live/empty/reversed-run frames. CPU-ordered output bytes,
  priority/lumen/token/ordinal ties, padding and aggregate counters agree exactly. Scratch ranges from
  96 to 104,448 bytes in these **already quality-capped** input fixtures.
- Headless live analytic playback: two light-only sources, per-source cap 3/global cap 5:
  **48 requested, 48 candidates, 5 selected, 43 dropped; 1,728 scratch bytes**. World positions agree
  with the CPU analytic reference after root transforms. Forward refresh, despawn, zero-cap disable,
  re-enable under a smaller cap, invalid live gradient rejection, seed replacement and explicit
  resource rejection are checked.
- Headless stateful playback: two copies of the checked-in peony's launch → Main stars event chain,
  per-star-output cap 32/global cap 48, advancing forward at 60 Hz to 2.5 seconds:
  **512 requested, 512 candidates, 48 selected, 464 dropped; four source runs; 37,056 scratch bytes**.
  Event-born stable star ordinals, nested occurrence qualification and root-epoch replacement pass.
  Flash/smoke emitters and material/trail draws are deliberately omitted to isolate this integration;
  neither the full hero nor F6 show is certified by this probe.

## Remaining F7D2B / F7E gates

1. Add selection-enabled hero/overlap/F6 show probes with normal materials and trails intact. Measure
   selection and complete frame p50/p95/p99, candidate demand/drop counts and total reserved memory
   across quality tiers and explicit host caps. GPU diagnostics already expose
   `aestra::gpu::particle_lights` when the host enables Bevy render diagnostics.
2. Exercise full nested show lifecycle, repeated occurrences and authored dynamic overrides under
   those workloads. Pure analytic and stateful event-chain integration tests are not full-show proof.
3. Implement bounded asynchronous selected-set realization and measure delivery age, pool cleanup,
   visual contribution and selected-light/shadow costs (F7E). Default shadows remain off.
4. Verify lit smoke actually consumes scene lighting (F8.3).

No performance percentile, AAA acceptance, illuminated-scene capture or finale budget is claimed for
this integration slice. Playback-only remains supported; replay/checkpoints are not prerequisites.
