# F7E4B3B2C — bounded native private lifetime and overflow qualification

Base commit `60a4b79a`. This follows native public-list reuse, not a new
fireworks-specific runtime feature. The workspace's pinned Bevy PBR 0.19.1 patch
now fixes metadata staging ownership and GPU-to-CPU cleanup. External consumers
still need their own root Cargo patch or an upstream fixed release.

## Implementation

Completed native metadata maps previously recycled their buffer **without
removing its pending owning reference**. The pending CPU vector therefore
accumulated duplicate clones forever, even when the backend allocation count
looked stable. Failed maps also left buffers pending. The new staging ledger
removes terminal pending ownership, recycles only successful decoded/unmapped
buffers and ignores unknown/duplicate completions. Pending plus free slots are
bounded at **eight 48-byte buffers per view**. Saturation skips only adaptive
metadata feedback; GPU cluster passes still execute and completed slots resume
feedback. No GPU wait or selected-particle readback is added.

Cleanup now runs even when GPU clustering is disabled, dropping private owners,
native referring bind groups and per-view readback-map entries while preserving
the CPU's public bindings. Private Z-slice/scratchpad buffers have backend names.
A doc-hidden workspace diagnostic hook returns native `BufferId` generations,
physical bytes, logical scratch length, pool IDs/capacities and **weak** readback
probes. It never returns/clones owning GPU handles. This is not a new stable
Aestra host API.

Four dependency unit tests pass, including a **20,000-cycle** staging ledger
regression, eight-slot saturation, out-of-order success, failed completion,
duplicate completion and adaptive power-of-two growth without shrink.
Failure coverage here is deterministic ledger coverage, not injected native
device-loss/map-failure certification.

## Explicit native lifetime and oversized fixture

The original public reuse regression and the new private-lifetime regression
run as separate native processes with non-pipelined headless rendering. The new
fixture starts with **32 Z slices / 64 indices**, then introduces **64 overlapping
lights across 128 clusters**. Native feedback grows to **512 Z slices / 8,192
indices** and the full **8,192-index demand** recovers. Expected native resize
warnings remain visible; transient overflow frames can have incorrect lighting.
This is recovery within feasible device capacity, not visually perfect overload
or an enforced memory limit for arbitrary input. Production must pre-size its
initial native capacities appropriately.

There are **ten settled windows**: nine twelve-update windows and one
**240-update** recovered steady window, each after 24 settling updates. Private
generations remain stable in steady windows; only Z slices grow on light
overflow, with scratchpad/metadata identities preserved. Independent second
views, inactivity/reactivation, despawn, light removal and GPU/CPU/GPU switches
are checked. The largest observed per-view pending/free ledger contains **two**
unique staging generations, within the enforced eight-slot ceiling.

Nine backend census checkpoints follow asynchronous `on_submitted_work_done`
notifications. The test uses normal application updates to drive callbacks, not
blocking `PollType::Wait`. Named Z/scratch/metadata allocations match active
owners: **one → two → one → zero on CPU mode → one after GPU resumption → zero
after all views are removed**. Staging allocations are **two per view** in this
fixture and retire to zero on CPU mode/final removal. Four weak probes prove that
retired callback state is no longer alive. Retained reserved allocator blocks
are not incorrectly reported as leaked live resources.

| Native private buffer | Initial bytes | Recovered bytes | CPU mode / final removal |
| --- | ---: | ---: | ---: |
| Z slices | 384 | 6,144 | 0 |
| Scratchpad | 4,096 | 4,096 | 0 |
| Metadata | 48 | 48 | 0 |

Native owning generation IDs and all checkpoint counts/bytes are retained in
the raw logs, rather than inferred from allocator block offsets. Counts at
completed-submission checkpoints do **not** establish the exact driver free
instant or a complete allocation/free trace.

## Authored high-tier matrix

All **18** fresh hero/volley/show enabled/control processes pass the existing
hash/workload/admission/log/retirement/public no-churn gates without altered
thresholds. The additional read-only private gate requires the unchanged
960×540, single-view **17×9×24** grid and 4,096/524,288 initial capacities.
Every measured census has exactly **one** named Z buffer (**49,152 bytes**),
scratchpad (**117,504 bytes**) and metadata (**48 bytes**), with **2–3 staging
buffers (96–144 bytes)**. Missing labels/reports, duplicate owners, wrong sizes
and more than eight staging buffers fail the gate; its positive/negative
controls are retained in the validator.

This combines owning-generation/lifecycle proof in the dedicated fixture with
sampled private backend counts under authored load. It is not an exact
generation/fence trace for every authored frame, total VRAM, a full-driver memory
budget, production-finale certification or ordinary cost-improvement evidence.

The paced pipelined adapter/PBR regression also passes at **25/75/150 m/s**.
All **107 PNGs match** the preceding accepted reuse run byte-for-byte, including
rejection/removal and +10 km controls. Absolute-offset p95 remains
**0.006633 / 0.009151 / 0.014981 equivalent frames**. Measured GPU windows add
no selected-light submissions and have zero pending/staging bytes; the lifetime
submission counter includes earlier portable async history. This remains a
synthetic registration gate, not natural-star or full-show art approval.

## Evidence and reproduction

[Source/raw hashes and measured results](cluster-private-lifetime-2026-10-05.json)
identify the accepted native run, raw named allocations and eighteen authored
rows. Prior accepted reuse evidence is kept unchanged.
Viewer **95 tests / 12 ignored**, Aestra Bevy **55 tests**, dependency **four
tests**, all-targets Clippy, workspace all-targets/format checks and all three
validator positive/negative controls pass.

```powershell
cargo test --locked -p bevy_pbr --lib aestra_reuse_tests
& benchmarks/fireworks/run-cluster-lifetimes.ps1 -ReportsDirectory target/fireworks-f7/private-lifetime-fresh
# Run alone, after builds/native tests finish. Fresh directories only.
cargo build --locked -p aestra-viewer
& benchmarks/fireworks/run-particle-light-costs.ps1 -ReportsDirectory target/fireworks-f7/private-census-fresh -AllocationSnapshots
& benchmarks/fireworks/validate-cluster-private-allocations.ps1 -ReportsDirectory target/fireworks-f7/private-census-fresh
& benchmarks/fireworks/validate-cluster-private-allocations.ps1 -SelfTest
```

## Next bounded step

Finish F7F's host-controlled lighting quality policy and medium/low repeated
cost/receiver-image gates, then start F8.3 lit smoke. Retain default-layer
restrictions, explicit general multi-view/layer limits, natural-star art review,
additional hardware/cadences and production-finale certification as open gates.
