# F7E4B3B2B — native per-view GPU cluster buffer reuse

Base commit `8fd7f73e` (allocation observability and failed no-churn qualification).
The root Cargo patch uses the published Bevy PBR **0.19.1** source with a localized
change in `src/cluster/gpu.rs`. Licenses, embedded shaders/LUTs and published-file
hashes are retained in `vendor/bevy_pbr`. No registry cache is modified and no
fireworks-specific simulation or Aestra host API is introduced.

## Fix and boundaries

Reuse existing `ViewClusterBindings` and `ViewGpuClusteringBuffers` **before**
reserve/upload, initializing only missing containers. Refresh metadata and zero
logical public list contents/counts each frame. Reset scratchpad logical length
before sizing for the current grid; the existing allocation shader already zeroes
scratchpad counts before populate. Preserve adaptive index/Z-slice growth and
physical high-water capacities. Convert uniform bindings to storage when required
on GPU preparation. Remove inactive owning containers and referring bind groups;
despawned views drop normally and the existing readback map is pruned.

No retained-handle cache, after-allocation binding swap, blocking wait, particle
readback or changed shader is added. This fixes buffer lifetime, not all renderer
allocation: native bind groups and staging work still exist. Public list no-churn
and sampled named-private counts do not certify exact private/in-flight lifetime,
complete creation/free traces, hard memory bounds, total VRAM or production finale.

The dependency patch applies to the workspace's hosts/editor, not only the viewer.
Cargo patches are **not transitive**: an external consumer must apply the patch in
its own root or use an upstream fixed release. Upgrading/removing the patch requires
equivalent native regression and allocation-gate evidence.

## Native lifecycle regression

The explicitly ignored `cluster_buffer_reuse` integration test runs alone on a
native GPU, without pipelined rendering. It has **twelve settled twelve-update
windows**, each after 24 settling updates. It checks steady identities, grid
growth/shrink, unchanged index capacity, two independent views, inactive/reactivated/
despawned cameras, CPU/GPU mode recovery, host-light removal and final camera removal.
This is not an Aestra multi-view/layer or timing certification.

The accepted run reports **262144-byte indices**, offsets growing **512 → 4096
bytes**, with physical capacity retained on shrink. Active native asynchronous
index demand is **16**, returning to measured empty baseline **1**. CPU/GPU recovery
and steady logical-count reset preserve that demand. The first attempt expected
zero after removing the light and failed: native empty storage inserts a default
clustered light. Its unaccepted record is retained; the corrected fixture measures
empty baseline first and requires the active light to be distinguishable from it.

Two dependency unit regressions cover repeated logical counts/content reset across
growth/shrink and uniform-to-storage initialization without GPU setup.

## Paced pipelined registration and authored matrix

The existing native adapter/PBR test passes at **25/75/150 m/s**, preserving the
seven fixed twelve-update cluster windows: host-only/hidden/removed **20**, active
two-root **51**, rejected **26**, empty selection **23**. Final-image GPU light/star
absolute-offset p95 is **0.006633 / 0.009151 / 0.014981 equivalent frames**. GPU mode
adds no selected-light submissions during the measured windows, with pending and
staging bytes both zero. The lifetime submission counter includes earlier portable
async work and stays constant at **108 / 222 / 336** for those GPU windows;
portable async is tested separately as a delayed comparison. The **107 PNGs all match** the preceding accepted B3B1 native
registration run byte-for-byte, including removal/rejection and +10 km controls.
That is narrow synthetic registration/image preservation, not natural-star art or
full-show selected-light certification.

All **18** fresh authored high-tier native processes exit **0**. The original
accounting/hash/workload/resource/log/retirement/no-churn validator accepts the
matrix without changed thresholds: **zero public index and offsets-counts buffer
replacements** across all **598 hero/volley / 1678 show** measured observations in
each enabled/control run. Public sizes remain **2097152 / 117504 bytes**, grid
**17×9×24**. Each show preserves **8,413-child** admission and its final sixty-sample
acknowledgment window matches control at **one native index**. Global light storage
still changes twice during each show and zero times in hero/volley; that is separate
high-water light-storage growth, not unconditional per-view list recreation.

Measured snapshots contain **one metadata allocation (48 bytes)** and **2–3 staging
allocations (96–144 bytes)**, within the unchanged sampled ceiling of eight each.
Whole-device sampled allocated peaks span **114441872–130603680 bytes**, with
**268435456 reserved bytes** throughout measured snapshots. Snapshot CPU times span
**0.0237–0.2426 ms**. These are not cluster-only memory totals, exact in-flight/fence
traces or evidence of ordinary GPU-time improvement.

Raw matrix reports/logs are under `target/fireworks-f7/gpu-cluster-reuse-census-2026-10-05`;
paced images/CSVs under `target/fireworks-f7/gpu-cluster-reuse-registration-2026-10-05`.
[Tracked source/raw hashes and all eighteen rows](particle-light-gpu-cluster-reuse-2026-10-05.json)
retain both the accepted reuse evidence and the initial incorrect-empty-demand
test failure. B3B2A's preceding eighteen failed no-churn cells remain the negative
control. Viewer **95 tests / 12 ignored**, Aestra Bevy **55 tests**, dependency **two
tests**, all-targets Clippy, workspace all-targets/format checks and validator
positive/negative controls pass.

## Reproduction

```powershell
cargo test --locked -p bevy_pbr --lib aestra_reuse_tests
cargo test --locked -p aestra-bevy --test cluster_buffer_reuse -- --ignored --nocapture --test-threads=1
# Run alone, after all builds/native tests finish; fresh output directory only.
cargo build --locked -p aestra-viewer
& benchmarks/fireworks/run-particle-light-costs.ps1 -ReportsDirectory target/fireworks-f7/gpu-cluster-reuse-fresh -AllocationSnapshots
& benchmarks/fireworks/validate-particle-light-allocations.ps1 -ReportsDirectory target/fireworks-f7/gpu-cluster-reuse-fresh
```

Keep high-tier sources, IDs/seed/capacities, fast rendering, playback-only, HDR,
audience camera, preallocated cluster target, selection cap 96, adapter caps 96/0
and representative show lights unchanged. Do not relax the earlier no-churn gate
or discard failed processes. Instrumented allocator timings are not ordinary
matched-cost evidence or a cross-session performance-improvement claim.

## Remaining work

Private unlabeled Z/scratch attribution, exact generations/submission retirement,
deliberately oversized native overflow, medium/low costs/images, general Aestra
layers/multiple views, natural-star art, editor lighting, lit smoke, other hardware
and production-finale acceptance remain separate. The earlier B3A abnormal native
shutdown is not declared fixed by buffer reuse.
