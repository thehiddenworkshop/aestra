# F7E4B3B1 — idle GPU-light work retirement, 2026-10-05

Base commit `c016d028` (F7E4B3A). This implements the **playback work-retirement
slice**, not all of F7E4B3B's private/in-flight native allocation qualification.
Retained [source/raw hashes, registration and nine cost pairs](particle-light-gpu-retirement-2026-10-05.json)
cover RTX 4070 SUPER / Vulkan / Windows, default layers and high tier only.

## Runtime change

The reusable pool keeps its bounded slot entities but hides them when no possible
GPU particle-light source remains. A conservative main-world presence scan checks
GPU runtime status, enabled outputs and current owner/routing-root visibility;
it does **not** read particle counts/positions, spatially cull authored lights or
duplicate the render authorization's epoch/artifact/layer checks. Explicit
`Visibility::Visible` has Bevy's ancestor-override semantics; a hidden or missing
routing root still prevents its child from waking the pool.

Visible slots retain a **zero-lumen, zero-radius, finite 1 mm CPU fallback**, not
the host's maximum authored range. Bevy prepares inverse-square range even at
zero intensity: using zero would upload infinity. The fallback bounds absent or
rejected injection without broadcasting inactive slots across the whole view.
Current-frame GPU injection still writes each active light's actual world-space
position/range before native clustering. `NoFrustumCulling` remains required:
the CPU fallback position must never cull an active GPU light.

When no source is authorized, injection returns cleanly before looking for hidden
slots in native destination mappings. Explicit rejection gates still run first.
Normal native preparation restores the neutral fallback, preventing yesterday's
light from surviving source loss or rejection. Budgets, portable async behavior,
host lights and GPU selection are unchanged. No new readbacks, GPU waits or replay
work are introduced. Changing the authored maximum range does not dirty unchanged
neutral light components.

This is **not exact per-slot liveness compaction**. While a possible source remains,
an empty GPU selection can still reserve visible pointlike slots. The standalone
hero/volley probes end with 96 acknowledged indices; no zero-work claim is made
for those live-source empty-selection frames.

## Native lifecycle and registration gate

The existing production-adapter test now retains a host-only baseline and seven
fixed 12-update cluster windows after ordinary paced capture/drain. It never waits
until an expected counter appears. `Clusters.last_frame_total_cluster_index_count`
is Bevy's **asynchronous acknowledgment**, not same-frame demand or allocation size.

| Phase | Acknowledged indices in every window sample | Visible / allocated slots |
|---|---:|---:|
| Host-only baseline | 20 | 0 / 3 |
| Two active roots | 51 | 3 / 3 |
| Hierarchy hidden | 20 | 0 / 3 |
| Unsupported source layer | 26 | 3 / 3 |
| Manifest rejected | 26 | 3 / 3 |
| Empty selection with possible source | 23 | 3 / 3 |
| All roots removed after far-origin recovery | 20 | 0 / 1 |

Pool identities survive idle→active transitions and stable playback. Hidden/removal
windows match the unchanged ordinary host light's baseline; empty/rejection windows
stay within the conservative fixture bound of baseline + eight cells per slot.
The images assert zero residual green light, preserved blue host light, one-root
removal, zero-byte-budget rejection, recovery and active GPU lights at a **+10 km**
world offset. The cap intentionally shrinks to one during recovery.

The native harness exits successfully. The existing read-only registration/cadence
gate also passes 25/75/150 m/s, ten image observations per phase: GPU p95 equivalent
spatial offsets are **0.00663 / 0.00916 / 0.01498 frames**, not display/scanout latency.
The legacy F7E4A validator's serialized milestone/date remain nested in the evidence;
the enclosing F7E4B3B1 record gives this run's actual date and expanded fixture scope.
An earlier successful local fixture run lacked the empty-selection case; only the
expanded `run2` is retained as this slice's complete native gate.

## Repeated authored workload gate

Three alternating on/off pairs per hero, volley and thirteen-clip show: **18 fresh
native processes / nine pairs**, all exit zero. Same shaders, authored paths/seeds,
compiled capacities, selection cap 96, adapter caps 96/0, materials, trail rendering
and representative show flashes. Every show passes the **8,413-child** admission
and cleanup gate. Asset/executable hashes stay unchanged. The B3A method's audience
perspective/HDR/Tony/EV0/bloom 0.15, 960×540, playback-only history, 120 warmup and
600/1680 measured updates, matched cluster preallocation and unpaced sequential
execution are retained; no concurrent build/native GPU workload.

Every enabled show now acknowledges **1 index in all of its final 60 samples**,
matching its control's fixed 60-sample window. Previously retained B3A shows ended
at **346,176** with zero writable capacity. Peak enabled show demand is **2,870**
in all three new runs versus the previous 352,512. The new read-only retirement
gate rejects the old matrix, unknown/short/unsettled windows, active capacity,
invalid/rejected sources and residual demand above control. No workload or gate
was reduced to obtain these results.

Independent run distributions remain qualified, not frame-paired causal timing:

| Probe | Render GPU p95, on / control (ms) | On−control render GPU run mean (ms) |
|---|---|---|
| Hero | 2.416–2.465 / 2.015–2.382 | +0.124–0.209 |
| Volley | 2.355–2.527 / 2.195–2.252 | +0.193–0.219 |
| Show | 9.507–9.653 / 9.249–9.287 | +0.003–0.112 |

Show clustering GPU p95 is **0.146–0.153 ms on / 0.150–0.152 ms control**.
Do not compare timing improvements across the earlier session: control timings
also changed. These are neither a universal game budget nor statistical-significance
or natural-star artistic approval.

Public lists remain **2,097,152-byte indices / 117,504-byte offsets-counts**, grid
**17×9×24**, without lost observations, measured list-size changes or native
resize/corruption/error warnings. Public global light storage remains at its
allocated high-water capacity: enabled show **7760–7920 bytes**, even after work
retires. Logical adapter statistics still report 96 reusable reserved slots.
**Work retirement does not mean buffer deallocation or zero resident memory.**

## Checks and reproduction

55 `aestra-bevy` library tests pass; crate all-targets Clippy with warnings denied,
workspace all-targets check, formatting and whitespace checks pass. The cost gate's
positive/negative controls pass, including seven retirement negative controls.
Native registration and cost probes run sequentially, without concurrent builds.

```powershell
# Choose fresh output directories; do not overwrite retained attempts.
$env:AESTRA_GPU_LIGHT_ADAPTER_REPORTS = 'target/fireworks-f7/gpu-retirement-registration-fresh'
cargo test --locked -p aestra-bevy --test particle_light_latency bounded_gpu_adapter_registers_without_selected_light_readback -- --ignored --exact --nocapture
& benchmarks/fireworks/validate-particle-light-latency.ps1 -ReportsDirectory $env:AESTRA_GPU_LIGHT_ADAPTER_REPORTS -GpuAdapter
cargo build --locked -p aestra-viewer
& benchmarks/fireworks/run-particle-light-costs.ps1 -ReportsDirectory target/fireworks-f7/gpu-retirement-costs-fresh -RequireRetirement
& benchmarks/fireworks/validate-particle-light-costs.ps1 -SelfTest
```

Retained raw directories:
`target/fireworks-f7/gpu-retirement-registration-2026-10-05-run2` and
`target/fireworks-f7/gpu-retirement-costs-2026-10-05`. Existing earlier B3A failures
remain recorded; this passing matrix does not establish that the earlier abnormal
native shutdown exit is fixed.

## Remaining F7E4B3B scope

Private Z-slice/scratchpad/metadata/staging allocations, old in-flight buffers,
creation/churn and comprehensive overflow qualification remain unmeasured. Public
size stability and native demand are not a total resident-memory cap. Continue that
qualification next, then medium/low repeated costs and receiver images. General
per-view layers/multiple views, additional hardware/cadences, natural-star art,
editor lighting, lit smoke and production-finale acceptance remain open.
